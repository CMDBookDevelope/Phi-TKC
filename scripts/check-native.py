#!/usr/bin/env python3
"""Type-check local native renderer forks without saving path entries in Cargo.lock.

Run from any directory. Additional arguments are passed to cargo check, e.g.
--target aarch64-apple-ios on a host with Xcode installed.
"""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

project = Path(__file__).resolve().parents[1]
workspace = project.parent
manifest = project / "src-tauri" / "Cargo.toml"
lock = manifest.with_name("Cargo.lock")
original_lock = lock.read_bytes()
patches = {
    "prpr-macroquad": ("macroquad", workspace / "prpr-macroquad"),
    "prpr-miniquad": ("miniquad", workspace / "prpr-miniquad"),
    "Phi-TKC-render-lib": ("prpr", workspace / "Phi-TKC-render-lib" / "prpr"),
}
for name, (_, path) in patches.items():
    if not (path / "Cargo.toml").is_file():
        raise SystemExit(f"Missing sibling checkout {name}: {path}")

with tempfile.TemporaryDirectory(prefix="phi-tkc-native-check-") as temp:
    config = Path(temp) / "patches.toml"
    config.write_text("\n".join(
        f'[patch."https://github.com/CMDBookDevelope/{repo}"]\n'
        f'{package} = {{ path = {json.dumps(str(path))} }}'
        for repo, (package, path) in patches.items()
    ))
    cargo = os.environ.get("CARGO", "cargo")
    try:
        result = subprocess.run([
            cargo, "--config", str(config), "check", "--manifest-path", str(manifest),
            "--features", "apple-render-check,macroquad/metal", *sys.argv[1:]
        ], cwd=project)
    finally:
        lock.write_bytes(original_lock)
    raise SystemExit(result.returncode)
