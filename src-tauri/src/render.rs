prpr::tl_file!("render");

use anyhow::{bail, Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
// `macroquad::prelude::*` also glob-imports miniquad's `info!`/`warn!`
// macros, which print raw lines on stderr and ignore RUST_LOG. Importing
// the `log` macros explicitly shadows them, so every log line goes through
// `init_colored_logger` (stdout + the coloured format).
use log::{debug, info, warn};
use term_size::dimensions;
use macroquad::{miniquad::{MipmapFilter, RenderPass as MQRenderPass, Texture, TextureFormat, TextureParams, TextureWrap}, prelude::*, window::miniquad_context};
use prpr::{
    config::{BackgroundVideoPlayback, ChallengeModeColor, Config, HdrSettings, Mods},
    core::{MSRenderTarget, RhythmComposite, RhythmLayout, RhythmNoteSelection, RhythmStrip},
    ext::SafeTexture,
    fs,
    info::ChartInfo,
    scene::{BasicPlayer, GameMode, GameScene, LoadingScene},
    time::TimeManager,
    ui::{FontArc, TextPainter},
    Main,
};
use sasa::AudioClip;
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    io::{BufRead, BufWriter, Write},
    ops::DerefMut,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc, Condvar, Mutex,
    },
    time::Instant,
};
use std::{ffi::OsStr, fmt::Write as _};
use tempfile::NamedTempFile;

const POOL_TARGET_BYTES: usize = 256 << 20;

/// RGB8 game targets must be copied before a four-channel encoder readback.
struct RgbaReadback(RenderTarget);

impl RgbaReadback {
    fn new(width: u32, height: u32) -> Self {
        let context = miniquad_context();
        let texture = Texture::new_render_texture(context, TextureParams {
            width, height, format: TextureFormat::RGBA8, ..Default::default()
        });
        let render_pass = MQRenderPass::new(context, texture, None);
        Self(RenderTarget { texture: Texture2D::from_miniquad_texture(texture), render_pass })
    }
}

impl Drop for RgbaReadback {
    fn drop(&mut self) {
        self.0.delete();
    }
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RenderConfig {
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
    pub encoder: String,
    pub show_progress_text: bool,
    pub show_time_text: bool,
    pub target_audio: u32,
    pub autoplay: Option<bool>,
    pub bitrate_control: String,
    pub bitrate: String,
    pub watermark: String,
    pub background: bool,
    /// Background source: `chart`, `solid`, `video`, or `none`.
    /// `background` remains as a compatibility alias for `chart`.
    pub background_mode: String,
    pub background_color: String,
    pub background_video: Option<String>,
    pub background_video_playback: BackgroundVideoPlayback,
    /// Output transfer/colour signalling. SDR defaults to Rec.709.
    pub color_space: String,
    /// Per-layer HDR tone mapping controls. Kept as JSON so older clients can
    /// omit the field while newer clients can add controls independently.
    pub hdr_settings: HdrSettings,
    pub stereo_enabled: bool,
    pub stereo_strength: f32,
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
    pub hand_left_color: String,
    pub hand_right_color: String,
    pub hand_left_opacity: f32,
    pub hand_right_opacity: f32,
    pub hit_effect_enabled: bool,
    pub note_speed_factor: f32,
    pub bar: bool,
    pub score_aim: bool,
    /// Draw the HUD above 噪域 (`blockAreaList`) instead of under it.
    pub block_area_ui: bool,
    pub rhythm_analysis: bool,
    /// Infer musical tempo from audio only when the chart has no authored BPM list.
    pub rhythm_auto_bpm: bool,
    pub rhythm_tap: bool,
    pub rhythm_drag: bool,
    pub rhythm_hold: bool,
    pub rhythm_flick: bool,
    /// Rhythm timeline spacing and movement, independent of chart playback.
    pub rhythm_speed: f32,
    /// Exact custom strip colors; None extracts each chart's artwork colors.
    pub rhythm_colors: Option<[String; 3]>,

    pub ui_score: bool,
    pub ui_combo: bool,
    pub ui_name: bool,
    pub ui_level: bool,
    pub ui_line: bool,
    pub ui_block_area: bool,
    pub ui_pb: bool,
    pub ui_pause: bool,

    pub ffmpeg_thread: bool,

    pub gpu_yuv: bool,
}

#[tauri::command]
pub async fn get_encoder_availability(
    color_space: Option<String>,
) -> Result<std::collections::HashMap<String, Vec<String>>, tauri::ipc::InvokeError> {
    let color_space = ExportColorSpace::parse(color_space.as_deref().unwrap_or("rec709"));
    tokio::task::spawn_blocking(move || query_encoder_availability(color_space))
        .await
        .map_err(|error| tauri::ipc::InvokeError::from(error.to_string()))?
        .map_err(tauri::ipc::InvokeError::from_anyhow)
}

fn query_encoder_availability(
    color_space: ExportColorSpace,
) -> Result<std::collections::HashMap<String, Vec<String>>> {
    let mut support = std::collections::HashMap::<String, Vec<String>>::new();
    #[cfg(target_os = "android")]
    {
        use crate::android_render::mobile_hdr::Transfer;
        let (codecs, transfer): (&[&str], _) = match color_space {
            ExportColorSpace::Rec2100Pq => (&["hevc"], Some(Transfer::Pq)),
            ExportColorSpace::Rec2100Hlg => (&["hevc"], Some(Transfer::Hlg)),
            _ => (&["h264", "hevc"], None),
        };
        for &codec in codecs {
            if let Ok(encoder) = crate::android_media::Encoder::video(320, 240, 2_000_000, 30, 1, codec, transfer) {
                support.insert(codec.to_owned(), vec![encoder.identity().to_owned()]);
            }
        }
        for profile in crate::android_render::prores_availability(transfer)? {
            support.insert(profile.to_owned(), vec!["cpu".to_owned()]);
        }
    }
    #[cfg(target_os = "ios")]
    {
        use crate::apple_media::{Encoder, ProRes};
        use crate::apple_render::mobile_hdr::Transfer;
        let (codec, transfer) = match color_space {
            ExportColorSpace::Rec2100Pq => ("hevc", Some(Transfer::Pq)),
            ExportColorSpace::Rec2100Hlg => ("hevc", Some(Transfer::Hlg)),
            _ => ("h264", None),
        };
        let directory = tempfile::tempdir()?;
        if Encoder::probe(&directory.path().join("probe.mp4"), transfer, None, false).is_ok() {
            support.insert(codec.to_owned(), vec!["videotoolbox".into()]);
        }
        for name in ProRes::NAMES {
            let profile = ProRes::parse(name).expect("known ProRes profile");
            if color_space.is_hdr() && profile.alpha {
                continue;
            }
            if Encoder::probe(
                &directory.path().join(format!("{name}.mov")), transfer, Some(profile),
                color_space == ExportColorSpace::Srgb,
            ).is_ok() {
                support.insert(name.to_owned(), vec!["videotoolbox".into()]);
            }
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if let Some(ffmpeg) = find_ffmpeg()? {
        let path = Path::new(&ffmpeg);
        let result = cmd_hidden(path).args(["-hide_banner", "-encoders"]).output()?;
        let listing = String::from_utf8_lossy(&result.stdout);
        let known = |name: &str| listing.lines().any(|line| line.split_whitespace().nth(1) == Some(name));
        for codec in ["h264", "hevc", "av1"] {
            let mut options = Vec::new();
            for vendor in ["nvenc", "qsv", "amf", "vulkan"] {
                if color_space.is_hdr() && vendor == "vulkan" {
                    continue;
                }
                let name = format!("{codec}_{vendor}");
                if known(&name) && probe_video_encoder_for_color(path, &name, None, color_space) {
                    options.push(vendor.to_owned());
                }
            }
            if software_encoders(codec).iter().any(|name| known(name) && probe_video_encoder_for_color(path, name, None, color_space)) {
                options.push("cpu".to_owned());
            }
            support.insert(codec.to_owned(), options);
        }
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if let Some(ffmpeg) = find_prores_ffmpeg()? {
        let path = Path::new(&ffmpeg);
        let result = cmd_hidden(path).args(["-hide_banner", "-encoders"]).output()?;
        let listing = String::from_utf8_lossy(&result.stdout);
        let known = |name: &str| listing.lines().any(|line| line.split_whitespace().nth(1) == Some(name));
        for profile_name in ["prores_422_hq", "prores_422", "prores_422_lt", "prores_422_proxy", "prores_4444", "prores_4444_xq", "prores_4444_alpha"] {
            let profile = prores_profile(profile_name).unwrap();
            if color_space.is_hdr() && profile.alpha {
                continue;
            }
            let mut options = Vec::new();
            if known("prores_ks") && probe_video_encoder_for_color(path, "prores_ks", Some(profile), color_space) {
                options.push("cpu".to_owned());
            }
            if known("prores_videotoolbox") && probe_video_encoder_for_color(path, "prores_videotoolbox", Some(profile), color_space) {
                options.push("metal".to_owned());
            }
            if DESKTOP_PRORES_VULKAN && known(PRORES_VULKAN_ENCODER)
                && probe_prores_vulkan(path, profile, color_space, (320, 240)).is_ok()
            {
                options.push("vulkan".to_owned());
            }
            support.insert(profile_name.to_owned(), options);
        }
    }
    Ok(support)
}

fn software_encoders(codec: &str) -> &'static [&'static str] {
    match codec {
        "h264" => &["libx264"],
        "hevc" => &["libx265"],
        "av1" => &["libsvtav1", "libaom-av1"],
        _ => &[],
    }
}

fn hdr_encoder_pixel_format(encoder: &str, profile: Option<ProresProfile>) -> &'static str {
    match profile {
        Some(profile) => profile.pixel_format,
        None if encoder.ends_with("_nvenc") || encoder.ends_with("_qsv") || encoder.ends_with("_amf") => "p010le",
        None => "yuv420p10le",
    }
}

fn select_hdr_encoder(ffmpeg: &Path, config: &RenderConfig, color_space: ExportColorSpace) -> Result<String> {
    let codec = config.video_codec.as_str();
    let selected = config.encoder.as_str();
    let software = software_encoders(codec);
    let mut candidates = Vec::new();
    match selected {
        "auto" => {
            if config.hardware_accel && !software.is_empty() {
                candidates.extend(["nvenc", "qsv", "amf"].map(|vendor| format!("{codec}_{vendor}")));
            }
            candidates.extend(software.iter().map(|name| (*name).to_owned()));
        }
        "cpu" => candidates.extend(software.iter().map(|name| (*name).to_owned())),
        "nvenc" | "qsv" | "amf" if config.hardware_accel && !software.is_empty() => {
            candidates.push(format!("{codec}_{selected}"));
        }
        _ => {}
    }
    candidates.into_iter()
        .find(|name| probe_video_encoder_for_color(ffmpeg, name, None, color_space))
        .with_context(|| format!("No compatible 10-bit {codec} encoder for {selected} and {}", config.color_space))
}

fn probe_video_encoder_for_color(
    ffmpeg: &Path,
    encoder: &str,
    profile: Option<ProresProfile>,
    color_space: ExportColorSpace,
) -> bool {
    let mut command = cmd_hidden(ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
    if encoder.ends_with("_vulkan") {
        command.args(["-init_hw_device", "vulkan=vk", "-filter_hw_device", "vk"]);
    }
    command.args(["-f", "lavfi", "-i", "testsrc2=size=320x240:rate=30", "-frames:v", "1"]);
    if let Some(filter) = color_space.ffmpeg_filter(profile.map_or("yuv420p10le", |profile| profile.pixel_format)) {
        command.args(["-vf", &format!("format=gbrpf32le,{filter}")]);
        command.args(["-pix_fmt", hdr_encoder_pixel_format(encoder, profile)]);
        command.args(color_space.ffmpeg_signaling().split_whitespace());
    } else if encoder.ends_with("_vulkan") {
        command.args(["-vf", "format=nv12,hwupload"]);
    }
    if let Some(profile) = profile {
        command.args(["-profile:v", &profile.number.to_string(), "-pix_fmt", profile.pixel_format]);
    }
    command.args(["-c:v", encoder, "-f", "null", "-"]);
    if color_space.is_hdr() {
        // FFmpeg can accept an incompatible requested pixel format and choose 8-bit.
        command.args(["-loglevel", "warning"]);
        return command.stdout(Stdio::null()).output().is_ok_and(|output| {
            output.status.success()
                && !String::from_utf8_lossy(&output.stderr).contains("Incompatible pixel format")
        });
    }
    command.stdout(Stdio::null()).stderr(Stdio::null());
    command.status().is_ok_and(|status| status.success())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExportColorSpace {
    Srgb,
    Rec709,
    Rec2100Pq,
    Rec2100Hlg,
}

impl ExportColorSpace {
    fn parse(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "rec2100_pq" | "rec2100-pq" => Self::Rec2100Pq,
            "rec2100_hlg" | "rec2100-hlg" => Self::Rec2100Hlg,
            "srgb" => Self::Srgb,
            _ => Self::Rec709,
        }
    }

    fn is_hdr(self) -> bool {
        matches!(self, Self::Rec2100Pq | Self::Rec2100Hlg)
    }

    fn ffmpeg_filter(self, pixel_format: &str) -> Option<String> {
        let transfer = match self {
            Self::Rec2100Pq => "smpte2084",
            Self::Rec2100Hlg => "arib-std-b67",
            Self::Srgb | Self::Rec709 => return None,
        };
        Some(format!(
            "zscale=pin=2020:tin=linear:p=2020:t={transfer}:npl=100:m=2020_ncl:r=limited,format={pixel_format}"
        ))
    }

    fn ffmpeg_signaling(self) -> &'static str {
        match self {
            Self::Rec2100Pq => "-colorspace bt2020nc -color_primaries bt2020 -color_trc smpte2084 -color_range tv",
            Self::Rec2100Hlg => "-colorspace bt2020nc -color_primaries bt2020 -color_trc arib-std-b67 -color_range tv",
            Self::Srgb => "-colorspace bt709 -color_primaries bt709 -color_trc iec61966-2-1 -color_range tv",
            Self::Rec709 => "-colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv",
        }
    }
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            resolution: (1920, 1080),
            ffmpeg_preset: "medium p4 balanced".to_string(),
            ending_length: -2.0,
            disable_loading: true,
            chart_debug: false,
            audio_delay_frames: 0,
            flid_x: false,
            chart_ratio: 1.0,
            buffer_size: 256.0,
            combo: "AUTOPLAY".to_string(),
            fps: 60,
            hardware_accel: true,
            video_codec: "h264".to_string(),
            encoder: "auto".to_string(),
            show_progress_text: false,
            show_time_text: false,
            target_audio: 44100,
            autoplay: None,
            bitrate_control: "CRF".to_string(),
            bitrate: "28".to_string(),
            watermark: "".to_string(),
            background: false,
            background_mode: "chart".into(),
            background_color: "#000000".into(),
            background_video: None,
            background_video_playback: BackgroundVideoPlayback::Normal,
            color_space: "rec709".into(),
            hdr_settings: HdrSettings::default(),
            stereo_enabled: false,
            stereo_strength: 1.0,
            aggressive: false,
            challenge_color: ChallengeModeColor::Golden,
            challenge_rank: 45,
            disable_effect: false,
            double_hint: true,
            fxaa: false,
            note_scale: 1.0,
            particle: true,
            player_avatar: None,
            player_name: "".to_string(),
            player_rks: 15.0,
            sample_count: 4,
            res_pack_path: None,
            speed: 1.0,
            volume_music: 1.0,
            volume_sfx: 1.0,
            hand_split: false,
            hand_left_color: "#3380FF".into(),
            hand_right_color: "#FF99B3".into(),
            hand_left_opacity: 1.0,
            hand_right_opacity: 1.0,
            hit_effect_enabled: true,
            note_speed_factor: 1.0,
            video: false,
            audio_bit: None,
            audio_format: "flac".to_string(),
            ui_score: true,
            ui_combo: true,
            ui_name: true,
            ui_level: true,
            ui_line: true,
            ui_block_area: true,
            ui_pb: true,
            ui_pause: true,
            bar: false,
            score_aim: false,
            block_area_ui: false,
            rhythm_analysis: false,
            rhythm_auto_bpm: true,
            rhythm_tap: true,
            rhythm_drag: false,
            rhythm_hold: true,
            rhythm_flick: false,
            rhythm_speed: 1.0,
            rhythm_colors: None,

            ffmpeg_thread: false,

            gpu_yuv: true,
        }
    }
}

impl RenderConfig {
    pub(crate) fn is_hdr(&self) -> bool {
        ExportColorSpace::parse(&self.color_space).is_hdr()
    }

    pub(crate) fn output_extension(&self) -> &'static str {
        if prores_profile(&self.video_codec).is_some()
            || (self.video && !self.audio_format.eq_ignore_ascii_case("flac"))
        {
            "mov"
        } else {
            "mp4"
        }
    }

    /// Keep the requested canvas intact; encoders require even dimensions.
    pub fn output_resolution(&self) -> Result<(u32, u32)> {
        self.validate_rhythm_speed()?;
        let (width, height) = self.canvas_resolution()?;
        let layout = RhythmLayout::new(width, height, self.rhythm_analysis)?;
        Ok((width, layout.output_height))
    }

    pub fn validate_rhythm_speed(&self) -> Result<()> {
        if self.rhythm_analysis && (!self.rhythm_speed.is_finite() || !(0.1..=10.0).contains(&self.rhythm_speed)) {
            bail!("rhythm speed must be a finite number between 0.1 and 10");
        }
        Ok(())
    }

    pub fn canvas_resolution(&self) -> Result<(u32, u32)> {
        let (width, height) = self.resolution;
        if width < 16 || height < 16 || width > i32::MAX as u32 || height > i32::MAX as u32 {
            bail!("resolution must be at least 16x16 and fit the renderer viewport (got {width}x{height})");
        }
        if width % 2 != 0 || height % 2 != 0 {
            bail!("resolution must use even dimensions for video encoding (got {width}x{height})");
        }
        Ok((width, height))
    }

    pub fn to_config(&self) -> Config {
        let background_mode = match self.background_mode.to_ascii_lowercase().as_str() {
            "solid" => "solid",
            "video" => "video",
            "none" => "none",
            _ => "chart",
        };
        let background_color = parse_background_color(&self.background_color);
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
            background_mode: background_mode.to_owned(),
            background_color,
            background_video: self.background_video.clone(),
            background_video_playback: self.background_video_playback,
            color_space: self.color_space.clone(),
            hdr_settings: self.hdr_settings.clone(),
            disable_loading: self.disable_loading,
            hand_split: self.hand_split,
            hand_left_color: self.hand_left_color.clone(),
            hand_right_color: self.hand_right_color.clone(),
            hand_left_opacity: self.hand_left_opacity,
            hand_right_opacity: self.hand_right_opacity,
            hit_effect_enabled: self.hit_effect_enabled,
            note_speed_factor: self.note_speed_factor,

            ui_score: self.ui_score,
            ui_combo: self.ui_combo,
            ui_name: self.ui_name,
            ui_level: self.ui_level,
            ui_line: self.ui_line,
            ui_block_area: self.ui_block_area,
            ui_pb: self.ui_pb,
            ui_pause: self.ui_pause,
            bar: self.bar,
            score_aim: self.score_aim,
            block_area_ui: self.block_area_ui,
            rhythm_analysis: self.rhythm_analysis,
            rhythm_auto_bpm: self.rhythm_auto_bpm,
            rhythm_tap: self.rhythm_tap,
            rhythm_drag: self.rhythm_drag,
            rhythm_hold: self.rhythm_hold,
            rhythm_flick: self.rhythm_flick,
            rhythm_speed: self.rhythm_speed,
            rhythm_colors: self.rhythm_colors.clone(),
            ..Default::default()
        }
    }
}

pub(crate) fn set_background_video_interval(config: &mut Config, fps: u32, skip: u64, frames: u64) {
    if config.background_mode == "video" {
        let fps = f64::from(fps.max(1));
        config.background_video_interval = Some((skip as f64 / fps, frames.saturating_sub(skip) as f64 / fps));
    }
}

