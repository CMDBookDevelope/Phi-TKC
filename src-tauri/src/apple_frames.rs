//! Asynchronous Metal frame export for VideoToolbox.
use crate::{
    apple_media::{Encoder, PixelBuffer, ProRes},
    apple_prores_gpu::ProResGpu,
    apple_render::mobile_hdr::Transfer,
};
use anyhow::{ensure, Context, Result};
use macroquad::miniquad::{
    graphics::metal::{MetalContext, MetalSubmission},
    PassAction, RenderPass, Texture,
};
use objc::{class, msg_send, runtime::Object, sel, sel_impl};
use std::{collections::VecDeque, ffi::c_void, ptr::null_mut};

type Ref = *mut c_void;
const MAX_GPU_FRAMES: usize = 3;
const BGRA8_UNORM: u64 = 80;

/// Initialize the exporter's fresh color/MSAA, depth and stencil attachments.
/// Chart scenes draw using load passes; their first frame must not blend with
/// undefined allocation contents. Keep this on the GPU and on its render queue.
pub fn initialize_render_target(metal: &mut MetalContext, target: RenderPass) {
    metal.begin_pass(
        Some(target),
        PassAction::Clear {
            color: Some((0., 0., 0., 1.)),
            depth: Some(1.),
            stencil: Some(0),
        },
    );
    metal.end_render_pass();
}

#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreVideo", kind = "framework")
)]
extern "C" {
    fn CVMetalTextureCacheCreate(
        allocator: Ref,
        attributes: Ref,
        device: Ref,
        texture_attributes: Ref,
        output: *mut Ref,
    ) -> i32;
    fn CVMetalTextureCacheCreateTextureFromImage(
        allocator: Ref,
        cache: Ref,
        image: Ref,
        attributes: Ref,
        format: u64,
        width: usize,
        height: usize,
        plane: usize,
        output: *mut Ref,
    ) -> i32;
    fn CVMetalTextureGetTexture(image: Ref) -> Ref;
    fn CVMetalTextureCacheFlush(cache: Ref, options: u64);
    static kCVMetalTextureUsage: Ref;
}
#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreFoundation", kind = "framework")
)]
extern "C" {
    fn CFRelease(value: Ref);
}

struct Cf(Ref);
impl Drop for Cf {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

struct BgraTarget {
    cache: Cf,
    attributes: Cf,
    width: usize,
    height: usize,
}

impl BgraTarget {
    fn new(metal: &MetalContext, width: u32, height: u32) -> Result<Self> {
        unsafe {
            let usage: *mut Object = msg_send![class!(NSNumber), numberWithUnsignedLong:5u64];
            let attributes: *mut Object = msg_send![class!(NSDictionary), dictionaryWithObject:usage forKey:kCVMetalTextureUsage];
            let attributes: *mut Object = msg_send![attributes, retain];
            let attributes = Cf(attributes as Ref);
            let mut cache = null_mut();
            let code = CVMetalTextureCacheCreate(
                null_mut(),
                null_mut(),
                metal.native_device(),
                attributes.0,
                &mut cache,
            );
            let cache = Cf(cache);
            ensure!(
                code == 0 && !cache.0.is_null(),
                "CoreVideo Metal texture cache creation failed: {code}"
            );
            Ok(Self {
                cache,
                attributes,
                width: width as usize,
                height: height as usize,
            })
        }
    }

    fn encode(&self, metal: &mut MetalContext, source: Texture, pixel: &PixelBuffer) -> Result<Cf> {
        unsafe {
            let mut image = null_mut();
            let code = CVMetalTextureCacheCreateTextureFromImage(
                null_mut(),
                self.cache.0,
                pixel.as_ptr(),
                self.attributes.0,
                BGRA8_UNORM,
                self.width,
                self.height,
                0,
                &mut image,
            );
            let alias = Cf(image);
            ensure!(
                code == 0 && !alias.0.is_null(),
                "CoreVideo Metal video texture mapping failed: {code}"
            );
            let texture = CVMetalTextureGetTexture(alias.0);
            ensure!(!texture.is_null(), "CoreVideo returned no Metal texture");
            // The renderer's target uses bottom-first rows; video is top-first.
            metal
                .copy_to_native_texture(source, texture, true)
                .map_err(anyhow::Error::msg)?;
            Ok(alias)
        }
    }

    fn flush(&self) {
        unsafe { CVMetalTextureCacheFlush(self.cache.0, 0) };
    }
}

enum Target {
    Bgra(BgraTarget),
    ProRes(ProResGpu<MAX_GPU_FRAMES>),
}

enum Storage {
    Bgra(Cf),
    ProRes(usize),
}

struct PendingFrame {
    submission: MetalSubmission,
    storage: Storage,
    pixel: PixelBuffer,
    index: u64,
}

pub struct MetalFrames {
    pending: VecDeque<PendingFrame>,
    target: Target,
    next_slot: usize,
}
impl MetalFrames {
    pub fn new(metal: &MetalContext, width: u32, height: u32) -> Result<Self> {
        Ok(Self::with_target(Target::Bgra(BgraTarget::new(
            metal, width, height,
        )?)))
    }

