//! Native integration tests, included by the independent Apple CI smoke crate.
//! Uses the actual MetalFrames and Encoder; decoding is only test verification.
use crate::apple_frames::MetalFrames;
use crate::apple_media::{Encoder, Time};
use anyhow::{ensure, Result};
use macroquad::miniquad as mq;
use objc::{
    class, msg_send,
    runtime::{Object, NO, YES},
    sel, sel_impl,
};
use std::{
    ffi::c_void,
    path::{Path, PathBuf},
    ptr::null_mut,
    sync::{Arc, Mutex},
};

type Ref = *mut c_void;
const WIDTH: u32 = 128;
const HEIGHT: u32 = 96;
const FPS: u32 = 30;
const FRAMES: u64 = 120;
const RATE: u32 = 48_000;
const AUDIO_FRAMES: usize = FRAMES as usize * RATE as usize / FPS as usize;

#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreFoundation", kind = "framework")
)]
extern "C" {
    fn CFRelease(value: Ref);
}
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreMedia", kind = "framework")
)]
extern "C" {
    fn CMSampleBufferIsValid(sample: Ref) -> u8;
    fn CMSampleBufferGetNumSamples(sample: Ref) -> i64;
    fn CMSampleBufferGetPresentationTimeStamp(sample: Ref) -> Time;
    fn CMSampleBufferGetDuration(sample: Ref) -> Time;
    fn CMSampleBufferGetImageBuffer(sample: Ref) -> Ref;
    fn CMSampleBufferGetDataBuffer(sample: Ref) -> Ref;
    fn CMTimeGetSeconds(time: Time) -> f64;
    fn CMBlockBufferGetDataLength(block: Ref) -> usize;
    fn CMBlockBufferCopyDataBytes(block: Ref, offset: usize, length: usize, output: Ref) -> i32;
}
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreVideo", kind = "framework")
)]
extern "C" {
    fn CVPixelBufferLockBaseAddress(pixel: Ref, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(pixel: Ref, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddress(pixel: Ref) -> Ref;
    fn CVPixelBufferGetBytesPerRow(pixel: Ref) -> usize;
    fn CVPixelBufferGetWidth(pixel: Ref) -> usize;
    fn CVPixelBufferGetHeight(pixel: Ref) -> usize;
    fn CVPixelBufferGetPixelFormatType(pixel: Ref) -> u32;
    static kCVPixelBufferPixelFormatTypeKey: Ref;
}
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "AVFoundation", kind = "framework")
)]
extern "C" {}

struct Cf(Ref);
impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}
struct Obj(*mut Object);
impl Drop for Obj {
    fn drop(&mut self) {
        unsafe {
            let _: () = msg_send![self.0, release];
        }
    }
}
fn string(value: &str) -> Obj {
    unsafe {
        let object: *mut Object = msg_send![class!(NSString), alloc];
        Obj(msg_send![object, initWithBytes:value.as_ptr() length:value.len() encoding:4usize])
    }
}
fn number(value: i64) -> Obj {
    unsafe {
        let object: *mut Object = msg_send![class!(NSNumber), numberWithLongLong:value];
        let _: () = msg_send![object, retain];
        Obj(object)
    }
}
fn dictionary(entries: &[(&str, &Obj)]) -> Obj {
    unsafe {
        let keys: Vec<Obj> = entries.iter().map(|(key, _)| string(key)).collect();
        let keys: Vec<*mut Object> = keys.iter().map(|key| key.0).collect();
        let values: Vec<*mut Object> = entries.iter().map(|(_, value)| value.0).collect();
        let object: *mut Object = msg_send![class!(NSDictionary), dictionaryWithObjects:values.as_ptr() forKeys:keys.as_ptr() count:values.len()];
        let _: () = msg_send![object, retain];
        Obj(object)
    }
}

// Logical GL quadrants, in bottom-left / bottom-right / top-left / top-right order.
fn quadrants(frame: u64) -> [[u8; 3]; 4] {
    if frame % 2 == 0 {
        [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0]]
    } else {
        [[0, 0, 255], [255, 255, 0], [255, 0, 0], [0, 255, 0]]
    }
}

