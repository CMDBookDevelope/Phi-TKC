#!/usr/bin/env python3
"""Render a legacy 16:9 chart at several canvas sizes and check all four corners.

Requires a built TKC/CLI binary, renderer assets, FFmpeg and a Vulkan device.
Temporary chart/video files are removed after the check.
"""
import argparse
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import wave
import zipfile
import zlib


def make_chart(folder: Path) -> Path:
    def chunk(kind, data):
        return struct.pack("!I", len(data)) + kind + data + struct.pack("!I", zlib.crc32(kind + data))

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack("!IIBBBBB", 32, 32, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress((b"\0" + bytes((40, 180, 100)) * 32) * 32))
    png += chunk(b"IEND", b"")
    (folder / "background.png").write_bytes(png)
    with wave.open(str(folder / "song.wav"), "wb") as audio:
        audio.setparams((2, 2, 44100, 0, "NONE", "not compressed"))
        audio.writeframes(b"".join(
            struct.pack("<hh", *([int(800 * math.sin(i * 440 * 2 * math.pi / 44100))] * 2))
            for i in range(88200)
        ))
    event = lambda value: {"startTime": 0, "endTime": 100000, "start": value, "end": value}
    move = {**event(0.5), "start2": 0.5, "end2": 0.5}
    chart = {"formatVersion": 3, "offset": 0, "judgeLineList": [{
        "bpm": 120, "judgeLineDisappearEvents": [event(1)],
        "judgeLineRotateEvents": [event(0)], "judgeLineMoveEvents": [move],
        "speedEvents": [{"startTime": 0, "endTime": 100000, "value": 1}],
        "notesAbove": [{"type": 1, "time": 32, "positionX": 0,
                        "holdTime": 0, "speed": 1, "floorPosition": 0.5}],
        "notesBelow": [],
    }]}
    (folder / "chart.json").write_text(json.dumps(chart))
    (folder / "info.txt").write_text(
        "#\nName: Canvas regression\nLevel: EZ Lv.1\nCharter: regression\n"
        "Chart: chart.json\nMusic: song.wav\nPicture: background.png\n"
        "AspectRatio: 1.7777777778\nBackgroundDim: 0\n"
    )
    archive = folder / "legacy.pez"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as out:
        for name in ["chart.json", "song.wav", "background.png", "info.txt"]:
            out.write(folder / name, name)
    return archive


def check_video(path: Path, width: int, height: int):
    probe = json.loads(subprocess.check_output([
        "ffprobe", "-v", "error", "-show_streams", "-of", "json", str(path)
    ]))
    video = next(s for s in probe["streams"] if s["codec_type"] == "video")
    assert (video["width"], video["height"]) == (width, height), video
    assert any(s["codec_type"] == "audio" for s in probe["streams"]), probe
    rgb = subprocess.check_output([
        "ffmpeg", "-v", "error", "-ss", "0.5", "-i", str(path),
        "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"
    ])
    assert len(rgb) == width * height * 3, len(rgb)
    corners = []
    for x, y in [(2, 2), (width - 3, 2), (2, height - 3), (width - 3, height - 3)]:
        start = (y * width + x) * 3
        red, green, blue = rgb[start:start + 3]
        assert green > 70 and green > red + 30 and green > blue + 20, (x, y, red, green, blue)
        corners.append([red, green, blue])
    return {"resolution": [width, height], "codec": video["codec_name"], "corners": corners}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("assets", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    binary, assets = args.binary.resolve(), args.assets.resolve()
    env = os.environ.copy()
    env.pop("DISPLAY", None)
    env.pop("WAYLAND_DISPLAY", None)
    results = []
    with tempfile.TemporaryDirectory(prefix="phi-canvas-regression-") as temp:
        folder = Path(temp)
        archive = make_chart(folder)
        cases = [(320, 180, 4), (320, 240, 4), (640, 180, 4), (180, 320, 4), (320, 240, 1)]
        for width, height, samples in cases:
            output = folder / f"{width}x{height}-{samples}.mp4"
            log = folder / "render.log"
            with log.open("w") as out:
                result = subprocess.run([
                    str(binary), "-i", str(archive), "-o", str(output),
                    "-r", f"{width}x{height}", "--assets", str(assets),
                    "--fps", "5", "--encoder", "cpu", "--no-hw-accel",
                    "--preset", "ultrafast", "--audio-format", "aac",
                    "--no-particle", "--ui", "none", "--dim", "0",
                    "--sample-count", str(samples),
                    *(["--no-effect"] if samples == 1 else []),
                ], env=env, stdout=out, stderr=subprocess.STDOUT, timeout=180)
            if result.returncode:
                raise RuntimeError(log.read_text(errors="replace")[-6000:])
            results.append({**check_video(output, width, height), "sample_count": samples})
            print(json.dumps(results[-1]), flush=True)
        for invalid in ["0x1080", "320x241", "bad", "320x240x2"]:
            result = subprocess.run([
                str(binary), "-i", str(archive), "-r", invalid, "--assets", str(assets)
            ], env=env, capture_output=True, text=True, timeout=30)
            assert result.returncode != 0, invalid
            assert "resolution" in (result.stdout + result.stderr).lower(), result.stderr
    if args.report:
        args.report.write_text(json.dumps({"binary": str(binary), "renders": results,
                                          "invalid_resolutions_rejected": 4}, indent=2) + "\n")


if __name__ == "__main__":
    main()
