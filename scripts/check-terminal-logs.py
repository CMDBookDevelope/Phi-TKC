#!/usr/bin/env python3
"""Check terminal formatting and a real server render without a display server."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request

ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
PREFIX = re.compile(r"^(?:\(｀皿´＃\)\.ERROR!|\(╬ Ò﹏Ó\)\.WARN!|\(\*・ω・\)っ\.INFO♪|≽\^•⩊•\^≼\.DEBUG:|\(◎_◎\)\.TRACE~) ♡ ")


def formatted(text):
    lines = ANSI.sub("", text).splitlines()
    assert lines, "no terminal logs"
    for line in lines:
        assert PREFIX.match(line), repr(line)
    return len(lines)


def request(url, data=None, headers=None):
    with urllib.request.urlopen(urllib.request.Request(url, data=data, headers=headers or {}), timeout=10) as response:
        return json.load(response)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("assets", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    binary, assets = args.binary.resolve(), args.assets.resolve()
    env = os.environ.copy()
    for key in ["DISPLAY", "WAYLAND_DISPLAY", "PHI_TKC_IPC"]:
        env.pop(key, None)
    env.update(NO_COLOR="1", RUST_LOG="info", MINIQUAD_VULKAN_VERBOSE="1")
    results = []
    for argv, exit_code in [(["--help"], 0), (["--version"], 0), (["--invalid-option"], 2),
                            (["server", "--help"], 0), (["server", "--invalid-option"], 2)]:
        result = subprocess.run([str(binary), *argv], capture_output=True, text=True, env=env, timeout=30)
        assert result.returncode == exit_code, (argv, result.returncode, result.stdout, result.stderr)
        assert not result.stderr, result.stderr
        results.append({"args": argv, "exit": exit_code, "formatted_lines": formatted(result.stdout)})
    spec = importlib.util.spec_from_file_location("canvas_check", Path(__file__).with_name("check-render-resolution.py"))
    canvas = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(canvas)
    with tempfile.TemporaryDirectory(prefix="phi-terminal-check-") as temp:
        folder = Path(temp)
        archive = canvas.make_chart(folder)
        video = folder / "cli.mp4"
        render = subprocess.run([
            str(binary), "-i", str(archive), "-o", str(video), "--assets", str(assets),
            "-r", "320x180", "--fps", "5", "--encoder", "cpu", "--no-hw-accel",
            "--preset", "ultrafast", "--audio-format", "aac", "--ui", "none", "--dim", "0",
        ], capture_output=True, text=True, env=env, timeout=180)
        assert render.returncode == 0, render.stdout + render.stderr
        assert not render.stderr, render.stderr
        results.append({"cli_render": canvas.check_video(video, 320, 180), "formatted_lines": formatted(render.stdout)})
        with socket.socket() as reserved:
            reserved.bind(("127.0.0.1", 0))
            port = reserved.getsockname()[1]
            collision = subprocess.run([str(binary), "server", "--port", str(port)], capture_output=True, text=True, env=env, timeout=30)
            assert collision.returncode == 1 and not collision.stderr, collision
            formatted(collision.stdout)
        server_log, server_err = folder / "server.log", folder / "server.err"
        ids = []
        with server_log.open("w") as out, server_err.open("w") as err:
            server = subprocess.Popen([str(binary), "server", "--host", "127.0.0.1", "--port", str(port), "--assets", str(assets)], stdout=out, stderr=err, env=env)
            try:
                base = f"http://127.0.0.1:{port}"
                for attempt in range(100):
                    try:
                        status = request(base + "/api/status")
                        break
                    except (OSError, ValueError):
                        assert server.poll() is None, server_log.read_text()
                        time.sleep(0.1)
                else:
                    raise AssertionError("server did not start")
                for resolution in ["320x180", "320x181"]:
                    boundary = "phi-terminal-regression"
                    fields = {"resolution": resolution, "fps": "5", "encoder": "cpu", "no-hw-accel": "true", "preset": "ultrafast", "audio-format": "aac"}
                    body = bytearray()
                    for key, value in fields.items():
                        body.extend(f'--{boundary}\r\nContent-Disposition: form-data; name="{key}"\r\n\r\n{value}\r\n'.encode())
                    body.extend(f'--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="chart.pez"\r\nContent-Type: application/zip\r\n\r\n'.encode())
                    body.extend(archive.read_bytes())
                    body.extend(f"\r\n--{boundary}--\r\n".encode())
                    posted = request(base + "/api/render", bytes(body), {"Content-Type": f"multipart/form-data; boundary={boundary}"})
                    ids.append(posted["id"])
                    for attempt in range(1800):
                        job = request(base + "/api/jobs/" + ids[-1])
                        if job["status"] in ["done", "failed", "canceled"]:
                            break
                        time.sleep(0.1)
                    if resolution == "320x180":
                        assert job["status"] == "done" and job["progress"] == 1, job
                        with urllib.request.urlopen(base + job["download"], timeout=30) as download:
                            output = folder / "server.mp4"
                            output.write_bytes(download.read())
                        results.append({"server_render": canvas.check_video(output, 320, 180), "ipc_progress": job["progress"]})
                    else:
                        assert job["status"] == "failed" and "resolution" in job["error"].lower(), job
                        results.append({"failed_job_reports_resolution": True})
            finally:
                server.terminate()
                server.wait(timeout=10)
                for job_id in ids:
                    shutil.rmtree(Path(tempfile.gettempdir()) / "phi-tkc-server" / job_id, ignore_errors=True)
        text = server_log.read_text()
        assert not server_err.read_text(), server_err.read_text()
        assert "started" in text and "completed" in text and "ERROR!" in text, text
        assert "headless Vulkan device" in text and '"Frame"' not in text, text
        results.append({"server_formatted_lines": formatted(text), "child_logs_relayed": True, "raw_ipc_hidden": True})
    if args.report:
        args.report.write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results))


if __name__ == "__main__":
    main()
