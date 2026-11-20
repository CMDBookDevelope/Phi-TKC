#!/usr/bin/env python3
"""Compare actual old/new Apple pipelines on one Mac using an identical 20s clip.

Only temporary source copies receive duration/timing instrumentation. The input
archive and rendered videos stay private; the public report contains metrics and
hashes. Build both binaries before the alternating timed runs.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import tempfile
import time
import tomllib


def replace_once(source, anchor, replacement):
    if source.count(anchor) != 1:
        raise ValueError(f"Production benchmark anchor is not unique: {anchor!r}")
    return source.replace(anchor, replacement, 1)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def instrument(source):
    anchor = "let video_length = O + length + A + params.config.ending_length;"
    source = replace_once(source, anchor, anchor + '''
    // Test-only: bound mixing allocation as well as output frames. A tiny tail
    // guards float-to-sample rounding; the encoder receives exactly 20s below.
    let __bench_ss = if params.config.disable_loading { O } else { 0.1 };
    let video_length = video_length.min(__bench_ss + 20.01);
''')
    anchor = "let pcm16 = &pcm16[start_bytes..end_bytes.max(start_bytes)];"
    source = replace_once(source, anchor, anchor + '''
    let __bench_bytes = 20 * sample_rate * 4;
    anyhow::ensure!(pcm16.len() >= __bench_bytes, "fixture is shorter than the benchmark window");
    let pcm16 = &pcm16[..__bench_bytes];
    crate::bench::pcm(pcm16, sample_rate)?;
''')
    source = replace_once(source, "let frames = (video_length * fps_f64).ceil() as u64;",
                          "let frames = skip + 20 * fps as u64;")
    anchor = "let mut fps_timer = Instant::now();"
    source = replace_once(source, anchor, "crate::bench::begin();\n    " + anchor)
    anchor = '    info!(\n        "Metal render complete: {output_frames} frames in {:.2}s",'
    source = replace_once(source, anchor, "    crate::bench::finish(output_frames)?;\n" + anchor)
    anchor = ".read_rgba(texture, &mut rgba);"
    readback = source.count(anchor)
    if readback:
        source = replace_once(source, anchor, anchor + "\n            crate::bench::readback(rgba.len());")
    return source, readback


BENCH_RS = '''use anyhow::Result;
use sha2::{Digest, Sha256};
use std::{sync::{Mutex, OnceLock}, time::Instant};
struct State { started: Option<Instant>, pcm_sha: String, pcm_frames: usize, rate: usize, readbacks: u64, bytes: u64 }
fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State { started: None, pcm_sha: String::new(), pcm_frames: 0, rate: 0, readbacks: 0, bytes: 0 }))
}
pub fn pcm(bytes: &[u8], rate: usize) -> Result<()> {
    anyhow::ensure!(bytes.len() == 20 * rate * 4, "PCM window differs");
    let mut s = state().lock().unwrap();
    s.pcm_sha = format!("{:x}", Sha256::digest(bytes)); s.pcm_frames = bytes.len()/4; s.rate = rate;
    Ok(())
}
pub fn begin() { state().lock().unwrap().started = Some(Instant::now()); }
pub fn readback(bytes: usize) { let mut s = state().lock().unwrap(); s.readbacks += 1; s.bytes += bytes as u64; }
pub fn finish(frames: u64) -> Result<()> {
    let s = state().lock().unwrap();
    let seconds = s.started.unwrap().elapsed().as_secs_f64();
    let metrics = serde_json::json!({"frames":frames,"frame_loop_and_finish_seconds":seconds,"fps":frames as f64/seconds,
        "pcm_sha256":s.pcm_sha,"pcm_frames":s.pcm_frames,"sample_rate":s.rate,"cpu_readback_calls":s.readbacks,"cpu_readback_bytes":s.bytes});
    std::fs::write(std::env::var("PHI_TKC_BENCH_METRICS")?, serde_json::to_vec_pretty(&metrics)?)?;
    Ok(())
}
'''

MAIN_RS = '''mod apple_media;
mod apple_platform;
mod apple_render;
mod common;
mod render;
mod bench;
__EXTRA_MODULES__
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(args.len() == 6, "expected fixture, output, assets, width, height");
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let _guard = runtime.enter();
    let directory = tempfile::tempdir()?;
    let _ = common::CONFIG_DIR.set(directory.path().to_owned());
    let _ = common::DATA_DIR.set(directory.path().to_owned());
    let path = std::path::PathBuf::from(&args[1]);
    let mut fs = prpr::fs::fs_from_file(&path)?;
    let info = runtime.block_on(prpr::fs::load_info(&mut *fs))?;
    let mut config = render::RenderConfig::default();
    config.resolution = (args[4].parse()?, args[5].parse()?);
    config.fps = 60; config.sample_count = 4; config.disable_loading = true;
    config.ending_length = 0.; config.bitrate_control = "CBR".into(); config.bitrate = "8M".into();
    config.target_audio = 48000; config.video_codec = "h264".into();
    apple_render::run(apple_render::RenderRequest {
        params: render::RenderParams { path, info, config },
        output_path: args[2].clone().into(), asset_dir: args[3].clone().into(), request_cancel: None,
    }, |_| {})
}
'''


def prepare(source, deps, output):
    source, deps, output = map(lambda p: Path(p).resolve(), (source, deps, output))
    (output / "src").mkdir(parents=True, exist_ok=True)
    native = source / "src-tauri/src"
    hashes = {}
    extras = []
    for name in ["apple_media", "apple_platform", "common", "apple_frames", "apple_lifecycle"]:
        path = native / f"{name}.rs"
        if not path.exists():
            continue
        data = path.read_bytes()
        (output / "src" / path.name).write_bytes(data)
        hashes[str(path.relative_to(source))] = hashlib.sha256(data).hexdigest()
        if name in ["apple_frames", "apple_lifecycle"]:
            extras.append(f"mod {name};")
    path = native / "apple_render.rs"
    original = path.read_text()
    hashes[str(path.relative_to(source))] = hashlib.sha256(original.encode()).hexdigest()
    patched, readback = instrument(original)
    (output / "src/apple_render.rs").write_text(patched)
    render_path = native / "render.rs"
    render = render_path.read_text()
    hashes[str(render_path.relative_to(source))] = hashlib.sha256(render.encode()).hexdigest()
    start = render.index('#[derive(Deserialize, Serialize, Clone)]\n#[serde(rename_all = "camelCase")]\n#[serde(default)]\npub struct RenderConfig')
    end = render.index('#[derive(Serialize, Deserialize)]\npub enum IPCEvent', start)
    player = render.index("pub async fn build_player(")
    player_end = render.index('\nconst DX_DEVICE:', player)
    header = '''prpr::tl_file!("render");
use anyhow::{bail, Context, Result};
use macroquad::prelude::*;
use prpr::{config::{ChallengeModeColor, Config}, ext::SafeTexture, info::ChartInfo, scene::BasicPlayer};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
'''
    (output / "src/render.rs").write_text(header + render[start:end] + render[player:player_end])
    (output / "src/bench.rs").write_text(BENCH_RS)
    (output / "src/main.rs").write_text(MAIN_RS.replace("__EXTRA_MODULES__", "\n".join(extras)))
    shutil.copytree(source / "src-tauri/locales", output / "locales", dirs_exist_ok=True)
    patches = []
    for repo, package, suffix in [("prpr-miniquad", "miniquad", ""), ("prpr-macroquad", "macroquad", ""), ("Phi-TKC-render-lib", "prpr", "prpr")]:
        path = deps / repo / suffix
        if not (path / "Cargo.toml").is_file():
            raise ValueError(f"Missing exact native dependency {path}")
        patches.append(f'[patch."https://github.com/CMDBookDevelope/{repo}"]\n{package}={{path={json.dumps(str(path))}}}\n')
    source_lock = tomllib.loads((source / "src-tauri/Cargo.lock").read_text())
    sasa = next(p["source"] for p in source_lock["package"] if p["name"] == "sasa")
    sasa_url, sasa_sha = sasa.removeprefix("git+").split("#")
    manifest = '''[package]
name="apple-pipeline-bench"
version="0.1.0"
edition="2021"
[features]
apple-render-check=[]
[dependencies]
anyhow="1"
log="0.4"
objc="0.2"
tempfile="3"
serde={version="1",features=["derive"]}
serde_json="1"
sha2="0.10"
tokio={version="1",features=["full"]}
'''
    manifest += f'macroquad={{path={json.dumps(str(deps / "prpr-macroquad"))},default-features=false,features=["metal"]}}\n'
    manifest += f'prpr={{path={json.dumps(str(deps / "Phi-TKC-render-lib/prpr"))}}}\n'
    manifest += f'sasa={{git={json.dumps(sasa_url)},rev={json.dumps(sasa_sha)}}}\n'
    manifest += '[profile.release]\nopt-level=3\n' + "\n".join(patches)
    # Use sasa's original source identity, with its exact revision kept by the
    # copied lock. Adding rev would create a second identity in prpr's graph.
    manifest = manifest.replace(f',rev={json.dumps(sasa_sha)}', '')
    (output / "Cargo.toml").write_text(manifest)
    if not (output / "Cargo.lock").exists():
        shutil.copy2(source / "src-tauri/Cargo.lock", output / "Cargo.lock")
    metadata = {"source_sha256": hashes, "instrumented_source_sha256": sha(output / "src/apple_render.rs"), "source_cpu_readback_sites": readback,
                "settings": {"fps": 60, "sample_count": 4, "seconds": 20, "output_frames": 1200, "sample_rate": 48000, "bitrate": "8M", "disable_loading": True}}
    (output / "source-hashes.json").write_text(json.dumps(metadata, indent=2) + "\n")
    return metadata


def command(args, *, env=None, log=None):
    result = subprocess.run(list(map(str, args)), env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if log:
        Path(log).write_text(result.stdout)
    if result.returncode:
        if str(args[0]) == "cargo":
            # Compiler output concerns public source, not the private fixture.
            print(result.stdout)
        raise RuntimeError(f"{Path(str(args[0])).name} failed with code {result.returncode}; private log retained")
    return result.stdout


def probe(path, frames, width, height):
    data = json.loads(command(["ffprobe", "-v", "error", "-count_frames", "-show_streams", "-show_format", "-of", "json", path]))
    video = next(s for s in data["streams"] if s["codec_type"] == "video")
    audio = next(s for s in data["streams"] if s["codec_type"] == "audio")
    assert video["codec_name"] == "h264" and int(video["nb_read_frames"]) == frames
    assert (video["width"], video["height"]) == (width, height)
    assert abs(float(video["duration"]) - 20) <= 1 / 60
    assert audio["codec_name"] == "aac" and int(audio["sample_rate"]) == 48000
    assert abs(float(audio["duration"]) - 20) <= 4096 / 48000
    return {"video_frames": int(video["nb_read_frames"]), "video_duration": float(video["duration"]), "audio_duration": float(audio["duration"]), "width": width, "height": height}


def run(args):
    if sys_platform() != "Darwin":
        raise SystemExit("Native benchmark must run on a Mac; use prepare for source-only checks")
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    private = output / "private"
    private.mkdir(mode=0o700, exist_ok=True)
    report = {"scope": "Native same-Mac actual pipeline comparison; 20-second window, identical PCM, real H264/AAC decode", "fixture_sha256": sha(args.fixture), "runs": [], "sources": {}}
    root = Path(args.current_source).resolve()
    binaries = {}
    for label in ["baseline", "current"]:
        source, deps = Path(getattr(args, label + "_source")).resolve(), Path(getattr(args, label + "_deps")).resolve()
        harness = private / label
        report["sources"][label] = prepare(source, deps, harness)
        env = os.environ.copy()
        env.pop("PHI_TKC_BENCH_KEY", None)
        env["PRPR_AVC_LIBS"] = str(deps / "Phi-TKC-render-lib/prpr-avc/static-lib")
        env["MACOSX_DEPLOYMENT_TARGET"] = "11.0"
        env["CARGO_TARGET_DIR"] = str(private / "target")
        command(["cargo", "build", "--release", "--features", "apple-render-check", "--manifest-path", harness / "Cargo.toml"], env=env, log=private / (label + "-build.log"))
        binaries[label] = private / (label + "-pipeline")
        shutil.copy2(private / "target/release/apple-pipeline-bench", binaries[label])
    # Registry packages must be identical across the shared graph. Native fork
    # changes are represented by the supplied source snapshots/path patches.
    versions = {}
    for label in binaries:
        lock = tomllib.loads((private / label / "Cargo.lock").read_text())
        versions[label] = {(p["name"], p["version"]) for p in lock["package"] if p.get("source", "").startswith("registry+")}
    assert versions["baseline"] == versions["current"], "Registry dependency versions differ between old/new builds"
    width, height = args.width, args.height
    pcm_hash = None
    first_videos = {}
    # Warm both modes on the exact same 20s workload, then alternate order to
    # reduce shader cache and thermal-order bias. Compilation is outside timing.
    sequence = [("baseline", False), ("current", False)]
    for iteration in range(args.repeat):
        sequence += [(label, True) for label in (["baseline", "current"] if iteration % 2 == 0 else ["current", "baseline"])]
    for index, (label, measured) in enumerate(sequence):
        path = private / f"{index}-{label}.mp4"
        metrics = private / f"{index}-{label}.json"
        env = os.environ.copy(); env.pop("PHI_TKC_BENCH_KEY", None)
        env["PHI_TKC_BENCH_METRICS"] = str(metrics)
        started = time.perf_counter()
        command([binaries[label], args.fixture, path, root / "src-tauri/assets", width, height], env=env, log=private / f"{index}-{label}.log")
        elapsed = time.perf_counter() - started
        data = json.loads(metrics.read_text())
        assert data["frames"] == 1200 and data["pcm_frames"] == 960000 and data["sample_rate"] == 48000
        if pcm_hash is None: pcm_hash = data["pcm_sha256"]
        assert data["pcm_sha256"] == pcm_hash, "Old/new input PCM differs"
        data.update({"mode": label, "measured": measured, "end_to_end_seconds": elapsed, "end_to_end_fps": 1200 / elapsed, "decoded": probe(path, 1200, width, height)})
        if label == "baseline": assert data["cpu_readback_calls"] == 1200
        else: assert data["cpu_readback_calls"] == 0
        report["runs"].append(data)
        first_videos.setdefault(label, path)
        print(f"{label}: frames=1200 render+finish={data['fps']:.2f}fps e2e={data['end_to_end_fps']:.2f}fps measured={measured}")
    # Decode all corresponding chart frames. This catches flipped channels/rows
    # and missing/reordered frames without publishing the chart's pixels.
    comparison = command(["ffmpeg", "-hide_banner", "-i", first_videos["baseline"], "-i", first_videos["current"], "-filter_complex", "[0:v]setpts=PTS-STARTPTS[a];[1:v]setpts=PTS-STARTPTS[b];[a][b]ssim", "-an", "-f", "null", "-"], log=private / "ssim.log")
    match = re.search(r"All:([0-9.]+)", comparison)
    assert match, "No actual decoded-frame SSIM result"
    report["decoded_frame_ssim"] = float(match[1])
    assert report["decoded_frame_ssim"] >= .98, "Old/new chart appearance differs"
    medians = {label: statistics.median(r["fps"] for r in report["runs"] if r["mode"] == label and r["measured"]) for label in binaries}
    report["median_render_fps"] = medians
    report["speedup"] = medians["current"] / medians["baseline"]
    report["host"] = {"system": command(["sw_vers"]).strip(), "cpu": command(["sysctl", "-n", "machdep.cpu.brand_string"]).strip(), "rust": command(["rustc", "--version"]).strip()}
    (output / "pipeline-report.json").write_text(json.dumps(report, indent=2) + "\n")
    assert medians["current"] >= medians["baseline"] / 1.05, "New pipeline regressed by more than 5%"
    return report


def sys_platform():
    import platform
    return platform.system()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_subparsers(dest="mode", required=True)
    prepare_parser = modes.add_parser("prepare")
    for name in ["source", "deps", "output"]: prepare_parser.add_argument("--" + name, required=True)
    run_parser = modes.add_parser("run")
    for name in ["baseline-source", "baseline-deps", "current-source", "current-deps", "fixture", "output"]: run_parser.add_argument("--" + name, required=True)
    run_parser.add_argument("--width", type=int, default=1920)
    run_parser.add_argument("--height", type=int, default=1080)
    run_parser.add_argument("--repeat", type=int, default=3)
    args = parser.parse_args()
    if args.mode == "prepare":
        print(json.dumps(prepare(args.source, args.deps, args.output), indent=2))
    else:
        if args.repeat < 2: parser.error("Use at least two alternating measurement pairs")
        run(args)
