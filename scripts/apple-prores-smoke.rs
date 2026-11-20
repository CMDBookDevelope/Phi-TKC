//! Apple CI integration test. FFmpeg reads the MOV but never encodes a fixture.
use crate::{
    apple_frames::{initialize_render_target, MetalFrames},
    apple_media::{Encoder, ProRes},
    apple_render::mobile_hdr::Transfer,
};
use anyhow::{bail, ensure, Context, Result};
use macroquad::miniquad as mq;
use serde_json::Value;
use std::{
    ffi::{c_void, OsStr},
    path::{Path, PathBuf},
    process::Command,
    ptr::null_mut,
    sync::{Arc, Mutex},
    time::Instant,
};

type Ref = *mut c_void;
const RATE: u32 = 48_000;
const FPS: u32 = 24;
const SMOKE: Size = Size {
    // Exercise both partial 16x8 compute groups and padded CoreVideo rows.
    width: 318,
    height: 238,
    frames: 8,
};

#[derive(Clone, Copy)]
struct Size {
    width: u32,
    height: u32,
    frames: u64,
}

#[derive(Clone, Copy)]
struct Case {
    codec: &'static str,
    fourcc: [u8; 4],
    alpha: bool,
    color: Color,
}

#[derive(Clone, Copy)]
enum Color {
    Rec709,
    Srgb,
    Pq,
    Hlg,
}

impl Case {
    fn suffix(self) -> &'static str {
        match self.color {
            Color::Rec709 => "rec709",
            Color::Srgb => "srgb",
            Color::Pq => "pq",
            Color::Hlg => "hlg",
        }
    }
    fn transfer(self) -> Option<Transfer> {
        match self.color {
            Color::Pq => Some(Transfer::Pq),
            Color::Hlg => Some(Transfer::Hlg),
            _ => None,
        }
    }
    fn blended_alpha(self) -> bool {
        self.alpha && matches!(self.color, Color::Rec709)
    }
    fn is_4444(self) -> bool {
        &self.fourcc[..3] == b"ap4"
    }
    fn pix_fmt(self) -> &'static str {
        if self.alpha {
            "yuva444p12le"
        } else if self.is_4444() {
            "yuv444p12le"
        } else {
            "yuv422p10le"
        }
    }
}

const CASES: [Case; 9] = [
    Case {
        codec: "prores_422",
        fourcc: *b"apcn",
        alpha: false,
        color: Color::Rec709,
    },
    Case {
        codec: "prores_422_proxy",
        fourcc: *b"apco",
        alpha: false,
        color: Color::Rec709,
    },
    Case {
        codec: "prores_422_lt",
        fourcc: *b"apcs",
        alpha: false,
        color: Color::Rec709,
    },
    Case {
        codec: "prores_422_hq",
        fourcc: *b"apch",
        alpha: false,
        color: Color::Srgb,
    },
    Case {
        codec: "prores_4444",
        fourcc: *b"ap4h",
        alpha: false,
        color: Color::Rec709,
    },
    Case {
        codec: "prores_4444_xq",
        fourcc: *b"ap4x",
        alpha: true,
        color: Color::Rec709,
    },
    Case {
        codec: "prores_4444_alpha",
        fourcc: *b"ap4h",
        alpha: true,
        color: Color::Srgb,
    },
    Case {
        codec: "prores_4444",
        fourcc: *b"ap4h",
        alpha: false,
        color: Color::Pq,
    },
    Case {
        codec: "prores_422_hq",
        fourcc: *b"apch",
        alpha: false,
        color: Color::Hlg,
    },
];

#[cfg_attr(target_os = "macos", link(name = "VideoToolbox", kind = "framework"))]
extern "C" {
    fn VTCompressionSessionCreate(
        allocator: Ref,
        width: i32,
        height: i32,
        codec: u32,
        spec: Ref,
        attributes: Ref,
        compressed_allocator: Ref,
        callback: extern "C" fn(Ref, Ref, i32, u32, Ref),
        context: Ref,
        output: *mut Ref,
    ) -> i32;
    fn VTCompressionSessionInvalidate(session: Ref);
}
#[cfg_attr(target_os = "macos", link(name = "CoreFoundation", kind = "framework"))]
extern "C" {
    fn CFRelease(object: Ref);
}