fn parse_background_color(value: &str) -> [f32; 4] {
    let hex = value.trim().trim_start_matches('#');
    let parse = |s: &str| u8::from_str_radix(s, 16).unwrap_or(0) as f32 / 255.0;
    match hex.len() {
        6 if hex.bytes().all(|byte| byte.is_ascii_hexdigit()) => [parse(&hex[0..2]), parse(&hex[2..4]), parse(&hex[4..6]), 1.0],
        8 if hex.bytes().all(|byte| byte.is_ascii_hexdigit()) => [parse(&hex[0..2]), parse(&hex[2..4]), parse(&hex[4..6]), parse(&hex[6..8])],
        _ => [0.0, 0.0, 0.0, 1.0],
    }
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RenderParams {
    pub path: PathBuf,
    pub info: ChartInfo,
    pub config: RenderConfig,
}

#[derive(Serialize, Deserialize)]
pub enum IPCEvent {
    StartMixing,
    PrepareEncoder,
    StartRender(u64),
    Finalizing,
    Frame,
    EncoderFps(f64),
    Done(f64),
}

struct FramePipeline {
    pool: Vec<Vec<u8>>,
    pool_addrs: Vec<usize>,
    free_pool: Mutex<(Vec<usize>, bool)>, // (available slots, writer exited)
    free_cv: Condvar,
    jobq: Mutex<(VecDeque<usize>, bool)>, // (queue, stopped)
    jobq_cv: Condvar,
    /// How often / how long the render thread had to wait for a free buffer.
    acquire_blocks: AtomicU64,
    acquire_wait_ns: AtomicU64,
    acquire_max_ns: AtomicU64,
    /// Deepest writer backlog ever observed, in frames.
    queue_max: AtomicUsize,
    /// Time spent inside `write_all` (i.e. blocked on the ffmpeg pipe).
    write_ns: AtomicU64,
    write_count: AtomicU64,
    write_max_ns: AtomicU64,
}

struct FrameWriterClosed<'a>(&'a FramePipeline);

impl Drop for FrameWriterClosed<'_> {
    fn drop(&mut self) {
        self.0.writer_exited();
    }
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
    av1_svt: bool,
}

#[cfg(target_os = "windows")]
mod hw_detect {
    use super::cmd_hidden;
    use std::path::Path;
    use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};

    pub fn detect_nvidia() -> bool {
        if let Ok(key) = RegKey::predef(HKEY_LOCAL_MACHINE)
            .open_subkey(r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}")
        {
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
        cmd_hidden("nvidia-smi").output().is_ok()
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
    use super::cmd_hidden;
    use std::path::Path;

    pub fn detect_nvidia() -> bool {
        Path::new("/dev/nvidia0").exists() || cmd_hidden("nvidia-smi").status().is_ok()
    }

    pub fn detect_intel_qsv() -> bool {
        Path::new("/dev/dri/renderD128").exists()
            && cmd_hidden("vainfo")
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains("VAProfileH264"))
            .unwrap_or(false)
    }

    pub fn detect_amd() -> bool {
        Path::new("/dev/kfd").exists()
            && cmd_hidden("vainfo")
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
    use super::cmd_hidden;

    pub fn detect_nvidia() -> bool {
        cmd_hidden("system_profiler")
            .args(&["SPDisplaysDataType"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains("NVIDIA"))
            .unwrap_or(false)
    }

    pub fn detect_intel_qsv() -> bool {
        cmd_hidden("system_profiler")
            .args(&["SPDisplaysDataType"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains("Intel"))
            .unwrap_or(false)
    }

    pub fn detect_amd() -> bool {
        cmd_hidden("system_profiler")
            .args(&["SPDisplaysDataType"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).contains("AMD"))
            .unwrap_or(false)
    }

    pub fn detect_vulkan() -> bool {
        cmd_hidden("sh")
            .arg("-c")
            .arg("ls /usr/local/lib/libMoltenVK.dylib 2>/dev/null || ls /opt/homebrew/lib/libMoltenVK.dylib 2>/dev/null || ls ~/Library/Frameworks/libMoltenVK.dylib 2>/dev/null")
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

/// 移动端没有可以探测的外部编码器（也不允许 spawn 外部进程），
/// 一律当作「无硬件加速」，让编码器选择回退到软件路径。
#[cfg(mobile)]
mod hw_detect {
    pub fn detect_nvidia() -> bool {
        false
    }

    pub fn detect_intel_qsv() -> bool {
        false
    }

    pub fn detect_amd() -> bool {
        false
    }

    pub fn detect_vulkan() -> bool {
        false
    }
}

pub async fn build_player(config: &RenderConfig) -> Result<BasicPlayer> {
    Ok(BasicPlayer {
        avatar: if let Some(path) = &config.player_avatar {
            Some(
                SafeTexture::from(
                    Texture2D::from_file_with_format(
                        &tokio::fs::read(path)
                            .await
                            .with_context(|| tl!("load-avatar-failed"))?,
                        None,
                    )
                ),
            )
        } else {
            None
        },
        id: 0,
        rks: config.player_rks,
    })
}

const DX_DEVICE: &str = "dx";

pub(crate) fn cmd_hidden(program: impl AsRef<OsStr>) -> Command {
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

/// Bind a loopback TCP listener for the raw video input.
pub fn drain_stderr<R>(mut stderr: R) -> std::thread::JoinHandle<String>where
    R: std::io::Read + Send + 'static,
{
    const LIMIT: usize = 64 * 1024;
    std::thread::Builder::new()
        .name("ffmpeg-stderr".to_owned())
        .spawn(move || {
            let mut collected: Vec<u8> = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match stderr.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if collected.len() < LIMIT {
                            let room = LIMIT - collected.len();
                            collected.extend_from_slice(&buf[..n.min(room)]);
                        }
                    }
                }
            }
            String::from_utf8_lossy(&collected).into_owned()
        })
        .expect("failed to spawn ffmpeg stderr reader")
}

pub fn drain_stderr_with_progress<R>(
    mut stderr: R,
    frames_written: std::sync::Arc<std::sync::atomic::AtomicU64>,
) -> std::thread::JoinHandle<String>
where
    R: std::io::Read + Send + 'static,
{
    const LIMIT: usize = 64 * 1024;
    std::thread::Builder::new()
        .name("ffmpeg-progress".to_owned())
        .spawn(move || {
            let mut collected: Vec<u8> = Vec::new();
            let mut line: Vec<u8> = Vec::new();
            let mut buf = [0u8; 8192];
            let mut last_sent = f64::NAN;
            loop {
                match stderr.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        for &b in &buf[..n] {
                            if b != b'\n' && b != b'\r' {
                                line.push(b);
                                continue;
                            }
                            if let Some(n) = parse_progress_u64(&line, "frame=") {
                                frames_written.store(n, Ordering::Relaxed);
                            }
                            if let Some(fps) = parse_progress_fps(&line) {
                                if fps > 0.0 && fps != last_sent {
                                    last_sent = fps;
                                    crate::ipc::client::send(IPCEvent::EncoderFps(fps));
                                }
                            } else if !is_progress_line(&line) && collected.len() < LIMIT {
                                let room = LIMIT - collected.len();
                                collected.extend_from_slice(&line[..line.len().min(room)]);
                                collected.push(b'\n');
                            }
                            line.clear();
                        }
                    }
                }
            }
            String::from_utf8_lossy(&collected).into_owned()
        })
        .expect("failed to spawn ffmpeg progress reader")
}

fn parse_progress_fps(line: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(line).ok()?;
    let rest = text.strip_prefix("fps=")?;
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn parse_progress_u64(line: &[u8], key: &str) -> Option<u64> {
    let text = std::str::from_utf8(line).ok()?;
    let rest = text.strip_prefix(key)?;
    let digits: String = rest
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() { None } else { digits.parse().ok() }
}

fn is_progress_line(line: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(line) else { return false; };
    let Some((key, _)) = text.split_once('=') else { return false; };
    matches!(
        key,
        "frame"
            | "fps"
            | "bitrate"
            | "total_size"
            | "out_time_us"
            | "out_time_ms"
            | "out_time"
            | "dup_frames"
            | "drop_frames"
            | "speed"
            | "progress"
    ) || key.starts_with("stream_")
}


pub(crate) fn find_prores_ffmpeg() -> Result<Option<String>> {
    let executable = std::env::current_exe()?;
    let name = if cfg!(target_os = "windows") { "ffmpeg-prores.exe" } else { "ffmpeg-prores" };
    let mut candidates = vec![executable.parent().unwrap().join(name)];
    if let Some(assets) = crate::ASSET_PATH.get() {
        if let Some(parent) = assets.parent() { candidates.push(parent.join(name)); }
    }
    for path in candidates {
        if cmd_hidden(&path).arg("-version").output().is_ok_and(|o| o.status.success()) {
            return Ok(Some(path.to_string_lossy().into_owned()));
        }
    }
    find_ffmpeg()
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




pub(crate) fn new_plane(w: u32, h: u32, format: TextureFormat) -> RenderTarget {
    let quad_context = miniquad_context();
    let texture = Texture::new_render_texture(
        quad_context,
        TextureParams {
            width: w,
            height: h,
            format,
            filter: FilterMode::Nearest,
            wrap: TextureWrap::Clamp,
            mipmap_filter: MipmapFilter::None,
        },
    );
    RenderTarget {
        texture: Texture2D::from_miniquad_texture(texture),
        render_pass: MQRenderPass::new(quad_context, texture, None),
    }
}

fn yuv_material(fragment: &str) -> Result<Material> {
    load_material(
        yuv_shader::VERTEX,
        fragment,
        MaterialParams {
            pipeline_params: PipelineParams::default(),
            uniforms: Vec::new(),
            textures: vec!["tex".to_owned()],
        },
    )
    .map_err(|e| anyhow::anyhow!("{e:?}"))
}

struct YuvTarget {
    y: RenderTarget,
    uv: RenderTarget,
    y_material: Material,
    uv_material: Material,
    y_index: usize,
    uv_index: usize,
}

impl YuvTarget {
    fn new(w: u32, h: u32) -> Result<Self> {
        let y = new_plane(w, h, TextureFormat::Alpha);
        let uv = new_plane(w / 2, h / 2, TextureFormat::LuminanceAlpha);
        Ok(Self {
            y_index: y.texture.raw_miniquad_texture_handle().gl_internal_id() as usize,
            uv_index: uv.texture.raw_miniquad_texture_handle().gl_internal_id() as usize,
            y,
            uv,
            y_material: yuv_material(&yuv_shader::Y)?,
            uv_material: yuv_material(&yuv_shader::UV)?,
        })
    }

    fn convert(&self, src: &RenderTarget) {
        self.y_material.set_texture("tex", src.texture);
        self.uv_material.set_texture("tex", src.texture);
        let vertices = [
            Vertex::new(-1., -1., 0., 0., 0., WHITE),
            Vertex::new(1., -1., 0., 1., 0., WHITE),
            Vertex::new(-1., 1., 0., 0., 1., WHITE),
            Vertex::new(1., 1., 0., 1., 1., WHITE),
        ];
        let indices = [0u16, 2, 3, 0, 1, 3];
        let previous = unsafe { get_internal_gl() }.quad_gl.get_viewport();
        for (material, target) in [(&self.y_material, &self.y), (&self.uv_material, &self.uv)] {
            gl_use_material(*material);
            let quad = unsafe { get_internal_gl() }.quad_gl;
            quad.scissor(None);
            quad.viewport(None);
            quad.render_pass(Some(target.render_pass));
            quad.draw_mode(DrawMode::Triangles);
            quad.geometry(&vertices, &indices);
            unsafe { get_internal_gl() }.flush();
            gl_use_default_material();
        }
        let quad = unsafe { get_internal_gl() }.quad_gl;
        quad.render_pass(Some(src.render_pass));
        quad.viewport(previous);
    }
}

struct SendVideoContext(crate::vulkan_video::VideoContext);
unsafe impl Send for SendVideoContext {}

struct VulkanPipe {
    job_tx: mpsc::Sender<(u64, usize)>,
    nv12_raw: Vec<u64>,
    free: VecDeque<usize>,
    free_rx: mpsc::Receiver<usize>,
}

struct ProresPipe {
    writer: crate::frame_handoff::FrameHandoff,
    readback: crate::prores_gpu::Readback,
    packer: crate::prores_gpu::Packer,
    pending: [Option<(u64, usize)>; 2],
    current_read: Option<usize>,
}
impl Drop for ProresPipe {
    fn drop(&mut self) {
        let context = miniquad_context().vulkan_context().unwrap();
        if context.frame_active { context.finish_offscreen_frame(); }
        context.wait_offscreen_idle();
        let _ = self.writer.finish();
    }
}

struct Nv12Pipe {
    // Drop joins the reader before readback storage (and later the GPU context) is released.
    direct: Option<crate::frame_handoff::FrameHandoff>,
    readback: crate::vulkan_readback::Nv12Readback,
    free: VecDeque<usize>,
    ptrs: Vec<usize>,
    frame_bytes: usize,
    pending: [Option<(u64, usize)>; 2],
    current_read: Option<usize>,
}


impl Nv12Pipe {
    fn hand_off_completed(
        &mut self,
        index: u64,
        read_slot: usize,
        pipeline: Option<&FramePipeline>,
    ) -> Result<(f64, f64)> {
        if let Some(direct) = &mut self.direct {
            direct.submit(index, read_slot).context("NV12 writer stopped")?;
            return Ok((0., 0.));
        }
        let pipeline = pipeline.expect("copy handoff needs a frame pool");
        let start = Instant::now();
        let buf = pipeline.acquire_buf()?;
        let wait = start.elapsed().as_secs_f64();
        let start = Instant::now();
        // The GPU fence for read_slot was consumed by the caller. Copy mode
        // releases staging immediately and lends the CPU pool to the writer.
        let src = unsafe { std::slice::from_raw_parts(self.ptrs[read_slot] as *const u8, self.frame_bytes) };
        let dst = unsafe { std::slice::from_raw_parts_mut(pipeline.pool_addrs[buf] as *mut u8, self.frame_bytes) };
        dst.copy_from_slice(src);
        let copy = start.elapsed().as_secs_f64();
        self.free.push_back(read_slot);
        pipeline.push_job(buf);
        Ok((wait, copy))
    }

    fn acquire_readback(&mut self) -> Result<(usize, f64)> {
        if let Some(direct) = &mut self.direct {
            let start = Instant::now();
            let slot = direct.acquire_slot().context("NV12 writer stopped returning readback slots")?;
            Ok((slot, start.elapsed().as_secs_f64()))
        } else {
            Ok((self.free.pop_front().expect("copy handoff exhausted readback slots"), 0.))
        }
    }
}

fn rgba_unpremultiply(input: &[u8], output: &mut [u8]) {
    for (src, dst) in input.chunks_exact(4).zip(output.chunks_exact_mut(4)) {
        let alpha = u32::from(src[3]);
        for channel in 0..3 {
            dst[channel] = if alpha == 0 { 0 } else { ((u32::from(src[channel]) * 255 + alpha / 2) / alpha).min(255) as u8 };
        }
        dst[3] = src[3];
    }
}

fn hdr_rgba_to_gbr(input: &[u8], output: &mut [u8]) {
    let plane_size = input.len() / 2;
    let (green, remaining) = output.split_at_mut(plane_size);
    let (blue, red) = remaining.split_at_mut(plane_size);
    for (index, pixel) in input.chunks_exact(8).enumerate() {
        let start = index * 4;
        for (channel, plane) in [(0, &mut *red), (2, &mut *green), (4, &mut *blue)] {
            let bits = u16::from_ne_bytes([pixel[channel], pixel[channel + 1]]);
            plane[start..start + 4].copy_from_slice(&half::f16::from_bits(bits).to_f32().to_le_bytes());
        }
    }
}

impl FramePipeline {
    fn new(frame_size: usize) -> Self {
        let pool_size = (POOL_TARGET_BYTES / frame_size.max(1)).clamp(8, 32);
        Self {
            pool: (0..pool_size).map(|_| vec![0u8; frame_size]).collect(),
            pool_addrs: Vec::new(),
            free_pool: Mutex::new(((0..pool_size).collect(), false)),
            free_cv: Condvar::new(),
            jobq: Mutex::new((VecDeque::new(), false)),
            jobq_cv: Condvar::new(),

            acquire_blocks: AtomicU64::new(0),
            acquire_wait_ns: AtomicU64::new(0),
            acquire_max_ns: AtomicU64::new(0),

            queue_max: AtomicUsize::new(0),

            write_ns: AtomicU64::new(0),
            write_count: AtomicU64::new(0),
            write_max_ns: AtomicU64::new(0),
        }
    }

    fn finish_init(&mut self) {
        self.pool_addrs = self.pool.iter_mut().map(|b| b.as_mut_ptr() as usize).collect();
    }

    fn acquire_buf(&self) -> Result<usize> {
        let mut free = self.free_pool.lock().unwrap();
        let mut wait_start: Option<Instant> = None;
        loop {
            anyhow::ensure!(!free.1, "FFmpeg writer exited before all frames were submitted");
            if let Some(idx) = free.0.pop() {
                if let Some(start) = wait_start {
                    let d = start.elapsed().as_nanos() as u64;
                    self.acquire_blocks.fetch_add(1, Ordering::Relaxed);
                    self.acquire_wait_ns.fetch_add(d, Ordering::Relaxed);
                    self.acquire_max_ns.fetch_max(d, Ordering::Relaxed);
                }
                return Ok(idx);
            }
            wait_start.get_or_insert_with(Instant::now);
            free = self.free_cv.wait(free).unwrap();
        }
    }

    fn release_buf(&self, idx: usize) {
        self.free_pool.lock().unwrap().0.push(idx);
        self.free_cv.notify_one();
    }

    fn writer_exited(&self) {
        self.free_pool.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
        self.free_cv.notify_all();
    }

    fn push_job(&self, idx: usize) {
        let mut jobq = self.jobq.lock().unwrap();
        jobq.0.push_back(idx);
        let queued = jobq.0.len();
        drop(jobq);
        self.queue_max.fetch_max(queued, Ordering::Relaxed);
        self.jobq_cv.notify_one();
    }

    fn push_stop(&self) {
        self.jobq.lock().unwrap().1 = true;
        self.jobq_cv.notify_one();
    }

    fn pop_job(&self) -> Option<usize> {
        let mut jobq = self.jobq.lock().unwrap();
        loop {
            if let Some(idx) = jobq.0.pop_front() {
                return Some(idx);
            }
            if jobq.1 {
                return None;
            }
            jobq = self.jobq_cv.wait(jobq).unwrap();
        }
    }
}

/// Parse an ffmpeg-style bitrate (`8000k`, `8M`, `8000kbps`, or plain bits/s)
fn parse_bitrate_bps(raw: &str) -> Option<f64> {
    let s = raw.trim().to_ascii_lowercase().replace(' ', "");
    if s.is_empty() {
        return None;
    }
    let (num, mult) = if let Some(v) = s.strip_suffix("kbps") {
        (v, 1e3)
    } else if let Some(v) = s.strip_suffix("mbps") {
        (v, 1e6)
    } else if let Some(v) = s.strip_suffix('k') {
        (v, 1e3)
    } else if let Some(v) = s.strip_suffix('m') {
        (v, 1e6)
    } else {
        (s.as_str(), 1.0)
    };
    let num: f64 = num.parse().ok()?;
    (num > 0.0).then_some(num * mult)
}

fn encoder_qp(config: &RenderConfig, width: u32, height: u32) -> u8 {
    let is_cbr = config.bitrate_control.eq_ignore_ascii_case("CBR");
    if is_cbr {
        if let Some(bps) = parse_bitrate_bps(&config.bitrate) {
            let pixels = width as f64 * height as f64 * config.fps.max(1) as f64;
            let bpp = bps / pixels.max(1.0);
            let qp = 28.0 - 6.0 * (bpp / 0.040).log2();
            return qp.round().clamp(1.0, 51.0) as u8;
        }
        warn!(
            "Invalid CBR bitrate '{}', falling back to QP 28",
            config.bitrate
        );
        return 28;
    }
    let crf = config
        .bitrate
        .trim()
        .parse::<f64>()
        .unwrap_or(28.0)
        .round();
    crf.clamp(1.0, 51.0) as u8
}

const KEYFRAME_INTERVAL_SECS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProresProfile {
    pub(crate) number: u8,
    pixel_format: &'static str,
    pub(crate) alpha: bool,
}

pub(crate) fn prores_profile(codec: &str) -> Option<ProresProfile> {
    let (number, pixel_format, alpha) = match codec {
        "prores_422_proxy" => (0, "yuv422p10le", false),
        "prores_422_lt" => (1, "yuv422p10le", false),
        "prores_422" => (2, "yuv422p10le", false),
        "prores_422_hq" => (3, "yuv422p10le", false),
        "prores_4444" => (4, "yuv444p10le", false),
        "prores_4444_alpha" => (4, "yuva444p10le", true),
        "prores_4444_xq" => (5, "yuva444p12le", true),
        _ => return None,
    };
    Some(ProresProfile { number, pixel_format, alpha })
}

const PRORES_VULKAN_ENCODER: &str = "prores_ks_vulkan";
const DESKTOP_PRORES_VULKAN: bool = cfg!(any(target_os = "windows", target_os = "linux"));
const FFMPEG_VULKAN_DEVICE_ARGS: [&str; 4] = ["-init_hw_device", "vulkan=vk", "-filter_hw_device", "vk"];

fn prores_vulkan_video_args(profile: ProresProfile, color_space: ExportColorSpace) -> String {
    // The Vulkan alpha shader expands 10-bit samples to 16 bits for both
    // 4444 and XQ. Feeding the CPU path's 12-bit XQ format clips/wraps alpha.
    let pixel_format = if profile.alpha { "yuva444p10le" } else { profile.pixel_format };
    let filter = color_space.ffmpeg_filter(pixel_format).unwrap_or_else(|| {
        let transfer = if color_space == ExportColorSpace::Srgb { "iec61966-2-1" } else { "bt709" };
        format!("scale=out_color_matrix=bt709:out_range=tv,format={pixel_format},setparams=range=limited:color_primaries=bt709:color_trc={transfer}:colorspace=bt709")
    });
    // The encoder reads color properties from each frame. Keep them on the
    // software frames before upload and signal them on the MOV stream too.
    format!(
        "-vf {filter},hwupload -pix_fmt vulkan -profile:v {} -alpha_bits {} {}",
        profile.number,
        if profile.alpha { 16 } else { 0 },
        color_space.ffmpeg_signaling(),
    )
}

fn probe_prores_vulkan(
    ffmpeg: &Path,
    profile: ProresProfile,
    color_space: ExportColorSpace,
    (width, height): (u32, u32),
) -> Result<()> {
    let source_format = if color_space.is_hdr() { "gbrpf32le" } else { "rgba" };
    let source = format!("testsrc=size={width}x{height}:rate=30,format={source_format}");
    // Some upstream shader initialization failures are logged without being
    // propagated as return errors. Require both a packet and a clean error log.
    let output = cmd_hidden(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-xerror", "-abort_on", "empty_output_stream"])
        .args(FFMPEG_VULKAN_DEVICE_ARGS)
        .args(["-f", "lavfi", "-i", &source, "-frames:v", "1"])
        .args(prores_vulkan_video_args(profile, color_space).split_whitespace())
        .args(["-c:v", PRORES_VULKAN_ENCODER, "-f", "null", "-"])
        .stdout(Stdio::null())
        .output()
        .context("could not run FFmpeg ProRes Vulkan probe")?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() || !stderr.trim().is_empty() {
        bail!("FFmpeg sample encode failed ({}): {}", output.status, stderr.trim());
    }
    Ok(())
}

fn select_prores_encoder(
    ffmpeg: &Path,
    config: &RenderConfig,
    profile: ProresProfile,
    color_space: ExportColorSpace,
    dimensions: (u32, u32),
) -> Result<&'static str> {
    select_prores_encoder_with(config, DESKTOP_PRORES_VULKAN, |encoder| {
        if encoder == PRORES_VULKAN_ENCODER {
            probe_prores_vulkan(ffmpeg, profile, color_space, dimensions)
        } else if probe_video_encoder_for_color(ffmpeg, encoder, Some(profile), color_space) {
            Ok(())
        } else {
            bail!("sample encode failed");
        }
    })
    .with_context(|| format!("ProRes profile {} ({}) for {}", profile.number, profile.pixel_format, config.color_space))
}

fn select_prores_encoder_with(
    config: &RenderConfig,
    desktop_vulkan: bool,
    mut probe: impl FnMut(&str) -> Result<()>,
) -> Result<&'static str> {
    let encoder = match config.encoder.as_str() {
        "auto" => {
            if desktop_vulkan && config.hardware_accel {
                match probe(PRORES_VULKAN_ENCODER) {
                    Ok(()) => return Ok(PRORES_VULKAN_ENCODER),
                    Err(error) => debug!("ProRes Vulkan probe failed: {error:#}; trying CPU"),
                }
            }
            "prores_ks"
        }
        "cpu" => "prores_ks",
        "metal" | "videotoolbox" => "prores_videotoolbox",
        "vulkan" => {
            anyhow::ensure!(desktop_vulkan, "FFmpeg ProRes Vulkan is supported on desktop Windows and Linux");
            anyhow::ensure!(config.hardware_accel, "enable hardware acceleration to use FFmpeg ProRes Vulkan");
            PRORES_VULKAN_ENCODER
        }
        selected => bail!("encoder '{selected}' does not support ProRes"),
    };
    probe(encoder).with_context(|| format!("FFmpeg ProRes encoder '{encoder}' is unavailable on this device"))?;
    Ok(encoder)
}

pub(crate) fn stereo_pan(normalized_x: f32, strength: f32) -> f32 {
    if !normalized_x.is_finite() || !strength.is_finite() {
        return 0.0;
    }
    let strength = strength.clamp(0.0, 2.0);
    if strength <= 1.0 {
        return (normalized_x * strength).clamp(-1.0, 1.0);
    }
    if strength == 2.0 {
        return if normalized_x > 0.0 { 1.0 } else if normalized_x < 0.0 { -1.0 } else { 0.0 };
    }
    // Above 100%, narrow the center-to-edge transition continuously.
    (normalized_x / (2.0 - strength)).clamp(-1.0, 1.0)
}

pub(crate) fn stereo_gains(pan: f32) -> (f32, f32) {
    let pan = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
    (1.0 - pan.max(0.0), 1.0 + pan.min(0.0))
}

#[cfg(test)]
mod render_config_tests {
    use super::*;

    #[test]
    fn prores_profiles_match_ffmpeg_profile_and_alpha_formats() {
        let cases = [
            ("prores_422_proxy", 0, "yuv422p10le", false),
            ("prores_422_lt", 1, "yuv422p10le", false),
            ("prores_422", 2, "yuv422p10le", false),
            ("prores_422_hq", 3, "yuv422p10le", false),
            ("prores_4444", 4, "yuv444p10le", false),
            ("prores_4444_xq", 5, "yuva444p12le", true),
            ("prores_4444_alpha", 4, "yuva444p10le", true),
        ];
        for (name, number, pixel_format, alpha) in cases {
            assert_eq!(prores_profile(name), Some(ProresProfile { number, pixel_format, alpha }));
        }
        assert_eq!(prores_profile("h264"), None);
    }

    #[test]
    fn prores_vulkan_uses_planar_upload_and_profile_specific_signaling() {
        let hq = prores_profile("prores_422_hq").unwrap();
        let sdr = prores_vulkan_video_args(hq, ExportColorSpace::Rec709);
        assert!(sdr.contains("format=yuv422p10le,setparams=range=limited:"));
        assert!(sdr.contains(",hwupload -pix_fmt vulkan -profile:v 3 -alpha_bits 0"));
        assert!(sdr.contains("-colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv"));
        let srgb = prores_vulkan_video_args(hq, ExportColorSpace::Srgb);
        assert!(srgb.contains("-color_trc iec61966-2-1"));
        let pq = prores_vulkan_video_args(hq, ExportColorSpace::Rec2100Pq);
        assert!(pq.contains("t=smpte2084:npl=100:m=2020_ncl:r=limited,format=yuv422p10le,hwupload"));
        assert!(pq.contains("-colorspace bt2020nc -color_primaries bt2020 -color_trc smpte2084 -color_range tv"));
        let hlg = prores_vulkan_video_args(prores_profile("prores_4444").unwrap(), ExportColorSpace::Rec2100Hlg);
        assert!(hlg.contains("t=arib-std-b67:npl=100:m=2020_ncl:r=limited,format=yuv444p10le,hwupload"));
        assert!(hlg.contains("-pix_fmt vulkan -profile:v 4 -alpha_bits 0"));
        let xq = prores_vulkan_video_args(prores_profile("prores_4444_xq").unwrap(), ExportColorSpace::Rec709);
        assert!(xq.contains("format=yuva444p10le,setparams="));
        assert!(xq.contains("-pix_fmt vulkan -profile:v 5 -alpha_bits 16"));
        for args in [sdr, srgb, pq, hlg, xq] {
            assert!(!args.contains("nv12"));
            assert!(args.split_whitespace().all(|arg| !matches!(arg, "-b:v" | "-crf" | "-preset" | "-async_depth")));
        }
    }

    #[test]
    fn prores_vulkan_auto_falls_back_and_explicit_selection_reports_failure() {
        let mut config = RenderConfig {
            encoder: "auto".into(),
            hardware_accel: true,
            ..Default::default()
        };
        let mut attempts = Vec::new();
        let mut probe_failure = |name: &str| {
            attempts.push(name.to_owned());
            anyhow::ensure!(name != PRORES_VULKAN_ENCODER, "missing Vulkan driver feature");
            Ok(())
        };
        assert_eq!(
            select_prores_encoder_with(&config, true, |name| {
                assert_eq!(name, PRORES_VULKAN_ENCODER);
                Ok(())
            }).unwrap(),
            PRORES_VULKAN_ENCODER
        );
        assert_eq!(select_prores_encoder_with(&config, true, &mut probe_failure).unwrap(), "prores_ks");
        config.encoder = "vulkan".into();
        let error = select_prores_encoder_with(&config, true, &mut probe_failure).unwrap_err();
        assert!(format!("{error:#}").contains("missing Vulkan driver feature"));
        assert_eq!(attempts, [PRORES_VULKAN_ENCODER, "prores_ks", PRORES_VULKAN_ENCODER]);
        assert!(select_prores_encoder_with(&config, false, |_| panic!("unsupported platform")).is_err());
        config.hardware_accel = false;
        assert!(select_prores_encoder_with(&config, true, |_| panic!("acceleration disabled")).is_err());
        config.encoder = "auto".into();
        assert_eq!(select_prores_encoder_with(&config, true, |name| {
            assert_eq!(name, "prores_ks");
            Ok(())
        }).unwrap(), "prores_ks");
        config.encoder = "metal".into();
        assert_eq!(select_prores_encoder_with(&config, false, |name| {
            assert_eq!(name, "prores_videotoolbox");
            Ok(())
        }).unwrap(), "prores_videotoolbox");
    }

    #[test]
    fn stereo_pan_preserves_center_and_moves_notes_to_the_correct_channel() {
        assert_eq!(stereo_pan(0.0, 1.0), 0.0);
        assert_eq!(stereo_gains(0.0), (1.0, 1.0));
        assert_eq!(stereo_pan(-0.75, 0.5), -0.375);
        assert_eq!(stereo_gains(-1.0), (1.0, 0.0));
        assert_eq!(stereo_gains(1.0), (0.0, 1.0));
        assert_eq!(stereo_pan(f32::NAN, 1.0), 0.0);
        assert_eq!(stereo_pan(2.0, 1.0), 1.0);
        assert_eq!(stereo_pan(0.25, 1.5), 0.5);
        let one_pixel = 2.0 / 1920.0;
        assert_eq!(stereo_gains(stereo_pan(-one_pixel, 2.0)), (1.0, 0.0));
        assert_eq!(stereo_gains(stereo_pan(one_pixel, 2.0)), (0.0, 1.0));
        assert_eq!(stereo_gains(stereo_pan(0.0, 2.0)), (1.0, 1.0));
    }

    #[test]
    fn writer_exit_wakes_a_full_frame_pool() {
        let pipeline = std::sync::Arc::new(FramePipeline::new(4));
        for _ in 0..pipeline.pool.len() {
            pipeline.acquire_buf().unwrap();
        }
        let producer = std::sync::Arc::clone(&pipeline);
        let (send, recv) = mpsc::channel();
        let join = std::thread::spawn(move || send.send(producer.acquire_buf()).unwrap());
        drop(FrameWriterClosed(&pipeline));
        assert!(recv.recv_timeout(std::time::Duration::from_secs(1)).unwrap().is_err());
        join.join().unwrap();
    }

    #[test]
    fn hdr_transfer_preserves_linear_planes_and_drops_alpha() {
        let input: Vec<_> = [0.5, 2.0, 4.0, 1.0, 8.0, 16.0, 32.0, 0.0]
            .into_iter()
            .flat_map(|v| half::f16::from_f32(v).to_bits().to_ne_bytes())
            .collect();
        let mut output = [0; 24];
        hdr_rgba_to_gbr(&input, &mut output);
        let values: Vec<_> = output.chunks_exact(4).map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap())).collect();
        assert_eq!(values, [2.0, 16.0, 4.0, 32.0, 0.5, 8.0]);
    }

    #[test]
    fn legacy_and_partial_render_configs_keep_background_hdr_and_stereo_defaults() {
        let legacy: RenderConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.background_mode, "chart");
        assert!(!legacy.stereo_enabled);
        assert_eq!(legacy.hdr_settings.ui.len(), 12);
        let partial: RenderConfig = serde_json::from_str(r#"{"hdrSettings":{"ui":{"score":{"mode":"mapped"}}}}"#).unwrap();
        assert_eq!(partial.hdr_settings.ui.get("score").unwrap().mode, "mapped");
        assert_eq!(partial.hdr_settings.ui.len(), 1);
    }

    #[test]
    fn background_colors_decode_and_reject_invalid_strings_without_panicking() {
        assert_eq!(parse_background_color("#00FF80"), [0., 1., 128. / 255., 1.]);
        assert_eq!(parse_background_color("FF0080CC"), [1., 0., 128. / 255., 204. / 255.]);
        assert_eq!(parse_background_color("#12zzab"), [0., 0., 0., 1.]);
        assert_eq!(parse_background_color("ééé"), [0., 0., 0., 1.]);
    }

    #[test]
    fn local_prores_encoder_accepts_all_declared_profiles_and_selects_mov() {
        let ffmpeg = find_ffmpeg().unwrap().unwrap();
        for name in ["prores_422_proxy", "prores_422_lt", "prores_422", "prores_422_hq", "prores_4444", "prores_4444_xq", "prores_4444_alpha"] {
            let profile = prores_profile(name).unwrap();
            assert!(probe_video_encoder_for_color(Path::new(&ffmpeg), "prores_ks", Some(profile), ExportColorSpace::Rec709), "profile {name}");
        }
        let mut config = RenderConfig::default();
        config.video_codec = "prores_422_hq".to_owned();
        assert_eq!(config.output_extension(), "mov");
        config.video = false;
        assert_eq!(config.output_extension(), "mov");
        config.video_codec = "h264".to_owned();
        assert_eq!(config.output_extension(), "mp4");
        config.video = true;
        config.audio_format = "flac".to_owned();
        assert_eq!(config.output_extension(), "mp4");
    }
}

