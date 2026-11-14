#!/usr/bin/env python3
"""Prepare a native-only test crate from the actual Apple renderer sources."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil


def prepare(source, deps, output):
    source, deps, output = map(lambda p: Path(p).resolve(), (source, deps, output))
    output.mkdir(parents=True, exist_ok=True)
    (output / "src").mkdir(exist_ok=True)
    modules = ["apple_media", "apple_frames", "apple_lifecycle"]
    hashes = {}
    for name in modules:
        path = source / "src-tauri/src" / f"{name}.rs"
        data = path.read_bytes()
        (output / "src" / path.name).write_bytes(data)
        hashes[str(path.relative_to(source))] = hashlib.sha256(data).hexdigest()
    integration = source / "scripts/apple-gpu-smoke.rs"
    if not integration.is_file():
        raise SystemExit("Missing actual GPU/CoreVideo integration tests")
    data = integration.read_bytes()
    (output / "src/apple_gpu_smoke.rs").write_bytes(data)
    hashes["scripts/apple-gpu-smoke.rs"] = hashlib.sha256(data).hexdigest()
    (output / "src/lib.rs").write_text(
        "mod apple_media;\nmod apple_frames;\nmod apple_lifecycle;\n"
        "#[cfg(test)] mod apple_gpu_smoke;\n"
    )
    patches = []
    for repo, package in [("prpr-miniquad", "miniquad"), ("prpr-macroquad", "macroquad")]:
        path = deps / repo
        if not (path / "Cargo.toml").is_file():
            raise SystemExit(f"Missing native dependency {path}")
        patches.append(f'[patch."https://github.com/CMDBookDevelope/{repo}"]\n{package} = {{ path = {json.dumps(str(path))} }}\n')
    (output / "Cargo.toml").write_text(
        '[package]\nname="apple-native-smoke"\nversion="0.1.0"\nedition="2021"\n'
        '[features]\napple-render-check=[]\n'
        '[dependencies]\nanyhow="1"\nobjc="0.2"\ntempfile="3"\nlog="0.4"\n'
        f'macroquad={{path={json.dumps(str(deps / "prpr-macroquad"))},default-features=false,features=["metal"]}}\n'
        f'miniquad={{path={json.dumps(str(deps / "prpr-miniquad"))},features=["metal"]}}\n'
        '[profile.release]\nopt-level=3\n' + "\n".join(patches)
    )
    if not (output / "Cargo.lock").exists():
        shutil.copy2(source / "src-tauri/Cargo.lock", output / "Cargo.lock")
    (output / "source-hashes.json").write_text(json.dumps(hashes, indent=2) + "\n")
    print(output / "Cargo.toml")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", required=True)
    parser.add_argument("--deps", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    prepare(args.source, args.deps, args.output)
