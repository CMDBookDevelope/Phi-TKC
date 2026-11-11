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
DIAGNOSTICS = (
    "Using ffmpeg:", "setup chart + audio load:", "=== Encoder Selection ===",
    "ffmpeg video args:", "Readback:", "NV12 handoff:", "NV12 direct handoff:",
    "prpr stage times:", "Render loop Time:", "Video Pipeline Avg FPS:",
    "End-to-End Time:", "headless Vulkan device:", "Writer thread blocked",
    "Render thread waits:", "frame pool exhausted", "Speed preset:",
    "User preference:", "Encoder Availability", "setup encoder probe:",
    "[vulkan-encode]",
)


def messages(text, level):
    return [line.split(" ♡ ", 1)[1] for line in ANSI.sub("", text).splitlines()
            if " ♡ " in line and f".{level}" in line.split(" ♡ ", 1)[0]]


def user_logs(text):
    info = messages(text, "INFO♪")
    for stage in ["Preparing chart:", "Mixing audio", "Rendering video", "Finalizing video", "Video saved to"]:
        assert any(stage in message for message in info), (stage, info)
    for detail in DIAGNOSTICS:
        assert not any(detail in message for message in info), (detail, info)


def diagnostic_logs(text):
    debug = messages(text, "DEBUG:")
    for detail in ["Using ffmpeg:", "setup chart + audio load:", "=== Encoder Selection ===",
                   "ffmpeg video args:", "Readback:", "NV12 handoff:",
                   "prpr stage times:", "Video Pipeline Avg FPS:", "headless Vulkan device:"]:
        assert any(detail in message for message in debug), (detail, debug)