// An unconfigured VT session separates an unavailable encoder on a VM from a
// regression in our pixel pool, color properties, frame conversion or writer.
fn unavailable(case: Case, size: Size) -> Option<i32> {
    extern "C" fn unused_callback(_: Ref, _: Ref, _: i32, _: u32, _: Ref) {}
    let mut session = null_mut();
    let status = unsafe {
        let status = VTCompressionSessionCreate(
            null_mut(),
            size.width as i32,
            size.height as i32,
            u32::from_be_bytes(case.fourcc),
            null_mut(),
            null_mut(),
            null_mut(),
            unused_callback,
            null_mut(),
            &mut session,
        );
        if !session.is_null() {
            VTCompressionSessionInvalidate(session);
            CFRelease(session);
        }
        status
    };
    // kVTCouldNotFindVideoEncoderErr, kVTVideoEncoderNotAvailableNowErr.
    matches!(status, -12908 | -12915).then_some(status)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Gpu,
    Abort,
    CpuBaseline,
}

#[derive(Debug)]
enum Outcome {
    Encoded,
    Aborted,
    Unavailable(i32),
}

struct DrawEncode {
    path: PathBuf,
    case: Case,
    size: Size,
    mode: Mode,
    result: Arc<Mutex<Option<Result<Outcome>>>>,
}

impl mq::EventHandler for DrawEncode {
    fn update(&mut self, _: &mut mq::Context) {}
    fn draw(&mut self, ctx: &mut mq::Context) {
        let result = objc::rc::autoreleasepool(|| self.encode(ctx));
        *self.result.lock().unwrap() = Some(result);
        ctx.request_quit();
    }
}