struct GpuSmoke {
    path: PathBuf,
    pcm: Vec<u8>,
    pause: Option<u64>,
    finished: Arc<Mutex<Option<Result<()>>>>,
    ran: bool,
}
impl mq::EventHandler for GpuSmoke {
    fn update(&mut self, _: &mut mq::Context) {}
    fn draw(&mut self, ctx: &mut mq::Context) {
        if self.ran {
            return;
        }
        self.ran = true;
        let result = objc::rc::autoreleasepool(|| self.encode(ctx));
        *self.finished.lock().unwrap() = Some(result);
        ctx.request_quit();
    }
}
impl GpuSmoke {
    fn encode(&self, ctx: &mut mq::Context) -> Result<()> {
        let source = mq::Texture::new_render_texture(
            ctx,
            mq::TextureParams {
                width: WIDTH,
                height: HEIGHT,
                format: mq::TextureFormat::RGBA8,
                ..Default::default()
            },
        );
        let pass = mq::RenderPass::new_multisampled(ctx, source, 4);
        let shader = mq::Shader::new(ctx,
            "#version 450 core\nlayout(location=0) in vec2 position;\nvoid main(){gl_Position=vec4(position,0,1);}",
            "#version 450 core\nlayout(set=0,binding=0) uniform U{vec4 tint;};\nlayout(location=0) out vec4 color;\nvoid main(){color=tint;}",
            mq::ShaderMeta { images: vec![], uniforms: mq::UniformBlockLayout {
                uniforms: vec![mq::UniformDesc::new("tint", mq::UniformType::Float4)] }
            }).map_err(anyhow::Error::msg)?;
        let pipeline = mq::Pipeline::new(
            ctx,
            &[mq::BufferLayout::default()],
            &[mq::VertexAttribute::new(
                "position",
                mq::VertexFormat::Float2,
            )],
            shader,
        );
        let vertices = mq::Buffer::immutable(
            ctx,
            mq::BufferType::VertexBuffer,
            &[[-1f32, -1.], [3., -1.], [-1., 3.]],
        );
        let indices = mq::Buffer::immutable(ctx, mq::BufferType::IndexBuffer, &[0u16, 1, 2]);
        let bindings = mq::Bindings {
            vertex_buffers: vec![vertices],
            index_buffer: indices,
            images: vec![],
        };
        let mut encoder = Encoder::new(&self.path, WIDTH, HEIGHT, FPS, 4_000_000, RATE)?;
        let mut frames = MetalFrames::new(ctx.metal_context().unwrap(), WIDTH, HEIGHT)?;
        for frame in 0..FRAMES {
            objc::rc::autoreleasepool(|| -> Result<()> {
                if self.pause == Some(frame) {
                    // The production background path must preserve all GPU leases
                    // without feeding a possibly blocked AVAssetWriter input.
                    frames.wait_all()?;
                    ctx.metal_context().unwrap().finish();
                    encoder.suspend(&self.pcm)?;
                    frames.defer_all(&mut encoder, &self.pcm)?;
                    ensure!(encoder.needs_resume()?, "suspend did not request resume");
                    encoder.resume(&self.pcm)?;
                    ensure!(!encoder.needs_resume()?, "native VT did not resume");
                }
                // Deliberately retain the full three-GPU-frame window, while
                // repeatedly overwriting the source on the same command queue.
                if frames.at_capacity() {
                    frames.drain_one(&mut encoder, &self.pcm)?;
                }
                ctx.begin_pass(pass, mq::PassAction::clear_color(0., 0., 0., 1.));
                ctx.apply_pipeline(&pipeline);
                ctx.apply_bindings(&bindings);
                for (index, color) in quadrants(frame).into_iter().enumerate() {
                    let x = (index % 2) as i32 * WIDTH as i32 / 2;
                    let y = (index / 2) as i32 * HEIGHT as i32 / 2;
                    ctx.apply_scissor_rect(x, y, WIDTH as i32 / 2, HEIGHT as i32 / 2);
                    ctx.apply_uniforms(&[
                        color[0] as f32 / 255.,
                        color[1] as f32 / 255.,
                        color[2] as f32 / 255.,
                        1.,
                    ]);
                    ctx.draw(0, 3, 1);
                }
                ctx.end_render_pass();
                let pixel = encoder.pixel_buffer(&self.pcm)?;
                frames.enqueue(ctx.metal_context().unwrap(), source, pixel, frame)?;
                Ok(())
            })?;
        }
        frames.drain_all(&mut encoder, &self.pcm)?;
        ensure!(
            encoder.try_finish(&self.pcm)?,
            "unexpected native finalization interruption"
        );
        ctx.metal_context().unwrap().finish();
        Ok(())
    }
}

unsafe fn reader(asset: *mut Object, track: *mut Object, settings: *mut Object) -> (Obj, Obj) {
    let allocated: *mut Object = msg_send![class!(AVAssetReader), alloc];
    let mut error: *mut Object = null_mut();
    let object: *mut Object = msg_send![allocated, initWithAsset:asset error:&mut error];
    assert!(!object.is_null(), "cannot create AVAssetReader");
    let reader = Obj(object);
    let allocated: *mut Object = msg_send![class!(AVAssetReaderTrackOutput), alloc];
    let object: *mut Object = msg_send![allocated, initWithTrack:track outputSettings:settings];
    assert!(!object.is_null(), "cannot create track output");
    let output = Obj(object);
    let can: objc::runtime::BOOL = msg_send![reader.0, canAddOutput:output.0];
    assert_eq!(can, YES);
    let _: () = msg_send![reader.0, addOutput:output.0];
    let started: objc::runtime::BOOL = msg_send![reader.0, startReading];
    assert_eq!(started, YES);
    (reader, output)
}

