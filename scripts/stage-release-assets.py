#!/usr/bin/env python3
"""Stage only the explicit release files; Actions evidence never ships here."""
import argparse
import shutil
from pathlib import Path


def release_files(platform: str, version: str) -> tuple[str, ...]:
    if platform == 'mobile':
        return ('Phi-TKC-android-arm64.apk', 'Phi-TKC.ipa')
    return tuple(f'Phi-TKC_{version}_{suffix}' for suffix in (
        'aarch64.dmg', 'amd64.AppImage', 'amd64.deb', 'linux_x86_64.tar.gz',
        'windows_x86_64.7z', 'windows_x86_64.zip', 'x64-setup.exe',
    ))


def stage(platform: str, version: str, artifacts: Path, output: Path) -> None:
    paths = []
    for name in release_files(platform, version):
        matches = [p for p in artifacts.rglob(name) if p.is_file()]
        if len(matches) != 1 or matches[0].stat().st_size == 0:
            raise ValueError(f'Expected one nonempty {name}, found {matches}')
        paths.append(matches[0])
    output.mkdir()
    for path in paths:
        shutil.copyfile(path, output / path.name)
        print(f'Staged {path.name}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('platform', choices=('desktop', 'mobile'))
    parser.add_argument('version')
    parser.add_argument('artifacts', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    try:
        stage(args.platform, args.version, args.artifacts, args.output)
    except (ValueError, OSError) as exc:
        parser.exit(1, f'{exc}\n')