impl DrawEncode {
    fn encode(&self, ctx: &mut mq::Context) -> Result<Outcome> {
        let Size {
            width,
            height,
            frames,
        } = self.size;
        let prores = ProRes::parse(self.case.codec).context("unknown Apple ProRes profile")?;
        ensure!(
            prores.alpha == self.case.alpha,
            "unexpected profile alpha policy"
        );
        let hdr = self.case.transfer();
        let mut encoder = match Encoder::new_with_prores(
            &self.path,
            width,
            height,
            FPS,
            0,
            RATE,
            hdr,
            Some(prores),
            matches!(self.case.color, Color::Srgb),
        ) {
            Ok(encoder) => encoder,
            Err(error) => {
                if let Some(status) = unavailable(self.case, self.size) {
                    return Ok(Outcome::Unavailable(status));
                }
                return Err(error);
            }
        };
        let source = mq::Texture::new_render_texture(
            ctx,
            mq::TextureParams {
                width,
                height,
                format: if hdr.is_some() {
                    mq::TextureFormat::RGBA16F
                } else {
                    mq::TextureFormat::RGBA8
                },
                ..Default::default()
            },
        );
        // Match production: HDR is single-sample, while SDR resolves MSAA.
        let pass = if hdr.is_some() {
            mq::RenderPass::new(ctx, source, None)
        } else {
            mq::RenderPass::new_multisampled(ctx, source, 4)
        };
        let shader = mq::Shader::new(ctx,
            "#version 450 core\nlayout(location=0) in vec2 position;\nvoid main(){gl_Position=vec4(position,0,1);}",
            "#version 450 core\nlayout(set=0,binding=0) uniform U{vec4 tint;};\nlayout(location=0) out vec4 color;\nvoid main(){color=tint;}",
            mq::ShaderMeta { images: vec![], uniforms: mq::UniformBlockLayout {
                uniforms: vec![mq::UniformDesc::new("tint", mq::UniformType::Float4)] } },
        ).map_err(anyhow::Error::msg)?;
        let blend = self.case.blended_alpha().then(|| mq::BlendState::new(
            mq::Equation::Add,
            mq::BlendFactor::Value(mq::BlendValue::SourceAlpha),
            mq::BlendFactor::OneMinusValue(mq::BlendValue::SourceAlpha),
        ));
        let alpha_blend = blend.map(|_| mq::BlendState::new(
            mq::Equation::Add,
            mq::BlendFactor::One,
            mq::BlendFactor::OneMinusValue(mq::BlendValue::SourceAlpha),
        ));
        let pipeline = mq::Pipeline::with_params(
            ctx,
            &[mq::BufferLayout::default()],
            &[mq::VertexAttribute::new(
                "position",
                mq::VertexFormat::Float2,
            )],
            shader,
            mq::PipelineParams { color_blend: blend, alpha_blend, ..Default::default() },
        );
        let bindings = mq::Bindings {
            vertex_buffers: vec![mq::Buffer::immutable(
                ctx,
                mq::BufferType::VertexBuffer,
                &[[-1f32, -1.], [3., -1.], [-1., 3.]],
            )],
            index_buffer: mq::Buffer::immutable(ctx, mq::BufferType::IndexBuffer, &[0u16, 1, 2]),
            images: vec![],
        };
        let mut gpu_frames = if self.mode == Mode::CpuBaseline {
            None
        } else {
            Some(MetalFrames::new_with_prores(
                ctx.metal_context().context("missing Metal context")?,
                width,
                height,
                prores,
                hdr,
            )?)
        };
        let mut baseline = if self.mode == Mode::CpuBaseline {
            vec![0; width as usize * height as usize * 8]
        } else {
            Vec::new()
        };
        let pcm = vec![0; (frames * u64::from(RATE / FPS) * 4) as usize];
        initialize_render_target(ctx.metal_context().context("missing Metal context")?, pass);
        for frame in 0..frames {
            objc::rc::autoreleasepool(|| -> Result<()> {
                // Fill the three-lease queue before draining and reusing the source.
                if let Some(gpu) = gpu_frames.as_mut().filter(|gpu| gpu.at_capacity()) {
                    gpu.drain_one(&mut encoder, &pcm)?;
                }
                ctx.begin_pass(pass, mq::PassAction::clear_color(0., 0., 0., 0.));
                ctx.apply_pipeline(&pipeline);
                ctx.apply_bindings(&bindings);
                for quadrant in 0..4 {
                    let x = (quadrant % 2) as i32 * width as i32 / 2;
                    let y = (quadrant / 2) as i32 * height as i32 / 2;
                    ctx.apply_scissor_rect(x, y, width as i32 / 2, height as i32 / 2);
                    let mut rgba = sample(frame, quadrant, self.case, true);
                    // Rec709 alpha exercises real source-over on transparent MSAA
                    // targets, including overlap. sRGB alpha starts with directly
                    // associated RGB. Neither fixture assumes straight encoder input.
                    if self.case.alpha && !self.case.blended_alpha() {
                        rgba = associate(rgba);
                    }
                    ctx.apply_uniforms(&rgba);
                    ctx.draw(0, 3, 1);
                    if self.case.blended_alpha() {
                        ctx.apply_uniforms(&overlay_sample(sample(frame, quadrant, self.case, true)));
                        ctx.draw(0, 3, 1);
                    }
                }
                ctx.end_render_pass();
                let pixel = encoder.pixel_buffer(&pcm)?;
                if let Some(gpu) = &mut gpu_frames {
                    gpu.enqueue(
                        ctx.metal_context().context("missing Metal context")?,
                        source,
                        pixel,
                        frame,
                    )?;
                } else {
                    source.read_pixels(ctx, &mut baseline);
                    flip_rows(&mut baseline, width as usize * 8);
                    encoder.write_prores_pixel(&pixel, &baseline)?;
                    encoder.frame_pixel(pixel, frame, &pcm)?;
                }
                Ok(())
            })
            .with_context(|| format!("draw and enqueue frame {frame}"))?;
            if self.mode == Mode::Abort && frame == 2 {
                // Drop a live encoder and three outstanding GPU pixel leases.
                return Ok(Outcome::Aborted);
            }
        }
        if let Some(gpu) = &mut gpu_frames {
            gpu.drain_all(&mut encoder, &pcm)?;
        }
        ensure!(
            encoder.try_finish(&pcm)?,
            "unexpected ProRes finalization interruption"
        );
        ctx.metal_context()
            .context("missing Metal context")?
            .finish();
        Ok(Outcome::Encoded)
    }
}