def welcome_logs(text, path):
    expected = ANSI.sub("", path.read_text()).splitlines()
    actual = messages(text, "INFO♪")
    assert actual[:len(expected)] == expected, (actual[:len(expected)], expected)


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
    parser.add_argument("--welcome-file", type=Path, help="Optional local reference ASCII; not needed in CI")
    args = parser.parse_args()
    binary, assets = args.binary.resolve(), args.assets.resolve()
    env = os.environ.copy()
    for key in ["DISPLAY", "WAYLAND_DISPLAY", "PHI_TKC_IPC", "RUST_LOG",
                "MINIQUAD_VULKAN_VERBOSE", "PHITK_NULL_SINK", "PHITK_NV12_HANDOFF"]:
        env.pop(key, None)
    env.update(NO_COLOR="1")
    results = []
    for argv, exit_code in [(["--help"], 0), (["--version"], 0), (["--invalid-option"], 2),
                            (["server", "--help"], 0), (["server", "--invalid-option"], 2)]:
        result = subprocess.run([str(binary), *argv], capture_output=True, text=True, env=env, timeout=30)
        assert result.returncode == exit_code, (argv, result.returncode, result.stdout, result.stderr)
        assert not result.stderr, result.stderr
        if exit_code:
            assert messages(result.stdout, "ERROR!"), (argv, result.stdout)
        results.append({"args": argv, "exit": exit_code, "formatted_lines": formatted(result.stdout)})
    spec = importlib.util.spec_from_file_location("canvas_check", Path(__file__).with_name("check-render-resolution.py"))
    canvas = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(canvas)
    with tempfile.TemporaryDirectory(prefix="phi-terminal-check-") as temp:
        folder = Path(temp)
        archive = canvas.make_chart(folder)
        video = folder / "cli.mp4"
        render_command = [
            str(binary), "-i", str(archive), "-o", str(video), "--assets", str(assets),
            "-r", "320x180", "--fps", "5", "--encoder", "cpu", "--no-hw-accel",
            "--preset", "ultrafast", "--audio-format", "aac", "--ui", "none", "--dim", "0",
        ]
        render = subprocess.run(render_command, capture_output=True, text=True, env=env, timeout=180)
        assert render.returncode == 0, render.stdout + render.stderr
        assert not render.stderr, render.stderr
        user_logs(render.stdout)
        assert not messages(render.stdout, "DEBUG:") and not messages(render.stdout, "TRACE~"), render.stdout
        if args.welcome_file:
            welcome_logs(render.stdout, args.welcome_file)
        results.append({"cli_render": canvas.check_video(video, 320, 180),
                        "mode": "default_info", "formatted_lines": formatted(render.stdout),
                        "welcome_matches_reference": bool(args.welcome_file)})
        debug_env = dict(env, RUST_LOG="debug", MINIQUAD_VULKAN_VERBOSE="1")
        debug_video = folder / "cli-debug.mp4"
        debug_command = render_command.copy()
        debug_command[debug_command.index("-o") + 1] = str(debug_video)
        debug_render = subprocess.run(debug_command, capture_output=True, text=True, env=debug_env, timeout=180)
        assert debug_render.returncode == 0 and not debug_render.stderr, debug_render.stdout + debug_render.stderr
        user_logs(debug_render.stdout)
        diagnostic_logs(debug_render.stdout)
        if args.welcome_file:
            welcome_logs(debug_render.stdout, args.welcome_file)
        results.append({"debug_cli_render": canvas.check_video(debug_video, 320, 180),
                        "debug_cli_formatted_lines": formatted(debug_render.stdout), "diagnostics_restored": True,
                        "default_info_contains_only_user_messages": True})
        with socket.socket() as reserved:
            reserved.bind(("127.0.0.1", 0))
            port = reserved.getsockname()[1]
            collision = subprocess.run([str(binary), "server", "--port", str(port)], capture_output=True, text=True, env=env, timeout=30)
            assert collision.returncode == 1 and not collision.stderr, collision
            formatted(collision.stdout)
            assert messages(collision.stdout, "ERROR!"), collision.stdout
        for mode, server_env, resolutions in [
            ("default_info", env, ["320x180", "320x181"]),
            ("debug", debug_env, ["320x180"]),
        ]:
            # Each mode starts its own server so the child inherits the intended
            # filter. Reserve a fresh port after the collision check above.
            with socket.socket() as reserved:
                reserved.bind(("127.0.0.1", 0))
                port = reserved.getsockname()[1]
            server_log, server_err = folder / f"server-{mode}.log", folder / f"server-{mode}.err"
            ids = []
            with server_log.open("w") as out, server_err.open("w") as err:
                server = subprocess.Popen([str(binary), "server", "--host", "127.0.0.1", "--port", str(port), "--assets", str(assets)], stdout=out, stderr=err, env=server_env)
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
                    for resolution in resolutions:
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
                                output = folder / f"server-{mode}.mp4"
                                output.write_bytes(download.read())
                            results.append({"server_render": canvas.check_video(output, 320, 180), "ipc_progress": job["progress"], "mode": mode})
                        else:
                            assert job["status"] == "failed" and "resolution" in job["error"].lower(), job
                            results.append({"failed_job_reports_resolution": True, "mode": mode})
                finally:
                    server.terminate()
                    server.wait(timeout=10)
                    for job_id in ids:
                        shutil.rmtree(Path(tempfile.gettempdir()) / "phi-tkc-server" / job_id, ignore_errors=True)
            text = server_log.read_text()
            assert not server_err.read_text(), server_err.read_text()
            assert "started" in text and "completed" in text, text
            user_logs(text)
            assert '"Frame"' not in text, text
            if mode == "default_info":
                assert messages(text, "ERROR!"), text
                assert not messages(text, "DEBUG:") and not messages(text, "TRACE~"), text
            else:
                diagnostic_logs(text)
            results.append({"server_formatted_lines": formatted(text), "mode": mode,
                            "child_logs_relayed": True, "raw_ipc_hidden": True,
                            "debug_diagnostics_relayed": mode == "debug"})
    if args.report:
        args.report.write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results))


if __name__ == "__main__":
    main()
