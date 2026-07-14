// Prevents additional console window on Windows in release, DO NOT REMOVE!!
prpr::tl_file!("render");

use anyhow::{bail, Context, Result};
use macroquad::{
    miniquad::{
        gl::{
            glBindBuffer, glBindFramebuffer, glBufferData, glDeleteBuffers, glGenBuffers,
            glMapBufferRange, glReadBuffer, glReadPixels, glUnmapBuffer, glViewport, GLsizei,
            GLsizeiptr, GLuint, GLvoid, GL_COLOR_ATTACHMENT0, GL_FRAMEBUFFER, GL_MAP_READ_BIT,
            GL_MAP_UNSYNCHRONIZED_BIT, GL_PIXEL_PACK_BUFFER, GL_READ_FRAMEBUFFER, GL_RGBA,
            GL_STREAM_READ, GL_UNSIGNED_BYTE,
        },
        RenderPass as MQRenderPass,
    },
    prelude::*,
};
use std::sync::Once;
use prpr::{
    config::{ChallengeModeColor, Config, Mods},
    core::{internal_id, MSRenderTarget, NoteKind},
    ext::SafeTexture,
    fs,
    info::ChartInfo,
    scene::{BasicPlayer, GameMode, GameScene, LoadingScene},
    time::TimeManager,
    ui::{FontArc, TextPainter},
    Main,
};
use sasa::AudioClip;
use indicatif::{ProgressBar, ProgressStyle, ProgressDrawTarget};
use log::{Level, LevelFilter, info, warn, error, debug, trace};
use env_logger::{Builder, Target, Env};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::{
    cell::RefCell,
    ops::DerefMut,
    io::{BufWriter, Write},
    path::PathBuf,
    process::{Command, Stdio},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use term_size::dimensions;

fn get_term_width() -> usize {
    dimensions().map(|(w, _)| w).unwrap_or(80)
}

use std::{ffi::OsStr, fmt::Write as _};
use tempfile::NamedTempFile;

static INIT: Once = Once::new();

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RenderConfig {
    pub codec: String,
    pub resolution: (u32, u32),
    pub ffmpeg_preset: String,
    pub ending_length: f64,
    pub disable_loading: bool,
    pub audio_delay_frames: i32,
    pub chart_debug: bool,
    pub flid_x: bool,
    pub chart_ratio: f32,
    pub buffer_size: f32,
    pub combo: String,
    pub fps: u32,
    pub hardware_accel: bool,
    pub video_codec: String,
    pub encoder: String, // 'auto', 'nvenc', 'qsv', 'amf', 'vulkan', 'cpu'
    pub show_progress_text: bool,
    pub show_time_text: bool,
    pub target_audio: u32,
    pub autoplay: Option<bool>,
    pub bitrate_control: String,
    pub bitrate: String,
    pub watermark: String,
    pub background: bool,
    pub video: bool,
    pub audio_bit: Option<u32>,
    pub audio_format: String,

    pub aggressive: bool,
    pub challenge_color: ChallengeModeColor,
    pub challenge_rank: u32,
    pub disable_effect: bool,
    pub double_hint: bool,
    pub fxaa: bool,
    pub note_scale: f32,
    pub particle: bool,
    pub player_avatar: Option<String>,
    pub player_name: String,
    pub player_rks: f32,
    pub sample_count: u32,
    pub res_pack_path: Option<String>,
    pub speed: f32,
    pub volume_music: f32,
    pub volume_sfx: f32,

    pub hand_split: bool,
    pub note_speed_factor: f32,
    pub bar: bool,

    pub ui_score: bool,
    pub ui_combo: bool,
    pub ui_name: bool,
    pub ui_level: bool,
    pub ui_line: bool,
    pub ui_pb: bool,
    pub ui_pause: bool,

    //ffmpeg
    pub ffmpeg_thread: bool,
}

fn stderr_for_loglevel() -> Stdio {
    if log::max_level() >= LevelFilter::Debug {
        Stdio::inherit()
    } else {
        Stdio::null()
    }
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            codec: "hevc_qsv".to_string(),
            resolution: (1920, 1080),
            ffmpeg_preset: "medium p4 balanced".to_string(),
            ending_length: -2.0,
            disable_loading: true,
            chart_debug: false,
            audio_delay_frames: 4,
            flid_x: false,
            chart_ratio: 1.0,
            buffer_size: 256.0,
            combo: "Phi-TK POWERED".to_string(),
            fps: 30,
            hardware_accel: false,
            video_codec: "h264".to_string(),
            encoder: "qsv".to_string(),
            show_progress_text: false,
            show_time_text: false,
            target_audio: 44100,
            autoplay: None,
            bitrate_control: "CRF".to_string(),
            bitrate: "28".to_string(),
            watermark: "".to_string(),
            background: false,
            aggressive: false,
            challenge_color: ChallengeModeColor::Golden,
            challenge_rank: 45,
            disable_effect: false,
            double_hint: true,
            fxaa: false,
            note_scale: 1.0,
            particle: true,
            player_avatar: None,
            player_name: "Link/Mivik".to_string(),
            player_rks: 17.0,
            sample_count: 1,
            res_pack_path: None,
            speed: 1.0,
            volume_music: 1.0,
            volume_sfx: 1.0,
            hand_split: false,
            note_speed_factor: 1.0,
            video: false,
            audio_bit: None,
            audio_format: "mp3".to_string(),
            ui_score: true,
            ui_combo: true,
            ui_name: false,
            ui_level: false,
            ui_line: true,
            ui_pb: true,
            ui_pause: true,
            bar: false,
            ffmpeg_thread: false,
        }
    }
}

impl RenderConfig {
    pub fn to_config(&self) -> Config {
        Config {
            aggressive: self.aggressive,
            challenge_color: self.challenge_color.clone(),
            challenge_rank: self.challenge_rank,
            disable_effect: self.disable_effect,
            double_hint: self.double_hint,
            fxaa: self.fxaa,
            note_scale: self.note_scale,
            particle: self.particle,
            player_name: self.player_name.clone(),
            player_rks: self.player_rks,
            sample_count: self.sample_count,
            res_pack_path: self.res_pack_path.clone(),
            speed: self.speed,
            volume_music: self.volume_music,
            volume_sfx: self.volume_sfx,
            chart_debug: self.chart_debug,
            chart_ratio: self.chart_ratio,
            buffer_size: self.buffer_size,
            combo: self.combo.clone(),
            flid_x: self.flid_x,
            show_progress_text: self.show_progress_text,
            show_time_text: self.show_time_text,
            autoplay: self.autoplay,
            watermark: self.watermark.clone(),
            background: self.background.clone(),
            disable_loading: self.disable_loading,
            hand_split: self.hand_split,
            note_speed_factor: self.note_speed_factor,
            ui_score: self.ui_score,
            ui_combo: self.ui_combo,
            ui_name: self.ui_name,
            ui_level: self.ui_level,
            ui_line: self.ui_line,
            ui_pb: self.ui_pb,
            ui_pause: self.ui_pause,
            bar: self.bar,
            ..Default::default()
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderParams {
    pub path: PathBuf,
    pub info: ChartInfo,
    pub config: RenderConfig,
}

#[derive(Serialize, Deserialize)]
pub enum IPCEvent {
    StartMixing,
    StartRender(u64),
    Frame,
    Done(f64),
}

struct EncoderAvailability {
    h264_nvenc: bool,
    hevc_nvenc: bool,
    h264_qsv: bool,
    hevc_qsv: bool,
    h264_amf: bool,
    hevc_amf: bool,
    av1_nvenc: bool,
    av1_amf: bool,
    av1_qsv: bool,
    h264_cuvid: bool,
    hevc_cuvid: bool,
    av1_cuvid: bool,
    h264_vulkan: bool,
    hevc_vulkan: bool,
    av1_vulkan: bool,
}

#[cfg(target_os = "windows")]
mod hw_detect {
    use std::path::Path;
    use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};

    pub fn detect_nvidia() -> bool {
        use std::process::Command;
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(
            r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}",
        ) {
            for subkey_name in key.enum_keys().filter_map(|x| x.ok()) {
                if let Ok(subkey) = key.open_subkey(&subkey_name) {
                    if let Ok(provider) = subkey.get_value::<String, _>("ProviderName") {
                        if provider.to_lowercase().contains("nvidia") {
                            return true;
                        }
                    }
                }
            }
        }
        if Path::new(r"C:\Windows\System32\nvcuda.dll").exists() {
            return true;
        }
        Command::new("nvidia-smi").output().is_ok()
    }

    pub fn detect_intel_qsv() -> bool {
        let mut found = false;
        let classes = [
            "{4d36e968-e325-11ce-bfc1-08002be10318}",
            "{4d36e97d-e325-11ce-bfc1-08002be10318}",
        ];
        for class in classes {
            if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
                .open_subkey(format!(r"SYSTEM\CurrentControlSet\Control\Class\{}", class))
            {
                for subkey in key.enum_keys().filter_map(|x| x.ok()) {
                    if let Ok(subkey) = key.open_subkey(subkey) {
                        if let Ok(provider) = subkey.get_value::<String, _>("ProviderName") {
                            if provider.contains("Intel") {
                                found = true;
                                break;
                            }
                        }
                    }
                }
            }
        }
        found
    }

    pub fn detect_amd() -> bool {
        Path::new(r"C:\Windows\System32\amdvlk64.dll").exists()
            || Path::new(r"C:\Windows\System32\amfrt64.dll").exists()
    }

    pub fn detect_vulkan() -> bool {
        Path::new(r"C:\Windows\System32\vulkan-1.dll").exists()
    }
}

#[cfg(target_os = "linux")]
mod hw_detect {
    use std::path::Path;
    use std::process::Command;

