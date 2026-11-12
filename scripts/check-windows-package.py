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
        files, _ = runtime.audit(executable, root, [loader])
        payload = {
            str(path.relative_to(root)).replace("\\", "/"): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in [executable, *root.glob("*.dll"), *root.glob("*.DLL"), *root.joinpath("assets").rglob("*")]
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
        else:
            print("Native Windows startup skipped on this host; PE dependency audit completed")
        print(f"Verified {archive.name}: {len(files) - 1} runtime DLLs, complete assets")
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
