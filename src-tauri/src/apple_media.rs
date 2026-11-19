//! VideoToolbox compression and AVFoundation AAC/MP4 and MOV writing.
//! No subprocesses or FFmpeg executable are used. Compression callbacks retain
//! their CMSampleBuffers until the writer has consumed them.
use crate::apple_render::mobile_hdr::{is_10_bit_420_sps, Transfer};
use anyhow::{ensure, Context, Result};
use objc::{
    class, msg_send,
    runtime::{Object, NO, YES},
    sel, sel_impl, Encode, Encoding,
};
use std::{
    collections::{BTreeMap, VecDeque},
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
    fn CFEqual(first: Ref, second: Ref) -> u8;
}
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreVideo", kind = "framework")
)]
extern "C" {
    fn CVPixelBufferPoolCreate(
        allocator: Ref,
        pool_attributes: Ref,
        pixel_attributes: Ref,
        output: *mut Ref,
    ) -> i32;
    fn CVPixelBufferPoolCreatePixelBufferWithAuxAttributes(
        allocator: Ref,
        pool: Ref,
        auxiliary: Ref,
        output: *mut Ref,
    ) -> i32;
    fn CVPixelBufferLockBaseAddress(buffer: Ref, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(buffer: Ref, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddress(buffer: Ref) -> Ref;
    fn CVPixelBufferGetBytesPerRow(buffer: Ref) -> usize;
    fn CVPixelBufferGetWidth(buffer: Ref) -> usize;
    fn CVPixelBufferGetHeight(buffer: Ref) -> usize;
    fn CVPixelBufferGetPixelFormatType(buffer: Ref) -> u32;
    fn CVPixelBufferGetPlaneCount(buffer: Ref) -> usize;
    fn CVPixelBufferGetBaseAddressOfPlane(buffer: Ref, plane: usize) -> Ref;
    fn CVPixelBufferGetBytesPerRowOfPlane(buffer: Ref, plane: usize) -> usize;
    fn CVBufferSetAttachment(buffer: Ref, key: Ref, value: Ref, mode: u32);
    static kCVImageBufferColorPrimariesKey: Ref;
    static kCVImageBufferTransferFunctionKey: Ref;
    static kCVImageBufferYCbCrMatrixKey: Ref;
    static kCVImageBufferColorPrimaries_ITU_R_2020: Ref;
    static kCVImageBufferColorPrimaries_ITU_R_709_2: Ref;
    static kCVImageBufferTransferFunction_ITU_R_709_2: Ref;
    static kCVImageBufferYCbCrMatrix_ITU_R_709_2: Ref;
    static kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ: Ref;
    static kCVImageBufferTransferFunction_ITU_R_2100_HLG: Ref;
    static kCVImageBufferYCbCrMatrix_ITU_R_2020: Ref;
    static kCVPixelBufferWidthKey: Ref;
    static kCVPixelBufferHeightKey: Ref;
    static kCVPixelBufferPixelFormatTypeKey: Ref;
    static kCVPixelBufferMetalCompatibilityKey: Ref;
    static kCVPixelBufferIOSurfacePropertiesKey: Ref;
    static kCVPixelBufferPoolMinimumBufferCountKey: Ref;
    static kCVPixelBufferPoolAllocationThresholdKey: Ref;
}
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreMedia", kind = "framework")
)]
extern "C" {
    fn CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
        format: Ref,
        index: usize,
        bytes: *mut *const u8,
        length: *mut usize,
        count: *mut usize,
        nal_header_length: *mut i32,
    ) -> i32;
    fn CMFormatDescriptionGetMediaSubType(format: Ref) -> u32;
    fn CMSampleBufferGetDataBuffer(sample: Ref) -> Ref;
    fn CMBlockBufferCopyDataBytes(block: Ref, offset: usize, length: usize, output: *mut c_void) -> i32;
    fn CMFormatDescriptionGetExtension(format: Ref, key: Ref) -> Ref;
    static kCMFormatDescriptionExtension_ColorPrimaries: Ref;
    static kCMFormatDescriptionExtension_TransferFunction: Ref;
    static kCMFormatDescriptionExtension_YCbCrMatrix: Ref;
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
    static kVTEncodeFrameOptionKey_ForceKeyFrame: Ref;
    static kVTCompressionPropertyKey_MaxFrameDelayCount: Ref;
    static kVTProfileLevel_H264_High_AutoLevel: Ref;
    static kVTProfileLevel_HEVC_Main10_AutoLevel: Ref;
    static kVTCompressionPropertyKey_ColorPrimaries: Ref;
    static kVTCompressionPropertyKey_TransferFunction: Ref;
    static kVTCompressionPropertyKey_YCbCrMatrix: Ref;
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