    pub(crate) fn new_with_prores(
        metal: &MetalContext,
        width: u32,
        height: u32,
        profile: ProRes,
        transfer: Option<Transfer>,
    ) -> Result<Self> {
        Ok(Self::with_target(Target::ProRes(ProResGpu::new(
            metal, width, height, profile, transfer,
        )?)))
    }

    fn with_target(target: Target) -> Self {
        Self {
            pending: VecDeque::with_capacity(MAX_GPU_FRAMES),
            target,
            next_slot: 0,
        }
    }

    pub fn at_capacity(&self) -> bool {
        self.pending.len() >= MAX_GPU_FRAMES
    }

    /// The returned surface is retained until its submission completes. All
    /// copies, conversions and chart draws share the same queue and ordering.
    pub fn enqueue(
        &mut self,
        metal: &mut MetalContext,
        source: Texture,
        pixel: PixelBuffer,
        index: u64,
    ) -> Result<()> {
        ensure!(!self.at_capacity(), "Metal video queue is full");
        let storage = match &self.target {
            Target::Bgra(target) => Storage::Bgra(target.encode(metal, source, &pixel)?),
            Target::ProRes(target) => {
                // FIFO draining frees slots in the same order they are leased.
                target.encode(metal, source, self.next_slot)?;
                Storage::ProRes(self.next_slot)
            }
        };
        let submission = match metal.submit() {
            Ok(submission) => submission,
            Err(error) => {
                // Already encoded work must finish even if submit reports an
                // earlier command's error. Keep the pixel and storage alive.
                let _ = metal.try_finish();
                return Err(anyhow::Error::msg(error));
            }
        };
        self.next_slot = (self.next_slot + 1) % MAX_GPU_FRAMES;
        self.pending.push_back(PendingFrame {
            submission,
            storage,
            pixel,
            index,
        });
        Ok(())
    }

    pub fn drain_ready(&mut self, encoder: &mut Encoder, pcm16: &[u8]) -> Result<()> {
        while self
            .pending
            .front()
            .is_some_and(|frame| frame.submission.is_complete())
        {
            self.drain_one(encoder, pcm16)?;
        }
        Ok(())
    }

    fn complete(&self, frame: PendingFrame, encoder: &Encoder) -> Result<(PixelBuffer, u64)> {
        let PendingFrame {
            submission,
            storage,
            pixel,
            index,
        } = frame;
        submission
            .wait()
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("Metal video frame {index}"))?;
        drop(submission);
        match (&self.target, storage) {
            (Target::Bgra(target), Storage::Bgra(alias)) => {
                drop(alias);
                // The pool's ten buffers bound GPU (3) + encoder (6) + next (1).
                target.flush();
            }
            (Target::ProRes(target), Storage::ProRes(slot)) => {
                target
                    .copy_to_pixel(slot, &pixel)
                    .with_context(|| format!("Metal ProRes frame {index}"))?;
                encoder.tag_prores_pixel(&pixel)?;
            }
            _ => unreachable!("Metal video frame storage must match its target"),
        }
        Ok((pixel, index))
    }

    pub fn drain_one(&mut self, encoder: &mut Encoder, pcm16: &[u8]) -> Result<()> {
        let Some(frame) = self.pending.pop_front() else {
            return Ok(());
        };
        let (pixel, index) = self.complete(frame, encoder)?;
        encoder
            .frame_pixel(pixel, index, pcm16)
            .with_context(|| format!("Apple media at video frame {index}"))
    }

    pub fn drain_all(&mut self, encoder: &mut Encoder, pcm16: &[u8]) -> Result<()> {
        while !self.pending.is_empty() {
            self.drain_one(encoder, pcm16)?;
        }
        Ok(())
    }

    /// Stop GPU access before UIKit completes its background transition. Do
    /// not feed the writer here: a blocked track must not delay quiescence.
    pub fn wait_all(&self) -> Result<()> {
        let mut result = Ok(());
        for frame in &self.pending {
            if let Err(error) = frame.submission.wait() {
                if result.is_ok() {
                    result = Err(anyhow::Error::msg(error))
                        .with_context(|| format!("Metal video frame {}", frame.index));
                }
            }
        }
        result
    }

    pub fn defer_all(&mut self, encoder: &mut Encoder, pcm16: &[u8]) -> Result<()> {
        while let Some(frame) = self.pending.pop_front() {
            let (pixel, index) = self.complete(frame, encoder)?;
            encoder.defer_pixel(pixel, index, pcm16)?;
        }
        Ok(())
    }
}
impl Drop for MetalFrames {
    fn drop(&mut self) {
        // Cancellation/unwind must not free resources a command is using.
        if let Err(error) = self.wait_all() {
            log::error!("{error:#}");
        }
        self.pending.clear();
        if let Target::Bgra(target) = &self.target {
            target.flush();
        }
    }
}
