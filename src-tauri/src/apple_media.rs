//! VideoToolbox H.264 compression and AVFoundation AAC/MP4 writing.
//! No subprocesses or FFmpeg executable are used. Compression callbacks retain
//! their CMSampleBuffers until the writer has consumed them.
use anyhow::{ensure, Result};
use objc::{
    class, msg_send,
    runtime::{Object, NO, YES},
    sel, sel_impl, Encode, Encoding,
};
use std::{
    ffi::c_void,
    path::Path,
    ptr::null_mut,
    sync::Mutex,
    time::{Duration, Instant},
};
type Ref = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Time {
    value: i64,
    scale: i32,
    flags: u32,
    epoch: i64,
}
impl Time {
    fn new(value: i64, scale: u32) -> Self {
        Self {
            value,
            scale: scale as i32,
            flags: 1,
            epoch: 0,
        }
    }
}
unsafe impl Encode for Time {
    fn encode() -> Encoding {
        unsafe { Encoding::from_str("{CMTime=qiIq}") }
    }
}
#[repr(C)]
struct Timing {
    duration: Time,
    presentation: Time,
    decode: Time,
}
#[repr(C)]
struct AudioDescription {
    rate: f64,
    format: u32,
    flags: u32,
    bytes_packet: u32,
    frames_packet: u32,
    bytes_frame: u32,
    channels: u32,
    bits: u32,
    reserved: u32,
}

#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreFoundation", kind = "framework")
)]
extern "C" {
    fn CFRetain(value: Ref) -> Ref;
    fn CFRelease(value: Ref);
}
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreVideo", kind = "framework")
)]
extern "C" {
    fn CVPixelBufferCreate(
        allocator: Ref,
        width: usize,
        height: usize,
        format: u32,
        attributes: Ref,
        output: *mut Ref,
    ) -> i32;
    fn CVPixelBufferLockBaseAddress(buffer: Ref, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(buffer: Ref, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddress(buffer: Ref) -> Ref;
    fn CVPixelBufferGetBytesPerRow(buffer: Ref) -> usize;
}
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreMedia", kind = "framework")
)]
extern "C" {
    fn CMSampleBufferGetFormatDescription(sample: Ref) -> Ref;
    fn CMAudioFormatDescriptionCreate(
        allocator: Ref,
        description: *const AudioDescription,
        layout_size: usize,
        layout: Ref,
        cookie_size: usize,
        cookie: Ref,
        extensions: Ref,
        output: *mut Ref,
    ) -> i32;
    fn CMBlockBufferCreateWithMemoryBlock(
        allocator: Ref,
        memory: Ref,
        length: usize,
        block_allocator: Ref,
        source: Ref,
        offset: usize,
        data_length: usize,
        flags: u32,
        output: *mut Ref,
    ) -> i32;
    fn CMBlockBufferReplaceDataBytes(
        source: *const c_void,
        block: Ref,
        offset: usize,
        length: usize,
    ) -> i32;
    fn CMSampleBufferCreateReady(
        allocator: Ref,
        block: Ref,
        format: Ref,
        samples: i64,
        timing_count: i64,
        timing: *const Timing,
        size_count: i64,
        sizes: *const usize,
        output: *mut Ref,
    ) -> i32;
}
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "VideoToolbox", kind = "framework")
)]
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
        callback_context: Ref,
        output: *mut Ref,
    ) -> i32;
    fn VTSessionSetProperty(session: Ref, key: Ref, value: Ref) -> i32;
    fn VTCompressionSessionPrepareToEncodeFrames(session: Ref) -> i32;
    fn VTCompressionSessionEncodeFrame(
        session: Ref,
        image: Ref,
        presentation: Time,
        duration: Time,
        properties: Ref,
        context: Ref,
        flags: *mut u32,
    ) -> i32;
    fn VTCompressionSessionCompleteFrames(session: Ref, until: Time) -> i32;
    fn VTCompressionSessionInvalidate(session: Ref);
    static kVTCompressionPropertyKey_RealTime: Ref;
    static kVTCompressionPropertyKey_AllowFrameReordering: Ref;
    static kVTCompressionPropertyKey_AverageBitRate: Ref;
    static kVTCompressionPropertyKey_ExpectedFrameRate: Ref;
    static kVTCompressionPropertyKey_MaxKeyFrameInterval: Ref;
    static kVTCompressionPropertyKey_ProfileLevel: Ref;
    static kVTProfileLevel_H264_High_AutoLevel: Ref;
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
            unsafe {
                CFRelease(self.0);
            }
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
        let key_ptrs: Vec<*mut Object> = keys.iter().map(|key| key.0).collect();
        let values: Vec<*mut Object> = entries.iter().map(|(_, value)| value.0).collect();
        let object: *mut Object = msg_send![class!(NSDictionary), dictionaryWithObjects:values.as_ptr() forKeys:key_ptrs.as_ptr() count:values.len()];
        let _: () = msg_send![object, retain];
        Obj(object)
    }
}
fn status(code: i32, operation: &str) -> Result<()> {
    ensure!(code == 0, "{operation} failed (OSStatus {code})");
    Ok(())
}
fn description(object: *mut Object) -> String {
    unsafe {
        if object.is_null() {
            return "unknown Apple media error".into();
        }
        let text: *mut Object = msg_send![object, description];
        let bytes: *const i8 = msg_send![text, UTF8String];
        if bytes.is_null() {
            "unknown Apple media error".into()
        } else {
            std::ffi::CStr::from_ptr(bytes)
                .to_string_lossy()
                .into_owned()
        }
    }
}