/// An owned BGRA8 IOSurface from the encoder's reusable pixel buffer pool.
/// Keep this lease alive while Metal writes the surface. Submit it only after
/// those writes have completed, and do not write the surface after submission.
pub struct PixelBuffer(Cf);
impl PixelBuffer {
    pub fn as_ptr(&self) -> *mut c_void {
        self.0 .0
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
fn reference_dictionary(entries: &[(Ref, Ref)]) -> Obj {
    unsafe {
        let keys: Vec<Ref> = entries.iter().map(|(key, _)| *key).collect();
        let values: Vec<Ref> = entries.iter().map(|(_, value)| *value).collect();
        let object: *mut Object = msg_send![class!(NSDictionary), dictionaryWithObjects:values.as_ptr() forKeys:keys.as_ptr() count:values.len()];
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
const MAX_IN_FLIGHT_VIDEO: usize = 6;
// Six VideoToolbox inputs, three GPU frame leases and one allocation margin.
const MAX_PIXEL_BUFFERS: usize = MAX_IN_FLIGHT_VIDEO + 4;
const BGRA: u32 = u32::from_be_bytes(*b"BGRA");
const X420: u32 = u32::from_be_bytes(*b"x420");

const X422: u32 = u32::from_be_bytes(*b"x422");
const AYUV64: u32 = u32::from_be_bytes(*b"y416");

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProRes {
    tag: u32,
    pub(crate) alpha: bool,
}

impl ProRes {
    const PROFILES: [(&'static str, [u8; 4], bool); 7] = [
        ("prores_422_proxy", *b"apco", false),
        ("prores_422_lt", *b"apcs", false),
        ("prores_422", *b"apcn", false),
        ("prores_422_hq", *b"apch", false),
        ("prores_4444", *b"ap4h", false),
        ("prores_4444_xq", *b"ap4x", true),
        ("prores_4444_alpha", *b"ap4h", true),
    ];

    pub(crate) const NAMES: [&'static str; 7] = [
        Self::PROFILES[0].0, Self::PROFILES[1].0, Self::PROFILES[2].0,
        Self::PROFILES[3].0, Self::PROFILES[4].0, Self::PROFILES[5].0, Self::PROFILES[6].0,
    ];

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::PROFILES.iter().find(|profile| profile.0 == name)
            .map(|&(_, tag, alpha)| Self { tag: u32::from_be_bytes(tag), alpha })
    }

    fn is_4444(self) -> bool {
        matches!(self.tag.to_be_bytes(), [b'a', b'p', b'4', b'h' | b'x'])
    }
}

fn pixel_format(hdr: Option<Transfer>, prores: Option<ProRes>) -> u32 {
    match (hdr, prores) {
        (_, Some(profile)) if profile.is_4444() => AYUV64,
        (_, Some(_)) => X422,
        (Some(_), None) => X420,
        _ => BGRA,
    }
}

fn transfer_value(transfer: Transfer) -> Ref {
    unsafe {
        match transfer {
            Transfer::Pq => kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ,
            Transfer::Hlg => kCVImageBufferTransferFunction_ITU_R_2100_HLG,
        }
    }
}
const POOL_THRESHOLD_EXCEEDED: i32 = -6689;
const INVALID_SESSION: i32 = -12903;

struct FrameInput {
    frame: u64,
    pixel: PixelBuffer,
}
struct EncodedFrame {
    sample: Cf,
    // Keep the source until this output joins the contiguous writer prefix.
    // After a session interruption, later P frames must be reencoded with the
    // gap rather than spliced onto another session's reference pictures.
    pixel: PixelBuffer,
}

#[derive(Default)]
struct CallbackState {
    samples: BTreeMap<u64, EncodedFrame>,
    // Register before EncodeFrame: its callback may run synchronously. The
    // sourceFrameRefCon is a unique integer token, never a pointer to a Box.
    pixels: BTreeMap<usize, FrameInput>,
    retry: BTreeMap<u64, PixelBuffer>,
    needs_resume: bool,
    error: Option<i32>,
    error_frame: Option<u64>,
}
extern "C" fn compressed(context: Ref, frame: Ref, code: i32, flags: u32, sample: Ref) {
    // Never unwind through Apple's C callback ABI.
    let state = unsafe { &*(context as *const Mutex<CallbackState>) };
    if let Ok(mut state) = state.lock() {
        let token = frame as usize;
        let Some(input) = state.pixels.remove(&token) else {
            // A cancelled or failed submission can receive a late callback.
            // Tokens never repeat, so it cannot refer to a resumed submission.
            return;
        };
        let FrameInput { frame, pixel } = input;
        if code == INVALID_SESSION {
            state.retry.insert(frame, pixel);
            state.needs_resume = true;
            return;
        }
        // Bit zero reports asynchronous encoding; bit one means frame dropped.
        if code != 0 || flags & 2 != 0 || sample.is_null() {
            if state.error.is_none() {
                state.error = Some(if code != 0 { code } else { -1 });
                state.error_frame = Some(frame);
            }
        } else {
            state.samples.entry(frame).or_insert_with(|| EncodedFrame {
                sample: Cf(unsafe { CFRetain(sample) }),
                pixel,
            });
        }
        // On success the source lease follows its compressed sample until it
        // joins the ordered writer prefix; an interruption can replay that source.
    }
}

pub struct Encoder {
    session: Ref,
    callbacks: Box<Mutex<CallbackState>>,
    pixel_pool: Cf,
    pixel_attributes: Obj,
    pixel_auxiliary: Obj,
    bounded_frame_delay: bool,
    force_next_keyframe: bool,
    keyframe_properties: Obj,
    bitrate: u32,
    hdr: Option<Transfer>,
    prores: Option<ProRes>,
    next_token: usize,
    next_output_frame: u64,
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
    #[cfg(any(target_os = "ios", feature = "apple-render-check"))]
    pub(crate) fn probe(path: &Path, transfer: Option<Transfer>, prores: Option<ProRes>) -> Result<()> {
        const WIDTH: u32 = 320;
        const HEIGHT: u32 = 240;
        const FPS: u32 = 30;
        const RATE: u32 = 48_000;
        const PIXELS: usize = (WIDTH * HEIGHT) as usize;
        objc::rc::autoreleasepool(|| {
            let mut encoder = Self::new_with_prores(
                path, WIDTH, HEIGHT, FPS, 2_000_000, RATE, transfer, prores,
            )?;
            let pcm = vec![0; (RATE / FPS * 4) as usize];
            if prores.is_some() {
                let pixel = encoder.pixel_buffer(&pcm)?;
                let rgba = if transfer.is_some() {
                    half::f16::from_f32(0.5).to_bits().to_ne_bytes().repeat(PIXELS * 4)
                } else {
                    vec![128; PIXELS * 4]
                };
                encoder.write_prores_pixel(&pixel, &rgba)?;
                encoder.frame_pixel(pixel, 0, &pcm)?;
            } else if transfer.is_some() {
                let pixel = encoder.pixel_buffer(&pcm)?;
                let mut p010 = (64_u16 << 6).to_le_bytes().repeat(PIXELS);
                p010.extend((512_u16 << 6).to_le_bytes().repeat(PIXELS / 2));
                encoder.write_hdr_pixel(&pixel, &p010)?;
                encoder.frame_pixel(pixel, 0, &pcm)?;
            } else {
                encoder.frame(&vec![128; PIXELS * 4], 0, &pcm)?;
            }
            encoder.finish(&pcm)
        })
    }

    pub fn new(
        path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        rate: u32,
        hdr: Option<Transfer>,
    ) -> Result<Self> {
        Self::new_with_prores(path, width, height, fps, bitrate, rate, hdr, None)
    }

    pub(crate) fn new_with_prores(
        path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
        rate: u32,
        hdr: Option<Transfer>,
        prores: Option<ProRes>,
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
            let kind = string(if prores.is_some() { "com.apple.quicktime-movie" } else { "public.mpeg-4" });
            let mut error: *mut Object = null_mut();
            let allocated: *mut Object = msg_send![class!(AVAssetWriter), alloc];
            let writer: *mut Object =
                msg_send![allocated, initWithURL:url fileType:kind.0 error:&mut error];
            ensure!(
                !writer.is_null(),
                "Cannot open Apple video output: {}",
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
            let audio_format = Cf(audio_format);
            let pixel_width = number(width as i64);
            let pixel_height = number(height as i64);
            let pixel_format = number(pixel_format(hdr, prores) as i64);
            let true_value: *mut Object = msg_send![class!(NSNumber), numberWithBool:YES];
            let _: () = msg_send![true_value, retain];
            let true_value = Obj(true_value);
            let surface_properties = dictionary(&[]);
            let pixel_attributes = reference_dictionary(&[
                (kCVPixelBufferWidthKey, pixel_width.0 as Ref),
                (kCVPixelBufferHeightKey, pixel_height.0 as Ref),
                (kCVPixelBufferPixelFormatTypeKey, pixel_format.0 as Ref),
                (kCVPixelBufferMetalCompatibilityKey, true_value.0 as Ref),
                (
                    kCVPixelBufferIOSurfacePropertiesKey,
                    surface_properties.0 as Ref,
                ),
            ]);
            let minimum_buffers = number(2);
            let pool_attributes = reference_dictionary(&[(
                kCVPixelBufferPoolMinimumBufferCountKey,
                minimum_buffers.0 as Ref,
            )]);
            let maximum_buffers = number(MAX_PIXEL_BUFFERS as i64);
            let pixel_auxiliary = reference_dictionary(&[(
                kCVPixelBufferPoolAllocationThresholdKey,
                maximum_buffers.0 as Ref,
            )]);
            let mut pixel_pool = null_mut();
            status(
                CVPixelBufferPoolCreate(
                    null_mut(),
                    pool_attributes.0 as Ref,
                    pixel_attributes.0 as Ref,
                    &mut pixel_pool,
                ),
                "IOSurface pixel buffer pool creation",
            )?;
            let mut encoder = Self {
                session: null_mut(),
                callbacks: Box::default(),
                pixel_pool: Cf(pixel_pool),
                pixel_attributes,
                bitrate,
                hdr,
                prores,
                next_token: 1,
                next_output_frame: 0,
                force_next_keyframe: true,
                keyframe_properties: reference_dictionary(&[(
                    kVTEncodeFrameOptionKey_ForceKeyFrame,
                    true_value.0 as Ref,
                )]),
                pixel_auxiliary,
                bounded_frame_delay: false,
                writer,
                video: None,
                pending_video: VecDeque::new(),
                audio,
                audio_format,
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
            encoder.create_session().with_context(|| {
                if prores.is_some() {
                    "Failed to open the requested VideoToolbox ProRes profile"
                } else if hdr.is_some() {
                    "This Apple device needs VideoToolbox HEVC Main10 with 10-bit 4:2:0 input for HDR. Try a lower resolution or frame rate, or choose SDR."
                } else {
                    "Failed to open the VideoToolbox H.264 encoder"
                }
            })?;
            Ok(encoder)
        }
    }
    fn create_session(&mut self) -> Result<()> {
        ensure!(
            self.session.is_null(),
            "compression session is already active"
        );
        self.force_next_keyframe = true;
        unsafe {
            let code = VTCompressionSessionCreate(
                null_mut(),
                self.width as i32,
                self.height as i32,
                self.prores.map_or_else(
                    || u32::from_be_bytes(if self.hdr.is_some() { *b"hvc1" } else { *b"avc1" }),
                    |profile| profile.tag,
                ),
                null_mut(),
                self.pixel_attributes.0 as Ref,
                null_mut(),
                compressed,
                self.callbacks.as_mut() as *mut _ as Ref,
                &mut self.session,
            );
            if code == INVALID_SESSION {
                self.invalidate_for_resume()?;
                return Ok(());
            }
            status(code, "VideoToolbox session creation")?;
            ensure!(
                !self.session.is_null(),
                "VideoToolbox returned a null session"
            );
            if self.prores.is_none() {
                for (key, value) in [
                    (kVTCompressionPropertyKey_RealTime, 0),
                    (kVTCompressionPropertyKey_AllowFrameReordering, 0),
                    (
                        kVTCompressionPropertyKey_AverageBitRate,
                        self.bitrate as i64,
                    ),
                    (kVTCompressionPropertyKey_ExpectedFrameRate, self.fps as i64),
                    (
                        kVTCompressionPropertyKey_MaxKeyFrameInterval,
                        self.fps.saturating_mul(2) as i64,
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
                    let code = VTSessionSetProperty(self.session, key, value.0 as Ref);
                    if code == INVALID_SESSION {
                        self.invalidate_for_resume()?;
                        return Ok(());
                    }
                    status(code, "VideoToolbox property")?;
                }
                let code = VTSessionSetProperty(
                    self.session,
                    kVTCompressionPropertyKey_ProfileLevel,
                    if self.hdr.is_some() {
                        kVTProfileLevel_HEVC_Main10_AutoLevel
                    } else {
                        kVTProfileLevel_H264_High_AutoLevel
                    },
                );
                if code == INVALID_SESSION {
                    self.invalidate_for_resume()?;
                    return Ok(());
                }
                status(code, "VideoToolbox profile")?;
            }
            if let Some(transfer) = self.hdr {
                for (key, value) in [
                    (kVTCompressionPropertyKey_ColorPrimaries, kCVImageBufferColorPrimaries_ITU_R_2020),
                    (kVTCompressionPropertyKey_TransferFunction, transfer_value(transfer)),
                    (kVTCompressionPropertyKey_YCbCrMatrix, kCVImageBufferYCbCrMatrix_ITU_R_2020),
                ] {
                    let code = VTSessionSetProperty(self.session, key, value);
                    if code == INVALID_SESSION {
                        self.invalidate_for_resume()?;
                        return Ok(());
                    }
                    status(code, "VideoToolbox HDR color property")?;
                }
            }
            // Limit the encoder's compression window so the producer can stop
            // at its bounded in-flight limit without waiting for a future frame.
            let delay = number((MAX_IN_FLIGHT_VIDEO - 2) as i64);
            let delay_status = VTSessionSetProperty(
                self.session,
                kVTCompressionPropertyKey_MaxFrameDelayCount,
                delay.0 as Ref,
            );
            if delay_status == INVALID_SESSION {
                self.invalidate_for_resume()?;
                return Ok(());
            }
            self.bounded_frame_delay = delay_status == 0;
            if !self.bounded_frame_delay {
                log::warn!("VideoToolbox frame delay bound unsupported ({delay_status}); flush only when the in-flight window fills");
            }
            let code = VTCompressionSessionPrepareToEncodeFrames(self.session);
            if code == INVALID_SESSION {
                self.invalidate_for_resume()?;
                return Ok(());
            }
            status(code, "VideoToolbox preparation")?;
        }
        Ok(())
    }

    /// Acquire a reusable Metal-compatible IOSurface. The caller must
    /// drop temporary texture aliases after completing GPU writes so the pool
    /// can recycle the surface when the encoder releases its input ownership.
    pub fn pixel_buffer(&mut self, pcm16: &[u8]) -> Result<PixelBuffer> {
        self.prepare_audio(pcm16)?;
        ensure!(
            !self.video_finished && !self.complete,
            "video input is already finished"
        );
        self.wait_capacity(pcm16)?;
        let mut idle_since = Instant::now();
        loop {
            let mut pixel = null_mut();
            let code = unsafe {
                CVPixelBufferPoolCreatePixelBufferWithAuxAttributes(
                    null_mut(),
                    self.pixel_pool.0,
                    self.pixel_auxiliary.0 as Ref,
                    &mut pixel,
                )
            };
            let pixel = Cf(pixel);
            if code == 0 {
                ensure!(
                    !pixel.0.is_null(),
                    "pixel buffer pool returned a null buffer"
                );
                return Ok(PixelBuffer(pixel));
            }
            drop(pixel);
            if code != POOL_THRESHOLD_EXCEEDED {
                status(code, "IOSurface pixel buffer pool allocation")?;
            }
            ensure!(
                !self.needs_resume()?,
                "IOSurface allocation interrupted: VideoToolbox requires foreground resume"
            );
            let before = self.in_flight()?;
            self.service(pcm16)?;
            ensure!(
                !self.needs_resume()?,
                "IOSurface allocation interrupted: VideoToolbox requires foreground resume"
            );
            if self.in_flight()? < before {
                idle_since = Instant::now();
            }
            ensure!(idle_since.elapsed() < Duration::from_secs(30),
                "IOSurface pool exhausted: limit={MAX_PIXEL_BUFFERS}, in_flight={}; {}; release completed Metal texture aliases",
                self.in_flight()?, self.error());
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Submit an owned pixel buffer after the caller completes all GPU writes.
    /// It must never modify that surface again after calling this method.
    pub fn frame_pixel(&mut self, pixel: PixelBuffer, frame: u64, pcm16: &[u8]) -> Result<()> {
        self.prepare_audio(pcm16)?;
        ensure!(
            !self.video_finished && !self.complete,
            "video input is already finished"
        );
        ensure!(
            frame == self.video_frames && frame < i64::MAX as u64 && frame < usize::MAX as u64 - 1,
            "video frames must be consecutive"
        );
        unsafe {
            ensure!(
                CVPixelBufferGetWidth(pixel.as_ptr()) == self.width as usize
                    && CVPixelBufferGetHeight(pixel.as_ptr()) == self.height as usize
                    && CVPixelBufferGetPixelFormatType(pixel.as_ptr()) == pixel_format(self.hdr, self.prores),
                "wrong VideoToolbox pixel buffer size or format"
            );
        }
        self.submit_pixel(pixel, frame, pcm16)?;
        self.video_frames = frame + 1;
        self.service(pcm16)
    }

    /// Transfer a completed GPU surface while the compression session is
    /// suspended. This performs no writer service or VideoToolbox calls, so
    /// background quiescence cannot wait on an AVAssetWriter input.
    pub fn defer_pixel(&mut self, pixel: PixelBuffer, frame: u64, pcm16: &[u8]) -> Result<()> {
        self.prepare_audio(pcm16)?;
        ensure!(
            !self.video_finished && !self.complete,
            "video input is already finished"
        );
        ensure!(
            self.needs_resume()?,
            "cannot defer a frame while VideoToolbox is active"
        );
        ensure!(
            frame == self.video_frames && frame < i64::MAX as u64 && frame < usize::MAX as u64 - 1,
            "video frames must be consecutive"
        );
        unsafe {
            ensure!(
                CVPixelBufferGetWidth(pixel.as_ptr()) == self.width as usize
                    && CVPixelBufferGetHeight(pixel.as_ptr()) == self.height as usize
                    && CVPixelBufferGetPixelFormatType(pixel.as_ptr()) == pixel_format(self.hdr, self.prores),
                "wrong VideoToolbox pixel buffer size or format"
            );
        }
        self.callbacks
            .lock()
            .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?
            .retry
            .insert(frame, pixel);
        self.video_frames = frame + 1;
        Ok(())
    }

    fn submit_pixel(&mut self, pixel: PixelBuffer, frame: u64, pcm16: &[u8]) -> Result<()> {
        self.wait_capacity(pcm16)?;
        if self.needs_resume()? {
            self.callbacks
                .lock()
                .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?
                .retry
                .insert(frame, pixel);
            return Ok(());
        }
        let token = self.next_token;
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("compression submission token overflow"))?;
        let image = pixel.as_ptr();
        {
            let mut state = self
                .callbacks
                .lock()
                .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?;
            state.pixels.insert(token, FrameInput { frame, pixel });
        }
        let mut flags = 0;
        let properties = if self.force_next_keyframe && self.prores.is_none() {
            self.keyframe_properties.0 as Ref
        } else {
            null_mut()
        };
        let code = unsafe {
            VTCompressionSessionEncodeFrame(
                self.session,
                image,
                Time::new(frame as i64, self.fps),
                Time::new(1, self.fps),
                properties,
                token as Ref,
                &mut flags,
            )
        };
        if code == INVALID_SESSION {
            let mut state = self
                .callbacks
                .lock()
                .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?;
            if let Some(input) = state.pixels.remove(&token) {
                state.retry.insert(input.frame, input.pixel);
            }
            state.needs_resume = true;
            return Ok(());
        }
        if code != 0 || flags & 2 != 0 {
            // The callback may already have removed this token. Never reconstruct
            // a Box from sourceFrameRefCon or release another generation's input.
            let mut state = self
                .callbacks
                .lock()
                .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?;
            state.pixels.remove(&token);
            anyhow::bail!(
                "VideoToolbox frame {frame} encode failed: OSStatus={code}, flags={flags}; {}",
                self.error()
            );
        }
        self.force_next_keyframe = false;
        Ok(())
    }

    pub fn needs_resume(&self) -> Result<bool> {
        self.callbacks
            .lock()
            .map(|state| state.needs_resume || self.session.is_null())
            .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))
    }

    /// Called on the rendering thread before the app enters the background.
    /// Keep the writer and every unencoded pixel so foreground resume can replay
    /// the original global frame indices without dropping a frame.
    pub fn suspend(&mut self, pcm16: &[u8]) -> Result<()> {
        self.prepare_audio(pcm16)?;
        let code = if self.session.is_null() {
            0
        } else {
            unsafe {
                VTCompressionSessionCompleteFrames(
                    self.session,
                    Time {
                        value: 0,
                        scale: 0,
                        flags: 0,
                        epoch: 0,
                    },
                )
            }
        };
        self.invalidate_for_resume()?;
        // Failed flush still requires deterministic teardown; only InvalidSession
        // is recoverable. Other VideoToolbox errors retain the normal failure path.
        if code != INVALID_SESSION {
            status(code, "VideoToolbox suspend completion")?;
        }
        // No writer waits here: UIApplication's quiesce gate needs the GPU and
        // VideoToolbox stopped promptly. Outputs are retained for foreground.
        self.check_callback_error()
    }

    fn invalidate_for_resume(&mut self) -> Result<()> {
        if !self.session.is_null() {
            unsafe {
                VTCompressionSessionInvalidate(self.session);
                CFRelease(self.session);
            }
            self.session = null_mut();
        }
        let mut state = self
            .callbacks
            .lock()
            .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?;
        state.needs_resume = true;
        let encoded = std::mem::take(&mut state.samples);
        for (frame, output) in encoded {
            if frame >= self.next_output_frame {
                state.retry.entry(frame).or_insert(output.pixel);
            }
        }
        let remaining = std::mem::take(&mut state.pixels);
        for (_, input) in remaining {
            if input.frame >= self.next_output_frame {
                state.retry.entry(input.frame).or_insert(input.pixel);
            }
        }
        Ok(())
    }

    /// Recreate VideoToolbox on foreground activation and replay retained pixels
    /// in their original frame order. The first encoded frame is forced to IDR.
    pub fn resume(&mut self, pcm16: &[u8]) -> Result<()> {
        self.prepare_audio(pcm16)?;
        if !self.needs_resume()? {
            return Ok(());
        }
        self.invalidate_for_resume()?;
        self.create_session()?;
        if self.session.is_null() {
            return Ok(());
        }
        self.callbacks
            .lock()
            .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?
            .needs_resume = false;
        loop {
            if self.needs_resume()? {
                break;
            }
            let next = {
                let mut state = self
                    .callbacks
                    .lock()
                    .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?;
                let next = state.retry.keys().next().copied();
                next.map(|frame| (frame, state.retry.remove(&frame).unwrap()))
            };
            let Some((frame, pixel)) = next else {
                break;
            };
            self.submit_pixel(pixel, frame, pcm16)?;
            self.service(pcm16)?;
        }
        Ok(())
    }

    // CPU compatibility and the native baseline tests use the same pool and
    // asynchronous scheduler. Production Metal submits frame_pixel directly.
    pub(crate) fn write_hdr_pixel(&self, pixel: &PixelBuffer, p010: &[u8]) -> Result<()> {
        let transfer = self.hdr.context("HDR encoder is required for P010 frames")?;
        let row_bytes = self.width as usize * 2;
        let y_bytes = row_bytes * self.height as usize;
        ensure!(p010.len() == y_bytes + y_bytes / 2, "wrong P010 frame size");
        let buffer = pixel.as_ptr();
        unsafe {
            status(CVPixelBufferLockBaseAddress(buffer, 0), "10-bit pixel buffer lock")?;
            let result: Result<()> = (|| {
                ensure!(CVPixelBufferGetPlaneCount(buffer) == 2, "invalid x420 plane count");
                let (luma, chroma) = p010.split_at(y_bytes);
                for (plane, source) in [luma, chroma].into_iter().enumerate() {
                    let base = CVPixelBufferGetBaseAddressOfPlane(buffer, plane) as *mut u8;
                    let stride = CVPixelBufferGetBytesPerRowOfPlane(buffer, plane);
                    ensure!(!base.is_null() && stride >= row_bytes, "invalid x420 plane layout");
                    for (row, data) in source.chunks_exact(row_bytes).enumerate() {
                        std::ptr::copy_nonoverlapping(data.as_ptr(), base.add(row * stride), row_bytes);
                    }
                }
                Ok(())
            })();
            let unlock = CVPixelBufferUnlockBaseAddress(buffer, 0);
            result?;
            status(unlock, "10-bit pixel buffer unlock")?;
            for (key, value) in [
                (kCVImageBufferColorPrimariesKey, kCVImageBufferColorPrimaries_ITU_R_2020),
                (kCVImageBufferTransferFunctionKey, transfer_value(transfer)),
                (kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_2020),
            ] {
                CVBufferSetAttachment(buffer, key, value, 1);
            }
        }
        Ok(())
    }

    pub(crate) fn write_prores_pixel(&self, pixel: &PixelBuffer, rgba: &[u8]) -> Result<()> {
        let profile = self.prores.context("ProRes encoder is required")?;
        let width = self.width as usize;
        let height = self.height as usize;
        let bpp = if self.hdr.is_some() { 8 } else { 4 };
        ensure!(rgba.len() == width * height * bpp, "wrong ProRes RGBA frame size");
        let read = |sample: &[u8]| {
            let [r, g, b, a] = if let Some(transfer) = self.hdr {
                let [r, g, b, a] = [0, 2, 4, 6].map(|offset| {
                    half::f16::from_bits(u16::from_ne_bytes([sample[offset], sample[offset + 1]])).to_f32()
                });
                let [r, g, b] = transfer.encode_rgb([r, g, b]);
                [r, g, b, a]
            } else {
                std::array::from_fn(|index| f32::from(sample[index]) / 255.0)
            };
            let (kr, kb) = if self.hdr.is_some() { (0.2627, 0.0593) } else { (0.2126, 0.0722) };
            let y = r * kr + g * (1.0 - kr - kb) + b * kb;
            [a, y, (b - y) / (2.0 * (1.0 - kb)), (r - y) / (2.0 * (1.0 - kr))]
        };
        let buffer = pixel.as_ptr();
        unsafe {
            status(CVPixelBufferLockBaseAddress(buffer, 0), "ProRes pixel buffer lock")?;
            let result = (|| {
                if profile.is_4444() {
                    let base = CVPixelBufferGetBaseAddress(buffer).cast::<u8>();
                    let stride = CVPixelBufferGetBytesPerRow(buffer);
                    ensure!(!base.is_null() && stride >= width * 8, "invalid y416 layout");
                    for (row, source) in rgba.chunks_exact(width * bpp).enumerate() {
                        let destination = std::slice::from_raw_parts_mut(base.add(row * stride), width * 8);
                        for (sample, output) in source.chunks_exact(bpp).zip(destination.chunks_exact_mut(8)) {
                            let [a, y, cb, cr] = read(sample);
                            let a = if profile.alpha { (a.clamp(0.0, 1.0) * 65535.0).round() as u16 } else { u16::MAX };
                            let channels = [
                                a,
                                (4096.0 + 56064.0 * y).round() as u16,
                                (32768.0 + 57344.0 * cb).round() as u16,
                                (32768.0 + 57344.0 * cr).round() as u16,
                            ];
                            for (channel, target) in channels.into_iter().zip(output.chunks_exact_mut(2)) {
                                target.copy_from_slice(&channel.to_le_bytes());
                            }
                        }
                    }
                } else {
                    ensure!(width % 2 == 0 && CVPixelBufferGetPlaneCount(buffer) == 2, "invalid x422 layout");
                    let bases = [0, 1].map(|plane| CVPixelBufferGetBaseAddressOfPlane(buffer, plane).cast::<u8>());
                    let strides = [0, 1].map(|plane| CVPixelBufferGetBytesPerRowOfPlane(buffer, plane));
                    ensure!(bases.iter().all(|base| !base.is_null()) && strides.iter().all(|&stride| stride >= width * 2),
                        "invalid x422 plane layout");
                    for (row, source) in rgba.chunks_exact(width * bpp).enumerate() {
                        let [luma, chroma] = [0, 1].map(|plane| {
                            std::slice::from_raw_parts_mut(bases[plane].add(row * strides[plane]), width * 2)
                        });
                        for (pair, samples) in source.chunks_exact(bpp * 2).enumerate() {
                            let [_, y0, cb0, cr0] = read(&samples[..bpp]);
                            let [_, y1, cb1, cr1] = read(&samples[bpp..]);
                            let codes = [
                                [64.0 + 876.0 * y0, 64.0 + 876.0 * y1],
                                [512.0 + 448.0 * (cb0 + cb1), 512.0 + 448.0 * (cr0 + cr1)],
                            ];
                            for (destination, values) in [&mut *luma, &mut *chroma].into_iter().zip(codes) {
                                for (offset, value) in values.into_iter().enumerate() {
                                    let start = pair * 4 + offset * 2;
                                    destination[start..start + 2].copy_from_slice(&((value.round() as u16) << 6).to_le_bytes());
                                }
                            }
                        }
                    }
                }
                Ok(())
            })();
            let unlock = CVPixelBufferUnlockBaseAddress(buffer, 0);
            result?;
            status(unlock, "ProRes pixel buffer unlock")?;
            if let Some(transfer) = self.hdr {
                for (key, value) in [
                    (kCVImageBufferColorPrimariesKey, kCVImageBufferColorPrimaries_ITU_R_2020),
                    (kCVImageBufferTransferFunctionKey, transfer_value(transfer)),
                    (kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_2020),
                ] {
                    CVBufferSetAttachment(buffer, key, value, 1);
                }
            } else {
                for (key, value) in [
                    (kCVImageBufferColorPrimariesKey, kCVImageBufferColorPrimaries_ITU_R_709_2),
                    (kCVImageBufferTransferFunctionKey, kCVImageBufferTransferFunction_ITU_R_709_2),
                    (kCVImageBufferYCbCrMatrixKey, kCVImageBufferYCbCrMatrix_ITU_R_709_2),
                ] {
                    CVBufferSetAttachment(buffer, key, value, 1);
                }
            }
        }
        Ok(())
    }

    pub fn frame(&mut self, rgba: &[u8], frame: u64, pcm16: &[u8]) -> Result<()> {
        ensure!(
            rgba.len() == self.width as usize * self.height as usize * 4,
            "wrong RGBA frame size"
        );
        let pixel = self.pixel_buffer(pcm16)?;
        unsafe {
            status(
                CVPixelBufferLockBaseAddress(pixel.as_ptr(), 0),
                "pixel buffer lock",
            )?;
            let stride = CVPixelBufferGetBytesPerRow(pixel.as_ptr());
            let base = CVPixelBufferGetBaseAddress(pixel.as_ptr()) as *mut u8;
            if base.is_null() || stride < self.width as usize * 4 {
                let _ = CVPixelBufferUnlockBaseAddress(pixel.as_ptr(), 0);
                anyhow::bail!("invalid pooled BGRA pixel buffer layout");
            }
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
                CVPixelBufferUnlockBaseAddress(pixel.as_ptr(), 0),
                "pixel buffer unlock",
            )?;
        }
        self.frame_pixel(pixel, frame, pcm16)
    }

    fn in_flight(&self) -> Result<usize> {
        self.callbacks
            .lock()
            .map(|state| state.pixels.len())
            .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))
    }

    fn wait_capacity(&mut self, pcm16: &[u8]) -> Result<()> {
        self.service(pcm16)?;
        let idle_since = Instant::now();
        while self.in_flight()? >= MAX_IN_FLIGHT_VIDEO {
            if self.needs_resume()? {
                return Ok(());
            }
            if !self.bounded_frame_delay {
                let token = self
                    .callbacks
                    .lock()
                    .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?
                    .pixels
                    .values()
                    .next()
                    .unwrap()
                    .frame;
                let code = unsafe {
                    VTCompressionSessionCompleteFrames(
                        self.session,
                        Time::new(token as i64, self.fps),
                    )
                };
                if code == INVALID_SESSION {
                    self.invalidate_for_resume()?;
                    return Ok(());
                }
                status(code, "VideoToolbox bounded-window completion")?;
            }
            self.service(pcm16)?;
            if self.in_flight()? < MAX_IN_FLIGHT_VIDEO {
                break;
            }
            ensure!(
                idle_since.elapsed() < Duration::from_secs(30),
                "VideoToolbox callback stalled: in_flight={}; {}",
                self.in_flight()?,
                self.error()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    fn drain_callbacks(&mut self, pcm16: &[u8]) -> Result<()> {
        loop {
            let sample = {
                let mut state = self
                    .callbacks
                    .lock()
                    .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?;
                if let Some(error) = state.error.take() {
                    anyhow::bail!(
                        "VideoToolbox callback failed: frame={:?}, OSStatus={error}; {}",
                        state.error_frame.take(),
                        self.error()
                    );
                }
                state
                    .samples
                    .retain(|frame, _| *frame >= self.next_output_frame);
                // Leave ownership in callback state if the writer queue is full.
                if self.pending_video.len() >= MAX_PENDING_VIDEO {
                    None
                } else {
                    state.samples.remove(&self.next_output_frame)
                }
            };
            if let Some(output) = sample {
                self.start_video(&output.sample)?;
                self.pending_video.push_back(output.sample);
                self.next_output_frame += 1;
                continue;
            }
            let has_next = self
                .callbacks
                .lock()
                .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?
                .samples
                .contains_key(&self.next_output_frame);
            if has_next && self.pending_video.len() >= MAX_PENDING_VIDEO {
                self.pump(pcm16, PumpGoal::MakeRoom)?;
                continue;
            }
            return Ok(());
        }
    }

    fn check_callback_error(&mut self) -> Result<()> {
        let mut state = self
            .callbacks
            .lock()
            .map_err(|_| anyhow::anyhow!("compression callback lock poisoned"))?;
        if let Some(error) = state.error.take() {
            anyhow::bail!(
                "VideoToolbox callback failed: frame={:?}, OSStatus={error}; {}",
                state.error_frame.take(),
                self.error()
            );
        }
        Ok(())
    }

    fn service(&mut self, pcm16: &[u8]) -> Result<()> {
        self.drain_callbacks(pcm16)?;
        // The writer starts only after the first asynchronous sample supplies
        // its H.264 format description. Unknown is normal before that point.
        if self.video.is_some() {
            self.pump(pcm16, PumpGoal::Available)?;
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
            ensure!(!hint.is_null(), "VideoToolbox returned no video format description");
            if let Some(profile) = self.prores {
                ensure!(CMFormatDescriptionGetMediaSubType(hint) == profile.tag,
                    "VideoToolbox returned a different ProRes profile");
                if profile.alpha {
                    let block = CMSampleBufferGetDataBuffer(sample.0);
                    let mut header = [0_u8; 26];
                    ensure!(!block.is_null(), "ProRes sample contains no encoded frame");
                    status(CMBlockBufferCopyDataBytes(block, 0, header.len(), header.as_mut_ptr().cast()),
                        "ProRes frame header read")?;
                    ensure!(&header[4..8] == b"icpf" && matches!(header[25] & 15, 1 | 2),
                        "VideoToolbox did not encode a ProRes alpha plane");
                }
            }
            if let Some(transfer) = self.hdr {
                if self.prores.is_none() {
                    let mut count = 0;
                    let mut bytes = std::ptr::null();
                    let mut length = 0;
                    status(
                        CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
                            hint, 0, &mut bytes, &mut length, &mut count, null_mut(),
                        ),
                        "VideoToolbox HEVC parameter set query",
                    )?;
                    let mut main10 = false;
                    for index in 0..count {
                        status(
                            CMVideoFormatDescriptionGetHEVCParameterSetAtIndex(
                                hint, index, &mut bytes, &mut length, null_mut(), null_mut(),
                            ),
                            "VideoToolbox HEVC parameter set read",
                        )?;
                        if !bytes.is_null() && is_10_bit_420_sps(std::slice::from_raw_parts(bytes, length)) == Some(true) {
                            main10 = true;
                            break;
                        }
                    }
                    ensure!(main10, "VideoToolbox did not produce 10-bit 4:2:0 HEVC. Try a different device or choose SDR.");
                }
                for (key, expected) in [
                    (kCMFormatDescriptionExtension_ColorPrimaries, kCVImageBufferColorPrimaries_ITU_R_2020),
                    (kCMFormatDescriptionExtension_TransferFunction, transfer_value(transfer)),
                    (kCMFormatDescriptionExtension_YCbCrMatrix, kCVImageBufferYCbCrMatrix_ITU_R_2020),
                ] {
                    let actual = CMFormatDescriptionGetExtension(hint, key);
                    ensure!(!actual.is_null() && CFEqual(actual, expected) != 0,
                        "VideoToolbox did not preserve HDR color signaling. Try a different device or choose SDR.");
                }
            }
            let video: *mut Object = msg_send![class!(AVAssetWriterInput), assetWriterInputWithMediaType:media.0 outputSettings:null_mut::<Object>() sourceFormatHint:hint];
            ensure!(!video.is_null(), "Cannot create the compressed video track");
            let _: () = msg_send![video, retain];
            let video = Obj(video);
            let _: () = msg_send![video.0,setExpectsMediaDataInRealTime:NO];
            let can: objc::runtime::BOOL = msg_send![self.writer.0,canAddInput:video.0];
            ensure!(can == YES, "AVAssetWriter rejected the compressed video track");
            let _: () = msg_send![self.writer.0,addInput:video.0];
            let started: objc::runtime::BOOL = msg_send![self.writer.0, startWriting];
            ensure!(started == YES, "Cannot start AVAssetWriter: {}", self.error());
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
                "Cannot append {track} sample, PTS={}/{}: {}",
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
                "AVAssetWriter stopped during {goal:?}: {}",
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
                    "AVAssetWriter stalled during {goal:?}: video_ready={} audio_ready={}; {}",
                    Self::ready(self.video.as_ref().unwrap()),
                    Self::ready(&self.audio),
                    self.error()
                );
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
    pub fn finish(&mut self, pcm16: &[u8]) -> Result<()> {
        ensure!(
            self.try_finish(pcm16)?,
            "VideoToolbox interrupted; resume before finalizing the video"
        );
        Ok(())
    }

    /// Returns false after a recoverable interruption. The lifecycle gate must
    /// wait for foreground activation, call resume, then try finishing again.
    pub fn try_finish(&mut self, pcm16: &[u8]) -> Result<bool> {
        if self.complete {
            return Ok(true);
        }
        ensure!(self.video_frames > 0, "render contains no video frames");
        self.prepare_audio(pcm16)?;
        if self.needs_resume()? {
            return Ok(false);
        }
        // A non-numeric CMTime emits every outstanding frame before returning.
        // This is the only normal-path forced completion for the entire clip.
        let code = unsafe {
            VTCompressionSessionCompleteFrames(
                self.session,
                Time {
                    value: 0,
                    scale: 0,
                    flags: 0,
                    epoch: 0,
                },
            )
        };
        if code == INVALID_SESSION || self.needs_resume()? {
            self.invalidate_for_resume()?;
            self.check_callback_error()?;
            return Ok(false);
        }
        status(code, "VideoToolbox final completion")?;
        self.drain_callbacks(pcm16)?;
        ensure!(
            self.in_flight()? == 0,
            "VideoToolbox final completion left pending frame tokens"
        );
        ensure!(
            self.video.is_some(),
            "VideoToolbox produced no video frames"
        );
        ensure!(
            self.next_output_frame == self.video_frames,
            "VideoToolbox omitted frames: wrote {}, submitted {}",
            self.next_output_frame,
            self.video_frames
        );
        self.pump(pcm16, PumpGoal::Finish)?;
        unsafe {
            let ok: objc::runtime::BOOL = msg_send![self.writer.0, finishWriting];
            ensure!(ok == YES, "Cannot finalize video: {}", self.error());
        }
        self.complete = true;
        Ok(true)
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
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFCopyDescription(value: Ref) -> Ref;
        fn CMSampleBufferGetTypeID() -> usize;
        fn CMSampleBufferIsValid(sample: Ref) -> u8;
        fn CMSampleBufferGetNumSamples(sample: Ref) -> i64;
        fn CMSampleBufferGetPresentationTimeStamp(sample: Ref) -> Time;
        fn CMSampleBufferGetDecodeTimeStamp(sample: Ref) -> Time;
        fn CMSampleBufferGetDuration(sample: Ref) -> Time;
        fn CMSampleBufferGetImageBuffer(sample: Ref) -> Ref;
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
        empty_buffers: usize,
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
                assert_eq!(
                    CFGetTypeID(sample.0),
                    CMSampleBufferGetTypeID(),
                    "Reader returned a non-sample object"
                );
                assert!(
                    CMSampleBufferIsValid(sample.0) != 0,
                    "Reader returned an invalid sample buffer"
                );
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
                let detail = if count == 0 {
                    let text = Cf(CFCopyDescription(sample.0));
                    description(text.0 as *mut Object)
                        .chars()
                        .take(4000)
                        .collect::<String>()
                } else {
                    String::new()
                };
                Some((
                    count,
                    time,
                    decode,
                    duration,
                    empty_media,
                    ends_previous,
                    detail,
                ))
            });
            let Some((count, time, decode, duration, empty_media, ends_previous, detail)) = next
            else {
                break;
            };
            if count == 0 {
                // Empty buffers do not contribute media samples. Their metadata
                // can describe edit or control intervals without a marker flag.
                // Count only actual samples; the final exact video count still
                // rejects missing frames and a track consisting only of markers.
                result.empty_buffers += 1;
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
                if result.empty_buffers <= 8 {
                    eprintln!("Native MP4 {} empty buffer: empty_media={empty_media}, ends_previous={ends_previous}, PTS={time:.6}s, DTS={decode:.6}s, duration={duration:.6}s",
                        if audio { "audio" } else { "video" });
                    eprintln!("{detail}");
                }
                continue;
            }
            assert!(
                count > 0,
                "Invalid sample count {count} in {} track at PTS={time}, duration={duration}",
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
        eprintln!(
            "Native MP4 {} readback: {} media samples, {} empty buffers, end={:.6}s",
            if audio { "audio" } else { "video" },
            result.count,
            result.empty_buffers,
            result.end_time
        );
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

    unsafe fn read_video_colors(path: &Path, expected: u64) {
        let path = string(&path.to_string_lossy());
        let url: *mut Object = msg_send![class!(NSURL), fileURLWithPath:path.0];
        let asset: *mut Object =
            msg_send![class!(AVURLAsset), URLAssetWithURL:url options:null_mut::<Object>()];
        let media = string("vide");
        let tracks: *mut Object = msg_send![asset, tracksWithMediaType:media.0];
        let track: *mut Object = msg_send![tracks, objectAtIndex:0usize];
        let allocated: *mut Object = msg_send![class!(AVAssetReader), alloc];
        let mut error: *mut Object = null_mut();
        let reader: *mut Object = msg_send![allocated, initWithAsset:asset error:&mut error];
        assert!(
            !reader.is_null(),
            "Cannot create color decoder: {}",
            description(error)
        );
        let reader = Obj(reader);
        let format = number(BGRA as i64);
        let settings = reference_dictionary(&[(kCVPixelBufferPixelFormatTypeKey, format.0 as Ref)]);
        let allocated: *mut Object = msg_send![class!(AVAssetReaderTrackOutput), alloc];
        let output: *mut Object =
            msg_send![allocated, initWithTrack:track outputSettings:settings.0];
        assert!(!output.is_null());
        let output = Obj(output);
        let can_add: objc::runtime::BOOL = msg_send![reader.0, canAddOutput:output.0];
        assert_eq!(can_add, YES);
        let _: () = msg_send![reader.0, addOutput:output.0];
        let started: objc::runtime::BOOL = msg_send![reader.0, startReading];
        assert_eq!(started, YES);
        let mut frames = 0;
        loop {
            let color = objc::rc::autoreleasepool(|| {
                let sample: Ref = msg_send![output.0, copyNextSampleBuffer];
                if sample.is_null() {
                    return None;
                }
                let sample = Cf(sample);
                if CMSampleBufferGetNumSamples(sample.0) == 0 {
                    return Some(None);
                }
                let pixel = CMSampleBufferGetImageBuffer(sample.0);
                assert!(!pixel.is_null(), "H.264 decoder returned no image");
                assert_eq!(CVPixelBufferGetPixelFormatType(pixel), BGRA);
                status(
                    CVPixelBufferLockBaseAddress(pixel, 1),
                    "decoded pixel read lock",
                )
                .unwrap();
                let base = CVPixelBufferGetBaseAddress(pixel) as *const u8;
                let offset = CVPixelBufferGetHeight(pixel) / 2 * CVPixelBufferGetBytesPerRow(pixel)
                    + CVPixelBufferGetWidth(pixel) / 2 * 4;
                let center = std::slice::from_raw_parts(base.add(offset), 4);
                let color = [center[0], center[1], center[2]];
                status(
                    CVPixelBufferUnlockBaseAddress(pixel, 1),
                    "decoded pixel read unlock",
                )
                .unwrap();
                Some(Some(color))
            });
            let Some(color) = color else {
                break;
            };
            let Some([blue, green, red]) = color else {
                continue;
            };
            assert!(green < 40, "Unexpected green component on frame {frames}");
            if frames % 2 == 0 {
                assert!(
                    red > 215 && blue < 40,
                    "Wrong decoded red frame {frames}: R={red} B={blue}"
                );
            } else {
                assert!(
                    blue > 215 && red < 40,
                    "Wrong decoded blue frame {frames}: R={red} B={blue}"
                );
            }
            frames += 1;
        }
        let state: isize = msg_send![reader.0, status];
        let error: *mut Object = msg_send![reader.0, error];
        assert_eq!(
            state,
            2,
            "H.264 color decode failed: {}",
            description(error)
        );
        assert_eq!(
            frames, expected,
            "Decoded frame loss after session recovery"
        );
        eprintln!("Native MP4 session recovery decoded {frames} alternating red/blue images");
    }

    fn run_smoke(width: u32, height: u32, fps: u32, frames: u64, audio_frames: usize) {
        run_smoke_with_pause(width, height, fps, frames, audio_frames, None);
    }

    fn run_smoke_with_pause(
        width: u32,
        height: u32,
        fps: u32,
        frames: u64,
        audio_frames: usize,
        pause: Option<(u64, bool)>,
    ) {
        objc::rc::autoreleasepool(|| {
            const RATE: u32 = 48_000;
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("native.mp4");
            let mut encoder = Encoder::new(&path, width, height, fps, 4_000_000, RATE, None).unwrap();
            let red = [255u8, 0, 0, 255].repeat(width as usize * height as usize);
            let blue = [0u8, 0, 255, 255].repeat(width as usize * height as usize);
            // Nonzero stereo PCM lets the decoder test both sample count and
            // track presence rather than accidentally accepting an empty track.
            let audio = [1200i16.to_le_bytes(), (-1200i16).to_le_bytes()]
                .concat()
                .repeat(audio_frames);
            for frame in 0..frames {
                if pause == Some((frame, false)) {
                    encoder.suspend(&audio).unwrap();
                    assert!(encoder.needs_resume().unwrap());
                    encoder.resume(&audio).unwrap();
                    assert!(!encoder.needs_resume().unwrap());
                } else if pause == Some((frame, true)) {
                    // Exercise the real native InvalidSession return rather than
                    // simulating an app notification or a mock OSStatus.
                    unsafe {
                        VTCompressionSessionInvalidate(encoder.session);
                    }
                }
                let rgba = if frame % 2 == 0 { &red } else { &blue };
                objc::rc::autoreleasepool(|| encoder.frame(rgba, frame, &audio).unwrap());
                if pause == Some((frame, true)) {
                    assert!(encoder.needs_resume().unwrap());
                    encoder.resume(&audio).unwrap();
                    assert!(!encoder.needs_resume().unwrap());
                }
            }
            encoder.finish(&audio).unwrap();
            let state: isize = unsafe { msg_send![encoder.writer.0, status] };
            assert_eq!(state, 2, "MP4 writer did not complete");
            drop(encoder);

            // Retain these generated fixtures on CI so a readback failure can
            // be checked independently of AVAssetReader and the Rust FFI.
            if let Some(folder) = std::env::var_os("PHI_TKC_APPLE_SMOKE_ARTIFACTS") {
                let folder = std::path::PathBuf::from(folder);
                std::fs::create_dir_all(&folder).unwrap();
                let suffix = pause
                    .map(|(frame, invalid)| {
                        format!(
                            "-pause-{frame}-{}",
                            if invalid { "invalid" } else { "suspend" }
                        )
                    })
                    .unwrap_or_default();
                std::fs::copy(
                    &path,
                    folder.join(format!(
                        "{width}x{height}-{fps}fps-{frames}video-{audio_frames}audio{suffix}.mp4"
                    )),
                )
                .unwrap();
            }

            let (video, audio) = unsafe { read_media(&path) };
            if pause.is_some() {
                unsafe {
                    read_video_colors(&path, frames);
                }
            }
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

    #[test]
    fn videotoolbox_suspend_resume() {
        run_smoke_with_pause(64, 64, 30, 90, 144_000, Some((35, false)));
    }

    #[test]
    fn videotoolbox_invalid_session_replay() {
        run_smoke_with_pause(64, 64, 30, 90, 144_000, Some((35, true)));
    }
}
