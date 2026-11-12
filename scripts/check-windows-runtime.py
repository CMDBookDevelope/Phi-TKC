#!/usr/bin/env python3
"""Portable regression tests for PE runtime collection; no Windows SDK needed."""
import argparse
import importlib.util
import json
import os
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("windows_runtime", Path(__file__).with_name("windows-runtime.py"))
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)


def image(path, imports=(), delay_imports=(), machine=0x8664, delay_va=False):
    """Build a small valid PE32/PE32+ import table for dependency graph tests."""
    data = bytearray(4096)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 0x3C, 0x80)
    data[0x80:0x84] = b"PE\0\0"
    is64 = machine == 0x8664
    optional_size = 240 if is64 else 224
    optional = 0x98
    struct.pack_into("<HH", data, 0x84, machine, 1)
    struct.pack_into("<H", data, 0x94, optional_size)
    struct.pack_into("<H", data, optional, 0x20B if is64 else 0x10B)
    base = 0x10000000
    struct.pack_into("<Q" if is64 else "<I", data, optional + (24 if is64 else 28), base)
    struct.pack_into("<I", data, optional + 60, 0x400)
    struct.pack_into("<I", data, optional + (108 if is64 else 92), 16)
    section = optional + optional_size
    data[section:section + 8] = b".rdata\0\0"
    struct.pack_into("<IIII", data, section + 8, 0xC00, 0x1000, 0xC00, 0x400)
    directories = optional + (112 if is64 else 96)
    strings = 0x900
    for index, stride, names, start in ((1, 20, imports, 0x400), (13, 32, delay_imports, 0x600)):
        if not names:
            continue
        struct.pack_into("<II", data, directories + index * 8, start - 0x400 + 0x1000, (len(names) + 1) * stride)
        for i, name in enumerate(names):
            encoded = name.encode("ascii") + b"\0"
            data[strings:strings + len(encoded)] = encoded
            name_rva = strings - 0x400 + 0x1000
            descriptor = start + i * stride
            if index == 1:
                struct.pack_into("<I", data, descriptor + 12, name_rva)
            else:
                struct.pack_into("<II", data, descriptor, 0 if delay_va else 1, name_rva + (base if delay_va else 0))
            strings += len(encoded)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return path


class DependencyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.app = self.root / "build" / "Phi-TKC.exe"
        self.libs = self.root / "toolchain"
        self.output = self.root / "payload"

    def tearDown(self):
        self.temp.cleanup()

    def tree(self):
        image(self.app, ["KERNEL32.dll", "LIBSTDC++-6.DLL", "WebView2Loader.dll"])
        image(self.app.parent / "WebView2Loader.dll", ["ole32.dll"])
        image(self.libs / "libstdc++-6.dll", ["libgcc_s_seh-1.dll", "libwinpthread-1.dll"])
        image(self.libs / "libgcc_s_seh-1.dll", ["libwinpthread-1.dll"])
        image(self.libs / "libwinpthread-1.dll", ["msvcrt.dll"])

    def collect(self):
        args = argparse.Namespace(binary=self.app, output=self.output, extra_dll=[], search_dir=[self.libs], tauri_config=self.root / "overlay.json")
        runtime.collect(args)

    def test_transitive_graph_and_root_resource_mapping(self):
        self.tree()
        self.collect()
        expected = {"libstdc++-6.dll", "libgcc_s_seh-1.dll", "libwinpthread-1.dll", "WebView2Loader.dll"}
        self.assertEqual(expected, {p.name for p in self.output.glob("*.dll")})
        resources = json.loads((self.root / "overlay.json").read_text())["bundle"]["resources"]
        self.assertEqual(resources["assets/"], "assets/")
        self.assertEqual(set(resources.values()), expected | {"assets/"})
        files, _ = runtime.audit(self.app, self.output, [self.output / "WebView2Loader.dll"])
        self.assertEqual(len(files), 5)

    def test_missing_transitive_dependency_fails(self):
        self.tree()
        (self.libs / "libwinpthread-1.dll").unlink()
        with patch.dict(os.environ, {"PATH": ""}), self.assertRaisesRegex(ValueError, "libwinpthread-1.dll"):
            self.collect()

    def test_audit_never_uses_path(self):
        self.tree()
        self.output.mkdir()
        with patch.dict(os.environ, {"PATH": str(self.libs)}), self.assertRaisesRegex(ValueError, "LIBSTDC"):
            runtime.audit(self.app, self.output, [self.app.parent / "WebView2Loader.dll"])

    def test_audit_never_uses_build_directory(self):
        self.tree()
        image(self.app.parent / "libstdc++-6.dll")
        self.output.mkdir()
        with self.assertRaisesRegex(ValueError, "LIBSTDC"):
            runtime.audit(self.app, self.output, [self.app.parent / "WebView2Loader.dll"])

    def test_wrong_architecture_fails(self):
        self.tree()
        image(self.libs / "libwinpthread-1.dll", machine=0x14C)
        with self.assertRaisesRegex(ValueError, "wrong PE architecture"):
            self.collect()

    def test_delay_imports_both_formats(self):
        for machine in (0x8664, 0x14C):
            for va in (False, True):
                p = image(self.app, ["kernel32.dll"], ["late-runtime.dll"], machine=machine, delay_va=va)
                self.assertEqual(runtime.pe_dependencies(p), (machine, ["kernel32.dll", "late-runtime.dll"]))

    def test_delay_dependency_is_collected(self):
        self.tree()
        image(self.app, ["WebView2Loader.dll"], ["libstdc++-6.dll"])
        self.collect()
        self.assertTrue((self.output / "libwinpthread-1.dll").is_file())

    def test_system_dll_policy_excludes_third_party(self):
        for name in ("KERNEL32.dll", "ktmw32.dll", "api-ms-win-crt-runtime-l1-1-0.dll"):
            self.assertTrue(runtime.system_dll(name), name)
        for name in ("libstdc++-6.dll", "libwinpthread-1.dll", "WebView2Loader.dll", "vulkan-1.dll", "vcruntime140.dll", "msvcp140.dll"):
            self.assertFalse(runtime.system_dll(name), name)

    def test_stale_dll_removed_on_recollection(self):
        self.tree()
        self.output.mkdir()
        image(self.output / "old.dll")
        self.collect()
        self.assertFalse((self.output / "old.dll").exists())

    def test_missing_webview_loader_fails(self):
        image(self.app, ["kernel32.dll"])
        with self.assertRaisesRegex(ValueError, "WebView2Loader"):
            self.collect()

    def test_invalid_name_and_truncated_image_fail(self):
        image(self.app, ["../evil.dll"])
        with self.assertRaisesRegex(ValueError, "invalid imported DLL"):
            runtime.pe_dependencies(self.app)
        self.app.write_bytes(b"MZ")
        with self.assertRaisesRegex(ValueError, "truncated PE"):
            runtime.pe_dependencies(self.app)

    def test_conflicting_flat_dll_names_fail(self):
        image(self.app, ["kernel32.dll"])
        left = image(self.root / "left" / "runtime.dll", ["kernel32.dll"])
        right = image(self.root / "right" / "runtime.dll", ["user32.dll"])
        with self.assertRaisesRegex(ValueError, "conflicting copies"):
            runtime.dependency_graph(self.app, [], [left, right])


if __name__ == "__main__":
    unittest.main()