#[derive(Default)]
struct CallbackState {
    samples: Vec<Cf>,
    error: Option<i32>,
}
extern "C" fn compressed(context: Ref, _frame: Ref, code: i32, flags: u32, sample: Ref) {
    // Never unwind through Apple's C callback ABI.
    let state = unsafe { &*(context as *const Mutex<CallbackState>) };
    if let Ok(mut state) = state.lock() {
        // Bit zero reports asynchronous encoding; bit one means frame dropped.
        if code != 0 || flags & 2 != 0 || sample.is_null() {
            state.error = Some(if code != 0 { code } else { -1 });
        } else {
            state.samples.push(Cf(unsafe { CFRetain(sample) }));
        }
    }
}

pub struct Encoder {
    session: Ref,
    callbacks: Box<Mutex<CallbackState>>,
    writer: Obj,
    video: Option<Obj>,
    audio: Obj,
    audio_format: Cf,
    width: u32,
    height: u32,
    fps: u32,
    rate: u32,
    audio_frames: usize,
    complete: bool,
}
impl Encoder {
    pub fn new(
        path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        rate: u32,
    ) -> Result<Self> {
        ensure!(
            width > 0
                && height > 0
                && width <= i32::MAX as u32
                && height <= i32::MAX as u32
                && fps > 0
                && fps <= i32::MAX as u32
                && rate > 0,
            "invalid Apple encoder dimensions or rates"
        );
        ensure!(!path.exists(), "output already exists: {}", path.display());
        unsafe {
            let path = string(&path.to_string_lossy());
            let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath:path.0];
            let kind = string("public.mpeg-4");
            let mut error: *mut Object = null_mut();
            let allocated: *mut Object = msg_send![class!(AVAssetWriter), alloc];
            let writer: *mut Object =
                msg_send![allocated, initWithURL:url fileType:kind.0 error:&mut error];
            ensure!(
                !writer.is_null(),
                "Cannot open MP4 output: {}",
                description(error)
            );
            let writer = Obj(writer);
            let codec = string("aac");
            let channels = number(2);
            let sample_rate = number(rate as i64);
            let audio_bitrate = number(192000);
            let settings = dictionary(&[
                ("AVFormatIDKey", &number(0x61616320)),
                ("AVNumberOfChannelsKey", &channels),
                ("AVSampleRateKey", &sample_rate),
                ("AVEncoderBitRateKey", &audio_bitrate),
            ]);
            let _ = codec;
            let media = string("soun");
            let audio: *mut Object = msg_send![class!(AVAssetWriterInput), assetWriterInputWithMediaType:media.0 outputSettings:settings.0];
            let _: () = msg_send![audio, retain];
            let audio = Obj(audio);
            let _: () = msg_send![audio.0, setExpectsMediaDataInRealTime:NO];
            let can: objc::runtime::BOOL = msg_send![writer.0, canAddInput:audio.0];
            ensure!(can == YES, "AVAssetWriter rejected the AAC track");
            let _: () = msg_send![writer.0, addInput:audio.0];
            let asbd = AudioDescription {
                rate: rate as f64,
                format: 0x6c70636d,
                flags: 4 | 8,
                bytes_packet: 4,
                frames_packet: 1,
                bytes_frame: 4,
                channels: 2,
                bits: 16,
                reserved: 0,
            };
            let mut audio_format = null_mut();
            status(
                CMAudioFormatDescriptionCreate(
                    null_mut(),
                    &asbd,
                    0,
                    null_mut(),
                    0,
                    null_mut(),
                    null_mut(),
                    &mut audio_format,
                ),
                "PCM format creation",
            )?;
            let mut encoder = Self {
                session: null_mut(),
                callbacks: Box::default(),
                writer,
                video: None,
                audio,
                audio_format: Cf(audio_format),
                width,
                height,
                fps,
                rate,
                audio_frames: 0,
                complete: false,
            };
            status(
                VTCompressionSessionCreate(
                    null_mut(),
                    width as i32,
                    height as i32,
                    0x61766331,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    compressed,
                    encoder.callbacks.as_mut() as *mut _ as Ref,
                    &mut encoder.session,
                ),
                "VideoToolbox session creation",
            )?;
            for (key, value) in [
                (kVTCompressionPropertyKey_RealTime, 0),
                (kVTCompressionPropertyKey_AllowFrameReordering, 0),
                (kVTCompressionPropertyKey_AverageBitRate, bitrate as i64),
                (kVTCompressionPropertyKey_ExpectedFrameRate, fps as i64),
                (
                    kVTCompressionPropertyKey_MaxKeyFrameInterval,
                    fps.saturating_mul(2) as i64,
                ),
            ] {
                // Boolean properties use true CFBoolean values, not CFNumber.
                let value = if key == kVTCompressionPropertyKey_RealTime
                    || key == kVTCompressionPropertyKey_AllowFrameReordering
                {
                    let object: *mut Object = msg_send![class!(NSNumber), numberWithBool:NO];
                    let _: () = msg_send![object, retain];
                    Obj(object)
                } else {
                    number(value)
                };
                status(
                    VTSessionSetProperty(encoder.session, key, value.0 as Ref),
                    "VideoToolbox property",
                )?;
            }
            status(
                VTSessionSetProperty(
                    encoder.session,
                    kVTCompressionPropertyKey_ProfileLevel,
                    kVTProfileLevel_H264_High_AutoLevel,
                ),
                "VideoToolbox profile",
            )?;
            status(
                VTCompressionSessionPrepareToEncodeFrames(encoder.session),
                "VideoToolbox preparation",
            )?;
            Ok(encoder)
        }
    }
    pub fn frame(&mut self, rgba: &[u8], frame: u64) -> Result<()> {
        ensure!(
            rgba.len() == self.width as usize * self.height as usize * 4,
            "wrong RGBA frame size"
        );
        unsafe {
            let mut pixel = null_mut();
            status(
                CVPixelBufferCreate(
                    null_mut(),
                    self.width as usize,
                    self.height as usize,
                    0x42475241,
                    null_mut(),
                    &mut pixel,
                ),
                "BGRA pixel buffer creation",
            )?;
            let pixel = Cf(pixel);
            status(
                CVPixelBufferLockBaseAddress(pixel.0, 0),
                "pixel buffer lock",
            )?;
            let stride = CVPixelBufferGetBytesPerRow(pixel.0);
            let base = CVPixelBufferGetBaseAddress(pixel.0) as *mut u8;
            for y in 0..self.height as usize {
                let output =
                    std::slice::from_raw_parts_mut(base.add(y * stride), self.width as usize * 4);
                for (source, target) in rgba
                    [y * self.width as usize * 4..(y + 1) * self.width as usize * 4]
                    .chunks_exact(4)
                    .zip(output.chunks_exact_mut(4))
                {
                    target.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
                }
            }
            status(
                CVPixelBufferUnlockBaseAddress(pixel.0, 0),
                "pixel buffer unlock",
            )?;
            let time = Time::new(frame as i64, self.fps);
            status(
                VTCompressionSessionEncodeFrame(
                    self.session,
                    pixel.0,
                    time,
                    Time::new(1, self.fps),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                ),
                "VideoToolbox frame encode",
            )?;
            status(
                VTCompressionSessionCompleteFrames(self.session, time),
                "VideoToolbox frame completion",
            )?;
        }
        let samples = {
            let mut state = self
                .callbacks
                .lock()
                .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?;
            if let Some(error) = state.error.take() {
                status(error, "VideoToolbox callback")?;
            }
            std::mem::take(&mut state.samples)
        };
        ensure!(!samples.is_empty(), "VideoToolbox produced no frame");
        for sample in samples {
            self.start_video(&sample)?;
            self.append(self.video.as_ref().unwrap(), &sample)?;
        }
        Ok(())
    }
    fn start_video(&mut self, sample: &Cf) -> Result<()> {
        if self.video.is_some() {
            return Ok(());
        }
        unsafe {
            let media = string("vide");
            let hint = CMSampleBufferGetFormatDescription(sample.0);
            let video: *mut Object = msg_send![class!(AVAssetWriterInput), assetWriterInputWithMediaType:media.0 outputSettings:null_mut::<Object>() sourceFormatHint:hint];
            ensure!(!video.is_null(), "Cannot create the H.264 track");
            let _: () = msg_send![video, retain];
            let video = Obj(video);
            let _: () = msg_send![video.0,setExpectsMediaDataInRealTime:NO];
            let can: objc::runtime::BOOL = msg_send![self.writer.0,canAddInput:video.0];
            ensure!(can == YES, "AVAssetWriter rejected the H.264 track");
            let _: () = msg_send![self.writer.0,addInput:video.0];
            let started: objc::runtime::BOOL = msg_send![self.writer.0, startWriting];
            ensure!(started == YES, "Cannot start MP4 writer: {}", self.error());
            let _: () = msg_send![self.writer.0,startSessionAtSourceTime:Time::new(0,self.fps)];
            self.video = Some(video);
        }
        Ok(())
    }
    fn error(&self) -> String {
        unsafe {
            let error: *mut Object = msg_send![self.writer.0, error];
            description(error)
        }
    }
    fn append(&self, input: &Obj, sample: &Cf) -> Result<()> {
        let start = Instant::now();
        loop {
            unsafe {
                let ready: objc::runtime::BOOL = msg_send![input.0, isReadyForMoreMediaData];
                if ready == YES {
                    break;
                }
                let state: i64 = msg_send![self.writer.0, status];
                ensure!(state == 1, "MP4 writer stopped: {}", self.error());
            }
            ensure!(
                start.elapsed() < Duration::from_secs(30),
                "MP4 writer stalled"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let ok: objc::runtime::BOOL = unsafe { msg_send![input.0,appendSampleBuffer:sample.0] };
        ensure!(ok == YES, "Cannot append MP4 sample: {}", self.error());
        Ok(())
    }
    pub fn audio_until(&mut self, pcm16: &[u8], until: usize) -> Result<()> {
        ensure!(
            pcm16.len() % 4 == 0,
            "PCM data must contain complete stereo frames"
        );
        ensure!(
            self.video.is_some(),
            "encode the first video frame before audio"
        );
        let until = until.min(pcm16.len() / 4);
        while self.audio_frames < until {
            let count = (until - self.audio_frames).min(1024);
            let data = &pcm16[self.audio_frames * 4..(self.audio_frames + count) * 4];
            unsafe {
                let mut block = null_mut();
                status(
                    CMBlockBufferCreateWithMemoryBlock(
                        null_mut(),
                        null_mut(),
                        data.len(),
                        null_mut(),
                        null_mut(),
                        0,
                        data.len(),
                        0,
                        &mut block,
                    ),
                    "PCM block allocation",
                )?;
                let block = Cf(block);
                status(
                    CMBlockBufferReplaceDataBytes(data.as_ptr() as _, block.0, 0, data.len()),
                    "PCM block copy",
                )?;
                let timing = Timing {
                    duration: Time::new(1, self.rate),
                    presentation: Time::new(self.audio_frames as i64, self.rate),
                    decode: Time {
                        value: 0,
                        scale: 0,
                        flags: 0,
                        epoch: 0,
                    },
                };
                let mut sample = null_mut();
                let size = 4usize;
                status(
                    CMSampleBufferCreateReady(
                        null_mut(),
                        block.0,
                        self.audio_format.0,
                        count as i64,
                        1,
                        &timing,
                        1,
                        &size,
                        &mut sample,
                    ),
                    "PCM sample creation",
                )?;
                self.append(&self.audio, &Cf(sample))?;
            }
            self.audio_frames += count;
        }
        Ok(())
    }
    pub fn finish(&mut self, pcm16: &[u8]) -> Result<()> {
        ensure!(self.video.is_some(), "render contains no video frames");
        unsafe {
            let _: () = msg_send![self.video.as_ref().unwrap().0, markAsFinished];
        }
        self.audio_until(pcm16, pcm16.len() / 4)?;
        unsafe {
            let _: () = msg_send![self.audio.0, markAsFinished];
            let ok: objc::runtime::BOOL = msg_send![self.writer.0, finishWriting];
            ensure!(ok == YES, "Cannot finalize MP4: {}", self.error());
        }
        self.complete = true;
        Ok(())
    }
}
impl Drop for Encoder {
    fn drop(&mut self) {
        unsafe {
            if !self.session.is_null() {
                VTCompressionSessionInvalidate(self.session);
                CFRelease(self.session);
            }
            if !self.complete {
                let _: () = msg_send![self.writer.0, cancelWriting];
            }
        }
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "ios")))]
mod tests {
    use super::*;
    #[test]
    fn videotoolbox_mp4_smoke() {
        objc::rc::autoreleasepool(|| {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("native.mp4");
            let mut encoder = Encoder::new(&path,64,64,30,1_000_000,48_000).unwrap();
            let rgba = [255u8,0,0,255].repeat(64*64);
            let audio = vec![0u8;48_000*4];
            for frame in 0..30 {
                encoder.frame(&rgba,frame).unwrap();
                encoder.audio_until(&audio,(frame as usize+1)*1600).unwrap();
            }
            encoder.finish(&audio).unwrap();
            let bytes=std::fs::read(path).unwrap();
            assert!(bytes.len()>1024);
            assert_eq!(&bytes[4..8],b"ftyp");
            assert!(bytes.windows(4).any(|tag|tag==b"mdat"));
            assert!(bytes.windows(4).any(|tag|tag==b"moov"));
        });
    }
}
