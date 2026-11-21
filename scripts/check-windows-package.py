#!/usr/bin/env python3
"""Check installed/portable payloads and launch them without the build's PATH."""
import argparse
import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "windows_runtime", Path(__file__).with_name("windows-runtime.py")
)
runtime = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runtime)
spec = importlib.util.spec_from_file_location("ffmpeg_bundle", Path(__file__).with_name("fetch-ffmpeg.py"))
ffmpeg = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ffmpeg)


def check(archive, version):
    with tempfile.TemporaryDirectory(prefix="phi-windows-check-") as temp:
        unpacked = Path(temp) / "payload"
        unpacked.mkdir()
        if archive.suffix.casefold() == ".zip":
            with zipfile.ZipFile(archive) as package:
                if package.testzip() is not None:
                    raise ValueError(f"{archive}: invalid ZIP checksum")
                package.extractall(unpacked)
        else:
            subprocess.run(
                ["7z", "x", str(archive.resolve()), f"-o{unpacked}", "-y"],
                check=True, capture_output=True,
            )
        executables = list(unpacked.rglob("Phi-TKC.exe"))
        if len(executables) != 1:
            raise ValueError(f"{archive}: expected exactly one Phi-TKC.exe")
        executable = executables[0]
        root = executable.parent
        loader = root / "WebView2Loader.dll"
        if not loader.is_file() or not (root / "assets" / "ending.mp3").is_file():
            raise ValueError(f"{archive}: loader and renderer assets must be beside the executable")
        sidecar = root / "ffmpeg-prores.exe"
        ffmpeg.verify_payload(root)
        files, _ = runtime.audit(executable, root, [loader, sidecar])
        payload = {
            str(path.relative_to(root)).replace("\\", "/"): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in [executable, sidecar, *root.glob("*.dll"), *root.glob("*.DLL"),
                         *root.joinpath("assets").rglob("*"), *root.joinpath(ffmpeg.DISTRIBUTION).rglob("*")]
            if path.is_file()
        }
        if os.name == "nt":
            system_root = Path(os.environ["SystemRoot"])
            env = dict(os.environ, PATH=os.pathsep.join([
                str(system_root / "System32"), str(system_root),
            ]))
            # An empty working directory also prevents a checkout/build DLL
            # from being found through the Windows current-directory search.
            working = Path(temp) / "empty-working-directory"
            working.mkdir()
            result = subprocess.run(
                [str(executable), "--version"], cwd=working, env=env,
                capture_output=True, text=True, encoding="utf-8", timeout=30,
            )
            if result.returncode or version not in result.stdout or "INFO♪" not in result.stdout or result.stderr:
                raise ValueError(
                    f"{archive}: isolated startup failed ({result.returncode:#x})\n"
                    f"{result.stdout}\n{result.stderr}"
                )
            def run_ffmpeg(*arguments):
                result = subprocess.run([str(sidecar), "-nostdin", "-hide_banner", *arguments],
                                        cwd=working, env=env, capture_output=True, text=True,
                                        encoding="utf-8", errors="replace", timeout=90)
                if result.returncode:
                    raise ValueError(f"{archive}: isolated FFmpeg failed ({result.returncode:#x})\n{result.stdout}\n{result.stderr}")
                return result.stdout + result.stderr

            encoders = run_ffmpeg("-encoders")
            names = {line.split()[1] for line in encoders.splitlines() if len(line.split()) >= 2}
            if not {"prores_ks", "prores_ks_vulkan"}.issubset(names):
                raise ValueError(f"{archive}: genuine CPU/Vulkan ProRes encoders missing")
            filters = run_ffmpeg("-filters")
            if not any(len(line.split()) >= 2 and line.split()[1] == "zscale" for line in filters.splitlines()):
                raise ValueError(f"{archive}: HDR zscale filter missing")
            license_text = run_ffmpeg("-L")
            if "Lesser General Public License" not in license_text:
                raise ValueError(f"{archive}: sidecar is not the source-complete LGPL build")
            fixture = working / "prores-cpu.mov"
            run_ffmpeg("-v", "error", "-f", "lavfi", "-i", "testsrc2=size=64x64:rate=1",
                       "-frames:v", "2", "-c:v", "prores_ks", "-profile:v", "3",
                       "-pix_fmt", "yuv422p10le", "-y", str(fixture))
            run_ffmpeg("-v", "error", "-i", str(fixture), "-frames:v", "2", "-f", "null", "-")
            print("Isolated FFmpeg encoder/filter listing and CPU ProRes encode/decode passed; GPU not tested")
        else:
            print("Native Windows startup skipped on this host; PE dependency audit completed")
        print(f"Verified {archive.name}: {len(files) - 2} runtime DLLs, complete assets and FFmpeg sources")
        return payload


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, action="append", type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    reference = None
    reports = {}
    for archive in args.archive:
        payload = check(archive, args.version)
        if reference is not None and payload != reference:
            missing = sorted(reference.keys() - payload.keys())
            extra = sorted(payload.keys() - reference.keys())
            changed = sorted(name for name in reference.keys() & payload.keys() if reference[name] != payload[name])
            raise ValueError(
                f"{archive}: package payload differs: "
                f"missing={missing}, extra={extra}, changed={changed}"
            )
        reference = payload
        reports[archive.name] = payload
    if args.report:
        args.report.write_text(json.dumps(reports, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
