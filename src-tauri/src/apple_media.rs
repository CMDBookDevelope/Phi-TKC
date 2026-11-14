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
    collections::VecDeque,
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
    fn CMSampleBufferGetPresentationTimeStamp(sample: Ref) -> Time;
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

fn media_error(error: *mut Object, depth: usize) -> String {
    if error.is_null() {
        return "NSError=none".into();
    }
    unsafe {
        let domain: *mut Object = msg_send![error, domain];
        let code: i64 = msg_send![error, code];
        let mut result = format!(
            "domain={} code={code}: {}",
            description(domain),
            description(error)
        );
        if depth < 3 {
            let info: *mut Object = msg_send![error, userInfo];
            let key = string("NSUnderlyingError");
            let underlying: *mut Object = msg_send![info, objectForKey:key.0];
            if !underlying.is_null() && underlying != error {
                result.push_str(&format!(
                    "; underlying=[{}]",
                    media_error(underlying, depth + 1)
                ));
            }
        }
        result
    }
}

// AVAssetWriter can withhold readiness on one input until the other advances.
// Available must yield to the renderer if another video frame is needed; only
// MakeRoom and Finish wait, while continuing to service both inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PumpGoal {
    Available,
    MakeRoom,
    Finish,
}
const MAX_PENDING_VIDEO: usize = 64;

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
    pending_video: VecDeque<Cf>,
    audio: Obj,
    audio_format: Cf,
    width: u32,
    height: u32,
    fps: u32,
    rate: u32,
    video_frames: u64,
    audio_frames: usize,
    audio_total: Option<usize>,
    video_finished: bool,
    audio_finished: bool,
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
                && rate > 0
                && rate <= i32::MAX as u32,
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
                media_error(error, 0)
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
                pending_video: VecDeque::new(),
                audio,
                audio_format: Cf(audio_format),
                width,
                height,
                fps,
                rate,
                video_frames: 0,
                audio_frames: 0,
                audio_total: None,
                video_finished: false,
                audio_finished: false,
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
    pub fn frame(&mut self, rgba: &[u8], frame: u64, pcm16: &[u8]) -> Result<()> {
        self.prepare_audio(pcm16)?;
        ensure!(!self.video_finished, "video input is already finished");
        ensure!(
            frame == self.video_frames && frame < i64::MAX as u64,
            "video frames must be consecutive"
        );
        ensure!(
            rgba.len() == self.width as usize * self.height as usize * 4,
            "wrong RGBA frame size"
        );
        if self.pending_video.len() >= MAX_PENDING_VIDEO {
            self.pump(pcm16, PumpGoal::MakeRoom)?;
        }
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
            if self.pending_video.len() >= MAX_PENDING_VIDEO {
                self.pump(pcm16, PumpGoal::MakeRoom)?;
            }
            self.pending_video.push_back(sample);
        }
        self.video_frames = frame + 1;
        self.pump(pcm16, PumpGoal::Available)
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
            let state: i64 = msg_send![self.writer.0, status];
            let state_name = match state {
                0 => "unknown",
                1 => "writing",
                2 => "completed",
                3 => "failed",
                4 => "cancelled",
                _ => "unrecognized",
            };
            let error: *mut Object = msg_send![self.writer.0, error];
            format!(
                "status={state_name}({state}); video={} frames @ {} fps, pending={}; audio={}/{} frames @ {} Hz; {}",
                self.video_frames,
                self.fps,
                self.pending_video.len(),
                self.audio_frames,
                self.audio_total.unwrap_or(0),
                self.rate,
                media_error(error, 0)
            )
        }
    }
    fn ready(input: &Obj) -> bool {
        let ready: objc::runtime::BOOL = unsafe { msg_send![input.0, isReadyForMoreMediaData] };
        ready == YES
    }
    fn append_ready(&self, input: &Obj, sample: &Cf, track: &str) -> Result<()> {
        let ok: objc::runtime::BOOL = unsafe { msg_send![input.0,appendSampleBuffer:sample.0] };
        if ok != YES {
            let time = unsafe { CMSampleBufferGetPresentationTimeStamp(sample.0) };
            anyhow::bail!(
                "Cannot append MP4 {track} sample, PTS={}/{}: {}",
                time.value,
                time.scale,
                self.error()
            );
        }
        Ok(())
    }
    fn prepare_audio(&mut self, pcm16: &[u8]) -> Result<()> {
        ensure!(
            pcm16.len() % 4 == 0,
            "PCM data must contain complete stereo frames"
        );
        let total = pcm16.len() / 4;
        ensure!(
            self.audio_total.map_or(true, |previous| previous == total),
            "PCM length changed during rendering"
        );
        self.audio_total = Some(total);
        Ok(())
    }
    fn append_audio(&mut self, pcm16: &[u8], until: usize) -> Result<()> {
        let count = (until - self.audio_frames).min(1024);
        let data = &pcm16[self.audio_frames * 4..(self.audio_frames + count) * 4];
        // Bound temporary Objective-C allocations even when a blocked video
        // input needs a large amount of ready audio to advance interleaving.
        objc::rc::autoreleasepool(|| -> Result<()> {
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
                self.append_ready(&self.audio, &Cf(sample), "audio")?;
            }
            Ok(())
        })?;
        self.audio_frames += count;
        Ok(())
    }
    fn pump(&mut self, pcm16: &[u8], goal: PumpGoal) -> Result<()> {
        let total = pcm16.len() / 4;
        let audio_limit = ((self.video_frames as f64 * self.rate as f64 / self.fps as f64).round()
            as usize)
            .min(total);
        let mut idle_since = Instant::now();
        let mut rounds = 0;
        loop {
            let state: i64 = unsafe { msg_send![self.writer.0, status] };
            ensure!(
                state == 1,
                "MP4 writer stopped during {goal:?}: {}",
                self.error()
            );
            let mut progressed = false;
            if self.audio_frames == total && !self.audio_finished {
                unsafe {
                    let _: () = msg_send![self.audio.0, markAsFinished];
                }
                self.audio_finished = true;
                progressed = true;
            }
            if let Some(sample) = self.pending_video.front() {
                let input = self.video.as_ref().unwrap();
                if Self::ready(input) {
                    self.append_ready(input, sample, "video")?;
                    self.pending_video.pop_front();
                    progressed = true;
                }
            }
            // Normally audio follows the rendered video. If video is blocked,
            // allow ready audio past that target instead of waiting on video.
            // Recheck video after every PCM chunk so neither input is starved.
            if !self.audio_finished
                && (self.audio_frames < audio_limit
                    || !self.pending_video.is_empty()
                    || goal == PumpGoal::Finish)
                && Self::ready(&self.audio)
            {
                let until = if !self.pending_video.is_empty() || goal == PumpGoal::Finish {
                    total
                } else {
                    audio_limit
                };
                self.append_audio(pcm16, until)?;
                progressed = true;
            }
            if self.audio_frames == total && !self.audio_finished {
                unsafe {
                    let _: () = msg_send![self.audio.0, markAsFinished];
                }
                self.audio_finished = true;
                progressed = true;
            }
            if goal == PumpGoal::Finish && self.pending_video.is_empty() && !self.video_finished {
                unsafe {
                    let _: () = msg_send![self.video.as_ref().unwrap().0, markAsFinished];
                }
                self.video_finished = true;
                progressed = true;
            }
            rounds += 1;
            match goal {
                PumpGoal::Available if !progressed || rounds >= 64 => return Ok(()),
                PumpGoal::MakeRoom if self.pending_video.len() < MAX_PENDING_VIDEO => return Ok(()),
                PumpGoal::Finish if self.video_finished && self.audio_finished => return Ok(()),
                _ => {}
            }
            if progressed {
                idle_since = Instant::now();
            } else {
                ensure!(
                    idle_since.elapsed() < Duration::from_secs(30),
                    "MP4 writer stalled during {goal:?}: video_ready={} audio_ready={}; {}",
                    Self::ready(self.video.as_ref().unwrap()),
                    Self::ready(&self.audio),
                    self.error()
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    pub fn finish(&mut self, pcm16: &[u8]) -> Result<()> {
        ensure!(self.video.is_some(), "render contains no video frames");
        self.prepare_audio(pcm16)?;
        self.pump(pcm16, PumpGoal::Finish)?;
        unsafe {
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

    extern "C" {
        fn CMSampleBufferGetNumSamples(sample: Ref) -> i64;
        fn CMSampleBufferGetPresentationTimeStamp(sample: Ref) -> Time;
        fn CMSampleBufferGetDecodeTimeStamp(sample: Ref) -> Time;
        fn CMSampleBufferGetDuration(sample: Ref) -> Time;
        fn CMFormatDescriptionGetMediaSubType(format: Ref) -> u32;
        fn CMTimeGetSeconds(time: Time) -> f64;
        fn CMGetAttachment(buffer: Ref, key: Ref, mode: *mut u32) -> Ref;
        static kCFBooleanTrue: Ref;
        static kCMSampleBufferAttachmentKey_EmptyMedia: Ref;
        static kCMSampleBufferAttachmentKey_EndsPreviousSampleDuration: Ref;
    }

    #[derive(Default, Debug)]
    struct TrackSamples {
        count: u64,
        first_time: Option<f64>,
        end_time: f64,
    }

    unsafe fn read_track(asset: *mut Object, track: *mut Object, audio: bool) -> TrackSamples {
        let mut error: *mut Object = null_mut();
        let allocated: *mut Object = msg_send![class!(AVAssetReader), alloc];
        let reader: *mut Object = msg_send![allocated, initWithAsset:asset error:&mut error];
        assert!(
            !reader.is_null(),
            "Cannot create AVAssetReader: {}",
            description(error)
        );
        let reader = Obj(reader);
        let false_value: *mut Object = msg_send![class!(NSNumber), numberWithBool:NO];
        let _: () = msg_send![false_value, retain];
        let false_value = Obj(false_value);
        let settings = audio.then(|| {
            dictionary(&[
                ("AVFormatIDKey", &number(0x6c70636d)),
                ("AVLinearPCMBitDepthKey", &number(16)),
                ("AVLinearPCMIsFloatKey", &false_value),
                ("AVLinearPCMIsBigEndianKey", &false_value),
                ("AVLinearPCMIsNonInterleaved", &false_value),
            ])
        });
        let settings_pointer = settings.as_ref().map_or(null_mut(), |value| value.0);
        let allocated: *mut Object = msg_send![class!(AVAssetReaderTrackOutput), alloc];
        let output: *mut Object =
            msg_send![allocated, initWithTrack:track outputSettings:settings_pointer];
        assert!(
            !output.is_null(),
            "Cannot create AVAssetReader track output"
        );
        let output = Obj(output);
        let can_add: objc::runtime::BOOL = msg_send![reader.0, canAddOutput:output.0];
        assert_eq!(can_add, YES);
        let _: () = msg_send![reader.0, addOutput:output.0];
        let started: objc::runtime::BOOL = msg_send![reader.0, startReading];
        assert_eq!(started, YES, "Cannot start AVAssetReader");

        let mut result = TrackSamples::default();
        let mut previous = f64::NEG_INFINITY;
        loop {
            // Use a per-buffer pool as well as owned CF buffers: the long test
            // must not retain autoreleased reader objects until the whole clip ends.
            let next = objc::rc::autoreleasepool(|| {
                let sample: Ref = msg_send![output.0, copyNextSampleBuffer];
                if sample.is_null() {
                    return None;
                }
                let sample = Cf(sample);
                let count = CMSampleBufferGetNumSamples(sample.0);
                let time = CMTimeGetSeconds(CMSampleBufferGetPresentationTimeStamp(sample.0));
                let decode = CMTimeGetSeconds(CMSampleBufferGetDecodeTimeStamp(sample.0));
                let duration = CMTimeGetSeconds(CMSampleBufferGetDuration(sample.0));
                let empty_media = CMGetAttachment(
                    sample.0,
                    kCMSampleBufferAttachmentKey_EmptyMedia,
                    null_mut(),
                ) == kCFBooleanTrue;
                let ends_previous = CMGetAttachment(
                    sample.0,
                    kCMSampleBufferAttachmentKey_EndsPreviousSampleDuration,
                    null_mut(),
                ) == kCFBooleanTrue;
                Some((count, time, decode, duration, empty_media, ends_previous))
            });
            let Some((count, time, decode, duration, empty_media, ends_previous)) = next else {
                break;
            };
            if count == 0 && (empty_media || ends_previous) {
                // AVAssetReader also emits marker buffers for empty edits and
                // the end of the previous sample. They contain no media frames.
                // Reject unmarked zero-sample buffers and still require the exact
                // media frame count after reading the whole track.
                if ends_previous && result.count > 0 {
                    // This marker terminates the previous decode interval.
                    // Reordering is disabled, so PTS is also usable if DTS is absent.
                    let end = if decode.is_finite() { decode } else { time };
                    assert!(
                        end.is_finite() && end >= previous,
                        "Invalid end-of-sample marker: PTS={time}, DTS={decode}"
                    );
                    result.end_time = end;
                }
                eprintln!("Native MP4 {} marker: empty_media={empty_media}, ends_previous={ends_previous}, PTS={time:.6}s, DTS={decode:.6}s, duration={duration:.6}s",
                    if audio { "audio" } else { "video" });
                continue;
            }
            assert!(
                count > 0,
                "Unmarked empty sample buffer in {} track at PTS={time}, duration={duration}",
                if audio { "audio" } else { "video" }
            );
            assert!(time.is_finite() && duration.is_finite() && duration > 0.0);
            assert!(
                time > previous,
                "Non-increasing track PTS: {time} after {previous}"
            );
            if !audio {
                assert_eq!(count, 1, "Expected one H.264 frame per sample buffer");
            }
            result.first_time.get_or_insert(time);
            result.count += count as u64;
            result.end_time = time + duration;
            previous = time;
        }
        let state: isize = msg_send![reader.0, status];
        let error: *mut Object = msg_send![reader.0, error];
        assert_eq!(state, 2, "AVAssetReader failed: {}", description(error));
        result
    }

    unsafe fn read_media(path: &Path) -> (TrackSamples, TrackSamples) {
        let path = string(&path.to_string_lossy());
        let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath:path.0];
        let asset: *mut Object =
            msg_send![class!(AVURLAsset), URLAssetWithURL:url options:null_mut::<Object>()];
        assert!(!asset.is_null());
        let _: () = msg_send![asset, retain];
        let asset = Obj(asset);
        let video_type = string("vide");
        let audio_type = string("soun");
        let video_tracks: *mut Object = msg_send![asset.0, tracksWithMediaType:video_type.0];
        let audio_tracks: *mut Object = msg_send![asset.0, tracksWithMediaType:audio_type.0];
        let video_count: usize = msg_send![video_tracks, count];
        let audio_count: usize = msg_send![audio_tracks, count];
        assert_eq!(video_count, 1, "MP4 must contain one video track");
        assert!(
            audio_count <= 1,
            "MP4 contains unexpected extra audio tracks"
        );
        let video_track: *mut Object = msg_send![video_tracks, objectAtIndex:0usize];
        let descriptions: *mut Object = msg_send![video_track, formatDescriptions];
        let count: usize = msg_send![descriptions, count];
        assert!(count > 0, "H.264 track has no format description");
        let format: Ref = msg_send![descriptions, objectAtIndex:0usize];
        assert_eq!(CMFormatDescriptionGetMediaSubType(format), 0x61766331);
        let video = read_track(asset.0, video_track, false);
        let audio = if audio_count == 0 {
            TrackSamples::default()
        } else {
            let audio_track: *mut Object = msg_send![audio_tracks, objectAtIndex:0usize];
            let descriptions: *mut Object = msg_send![audio_track, formatDescriptions];
            let count: usize = msg_send![descriptions, count];
            if count == 0 {
                // A finished input with no PCM may leave an empty track. There
                // is no format to decode; a nonempty source fails below on count.
                TrackSamples::default()
            } else {
                let format: Ref = msg_send![descriptions, objectAtIndex:0usize];
                assert_eq!(CMFormatDescriptionGetMediaSubType(format), 0x61616320);
                read_track(asset.0, audio_track, true)
            }
        };
        (video, audio)
    }

    fn run_smoke(width: u32, height: u32, fps: u32, frames: u64, audio_frames: usize) {
        objc::rc::autoreleasepool(|| {
            const RATE: u32 = 48_000;
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("native.mp4");
            let mut encoder = Encoder::new(&path, width, height, fps, 4_000_000, RATE).unwrap();
            let red = [255u8, 0, 0, 255].repeat(width as usize * height as usize);
            let blue = [0u8, 0, 255, 255].repeat(width as usize * height as usize);
            // Nonzero stereo PCM lets the decoder test both sample count and
            // track presence rather than accidentally accepting an empty track.
            let audio = [1200i16.to_le_bytes(), (-1200i16).to_le_bytes()]
                .concat()
                .repeat(audio_frames);
            for frame in 0..frames {
                let rgba = if frame % 2 == 0 { &red } else { &blue };
                objc::rc::autoreleasepool(|| encoder.frame(rgba, frame, &audio).unwrap());
            }
            encoder.finish(&audio).unwrap();
            let state: isize = unsafe { msg_send![encoder.writer.0, status] };
            assert_eq!(state, 2, "MP4 writer did not complete");
            drop(encoder);

            let (video, audio) = unsafe { read_media(&path) };
            assert_eq!(
                video.count, frames,
                "Written MP4 has missing or extra video frames"
            );
            assert!(video.first_time.unwrap().abs() < 1e-6);
            let video_duration = frames as f64 / fps as f64;
            assert!((video.end_time - video_duration).abs() <= 1.0 / fps as f64);
            if audio_frames == 0 {
                assert_eq!(
                    audio.count, 0,
                    "Empty PCM unexpectedly produced audio samples"
                );
            } else {
                // AAC uses 1024-frame packets. Apple TN2258 describes common
                // 2112-frame priming plus a tail packet; leave room for both when
                // decoded while requiring the actual track and bounded duration.
                assert!(
                    audio.count.abs_diff(audio_frames as u64) <= 4096,
                    "Wrong decoded AAC length: {audio:?}"
                );
                let audio_duration = audio_frames as f64 / RATE as f64;
                let tolerance = 4096.0 / RATE as f64;
                assert!(audio.first_time.unwrap().abs() <= tolerance);
                assert!(
                    (audio.end_time - audio_duration).abs() <= tolerance,
                    "Wrong AAC duration: {audio:?}"
                );
            }
            let media_duration = video.end_time.max(audio.end_time);
            let expected = video_duration.max(audio_frames as f64 / RATE as f64);
            assert!(
                (media_duration - expected).abs() <= (1.0 / fps as f64).max(4096.0 / RATE as f64)
            );
            eprintln!("Native MP4 verified {width}x{height}: {frames} frames, {audio_frames} PCM frames, {media_duration:.3}s");
        });
    }

    #[test]
    fn videotoolbox_mp4_smoke() {
        run_smoke(64, 64, 30, 30, 48_000);
    }

    #[test]
    fn videotoolbox_empty_audio() {
        run_smoke(64, 64, 30, 24, 0);
    }

    #[test]
    fn videotoolbox_short_audio() {
        run_smoke(64, 64, 30, 90, 9_600);
    }

    #[test]
    fn videotoolbox_long_audio() {
        run_smoke(64, 64, 30, 30, 96_000);
    }

    #[test]
    fn videotoolbox_sustained_720p() {
        run_smoke(1280, 720, 30, 600, 20 * 48_000);
    }
}