unsafe fn decode_video(asset: *mut Object, track: *mut Object) {
    let format = number(0x42475241);
    let settings: *mut Object = msg_send![class!(NSDictionary), dictionaryWithObject:format.0 forKey:kCVPixelBufferPixelFormatTypeKey];
    let (reader, output) = reader(asset, track, settings);
    let mut count = 0u64;
    let mut end = 0.0;
    loop {
        let next = objc::rc::autoreleasepool(|| {
            let sample: Ref = msg_send![output.0, copyNextSampleBuffer];
            if sample.is_null() {
                return None;
            }
            let sample = Cf(sample);
            assert!(CMSampleBufferIsValid(sample.0) != 0);
            let samples = CMSampleBufferGetNumSamples(sample.0);
            if samples == 0 {
                return Some(None);
            }
            assert_eq!(samples, 1);
            let pts = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample.0));
            let duration = CMTimeGetSeconds(CMSampleBufferGetDuration(sample.0));
            let pixel = CMSampleBufferGetImageBuffer(sample.0);
            assert!(!pixel.is_null(), "H.264 decoder returned no image");
            assert_eq!(CVPixelBufferGetPixelFormatType(pixel), 0x42475241);
            assert_eq!(CVPixelBufferGetWidth(pixel), WIDTH as usize);
            assert_eq!(CVPixelBufferGetHeight(pixel), HEIGHT as usize);
            assert_eq!(CVPixelBufferLockBaseAddress(pixel, 1), 0);
            let base = CVPixelBufferGetBaseAddress(pixel) as *const u8;
            let stride = CVPixelBufferGetBytesPerRow(pixel);
            let mut colors = [[0u8; 3]; 4];
            for (index, color) in colors.iter_mut().enumerate() {
                let x = (1 + 2 * (index % 2)) * WIDTH as usize / 4;
                let y = (1 + 2 * (index / 2)) * HEIGHT as usize / 4;
                let bytes = std::slice::from_raw_parts(base.add(y * stride + x * 4), 4);
                *color = [bytes[2], bytes[1], bytes[0]];
            }
            assert_eq!(CVPixelBufferUnlockBaseAddress(pixel, 1), 0);
            Some(Some((pts, duration, colors)))
        });
        let Some(next) = next else {
            break;
        };
        let Some((pts, duration, colors)) = next else {
            continue;
        };
        assert!(
            (pts - count as f64 / FPS as f64).abs() < 1e-5,
            "video PTS gap at frame {count}: {pts}"
        );
        assert!(duration.is_finite() && duration > 0.);
        let expected = quadrants(count);
        // CV/MP4 rows are top-first; GL scissor coordinates are bottom-first.
        for (index, actual) in colors.into_iter().enumerate() {
            for (channel, target) in actual.into_iter().zip(expected[index ^ 2]) {
                assert!(channel.abs_diff(target)<45,
                    "BGRA/channel/orientation/source-reuse failure at video frame {count}, quadrant {index}: {actual:?}");
            }
        }
        count += 1;
        end = pts + duration;
    }
    let state: isize = msg_send![reader.0, status];
    assert_eq!(state, 2, "video decoder did not complete");
    assert_eq!(
        count, FRAMES,
        "GPU export dropped or duplicated a video frame"
    );
    assert!((end - FRAMES as f64 / FPS as f64).abs() <= 1. / FPS as f64);
    eprintln!("Native GPU video decoded {count} frames with exact PTS, BGRA colors, all quadrants and top-first orientation");
}

