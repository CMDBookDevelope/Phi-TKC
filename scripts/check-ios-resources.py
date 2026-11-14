#!/usr/bin/env python3
"""Check that an iOS app or IPA includes the renderer's complete asset tree."""

import argparse
import hashlib
import json
import plistlib
import tempfile
import unittest
import zipfile
from pathlib import Path, PurePosixPath


ROOT = Path(__file__).resolve().parents[1]
REQUIRED = ("font.ttf", "ending.mp3", "respack/info.yml")
# Tauri copies bundle.resources into its own assets folder while preserving
# the configured "assets" prefix. Keep this order aligned with lib.rs.
ASSET_ROOTS = ("assets/assets", "assets", "")


def asset_path(root, name):
    return f"{root}/{name}" if root else name


def verify_files(read, exists, source):
    expected = {
        path.relative_to(source).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(source.rglob("*"))
        if path.is_file()
    }
    if not all(name in expected for name in REQUIRED):
        raise ValueError(f"Source renderer assets are incomplete: {source}")

    root = next(
        (root for root in ASSET_ROOTS if all(exists(asset_path(root, name)) for name in REQUIRED)),
        None,
    )
    if root is None:
        raise ValueError("No usable renderer asset directory; required: " + ", ".join(REQUIRED))

    missing, changed = [], []
    for name, digest in expected.items():
        target = asset_path(root, name)
        if not exists(target):
            missing.append(name)
        elif hashlib.sha256(read(target)).hexdigest() != digest:
            changed.append(name)
    if missing or changed:
        raise ValueError(f"Renderer assets differ at {root or '.'}: missing={missing}, changed={changed}")
    return {"asset_root": root or ".", "verified_files": len(expected)}


def verify_bundle(bundle, source):
    if bundle.is_dir() and bundle.suffix == ".app":
        read = lambda name: (bundle / name).read_bytes()
        exists = lambda name: (bundle / name).is_file()
        result = verify_application(read, exists, source)
    elif bundle.is_file() and bundle.suffix == ".ipa":
        with zipfile.ZipFile(bundle) as archive:
            names = {info.filename for info in archive.infolist() if not info.is_dir()}
            apps = sorted(
                name.removesuffix("/Info.plist")
                for name in names
                if len(PurePosixPath(name).parts) == 3
                and PurePosixPath(name).parts[0] == "Payload"
                and PurePosixPath(name).parts[1].endswith(".app")
                and PurePosixPath(name).name == "Info.plist"
            )
            if len(apps) != 1:
                raise ValueError(f"Expected one main application in IPA Payload, found {apps}")
            prefix = apps[0] + "/"
            read = lambda name: archive.read(prefix + name)
            exists = lambda name: prefix + name in names
            result = verify_application(read, exists, source)
    else:
        raise ValueError(f"Expected an existing .app directory or .ipa file: {bundle}")
    return {"bundle": str(bundle), **result}


def verify_application(read, exists, source):
    info = plistlib.loads(read("Info.plist"))
    executable = info.get("CFBundleExecutable")
    if not executable or Path(executable).name != executable or not exists(executable):
        raise ValueError("Application executable is missing or invalid in Info.plist")
    return verify_files(read, exists, source)


class ResourceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        for name in (*REQUIRED, "respack/click.png"):
            path = self.source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(("fixture " + name).encode())

    def make_app(self, root):
        app = self.root / "Phi-TKC.app"
        app.mkdir(exist_ok=True)
        (app / "Info.plist").write_bytes(plistlib.dumps({"CFBundleExecutable": "Phi-TKC"}))
        (app / "Phi-TKC").write_bytes(b"fixture executable")
        for source in self.source.rglob("*"):
            if source.is_file():
                target = app / root / source.relative_to(self.source)
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(source.read_bytes())
        return app

    def ipa(self, app):
        ipa = self.root / "Phi-TKC.ipa"
        with zipfile.ZipFile(ipa, "w") as archive:
            for path in app.rglob("*"):
                if path.is_file():
                    archive.write(path, "Payload/Phi-TKC.app/" + path.relative_to(app).as_posix())
        return ipa

    def test_supported_layouts_in_app_and_ipa(self):
        for root in ASSET_ROOTS:
            with self.subTest(root=root):
                # Each layout must be checked independently; a nested copy must
                # not accidentally make a flat/root fixture pass.
                with tempfile.TemporaryDirectory(dir=self.root) as temporary:
                    original = self.root
                    self.root = Path(temporary)
                    app = self.make_app(root)
                    for bundle in (app, self.ipa(app)):
                        result = verify_bundle(bundle, self.source)
                        self.assertEqual(result["asset_root"], root or ".")
                        self.assertEqual(result["verified_files"], 4)
                    self.root = original

    def test_missing_font(self):
        app = self.make_app("assets/assets")
        (app / "assets/assets/font.ttf").unlink()
        with self.assertRaisesRegex(ValueError, "font.ttf"):
            verify_bundle(self.ipa(app), self.source)

    def test_missing_texture(self):
        app = self.make_app("assets/assets")
        (app / "assets/assets/respack/click.png").unlink()
        with self.assertRaisesRegex(ValueError, "missing=.*respack/click.png"):
            verify_bundle(self.ipa(app), self.source)

    def test_corrupted_font(self):
        app = self.make_app("assets/assets")
        (app / "assets/assets/font.ttf").write_bytes(b"corrupted")
        with self.assertRaisesRegex(ValueError, "changed=.*font.ttf"):
            verify_bundle(app, self.source)

    def test_partial_first_root_is_not_hidden_by_complete_fallback(self):
        app = self.make_app("assets/assets")
        self.make_app("assets")
        (app / "assets/assets/respack/click.png").unlink()
        with self.assertRaisesRegex(ValueError, "assets/assets.*respack/click.png"):
            verify_bundle(app, self.source)

    def test_missing_executable(self):
        app = self.make_app("assets/assets")
        (app / "Phi-TKC").unlink()
        with self.assertRaisesRegex(ValueError, "executable"):
            verify_bundle(self.ipa(app), self.source)

    def test_incomplete_source(self):
        app = self.make_app("assets/assets")
        (self.source / "font.ttf").unlink()
        with self.assertRaisesRegex(ValueError, "Source renderer assets"):
            verify_bundle(app, self.source)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundles", type=Path, nargs="*")
    parser.add_argument("--source", type=Path, default=ROOT / "src-tauri/assets")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(ResourceTests)
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        if not result.wasSuccessful():
            raise SystemExit(1)
    if not args.bundles and not args.self_test:
        parser.error("Provide at least one .app directory or .ipa file")
    for bundle in args.bundles:
        print(json.dumps(verify_bundle(bundle, args.source), ensure_ascii=False))


if __name__ == "__main__":
    main()
