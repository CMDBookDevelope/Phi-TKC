#!/usr/bin/env python3
"""Collect and audit the complete PE DLL dependency graph for Windows packages.

No installed toolchain or PATH directories are consulted by ``verify``. This is
deliberate: a build runner's MinGW DLLs must not hide an incomplete release.
"""
import argparse
import hashlib
import json
import os
import shutil
import struct
from pathlib import Path


# Windows 10/11 inbox DLLs, not a list derived from the runner's System32.
# In particular, GNU, Visual C++, WebView2 and Vulkan runtimes are NOT exempt.
SYSTEM_DLLS = frozenset("""
advapi32.dll avrt.dll bcrypt.dll bcryptprimitives.dll cabinet.dll cfgmgr32.dll
clbcatq.dll comctl32.dll comdlg32.dll combase.dll credui.dll crypt32.dll
cryptbase.dll cryptnet.dll cryptsp.dll d2d1.dll d3d9.dll d3d10.dll d3d10_1.dll
d3d11.dll d3d12.dll d3dcompiler_47.dll dbghelp.dll dbgcore.dll devobj.dll
dhcpcsvc.dll dhcpcsvc6.dll dinput8.dll dnsapi.dll dsound.dll dwmapi.dll
dwrite.dll dxgi.dll dxva2.dll faultrep.dll fwpuclnt.dll gdi32.dll gdi32full.dll
hid.dll imagehlp.dll imm32.dll iphlpapi.dll kernel32.dll kernelbase.dll ksuser.dll ktmw32.dll
mf.dll mfplat.dll mfreadwrite.dll mfuuid.dll mmdevapi.dll mpr.dll msacm32.dll
msasn1.dll mscoree.dll msctf.dll msimg32.dll msvcrt.dll mswsock.dll ncrypt.dll
netapi32.dll normaliz.dll nsi.dll ntdll.dll ole32.dll oleacc.dll oleaut32.dll
opengl32.dll powrprof.dll profapi.dll propsys.dll psapi.dll rasapi32.dll
rasman.dll rpcrt4.dll rtworkq.dll samcli.dll secur32.dll security.dll
setupapi.dll shcore.dll shell32.dll shlwapi.dll srvcli.dll sspicli.dll
ucrtbase.dll urlmon.dll user32.dll userenv.dll usp10.dll uxtheme.dll version.dll
wevtapi.dll windowscodecs.dll winhttp.dll wininet.dll winmm.dll winmmbase.dll
winnsi.dll winspool.drv wintrust.dll wldap32.dll wldp.dll ws2_32.dll wtsapi32.dll
""".split())


def system_dll(name):
    name = name.casefold()
    return name in SYSTEM_DLLS or name.startswith(("api-ms-win-", "ext-ms-win-"))


