#!/usr/bin/env python3
"""Install tracked app icons after Tauri creates its mobile template projects."""

import argparse
import json
import shutil
import struct
from pathlib import Path
from xml.etree import ElementTree


ROOT = Path(__file__).resolve().parents[1]
ANDROID_NS = "{http://schemas.android.com/apk/res/android}"


def png_size(path):
    header = path.read_bytes()[:24]
    if header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
        raise ValueError(f"Invalid PNG icon: {path}")
    return struct.unpack(">II", header[16:24])


def install_android(source, project):
    manifest = ElementTree.parse(project / "app/src/main/AndroidManifest.xml")
    application = manifest.getroot().find("application")
    if application is None or application.get(ANDROID_NS + "icon") != "@mipmap/ic_launcher":
        raise ValueError("Android template no longer uses @mipmap/ic_launcher")
    round_icon = application.get(ANDROID_NS + "roundIcon")
    if round_icon and round_icon != "@mipmap/ic_launcher_round":
        raise ValueError("Android template uses an unexpected round launcher icon")

    # Validate all launcher variants before replacing any template resources.
    for density in ("mdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi"):
        for name in ("ic_launcher.png", "ic_launcher_round.png", "ic_launcher_foreground.png"):
            width, height = png_size(source / f"mipmap-{density}" / name)
            if width != height:
                raise ValueError(f"Android icon must be square: {density}/{name}")
    adaptive = source / "mipmap-anydpi-v26/ic_launcher.xml"
    ElementTree.parse(adaptive)
    ElementTree.parse(source / "values/ic_launcher_background.xml")

    destination = project / "app/src/main/res"
    files = sorted(path for path in source.rglob("*") if path.is_file())
    for path in files:
        target = destination / path.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
    # Some template versions have a separate adaptive round icon. It must use
    # the same Phi foreground, rather than leaving the template logo behind.
    shutil.copyfile(adaptive, destination / "mipmap-anydpi-v26/ic_launcher_round.xml")
    for path in files:
        target = destination / path.relative_to(source)
        if path.read_bytes() != target.read_bytes():
            raise ValueError(f"Android icon copy differs: {target}")
    print(f"Installed {len(files) + 1} Android icon resources in {destination}")


def install_ios(source, project):
    destination = project / "Assets.xcassets/AppIcon.appiconset"
    catalog = json.loads((destination / "Contents.json").read_text())
    images = catalog.get("images", [])
    if not images or not any(image.get("idiom") == "ios-marketing" for image in images):
        raise ValueError("iOS AppIcon catalog is missing its marketing icon")
    files = []
    for image in images:
        filename = image.get("filename")
        if not filename or Path(filename).name != filename:
            raise ValueError(f"Unexpected iOS AppIcon filename: {filename}")
        path = source / filename
        scale = float(image["scale"].removesuffix("x"))
        size = tuple(int(float(value) * scale) for value in image["size"].split("x"))
        if png_size(path) != size:
            raise ValueError(f"iOS icon dimensions differ from catalog: {path}")
        files.append(path)
    for path in files:
        shutil.copyfile(path, destination / path.name)
    for path in files:
        if path.read_bytes() != (destination / path.name).read_bytes():
            raise ValueError(f"iOS icon copy differs: {path.name}")
    print(f"Installed {len(files)} iOS app icons in {destination}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("platform", choices=("android", "ios"))
    parser.add_argument("--project", type=Path, help="Override the generated project path")
    args = parser.parse_args()
    project = args.project or ROOT / "src-tauri/gen" / ("apple" if args.platform == "ios" else "android")
    source = ROOT / "src-tauri/icons" / args.platform
    if args.platform == "android":
        install_android(source, project)
    else:
        install_ios(source, project)


if __name__ == "__main__":
    main()