unsafe fn decode_audio(asset: *mut Object, track: *mut Object) {
    let false_value: *mut Object = msg_send![class!(NSNumber), numberWithBool:NO];
    let _: () = msg_send![false_value, retain];
    let false_value = Obj(false_value);
    let settings = dictionary(&[
        ("AVFormatIDKey", &number(0x6c70636d)),
        ("AVLinearPCMBitDepthKey", &number(16)),
        ("AVLinearPCMIsFloatKey", &false_value),
        ("AVLinearPCMIsBigEndianKey", &false_value),
        ("AVLinearPCMIsNonInterleaved", &false_value),
    ]);
    let (reader, output) = reader(asset, track, settings.0);
    let mut frames = 0u64;
    let mut audible_stereo = 0usize;
    loop {
        let next = objc::rc::autoreleasepool(|| {
            let sample: Ref = msg_send![output.0, copyNextSampleBuffer];
            if sample.is_null() {
                return None;
            }
            let sample = Cf(sample);
            assert!(CMSampleBufferIsValid(sample.0) != 0);
            let count = CMSampleBufferGetNumSamples(sample.0);
            if count == 0 {
                return Some((0, 0));
            }
            assert!(count > 0);
            let block = CMSampleBufferGetDataBuffer(sample.0);
            assert!(!block.is_null());
            let length = CMBlockBufferGetDataLength(block).min(16 * 1024);
            let mut bytes = vec![0u8; length];
            assert_eq!(
                CMBlockBufferCopyDataBytes(block, 0, length, bytes.as_mut_ptr() as Ref),
                0
            );
            let audible = bytes
                .chunks_exact(4)
                .filter(|frame| {
                    let left = i16::from_le_bytes([frame[0], frame[1]]);
                    let right = i16::from_le_bytes([frame[2], frame[3]]);
                    left.unsigned_abs() > 100
                        && right.unsigned_abs() > 100
                        && (left < 0) != (right < 0)
                })
                .count();
            Some((count as u64, audible))
        });
        let Some((count, audible)) = next else {
            break;
        };
        frames += count;
        audible_stereo += audible;
    }
    let state: isize = msg_send![reader.0, status];
    assert_eq!(state, 2);
    assert!(
        frames.abs_diff(AUDIO_FRAMES as u64) <= 4096,
        "decoded AAC count {frames}"
    );
    assert!(
        audible_stereo > 100,
        "AAC did not preserve the nonzero opposite stereo channels"
    );
    eprintln!(
        "Native GPU audio decoded {frames} PCM frames, {audible_stereo} nonzero stereo frames"
    );
}

unsafe fn readback(path: &Path) {
    let path = string(&path.to_string_lossy());
    let url: *mut Object = msg_send![class!(NSURL),fileURLWithPath:path.0];
    let asset: *mut Object =
        msg_send![class!(AVURLAsset),URLAssetWithURL:url options:null_mut::<Object>()];
    assert!(!asset.is_null());
    let _: () = msg_send![asset, retain];
    let asset = Obj(asset);
    let video = string("vide");
    let audio = string("soun");
    let videos: *mut Object = msg_send![asset.0,tracksWithMediaType:video.0];
    let audios: *mut Object = msg_send![asset.0,tracksWithMediaType:audio.0];
    let count: usize = msg_send![videos, count];
    assert_eq!(count, 1);
    let count: usize = msg_send![audios, count];
    assert_eq!(count, 1);
    decode_video(asset.0, msg_send![videos,objectAtIndex:0usize]);
    decode_audio(asset.0, msg_send![audios,objectAtIndex:0usize]);
}

fn run(pause: Option<u64>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("gpu-roundtrip.mp4");
    let result = Arc::new(Mutex::new(None));
    let finished = Arc::clone(&result);
    let output = path.clone();
    // AAC may remove a DC signal. A 440 Hz tone exercises both actual channels
    // and keeps their opposite polarity without depending on codec high-pass.
    let mut pcm = Vec::with_capacity(AUDIO_FRAMES * 4);
    for sample in 0..AUDIO_FRAMES {
        let wave = (std::f64::consts::TAU * 440. * sample as f64 / RATE as f64).sin();
        pcm.extend_from_slice(&((wave * 4000.) as i16).to_le_bytes());
        pcm.extend_from_slice(&((wave * -3000.) as i16).to_le_bytes());
    }
    objc::rc::autoreleasepool(|| {
        mq::start(
            mq::conf::Conf {
                headless: true,
                window_width: WIDTH as i32,
                window_height: HEIGHT as i32,
                platform: mq::conf::Platform {
                    rendering_backend: mq::conf::RenderingBackend::Metal,
                    ..Default::default()
                },
                ..Default::default()
            },
            move |_| {
                Box::new(GpuSmoke {
                    path: output,
                    pcm,
                    pause,
                    finished,
                    ran: false,
                })
            },
        )
    });
    result
        .lock()
        .unwrap()
        .take()
        .expect("Metal event loop did not run")
        .unwrap();
    if let Some(folder) = std::env::var_os("PHI_TKC_APPLE_SMOKE_ARTIFACTS") {
        let folder = PathBuf::from(folder);
        std::fs::create_dir_all(&folder).unwrap();
        let name = format!(
            "gpu-{WIDTH}x{HEIGHT}-{FRAMES}frames-pause-{}.mp4",
            pause.unwrap_or(0)
        );
        std::fs::copy(&path, folder.join(name)).unwrap();
    }
    objc::rc::autoreleasepool(|| unsafe { readback(&path) });
}

#[test]
fn metal_cvpixelbuffer_encoder_roundtrip() {
    run(None);
}
#[test]
fn metal_cvpixelbuffer_pause_roundtrip() {
    run(Some(45));
}