fn flip_rows(bytes: &mut [u8], stride: usize) {
    let height = bytes.len() / stride;
    for top in 0..height / 2 {
        let bottom = (height - 1 - top) * stride;
        let (prefix, suffix) = bytes.split_at_mut(bottom);
        prefix[top * stride..(top + 1) * stride].swap_with_slice(&mut suffix[..stride]);
    }
}

fn draw_encode(path: &Path, case: Case, size: Size, mode: Mode) -> Result<Outcome> {
    let result = Arc::new(Mutex::new(None));
    let output = Arc::clone(&result);
    let path = path.to_path_buf();
    objc::rc::autoreleasepool(|| {
        mq::start(
            mq::conf::Conf {
                headless: true,
                window_width: size.width as i32,
                window_height: size.height as i32,
                platform: mq::conf::Platform {
                    rendering_backend: mq::conf::RenderingBackend::Metal,
                    ..Default::default()
                },
                ..Default::default()
            },
            move |_| {
                Box::new(DrawEncode {
                    path,
                    case,
                    size,
                    mode,
                    result: output,
                })
            },
        )
    });
    let outcome = result
        .lock()
        .unwrap()
        .take()
        .context("Metal event loop did not run")?;
    outcome
}

fn media_command(tool: &str, args: &[&OsStr]) -> Result<Vec<u8>> {
    let output = Command::new(tool)
        .args(args)
        .output()
        .with_context(|| format!("launch {tool}"))?;
    ensure!(
        output.status.success(),
        "{tool} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}

fn verify(path: &Path, case: Case, size: Size) -> Result<()> {
    let json = media_command(
        "ffprobe",
        &[
            "-v".as_ref(),
            "error".as_ref(),
            "-select_streams".as_ref(),
            "v".as_ref(),
            "-show_streams".as_ref(),
            "-show_frames".as_ref(),
            "-of".as_ref(),
            "json".as_ref(),
            path.as_os_str(),
        ],
    )?;
    let probe: Value = serde_json::from_slice(&json)?;
    let streams = probe["streams"].as_array().context("ffprobe streams")?;
    ensure!(
        streams.len() == 1,
        "expected one video stream, got {}",
        streams.len()
    );
    let stream = &streams[0];
    let (primaries, transfer, matrix) = match case.color {
        Color::Rec709 => ("bt709", "bt709", "bt709"),
        Color::Srgb => ("bt709", "iec61966-2-1", "bt709"),
        Color::Pq => ("bt2020", "smpte2084", "bt2020nc"),
        Color::Hlg => ("bt2020", "arib-std-b67", "bt2020nc"),
    };
    let actual_format = stream["pix_fmt"].as_str().context("decoded pixel format")?;
    // Some Apple encoders store an opaque alpha plane even for normal 4444.
    ensure!(
        actual_format == case.pix_fmt()
            || (!case.alpha && case.is_4444() && actual_format == "yuva444p12le"),
        "decoded pixel format {actual_format}, expected {}",
        case.pix_fmt()
    );
    for (key, expected) in [
        ("codec_name", "prores"),
        ("codec_tag_string", std::str::from_utf8(&case.fourcc)?),
        ("color_primaries", primaries),
        ("color_transfer", transfer),
        ("color_space", matrix),
    ] {
        ensure!(
            stream[key].as_str() == Some(expected),
            "{key} = {}, expected {expected}",
            stream[key]
        );
    }
    ensure!(
        stream["width"].as_u64() == Some(u64::from(size.width))
            && stream["height"].as_u64() == Some(u64::from(size.height)),
        "unexpected dimensions: {stream}"
    );
    let frames = probe["frames"].as_array().context("ffprobe frames")?;
    ensure!(
        frames.len() as u64 == size.frames,
        "decoded {} frames, expected {}",
        frames.len(),
        size.frames
    );
    for (index, frame) in frames.iter().enumerate() {
        let pts: f64 = frame["best_effort_timestamp_time"]
            .as_str()
            .context("missing frame PTS")?
            .parse()?;
        ensure!(
            (pts - index as f64 / f64::from(FPS)).abs() < 1e-5,
            "frame {index} PTS {pts} at {FPS} fps"
        );
    }
    let decoded = media_command(
        "ffmpeg",
        &[
            "-v".as_ref(),
            "error".as_ref(),
            "-xerror".as_ref(),
            "-i".as_ref(),
            path.as_os_str(),
            "-map".as_ref(),
            "0:v:0".as_ref(),
            "-fps_mode".as_ref(),
            "passthrough".as_ref(),
            "-f".as_ref(),
            "rawvideo".as_ref(),
            "-pix_fmt".as_ref(),
            actual_format.as_ref(),
            "-".as_ref(),
        ],
    )?;
    verify_pixels(&decoded, case, size, actual_format.starts_with("yuva"))
}

// Draw linear HDR values; PQ expectations use the independent ST 2084 table.
// HLG expectations use f64 reference OOTF/OETF math, not production encode_rgb.
// Four motion phases and two intensity levels distinguish all eight frames.
fn sample(frame: u64, quadrant: usize, case: Case, draw: bool) -> [f32; 4] {
    let slot = (quadrant + frame as usize) % 4;
    let (low, mid, high) = match (case.transfer().is_some(), draw || matches!(case.color, Color::Hlg), frame % 8 < 4) {
        (true, true, true) => (0.01, 0.1, 1.0),
        (true, false, true) => (0.149_946, 0.299_699, 0.508_078),
        (true, true, false) => (0.1, 4.0, 10.0),
        (true, false, false) => (0.299_699, 0.652_579, 0.751_827),
        (false, _, true) => (0.0, 0.25, 0.9),
        (false, _, false) => (0.25, 0.5, 0.75),
    };
    let [r, g, b] = [
        [high, low, low],
        [low, high, mid],
        [mid, low, high],
        [high, high, mid],
    ][slot];
    let mut rgba = [r, g, b, [0.0, 0.25, 0.5, 1.0][slot]];
    if !draw && case.alpha {
        let mut associated = associate(rgba);
        if case.blended_alpha() {
            let overlay = associate(overlay_sample(rgba));
            associated = std::array::from_fn(|i| overlay[i] + associated[i] * (1.0 - overlay[3]));
        }
        rgba = if associated[3] > 0.0 {
            [associated[0] / associated[3], associated[1] / associated[3], associated[2] / associated[3], associated[3]]
        } else {
            [0.0; 4]
        };
    }
    if !draw && matches!(case.color, Color::Hlg) {
        let encoded = reference_hlg([r, g, b]);
        rgba[..3].copy_from_slice(&encoded);
    }
    rgba
}

fn associate([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    [r * a, g * a, b * a, a]
}

fn overlay_sample([r, g, b, a]: [f32; 4]) -> [f32; 4] {
    [b, r, g, a * 0.5]
}

fn reference_hlg(rgb: [f32; 3]) -> [f32; 3] {
    let rgb = rgb.map(|c| (f64::from(c) / 10.0).clamp(0.0, 1.0));
    let luminance = 0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2];
    let scale = if luminance > 0.0 { luminance.powf(-1.0 / 6.0) } else { 0.0 };
    rgb.map(|c| {
        let scene = (c * scale).min(1.0);
        if scene <= 1.0 / 12.0 {
            (3.0 * scene).sqrt() as f32
        } else {
            (0.178_832_77 * (12.0 * scene - 0.284_668_92).ln() + 0.559_910_7) as f32
        }
    })
}

fn verify_pixels(decoded: &[u8], case: Case, size: Size, has_alpha: bool) -> Result<()> {
    let (w, h) = (size.width as usize, size.height as usize);
    let cw = if case.is_4444() { w } else { w / 2 };
    let (y_size, c_size) = (w * h, cw * h);
    let frame_size = (y_size + 2 * c_size + if has_alpha { y_size } else { 0 }) * 2;
    ensure!(
        decoded.len() == frame_size * size.frames as usize,
        "wrong raw decoded frame count"
    );
    let (kr, kb) = if case.transfer().is_some() {
        (0.2627, 0.0593)
    } else {
        (0.2126, 0.0722)
    };
    let shift = if case.is_4444() { 4.0 } else { 1.0 };
    for (frame_index, frame) in decoded.chunks_exact(frame_size).enumerate() {
        let read = |sample: usize| u16::from_le_bytes([frame[sample * 2], frame[sample * 2 + 1]]);
        for top_first in 0..4 {
            let x = (1 + 2 * (top_first % 2)) * w / 4;
            let y = (1 + 2 * (top_first / 2)) * h / 4;
            let luma = y * w + x;
            let chroma = y * cw + x * cw / w;
            let [r, g, b, alpha] = sample(frame_index as u64, top_first ^ 2, case, false);
            let l = kr * r + (1.0 - kr - kb) * g + kb * b;
            let expected = [
                64.0 + 876.0 * l,
                512.0 + 448.0 * (b - l) / (1.0 - kb),
                512.0 + 448.0 * (r - l) / (1.0 - kr),
            ];
            let actual = [
                read(luma),
                read(y_size + chroma),
                read(y_size + c_size + chroma),
            ];
            for (channel, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
                let actual = f32::from(actual) / shift;
                let tolerance = if case.alpha { 8.0 } else { 28.0 };
                ensure!((actual - expected).abs() <= tolerance,
                    "frame {frame_index}, top-first quadrant {top_first}, YCbCr channel {channel}: {actual}, expected {expected}");
            }
            if has_alpha {
                let expected = if case.alpha { alpha } else { 1.0 };
                let actual = f32::from(read(y_size + 2 * c_size + luma)) / 4095.0;
                ensure!((actual - expected).abs() <= 0.015,
                    "frame {frame_index}, quadrant {top_first}: alpha {actual}, expected {expected}");
                if case.alpha {
                    let yc = (f32::from(read(luma)) / shift - 64.0) / 876.0;
                    let cb = (f32::from(read(y_size + chroma)) / shift - 512.0) / 896.0;
                    let cr = (f32::from(read(y_size + c_size + chroma)) / shift - 512.0) / 896.0;
                    let rgb = [
                        yc + 2.0 * (1.0 - kr) * cr,
                        yc - 2.0 * (kb * (1.0 - kb) * cb + kr * (1.0 - kr) * cr) / (1.0 - kr - kb),
                        yc + 2.0 * (1.0 - kb) * cb,
                    ];
                    for (channel, (actual_rgb, expected_rgb)) in rgb.into_iter().zip([r, g, b]).enumerate() {
                        ensure!((actual_rgb - expected_rgb).abs() <= 0.035,
                            "frame {frame_index}, quadrant {top_first}: straight RGB {channel} {actual_rgb}, expected {expected_rgb}");
                        // Decode must be straight: applying coverage once should
                        // reproduce the source-over result on BOTH backgrounds.
                        for background in [0.0, 1.0] {
                            let composite = actual_rgb * actual + background * (1.0 - actual);
                            let reference = expected_rgb * expected + background * (1.0 - expected);
                            ensure!((composite - reference).abs() <= 0.04,
                                "frame {frame_index}, quadrant {top_first}: RGB {channel} over {background}: {composite}, expected {reference}");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
fn apple_metal_prores_profiles_roundtrip() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let directory = std::env::var_os("PHI_TKC_APPLE_SMOKE_ARTIFACTS").map(PathBuf::from);
    let directory = directory.as_deref().unwrap_or(temp.path());
    std::fs::create_dir_all(directory)?;
    // Run an aborted Metal context before any successful render. Different
    // run_profile calls create independent headless contexts on this thread.
    let aborted = draw_encode(
        &temp.path().join("aborted.mov"),
        CASES[0],
        SMOKE,
        Mode::Abort,
    )?;
    ensure!(
        matches!(aborted, Outcome::Aborted | Outcome::Unavailable(_)),
        "expected interrupted task: {aborted:?}"
    );
    let require_all = std::env::var_os("PHI_TKC_REQUIRE_APPLE_PRORES").is_some();
    let mut tested = 0;
    for case in CASES {
        let name = format!("{}-{}", case.codec, case.suffix());
        let path = directory.join(format!("apple-{name}.mov"));
        let started = Instant::now();
        match draw_encode(&path, case, SMOKE, Mode::Gpu).with_context(|| name.clone())? {
            Outcome::Encoded => {
                let elapsed = started.elapsed();
                verify(&path, case, SMOKE).with_context(|| name.clone())?;
                tested += 1;
                println!(
                    "APPLE_PRORES_PASS {name} {} frames in {:.3}s",
                    SMOKE.frames,
                    elapsed.as_secs_f64()
                );
            }
            Outcome::Unavailable(status) => {
                ensure!(
                    !require_all,
                    "{name} VideoToolbox encoder unavailable: {status}"
                );
                eprintln!("APPLE_PRORES_UNAVAILABLE {name} VideoToolbox OSStatus={status}");
            }
            Outcome::Aborted => bail!("unexpected interruption for {name}"),
        }
    }
    println!(
        "APPLE_PRORES_COVERAGE {tested}/{} profiles encoded",
        CASES.len()
    );
    Ok(())
}

// Exercise the actual QuadGl and rhythm material, not only the miniquad fixture
// shader above. This catches a regression in default target alpha or a second
// SrcAlpha multiply in the production compositor independently of VT decoding.
fn verify_rendered_alpha() -> Result<()> {
    use macroquad::prelude::*;
    use prpr::{
        core::{ChartExtra, MSRenderTarget, PremultipliedBlit, RhythmComposite, RhythmNoteSelection, RhythmStrip},
        ui::{FontArc, TextPainter},
    };
    let chart = prpr::parse::parse_phigros(
        r#"{"offset":0,"judgeLineList":[{"bpm":120,"judgeLineDisappearEvents":[],"judgeLineMoveEvents":[],"judgeLineRotateEvents":[],"speedEvents":[{"startTime":0,"endTime":1024,"value":1}],"notesAbove":[],"notesBelow":[]}]}"#,
        ChartExtra::default(),
    )?;
    let mut painter = TextPainter::new(FontArc::try_from_slice(include_bytes!("../src-tauri/assets/font.ttf"))?);
    let read_center = |target: RenderTarget, width: u32, height: u32, top_y: u32| {
        let mut gl = unsafe { get_internal_gl() };
        gl.flush();
        let mut bytes = vec![0; width as usize * height as usize * 4];
        target.texture.raw_miniquad_texture_handle().read_pixels(gl.quad_context, &mut bytes);
        flip_rows(&mut bytes, width as usize * 4);
        let i = ((top_y * width + width / 2) * 4) as usize;
        <[u8; 4]>::try_from(&bytes[i..i + 4]).unwrap()
    };
    let check = |actual: [u8; 4], expected: [u8; 4]| -> Result<()> {
        ensure!(actual.into_iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2),
            "rendered associated RGBA {actual:?}, expected {expected:?}");
        Ok(())
    };
    for samples in [1, 4] {
        let game = MSRenderTarget::new_rgba((64, 36), samples);
        set_camera(&Camera2D { zoom: vec2(1., 1.), render_target: Some(game.input()), ..Default::default() });
        clear_background(BLANK);
        draw_rectangle(-1., -1., 2., 2., Color::new(1., 0., 0., 0.5));
        check(read_center(game.output(), 64, 36, 18), [128, 0, 0, 128])?;
        draw_rectangle(-1., -1., 2., 2., Color::new(0., 0., 1., 0.5));
        check(read_center(game.output(), 64, 36, 18), [64, 0, 128, 192])?;
        let strip = RhythmStrip::from_chart(&chart, RhythmNoteSelection { tap: true, drag: true, hold: true, flick: true }, None);
        let rhythm = RhythmComposite::new_with_alpha(64, 36, Some(strip), 1, true)?;
        let rendered = rhythm.compose(0, &game, Some(0.), &mut painter);
        let height = rhythm.layout.output_height;
        check(read_center(rendered, 64, height, 18), [64, 0, 128, 192])?;
        let output = MSRenderTarget::new_rgba((64, height), 1);
        for (background, expected) in [(BLACK, [64, 0, 128, 255]), (WHITE, [127, 63, 191, 255])] {
            set_camera(&Camera2D { zoom: vec2(1., 1.), render_target: Some(output.input()), ..Default::default() });
            clear_background(background);
            {
                let blit = PremultipliedBlit::new()?;
                blit.draw(rendered.texture, -1., -1., WHITE, DrawTextureParams {
                    dest_size: Some(vec2(2., 2.)), flip_y: true, ..Default::default()
                });
                // Drop before caller flush: no queued pipeline handle may remain.
            }
            check(read_center(output.output(), 64, height, 18), expected)?;
        }
        println!("APPLE_ALPHA_RENDER_PASS samples={samples} alpha=.5/.75 rhythm=associated backgrounds=black/white drop=queued-safe");
    }
    Ok(())
}

#[test]
fn apple_metal_alpha_render_and_rhythm_source_over() -> Result<()> {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let result = Arc::new(Mutex::new(None));
    let finished = Arc::clone(&result);
    macroquad::Window::from_config(
        mq::conf::Conf {
            headless: true, window_width: 64, window_height: 48,
            platform: mq::conf::Platform { rendering_backend: mq::conf::RenderingBackend::Metal, ..Default::default() },
            ..Default::default()
        },
        async move {
            let checked = catch_unwind(AssertUnwindSafe(verify_rendered_alpha))
                .unwrap_or_else(|_| Err(anyhow::anyhow!("alpha renderer panicked inside the GPU frame")));
            *finished.lock().unwrap() = Some(checked);
            macroquad::window::miniquad_context().request_quit();
        },
    );
    let checked = result.lock().unwrap().take().context("alpha Metal event loop did not run")?;
    checked
}

#[test]
#[ignore = "native timing comparison, no CI performance threshold"]
fn apple_metal_prores_pq_timing() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let size = Size {
        width: 1920,
        height: 1080,
        frames: 60,
    };
    let case = CASES[7];
    for (name, mode) in [("cpu-readback", Mode::CpuBaseline), ("metal", Mode::Gpu)] {
        let path = temp.path().join(format!("{name}.mov"));
        let started = Instant::now();
        let outcome = draw_encode(&path, case, size, mode)?;
        let elapsed = started.elapsed();
        ensure!(matches!(outcome, Outcome::Encoded), "{name}: {outcome:?}");
        // Full pixel and timestamp verification remains in the small CI test.
        println!(
            "APPLE_PRORES_TIMING {name} {} frames, {:.3}s, {:.2} fps",
            size.frames,
            elapsed.as_secs_f64(),
            size.frames as f64 / elapsed.as_secs_f64()
        );
    }
    Ok(())
}