def pe_dependencies(path):
    """Return (COFF machine, normal + delay import DLL names) from a PE file.

    Based on Microsoft's PE format: import directory 1, delay directory 13.
    Delay descriptors can contain either RVAs or preferred-base virtual addresses.
    """
    data = Path(path).read_bytes()

    def unpack(fmt, offset):
        size = struct.calcsize(fmt)
        if offset < 0 or offset + size > len(data):
            raise ValueError(f"{path}: truncated PE structure at {offset:#x}")
        return struct.unpack_from(fmt, data, offset)

    if data[:2] != b"MZ":
        raise ValueError(f"{path}: not a Windows PE file")
    pe, = unpack("<I", 0x3C)
    if data[pe:pe + 4] != b"PE\0\0":
        raise ValueError(f"{path}: invalid PE signature")
    machine, section_count = unpack("<HH", pe + 4)
    optional_size, = unpack("<H", pe + 20)
    optional = pe + 24
    magic, = unpack("<H", optional)
    if magic == 0x20B:  # PE32+
        image_base, = unpack("<Q", optional + 24)
        directory_start, directory_count_offset = optional + 112, optional + 108
    elif magic == 0x10B:  # PE32
        image_base, = unpack("<I", optional + 28)
        directory_start, directory_count_offset = optional + 96, optional + 92
    else:
        raise ValueError(f"{path}: unsupported optional header {magic:#x}")
    directory_count, = unpack("<I", directory_count_offset)
    header_size, = unpack("<I", optional + 60)
    sections = []
    for i in range(section_count):
        header = optional + optional_size + i * 40
        virtual_size, virtual_address, raw_size, raw_offset = unpack("<IIII", header + 8)
        sections.append((virtual_address, virtual_size, raw_size, raw_offset))

    def offset(rva):
        if 0 <= rva < min(header_size, len(data)):
            return rva
        for start, virtual_size, raw_size, raw_offset in sections:
            if start <= rva < start + max(virtual_size, raw_size):
                delta = rva - start
                if delta < raw_size and raw_offset + delta < len(data):
                    return raw_offset + delta
                break
        raise ValueError(f"{path}: unmapped import RVA {rva:#x}")

    def dll_name(rva):
        start = offset(rva)
        end = data.find(b"\0", start, min(start + 1024, len(data)))
        if end < 0:
            raise ValueError(f"{path}: unterminated DLL name")
        name = data[start:end].decode("ascii")
        if not name or any(c in name for c in ("/", "\\", ":")) or name in (".", ".."):
            raise ValueError(f"{path}: invalid imported DLL name {name!r}")
        return name

    imports = set()
    for directory, stride in ((1, 20), (13, 32)):
        location = directory_start + directory * 8
        if directory >= directory_count:
            continue
        if location + 8 > optional + optional_size:
            raise ValueError(f"{path}: truncated PE data directories")
        address, size = unpack("<II", location)
        if not address:
            continue
        if size < stride:
            raise ValueError(f"{path}: undersized import directory")
        terminated = False
        for step in range(size // stride):
            descriptor = unpack("<" + "I" * (stride // 4), offset(address + step * stride))
            if not any(descriptor):
                terminated = True
                break
            name_address = descriptor[3] if directory == 1 else descriptor[1]
            if directory == 13 and not descriptor[0] & 1:
                name_address -= image_base
            imports.add(dll_name(name_address))
        if not terminated:
            raise ValueError(f"{path}: unterminated import directory")
    return machine, sorted(imports, key=str.casefold)


class Resolver:
    def __init__(self, directories):
        self.directories = []
        self.indexes = {}
        for directory in directories:
            directory = Path(directory).resolve()
            if directory.is_dir() and directory not in self.directories:
                self.directories.append(directory)

    def find(self, name, requester=None):
        directories = self.directories
        if requester is not None and requester.parent in directories:
            directories = [requester.parent, *directories]
        for directory in directories:
            if directory not in self.indexes:
                self.indexes[directory] = {
                    p.name.casefold(): p for p in directory.iterdir() if p.is_file()
                }
            path = self.indexes[directory].get(name.casefold())
            if path:
                return path
        raise ValueError(f"missing non-system DLL {name} (required by {requester})")


def dependency_graph(binary, directories, extras=()):
    binary = Path(binary).resolve()
    resolver = Resolver(directories)
    machine, _ = pe_dependencies(binary)
    if machine != 0x8664:
        raise ValueError(f"{binary}: expected x86_64 PE, got machine {machine:#x}")
    pending = [binary, *(Path(p).resolve() for p in extras)]
    files, imports = {}, {}
    while pending:
        path = pending.pop()
        key = path.name.casefold()
        if key in files:
            if files[key] != path and files[key].read_bytes() != path.read_bytes():
                raise ValueError(f"conflicting copies of DLL {path.name}")
            continue
        file_machine, required = pe_dependencies(path)
        if file_machine != machine:
            raise ValueError(f"{path}: wrong PE architecture {file_machine:#x}, expected {machine:#x}")
        files[key] = path
        imports[path.name] = required
        for name in required:
            if not system_dll(name):
                pending.append(resolver.find(name, path))
    return files, imports


def audit(binary, runtime_dir, extras=()):
    """Resolve every DLL exclusively inside the supplied flat runtime payload."""
    return dependency_graph(binary, [Path(runtime_dir)], extras)


def collect(args):
    binary = args.binary.resolve()
    output = args.output.resolve()
    # Never accidentally clear an executable/build directory.
    if output == binary.parent or output in binary.parents:
        raise ValueError("runtime output must be separate from the binary directory")
    extras = list(args.extra_dll)
    loader = binary.parent / "WebView2Loader.dll"
    if loader.is_file() and not any(p.name.casefold() == "webview2loader.dll" for p in extras):
        extras.append(loader)
    if not any(p.name.casefold() == "webview2loader.dll" for p in extras):
        raise ValueError("WebView2Loader.dll is required for the GNU Windows GUI")
    search_dirs = [binary.parent, *(p.resolve().parent for p in extras), *args.search_dir]
    search_dirs += [Path(p) for p in os.environ.get("PATH", "").split(os.pathsep) if p]
    files, imports = dependency_graph(binary, search_dirs, extras)
    output.mkdir(parents=True, exist_ok=True)
    # Stale DLLs must not survive a rebuild or become part of the resource glob.
    for old in output.iterdir():
        if old.is_file() and old.suffix.casefold() == ".dll":
            old.unlink()
    libraries = {}
    for name, path in sorted(files.items()):
        if path == binary:
            continue
        dest = output / path.name
        shutil.copy2(path, dest)
        libraries[path.name] = {
            "source": str(path),
            "sha256": hashlib.sha256(dest.read_bytes()).hexdigest(),
        }
    payload_loader = next(p for p in output.iterdir() if p.name.casefold() == "webview2loader.dll")
    audit(binary, output, [payload_loader])
    manifest = {"binary": binary.name, "machine": "x86_64", "dlls": libraries, "imports": imports}
    (output / "dependencies.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    if args.tauri_config:
        # Resource targets are relative to the installed executable on Windows.
        # Include assets because config merging replaces the resources value.
        resources = {"assets/": "assets/"}
        resources.update({str(output / name).replace("\\", "/"): name for name in libraries})
        args.tauri_config.parent.mkdir(parents=True, exist_ok=True)
        args.tauri_config.write_text(json.dumps({"bundle": {"resources": resources}}, indent=2) + "\n", encoding="utf-8")
    print("Collected runtime DLLs: " + ", ".join(libraries))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    collector = subparsers.add_parser("collect", help="resolve imports using the build's DLL search directories")
    collector.add_argument("--binary", required=True, type=Path)
    collector.add_argument("--output", required=True, type=Path)
    collector.add_argument("--extra-dll", action="append", default=[], type=Path)
    collector.add_argument("--search-dir", action="append", default=[], type=Path)
    collector.add_argument("--tauri-config", type=Path)
    verifier = subparsers.add_parser("verify", help="fail if a package needs any unshipped non-system DLL")
    verifier.add_argument("--binary", required=True, type=Path)
    verifier.add_argument("--runtime-dir", required=True, type=Path)
    verifier.add_argument("--extra-dll", action="append", default=[], type=Path)
    args = parser.parse_args()
    try:
        if args.command == "collect":
            collect(args)
        else:
            extras = list(args.extra_dll)
            loader = args.runtime_dir / "WebView2Loader.dll"
            if loader.is_file() and loader not in extras:
                extras.append(loader)
            if not any(p.name.casefold() == "webview2loader.dll" for p in extras):
                raise ValueError("package does not contain WebView2Loader.dll")
            files, _ = audit(args.binary, args.runtime_dir, extras)
            print(f"Verified dependency closure: {len(files) - 1} DLLs, no runner PATH used")
    except (OSError, ValueError, struct.error) as error:
        parser.exit(1, f"Windows runtime audit failed: {error}\n")


if __name__ == "__main__":
    main()
