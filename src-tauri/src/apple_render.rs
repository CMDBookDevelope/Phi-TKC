use crate::{
    apple_frames::{initialize_render_target, MetalFrames},
    apple_lifecycle::RenderActivity,
    apple_media::Encoder,
    render::{build_player, RenderParams},
};
use anyhow::{bail, Context, Result};
use log::info;
use macroquad::{
    prelude::*,
    window::{miniquad_context, Conf},
};
use prpr::{
    config::Mods,
    core::{MSRenderTarget, RhythmComposite, RhythmNoteSelection, RhythmStrip},
    fs,
    scene::{GameMode, GameScene, LoadingScene},
    time::TimeManager,
    ui::{FontArc, TextPainter},
    Main,
};
use sasa::AudioClip;
use std::{
    cell::RefCell,
    ops::DerefMut,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

const SAMPLE_RATE: usize = 44100;

pub fn build_conf(width: u32, height: u32) -> Conf {
    Conf {
        window_title: "Phi-TK".to_string(),
        window_width: width as i32,
        window_height: height as i32,
        headless: true,
        platform: macroquad::miniquad::conf::Platform {
            rendering_backend: macroquad::miniquad::conf::RenderingBackend::Metal,
            ..Default::default()
        },
        ..Default::default()
    }
}

pub struct RenderRequest {
    pub params: RenderParams,
    pub output_path: PathBuf,
    pub asset_dir: PathBuf,
    pub request_cancel: Option<Arc<AtomicBool>>,
}

pub fn run(request: RenderRequest, on_progress: impl Fn(f32, f64) + 'static) -> Result<crate::common::RenderStats> {
    crate::apple_platform::install_logger();
    let _log = crate::apple_platform::start_render_log(request.output_path.with_extension("log"))?;
    let mut request = request;
    normalize_config(&mut request.params.config)?;
    let (vw, vh) = request.params.config.output_resolution()?;
    crate::apple_platform::debug_log(&format!("render: using requested {vw}x{vh}"));
    crate::apple_platform::debug_log(&format!("render: assets={}", request.asset_dir.display()));
    let conf = build_conf(vw, vh);
    let activity = RenderActivity::begin();
    activity.checkpoint(|| check_canceled(&request.request_cancel), || Ok(()))?;

    let shared: std::sync::Arc<std::sync::Mutex<Option<Result<crate::common::RenderStats>>>> =
        std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot = std::sync::Arc::clone(&shared);
    // Tauri owns UIKit; the renderer creates only an offscreen Metal context.
    objc::rc::autoreleasepool(|| {
        macroquad::Window::from_config(conf, async move {
            let _resources = crate::common::NativeRenderResources;
            set_pc_assets_folder(&request.asset_dir.display().to_string());
            let result = render_async(request, activity, vw, vh, on_progress).await;
            if let Err(error) = &result {
                log::error!("Render failed: {error:?}");
            }
            if let Ok(mut guard) = slot.lock() {
                *guard = Some(result);
            }
            macroquad::window::miniquad_context().request_quit();
        })
    });
    shared
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
        .unwrap_or_else(|| Err(anyhow::anyhow!("render thread did not report a result")))
}

async fn render_async(
    request: RenderRequest,
    activity: RenderActivity,
    vw: u32,
    vh: u32,
    on_progress: impl Fn(f32, f64) + 'static,
) -> Result<crate::common::RenderStats> {
    let process_start = Instant::now();
    let mut phase = process_start;
    macro_rules! mark {
        ($name:literal) => {{
            let elapsed = phase.elapsed().as_secs_f64();
            info!("setup {}: {:.3}s", $name, elapsed);
            phase = Instant::now();
            elapsed
        }};
    }
    let check_cancel = || check_canceled(&request.request_cancel);
    activity.checkpoint(check_cancel, finish_gpu)?;
    let params = request.params;
    let (game_width, game_height) = params.config.canvas_resolution()?;
    let path = params.path.clone();
    let font = FontArc::try_from_vec(load_file("font.ttf").await?)?;
    let mut painter = TextPainter::new(font);

    let mut config = params.config.to_config();
    config.mods = Mods::AUTOPLAY;
    config.interactive = false;

    let mut info = params.info.clone();
    let mut chart_fs = fs::fs_from_file(&path)?;
    crate::apple_platform::debug_log(&format!(
        "render: path={} chart={:?} music={:?} illustration={:?}",
        path.display(),
        info.chart,
        info.music,
        info.illustration
    ));
    fs::fix_info(chart_fs.deref_mut(), &mut info)
        .await
        .context("failed to infer the chart files")?;
    crate::apple_platform::debug_log(&format!(
        "render: fixed chart={:?} music={:?} illustration={:?}",
        info.chart, info.music, info.illustration
    ));
    let (chart, ..) = GameScene::load_chart(chart_fs.deref_mut(), &info)
        .await
        .context("failed to load the chart")?;
    crate::apple_platform::debug_log("render: chart loaded");
    let music = AudioClip::new(
        chart_fs
            .load_file(&info.music)
            .await
            .with_context(|| format!("failed to read music {:?}", info.music))?,
    )
    .context("failed to decode the chart music")?;
    crate::apple_platform::debug_log("render: music decoded");
    let rhythm_strip = if params.config.rhythm_analysis {
        let cover = chart_fs.load_file(&info.illustration).await?;
        Some(RhythmStrip::from_chart_bytes_with_audio(
            &chart, RhythmNoteSelection::from(&config), &cover, &music, config.rhythm_auto_bpm,
            chart.offset + config.offset + info.offset,
        )?.with_speed(config.rhythm_speed).with_colors(config.rhythm_colors.as_ref()))
    } else { None };
    macro_rules! ld {
        ($path:literal) => {
            AudioClip::new(load_file($path).await?).context(concat!("failed to load ", $path))?
        };
    }
    let ending = ld!("ending.mp3");
    let sfx_click = ld!("click.ogg");
    let sfx_drag = ld!("drag.ogg");
    let sfx_flick = ld!("flick.ogg");

    mark!("chart + audio load");
    let volume_music = config.volume_music;
    let volume_sfx = config.volume_sfx;
    config.volume_music = 0.;
    config.volume_sfx = 0.;

    let track_length = music.length() as f64;
    const O: f64 = LoadingScene::TOTAL_TIME as f64 + GameScene::BEFORE_TIME as f64;
    const A: f64 = 1.0;
    let length = track_length - chart.offset.min(0.) as f64 + 1.;
    let video_length = O + length + A + params.config.ending_length;
    let offset = chart.offset.max(0.);

    let fps = params.config.fps.max(1);
    crate::apple_platform::debug_log(&format!("render: fps={fps}"));
    let fps_f64 = fps as f64;
    let frame_duration = 1.0 / fps_f64;
    let audio_delay = params.config.audio_delay_frames as f64 * frame_duration;

    crate::apple_platform::debug_log(&format!(
        "render: track_length={track_length:.3} chart_offset={:.3} length={length:.3} O={O:.3} A={A} ending={:.3} video_length={video_length:.3}",
        chart.offset, params.config.ending_length
    ));
    if !video_length.is_finite() || video_length <= 0. {
        bail!("render duration must be positive and finite");
    }
    let audio_buffer_length = video_length + audio_delay.abs();
    let mut output = vec![0.0_f32; (audio_buffer_length * SAMPLE_RATE as f64).ceil() as usize * 2];

    let ratio = 1.0 / SAMPLE_RATE as f64;
    let music_start = Instant::now();
    if volume_music != 0.0 {
        let pos = O - chart.offset.min(0.) as f64 + audio_delay;
        let start_index = (pos * SAMPLE_RATE as f64).round() as usize * 2;
        if start_index < output.len() {
            let output_ptr = output.as_mut_ptr();
            let max_i = (output.len() - start_index) / 2;
            let effective = ((music.length() as f64 * SAMPLE_RATE as f64) as usize).min(max_i);
            let mut time = 0.0_f64;
            for i in 0..effective {
                let frame = music.sample(time as f32).unwrap_or_default();
                unsafe {
                    let idx = start_index + i * 2;
                    *output_ptr.add(idx) += frame.0 * volume_music;
                    *output_ptr.add(idx + 1) += frame.1 * volume_music;
                }
                time += ratio;
            }
        }
    }

    info!("music Time:{:?}", music_start.elapsed());
    let mut place = |pos: f64, clip: &AudioClip, volume: f32| {
        let position = (pos * SAMPLE_RATE as f64).round() as usize * 2;
        if position >= output.len() {
            return 0;
        }
        let len = clip.frame_count();
        let output_len = output.len() - position;
        let valid = (output_len / 2).min(len);
        let output_ptr = unsafe { output.as_mut_ptr().add(position) };
        let frames_ptr = clip.frames().as_ptr();
        for i in 0..valid {
            unsafe {
                let sample = (*frames_ptr.add(i)).0 * volume;
                *output_ptr.add(i * 2) += sample;
                *output_ptr.add(i * 2 + 1) += sample;
            }
        }
        valid
    };

    let sfx_start = Instant::now();
    if volume_sfx != 0.0 {
        let o_offset = O + offset as f64 + audio_delay;
        let sfx = [
            &sfx_click as *const _,
            &sfx_drag as *const _,
            &sfx_click as *const _,
            &sfx_flick as *const _,
        ];
        unsafe {
            for line in chart.lines.iter() {
                for note in line.notes.iter() {
                    if !note.above {
                        continue;
                    }
                    let clip = &*sfx[note.kind.order() as usize];
                    place(o_offset + note.time as f64, clip, volume_sfx);
                }
            }
        }
    }

    info!("sfx Time:{:?}", sfx_start.elapsed());
    let mut pos = O + length + A + audio_delay;
    info!("Ending music start: {:.6}s", pos);
    if params.config.ending_length > 0.1 {
        while place(pos, &ending, volume_music) != 0 {
            pos += ending.length() as f64;
        }
    }
    mark!("mixing");
    activity.checkpoint(check_cancel, finish_gpu)?;

    let sample_rate = params.config.target_audio.max(8000) as usize;
    let pcm = resample_pcm(&output, SAMPLE_RATE, sample_rate);
    let pcm16 = pcm_f32_to_i16(&pcm);
    info!(
        "audio ready: {} samples @ {} Hz ({:.1}s)",
        pcm.len() / 2,
        sample_rate,
        pcm.len() as f64 / 2. / sample_rate as f64
    );
    drop(pcm);
    drop(output);

    let ss = if params.config.disable_loading {
        O
    } else {
        0.1
    };
    let ss_samples = (ss * sample_rate as f64).round() as usize * 2;
    let start_bytes = (ss_samples * 2).min(pcm16.len());
    let end_bytes = (((video_length * sample_rate as f64) as usize) * 4).min(pcm16.len());
    let pcm16 = &pcm16[start_bytes..end_bytes.max(start_bytes)];
    crate::apple_platform::debug_log(&format!(
        "audio: skipped {ss:.3}s, kept {} frames ({:.3}s) from {:.3}s",
        pcm16.len() / 4,
        pcm16.len() as f64 / 4. / sample_rate as f64,
        start_bytes as f64 / 4. / sample_rate as f64
    ));

    let bitrate = bitrate_bps(&params.config)?;
    activity.checkpoint(check_cancel, finish_gpu)?;
    let mut encoder = Encoder::new(
        &request.output_path,
        vw,
        vh,
        fps,
        bitrate,
        sample_rate as u32,
    )?;
    let target = Rc::new(MSRenderTarget::new((game_width, game_height), params.config.sample_count));
    let rhythm = RhythmComposite::new(game_width, game_height, rhythm_strip, 1)?;
    initialize_render_target(
        miniquad_context()
            .metal_context()
            .context("Metal context is unavailable")?,
        target.input().render_pass,
    );
    let my_time = Rc::new(RefCell::new(0.));
    let tm = TimeManager::manual(Box::new({
        let clock = Rc::clone(&my_time);
        move || *clock.borrow()
    }));
    let player = build_player(&params.config).await?;
    let mut main = Main::new(
        Box::new(
            LoadingScene::new(
                GameMode::Normal,
                info.clone(),
                config,
                chart_fs,
                Some(player),
                None,
                None,
            )
            .await?,
        ),
        tm,
        {
            let target = Rc::clone(&target);
            move || Some(target.input())
        },
    )
    .await?;
    main.top_level = false;
    main.viewport = Some((0, 0, game_width as i32, game_height as i32));
    main.update()?;
    let skip = (ss * fps_f64 - 1e-6).ceil().max(0.) as u64;
    let frames = (video_length * fps_f64).ceil() as u64;
    let output_frames = frames.saturating_sub(skip);
    if output_frames == 0 {
        bail!("render duration contains no output frames");
    }
    let mut gpu_frames = MetalFrames::new(
        miniquad_context()
            .metal_context()
            .context("Metal context is unavailable")?,
        vw,
        vh,
    )?;
    let mut fps_timer = Instant::now();
    let mut realtime_fps = 0.;
    for frame in 0..frames {
        objc::rc::autoreleasepool(|| -> Result<()> {
            if prepare_encoder(
                &activity,
                &mut gpu_frames,
                &mut encoder,
                pcm16,
                check_cancel,
            )? {
                fps_timer = Instant::now();
            }
            *my_time.borrow_mut() = frame as f64 * frame_duration;
            if frame != 0 {
                main.update()?;
            }
            if frame < skip {
                return Ok(());
            }
            unsafe { get_internal_gl() }
                .quad_gl
                .render_pass(Some(target.input().render_pass));
            main.render(&mut painter)?;
            let rendered = rhythm.compose(0, &target,
                main.scenes.last().and_then(|scene| scene.chart_time()), &mut painter);
            unsafe { get_internal_gl() }.flush();
            let texture = rendered.texture.raw_miniquad_texture_handle();
            let output_index = frame - skip;
            gpu_frames.drain_ready(&mut encoder, pcm16)?;
            if gpu_frames.at_capacity() {
                gpu_frames.drain_one(&mut encoder, pcm16)?;
            }
            // The pool bounds both queues. Drain a completed GPU lease before
            // acquiring the next one; VT owns every surface after frame_pixel.
            let pixel = loop {
                if prepare_encoder(
                    &activity,
                    &mut gpu_frames,
                    &mut encoder,
                    pcm16,
                    check_cancel,
                )? {
                    fps_timer = Instant::now();
                }
                match encoder.pixel_buffer(pcm16) {
                    Ok(pixel) => break pixel,
                    Err(_) if encoder.needs_resume()? => continue,
                    Err(error) => return Err(error),
                }
            };
            gpu_frames
                .enqueue(
                    miniquad_context()
                        .metal_context()
                        .context("Metal context is unavailable")?,
                    texture,
                    pixel,
                    output_index,
                )
                .with_context(|| {
                    format!(
                        "Metal video frame {output_index}/{output_frames}, {vw}x{vh} @ {fps} fps"
                    )
                })?;
            if output_index % 10 == 9 {
                realtime_fps = 10. / fps_timer.elapsed().as_secs_f64().max(1e-9);
                fps_timer = Instant::now();
            }
            on_progress(
                (output_index + 1) as f32 / output_frames as f32,
                realtime_fps,
            );
            Ok(())
        })?;
    }
    loop {
        prepare_encoder(
            &activity,
            &mut gpu_frames,
            &mut encoder,
            pcm16,
            check_cancel,
        )?;
        gpu_frames.drain_all(&mut encoder, pcm16)?;
        if encoder
            .try_finish(pcm16)
            .context("Apple media finalization")?
        {
            break;
        }
    }
    info!(
        "Metal render complete: {output_frames} frames in {:.2}s",
        process_start.elapsed().as_secs_f64()
    );
    Ok(crate::common::RenderStats {
        frames: output_frames,
        duration: process_start.elapsed().as_secs_f64(),
    })
}

fn check_canceled(cancel: &Option<Arc<AtomicBool>>) -> Result<()> {
    if cancel
        .as_ref()
        .is_some_and(|value| value.load(Ordering::Relaxed))
    {
        bail!("render canceled");
    }
    Ok(())
}

fn finish_gpu() -> Result<()> {
    unsafe { get_internal_gl() }.flush();
    miniquad_context()
        .metal_context()
        .context("Metal context is unavailable")?
        .finish();
    Ok(())
}

fn prepare_encoder(
    activity: &RenderActivity,
    frames: &mut MetalFrames,
    encoder: &mut Encoder,
    pcm16: &[u8],
    check_cancel: impl Fn() -> Result<()> + Copy,
) -> Result<bool> {
    let mut resumed = false;
    let mut retry_since = Instant::now();
    loop {
        if activity.checkpoint(check_cancel, || {
            frames.wait_all()?;
            finish_gpu()?;
            encoder.suspend(pcm16)?;
            frames.defer_all(encoder, pcm16)
        })? {
            resumed = true;
            retry_since = Instant::now();
        }
        if !encoder.needs_resume()? {
            return Ok(resumed);
        }
        frames.wait_all()?;
        finish_gpu()?;
        encoder.suspend(pcm16)?;
        frames.defer_all(encoder, pcm16)?;
        encoder.resume(pcm16)?;
        if !encoder.needs_resume()? {
            return Ok(true);
        }
        // Activation and VideoToolbox availability can arrive separately. A
        // foreground retry never advances the chart or its output frame index.
        check_cancel()?;
        anyhow::ensure!(
            retry_since.elapsed().as_secs() < 30,
            "VideoToolbox session is still unavailable in the foreground"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn pcm_f32_to_i16(input: &[f32]) -> Vec<u8> {
    input
        .iter()
        .flat_map(|&value| ((value.clamp(-1., 1.) * 32767.).round() as i16).to_le_bytes())
        .collect()
}

fn resample_pcm(input: &[f32], from: usize, to: usize) -> Vec<f32> {
    if from == to {
        return input.to_vec();
    }
    let frames = input.len() / 2;
    let ratio = from as f64 / to as f64;
    let out_frames = (frames as f64 / ratio) as usize;
    let mut out = vec![0.0f32; out_frames * 2];
    for index in 0..out_frames {
        let position = index as f64 * ratio;
        let base = position.floor() as usize;
        let frac = (position - base as f64) as f32;
        let next = (base + 1).min(frames.saturating_sub(1));
        for channel in 0..2 {
            let a = input.get(base * 2 + channel).copied().unwrap_or(0.);
            let b = input.get(next * 2 + channel).copied().unwrap_or(0.);
            out[index * 2 + channel] = a + (b - a) * frac;
        }
    }
    out
}
fn normalize_config(config: &mut crate::render::RenderConfig) -> Result<()> {
    if config.bitrate.trim().is_empty() || config.bitrate_control.eq_ignore_ascii_case("CRF") {
        config.bitrate = "8M".to_owned();
    }
    config.bitrate_control = "CBR".to_owned();
    config.target_audio = 48_000;
    config.video_codec = "h264".to_owned();
    bitrate_bps(config)?;
    Ok(())
}

fn bitrate_bps(config: &crate::render::RenderConfig) -> Result<u32> {
    let value = parse_bitrate_bps(&config.bitrate).context("invalid target bitrate")?;
    if !(200_000.0..=i32::MAX as f64).contains(&value) {
        bail!("target bitrate must be between 200000 and {} bps", i32::MAX);
    }
    Ok(value.round() as u32)
}

fn parse_bitrate_bps(raw: &str) -> Option<f64> {
    let s = raw.trim().to_ascii_lowercase().replace(' ', "");
    if s.is_empty() {
        return None;
    }
    let (num, mult) = if let Some(v) = s.strip_suffix("kbps") {
        (v, 1e3)
    } else if let Some(v) = s.strip_suffix("mbps") {
        (v, 1e6)
    } else if let Some(v) = s.strip_suffix("bps") {
        (v, 1.0)
    } else if let Some(v) = s.strip_suffix('k') {
        (v, 1e3)
    } else if let Some(v) = s.strip_suffix('m') {
        (v, 1e6)
    } else {
        (s.as_str(), 1.0)
    };
    if num.is_empty()
        || num.bytes().filter(|&value| value == b'.').count() > 1
        || !num
            .bytes()
            .all(|value| value.is_ascii_digit() || value == b'.')
        || !num.as_bytes().first()?.is_ascii_digit()
        || !num.as_bytes().last()?.is_ascii_digit()
    {
        return None;
    }
    let num: f64 = num.parse().ok()?;
    (num.is_finite() && num > 0.0 && (num * mult).is_finite()).then_some(num * mult)
}