    pub fn detect_nvidia() -> bool {
        Path::new("/dev/nvidia0").exists() || Command::new("nvidia-smi").status().is_ok()
    }

    pub fn detect_intel_qsv() -> bool {
        true
    }

    pub fn detect_amd() -> bool {
        Path::new("/dev/kfd").exists()
            && Command::new("vainfo")
                .output()
                .map(|out| String::from_utf8_lossy(&out.stdout).contains("AMD"))
                .unwrap_or(false)
    }

    pub fn detect_vulkan() -> bool {
        Path::new("/usr/share/vulkan/icd.d").exists()
            || Path::new("/etc/vulkan/icd.d").exists()
            || Path::new("/usr/local/share/vulkan/icd.d").exists()
    }
}

#[cfg(target_os = "macos")]
mod hw_detect {
    use std::process::Command;

    pub fn detect_nvidia() -> bool {
        Command::new("system_profiler")
            .args(&["SPDisplaysDataType"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains("NVIDIA"))
            .unwrap_or(false)
    }

    pub fn detect_intel_qsv() -> bool {
        Command::new("system_profiler")
            .args(&["SPDisplaysDataType"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains("Intel"))
            .unwrap_or(false)
    }

    pub fn detect_amd() -> bool {
        Command::new("system_profiler")
            .args(&["SPDisplaysDataType"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains("AMD"))
            .unwrap_or(false)
    }

    pub fn detect_vulkan() -> bool {
        Command::new("sh")
            .arg("-c")
            .arg("ls /usr/local/lib/libMoltenVK.dylib 2>/dev/null || ls /opt/homebrew/lib/libMoltenVK.dylib 2>/dev/null || ls ~/Library/Frameworks/libMoltenVK.dylib 2>/dev/null")
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

pub async fn build_player(config: &RenderConfig) -> Result<BasicPlayer> {
    Ok(BasicPlayer {
        avatar: if let Some(path) = &config.player_avatar {
            Some(SafeTexture::from(Texture2D::from_file_with_format(
                &tokio::fs::read(path)
                    .await
                    .with_context(|| tl!("load-avatar-failed"))?,
                None,
            )))
        } else {
            None
        },
        id: 0,
        rks: config.player_rks,
    })
}

pub fn cmd_hidden(program: impl AsRef<OsStr>) -> Command {
    let cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let mut cmd = cmd;
        cmd.creation_flags(0x08000000);
        cmd
    }
    #[cfg(not(target_os = "windows"))]
    cmd
}

pub fn find_ffmpeg() -> Result<Option<String>> {
    fn test(path: impl AsRef<OsStr>) -> bool {
        matches!(cmd_hidden(path).arg("-version").output(), Ok(_))
    }

    let ffmpeg_exe = if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };

    let exe_dir = std::env::current_exe()?
        .parent()
        .expect("Executable should have parent directory")
        .to_owned();
    let bundled_ffmpeg = exe_dir.join(ffmpeg_exe);
    if test(&bundled_ffmpeg) {
        return Ok(Some(bundled_ffmpeg.to_string_lossy().into_owned()));
    }

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(ffmpeg_exe);
            if test(&candidate) {
                return Ok(Some(candidate.to_string_lossy().into_owned()));
            }
        }
    }

    warn!("Failed to find global ffmpeg. Using bundled ffmpeg");
    Ok(if test(&bundled_ffmpeg) {
        Some(bundled_ffmpeg.to_string_lossy().into_owned())
    } else {
        None
    })
}

// ⚠️ 原 render_clionly.rs 中的 pub async fn main() -> Result<()> 已被移除，不再重复定义

pub async fn main_with_params(params: RenderParams, output_path: PathBuf) -> Result<()> {
    init_colored_logger();
    welcome_logger();
    unsafe{
        std::env::set_var("EGL_PLATFORM", "surfaceless");
        std::env::remove_var("DISPLAY");
    }
    
    let path = params.path;
    let mut fs = fs::fs_from_file(&path)?;
    let font = FontArc::try_from_vec(load_file("font.ttf").await?)?;
    let Some(ffmpeg) = find_ffmpeg()? else {
        error!("FFmpeg not found");
        return Err(anyhow::anyhow!("FFmpeg not found"));
    };
    debug!("Using ffmpeg: {}", ffmpeg);
    let mut painter = TextPainter::new(font);
    let mut config = params.config.to_config();
    config.mods = Mods::AUTOPLAY;
    let info = params.info;
    let (chart, ..) = prpr::scene::GameScene::load_chart(fs.deref_mut(), &info)
        .await
        .with_context(|| "load-chart-failed")?;
    let music = sasa::AudioClip::new(fs.load_file(&info.music).await?)
        .with_context(|| "load-music-failed")?;
    let ending = sasa::AudioClip::new(macroquad::prelude::load_file("ending.mp3").await?)?;
    let sfx_click = sasa::AudioClip::new(macroquad::prelude::load_file("click.ogg").await?)?;
    let sfx_drag = sasa::AudioClip::new(macroquad::prelude::load_file("drag.ogg").await?)?;
    let sfx_flick = sasa::AudioClip::new(macroquad::prelude::load_file("flick.ogg").await?)?;
    let gl = unsafe { get_internal_gl() };
    let volume_music = std::mem::take(&mut config.volume_music);
    let volume_sfx = std::mem::take(&mut config.volume_sfx);
    let track_length = music.length() as f64;
    let length = track_length - chart.offset.min(0.) as f64 + 1.;
    let video_length = O + length + A + params.config.ending_length;
    let offset = chart.offset.max(0.);
    let render_start_time = Instant::now();

    debug!("IPCEvent::StartMixing");
    let mixing_output = NamedTempFile::new()?;
    let target_sample_rate = params.config.target_audio;
    let sample_rate = 44100;
    let sample_rate_f64 = sample_rate as f64;
    assert_eq!(sample_rate, ending.sample_rate());
    assert_eq!(sample_rate, sfx_click.sample_rate());
    assert_eq!(sample_rate, sfx_drag.sample_rate());
    assert_eq!(sample_rate, sfx_flick.sample_rate());

    let fps_f64 = params.config.fps as f64;
    let frame_duration = 1.0 / fps_f64;
    let audio_delay = params.config.audio_delay_frames as f64 * frame_duration;

    debug!("{}", split_line('=', Some(" Audio/Video Sync Configuration s")));
    debug!("{}", split_line(' ', Some(format!("Audio delay: {} frames", params.config.audio_delay_frames).as_str())));
    debug!("{}", split_line(' ', Some(format!("Audio delay: {:.6} seconds", audio_delay).as_str())));
    debug!("{}",
        split_line(' ',
            Some(
                format!(
                "Frame duration: {:.6}s @ {}fps",
                frame_duration, params.config.fps
                ).as_str()
            )
        )
    );
    debug!("{}",
        split_line(' ',
            Some(
                format!(
                    "Sample delay: {} samples @ {}Hz",
                    (audio_delay * sample_rate_f64).round() as i64, sample_rate
                ).as_str()
            )
        )
    );
    debug!("{}", split_line('=', None));

    let audio_buffer_length = video_length + audio_delay.abs();
    let mut output = vec![0.0_f32; (audio_buffer_length * sample_rate_f64).ceil() as usize * 2];

    if volume_music != 0.0 {
        let start_time = Instant::now();
        let original_pos = O - chart.offset.min(0.) as f64;
        let pos = original_pos + audio_delay;
        debug!(
            "Music mixing: original_pos={:.6}s, delayed_pos={:.6}s",
            original_pos, pos
        );
        let start_index = (pos * sample_rate_f64).round() as usize * 2;
        let ratio = 1.0 / sample_rate_f64;
        if start_index >= output.len() {
            warn!(
                "Music start position {} exceeds output buffer length {}",
                start_index,
                output.len()
            );
        } else {
            let output_ptr = output.as_mut_ptr();
            let max_i = (output.len() - start_index) / 2;
            let effective_count = ((music.length() as f64 * sample_rate_f64) as usize).min(max_i);
            let mut time = 0.0_f64;
            for i in 0..effective_count {
                let frame = music.sample(time as f32).unwrap_or_default();
                let left = frame.0 * volume_music;
                let right = frame.1 * volume_music;
                unsafe {
                    let idx = start_index + i * 2;
                    *output_ptr.add(idx) += left;
                    *output_ptr.add(idx + 1) += right;
                }
                time += ratio;
            }
        }
        debug!("music Time:{:?}", start_time.elapsed());
    }

    let mut place = |pos: f64, clip: &AudioClip, volume: f32| {
        let position = (pos * sample_rate_f64).round() as usize * 2;
        if position >= output.len() {
            return 0;
        }
        let len = clip.frame_count();
        let output_len = output.len() - position;
        let valid_frames = (output_len / 2).min(len);
        let output_ptr = unsafe { output.as_mut_ptr().add(position) };
        let frames_ptr = clip.frames().as_ptr();
        for i in 0..valid_frames {
            unsafe {
                let sample = (*frames_ptr.add(i)).0 * volume;
                *output_ptr.add(i * 2) += sample;
                *output_ptr.add(i * 2 + 1) += sample;
            }
        }
        valid_frames
    };

    if volume_sfx != 0.0 {
        let start_time = Instant::now();
        let offset_f64 = offset as f64;
        let o_offset = O + offset_f64 + audio_delay;
        debug!(
            "SFX mixing: offset={:.6}s (includes {:.6}s delay)",
            o_offset, audio_delay
        );
        let sfx_click_ptr = &sfx_click as *const _;
        let sfx_drag_ptr = &sfx_drag as *const _;
        let sfx_flick_ptr = &sfx_flick as *const _;
        unsafe {
            let lines_ptr = chart.lines.as_ptr();
            let lines_len = chart.lines.len();
            for i in 0..lines_len {
                let line = &*lines_ptr.add(i);
                let notes_ptr = line.notes.as_ptr();
                let notes_len = line.notes.len();
                for j in 0..notes_len {
                    let note = &*notes_ptr.add(j);
                    if !note.fake {
                        let sfx = match note.kind {
                            NoteKind::Click | NoteKind::Hold { .. } => &*sfx_click_ptr,
                            NoteKind::Drag => &*sfx_drag_ptr,
                            NoteKind::Flick => &*sfx_flick_ptr,
                        };
                        let time = o_offset + note.time as f64;
                        place(time, sfx, volume_sfx);
                    }
                }
            }
        }
        debug!("sfx Time:{:?}", start_time.elapsed());
    }

    let mut pos = O + length + A + audio_delay;
    debug!("Ending music start: {:.6}s", pos);
    while place(pos, &ending, volume_music) != 0 && params.config.ending_length > 0.1 {
        pos += ending.frame_count() as f64 / sample_rate_f64;
    }

    let audio_bit = params.config.audio_bit;
    let audio_format = params.config.audio_format.to_lowercase();
    let supported_formats = ["flac", "mp3", "aac", "opus", "wav"];
    if !supported_formats.contains(&audio_format.as_str()) {
        error!(
            "Unsupported audio format: {}. Supported formats are: {}",
            audio_format,
            supported_formats.join(", ")
        );
    }
    if let Some(bit) = audio_bit {
        if ![16, 24, 32].contains(&bit) {
            error!(
                "Invalid audio bit depth: {}. Supported values are 16, 24, 32.",
                bit
            );
            return Err(anyhow::anyhow!("Invalid audio bit depth: {}. Supported values are 16, 24, 32.", bit));
        }
        if audio_format != "wav" {
            return Err(anyhow::anyhow!(
                "PCM audio bit depth requires WAV format, but {} was specified",
                audio_format
            ));
        }
    }

    let (audio_codec, output_format) = if let Some(bit) = audio_bit {
        (format!("pcm_f{}le", bit), "wav".to_string())
    } else {
        match audio_format.as_str() {
            "flac" => ("flac".to_string(), "flac".to_string()),
            "mp3" => ("libmp3lame".to_string(), "mp3".to_string()),
            "aac" => ("aac".to_string(), "mp4".to_string()),
            "opus" => ("libopus".to_string(), "opus".to_string()),
            "wav" => ("pcm_f16le".to_string(), "wav".to_string()),
            _ => {
                error!(
                    "Unknown audio format '{}', using AAC/MP4 as default",
                    audio_format
                );
                ("aac".to_string(), "mp4".to_string())
            }
        }
    };

    let args_str = if target_sample_rate != sample_rate {
        let resample_filter = format!(
            "aresample=resampler=soxr:precision=33:osr={}:dither_method=triangular",
            target_sample_rate
        );
        format!(
            "-y -f f32le -ar {} -ac 2 -i - -af {} -c:a {} -f {}",
            sample_rate, resample_filter, audio_codec, output_format
        )
    } else {
        format!(
            "-y -f f32le -ar {} -ac 2 -i - -c:a {} -f {}",
            sample_rate, audio_codec, output_format
        )
    };
        
    let mut proc = cmd_hidden(&ffmpeg)
        .args(args_str.split_whitespace())
        .arg(mixing_output.path())
        .arg("-loglevel")
        .arg("warning")
        .stdin(Stdio::piped())
        .stderr(stderr_for_loglevel())
        .spawn()
        .with_context(|| tl!("run-ffmpeg-failed"))?;
    let input = proc.stdin.as_mut().unwrap();
    let mut writer = BufWriter::new(input);
    for sample in output.into_iter() {
        writer.write_all(&sample.to_le_bytes())?;
    }
    drop(writer);
    proc.wait()?;

    let (vw, vh) = params.config.resolution;
    let mst = Rc::new(MSRenderTarget::new((vw, vh), config.sample_count));
    let my_time: Rc<RefCell<f64>> = Rc::new(RefCell::new(0.));
    let tm = TimeManager::manual(Box::new({
        let my_time = Rc::clone(&my_time);
        move || *(*my_time).borrow()
    }));
    static MSAA: AtomicBool = AtomicBool::new(false);
    let player = build_player(&params.config).await?;
    let mut main = Main::new(
        Box::new(
            LoadingScene::new(GameMode::Normal, info, config, fs, Some(player), None, None).await?,
        ),
        tm,
        {
            let mut cnt = 0;
            let mst = Rc::clone(&mst);
            move || {
                cnt += 1;
                if cnt % 2 == 1 {
                    MSAA.store(true, Ordering::SeqCst);
                    Some(mst.input())
                } else {
                    MSAA.store(false, Ordering::SeqCst);
                    Some(mst.output())
                }
            }
        },
    )
    .await?;
    main.top_level = false;
    main.viewport = Some((0, 0, vw as _, vh as _));

    const O: f64 = LoadingScene::TOTAL_TIME as f64 + GameScene::BEFORE_TIME as f64;
    const A: f64 = 1.0;

    let fps = params.config.fps;
    let frames = (video_length * fps as f64).ceil() as u64;
    let total_frames = frames;
    let draw_target = ProgressDrawTarget::stderr();
    let pb = ProgressBar::with_draw_target(Some(total_frames), draw_target);
    pb.set_style(
        ProgressStyle::default_bar()
            .progress_chars("#>_"),
    );
    debug!("progress bar successfully created");
    log::info!("IPCEvent::StartRender({})", frames);

    let test_encoder = |ffmpeg: &Path, encoder: &str| -> Result<(bool, String)> {
        let mut cmd = Command::new(ffmpeg);
        if encoder.ends_with("_vulkan") {
            cmd.args(&[
                "-init_hw_device",
                "vulkan=vk",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=0.1:size=320x240:rate=30",
                "-filter_hw_device",
                "vk",
                "-vf",
                "format=nv12,hwupload",
                "-c:v",
                encoder,
                "-f",
                "null",
                "-",
            ]);
        } else {
            cmd.args(&[
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=320x240:d=0",
                "-c:v",
                encoder,
                "-f",
                "null",
                "-",
            ]);
        }
        cmd.arg("-loglevel")
            .arg("warning")
            .arg("-hide_banner")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = cmd
            .output()
            .with_context(|| format!("Failed to start encoder test for {}", encoder))?;
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Ok((output.status.success(), stderr))
    };

    let hw_detected = EncoderAvailability {
        h264_nvenc: params.config.hardware_accel && hw_detect::detect_nvidia(),
        hevc_nvenc: params.config.hardware_accel
            && params.config.video_codec == "hevc"
            && hw_detect::detect_nvidia(),
        h264_qsv: params.config.hardware_accel && hw_detect::detect_intel_qsv(),
        hevc_qsv: params.config.hardware_accel
            && params.config.video_codec == "hevc"
            && hw_detect::detect_intel_qsv(),
        h264_amf: params.config.hardware_accel && hw_detect::detect_amd(),
        hevc_amf: params.config.hardware_accel
            && params.config.video_codec == "hevc"
            && hw_detect::detect_amd(),
        h264_cuvid: params.config.hardware_accel && hw_detect::detect_nvidia(),
        hevc_cuvid: params.config.hardware_accel
            && params.config.video_codec == "hevc"
            && hw_detect::detect_nvidia(),
        av1_cuvid: params.config.hardware_accel
            && params.config.video_codec == "av1"
            && hw_detect::detect_nvidia(),
        av1_nvenc: params.config.hardware_accel
            && params.config.video_codec == "av1"
            && hw_detect::detect_nvidia(),
        av1_qsv: params.config.hardware_accel
            && params.config.video_codec == "av1"
            && hw_detect::detect_intel_qsv(),
        av1_amf: params.config.hardware_accel
            && params.config.video_codec == "av1"
            && hw_detect::detect_amd(),
        h264_vulkan: params.config.hardware_accel && hw_detect::detect_vulkan(),
        hevc_vulkan: params.config.hardware_accel
        && params.config.video_codec == "hevc"
        && hw_detect::detect_vulkan(),
        av1_vulkan: params.config.hardware_accel
        && params.config.video_codec == "av1"
        && hw_detect::detect_vulkan(),
    };

    let mut hw_errors = Vec::new();
    let mut encoder_availability = EncoderAvailability {
        h264_nvenc: false,
        hevc_nvenc: false,
        av1_nvenc: false,
        h264_qsv: false,
        hevc_qsv: false,
        av1_qsv: false,
        h264_amf: false,
        hevc_amf: false,
        av1_amf: false,
        h264_cuvid: false,
        hevc_cuvid: false,
        av1_cuvid: false,
        h264_vulkan: false,
        hevc_vulkan: false,
        av1_vulkan: false,
    };

    let encoders_to_test = [
        (
            "h264_nvenc",
            hw_detected.h264_nvenc,
            &mut encoder_availability.h264_nvenc,
        ),
        (
            "hevc_nvenc",
            hw_detected.hevc_nvenc,
            &mut encoder_availability.hevc_nvenc,
        ),
        (
            "av1_nvenc",
            hw_detected.av1_nvenc,
            &mut encoder_availability.av1_nvenc,
        ),
        (
            "h264_qsv",
            hw_detected.h264_qsv,
            &mut encoder_availability.h264_qsv,
        ),
        (
            "hevc_qsv",
            hw_detected.hevc_qsv,
            &mut encoder_availability.hevc_qsv,
        ),
        (
            "av1_qsv",
            hw_detected.av1_qsv,
            &mut encoder_availability.av1_qsv,
        ),
        (
            "h264_amf",
            hw_detected.h264_amf,
            &mut encoder_availability.h264_amf,
        ),
        (
            "hevc_amf",
            hw_detected.hevc_amf,
            &mut encoder_availability.hevc_amf,
        ),
        (
            "av1_amf",
            hw_detected.av1_amf,
            &mut encoder_availability.av1_amf,
        ),
        ("h264_vulkan", hw_detected.h264_vulkan, &mut encoder_availability.h264_vulkan),
        ("hevc_vulkan", hw_detected.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
        ("av1_vulkan", hw_detected.av1_vulkan, &mut encoder_availability.av1_vulkan),
    ];

    for (name, detected, availability_flag) in encoders_to_test {
        if detected {
            match test_encoder(ffmpeg.as_ref(), name) {
                Ok((success, error_output)) => {
                    *availability_flag = success;
                    if !success {
                        hw_errors.push(format!("{} test failed:\n{}", name, error_output.trim()));
                    }
                }
                Err(e) => {
                    *availability_flag = false;
                    hw_errors.push(format!("{} test error: {}", name, e));
                }
            }
        }
    }

    let cuvid_to_test = [
        (
            "h264_cuvid",
            hw_detected.h264_cuvid,
            &mut encoder_availability.h264_cuvid,
        ),
        (
            "hevc_cuvid",
            hw_detected.hevc_cuvid,
            &mut encoder_availability.hevc_cuvid,
        ),
        (
            "av1_cuvid",
            hw_detected.av1_cuvid,
            &mut encoder_availability.av1_cuvid,
        ),
    ];

    for (name, detected, availability_flag) in cuvid_to_test {
        if detected {
            let (encoder_name, container_format) = if name == "h264_cuvid" {
                ("libx264", "mpegts")
            } else if name == "hevc_cuvid" {
                ("libx265", "mpegts")
            } else {
                ("libaom-av1", "matroska")
            };
            let mut encode_cmd = Command::new(&ffmpeg);
            encode_cmd
                .args(&[
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc=duration=1:size=320x240:rate=30",
                    "-vf",
                    "format=yuv420p",
                    "-c:v",
                    encoder_name,
                    "-t",
                    "0.5",
                    "-f",
                    container_format,
                    "-",
                ])
                .stdout(stderr_for_loglevel())
                .stderr(stderr_for_loglevel());
            let mut decode_cmd = Command::new(&ffmpeg);
            decode_cmd
                .args(&[
                    "-hwaccel",
                    "cuvid",
                    "-hwaccel_device",
                    "0",
                    "-c:v",
                    name,
                    "-f",
                    container_format,
                    "-i",
                    "-",
                    "-f",
                    "null",
                    "-",
                ])
                .stdin(Stdio::piped())
                .stdout(stderr_for_loglevel())
                .stderr(stderr_for_loglevel());
            let encoded = match encode_cmd.spawn() {
                Ok(child) => child,
                Err(e) => {
                    *availability_flag = false;
                    hw_errors.push(format!("{} test encode setup failed: {}", name, e));
                    continue;
                }
            };
            decode_cmd.stdin(encoded.stdout.unwrap());
            match decode_cmd.output() {
                Ok(output) => {
                    *availability_flag = output.status.success();
                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        hw_errors.push(format!(
                            "{} decode test failed (code {}):\n{}",
                            name,
                            output.status.code().unwrap_or(-1),
                            stderr.trim()
                        ));
                    }
                }
                Err(e) => {
                    *availability_flag = false;
                    hw_errors.push(format!("{} decode test execution failed: {}", name, e));
                }
            }
        }
    }
    let mut dummy_flag = false;
    let encoder_type = params.config.encoder.as_str();
    let candidates: Vec<(&str, bool, &mut bool)> = match params.config.video_codec.as_str() {
        "hevc" => {
            match encoder_type {
                "nvenc" => vec![
                    (
                        "hevc_nvenc",
                        encoder_availability.hevc_nvenc,
                        &mut encoder_availability.hevc_nvenc,
                    ),
                    ("hevc_vulkan", encoder_availability.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
                    (
                        "hevc_qsv",
                        encoder_availability.hevc_qsv,
                        &mut encoder_availability.hevc_qsv,
                    ),
                    (
                        "hevc_amf",
                        encoder_availability.hevc_amf,
                        &mut encoder_availability.hevc_amf,
                    ),
                    ("libx265", true, &mut dummy_flag),
                ],
                "qsv" => vec![
                    (
                        "hevc_qsv",
                        encoder_availability.hevc_qsv,
                        &mut encoder_availability.hevc_qsv,
                    ),
                    ("hevc_vulkan", encoder_availability.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
                    (
                        "hevc_nvenc",
                        encoder_availability.hevc_nvenc,
                        &mut encoder_availability.hevc_nvenc,
                    ),
                    (
                        "hevc_amf",
                        encoder_availability.hevc_amf,
                        &mut encoder_availability.hevc_amf,
                    ),
                    ("libx265", true, &mut dummy_flag),
                ],
                "amf" => vec![
                    (
                        "hevc_amf",
                        encoder_availability.hevc_amf,
                        &mut encoder_availability.hevc_amf,
                    ),
                    ("hevc_vulkan", encoder_availability.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
                    (
                        "hevc_nvenc",
                        encoder_availability.hevc_nvenc,
                        &mut encoder_availability.hevc_nvenc,
                    ),
                    (
                        "hevc_qsv",
                        encoder_availability.hevc_qsv,
                        &mut encoder_availability.hevc_qsv,
                    ),
                    ("libx265", true, &mut dummy_flag),
                ],
                "vulkan" => vec![
                    ("hevc_vulkan", encoder_availability.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
                    (
                        "hevc_nvenc",
                        encoder_availability.hevc_nvenc,
                        &mut encoder_availability.hevc_nvenc,
                    ),
                    (
                        "hevc_qsv",
                        encoder_availability.hevc_qsv,
                        &mut encoder_availability.hevc_qsv,
                    ),
                    (
                        "hevc_amf",
                        encoder_availability.hevc_amf,
                        &mut encoder_availability.hevc_amf,
                    ),
                    ("libx265", true, &mut dummy_flag),
                ],
                "cpu" => vec![("libx265", true, &mut dummy_flag)],
                _ => vec![
                    (
                        "hevc_nvenc",
                        encoder_availability.hevc_nvenc,
                        &mut encoder_availability.hevc_nvenc,
                    ),
                    (
                        "hevc_qsv",
                        encoder_availability.hevc_qsv,
                        &mut encoder_availability.hevc_qsv,
                    ),
                    (
                        "hevc_amf",
                        encoder_availability.hevc_amf,
                        &mut encoder_availability.hevc_amf,
                    ),
                    ("hevc_vulkan", encoder_availability.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
                    ("libx265", true, &mut dummy_flag),
                ],
            }
        }
        "av1" => {
            match encoder_type {
                "nvenc" => vec![
                    (
                        "av1_nvenc",
                        encoder_availability.av1_nvenc,
                        &mut encoder_availability.av1_nvenc,
                    ),
                    (
                        "av1_qsv",
                        encoder_availability.av1_qsv,
                        &mut encoder_availability.av1_qsv,
                    ),
                    (
                        "av1_amf",
                        encoder_availability.av1_amf,
                        &mut encoder_availability.av1_amf,
                    ),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
                "qsv" => vec![
                    (
                        "av1_qsv",
                        encoder_availability.av1_qsv,
                        &mut encoder_availability.av1_qsv,
                    ),
                    (
                        "av1_nvenc",
                        encoder_availability.av1_nvenc,
                        &mut encoder_availability.av1_nvenc,
                    ),
                    (
                        "av1_amf",
                        encoder_availability.av1_amf,
                        &mut encoder_availability.av1_amf,
                    ),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
                "amf" => vec![
                    (
                        "av1_amf",
                        encoder_availability.av1_amf,
                        &mut encoder_availability.av1_amf,
                    ),
                    (
                        "av1_nvenc",
                        encoder_availability.av1_nvenc,
                        &mut encoder_availability.av1_nvenc,
                    ),
                    (
                        "av1_qsv",
                        encoder_availability.av1_qsv,
                        &mut encoder_availability.av1_qsv,
                    ),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
                "vulkan" => vec![
                    (
                        "av1_nvenc",
                        encoder_availability.av1_nvenc,
                        &mut encoder_availability.av1_nvenc,
                    ),
                    (
                        "av1_qsv",
                        encoder_availability.av1_qsv,
                        &mut encoder_availability.av1_qsv,
                    ),
                    (
                        "av1_amf",
                        encoder_availability.av1_amf,
                        &mut encoder_availability.av1_amf,
                    ),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
                "cpu" => vec![("libaom-av1", true, &mut dummy_flag)],
                _ => vec![
                    (
                        "av1_nvenc",
                        encoder_availability.av1_nvenc,
                        &mut encoder_availability.av1_nvenc,
                    ),
                    (
                        "av1_qsv",
                        encoder_availability.av1_qsv,
                        &mut encoder_availability.av1_qsv,
                    ),
                    (
                        "av1_amf",
                        encoder_availability.av1_amf,
                        &mut encoder_availability.av1_amf,
                    ),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
            }
        }
        _ => {
            match encoder_type {
                "nvenc" => vec![
                    (
                        "h264_nvenc",
                        encoder_availability.h264_nvenc,
                        &mut encoder_availability.h264_nvenc,
                    ),
                    ("h264_vulkan", encoder_availability.h264_vulkan, &mut encoder_availability.h264_vulkan),
                    (
                        "h264_qsv",
                        encoder_availability.h264_qsv,
                        &mut encoder_availability.h264_qsv,
                    ),
                    (
                        "h264_amf",
                        encoder_availability.h264_amf,
                        &mut encoder_availability.h264_amf,
                    ),
                    ("libx264", true, &mut dummy_flag),
                ],
                "qsv" => vec![
                    (
                        "h264_qsv",
                        encoder_availability.h264_qsv,
                        &mut encoder_availability.h264_qsv,
                    ),
                    ("h264_vulkan", encoder_availability.h264_vulkan, &mut encoder_availability.h264_vulkan),
                    (
                        "h264_nvenc",
                        encoder_availability.h264_nvenc,
                        &mut encoder_availability.h264_nvenc,
                    ),
                    (
                        "h264_amf",
                        encoder_availability.h264_amf,
                        &mut encoder_availability.h264_amf,
                    ),
                    ("libx264", true, &mut dummy_flag),
                ],
                "amf" => vec![
                    (
                        "h264_amf",
                        encoder_availability.h264_amf,
                        &mut encoder_availability.h264_amf,
                    ),
                    ("h264_vulkan", encoder_availability.h264_vulkan, &mut encoder_availability.h264_vulkan),
                    (
                        "h264_nvenc",
                        encoder_availability.h264_nvenc,
                        &mut encoder_availability.h264_nvenc,
                    ),
                    (
                        "h264_qsv",
                        encoder_availability.h264_qsv,
                        &mut encoder_availability.h264_qsv,
                    ),
                    ("libx264", true, &mut dummy_flag),
                ],
                "vulkan" => vec![
                    ("h264_vulkan", encoder_availability.h264_vulkan, &mut encoder_availability.h264_vulkan),
                    (
                        "h264_nvenc",
                        encoder_availability.h264_nvenc,
                        &mut encoder_availability.h264_nvenc,
                    ),
                    (
                        "h264_qsv",
                        encoder_availability.h264_qsv,
                        &mut encoder_availability.h264_qsv,
                    ),
                    (
                        "h264_amf",
                        encoder_availability.h264_amf,
                        &mut encoder_availability.h264_amf,
                    ),
                    ("libx264", true, &mut dummy_flag),
                ],
                "cpu" => vec![("libx264", true, &mut dummy_flag)],
                _ => vec![
                    (
                        "h264_nvenc",
                        encoder_availability.h264_nvenc,
                        &mut encoder_availability.h264_nvenc,
                    ),
                    (
                        "h264_qsv",
                        encoder_availability.h264_qsv,
                        &mut encoder_availability.h264_qsv,
                    ),
                    (
                        "h264_amf",
                        encoder_availability.h264_amf,
                        &mut encoder_availability.h264_amf,
                    ),
                    ("h264_vulkan", false, &mut encoder_availability.h264_vulkan),
                    ("libx264", true, &mut dummy_flag),
                ],
            }
        }
    };

    let ffmpeg_encoder = candidates
        .iter()
        .find(|&&(_name, available, _)| available)
        .map(|&(name, _, _)| name)
        .expect("At least one software encoder is available.");

    debug!("{}", split_line('=', Some(" Encoder Selection ")));
    debug!("{}", split_line(' ', Some(format!("Video codec: {}", params.config.video_codec).as_str())));
    debug!("{}", split_line(' ', Some(format!("User preference: {}", params.config.encoder).as_str())));
    debug!("{}", split_line(' ', Some(" Encoder Availability ")));
    debug!("{}", split_line(' ', Some(format!("h264_nvenc: {}", encoder_availability.h264_nvenc).as_str())));
    debug!("{}", split_line(' ', Some(format!("h264_qsv: {}", encoder_availability.h264_qsv).as_str())));
    debug!("{}", split_line(' ', Some(format!("h264_amf: {}", encoder_availability.h264_amf).as_str())));
    debug!("{}", split_line(' ', Some(format!("h264_vulkan: {}", encoder_availability.h264_vulkan).as_str())));
    debug!("{}", split_line(' ', Some(format!("hevc_nvenc: {}", encoder_availability.hevc_nvenc).as_str())));
    debug!("{}", split_line(' ', Some(format!("hevc_qsv: {}", encoder_availability.hevc_qsv).as_str())));
    debug!("{}", split_line(' ', Some(format!("hevc_amf: {}", encoder_availability.hevc_amf).as_str())));
    debug!("{}", split_line(' ', Some(format!("hevc_vulkan: {}", encoder_availability.hevc_vulkan).as_str())));
    debug!("{}", split_line(' ', Some(format!("av1_nvenc: {}", encoder_availability.av1_nvenc).as_str())));
    debug!("{}", split_line(' ', Some(format!("av1_qsv: {}", encoder_availability.av1_qsv).as_str())));
    debug!("{}", split_line(' ', Some(format!("av1_amf: {}", encoder_availability.av1_amf).as_str())));
    debug!("{}", split_line(' ', Some(format!("av1_vulkan: {}", encoder_availability.av1_vulkan).as_str())));
    debug!("{}", split_line('=', None));
    if !hw_errors.is_empty() {
        error!("{}", split_line('-', Some(" Encoder Errors ")));
        for error in &hw_errors {
            error!("    {}", error);
        }
    }
    log::info!("{}", split_line('=', None));
    log::info!("{}", split_line(' ', Some(format!("Selected encoder: {}", ffmpeg_encoder).as_str())));
    log::info!("{}", split_line('=', None));

    let ffmpeg_preset = match ffmpeg_encoder {
        "h264_amf" | "hevc_amf" | "av1_amf" => "-quality",
        "h264_vulkan" | "hevc_vulkan" | "av1_vulkan" => "-preset",
        _ => "-preset",
    };

    let ffmpeg_preset_name = match ffmpeg_encoder {
        "h264_nvenc" | "hevc_nvenc" | "av1_nvenc" => params
            .config
            .ffmpeg_preset
            .split_whitespace()
            .nth(1)
            .unwrap_or("p4"),
        "h264_qsv" | "hevc_qsv" | "av1_qsv" => params
            .config
            .ffmpeg_preset
            .split_whitespace()
            .next()
            .unwrap_or("medium"),
        "h264_amf" | "hevc_amf" | "av1_amf" => params
            .config
            .ffmpeg_preset
            .split_whitespace()
            .nth(2)
            .unwrap_or("balanced"),
        "h264_vulkan" | "hevc_vulkan" | "av1_vulkan" => params
            .config
            .ffmpeg_preset
            .split_whitespace()
            .next()
            .unwrap_or("default"),
        _ => params
            .config
            .ffmpeg_preset
            .split_whitespace()
            .next()
            .unwrap_or("medium"),
    };

    let bitrate_control = if params.config.bitrate_control == "CRF" {
        match ffmpeg_encoder {
            "h264_nvenc" | "hevc_nvenc" | "av1_nvenc" => "-cq",
            "h264_qsv" | "hevc_qsv" | "av1_qsv" => "-q",
            "h264_amf" | "hevc_amf" | "av1_amf" => "-qp_p",
            "h264_vulkan" | "hevc_vulkan" | "av1_vulkan" => "-qp",
            _ => "-crf",
        }
    } else {
        "-b:v"
    };

    if params.config.hardware_accel {
        let h264_supported = encoder_availability.h264_nvenc
            || encoder_availability.h264_qsv
            || encoder_availability.h264_amf
            || encoder_availability.h264_vulkan;
        let hevc_supported = encoder_availability.hevc_nvenc
            || encoder_availability.hevc_qsv
            || encoder_availability.hevc_amf
            || encoder_availability.hevc_vulkan;
        let av1_supported = encoder_availability.av1_nvenc
            || encoder_availability.av1_qsv
            || encoder_availability.av1_amf
            || encoder_availability.av1_vulkan;

        if (params.config.video_codec == "h264" && !h264_supported)
            || (params.config.video_codec == "hevc" && !hevc_supported)
            || (params.config.video_codec == "av1" && !av1_supported)
        {
            error!("{}", tl!("no-hwacc"));
            error!("Hardware detection summary:");
            error!("  - NVIDIA: {}", hw_detected.h264_nvenc);
            error!("  - Intel Quick Sync: {}", hw_detected.h264_qsv);
            error!("  - AMD AMF: {}", hw_detected.h264_amf);
            error!("  - Vulkan: {}", hw_detected.h264_vulkan);
            error!("Encoder test results:");
            error!("  - h264_nvenc: {}", if encoder_availability.h264_nvenc { "SUCCESS" } else { "FAILED" });
            error!("  - hevc_nvenc: {}", if encoder_availability.hevc_nvenc { "SUCCESS" } else { "FAILED" });
            error!("  - av1_nvenc: {}", if encoder_availability.av1_nvenc { "SUCCESS" } else { "FAILED" });
            error!("  - h264_qsv: {}", if encoder_availability.h264_qsv { "SUCCESS" } else { "FAILED" });
            error!("  - hevc_qsv: {}", if encoder_availability.hevc_qsv { "SUCCESS" } else { "FAILED" });
            error!("  - av1_qsv: {}", if encoder_availability.av1_qsv { "SUCCESS" } else { "FAILED" });
            error!("  - h264_amf: {}", if encoder_availability.h264_amf { "SUCCESS" } else { "FAILED" });
            error!("  - hevc_amf: {}", if encoder_availability.hevc_amf { "SUCCESS" } else { "FAILED" });
            error!("  - av1_amf: {}", if encoder_availability.av1_amf { "SUCCESS" } else { "FAILED" });
            error!("  - h264_vulkan: {}", if encoder_availability.h264_vulkan { "SUCCESS" } else { "FAILED" });
            error!("  - hevc_vulkan: {}", if encoder_availability.hevc_vulkan { "SUCCESS" } else { "FAILED" });
            error!("  - av1_vulkan: {}", if encoder_availability.av1_vulkan { "SUCCESS" } else { "FAILED" });
            error!("  - h264_cuvid: {}", if encoder_availability.h264_cuvid { "SUCCESS" } else { "FAILED" });
            error!("  - hevc_cuvid: {}", if encoder_availability.hevc_cuvid { "SUCCESS" } else { "FAILED" });
            error!("  - av1_cuvid: {}", if encoder_availability.av1_cuvid { "SUCCESS" } else { "FAILED" });

            if !hw_errors.is_empty() {
                error!("Detailed error logs:");
                for (i, e) in hw_errors.iter().enumerate() {
                    error!("  {}. {}", i + 1, e);
                }
            } else {
                error!("No hardware encoders were tested (all detection failed).");
            }
            return Err(anyhow::anyhow!(tl!("no-hwacc")));
        }
    }                                                                           
    
    let global_args = "-y";
    let mut input_args = String::new();
    write!(
        &mut input_args,
        "-f rawvideo -c:v rawvideo -s {vw}x{vh} -r {fps} -pix_fmt rgba -i - -i"
    )?;

    let ffmpeg_thread = if params.config.ffmpeg_thread {
        "-thread_queue_size 2048 "
    } else {
        ""
    };

    let video = match params.config.video {
        true => "mov",
        false => "mp4",
    };

    let strict_flag = if params.config.audio_format == "flac" && video == "mp4" {
        "-strict -2 "
    } else {
        ""
    };
    let is_vulkan_encoder = ffmpeg_encoder.ends_with("_vulkan");
    let video_filter = if is_vulkan_encoder {
        "format=nv12,vflip,hwupload"
    } else {
        "format=yuv420p,vflip"
    };

    let args2 = if is_vulkan_encoder {
        format!(
            "-c:a {} -c:v {} {} {} -map 0:v:0 -map 1:a:0 {} {} {} -vf {} -f {}",
            audio_codec,
            ffmpeg_encoder,
            bitrate_control,
            params.config.bitrate,
            strict_flag,
            ffmpeg_thread,
            if params.config.disable_loading {
                format!("-ss {}", LoadingScene::TOTAL_TIME + GameScene::BEFORE_TIME)
            } else {
                "-ss 0.1".to_string()
            },
            video_filter,
            video,
        )
    } else {
        format!(
            "-c:a {} -c:v {} {} {} {} {} -map 0:v:0 -map 1:a:0 {} {} {} -vf {} -f {}",
            audio_codec,
            ffmpeg_encoder,
            bitrate_control,
            params.config.bitrate,
            ffmpeg_preset,
            ffmpeg_preset_name,
            strict_flag,
            ffmpeg_thread,
            if params.config.disable_loading {
                format!("-ss {}", LoadingScene::TOTAL_TIME + GameScene::BEFORE_TIME)
            } else {
                "-ss 0.1".to_string()
            },
            video_filter,
            video,
        )
    };

    let mut proc = {
        let mut cmd = cmd_hidden(&ffmpeg);
        cmd.args(global_args.split_whitespace());
        if is_vulkan_encoder {
            cmd.arg("-init_hw_device")
                .arg("vulkan=vk")
                .arg("-filter_hw_device")
                .arg("vk");
        }
        cmd.args(input_args.split_whitespace())
            .arg(mixing_output.path())
            .args(args2.split_whitespace())
            .arg(&output_path)
            .arg("-loglevel")
            .arg("warning")
            .stdin(Stdio::piped())
            .stderr(stderr_for_loglevel())
            .spawn()
            .with_context(|| tl!("run-ffmpeg-failed"))?
    };
    let mut input = proc.stdin.take().unwrap();

    let rgba_size = vw as usize * vh as usize * 4;
    debug!("RGBA buffer size: {}", rgba_size);

    const MAX_PBO_COUNT: usize = 4;
    let n = MAX_PBO_COUNT.min(fps as usize).max(2);
    let mut pbos: Vec<GLuint> = vec![0; n];
    debug!("Using {} PBOs for async readback (buffering strategy)", n);

    unsafe {
        use miniquad::gl::*;
        glGenBuffers(n as _, pbos.as_mut_ptr());
        for pbo in &pbos {
            glBindBuffer(GL_PIXEL_PACK_BUFFER, *pbo);
            glBufferData(
                GL_PIXEL_PACK_BUFFER,
                rgba_size as _,
                std::ptr::null(),
                GL_STREAM_READ,
            );
        }
        glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
    }

    let fps_f64 = params.config.fps as f64;
    let frame_duration = 1.0 / fps_f64;
    let total_frames = frames;

    let frames10 = total_frames / 10;
    let mut step_time = Instant::now();

    let mut current_pbo = 0;
    let mut previous_pbo = 0;

    for frame in 0..total_frames {
        pb.inc(1);
        if frame % frames10 == 0 || frame == total_frames - 1 {
            let progress = (frame as f64 / total_frames as f64).min(1.0);
            let percent = (progress * 100.).ceil() as i8;
            let bar_width = 20;
            let filled = (progress * bar_width as f64).round() as usize;
            let empty = bar_width - filled;

            let time_text = if frame == total_frames - 1 {
                "Final frame".to_string()
            } else {
                format!("{:.2}s", step_time.elapsed().as_secs_f32())
            };
            step_time = Instant::now();
        }
        let current_frame_time = frame as f64 * frame_duration;
        *my_time.borrow_mut() = current_frame_time;
        let output = mst.output();
        let render_pass: MQRenderPass = unsafe { std::mem::transmute(output.render_pass) };
        gl.quad_gl.render_pass(Some(render_pass));
        main.update()?;
        main.render(&mut painter)?;
        if current_frame_time <= LoadingScene::TOTAL_TIME as f64 && !params.config.disable_loading {
            draw_rectangle(0., 0., 0., 0., Color::default());
        }
        
        if MSAA.load(Ordering::SeqCst) {
            mst.blit();
        }
        
        if frame > 0 {
            unsafe {
                use miniquad::gl::*;
                glBindBuffer(GL_PIXEL_PACK_BUFFER, pbos[previous_pbo]);
                let src = glMapBufferRange(
                    GL_PIXEL_PACK_BUFFER,
                    0,
                    rgba_size as GLsizeiptr,
                    GL_MAP_READ_BIT,
                );
                if !src.is_null() {
                    input.write_all(std::slice::from_raw_parts(src as *const u8, rgba_size))?;
                    glUnmapBuffer(GL_PIXEL_PACK_BUFFER);
                } else {
                    error!("Failed to map PBO at frame {}", frame);
                    return Err(anyhow::anyhow!("Failed to map PBO at frame {}", frame));
                }
                glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
            }
        }
        
        unsafe {
            use miniquad::gl::*;
            glBindFramebuffer(GL_READ_FRAMEBUFFER, internal_id(&mst.output()));
            glBindBuffer(GL_PIXEL_PACK_BUFFER, pbos[current_pbo]);
            glReadPixels(
                0,
                0,
                vw as _,
                vh as _,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                std::ptr::null_mut(),
            );
            glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
        }
        
        previous_pbo = current_pbo;
        current_pbo = (current_pbo + 1) % n;
    }

    for _ in 0..n {
        unsafe {
            use miniquad::gl::*;
            glBindBuffer(GL_PIXEL_PACK_BUFFER, pbos[previous_pbo]);
            let src = glMapBufferRange(
                GL_PIXEL_PACK_BUFFER,
                0,
                rgba_size as GLsizeiptr,
                GL_MAP_READ_BIT,
            );
            if !src.is_null() {
                input.write_all(std::slice::from_raw_parts(src as *const u8, rgba_size))?;
                glUnmapBuffer(GL_PIXEL_PACK_BUFFER);
            }
            glBindBuffer(GL_PIXEL_PACK_BUFFER, 0);
        }
        previous_pbo = (previous_pbo + 1) % n;
    }

    drop(input);
    proc.wait()?;

    log::info!("Render Time: {:.2?}", render_start_time.elapsed());
    log::info!(
        "Average FPS: {:.2}",
        total_frames as f64 / render_start_time.elapsed().as_secs_f64()
    );

    unsafe {
        use miniquad::gl::*;
        glDeleteBuffers(n as _, pbos.as_ptr());
    }

    debug!("IPCEvent::Done(render_start_time.elapsed().as_secs_f64())");
    
    println!(
        "\n\n\n{} {}{:?}",
        split_line_println('#', Some(&"[渲染完成]".green().bold())),
        "\n输出文件:".cyan(),
        output_path
    );
    
    prpr::cleanup_billboard();
    std::process::exit(0);
    
    return Ok(());
}

use clap::Parser;

/// CLI 参数结构
#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct CliArgs {
    #[arg(short, long)]
    pub input: String,
    #[arg(short, long, default_value = "output.mp4")]
    pub output: String,
    #[arg(short, long, default_value = "1920x1440")]
    pub resolution: String,
    #[arg(short, long, default_value_t = 40)]
    pub crf: u32,
    #[clap(long, default_value = "/usr/lib/ptkc-assets")]
    pub assets: String,
    #[arg(long, default_value_t = 40)]
    pub dark: u8,
    #[arg(long, default_value_t = false)]
    pub load: bool,
    #[arg(long, default_value_t = false)]
    pub finish: bool,
    #[arg(long, default_value_t = 30)]
    pub fps: u32,
}

/// CLI 渲染入口
pub async fn render_cli(args: CliArgs) -> Result<()> {
    macroquad::file::set_pc_assets_folder(&args.assets);
    let mut config = RenderConfig::default();
    config.resolution = {
        let parts: Vec<&str> = args.resolution.split('x').collect();
        if parts.len() == 2 {
            (
                parts[0].parse().unwrap_or(1920),
                parts[1].parse().unwrap_or(1200),
            )
        } else {
            (1280, 720)
        }
    };
    config.fps = args.fps;
    config.bitrate = args.crf.to_string();
    config.hardware_accel = true;
    config.video_codec = "h264".to_string();
    config.encoder = "auto".to_string();
    config.target_audio = 48000;
    config.audio_format = "flac".to_string();
    config.combo = "PHI-TK POWERED".to_string();
    config.player_name.clear();
    config.player_rks = 6.16;
    config.ui_name = true;
    config.ui_level = true;

    config.disable_loading = !args.load;
    if args.finish {
        config.ending_length = 3.0;
        config.ui_score = true;
        config.ui_combo = true;
        config.ui_pb = true;
        config.ui_level = true;
        config.ui_name = true;
    }

    let input_path = PathBuf::from(&args.input);
    let mut chart_fs = fs::fs_from_file(&input_path)
        .with_context(|| format!("无法打开谱面: {}", input_path.display()))?;
    let mut info = fs::load_info(chart_fs.deref_mut())
        .await
        .with_context(|| format!("无法读取谱面元数据: {}", input_path.display()))?;

    if args.dark > 0 {
        info.background_dim = (args.dark as f32).clamp(0.0, 100.0) / 100.0;
    }

    let params = RenderParams {
        path: input_path,
        info,
        config,
    };

    main_with_params(params, PathBuf::from(&args.output)).await
}

pub fn init_colored_logger() {
    INIT.call_once(|| {
        let mut builder = env_logger::Builder::from_env(Env::default().default_filter_or("info"));
        builder.filter(Some("symphonia"), log::LevelFilter::Warn);
        builder.target(env_logger::Target::Stdout);
        builder.format(|buf, record| {
            let level = record.level();
            let custom_name = match level {
                Level::Error => "(｀皿´＃).",
                Level::Warn  => "(╬ Ò﹏Ó).",
                Level::Info  => "(*・ω・)っ.",
                Level::Debug => "≽^•⩊•^≼.",
                Level::Trace => "(◎_◎).",
            };
            let level_abbr = match level {
                Level::Error => "ERROR!".red(),
                Level::Warn  => "WARN!".yellow(),
                Level::Info  => "INFO♪".cyan(),
                Level::Debug => "DEBUG:".green(),
                Level::Trace => "TRACE~".magenta(),
            };
            writeln!(
                buf,
                "{}{}{}{}",
                custom_name.bright_black(),
                level_abbr.bold(),
                " ♡ ".bright_magenta(),
                record.args()
            )
        });
        builder.init();
    });
}

pub fn welcome_logger() {
    println!("\n\n\n");
    log::info!("{}", split_line(' ', Some("_|_|_|    _|        _|          _|_|_|_|_|  _|    _|  ")));
    log::info!("{}", split_line(' ', Some("_|    _|  _|_|_|                    _|      _|  _|    ")));
    log::info!("{}", split_line(' ', Some("_|_|_|    _|    _|  _|  _|_|_|_|    _|      _|_|      ")));
    log::info!("{}", split_line(' ', Some("_|        _|    _|  _|              _|      _|  _|    ")));
    log::info!("{}", split_line(' ', Some("_|        _|    _|  _|              _|      _|    _|  ")));
    println!("\n\n\n");
    log::info!("{}", split_line(' ', Some("  _|_|_|  _|        _|_|_|  ")));
    log::info!("{}", split_line(' ', Some("_|        _|          _|    ")));
    log::info!("{}", split_line(' ', Some("_|        _|          _|    ")));
    log::info!("{}", split_line(' ', Some("_|        _|          _|    ")));
    log::info!("{}", split_line(' ', Some("  _|_|_|  _|_|_|_|  _|_|_|  ")));
}

fn split_line(fill: char, title: Option<&str>) -> String {
    let total_width = dimensions().map(|(w, _)| w).unwrap_or(80);
    let log_width = total_width.saturating_sub(24);
    let text = match title {
        Some(t) if !t.is_empty() => t.bold().to_string(),
        _ => String::new(),
    };
    let text_len = text.chars().count();
    let pad_total = if text_len >= log_width {
        0
    } else {
        log_width - text_len
    };
    let left_pad = pad_total / 2;
    let right_pad = pad_total - left_pad;
    let fill_str = fill.to_string();
    let left = fill_str.repeat(left_pad);
    let right = fill_str.repeat(right_pad);
    format!("{left}{text}{right}")
}

fn split_line_println(fill: char, title: Option<&str>) -> String {
    let total_width = dimensions().map(|(w, _)| w).unwrap_or(80);
    let text = match title {
        Some(t) if !t.is_empty() => t.bold().to_string(),
        _ => String::new(),
    };
    let text_len = text.chars().count();
    let pad_total = if text_len >= total_width {
        0
    } else {
        total_width - text_len
    };
    let left_pad = pad_total / 2;
    let right_pad = pad_total - left_pad;
    let fill_str = fill.to_string();
    let left = fill_str.repeat(left_pad);
    let right = fill_str.repeat(right_pad);
    format!("{left}{text}{right}")
}
