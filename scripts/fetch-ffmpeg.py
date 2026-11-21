#!/usr/bin/env python3
"""Fetch SHA256-pinned LGPL FFmpeg sources and preserve them with the sidecar.

The bundled binary is built from these sources, not a vendor GPL binary with
unavailable corresponding sources for its many statically linked libraries.
Compressed downloads stay in a temporary directory (never the checkout).
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import tarfile
import tempfile
import urllib.request
from pathlib import Path

PIN_FILE = Path(__file__).with_name("ffmpeg-prores-pin.json")
DISTRIBUTION = "ffmpeg-prores-distribution"
BINARY = "ffmpeg-prores.exe"
MAX_DOWNLOAD = 600_000_000


def sha256(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_pin(path=PIN_FILE):
    pin = json.loads(Path(path).read_text(encoding="utf-8"))
    source = pin["source"]
    if not re.fullmatch(r"[0-9a-f]{64}", source["sha256"]):
        raise ValueError("source requires an explicit SHA256 pin")
    if not re.fullmatch(r"[0-9a-f]{40}", pin["commit"]):
        raise ValueError("FFmpeg requires a full immutable source commit")
    if "latest" in source["url"] or not source["url"].startswith("https://"):
        raise ValueError("FFmpeg source URL must be immutable HTTPS, not latest")
    if not 0 < source["size"] <= MAX_DOWNLOAD:
        raise ValueError("compressed download exceeds the 600 MB limit")
    if Path(source["filename"]).name != source["filename"]:
        raise ValueError("invalid source archive filename")
    return pin


def verify_source(archive, pin):
    source = pin["source"]
    if archive.stat().st_size != source["size"] or sha256(archive) != source["sha256"]:
        raise ValueError("FFmpeg source size/SHA256 does not match the checked-in pin")


def extract_source(archive, output):
    """Extract regular source files only; reject traversal, devices and links."""
    if output.exists() and any(output.iterdir()):
        raise ValueError(f"source directory must be empty: {output.resolve()}")
    output.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive) as package:
        members = package.getmembers()
        roots = {Path(member.name).parts[0] for member in members if member.name}
        if len(roots) != 1:
            raise ValueError("expected one FFmpeg archive root")
        for member in members:
            path = Path(member.name)
            if path.is_absolute() or ".." in path.parts or "\\" in member.name:
                raise ValueError("unsafe FFmpeg source archive path")
            if not (member.isdir() or member.isfile()):
                raise ValueError("source archive contains a link or special file")
        # Strip the upstream top-level folder without changing any source bytes.
        for member in members:
            parts = Path(member.name).parts[1:]
            if not parts:
                continue
            target = output.joinpath(*parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with package.extractfile(member) as source, target.open("wb") as dest:
                    shutil.copyfileobj(source, dest)
                target.chmod(member.mode & 0o755)
    if not (output / "configure").is_file() or not (output / "libavcodec/proresenc_kostya_vulkan.c").is_file():
        raise ValueError("pinned sources do not contain the Vulkan ProRes encoder")


def prepare(args):
    pin = load_pin()
    output = args.output.resolve()
    if output.exists() and any(output.iterdir()):
        raise ValueError(f"FFmpeg bundle output must be empty: {output}")
    if output == args.source_dir.resolve() or output in args.source_dir.resolve().parents:
        raise ValueError("source and distribution directories must be separate")
    with tempfile.TemporaryDirectory(prefix="phi-ffmpeg-download-") as temp:
        archive = args.archive or Path(temp) / pin["source"]["filename"]
        if args.archive is None:
            request = urllib.request.Request(pin["source"]["url"], headers={"User-Agent": "Phi-TKC-source-bundler"})
            with urllib.request.urlopen(request, timeout=90) as response, archive.open("wb") as stream:
                total = 0
                while block := response.read(1024 * 1024):
                    total += len(block)
                    if total > pin["source"]["size"] or total > MAX_DOWNLOAD:
                        raise ValueError("source download exceeded its pinned size")
                    stream.write(block)
        verify_source(archive, pin)
        extract_source(archive, args.source_dir)
        distribution = output / DISTRIBUTION
        distribution.mkdir(parents=True)
        shutil.copy2(archive, distribution / pin["source"]["filename"])
    for name in ("COPYING.LGPLv2.1", "COPYING.LGPLv3", "LICENSE.md", "COPYING.GPLv2", "COPYING.GPLv3"):
        shutil.copy2(args.source_dir / name, distribution / name)
    shutil.copy2(PIN_FILE, distribution / "pin.json")
    shutil.copy2(PIN_FILE, distribution / PIN_FILE.name)
    shutil.copy2(Path(__file__), distribution / "fetch-ffmpeg.py")
    shutil.copy2(Path(__file__).with_name("build-ffmpeg-prores.sh"), distribution / "build-ffmpeg-prores.sh")
    (distribution / "README.txt").write_text(
        "FFmpeg ProRes sidecar for Phi-TKC\n\n"
        f"Upstream: https://ffmpeg.org/\nRelease/tag: {pin['tag']}\nCommit: {pin['commit']}\n"
        f"Source: {pin['source']['url']}\nSHA256: {pin['source']['sha256']}\n\n"
        "FFmpeg is copyright its contributors, licensed LGPL 2.1 or later.\n"
        "This build disables GPL, version3 and nonfree components and third-party\n"
        "library autodetection. Its complete corresponding FFmpeg sources, license\n"
        "texts, exact build script and generated configuration accompany this binary.\n"
        "The executable is separate from Phi-TKC; you may rebuild, modify, debug\n"
        "and replace it. No source download or written source offer is required.\n"
        "Vulkan headers are used at build time; Vulkan is loaded dynamically from\n"
        "the graphics driver only when needed. Shader compiler tools are build-time\n"
        "tools only, not linked into this binary. See build-info.txt for versions.\n"
        "There is no warranty. This package contains no GPL vendor FFmpeg binary.\n",
        encoding="utf-8",
    )
    print(f"Prepared pinned FFmpeg source {pin['tag']} in {args.source_dir}")


def finalize(output):
    distribution = output / DISTRIBUTION
    pin = load_pin()
    if json.loads((distribution / "pin.json").read_text(encoding="utf-8")) != pin:
        raise ValueError("staged FFmpeg metadata differs from the checked-in pin")
    required = [output / BINARY, distribution / "build-info.txt", distribution / "config.h",
                distribution / "config_components.h", distribution / "config.mak", distribution / "zimg-LICENSE.txt"]
    if not all(path.is_file() for path in required):
        raise ValueError("FFmpeg executable and exact generated build configuration are required")
    verify_source(distribution / pin["source"]["filename"], pin)
    files = {path.relative_to(output).as_posix(): sha256(path)
             for path in [output / BINARY, *sorted(distribution.rglob("*"))]
             if path.is_file() and path.name != "provenance.json"}
    (distribution / "provenance.json").write_text(json.dumps({"pin": pin, "files": files}, indent=2) + "\n", encoding="utf-8")
    print(f"Finalized source-complete {BINARY}: SHA256 {files[BINARY]}")


def verify_payload(root):
    distribution = root / DISTRIBUTION
    pin = load_pin()
    provenance = json.loads((distribution / "provenance.json").read_text(encoding="utf-8"))
    if provenance["pin"] != pin:
        raise ValueError("package FFmpeg pin differs from the checked-in source pin")
    required = {BINARY, f"{DISTRIBUTION}/{pin['source']['filename']}",
                *(f"{DISTRIBUTION}/{name}" for name in ("README.txt", "COPYING.LGPLv2.1", "LICENSE.md",
                                                       "build-ffmpeg-prores.sh", "fetch-ffmpeg.py", "ffmpeg-prores-pin.json",
                                                       "build-info.txt", "zimg-LICENSE.txt", "config.h",
                                                       "config_components.h", "config.mak", "pin.json"))}
    if not required.issubset(provenance["files"]):
        raise ValueError("package omits FFmpeg corresponding source, license, or build configuration")
    for name, expected in provenance["files"].items():
        path = Path(name)
        if path.is_absolute() or ".." in path.parts or "\\" in name:
            raise ValueError("unsafe FFmpeg provenance path")
        if not (root / path).is_file() or sha256(root / path) != expected:
            raise ValueError(f"FFmpeg distribution checksum mismatch: {name}")
    verify_source(distribution / pin["source"]["filename"], pin)
    return provenance


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--source-dir", type=Path)
    parser.add_argument("--archive", type=Path, help="use a local source archive, still SHA256 verified")
    parser.add_argument("--finalize", action="store_true")
    args = parser.parse_args()
    try:
        if args.finalize:
            finalize(args.output)
        else:
            if args.source_dir is None:
                parser.error("--source-dir is required when preparing sources")
            prepare(args)
    except (OSError, ValueError, KeyError, tarfile.TarError) as error:
        parser.exit(1, f"FFmpeg source bundling failed: {error}\n")
