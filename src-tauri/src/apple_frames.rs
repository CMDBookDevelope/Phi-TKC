//! GPU-owned video surfaces. No CPU readback is needed between Metal and VT.
use crate::apple_media::{Encoder, PixelBuffer};
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
struct PendingFrame {
    submission: MetalSubmission,
    alias: Cf,
    pixel: PixelBuffer,
    index: u64,
}

pub struct MetalFrames {
    pending: VecDeque<PendingFrame>,
    cache: Cf,
    attributes: Cf,
    width: usize,
    height: usize,
}
impl MetalFrames {
    pub fn new(metal: &MetalContext, width: u32, height: u32) -> Result<Self> {
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
                pending: VecDeque::new(),
                cache,
                attributes,
                width: width as usize,
                height: height as usize,
            })
        }
    }

    pub fn at_capacity(&self) -> bool {
        self.pending.len() >= MAX_GPU_FRAMES
    }

    /// The returned surface is retained until its submission completes. All
    /// copies and chart draws share the same Metal command queue and ordering.
    pub fn enqueue(
        &mut self,
        metal: &mut MetalContext,
        source: Texture,
        pixel: PixelBuffer,
        index: u64,
    ) -> Result<()> {
        ensure!(!self.at_capacity(), "Metal video queue is full");
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
            let submission = match metal.submit() {
                Ok(submission) => submission,
                Err(error) => {
                    // A submission error may still leave already encoded work.
                    metal.finish();
                    return Err(anyhow::Error::msg(error));
                }
            };
            self.pending.push_back(PendingFrame {
                submission,
                alias,
                pixel,
                index,
            });
        }
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

    pub fn drain_one(&mut self, encoder: &mut Encoder, pcm16: &[u8]) -> Result<()> {
        let Some(frame) = self.pending.pop_front() else {
            return Ok(());
        };
        let PendingFrame {
            submission,
            alias,
            pixel,
            index,
        } = frame;
        submission
            .wait()
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("Metal video frame {index}"))?;
        drop(submission);
        drop(alias);
        // Flush unused aliases before handing the lease to VT. The pool's ten
        // buffers bound GPU (3) + encoder (6) + the next allocation (1).
        unsafe { CVMetalTextureCacheFlush(self.cache.0, 0) };
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
        for frame in &self.pending {
            frame.submission.wait().map_err(anyhow::Error::msg)?;
        }
        Ok(())
    }

    pub fn defer_all(&mut self, encoder: &mut Encoder, pcm16: &[u8]) -> Result<()> {
        while let Some(frame) = self.pending.pop_front() {
            let PendingFrame {
                submission,
                alias,
                pixel,
                index,
            } = frame;
            submission.wait().map_err(anyhow::Error::msg)?;
            drop(submission);
            drop(alias);
            unsafe { CVMetalTextureCacheFlush(self.cache.0, 0) };
            encoder.defer_pixel(pixel, index, pcm16)?;
        }
        Ok(())
    }
}
impl Drop for MetalFrames {
    fn drop(&mut self) {
        // Cancellation/unwind also must not free an IOSurface being written.
        for frame in self.pending.drain(..) {
            if let Err(error) = frame.submission.wait() {
                log::error!("{error}");
            }
            drop(frame);
        }
        unsafe { CVMetalTextureCacheFlush(self.cache.0, 0) };
    }
}