pub async fn main() -> Result<()> {
    use crate::ipc::client::*;

    set_pc_assets_folder(&std::env::args().nth(2).unwrap());

    if std::env::var_os("PHI_TKC_PRORES_GPU_SMOKE").is_some() {
        crate::prores_gpu::smoke()?;
        std::process::exit(0);
    }
    let mut stdin = std::io::BufReader::new(std::io::stdin());

    let mut line = String::new();
    stdin.read_line(&mut line)?;
    let params: RenderParams = serde_json::from_str(line.trim())?;

    line.clear();
    stdin.read_line(&mut line)?;
    let output_path: PathBuf = serde_json::from_str(line.trim())?;

    let control = std::sync::Arc::new(crate::render_control::RenderControl::default());
    control.listen(stdin)?;
    render_impl(params, output_path, true, false, control).await
}

/// Entry used by [`render_cli`]: same pipeline, parameters supplied directly.
pub async fn main_with_params(
    params: RenderParams,
    output_path: PathBuf,
    exit_after: bool,
    log_mode: bool,
) -> Result<()> {
    let result = render_impl(params, output_path, exit_after, log_mode, std::sync::Arc::new(crate::render_control::RenderControl::default())).await;
    if exit_after && result.is_ok() {
        // The CLI historically exits once the video is written; macroquad has no
        // window to pump in headless mode, so returning would just hang.
        std::process::exit(0);
    }
    result
}

