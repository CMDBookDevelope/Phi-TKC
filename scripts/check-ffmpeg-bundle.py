#!/usr/bin/env python3
"""Source pin, archive safety, and payload regressions; never simulate a GPU."""
import copy
import hashlib
import importlib.util
import io
import json
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("ffmpeg_bundle", Path(__file__).with_name("fetch-ffmpeg.py"))
ffmpeg = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ffmpeg)


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def pin_file(self, pin):
        path = self.root / "pin.json"
        path.write_text(json.dumps(pin))
        return path

    def archive(self, name, kind=tarfile.REGTYPE):
        archive = self.root / "test.tar.gz"
        with tarfile.open(archive, "w:gz") as out:
            member = tarfile.TarInfo(name)
            member.type = kind
            if kind == tarfile.SYMTYPE:
                member.linkname = "/etc/passwd"
            out.addfile(member, io.BytesIO())
        return archive

    def test_checked_in_source_is_immutable_and_small(self):
        pin = ffmpeg.load_pin()
        self.assertEqual(pin["tag"], "n9.0.2")
        self.assertEqual(pin["commit"], "946fcce07b6dcd0331c8cc609192aeff5e1924f8")
        self.assertLess(pin["source"]["size"], 600_000_000)

    def test_mutable_url_and_missing_sha_rejected(self):
        for field, value in (("url", "https://example.org/latest.tar.xz"), ("sha256", ""), ("size", 600_000_001)):
            pin = copy.deepcopy(ffmpeg.load_pin())
            pin["source"][field] = value
            with self.assertRaises(ValueError):
                ffmpeg.load_pin(self.pin_file(pin))

    def test_bad_source_checksum_rejected(self):
        source = self.root / "source"
        source.write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "SHA256"):
            ffmpeg.verify_source(source, ffmpeg.load_pin())

    def test_traversal_and_symlinks_rejected(self):
        for name, kind in (("ffmpeg/../escape", tarfile.REGTYPE), ("ffmpeg/link", tarfile.SYMTYPE), ("/absolute", tarfile.REGTYPE)):
            with self.assertRaises(ValueError):
                ffmpeg.extract_source(self.archive(name, kind), self.root / "source")

    def test_nonempty_source_never_overwritten(self):
        source = self.root / "source"
        source.mkdir()
        (source / "keep").write_text("keep")
        with self.assertRaisesRegex(ValueError, "empty"):
            ffmpeg.extract_source(self.archive("ffmpeg/file"), source)
        self.assertEqual((source / "keep").read_text(), "keep")

    def test_missing_executable_or_configuration_fails(self):
        distribution = self.root / ffmpeg.DISTRIBUTION
        distribution.mkdir()
        (distribution / "pin.json").write_text(json.dumps(ffmpeg.load_pin()))
        with self.assertRaisesRegex(ValueError, "configuration"):
            ffmpeg.finalize(self.root)

    def test_provenance_rejects_missing_source_and_binary(self):
        distribution = self.root / ffmpeg.DISTRIBUTION
        distribution.mkdir()
        (distribution / "provenance.json").write_text(json.dumps({"pin": ffmpeg.load_pin(), "files": {}}))
        with self.assertRaisesRegex(ValueError, "omits"):
            ffmpeg.verify_payload(self.root)

    def test_finalize_never_ships_build_objects(self):
        pin = ffmpeg.load_pin()
        distribution = self.root / ffmpeg.DISTRIBUTION
        distribution.mkdir()
        (distribution / "pin.json").write_text(json.dumps(pin))
        (self.root / ffmpeg.BINARY).write_bytes(b"test-PE-placeholder-not-executed")
        for name in ("build-info.txt", "config.h", "config_components.h", "config.mak", "zimg-LICENSE.txt"):
            (distribution / name).write_text(name)
        build = self.root / "build"
        build.mkdir()
        (build / "never-ship.o").write_bytes(b"object")
        with patch.object(ffmpeg, "verify_source"):
            ffmpeg.finalize(self.root)
        files = json.loads((distribution / "provenance.json").read_text())["files"]
        self.assertIn(ffmpeg.BINARY, files)
        self.assertFalse(any(name.startswith("build/") for name in files))


if __name__ == "__main__":
    unittest.main()
