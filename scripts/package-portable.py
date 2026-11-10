#!/usr/bin/env python3
"""Create archives with a runnable binary and adjacent renderer resources."""
import argparse
import shutil
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path


def package(args):
    binary = args.binary.resolve()
    assets = args.assets.resolve()
    if not binary.is_file() or not (assets / "ending.mp3").is_file():
        raise ValueError("the executable and complete renderer assets are required")
    windows = args.target.startswith("windows")
    if windows and (args.webview_loader is None or not args.webview_loader.is_file()):
        raise ValueError("Windows archives require WebView2Loader.dll")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    name = f"{args.app}_{args.version}_{args.target}"
    with tempfile.TemporaryDirectory(prefix="phi-portable-") as temp:
        root = Path(temp) / name
        root.mkdir()
        shutil.copy2(binary, root / binary.name)
        shutil.copytree(assets, root / "assets")
        if windows:
            shutil.copy2(args.webview_loader, root / "WebView2Loader.dll")
            for dll in args.runtime_dll:
                shutil.copy2(dll, root / dll.name)
        else:
            # The CLI has an explicit assets flag; let the caller override it.
            cli_assets = ""
            if args.app == "phi-tk-cli":
                cli_assets = '''for arg in "$@"; do
    case "$arg" in
        --assets|--assets=*) exec "$app_dir/phi-tk-cli" "$@" ;;
    esac
done
set -- --assets "$app_dir/assets" "$@"
'''
            launcher = root / "launch.sh"
            launcher.write_text(
                '#!/bin/sh\nset -eu\n'
                'app_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)\n'
                + cli_assets
                + f'exec "$app_dir/{binary.name}" "$@"\n'
            )
        instructions = (
            f"{args.app} {args.version} ({args.target})\n\n"
            "Keep the assets directory beside the executable.\n"
            "Install ffmpeg and a Vulkan-capable graphics driver to render charts.\n"
        )
        if windows:
            instructions += (
                f"Launch {binary.name} for the GUI, or pass CLI/server arguments in a terminal.\n"
                "The GUI requires the Microsoft Edge WebView2 Runtime.\n"
            )
        else:
            instructions += (
                "Run ./launch.sh with your usual command-line arguments.\n"
                "Linux runtime dependencies: libasound2, libvulkan1, libgl1, libx11-6,\n"
                "libxi6, libxcursor1, zlib1g.\n"
            )
            if args.app != "phi-tk-cli":
                instructions += "The desktop GUI also requires GTK 3 and WebKitGTK 4.1.\n"
        (root / "README.txt").write_text(instructions)
        # Do not ship the checkout's world-writable modes in portable archives.
        for path in [root, *root.rglob("*")]:
            path.chmod(0o755 if path.is_dir() or path.name in (binary.name, "launch.sh") else 0o644)
        if windows:
            archive = output / f"{name}.zip"
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as z:
                for path in sorted(root.rglob("*")):
                    if path.is_file():
                        z.write(path, path.relative_to(root.parent))
            archive7z = output / f"{name}.7z"
            archive7z.unlink(missing_ok=True)
            subprocess.run(["7z", "a", "-t7z", str(archive7z), name], cwd=root.parent, check=True)
            subprocess.run(["7z", "t", str(archive7z)], check=True)
        else:
            archive = output / f"{name}.tar.gz"
            with tarfile.open(archive, "w:gz") as tar:
                tar.add(root, arcname=name)
        print(f"Created portable archive(s) in {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", required=True, choices=["Phi-TKC", "phi-tk-cli"])
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True, choices=["linux_x86_64", "windows_x86_64"])
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--assets", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--webview-loader", type=Path)
    parser.add_argument("--runtime-dll", action="append", default=[], type=Path)
    package(parser.parse_args())