async fn render_impl(
    params: RenderParams,
    output_path: PathBuf,
    exit_after: bool,
    log_mode: bool,
    control: std::sync::Arc<crate::render_control::RenderControl>,
) -> Result<()> {
    use crate::ipc::client::*;

    // Every entry point (`render_cli`, the GUI child's `main()`, the server's
    // child process) goes through here, and everything below logs through the
    // `log` crate. Without this the GUI/server child produced no logs at all,
    // because only `render_cli` used to call `init_colored_logger`.
    init_colored_logger();

    // `log_mode` means "a human is watching this terminal": print the pretty
    // logs and the progress bar, and keep stdout free of IPC events. The GUI and
    // server child pass `false` and parse the events instead.
    set_enabled(!log_mode && std::env::var_os("PHI_TKC_IPC").is_some());
    let _ = exit_after;
    let process_start = Instant::now();
    let (vw, vh) = params.config.output_resolution()?;
    let (game_width, game_height) = params.config.canvas_resolution()?;
    let prores = prores_profile(&params.config.video_codec);
    let color_space = ExportColorSpace::parse(&params.config.color_space);
    if color_space.is_hdr() && prores.is_some_and(|profile| profile.alpha) {
        bail!("HDR output is not supported with alpha ProRes profiles");
    }
    let hdr_filter = color_space.ffmpeg_filter(prores.map_or("yuv420p10le", |profile| profile.pixel_format));
    let output_path = if prores.is_some() { output_path.with_extension("mov") } else { output_path };
    let output_owner = crate::output_guard::OutputGuard::new(&output_path)?;
    let final_output_path = output_path;
    let output_path = output_owner.path().to_owned();
    control.check()?;
    let path = params.path;
    info!("Preparing chart: {}", path.display());

    let mut fs = fs::fs_from_file(&path)?;

    let font = FontArc::try_from_vec(load_file("font.ttf").await?)?;

    let Some(ffmpeg) = (if prores.is_some() { find_prores_ffmpeg()? } else { find_ffmpeg()? }) else {
        bail!("FFmpeg not found")
    };
    debug!("Using ffmpeg: {}", ffmpeg);
    let ffmpeg_path = Path::new(&ffmpeg);
    let prores_encoder = prores
        .map(|profile| select_prores_encoder(ffmpeg_path, &params.config, profile, color_space, (vw, vh)))
        .transpose()?;

    let hdr_encoder = if color_space.is_hdr() && prores.is_none() {
        Some(select_hdr_encoder(ffmpeg_path, &params.config, color_space)?)
    } else {
        None
    };

    let mut painter = TextPainter::new(font);

    let mut config = params.config.to_config();
    config.hdr_output = color_space.is_hdr();
    config.mods = Mods::AUTOPLAY;
    config.interactive = false;

    let info = params.info;

    let (chart, ..) = GameScene::load_chart(fs.deref_mut(), &info)
        .await
        .with_context(|| tl!("load-chart-failed"))?;
    macro_rules! ld {
            ($path:literal) => {
                AudioClip::new(load_file($path).await?)
                    .with_context(|| tl!("load-sfx-failed", "name" => $path))?
            };
        }
    let music: Result<_> = async { AudioClip::new(fs.load_file(&info.music).await?) }.await;
    let music = music.with_context(|| tl!("load-music-failed"))?;
    let rhythm_strip = if params.config.rhythm_analysis {
        let cover = fs.load_file(&info.illustration).await?;
        Some(RhythmStrip::from_chart_bytes_with_audio(
            &chart, RhythmNoteSelection::from(&config), &cover, &music, config.rhythm_auto_bpm,
            chart.offset + config.offset + info.offset,
        )?.with_speed(config.rhythm_speed).with_colors(config.rhythm_colors.as_ref()))
    } else {
        None
    };
    let ending = ld!("ending.mp3");
    let track_length = music.length() as f64;
    let sfx_click = ld!("click.ogg");
    let sfx_drag = ld!("drag.ogg");
    let sfx_flick = ld!("flick.ogg");


    let volume_music = std::mem::take(&mut config.volume_music);
    let volume_sfx = std::mem::take(&mut config.volume_sfx);

    let length = track_length - chart.offset.min(0.) as f64 + 1.;
    let video_length = O + length + A + params.config.ending_length;
    let offset = chart.offset.max(0.);

    let render_start_time = Instant::now();
    debug!(
        "setup chart + audio load: {:.3}s",
        process_start.elapsed().as_secs_f64()
    );
    let mut phase = render_start_time;
    macro_rules! mark {
        ($name:literal) => {{
            let d = phase.elapsed().as_secs_f64();
            debug!("setup {}: {:.3}s", $name, d);
            phase = Instant::now();
            d
        }};
    }

    info!("Mixing audio");
    send(IPCEvent::StartMixing);
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

    //debug!("=== Audio/Video Sync Configuration ===");
    //debug!("  Audio delay: {} frames", params.config.audio_delay_frames);
    //debug!("  Audio delay: {:.6} seconds", audio_delay);
    //debug!("  Frame duration: {:.6}s @ {}fps", frame_duration, params.config.fps);
    //debug!("  Sample delay: {} samples @ {}Hz", (audio_delay * sample_rate_f64).round() as i64, sample_rate);
    //debug!("======================================");

    let audio_buffer_length = video_length + audio_delay.abs();
    let mut output = vec![0.0_f32; (audio_buffer_length * sample_rate_f64).ceil() as usize * 2];
    
    let ss = if params.config.disable_loading { O } else { 0.1 };
    let ss_samples = (ss * sample_rate_f64).round() as usize * 2;
    let skip = ((ss * params.config.fps as f64 - 1e-6).ceil().max(0.)) as u64;

    if volume_music != 0.0 {
        let start_time = Instant::now();
        let original_pos = O - chart.offset.min(0.) as f64;
        let pos = original_pos + audio_delay;

        //debug!("Music mixing: original_pos={:.6}s, delayed_pos={:.6}s", original_pos, pos);

        let start_index = (pos * sample_rate_f64).round() as usize * 2;
        let ratio = 1.0 / sample_rate_f64;

        if start_index >= output.len() {
            warn!("Music start position {} exceeds output buffer length {}", start_index, output.len());
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

    let mut place = |pos: f64, clip: &AudioClip, volume: f32, pan: f32| {
        let position = (pos * sample_rate_f64).round() as usize * 2;
        if position >= output.len() {
            return 0;
        }
        let len = clip.frame_count();
        let output_len = output.len() - position;
        let valid_frames = (output_len / 2).min(len);

        let output_ptr = unsafe { output.as_mut_ptr().add(position) };
        let frames_ptr = clip.frames().as_ptr();

        let (left_gain, right_gain) = stereo_gains(pan);
        for i in 0..valid_frames {
            unsafe {
                let sample = (*frames_ptr.add(i)).0 * volume;
                *output_ptr.add(i * 2) += sample * left_gain;
                *output_ptr.add(i * 2 + 1) += sample * right_gain;
            }
        }
        valid_frames
    };

    if volume_sfx != 0.0 {
        let start_time = Instant::now();

        let offset_f64 = offset as f64;
        let o_offset = O + offset_f64 + audio_delay;
        let hit_positions = params.config.stereo_enabled.then(||
            chart.note_hit_positions(&config, game_width as f32 / game_height as f32)
        );

        //debug!("SFX mixing: offset={:.6}s (includes {:.6}s delay)", o_offset, audio_delay);

        let sfx_lut =
        [
            &sfx_click as *const _,
            &sfx_drag as *const _,
            &sfx_click as *const _,
            &sfx_flick as *const _,
        ];

        unsafe {
            let lines_ptr = chart.lines.as_ptr();
            let lines_len = chart.lines.len();

            let mut note_offset = 0;
            for i in 0..lines_len {
                let line = &*lines_ptr.add(i);
                let notes_ptr = line.notes.as_ptr();
                let notes_len = line.notes.len();
                for j in 0..notes_len {
                    let note = &*notes_ptr.add(j);
                    if !note.fake {
                        let sfx = &*sfx_lut[note.kind.order() as usize];
                        let time = o_offset + note.time as f64;
                        let pan = if let Some(hit_positions) = &hit_positions {
                            stereo_pan(
                                hit_positions[note_offset + j].x,
                                params.config.stereo_strength,
                            )
                        } else {
                            0.0
                        };
                        place(time, sfx, volume_sfx, pan);
                    }
                }
                note_offset += notes_len;
            }
        }

        debug!("sfx Time:{:?}", start_time.elapsed());
    }

    let mut pos = O + length + A + audio_delay;
    debug!("Ending music start: {:.6}s", pos);

    while place(pos, &ending, volume_music, 0.0) != 0 && params.config.ending_length > 0.1 {
        pos += ending.frame_count() as f64 / sample_rate_f64;
    }
    let mixing_s = mark!("mixing");
    send(IPCEvent::PrepareEncoder);

    let audio_bit = params.config.audio_bit;
    let system_ffmpeg = find_ffmpeg()?;
    let sidecar_audio = system_ffmpeg.is_none() && prores.is_some();
    let mut audio_format = params.config.audio_format.to_lowercase();
    if prores.is_some() && matches!(audio_format.as_str(), "flac" | "opus") {
        info!("Using 24-bit PCM audio for ProRes MOV compatibility");
        audio_format = "pcm".into();
    }

    if sidecar_audio && audio_format == "mp3" {
        info!("Using AAC audio: the bundled ProRes encoder does not include libmp3lame");
        audio_format = "aac".into();
    }
    let supported_formats = ["flac", "mp3", "aac", "opus", "wav", "pcm"];
    if !supported_formats.contains(&audio_format.as_str()) {
        bail!
        ("Unsupported audio format: {}. Supported formats are: {}",

            audio_format,
            supported_formats.join(", ")

        );
    }

    if let Some(bit) = audio_bit {
        if ![16, 24, 32].contains(&bit)
        { bail!("Invalid audio bit depth: {}. Supported values are 16, 24, 32.", bit); }
        if audio_format != "wav"
        { return Err(anyhow::anyhow!("PCM audio bit depth requires WAV format, but {} was specified", audio_format)); }
    }

    let (audio_mux_args, output_format) = if let Some(bit) = audio_bit {
        (
            format!("-c:a pcm_s{}le", bit),
            "wav".to_string(),
        )
    } else {
        match audio_format.as_str()
        {
            "flac" => (
                "-c:a flac -sample_fmt s32 -bits_per_raw_sample 24 -strict -2".to_string(),
                "flac".to_string(),
            ),
            "mp3" => ("-c:a libmp3lame".to_string(), "mp3".to_string()),
            "aac" => ("-c:a aac".to_string(), "mp4".to_string()),
            "opus" => ("-c:a libopus".to_string(), "opus".to_string()),
            "wav" => ("-c:a pcm_s16le".to_string(), "wav".to_string()),
            "pcm" => ("-c:a pcm_s24le".to_string(), "wav".to_string()),
            _ => {
                warn!("Unknown audio format '{}', using AAC/MP4 as default", audio_format);
                ("-c:a aac".to_string(), "mp4".to_string())
            }
        }
    };

    let args_str = if target_sample_rate != sample_rate {
        let resample_filter = format!("aresample=resampler=soxr:precision=33:osr={}:dither_method=triangular", target_sample_rate);
        format!(
            "-y -f f32le -ar {} -ac 2 -i - -af {} {} -f {}",
            sample_rate,
            resample_filter,
            audio_mux_args,
            output_format
        )
    } else {
        format!(
            "-y -f f32le -ar {} -ac 2 -i - {} -f {}",
            sample_rate,
            audio_mux_args,
            output_format
        )
    };

    let audio_suffix = format!(".{output_format}");
    let audio_output_file = NamedTempFile::with_suffix(&audio_suffix)?;
    let audio_ffmpeg = system_ffmpeg.unwrap_or_else(|| ffmpeg.clone());
    let args_str = if sidecar_audio {
        args_str.replace("aresample=resampler=soxr:precision=33:", "aresample=")
    } else { args_str };
    let mut proc = cmd_hidden(&audio_ffmpeg)
        .args(args_str.split_whitespace())
        .arg(audio_output_file.path())
        .arg("-loglevel")
        .arg("warning")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| tl!("run-ffmpeg-failed"))?;
    let mut proc = control.process(proc);
    let audio_stderr = drain_stderr(proc.stderr.take().unwrap());
    let input = proc.stdin.as_mut().unwrap();
    let mut writer = BufWriter::new(input);
    for (index, sample) in output.into_iter().skip(ss_samples).enumerate() {
        if index % 8192 == 0 { control.check()?; }
        writer.write_all(&sample.to_le_bytes())?;
    }
    drop(writer);
    proc.stdin.take();
    let audio_status = proc.wait()?;
    if let Ok(text) = audio_stderr.join() {
        if !text.trim().is_empty() {
            warn!("[ffmpeg:audio]\n{}", text);
        }
    }
    if !audio_status.success() {
        bail!("audio encoding failed: {audio_status}");
    }
    let audio_s = mark!("audio encode + resample");



    let qp = encoder_qp(&params.config, vw, vh);
    // The Rust Vulkan encoder implements H.264 only, so the existing
    // "Hardware Encoder", "Video Codec" and "Hardware Acceleration" controls
    // decide whether it is used at all.  Other selections keep using the
    // ffmpeg encoder path.
    // The Vulkan Video encoder is driven from swapchain-backed frames; a
    // headless context has none, so fall back to the NV12 + ffmpeg path.
    let headless_vulkan = crate::vulkan_readback::headless_vulkan();
    let amd_gpu = macroquad::window::miniquad_context().vulkan_context()
        .map(|context| unsafe {
            context.instance.get_physical_device_properties(context.physical_device).vendor_id == 0x1002
        }).unwrap_or(false);
    // RX 7900 XTX can stall inside the driver's video-session/DPB creation,
    // before a fence timeout can apply. Keep rendering through the working
    // AMF path even when an AMD user selected Vulkan; other GPUs keep Vulkan.
    let amd_vulkan_fallback = prores.is_none() && amd_gpu && matches!(params.config.encoder.as_str(), "auto" | "vulkan");
    if amd_vulkan_fallback && params.config.hardware_accel {
        info!("AMD GPU: using FFmpeg AMF instead of Vulkan Video; software encoding is the fallback");
    }
    let want_vulkan_video = params.config.hardware_accel
        && !color_space.is_hdr()
        && params.config.video_codec == "h264"
        && !headless_vulkan
        && !amd_gpu
        && matches!(params.config.encoder.as_str(), "auto" | "vulkan");
    let mut video_context = if want_vulkan_video {
        crate::vulkan_video::VideoContext::new(vw, vh, qp, 16)
    } else {
        None
    };
    let vulkan_video = video_context.is_some();
    if want_vulkan_video && !vulkan_video {
        warn!("Rust Vulkan encoder unavailable; falling back to the ffmpeg encoder");
    }
    debug!(
        "Vulkan Video H.264: {} ({} source images, {}x{}, qp={})",
        if vulkan_video { "enabled" } else { "disabled" },
        video_context
            .as_ref()
            .map(|context| context.source_count())
            .unwrap_or(0),
        vw,
        vh,
        qp
    );
    if want_vulkan_video {
        if params.config.bitrate_control.eq_ignore_ascii_case("CBR") {
            debug!(
                "Vulkan encoder is constant-QP: CBR target '{}' mapped to qp={}",
                params.config.bitrate, qp
            );
        } else {
            debug!(
                "Vulkan encoder quality: CRF {} -> qp={}",
                params.config.bitrate, qp
            );
        }
    }

    let yuv_count = if prores.is_some() {
        0
    } else if !vulkan_video && vw % 2 == 0 && vh % 2 == 0 {
        2
    } else {
        1
    };
    let yuvs: Vec<YuvTarget> = (0..yuv_count)
        .filter_map(|_| YuvTarget::new(vw, vh).ok())
        .collect();
    let force_rgba_readback = color_space.is_hdr()
        || std::env::var("PHITK_READBACK").is_ok_and(|mode| mode == "rgba");
    let mut ffmpeg_nv12 = if !force_rgba_readback && !vulkan_video && yuvs.len() == 2 {
        match crate::vulkan_readback::Nv12Readback::new(vw, vh, 8) {
            Some(readback) => {
                debug!(
                    "ffmpeg NV12: Vulkan async readback, {} slots, {:.2} MB/frame",
                    readback.slot_count(),
                    readback.frame_size() as f64 / 1048576.0
                );
                Some(readback)
            }
            None => {
                warn!(
                    "ffmpeg NV12: readback ring creation failed, falling back to RGBA readback"
                );
                None
            }
        }
    } else {
        None
    };
    let nv12_mode = ffmpeg_nv12.is_some();
    let mut prores_staging = if headless_vulkan && !color_space.is_hdr()
        && prores.is_some_and(|p| p.alpha)
        && !std::env::var("PHITK_PRORES_CONVERSION").is_ok_and(|mode| mode == "cpu")
    {
        let create = || -> Result<_> {
            let packer = crate::prores_gpu::Packer::new(vw, vh)?;
            let readback = crate::prores_gpu::Readback::new(packer.frame_bytes(), 4)?;
            Ok((packer, readback))
        };
        match create() {
            Ok(staging) => Some(staging),
            Err(error) => { warn!("ProRes GPU conversion unavailable: {error:#}; using RGBA fallback"); None }
        }
    } else { None };
    let async_prores = prores_staging.is_some();
    let mst_count = if nv12_mode || async_prores { 2 } else { 1 };
    let alpha = prores.is_some_and(|profile| profile.alpha);
    config.transparent_background = alpha;
    let msts: Vec<Rc<MSRenderTarget>> = (0..mst_count)
        .map(|_| {
            let size = (game_width, game_height);
            Rc::new(if config.hdr_output { MSRenderTarget::new_hdr(size, 1) }
                else if alpha { MSRenderTarget::new_rgba(size, config.sample_count) }
                else { MSRenderTarget::new(size, config.sample_count) })
        })
        .collect();
    let rhythm = if config.hdr_output {
        RhythmComposite::new_hdr_with_settings(game_width, game_height, rhythm_strip, mst_count, alpha, config.hdr_settings.rhythm.clone())?
    } else {
        RhythmComposite::new_with_alpha(game_width, game_height, rhythm_strip, mst_count, alpha)?
    };
    let rgba_readback = (!nv12_mode && !async_prores && !vulkan_video && !config.hdr_output).then(|| RgbaReadback::new(vw, vh));
    let mst_slot = Rc::new(Cell::new(0usize));
    let my_time: Rc<RefCell<f64>> = Rc::new(RefCell::new(0.));
    let tm = TimeManager::manual(Box::new({
        let my_time = Rc::clone(&my_time);
        move || *(*my_time).borrow()
    }));
    if (nv12_mode || headless_vulkan) && !crate::vulkan_readback::enable_offscreen_video() {
        bail!("failed to enable the Vulkan offscreen video mode");
    }
    mark!("offscreen/readback setup");
    let fps = params.config.fps;
    let frames = (video_length * fps as f64).ceil() as u64;
    let out_frames = frames.saturating_sub(skip);
    anyhow::ensure!(out_frames > 0, "render duration contains no output frames");
    set_background_video_interval(&mut config, fps, skip, frames);
    let player = build_player(&params.config).await?;
    let mut main = Main::new(
        Box::new(
            LoadingScene::new(GameMode::Normal, info, config, fs, Some(player), None, None).await?,
        ),
        tm,
        {
            let msts = Rc::new(msts.iter().cloned().collect::<Vec<_>>());
            let mst_slot = Rc::clone(&mst_slot);
            move || Some(msts[mst_slot.get()].input())
        },
    )
        .await?;
    main.top_level = false;
    main.viewport = Some((0, 0, game_width as _, game_height as _));

    *my_time.borrow_mut() = 0.;
    main.update()?;
    mark!("player + scene load");

    const O: f64 = LoadingScene::TOTAL_TIME as f64 + GameScene::BEFORE_TIME as f64;
    const A: f64 = 1.0;

    static VULKAN_PROBE_LOCK: Mutex<()> = Mutex::new(());

    fn test_encoder(ffmpeg: &Path, encoder: &str) -> Result<(bool, String)> {
        let _vulkan_guard = if encoder.ends_with("_vulkan") {
            Some(VULKAN_PROBE_LOCK.lock().unwrap_or_else(|e| e.into_inner()))
        } else {
            None
        };
        let mut cmd = cmd_hidden(ffmpeg);

        if encoder.ends_with("_vulkan") {
            cmd.args(&[
                "-init_hw_device", "vulkan=vk",
                "-f", "lavfi",
                "-i", "testsrc=duration=0.1:size=320x240:rate=30",
                "-filter_hw_device", "vk",
                "-vf", "format=nv12,hwupload",
                "-c:v", encoder,
                "-f", "null", "-",
            ]);
        } else {
            cmd.args(&[
                "-f", "lavfi",
                "-i", "testsrc2=size=320x240:rate=30:duration=0.3",
                "-c:v", encoder,
                "-f", "null", "-",
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
    }

    #[derive(Clone, Copy)]
    struct DxEncoder {
        name: &'static str,
        hw: &'static str,
        pix: &'static str,
    }

    fn probe_dx_encoder(ffmpeg: &Path, enc: DxEncoder) -> bool {
        let device = format!("{}={}", enc.hw, DX_DEVICE);
        let mut cmd = cmd_hidden(ffmpeg);
        cmd.args([
            "-init_hw_device",
            device.as_str(),
            "-filter_hw_device",
            DX_DEVICE,
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=30:duration=0.3",
            "-vf",
            "format=nv12,hwupload",
            "-c:v",
            enc.name,
            "-pix_fmt",
            enc.pix,
        ]);
        cmd.args(["-bf", "0"]);
        cmd.args(["-f", "null", "-", "-loglevel", "error"]);
        cmd.arg("-hide_banner")
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        matches!(cmd.output(), Ok(out) if out.status.success())
    }

    let hw_detected = EncoderAvailability {
        h264_nvenc: params.config.hardware_accel && hw_detect::detect_nvidia(),
        hevc_nvenc: params.config.hardware_accel
            && params.config.video_codec == "hevc"
            && hw_detect::detect_nvidia(),
        h264_qsv: params.config.hardware_accel && hw_detect::detect_intel_qsv(),
        hevc_qsv: params.config.hardware_accel
            && params.config.video_codec == "hevc"
            && hw_detect::detect_intel_qsv(),
        h264_amf: params.config.hardware_accel && (amd_gpu || hw_detect::detect_amd()),
        hevc_amf: params.config.hardware_accel && params.config.video_codec == "hevc" && (amd_gpu || hw_detect::detect_amd()),
        h264_cuvid: params.config.hardware_accel && hw_detect::detect_nvidia(),
        hevc_cuvid: params.config.hardware_accel && params.config.video_codec == "hevc" && hw_detect::detect_nvidia(),
        av1_cuvid: params.config.hardware_accel && params.config.video_codec == "av1" && hw_detect::detect_nvidia(),
        av1_nvenc: params.config.hardware_accel
            && params.config.video_codec == "av1"
            && hw_detect::detect_nvidia(),
        av1_qsv: params.config.hardware_accel
            && params.config.video_codec == "av1"
            && hw_detect::detect_intel_qsv(),
        av1_amf: params.config.hardware_accel
            && params.config.video_codec == "av1"
            && (amd_gpu || hw_detect::detect_amd()),
        h264_vulkan: params.config.hardware_accel && hw_detect::detect_vulkan(),
        hevc_vulkan: params.config.hardware_accel
            && params.config.video_codec == "hevc"
            && hw_detect::detect_vulkan(),
        av1_vulkan: params.config.hardware_accel
            && params.config.video_codec == "av1"
            && hw_detect::detect_vulkan(),
        av1_svt: false,
    };

    let hw_errors = Vec::new();

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
        av1_svt: false,
    };

    fn set_avail(a: &mut EncoderAvailability, name: &str, ok: bool) {
        match name {
            "h264_nvenc" => a.h264_nvenc = ok,
            "hevc_nvenc" => a.hevc_nvenc = ok,
            "av1_nvenc" => a.av1_nvenc = ok,
            "h264_qsv" => a.h264_qsv = ok,
            "hevc_qsv" => a.hevc_qsv = ok,
            "av1_qsv" => a.av1_qsv = ok,
            "h264_amf" => a.h264_amf = ok,
            "hevc_amf" => a.hevc_amf = ok,
            "av1_amf" => a.av1_amf = ok,
            "h264_vulkan" => a.h264_vulkan = ok,
            "hevc_vulkan" => a.hevc_vulkan = ok,
            "av1_vulkan" => a.av1_vulkan = ok,
            "libsvtav1" => a.av1_svt = ok,
            _ => {}
        }
    }

    let codec_idx = match params.config.video_codec.as_str() {
        "hevc" => 1usize,
        "av1" => 2,
        _ => 0,
    };
    const V_NV: [&str; 3] = ["h264_nvenc", "hevc_nvenc", "av1_nvenc"];
    const V_QSV: [&str; 3] = ["h264_qsv", "hevc_qsv", "av1_qsv"];
    const V_AMF: [&str; 3] = ["h264_amf", "hevc_amf", "av1_amf"];
    const V_VK: [&str; 3] = ["h264_vulkan", "hevc_vulkan", "av1_vulkan"];
    let vendor_detected = |first: &str| -> bool {
        match first {
            "h264_nvenc" => hw_detected.h264_nvenc,
            "h264_qsv" => hw_detected.h264_qsv,
            "h264_amf" => hw_detected.h264_amf,
            _ => hw_detected.h264_vulkan,
        }
    };
    let vendors: Vec<[&str; 3]> = match params.config.encoder.as_str() {
        _ if prores.is_some() || color_space.is_hdr() => vec![],
        "cpu" => vec![],
        "auto" | "vulkan" if amd_vulkan_fallback => vec![V_AMF, V_NV, V_QSV],
        "amf" => vec![V_AMF, V_NV, V_QSV, V_VK],
        "qsv" => vec![V_QSV, V_NV, V_AMF, V_VK],
        "vulkan" => vec![V_VK, V_NV, V_QSV, V_AMF],
        _ => vec![V_NV, V_QSV, V_AMF, V_VK],
    };
    let mut hw_errors: Vec<String> = hw_errors;
    for v in vendors {
        if amd_gpu && v == V_VK {
            continue;
        }
        // The built-in Vulkan encoder produces the stream itself; probing
        // ffmpeg's encoders would only mislabel the run and waste ~0.5s.
        if vulkan_video {
            break;
        }
        if !vendor_detected(v[0]) {
            continue;
        }
        let names: Vec<&str> = v.iter().copied().filter(|n| !n.trim().is_empty()).collect();
        let handles: Vec<_> = names
            .iter()
            .map(|name| {
                let name = name.to_string();
                let ff = ffmpeg.clone();
                std::thread::spawn(move || {
                    let (ok, err) = match test_encoder(ff.as_ref(), &name) {
                        Ok((ok, err)) => (ok, err),
                        Err(e) => (false, format!("test error: {e}")),
                    };
                    (name, ok, err)
                })
            })
            .collect();
        let mut ok_for_codec = false;
        for handle in handles {
            match handle.join() {
                Ok((name, ok, err)) => {
                    if ok && name == v[codec_idx] {
                        ok_for_codec = true;
                    }
                    set_avail(&mut encoder_availability, &name, ok);
                    if !ok {
                        hw_errors.push(format!("{} test failed:\n{}", name, err.trim()));
                    }
                }
                Err(_) => hw_errors.push("encoder probe thread panicked".to_owned()),
            }
        }
        if ok_for_codec { break; }
    }
    if params.config.video_codec == "av1" && !color_space.is_hdr() {
        match test_encoder(ffmpeg.as_ref(), "libsvtav1") {
            Ok((ok, err)) => {
                encoder_availability.av1_svt = ok;
                if !ok {
                    hw_errors.push(format!("libsvtav1 test failed:\n{}", err.trim()));
                }
            }
            Err(e) => hw_errors.push(format!("libsvtav1 test error: {e}")),
        }
    }

    // This will be a future feature (cuvid)
    let cuvid_to_test = [
        ("h264_cuvid", hw_detected.h264_cuvid, &mut encoder_availability.h264_cuvid),
        ("hevc_cuvid", hw_detected.hevc_cuvid, &mut encoder_availability.hevc_cuvid),
        ("av1_cuvid", hw_detected.av1_cuvid, &mut encoder_availability.av1_cuvid),
    ];

    for (name, detected, availability_flag) in cuvid_to_test {
        if detected && !color_space.is_hdr() {
            let (encoder_name, container_format) = if name == "h264_cuvid" {
                ("libx264", "mpegts")
            } else if name == "hevc_cuvid" {
                ("libx265", "mpegts")
            } else {
                ("libsvtav1", "matroska")
            };

            let mut encode_cmd = cmd_hidden(&ffmpeg);
            encode_cmd.args(&[
                "-f", "lavfi",
                "-i", "testsrc=duration=1:size=320x240:rate=30",
                "-vf", "format=yuv420p",
                "-c:v", encoder_name,
                "-t", "0.5",
                "-f", container_format,
                "-"
            ])
                .stdout(Stdio::piped())
                .stderr(Stdio::null());

            let mut decode_cmd = cmd_hidden(&ffmpeg);
            decode_cmd.args(&[
                "-hwaccel", "cuvid",
                "-hwaccel_device", "0",
                "-c:v", name,
                "-f", container_format,
                "-i", "-",
                "-f", "null",
                "-"
            ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped());

            let mut encoded = match encode_cmd.spawn() {
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
                        let mut enc_err = String::new();
                        if let Some(mut e) = encoded.stderr.take() {
                            let _ = std::io::Read::read_to_string(&mut e, &mut enc_err);
                        }
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        hw_errors.push(format!(
                            "{} decode test failed (code {}):\n{}{}",
                            name,
                            output.status.code().unwrap_or(-1),
                            stderr.trim(),
                            if enc_err.trim().is_empty() {
                                String::new()
                            } else {
                                format!("\n[encoder side]\n{}", enc_err.trim())
                            }
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
    if !vulkan_video && !color_space.is_hdr() {
        if let Ok((ok, err)) = test_encoder(ffmpeg.as_ref(), "libsvtav1") {
            encoder_availability.av1_svt = ok;
            if !ok {
                hw_errors.push(format!("libsvtav1 test failed:
{}", err.trim()));
            }
        }
    }

    let mut dummy_flag = false;
    let encoder_type = params.config.encoder.as_str();
    let any_vendor_hw = match params.config.video_codec.as_str() {
        "hevc" => {
            encoder_availability.hevc_nvenc
                || encoder_availability.hevc_qsv
                || encoder_availability.hevc_amf
                || encoder_availability.hevc_vulkan
        }
        "av1" => {
            encoder_availability.av1_nvenc
                || encoder_availability.av1_qsv
                || encoder_availability.av1_amf
                || encoder_availability.av1_vulkan
        }
        _ => {
            encoder_availability.h264_nvenc
                || encoder_availability.h264_qsv
                || encoder_availability.h264_amf
                || encoder_availability.h264_vulkan
        }
    };

    let want_dx = !color_space.is_hdr() && params.config.hardware_accel
        && !vulkan_video
        && match encoder_type {
            "dx12" => true,
            "auto" => !any_vendor_hw,
            _ => false,
        };
    const DX12_H264: [DxEncoder; 1] = [DxEncoder { name: "h264_d3d12va", hw: "d3d12va", pix: "d3d12" }];
    const DX12_HEVC: [DxEncoder; 1] = [DxEncoder { name: "hevc_d3d12va", hw: "d3d12va", pix: "d3d12" }];
    const DX12_AV1: [DxEncoder; 1] = [DxEncoder { name: "av1_d3d12va", hw: "d3d12va", pix: "d3d12" }];
    let dx_candidates: Vec<DxEncoder> = match params.config.video_codec.as_str() {
        "hevc" => vec![DX12_HEVC[0]],
        "av1" => vec![DX12_AV1[0]],
        _ => vec![DX12_H264[0]],
    };
    let mut dx_selected: Option<DxEncoder> = None;
    let mut dx_codec_switched: Option<&str> = None;
    if want_dx && prores.is_none() {
        let mut tried = String::new();
        for enc in &dx_candidates {
            if probe_dx_encoder(ffmpeg.as_ref(), *enc) {
                debug!(
                    "  DX upload path: probe OK -> {} ({} default device, -pix_fmt {}, -bf 0)",
                    enc.name, enc.hw, enc.pix
                );
                dx_selected = Some(*enc);
                break;
            }
            tried.push_str(&format!("{}/{} ", enc.name, enc.hw));
        }
        if dx_selected.is_none() {
            for (codec, enc) in [
                ("hevc", DX12_HEVC[0]),
                ("h264", DX12_H264[0]),
                ("av1", DX12_AV1[0]),
            ] {
                if codec == params.config.video_codec.as_str() {
                    continue;
                }
                if probe_dx_encoder(ffmpeg.as_ref(), enc) {
                    debug!("  DX upload path: {} unavailable ({}), switching codec to {} via {}",
                        params.config.video_codec, tried.trim_end(), codec.to_uppercase(), enc.name);
                    dx_selected = Some(enc);
                    dx_codec_switched = Some(codec);
                    break;
                }
            }
        }
        if dx_selected.is_none() {
            warn!("  DX upload path: all probes failed ({}), falling back to other encoders", tried.trim_end());
        }
        if let Some(codec) = dx_codec_switched {
            warn!(
                "  Note: this ffmpeg build has no DX12 encoder for {}, the output will be {}",
                params.config.video_codec,
                codec.to_uppercase()
            );
        }
    }
    let candidates: Vec<(&str, bool, &mut bool)> = match params.config.video_codec.as_str() {
        "hevc" => {
            match encoder_type {
                "nvenc" => vec![
                    ("hevc_nvenc", encoder_availability.hevc_nvenc, &mut encoder_availability.hevc_nvenc),
                    ("hevc_vulkan", encoder_availability.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
                    ("hevc_qsv", encoder_availability.hevc_qsv, &mut encoder_availability.hevc_qsv),
                    ("hevc_amf", encoder_availability.hevc_amf, &mut encoder_availability.hevc_amf),
                    ("libx265", true, &mut dummy_flag),
                ],
                "qsv" => vec![
                    ("hevc_qsv", encoder_availability.hevc_qsv, &mut encoder_availability.hevc_qsv),
                    ("hevc_vulkan", encoder_availability.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
                    ("hevc_nvenc", encoder_availability.hevc_nvenc, &mut encoder_availability.hevc_nvenc),
                    ("hevc_amf", encoder_availability.hevc_amf, &mut encoder_availability.hevc_amf),
                    ("libx265", true, &mut dummy_flag),
                ],
                "amf" => vec![
                    ("hevc_amf", encoder_availability.hevc_amf, &mut encoder_availability.hevc_amf),
                    ("hevc_vulkan", encoder_availability.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
                    ("hevc_nvenc", encoder_availability.hevc_nvenc, &mut encoder_availability.hevc_nvenc),
                    ("hevc_qsv", encoder_availability.hevc_qsv, &mut encoder_availability.hevc_qsv),
                    ("libx265", true, &mut dummy_flag),
                ],
                "vulkan" => vec![
                    ("hevc_vulkan", encoder_availability.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
                    ("hevc_nvenc", encoder_availability.hevc_nvenc, &mut encoder_availability.hevc_nvenc),
                    ("hevc_qsv", encoder_availability.hevc_qsv, &mut encoder_availability.hevc_qsv),
                    ("hevc_amf", encoder_availability.hevc_amf, &mut encoder_availability.hevc_amf),
                    ("libx265", true, &mut dummy_flag),
                ],
                "cpu" => vec![
                    ("libx265", true, &mut dummy_flag),
                ],
                _ => vec![
                    ("hevc_nvenc", encoder_availability.hevc_nvenc, &mut encoder_availability.hevc_nvenc),
                    ("hevc_qsv", encoder_availability.hevc_qsv, &mut encoder_availability.hevc_qsv),
                    ("hevc_amf", encoder_availability.hevc_amf, &mut encoder_availability.hevc_amf),
                    ("hevc_vulkan", encoder_availability.hevc_vulkan, &mut encoder_availability.hevc_vulkan),
                    ("libx265", true, &mut dummy_flag),
                ],
            }
        },
        "av1" => {
            match encoder_type {
                "nvenc" => vec![
                    ("av1_nvenc", encoder_availability.av1_nvenc, &mut encoder_availability.av1_nvenc),
                    ("av1_qsv", encoder_availability.av1_qsv, &mut encoder_availability.av1_qsv),
                    ("av1_amf", encoder_availability.av1_amf, &mut encoder_availability.av1_amf),
                    ("av1_vulkan", encoder_availability.av1_vulkan, &mut encoder_availability.av1_vulkan),
                    ("libsvtav1", encoder_availability.av1_svt, &mut encoder_availability.av1_svt),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
                "qsv" => vec![
                    ("av1_qsv", encoder_availability.av1_qsv, &mut encoder_availability.av1_qsv),
                    ("av1_nvenc", encoder_availability.av1_nvenc, &mut encoder_availability.av1_nvenc),
                    ("av1_amf", encoder_availability.av1_amf, &mut encoder_availability.av1_amf),
                    ("av1_vulkan", encoder_availability.av1_vulkan, &mut encoder_availability.av1_vulkan),
                    ("libsvtav1", encoder_availability.av1_svt, &mut encoder_availability.av1_svt),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
                "amf" => vec![
                    ("av1_amf", encoder_availability.av1_amf, &mut encoder_availability.av1_amf),
                    ("av1_nvenc", encoder_availability.av1_nvenc, &mut encoder_availability.av1_nvenc),
                    ("av1_qsv", encoder_availability.av1_qsv, &mut encoder_availability.av1_qsv),
                    ("av1_vulkan", encoder_availability.av1_vulkan, &mut encoder_availability.av1_vulkan),
                    ("libsvtav1", encoder_availability.av1_svt, &mut encoder_availability.av1_svt),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
                "vulkan" => vec![
                    ("av1_vulkan", encoder_availability.av1_vulkan, &mut encoder_availability.av1_vulkan),
                    ("av1_nvenc", encoder_availability.av1_nvenc, &mut encoder_availability.av1_nvenc),
                    ("av1_qsv", encoder_availability.av1_qsv, &mut encoder_availability.av1_qsv),
                    ("av1_amf", encoder_availability.av1_amf, &mut encoder_availability.av1_amf),
                    ("libsvtav1", encoder_availability.av1_svt, &mut encoder_availability.av1_svt),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
                "cpu" => vec![
                    ("libsvtav1", encoder_availability.av1_svt, &mut encoder_availability.av1_svt),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
                _ => vec![
                    ("av1_nvenc", encoder_availability.av1_nvenc, &mut encoder_availability.av1_nvenc),
                    ("av1_qsv", encoder_availability.av1_qsv, &mut encoder_availability.av1_qsv),
                    ("av1_amf", encoder_availability.av1_amf, &mut encoder_availability.av1_amf),
                    ("av1_vulkan", encoder_availability.av1_vulkan, &mut encoder_availability.av1_vulkan),
                    ("libsvtav1", encoder_availability.av1_svt, &mut encoder_availability.av1_svt),
                    ("libaom-av1", true, &mut dummy_flag),
                ],
            }
        },
        _ => {
            match encoder_type {
                "nvenc" => vec![
                    ("h264_nvenc", encoder_availability.h264_nvenc, &mut encoder_availability.h264_nvenc),
                    ("h264_vulkan", encoder_availability.h264_vulkan, &mut encoder_availability.h264_vulkan),
                    ("h264_qsv", encoder_availability.h264_qsv, &mut encoder_availability.h264_qsv),
                    ("h264_amf", encoder_availability.h264_amf, &mut encoder_availability.h264_amf),
                    ("libx264", true, &mut dummy_flag),
                ],
                "qsv" => vec![
                    ("h264_qsv", encoder_availability.h264_qsv, &mut encoder_availability.h264_qsv),
                    ("h264_vulkan", encoder_availability.h264_vulkan, &mut encoder_availability.h264_vulkan),
                    ("h264_nvenc", encoder_availability.h264_nvenc, &mut encoder_availability.h264_nvenc),
                    ("h264_amf", encoder_availability.h264_amf, &mut encoder_availability.h264_amf),
                    ("libx264", true, &mut dummy_flag),
                ],
                "amf" => vec![
                    ("h264_amf", encoder_availability.h264_amf, &mut encoder_availability.h264_amf),
                    ("h264_vulkan", encoder_availability.h264_vulkan, &mut encoder_availability.h264_vulkan),
                    ("h264_nvenc", encoder_availability.h264_nvenc, &mut encoder_availability.h264_nvenc),
                    ("h264_qsv", encoder_availability.h264_qsv, &mut encoder_availability.h264_qsv),
                    ("libx264", true, &mut dummy_flag),
                ],
                "vulkan" => vec![
                    ("h264_vulkan", encoder_availability.h264_vulkan, &mut encoder_availability.h264_vulkan),
                    ("h264_nvenc", encoder_availability.h264_nvenc, &mut encoder_availability.h264_nvenc),
                    ("h264_qsv", encoder_availability.h264_qsv, &mut encoder_availability.h264_qsv),
                    ("h264_amf", encoder_availability.h264_amf, &mut encoder_availability.h264_amf),
                    ("libx264", true, &mut dummy_flag),
                ],
                "cpu" => vec![
                    ("libx264", true, &mut dummy_flag),
                ],
                _ => vec![
                    ("h264_nvenc", encoder_availability.h264_nvenc, &mut encoder_availability.h264_nvenc),
                    ("h264_qsv", encoder_availability.h264_qsv, &mut encoder_availability.h264_qsv),
                    ("h264_amf", encoder_availability.h264_amf, &mut encoder_availability.h264_amf),
                    ("h264_vulkan", encoder_availability.h264_vulkan, &mut encoder_availability.h264_vulkan),
                    ("libx264", true, &mut dummy_flag),
                ],
            }
        },
    };


    let mut ffmpeg_encoder = if let Some(encoder) = prores_encoder.or(hdr_encoder.as_deref()) {
        encoder
    } else { candidates
        .iter()
        .find(|&&(_name, available, _)| available)
        .map(|&(name, _, _)| name)
        .expect("At least one software encoder is available.") };

    let dx_selected = match encoder_type {
        "dx12" => dx_selected,
        _ => dx_selected.filter(|_| matches!(ffmpeg_encoder, "libx264" | "libx265" | "libaom-av1")),
    };
    if prores.is_none() {
    if let Some(enc) = &dx_selected {
        ffmpeg_encoder = enc.name;
    }
    }

    if vulkan_video {
        debug!("  (encoder probing skipped: the built-in Vulkan Video encoder produces the stream)");
    }
    debug!(
    "=== Encoder Selection ===\n\
     Video codec: {}\n\
     User preference: {}\n\
     --- Encoder Availability ---\n\
       h264_nvenc: {}\n\
       h264_qsv: {}\n\
       h264_amf: {}\n\
       h264_vulkan: {}\n\
       hevc_nvenc: {}\n\
       hevc_qsv: {}\n\
       hevc_amf: {}\n\
       hevc_vulkan: {}\n\
       av1_nvenc: {}\n\
       av1_qsv: {}\n\
       av1_amf: {}\n\
       av1_vulkan: {}\n\
     {}",
    params.config.video_codec,
    params.config.encoder,
    encoder_availability.h264_nvenc,
    encoder_availability.h264_qsv,
    encoder_availability.h264_amf,
    encoder_availability.h264_vulkan,
    encoder_availability.hevc_nvenc,
    encoder_availability.hevc_qsv,
    encoder_availability.hevc_amf,
    encoder_availability.hevc_vulkan,
    encoder_availability.av1_nvenc,
    encoder_availability.av1_qsv,
    encoder_availability.av1_amf,
    encoder_availability.av1_vulkan,
    if !params.config.hardware_accel {
        "  (hardware acceleration is off: hardware encoders were not probed)"
    } else if params.config.encoder == "cpu" {
        "  (cpu preference: hardware encoders were not probed)"
    } else {
        ""
    }
    );
    if !hw_errors.is_empty() {
        debug!("  --- Encoder Errors ---");
        for error in &hw_errors {
            debug!("    {}", error);
        }
    }
    debug!("  Selected encoder: {}", if vulkan_video {
        "Vulkan Video H.264 (built-in; ffmpeg only remuxes with -c:v copy)"
    } else {
        ffmpeg_encoder
    });
    let probe_s = mark!("encoder probe");
    if !color_space.is_hdr()
        && ffmpeg_encoder == "libsvtav1"
        && encoder_type == "auto"
        && (encoder_availability.av1_nvenc
            || encoder_availability.av1_qsv
            || encoder_availability.av1_amf
            || encoder_availability.av1_vulkan)
    {
        debug!(
            "  Note: AV1 uses the CPU encoder (SVT-AV1) because hardware AV1 quality is clearly weaker."
        );
        debug!("  Note: it is much slower 閳?select the nvenc/amf encoder explicitly if you want speed.");
    }
    debug!("=========================");

    let ffmpeg_preset = match ffmpeg_encoder {
        "h264_amf" | "hevc_amf" | "av1_amf" => "-quality",
        "h264_vulkan" | "hevc_vulkan" | "av1_vulkan" => "-preset",
        "libaom-av1" => "-cpu-used",
        "librav1e" => "-speed",
        _ => "-preset",
    };

    fn speed_rank(word: &str) -> u32 {
        match word {
            "ultrafast" | "veryfast" => 0,
            "faster" => 1,
            "fast" => 2,
            "medium" => 3,
            "slow" => 4,
            "slower" => 5,
            "veryslow" => 6,
            _ => 3,
        }
    }

    let preset_words: Vec<&str> = params.config.ffmpeg_preset.split_whitespace().collect();
    let ffmpeg_preset_name: String = match ffmpeg_encoder {
        "h264_nvenc" | "hevc_nvenc" | "av1_nvenc" => {
            match preset_words.get(1) {
                Some(w) if w.len() >= 2 && w.starts_with('p') && w[1..].chars().all(|c| c.is_ascii_digit()) => {
                    (*w).to_string()
                }
                Some(w) => format!("p{}", speed_rank(w) + 1),
                None => format!("p{}", speed_rank(preset_words.first().copied().unwrap_or("medium")) + 1),
            }
        }
        "h264_qsv" | "hevc_qsv" | "av1_qsv" => preset_words
            .first()
            .copied()
            .unwrap_or("medium")
            .to_string(),
        "h264_amf" | "hevc_amf" | "av1_amf" => match preset_words.get(2) {
            Some(w) => (*w).to_string(),
            None => match speed_rank(preset_words.first().copied().unwrap_or("medium")) {
                0..=2 => "speed",
                3 | 4 => "balanced",
                _ => "quality",
            }
            .to_string(),
        },
        "h264_vulkan" | "hevc_vulkan" | "av1_vulkan" => preset_words
            .first()
            .copied()
            .unwrap_or("default")
            .to_string(),
        "libsvtav1" => match speed_rank(preset_words.first().copied().unwrap_or("medium")) {
            0 | 1 => "12",
            2 => "10",
            3 => "8",
            4 => "6",
            5 => "4",
            _ => "2",
        }
        .to_string(),
        "libaom-av1" => match speed_rank(preset_words.first().copied().unwrap_or("medium")) {
            0 | 1 => "8",
            2 => "7",
            3 => "6",
            4 => "5",
            5 => "4",
            _ => "3",
        }
        .to_string(),
        "librav1e" => match speed_rank(preset_words.first().copied().unwrap_or("medium")) {
            0 | 1 => "10",
            2 => "9",
            3 => "8",
            4 => "7",
            5 => "6",
            _ => "5",
        }
        .to_string(),
        _ => preset_words
            .first()
            .copied()
            .unwrap_or("medium")
            .to_string(),
    };

    if vulkan_video {
        debug!("  Speed preset: (built-in Vulkan Video encoder; the ffmpeg preset is not used)");
    } else if ffmpeg_encoder.ends_with("_vulkan") {
        debug!("  Speed preset: (vulkan encoder takes no preset, ignoring the configured one)");
    } else {
        debug!("  Speed preset: {} {}", ffmpeg_preset, ffmpeg_preset_name);
    }

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

    if !color_space.is_hdr() && prores.is_none() && params.config.hardware_accel && encoder_type != "cpu" {
        let dx_ok = dx_selected.is_some();
        let any4 = |a: bool, b: bool, c: bool, d: bool| a || b || c || d || dx_ok;

        let h264_supported = any4(
            encoder_availability.h264_nvenc,
            encoder_availability.h264_qsv,
            encoder_availability.h264_amf,
            encoder_availability.h264_vulkan,
        );
        let hevc_supported = any4(
            encoder_availability.hevc_nvenc,
            encoder_availability.hevc_qsv,
            encoder_availability.hevc_amf,
            encoder_availability.hevc_vulkan,
        );
        let av1_supported = any4(
            encoder_availability.av1_nvenc,
            encoder_availability.av1_qsv,
            encoder_availability.av1_amf,
            encoder_availability.av1_vulkan,
        );

        // The built-in Vulkan Video encoder is the hardware encoder here; the
        // ffmpeg probes above were skipped on purpose, so do not treat their
        // empty result as "no hardware acceleration".
        let codec_unsupported = !vulkan_video
            && match params.config.video_codec.as_str() {
                "h264" => !h264_supported,
                "hevc" => !hevc_supported,
                "av1" => !av1_supported,
                _ => false,
            };

        if codec_unsupported {
            let yesno = |b: bool| if b { "SUCCESS" } else { "FAILED" };
            let mut msg = format!("{}\n", tl!("no-hwacc"));

            let _ = write!(
                msg,
                "Hardware detection summary:\n\
             - NVIDIA: {}\n\
             - Intel Quick Sync: {}\n\
             - AMD AMF: {}\n\
             - Vulkan: {}\n\n",
                hw_detected.h264_nvenc,
                hw_detected.h264_qsv,
                hw_detected.h264_amf,
                hw_detected.h264_vulkan,
            );

            msg += "Encoder test results:\n";
            for (name, ok) in [
                ("h264_nvenc",  encoder_availability.h264_nvenc),
                ("hevc_nvenc",  encoder_availability.hevc_nvenc),
                ("av1_nvenc",   encoder_availability.av1_nvenc),
                ("h264_qsv",    encoder_availability.h264_qsv),
                ("hevc_qsv",    encoder_availability.hevc_qsv),
                ("av1_qsv",     encoder_availability.av1_qsv),
                ("h264_amf",    encoder_availability.h264_amf),
                ("hevc_amf",    encoder_availability.hevc_amf),
                ("av1_amf",     encoder_availability.av1_amf),
                ("h264_vulkan", encoder_availability.h264_vulkan),
                ("hevc_vulkan", encoder_availability.hevc_vulkan),
                ("av1_vulkan",  encoder_availability.av1_vulkan),
                ("h264_cuvid",  encoder_availability.h264_cuvid),
                ("hevc_cuvid",  encoder_availability.hevc_cuvid),
                ("av1_cuvid",   encoder_availability.av1_cuvid),
            ] {
                let _ = writeln!(msg, "- {}: {}", name, yesno(ok));
            }
            msg.push('\n');

            if hw_errors.is_empty() {
                msg += "No hardware encoders were tested (all detection failed).\n\n";
            } else {
                msg += "Detailed error logs:\n";
                for (i, error) in hw_errors.iter().enumerate() {
                    let _ = writeln!(msg, "{}. {}", i + 1, error);
                }
                msg.push('\n');
            }

            // `auto` (the default) means "use hardware if it works, otherwise
            // software". Failing the whole render on a machine without NVENC/QSV/AMF
            // made the default configuration unusable, even though libx264 was
            // right there. An explicitly chosen hardware encoder still fails.
            if matches!(params.config.encoder.as_str(), "auto" | "vulkan") {
                warn!("{}", msg.trim_end());
                warn!("Falling back to software encoding");
            } else {
                bail!(msg);
            }
        }
    }


    if vulkan_video {
        debug!("Readback: none (Vulkan encoder reads GPU NV12 directly)");
    } else if nv12_mode {
        debug!(
            "Readback: Vulkan NV12 {:.2} MB/frame, async ring (no per-frame fence wait)",
            (vw as f64 * vh as f64 * 1.5) / 1048576.
        );
    } else {
        debug!(
            "Readback: RGBA {:.2} MB/frame, one transfer per frame",
            (vw as f64 * vh as f64 * 4.) / 1048576.
        );
    }

    let video_url = "-".to_owned();
    let global_args = "-y";
    let mut input_args = String::new();
    if vulkan_video {
        write!(&mut input_args, "-f h264 -r {fps} -i {video_url} -i")?;
    } else if nv12_mode {
        write!(
            &mut input_args,
            "-f rawvideo -c:v rawvideo -s {vw}x{vh} -r {fps} -pix_fmt nv12"
        )?;
        if params.config.ffmpeg_thread {
            input_args.push_str(" -thread_queue_size 2048");
        }
        write!(&mut input_args, " -i {video_url} -i")?;
    } else {
        let pixel_format = if color_space.is_hdr() { "gbrpf32le" } else { "rgba" };
        write!(
            &mut input_args,
            "-f rawvideo -c:v rawvideo -s {vw}x{vh} -r {fps} -pix_fmt {pixel_format}"
        )?;
        if params.config.ffmpeg_thread {
            input_args.push_str(" -thread_queue_size 2048");
        }
        write!(&mut input_args, " -i {video_url} -i")?;
    }

    let video = params.config.output_extension();

    let null_sink = std::env::var_os("PHITK_NULL_SINK").is_some();
    let video_mux = if null_sink { "null" } else { video };

    let is_vulkan_encoder = ffmpeg_encoder.ends_with("_vulkan");
    let is_hw_encoder = matches!(
        ffmpeg_encoder,
        "h264_nvenc"
            | "hevc_nvenc"
            | "av1_nvenc"
            | "h264_qsv"
            | "hevc_qsv"
            | "av1_qsv"
            | "h264_amf"
            | "hevc_amf"
            | "av1_amf"
    );
    let dx = dx_selected;
    let vf_arg = if let Some(profile) = prores {
        if ffmpeg_encoder == PRORES_VULKAN_ENCODER {
            prores_vulkan_video_args(profile, color_space)
        } else {
            let filter = hdr_filter
                .clone()
                .unwrap_or_else(|| format!("format={}", profile.pixel_format));
            format!("-vf {filter} -profile:v {} -alpha_bits {}",
                profile.number, if profile.alpha { 16 } else { 0 })
        }
    } else if let Some(filter) = hdr_filter.as_ref() {
        format!("-vf {filter}")
    } else if nv12_mode {
        // The pipe already carries NV12.  Only the GPU-upload encoders need a
        // filter; software encoders negotiate NV12 directly.
        if dx.is_some() || is_vulkan_encoder {
            "-vf hwupload".to_owned()
        } else {
            String::new()
        }
    } else if dx.is_some() || is_vulkan_encoder {
        "-vf format=nv12,hwupload".to_owned()
    } else if is_hw_encoder {
        "-vf format=nv12".to_owned()
    } else {
        "-vf format=yuv420p".to_owned()
    };

    let mut out_extra = vf_arg;
    if ffmpeg_encoder == "libaom-av1" {
        out_extra.push_str(" -row-mt 1");
    }
    if let Some(enc) = &dx {
        out_extra.push_str(&format!(" -pix_fmt {}", enc.pix));
        out_extra.push_str(" -bf 0");

    }

    let is_crf = params.config.bitrate_control == "CRF";
    let av1_quality_extra: &str = match ffmpeg_encoder {
        "libsvtav1" => " -svtav1-params tune=0:enable-tf=1:aq-mode=2",

        // Not using it for now because it's too slow
        "libaom-av1" => {
            if is_crf { " -b:v 0 -aq-mode 1 -enable-restoration 1 -lag-in-frames 35" }
            else {
                " -aq-mode 1 -enable-restoration 1 -lag-in-frames 35"
            }
        }
        "av1_amf" => " -bf 2",
        _ => "",
    };
    out_extra.push_str(av1_quality_extra);
    if !av1_quality_extra.is_empty() {
        debug!("  AV1 quality:{}", av1_quality_extra);
        if speed_rank(preset_words.first().copied().unwrap_or("medium")) <= 3 {
            debug!("  Note: for the best AV1 quality pick the VerySlow speed preset (SVT-AV1 preset 2)");
        }
    }
    if ffmpeg_encoder != PRORES_VULKAN_ENCODER {
        if color_space.is_hdr() || color_space == ExportColorSpace::Srgb {
            out_extra.push(' ');
            out_extra.push_str(color_space.ffmpeg_signaling());
        } else {
            out_extra.push_str(if vh > 576 {
                " -colorspace bt709 -color_primaries bt709 -color_trc bt709 -color_range tv"
            } else {
                " -colorspace smpte170m -color_primaries smpte170m -color_trc smpte170m -color_range tv"
            });
        }
    }
    if color_space.is_hdr() && ffmpeg_encoder != PRORES_VULKAN_ENCODER {
        out_extra.push_str(" -pix_fmt ");
        out_extra.push_str(hdr_encoder_pixel_format(ffmpeg_encoder, prores));
    } else if dx.is_none()
        && matches!(
            ffmpeg_encoder,
            "libsvtav1" | "libaom-av1" | "librav1e" | "av1_nvenc"
        )
    {
        out_extra.push_str(" -pix_fmt yuv420p10le");
    }

    let async_depth = "-async_depth 4"; // the "4" parallel process 4 frames (vulkan encoder)
    // The temporary audio already has the requested codec and sample rate.
    // FLAC stays in MP4; ProRes MOV uses compatible AAC, MP3 or PCM.
    let mux_audio = if video == "mp4" && audio_format == "wav" {
        audio_mux_args.as_str()
    } else {
        "-c:a copy -strict -2"
    };
    let args2 = if null_sink {
        "-c:v rawvideo -map 0:v:0 -f null".to_string()
    } else if vulkan_video {
        let duration = out_frames as f64 / fps.max(1) as f64;
        format!(
            "{mux_audio} -c:v copy -map 0:v:0 -map 1:a:0 -t {duration:.6} -f {video_mux}",
        )
    } else if prores.is_some() {
        format!("{mux_audio} -c:v {ffmpeg_encoder} -map 0:v:0 -map 1:a:0 -shortest {out_extra} -f mov")
    } else if is_vulkan_encoder {
        format!(
            "{} -c:v {} {} {} {} -map 0:v:0 -map 1:a:0 -shortest {} {} {} -f {}",
            mux_audio,
            ffmpeg_encoder,
            bitrate_control,
            params.config.bitrate,
            ffmpeg_preset,
            ffmpeg_preset_name,
            out_extra,
            async_depth,
            video_mux,
        )
    } else {
        format!(
            "{} -c:v {} {} {} {} {} -map 0:v:0 -map 1:a:0 -shortest {} -f {}",
            mux_audio,
            ffmpeg_encoder,
            bitrate_control,
            params.config.bitrate,
            ffmpeg_preset,
            ffmpeg_preset_name,
            out_extra,
            video_mux,
        )
    };

    let ffmpeg_frames = std::sync::Arc::new(AtomicU64::new(0));
    debug!("ffmpeg video args: {}", args2);
    let mut proc = {
        let mut cmd = cmd_hidden(&ffmpeg);
        cmd.args(global_args.split_whitespace());
        cmd.args(["-progress", "pipe:2", "-nostats"]);
        if is_vulkan_encoder && !vulkan_video {
            cmd.args(FFMPEG_VULKAN_DEVICE_ARGS);
        } else if let Some(enc) = dx.as_ref().filter(|_| !vulkan_video) {
            cmd.arg("-init_hw_device")
                .arg(format!("{}={}", enc.hw, DX_DEVICE))
                .arg("-filter_hw_device")
                .arg(DX_DEVICE);
        }
        cmd.args(input_args.split_whitespace())
            .arg(audio_output_file.path())
            .args(args2.split_whitespace())
            .arg(&output_path)
            .arg("-loglevel")
            .arg("error")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| tl!("run-ffmpeg-failed"))?
    };


    let mut proc = control.process(proc);
    let video_stderr = drain_stderr_with_progress(proc.stderr.take().unwrap(), ffmpeg_frames.clone());

    let hdr_readback = color_space.is_hdr();
    let frame_size = vw as usize * vh as usize * if hdr_readback { 8 } else { 4 };

    let mut ffmpeg_stdin = Some(proc.stdin.take().unwrap());
    let mut prores_pipe = if let Some((packer, readback)) = prores_staging.take() {
        let writer = unsafe { crate::frame_handoff::FrameHandoff::new_mapped(
            readback.addresses(), readback.frame_bytes(), ffmpeg_stdin.take().unwrap(),
        ) }?;
        info!("ProRes conversion: GPU straight-alpha RGBA, async mapped handoff; {} bytes/frame", readback.frame_bytes());
        Some(ProresPipe { writer, readback, packer, pending: [None, None], current_read: None })
    } else { None };
    let keyframe_interval = (fps.saturating_mul(KEYFRAME_INTERVAL_SECS)).max(1) as u64;

    let (mut vulkan_pipe, pipeline, writer, encoder_thread, mut nv12_pipe, nv12_pipeline) =
        if prores_pipe.is_some() {
            (None, None, None, None, None, None)
        } else if let Some(context) = video_context.take() {
            let nv12_raw = (0..context.source_count())
                .map(|slot| context.nv12_raw(slot))
                .collect::<Vec<_>>();

            let slot_count = nv12_raw.len();
            let (job_tx, job_rx) = mpsc::channel::<(u64, usize)>();
            let (result_tx, result_rx) = mpsc::channel::<(u64, Option<Vec<u8>>)>();
            let (free_tx, free_rx) = mpsc::channel::<usize>();
            let context = SendVideoContext(context);

            let encoder_thread = std::thread::Builder::new()
                .name("vulkan-encode".to_owned())
                .spawn(move || {
                    let mut context = context.0;
                    for (index, slot) in job_rx {
                        while context.encoded_in_flight() >= 2 {
                            if let Some((frame_index, data, source_slot)) =
                                context.collect_encoded()
                            {
                                result_tx.send((frame_index, Some(data))).ok();
                                free_tx.send(source_slot).ok();
                            } else {
                                break;
                            }
                        }
                        let force_keyframe = index % keyframe_interval == 0;
                        if context
                            .submit_encode_slot(slot, index, force_keyframe)
                            .is_none()
                        {
                            result_tx.send((index, None)).ok();
                            free_tx.send(slot).ok();
                        }
                    }
                    while let Some((frame_index, data, source_slot)) = context.collect_encoded()
                    {
                        result_tx.send((frame_index, Some(data))).ok();
                        free_tx.send(source_slot).ok();
                    }
                })
                .context("failed to spawn Vulkan encoder thread")?;
            let mut stdin = ffmpeg_stdin.take().unwrap();

            let writer = std::thread::Builder::new()
                .name("ffmpeg-writer".to_owned())
                .spawn(move || -> std::io::Result<u64> {
                    let mut busy: u64 = 0;
                    let mut next = 0u64;
                    let mut pending = BTreeMap::new();
                    while let Ok((index, data)) = result_rx.recv() {
                        let Some(data) = data else {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::Other,
                                "Vulkan encoder produced no bitstream",
                            ));
                        };
                        pending.insert(index, data);
                        while let Some(data) = pending.remove(&next) {
                            let t = Instant::now();
                            stdin.write_all(&data)?;
                            busy += t.elapsed().as_nanos() as u64;
                            next += 1;
                        }
                    }
                    let t = Instant::now();
                    stdin.flush()?;
                    busy += t.elapsed().as_nanos() as u64;
                    Ok(busy)
                })
                .context("failed to spawn ffmpeg writer thread")?;
            (
                Some(VulkanPipe {
                    job_tx,
                    nv12_raw,
                    free: (0..slot_count).collect(),
                    free_rx,
                }),
                None,
                Some(writer),
                Some(encoder_thread),
                None,
                None,
            )
        } else if let Some(readback) = ffmpeg_nv12.take() {
            let ptrs = readback.writer_ptrs();
            let frame_bytes = readback.frame_size();
            let slot_count = readback.slot_count();
            // The previous memcpy handoff remains available for same-binary
            // measurements and drivers where direct mapped reads are slower.
            let direct_mode = std::env::var("PHITK_NV12_HANDOFF")
                .map_or(true, |mode| !mode.eq_ignore_ascii_case("copy"));
            let (writer, direct, nv12_pipeline) = if direct_mode {
                let stdin = ffmpeg_stdin.take().unwrap();
                // Nv12Pipe owns the allocations and drops this RAII handoff
                // first. submit is only called after the frame fence is waited.
                let handoff = unsafe {
                    crate::frame_handoff::FrameHandoff::new_mapped(ptrs.clone(), frame_bytes, stdin)
                }.context("failed to spawn direct NV12 writer")?;
                debug!("NV12 handoff: direct mapped staging, {} slots", slot_count);
                (None, Some(handoff), None)
            } else {
                let mut pipeline = FramePipeline::new(frame_bytes);
                pipeline.finish_init();
                let pipeline = std::sync::Arc::new(pipeline);
                let mut stdin = ffmpeg_stdin.take().unwrap();
                let shared = std::sync::Arc::clone(&pipeline);
                let writer = std::thread::Builder::new()
                    .name("ffmpeg-writer".to_owned())
                    .spawn(move || -> std::io::Result<u64> {
                        let _closed = FrameWriterClosed(&shared);
                        let mut busy: u64 = 0;
                        while let Some(buf) = shared.pop_job() {
                            let t = Instant::now();
                            stdin.write_all(&shared.pool[buf])?;
                            let d = t.elapsed().as_nanos() as u64;
                            busy += d;
                            shared.write_ns.fetch_add(d, Ordering::Relaxed);
                            shared.write_count.fetch_add(1, Ordering::Relaxed);
                            shared.write_max_ns.fetch_max(d, Ordering::Relaxed);
                            shared.release_buf(buf);
                        }
                        let t = Instant::now();
                        stdin.flush()?;
                        busy += t.elapsed().as_nanos() as u64;
                        Ok(busy)
                    })
                    .context("failed to spawn ffmpeg writer thread")?;
                (Some(writer), None, Some(pipeline))
            };
            (
                None,
                None,
                writer,
                None,
                Some(Nv12Pipe {
                    direct,
                    readback,
                    free: (0..slot_count).collect(),
                    ptrs,
                    frame_bytes,
                    pending: std::array::from_fn(|_| None),
                    current_read: None,
                }),
                nv12_pipeline,
            )
        } else {
            let mut pipeline = FramePipeline::new(frame_size);

            pipeline.finish_init();

            let pipeline = std::sync::Arc::new(pipeline);
            let mut stdin = ffmpeg_stdin.take().unwrap();
            let shared = std::sync::Arc::clone(&pipeline);

            let writer = std::thread::Builder::new()
                .name("ffmpeg-writer".to_owned())
                .spawn(move || -> std::io::Result<u64> {
                    let _closed = FrameWriterClosed(&shared);
                    let mut busy = 0_u64;
                    // Reuse one planar frame on the writer thread. The render
                    // thread can submit the next RGBA16F frame while this runs.
                    let mut planar = if hdr_readback { vec![0; frame_size / 8 * 12] } else if alpha { vec![0; frame_size] } else { Vec::new() };
                    while let Some(buf) = shared.pop_job() {
                        let pixels = &shared.pool[buf];
                        let bytes = if hdr_readback {
                            hdr_rgba_to_gbr(pixels, &mut planar);
                            planar.as_slice()
                        } else if alpha {
                            rgba_unpremultiply(pixels, &mut planar);
                            planar.as_slice()
                        } else {
                            pixels.as_slice()
                        };
                        let t = Instant::now();
                        stdin.write_all(bytes)?;
                        let d = t.elapsed().as_nanos() as u64;
                        busy += d;
                        shared.write_ns.fetch_add(d, Ordering::Relaxed);
                        shared.write_count.fetch_add(1, Ordering::Relaxed);
                        shared.write_max_ns.fetch_max(d, Ordering::Relaxed);
                        shared.release_buf(buf);
                    }
                    let t = Instant::now();
                    stdin.flush()?;
                    busy += t.elapsed().as_nanos() as u64;
                    Ok(busy)
                })
                .context("failed to spawn ffmpeg writer thread")?;
            (None, Some(pipeline), Some(writer), None, None, None)
        };

    fn submit_frame(pipeline: &FramePipeline, target: &RenderTarget, rgba: Option<&RgbaReadback>, frame_size: usize) -> Result<(f64, f64)> {
        let acquire_start = Instant::now();
        let buf = pipeline.acquire_buf()?;
        let pool_wait = acquire_start.elapsed().as_secs_f64();
        let base = pipeline.pool_addrs[buf] as *mut u8;

        unsafe { get_internal_gl() }.flush();
        let quad_context = miniquad_context();
        let dma_start = Instant::now();
        let out = unsafe { std::slice::from_raw_parts_mut(base, frame_size) };
        if let Some(rgba) = rgba {
            quad_context.copy_render_target(target.render_pass, rgba.0.render_pass);
            rgba.0.texture.raw_miniquad_texture_handle().read_pixels(quad_context, out);
        } else {
            target.texture.raw_miniquad_texture_handle().read_pixels(quad_context, out);
        }
        let dma_wait = dma_start.elapsed().as_secs_f64();

        pipeline.push_job(buf);
        Ok((dma_wait, pool_wait))
    }

    let mut operation = control.scope();
    struct StopFrames(Option<std::sync::Arc<FramePipeline>>);
    impl Drop for StopFrames { fn drop(&mut self) { if let Some(pool) = &self.0 { pool.push_stop(); } } }
    let _stop_frames = StopFrames(pipeline.clone());
    let _stop_nv12 = StopFrames(nv12_pipeline.clone());
    let total_frames = frames;

    // Crate-based progress bar (indicatif). It draws on stderr, so terminal
    // log lines on stdout stay readable. The GUI/server child passes
    // `log_mode == false` and reports progress through IPC instead, so it gets
    // a hidden bar.
    let progress = if log_mode && log::log_enabled!(log::Level::Info) {
        let pb = ProgressBar::with_draw_target(
            Some(out_frames),
            ProgressDrawTarget::stderr_with_hz(20),
        );
        pb.set_style(
            ProgressStyle::default_bar()
                .template(&format!("{}{{spinner}} [{{elapsed_precise}}] [{{bar:32}}] {{pos}}/{{len}} ({{percent}}%) ETA {{eta_precise}}", crate::logger::prefix(log::Level::Info)))
                .unwrap()
                .progress_chars("#>-"),
        );
        pb
    } else {
        ProgressBar::hidden()
    };
    // log
    let mut dma_wait_time: f64 = 0.0;
    let mut output_index:u64 = 0;
    let mut pool_wait_time: f64 = 0.0;
    let mut slot_wait_time: f64 = 0.0;
    let mut fence_wait_time: f64 = 0.0;
    let mut update_time: f64 = 0.0;
    let mut scene_render_time: f64 = 0.0;
    let mut yuv_convert_time: f64 = 0.0;
    let mut nv12_submit_time: f64 = 0.0;
    let mut copy_time: f64 = 0.0;
    let gl = unsafe { get_internal_gl() };
    let spawn_s = mark!("ffmpeg spawn + threads");
    send(IPCEvent::StartRender(out_frames));
    info!("Rendering video ({} frames)", out_frames);
    let loop_start_time = Instant::now();

    for frame in 0..total_frames {
        control.check()?;
        let current_frame_time = frame as f64 * frame_duration;
        *my_time.borrow_mut() = current_frame_time;
        let keep = frame >= skip;
        let mut nv12_frame_slot = None;

        if keep {
            let mst_index = if let Some(pipe) = &mut prores_pipe {
                let context = miniquad_context().vulkan_context().unwrap();
                let start = Instant::now();
                let frame_slot = context.begin_offscreen_frame();
                fence_wait_time += start.elapsed().as_secs_f64();
                if let Some((index, slot)) = pipe.pending[frame_slot].take() { pipe.writer.submit(index, slot)?; }
                let start = Instant::now();
                pipe.current_read = Some(pipe.writer.acquire_slot()?);
                pool_wait_time += start.elapsed().as_secs_f64();
                nv12_frame_slot = Some(frame_slot);
                frame_slot
            } else if let Some(pipe) = &mut nv12_pipe {
                let context = miniquad_context().vulkan_context().unwrap();
                let wait_start = Instant::now();
                let frame_slot = context.begin_offscreen_frame();
                fence_wait_time += wait_start.elapsed().as_secs_f64();

                if let Some((index, read_slot)) = pipe.pending[frame_slot].take() {
                    let (wait, copy) = pipe.hand_off_completed(index, read_slot, nv12_pipeline.as_deref())?;
                    pool_wait_time += wait;
                    copy_time += copy;
                }
                let (read_slot, wait) = pipe.acquire_readback()?;
                pool_wait_time += wait;
                pipe.current_read = Some(read_slot);
                nv12_frame_slot = Some(frame_slot);
                frame_slot
            } else if headless_vulkan {
                miniquad_context().vulkan_context().unwrap().begin_offscreen_frame();
                0
            } else {
                0
            };
            mst_slot.set(mst_index);
            let input = msts[mst_index].input();
            gl.quad_gl.render_pass(Some(input.render_pass));
        }
        // Frame 0 was already updated by the pre-roll above.
        if frame != 0 {
            let update_start = Instant::now();
            main.update()?;
            update_time += update_start.elapsed().as_secs_f64();
        }
        if !keep {
            continue;
        }
        let render_start = Instant::now();
        main.render(&mut painter)?;
        let rendered = rhythm.compose(
            mst_slot.get(),
            &msts[mst_slot.get()],
            main.scenes.last().and_then(|scene| scene.chart_time()),
            &mut painter,
        );
        scene_render_time += render_start.elapsed().as_secs_f64();
        if current_frame_time <= LoadingScene::TOTAL_TIME as f64 && !params.config.disable_loading {
            draw_rectangle(0., 0., 0., 0., Color::default());
        }

        // Move the bar on every frame; indicatif throttles the actual redraw
        // (1 Hz), and the old code only advanced it once per 10% of the frames,
        // which made a 30 s render look like it updated every ~3 s.
        progress.set_position(frame.saturating_sub(skip).min(out_frames));

        if let Some(pipe) = &mut prores_pipe {
            let start = Instant::now();
            pipe.packer.convert(&rendered);
            yuv_convert_time += start.elapsed().as_secs_f64();
            let slot = pipe.current_read.take().unwrap();
            let start = Instant::now();
            pipe.packer.record(pipe.readback.buffer(slot));
            miniquad_context().vulkan_context().unwrap().finish_offscreen_frame();
            nv12_submit_time += start.elapsed().as_secs_f64();
            pipe.pending[nv12_frame_slot.unwrap()] = Some((output_index, slot));
            output_index += 1;
        } else if let Some(pipe) = &mut nv12_pipe {
            let frame_slot = nv12_frame_slot.unwrap();
            let read_slot = pipe.current_read.take().unwrap();
            {
                let context = miniquad_context().vulkan_context().unwrap();
                let yuv = &yuvs[frame_slot];
                let convert_start = Instant::now();
                yuv.convert(&rendered);
                yuv_convert_time += convert_start.elapsed().as_secs_f64();
                let submit_start = Instant::now();
                context.record_yuv_copy_to_staging(
                    yuv.y_index,
                    yuv.uv_index,
                    pipe.readback.buffer_raw(read_slot),
                    vw,
                    vh,
                );
                context.finish_offscreen_frame();
                nv12_submit_time += submit_start.elapsed().as_secs_f64();
            }
            pipe.pending[frame_slot] = Some((output_index, read_slot));
            output_index += 1;
        } else if let Some(pipe) = &mut vulkan_pipe {
            let wait_start = Instant::now();
            let slot = pipe
                .free
                .pop_front()
                .or_else(|| pipe.free_rx.recv().ok())
                .unwrap();
            slot_wait_time += wait_start.elapsed().as_secs_f64();
            if let Some(yuv) = yuvs.first() {
                yuv.convert(&rendered);
                {
                    let context = miniquad_context().vulkan_context().unwrap();
                    context.copy_nv12_from_textures(
                        yuv.y_index,
                        yuv.uv_index,
                        pipe.nv12_raw[slot],
                    );
                }
                pipe.job_tx.send((output_index, slot)).ok();
                output_index += 1;
            }
        } else {
            let (dma, pool) =
                submit_frame(pipeline.as_ref().unwrap(), &rendered, rgba_readback.as_ref(), frame_size)?;
            if headless_vulkan {
                miniquad_context().vulkan_context().unwrap().finish_offscreen_frame();
            }
            dma_wait_time += dma;
            pool_wait_time += pool;
        }
        send(IPCEvent::Frame);
    }

    let loop_wall = loop_start_time.elapsed().as_secs_f64().max(1e-9);
    mark!("render loop");
    send(IPCEvent::Finalizing);
    info!("Finalizing video");
    control.check()?;
    let backlog_at_end = output_index.saturating_sub(ffmpeg_frames.load(Ordering::Relaxed) as u64);
    drop(vulkan_pipe);
    if let Some(thread) = encoder_thread {
        thread.join().ok();
    }
    if let Some(pipeline) = &pipeline {
        pipeline.push_stop();
    }
    let mut direct_stats = None;
    if let Some(pipe) = &mut nv12_pipe {
        let context = miniquad_context().vulkan_context().unwrap();
        context.wait_offscreen_idle();
        for (index, read_slot) in crate::frame_handoff::take_pending(&mut pipe.pending) {
            let (_, copy) = pipe.hand_off_completed(index, read_slot, nv12_pipeline.as_deref())?;
            copy_time += copy;
        }
        if let Some(direct) = &mut pipe.direct {
            direct_stats = Some(direct.finish().context("failed to finish direct NV12 writer")?);
        } else if let Some(pipeline) = &nv12_pipeline {
            pipeline.push_stop();
        }
    }
    if let Some(pipe) = &mut prores_pipe {
        miniquad_context().vulkan_context().unwrap().wait_offscreen_idle();
        for (index, slot) in crate::frame_handoff::take_pending(&mut pipe.pending) { pipe.writer.submit(index, slot)?; }
        direct_stats = Some(pipe.writer.finish()?);
    }
    let writer_busy = if let Some(writer) = writer {
        match writer.join() {
            Ok(Ok(busy)) => busy as f64 / 1e9,
            Ok(Err(err)) => bail!("failed to write frames to ffmpeg: {err}"),
            Err(_) => bail!("ffmpeg writer thread panicked"),
        }
    } else {
        direct_stats.as_ref().expect("missing NV12 writer").write_ns as f64 / 1e9
    };
    let video_status = proc.wait()?;
    mark!("drain + finalize");
    let video_stderr = video_stderr.join().unwrap_or_default();
    if !video_stderr.trim().is_empty() { warn!("[ffmpeg:video]\n{}", video_stderr.trim()); }
    if !video_status.success() { bail!("ffmpeg muxing failed ({video_status})"); }
    control.check()?;
    let elapsed = render_start_time.elapsed();
    let wall = elapsed.as_secs_f64().max(1e-9);
    debug!("Render Time: {:.2?}", elapsed);
    debug!(
        "Writer thread blocked on video transport+ffmpeg: {:.2}s ({:.1}%)",
        writer_busy,
        writer_busy / wall * 100.
    );
    debug!(
        "Render thread waits: readback DMA wait {:.2}s ({:.1}%) | frame pool exhausted {:.2}s ({:.1}%) | vulkan-encoder slot wait {:.2}s ({:.1}%) | offscreen fence wait {:.2}s ({:.1}%)",
        dma_wait_time, dma_wait_time / wall * 100.,
        pool_wait_time, pool_wait_time / wall * 100.,
        slot_wait_time, slot_wait_time / wall * 100.,
        fence_wait_time, fence_wait_time / wall * 100.
    );
    let swap_wait_s = miniquad_context()
        .vulkan_context()
        .map(|c| c.frame_wait_ns as f64 / 1e9)
        .unwrap_or(0.0);
    if swap_wait_s > 0.0 {
        debug!(
            "Swapchain frame fence wait: {:.2}s ({:.1}% of scene render) -> CPU-side scene recording {:.2}s",
            swap_wait_s,
            swap_wait_s / scene_render_time.max(1e-9) * 100.,
            (scene_render_time - swap_wait_s).max(0.),
        );
    }
    if let Some(pipeline) = &nv12_pipeline {
        let frame_bytes = pipeline.pool.first().map(|b| b.len()).unwrap_or(0);
        let blocks = pipeline.acquire_blocks.load(Ordering::Relaxed);
        let acquire_ns = pipeline.acquire_wait_ns.load(Ordering::Relaxed);
        let acquire_max_ns = pipeline.acquire_max_ns.load(Ordering::Relaxed);
        let queue_max = pipeline.queue_max.load(Ordering::Relaxed);
        let writes = pipeline.write_count.load(Ordering::Relaxed).max(1);
        let write_ns = pipeline.write_ns.load(Ordering::Relaxed);
        let write_max_ns = pipeline.write_max_ns.load(Ordering::Relaxed);
        debug!(
            "NV12 handoff: pool {} x {:.1} MiB | render thread blocked on a free buffer {}x, {:.2}s (max {:.1}ms) | writer queue max {} frames | staging->pool copy {:.2}s ({:.0} fps) | writer {} writes, {:.2}s inside write_all (max {:.1}ms, {:.0} MiB/s) | backlog when loop ended {} frames",
            pipeline.pool.len(),
            frame_bytes as f64 / (1 << 20) as f64,
            blocks,
            acquire_ns as f64 / 1e9,
            acquire_max_ns as f64 / 1e6,
            queue_max,
            copy_time,
            out_frames as f64 / copy_time.max(1e-9),
            writes,
            write_ns as f64 / 1e9,
            write_max_ns as f64 / 1e6,
            (writes as f64 * frame_bytes as f64) / (write_ns as f64 / 1e9).max(1e-9) / (1 << 20) as f64,
            backlog_at_end,
        );
    }
    if let Some(stats) = direct_stats {
        debug!(
            "NV12 direct handoff: {} frames | staging->pool copy 0.00s | writer {:.2}s inside write_all/flush (max {:.1}ms/frame)",
            stats.frames, stats.write_ns as f64 / 1e9, stats.max_write_ns as f64 / 1e6,
        );
    }
    debug!(
        "prpr stage times: update {:.2}s ({:.0} fps) | scene render {:.2}s ({:.0} fps) | YUV convert {:.2}s ({:.0} fps) | NV12 copy/submit {:.2}s ({:.0} fps)",
        update_time,
        out_frames as f64 / update_time.max(1e-9),
        scene_render_time,
        out_frames as f64 / scene_render_time.max(1e-9),
        yuv_convert_time,
        out_frames as f64 / yuv_convert_time.max(1e-9),
        nv12_submit_time,
        out_frames as f64 / nv12_submit_time.max(1e-9)
    );
    let written = ffmpeg_frames.load(Ordering::Relaxed);
    progress.set_position(out_frames);
    progress.finish();
    let busy = (loop_wall - dma_wait_time - pool_wait_time - slot_wait_time - fence_wait_time)
        .max(1e-9);
    debug!(
        "Render loop Time: {:.2}s | prpr avg fps: {:.0}  (busy {:.2}s of {:.2}s, i.e. render side is idle {:.1}%)",
        loop_wall,
        out_frames as f64 / busy,
        busy,
        loop_wall,
        (1.0 - busy / loop_wall) * 100.0
    );
    let excluded_setup = mixing_s + audio_s + probe_s + spawn_s;
    let video_wall = (wall - excluded_setup).max(1e-9);
    if written > 0 {
        debug!(
            "Video Pipeline Avg FPS: {:.0} ({} frames / {:.2}s of video pipeline; {:.2}s setup excluded: audio {:.2}s, encoder probe {:.2}s, ffmpeg spawn {:.2}s)",
            written as f64 / video_wall,
            written,
            video_wall,
            excluded_setup,
            mixing_s + audio_s,
            probe_s,
            spawn_s,
        );
    } else {
        debug!("Video Pipeline Avg FPS: n/a (ffmpeg reported no progress)");
    }
    let total_wall = process_start.elapsed().as_secs_f64().max(1e-9);
    debug!(
        "End-to-End Time: {:.2}s",
        total_wall,
    );

    output_owner.commit()?;
    output_owner.cleanup()?;
    operation.complete();
    info!("Video saved to {} ({:.2}s)", final_output_path.display(), total_wall);
    send(IPCEvent::Done(render_start_time.elapsed().as_secs_f64()));
    Ok(())
}

mod yuv_shader {
    pub const VERTEX: &str = r#"#version 450 core
layout(location = 0) in vec3 position;
layout(location = 1) in vec2 texcoord;
layout(location = 2) in vec4 color0;

layout(set = 0, binding = 0) uniform Uniforms {
    mat4 Projection;
    mat4 Model;
    vec4 _Time;
};

layout(location = 0) out vec2 uv;

void main() {
    gl_Position = vec4(position.xy, 0.0, 1.0);
    uv = vec2(texcoord.x, 1.0 - texcoord.y);
}"#;

    pub const Y: &str = r#"#version 450 core
layout(set = 0, binding = 0) uniform Uniforms {
    mat4 Projection;
    mat4 Model;
    vec4 _Time;
};
layout(location = 0) in vec2 uv;
layout(set = 0, binding = 3) uniform sampler2D tex;
layout(location = 0) out vec4 FragColor;

void main() {
    vec2 size = vec2(textureSize(tex, 0));
    vec2 st = clamp(uv, vec2(0.5) / size, vec2(1.0) - vec2(0.5) / size);
    vec3 c = texture(tex, st).rgb;
    float y = (16.0 + 219.0 * (0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b)) / 255.0;
    FragColor = vec4(y, 0.0, 0.0, 1.0);
}"#;

    pub const UV: &str = r#"#version 450 core
layout(set = 0, binding = 0) uniform Uniforms {
    mat4 Projection;
    mat4 Model;
    vec4 _Time;
};
layout(location = 0) in vec2 uv;
layout(set = 0, binding = 3) uniform sampler2D tex;
layout(location = 0) out vec4 FragColor;

void main() {
    vec2 size = vec2(textureSize(tex, 0));
    vec2 st = clamp(uv, vec2(0.5) / size, vec2(1.0) - vec2(0.5) / size);
    vec3 c = texture(tex, st).rgb;
    float u = (128.0 + 224.0 * (-0.114572 * c.r - 0.385428 * c.g + 0.5 * c.b)) / 255.0;
    float v = (128.0 + 224.0 * (0.5 * c.r - 0.454153 * c.g - 0.045847 * c.b)) / 255.0;
    FragColor = vec4(u, v, 0.0, 1.0);
}"#;
}

use clap::Parser;

/// CLI 参数结构
#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct CliArgs {
    /// Chart archive (.pez / .zip / .pgr / .rpe)
    #[arg(short, long)]
    pub input: String,
    /// Output video path
    #[arg(short, long, default_value = "output.mp4")]
    pub output: String,
    /// Resolution, e.g. 1920x1440
    #[arg(short = 'r', long, default_value = "1920x1440")]
    pub resolution: String,
    /// CRF / QP value (1-51)
    #[arg(short = 'c', long, default_value_t = 20)]
    pub crf: u32,
    /// Frame rate
    #[arg(short = 'f', long, default_value_t = 30)]
    pub fps: u32,
    /// Assets folder (falls back to the bundled assets if missing)
    #[arg(long, default_value = "/usr/lib/ptkc-assets")]
    pub assets: String,

    // ===== UI / scene =====
    /// Background dimming percentage (0-100)
    #[arg(short = 'd', long, default_value_t = 30)]
    pub dim: u8,
    /// Show the result (ending) screen
    #[arg(short = 'R', long, default_value_t = false)]
    pub result: bool,
    /// Show the loading animation
    #[arg(short = 'l', long, default_value_t = false)]
    pub load: bool,
    /// Result screen length in seconds (defaults to 3 with --result)
    #[arg(long)]
    pub ending: Option<f64>,
    /// Player name shown in the UI
    #[arg(short = 'n', long)]
    pub name: Option<String>,
    /// Player rks shown in the UI
    #[arg(long, default_value_t = 6.16)]
    pub rks: f32,
    /// Player avatar image path
    #[arg(long)]
    pub avatar: Option<String>,
    /// Combo text
    #[arg(long, default_value = "AUTOPLAY")]
    pub combo: String,
    /// Watermark text (empty to disable)
    #[arg(short = 'w', long)]
    pub watermark: Option<String>,
    /// UI elements to show, comma separated: score,combo,name,level,line,pb,pause
    /// (also accepts `all` / `none`). Unlisted elements are hidden.
    #[arg(long)]
    pub ui: Option<String>,
    /// UI elements to hide, comma separated (applied after --ui)
    #[arg(long)]
    pub hide_ui: Option<String>,
    /// Draw a progress bar
    #[arg(long, default_value_t = false)]
    pub bar: bool,
    /// Show the score target
    #[arg(long, default_value_t = false)]
    pub score_aim: bool,
    /// Draw the UI above 噪域 (blockAreaList) instead of under it
    #[arg(long, default_value_t = false)]
    pub ui_above_ba: bool,
    /// Show chart debug info
    #[arg(long, default_value_t = false)]
    pub chart_debug: bool,
    /// Mirror the chart horizontally
    #[arg(long, default_value_t = false)]
    pub flid_x: bool,
    /// Chart stretch ratio (1.0 = square)
    #[arg(long, default_value_t = 1.0)]
    pub chart_ratio: f32,
    /// Show the chart illustration as the background
    #[arg(long)]
    pub background: bool,
    /// Resource pack path
    #[arg(long)]
    pub respack: Option<String>,

    // ===== gameplay / effects =====
    /// Chart speed multiplier
    #[arg(short = 's', long, default_value_t = 1.0)]
    pub speed: f32,
    /// Note scale
    #[arg(long, default_value_t = 1.0)]
    pub note_scale: f32,
    /// Music volume (0-1)
    #[arg(long, default_value_t = 1.0)]
    pub volume_music: f32,
    /// SFX volume (0-1)
    #[arg(long, default_value_t = 1.0)]
    pub volume_sfx: f32,
    /// Enable FXAA
    #[arg(long, default_value_t = false)]
    pub fxaa: bool,
    /// Disable particle effects
    #[arg(long, default_value_t = false)]
    pub no_particle: bool,
    /// Disable shader/background effects
    #[arg(long, default_value_t = false)]
    pub no_effect: bool,
    /// Disable the doubled hint line
    #[arg(long, default_value_t = false)]
    pub no_double_hint: bool,
    /// Aggressive mode
    #[arg(long, default_value_t = false)]
    pub aggressive: bool,
    /// Split hold notes between hands
    #[arg(long, default_value_t = false)]
    pub hand_split: bool,
    /// MSAA sample count (1, 2, 4, 8, ...)
    #[arg(long, default_value_t = 4)]
    pub sample_count: u32,

    // ===== encoder =====
    /// Video codec: h264 / hevc / av1
    #[arg(long, default_value = "h264")]
    pub codec: String,
    /// Encoder: auto / nvenc / qsv / amf / vulkan / dx12 / cpu
    #[arg(short = 'e', long, default_value = "auto")]
    pub encoder: String,
    /// FFmpeg preset (veryfast/fast/medium/slow/...)
    #[arg(short = 'p', long, default_value = "medium")]
    pub preset: String,
    /// Container: mp4 / mov
    #[arg(long, default_value = "mp4")]
    pub format: String,
    /// Rate control: CRF / CBR
    #[arg(long, default_value = "CRF")]
    pub bitrate_control: String,
    /// Explicit bitrate (overrides --crf), e.g. 8000k
    #[arg(short = 'b', long)]
    pub bitrate: Option<String>,
    /// Disable hardware acceleration
    #[arg(long, default_value_t = false)]
    pub no_hw_accel: bool,
    /// Multi-threaded ffmpeg
    #[arg(long, default_value_t = false)]
    pub ffmpeg_thread: bool,

    // ===== audio =====
    /// Audio sample rate
    #[arg(long, default_value_t = 48000)]
    pub target_audio: u32,
    /// Audio format: flac / aac / opus / ...
    #[arg(long, default_value = "flac")]
    pub audio_format: String,
    /// Audio bit depth: 16 / 24 / 32
    #[arg(long)]
    pub audio_bit: Option<u32>,
    /// Audio delay in frames
    #[arg(long)]
    pub audio_delay_frames: Option<i32>,
    /// Append the synchronized BPM and musical rhythm timeline.
    #[arg(long, default_value_t = false)]
    pub rhythm_analysis: bool,
    /// Infer audio BPM for charts without a BPM list; set false to keep the chart baseline.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub rhythm_auto_bpm: bool,
    /// Note kinds to analyze (comma separated: tap,drag,hold,flick).
    #[arg(long, default_value = "tap,hold")]
    pub rhythm_notes: String,
    /// Rhythm timeline flow speed (0.1..10); does not change playback or BPM.
    #[arg(long, default_value_t = 1.0)]
    pub rhythm_speed: f32,
}

/// Enable/disable the requested UI elements.
///
/// `--ui` is an allow-list (unlisted elements are hidden; `all`/`none` are
/// accepted), and `--hide-ui` is applied afterwards so a single element can be
/// dropped from `--ui all`.
pub(crate) fn apply_ui(config: &mut RenderConfig, ui: Option<&str>, hide: Option<&str>) {
    if let Some(list) = ui {
        let list = list.trim();
        let all = list.eq_ignore_ascii_case("all");
        let none = list.eq_ignore_ascii_case("none");
        if !list.is_empty() {
            config.ui_score = all;
            config.ui_combo = all;
            config.ui_name = all;
            config.ui_level = all;
            config.ui_line = all;
            config.ui_pb = all;
            config.ui_pause = all;
        }
        if !all && !none {
            for item in list.split(',').map(str::trim).filter(|item| !item.is_empty()) {
                match item.to_ascii_lowercase().as_str() {
                    "score" => config.ui_score = true,
                    "combo" => config.ui_combo = true,
                    "name" => config.ui_name = true,
                    "level" => config.ui_level = true,
                    "line" => config.ui_line = true,
                    "pb" => config.ui_pb = true,
                    "pause" => config.ui_pause = true,
                    other => warn!("unknown --ui element {other:?}"),
                }
            }
        }
    }
    if let Some(list) = hide {
        for item in list.split(',').map(str::trim).filter(|item| !item.is_empty()) {
            match item.to_ascii_lowercase().as_str() {
                "score" => config.ui_score = false,
                "combo" => config.ui_combo = false,
                "name" => config.ui_name = false,
                "level" => config.ui_level = false,
                "line" => config.ui_line = false,
                "pb" => config.ui_pb = false,
                "pause" => config.ui_pause = false,
                other => warn!("unknown --hide-ui element {other:?}"),
            }
        }
    }
}

/// Resolve the CLI's asset folder. The documented default is the legacy
/// `/usr/lib/ptkc-assets`, but normal Tauri packages install resources under
/// `/usr/lib/<product>/assets`, and a dev build keeps them in `src-tauri/assets`.
fn resolve_cli_assets(arg: &str) -> PathBuf {
    let is_assets_dir = |path: &Path| path.join("ending.mp3").is_file();
    let exe_assets = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("assets")));

    // An explicit --assets wins as long as it is a real directory.
    let explicit = PathBuf::from(arg);
    if arg != "/usr/lib/ptkc-assets" && !arg.trim().is_empty() && explicit.is_dir() {
        return explicit;
    }

    // A complete adjacent assets directory belongs to a portable bundle.
    let mut candidates = vec![
        exe_assets.unwrap_or_default(),
        PathBuf::from("/usr/lib/ptkc-assets"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"),
    ];
    candidates.retain(|path| !path.as_os_str().is_empty());
    candidates
        .iter()
        .find(|path| is_assets_dir(path))
        .or_else(|| candidates.iter().find(|path| path.is_dir()))
        .cloned()
        .unwrap_or_else(|| PathBuf::from("/usr/lib/ptkc-assets"))
}

/// CLI 渲染入口
pub async fn render_cli(args: CliArgs) -> Result<()> {
    let assets = resolve_cli_assets(&args.assets);
    macroquad::file::set_pc_assets_folder(&assets.display().to_string());

    let mut config = RenderConfig::default();
    let (width, height) = args.resolution.split_once(['x', 'X'])
        .context("resolution must be WIDTHxHEIGHT, e.g. 1920x1080")?;
    config.resolution = (
        width.parse().context("invalid resolution width")?,
        height.parse().context("invalid resolution height")?,
    );
    config.rhythm_analysis = args.rhythm_analysis;
    config.rhythm_auto_bpm = args.rhythm_auto_bpm;
    config.rhythm_speed = args.rhythm_speed;
    if config.rhythm_analysis {
        config.rhythm_tap = false;
        config.rhythm_drag = false;
        config.rhythm_hold = false;
        config.rhythm_flick = false;
        for kind in args.rhythm_notes.split(',').map(str::trim).filter(|kind| !kind.is_empty()) {
            match kind.to_ascii_lowercase().as_str() {
                "tap" | "click" => config.rhythm_tap = true,
                "drag" => config.rhythm_drag = true,
                "hold" => config.rhythm_hold = true,
                "flick" => config.rhythm_flick = true,
                "none" if args.rhythm_notes.trim().eq_ignore_ascii_case("none") => {},
                _ => bail!("unknown rhythm note kind '{kind}'; use tap,drag,hold,flick or none"),
            }
        }
    }
    config.output_resolution()?;
    config.fps = args.fps;
    config.bitrate_control = args.bitrate_control.to_uppercase();
    config.bitrate = args.bitrate.clone().unwrap_or_else(|| args.crf.to_string());
    config.hardware_accel = !args.no_hw_accel;
    config.video_codec = args.codec.clone();
    config.encoder = args.encoder.clone();
    config.ffmpeg_preset = args.preset.clone();
    config.video = args.format.eq_ignore_ascii_case("mov");
    config.ffmpeg_thread = args.ffmpeg_thread;

    config.target_audio = args.target_audio;
    config.audio_format = args.audio_format.clone();
    config.audio_bit = args.audio_bit;
    if let Some(delay) = args.audio_delay_frames {
        config.audio_delay_frames = delay;
    }

    // UI defaults for the CLI (the GUI keeps its own preset defaults).
    config.combo = args.combo.clone();
    config.player_name = args.name.clone().unwrap_or_default();
    config.player_rks = args.rks;
    config.player_avatar = args.avatar.clone();
    config.watermark = args.watermark.clone().unwrap_or_default();
    config.bar = args.bar;
    config.score_aim = args.score_aim;
    config.block_area_ui = args.ui_above_ba;
    config.chart_debug = args.chart_debug;
    config.flid_x = args.flid_x;
    config.chart_ratio = args.chart_ratio;
    config.background = args.background;
    config.res_pack_path = args.respack.clone();

    config.speed = args.speed;
    config.note_scale = args.note_scale;
    config.volume_music = args.volume_music;
    config.volume_sfx = args.volume_sfx;
    config.fxaa = args.fxaa;
    config.particle = !args.no_particle;
    config.disable_effect = args.no_effect;
    config.double_hint = !args.no_double_hint;
    config.aggressive = args.aggressive;
    config.hand_split = args.hand_split;
    config.sample_count = args.sample_count;

    config.disable_loading = !args.load;
    config.ending_length = args.ending.unwrap_or(if args.result { 3.0 } else { -2.0 });
    if args.result {
        config.ui_score = true;
        config.ui_combo = true;
        config.ui_pb = true;
        config.ui_level = true;
        config.ui_name = true;
    }
    apply_ui(&mut config, args.ui.as_deref(), args.hide_ui.as_deref());

    let input_path = PathBuf::from(&args.input);
    let mut chart_fs = fs::fs_from_file(&input_path)
        .with_context(|| format!("无法打开谱面: {}", input_path.display()))?;
    let mut info = fs::load_info(chart_fs.deref_mut())
        .await
        .with_context(|| format!("无法读取谱面元数据: {}", input_path.display()))?;

    if args.dim > 0 {
        info.background_dim = (args.dim as f32).clamp(0.0, 100.0) / 100.0;
    }

    let params = RenderParams {
        path: input_path,
        info,
        config,
    };
    init_colored_logger();
    welcome_logger();
    main_with_params(params, PathBuf::from(&args.output), true, true).await
}

pub fn init_colored_logger() {
    crate::logger::init();
}

pub fn welcome_logger() {
    const WELCOME: &str = concat!(
        " \x1b[0;1;35;95m_\x1b[0;1;31;91m__\x1b[0;1;33;93m__\x1b[0m  \x1b[0;1;36;96m_\x1b[0m     \x1b[0;1;31;91m_\x1b[0m     \x1b[0;1;36;96m__\x1b[0;1;34;94m__\x1b[0;1;35;95m__\x1b[0;1;31;91m_\x1b[0m \x1b[0;1;33;93m_\x1b[0m  \x1b[0;1;32;92m_\x1b[0;1;36;96m__\x1b[0;1;34;94m__\x1b[0;1;35;95m__\x1b[0m \n",
        "\x1b[0;1;31;91m|\x1b[0m  \x1b[0;1;33;93m_\x1b[0;1;32;92m_\x1b[0m \x1b[0;1;36;96m\\|\x1b[0m \x1b[0;1;34;94m|\x1b[0m   \x1b[0;1;31;91m(\x1b[0;1;33;93m_)\x1b[0m   \x1b[0;1;36;96m|\x1b[0;1;34;94m__\x1b[0m   \x1b[0;1;31;91m_\x1b[0;1;33;93m_|\x1b[0m \x1b[0;1;32;92m|\x1b[0;1;36;96m/\x1b[0m \x1b[0;1;34;94m/\x1b[0m \x1b[0;1;35;95m__\x1b[0;1;31;91m__\x1b[0;1;33;93m|\x1b[0m\n",
        "\x1b[0;1;33;93m|\x1b[0m \x1b[0;1;32;92m|_\x1b[0;1;36;96m_)\x1b[0m \x1b[0;1;34;94m|\x1b[0m \x1b[0;1;35;95m|\x1b[0;1;31;91m__\x1b[0m  \x1b[0;1;32;92m_\x1b[0m \x1b[0;1;36;96m__\x1b[0;1;34;94m__\x1b[0;1;35;95m__\x1b[0;1;31;91m|\x1b[0m \x1b[0;1;33;93m|\x1b[0m  \x1b[0;1;32;92m|\x1b[0m \x1b[0;1;36;96m'\x1b[0m \x1b[0;1;34;94m/\x1b[0m \x1b[0;1;35;95m|\x1b[0m     \n",
        "\x1b[0;1;32;92m|\x1b[0m  \x1b[0;1;36;96m_\x1b[0;1;34;94m__\x1b[0;1;35;95m/|\x1b[0m \x1b[0;1;31;91m'\x1b[0;1;33;93m_\x1b[0m \x1b[0;1;32;92m\\|\x1b[0m \x1b[0;1;36;96m|\x1b[0;1;34;94m__\x1b[0;1;35;95m__\x1b[0;1;31;91m__\x1b[0;1;33;93m|\x1b[0m \x1b[0;1;32;92m|\x1b[0m  \x1b[0;1;36;96m|\x1b[0m  \x1b[0;1;35;95m<|\x1b[0m \x1b[0;1;31;91m|\x1b[0m     \n",
        "\x1b[0;1;36;96m|\x1b[0m \x1b[0;1;34;94m|\x1b[0m    \x1b[0;1;31;91m|\x1b[0m \x1b[0;1;33;93m|\x1b[0m \x1b[0;1;32;92m|\x1b[0m \x1b[0;1;36;96m|\x1b[0m \x1b[0;1;34;94m|\x1b[0m      \x1b[0;1;32;92m|\x1b[0m \x1b[0;1;36;96m|\x1b[0m  \x1b[0;1;34;94m|\x1b[0m \x1b[0;1;35;95m.\x1b[0m \x1b[0;1;31;91m\\\x1b[0m \x1b[0;1;33;93m|\x1b[0;1;32;92m__\x1b[0;1;36;96m__\x1b[0m \n",
        "\x1b[0;1;34;94m|_\x1b[0;1;35;95m|\x1b[0m    \x1b[0;1;33;93m|\x1b[0;1;32;92m_|\x1b[0m \x1b[0;1;36;96m|\x1b[0;1;34;94m_|\x1b[0;1;35;95m_|\x1b[0m      \x1b[0;1;36;96m|_\x1b[0;1;34;94m|\x1b[0m  \x1b[0;1;35;95m|\x1b[0;1;31;91m_|\x1b[0;1;33;93m\\_\x1b[0;1;32;92m\\_\x1b[0;1;36;96m__\x1b[0;1;34;94m__\x1b[0;1;35;95m|\x1b[0m\n",
        "                                       \n",
        "                                       \n",
    );
    for line in WELCOME.lines() {
        log::info!("{line}");
    }
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
