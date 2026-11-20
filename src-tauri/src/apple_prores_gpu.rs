//! Convert rendered RGB to CoreVideo's ProRes layouts on the render queue.
//!
//! Encoding boundary: alpha profiles take SDR RGBA8 with associated RGB from
//! source-over rendering; the y416 kernel unassociates RGB and defines zero-alpha
//! color as black. CoreVideo / VideoToolbox receives straight Y'CbCr plus coverage.
//! Non-alpha profiles keep RGB unchanged (SDR encoded RGB or linear BT.2020 HDR,
//! 1.0 = 100 nits) and force opaque output. HDR+alpha is rejected by apple_render.
#![allow(unexpected_cfgs)] // objc 0.2 macros reference their historical cargo-clippy feature.

use crate::{
    apple_media::{PixelBuffer, ProRes},
    apple_render::mobile_hdr::Transfer,
};
use anyhow::{ensure, Context, Result};
use macroquad::miniquad::{graphics::metal::MetalContext, Texture};
use objc::{class, msg_send, runtime::Object, sel, sel_impl};
use std::{
    ffi::{c_void, CStr},
    ptr::{copy_nonoverlapping, null_mut},
};

type Ref = *mut c_void;
const STORAGE_MODE_SHARED: usize = 0;
const RGBA8_UNORM: usize = 70;
const RGBA16_FLOAT: usize = 115;
const X422: u32 = u32::from_be_bytes(*b"x422");
const Y416: u32 = u32::from_be_bytes(*b"y416");

#[repr(C)]
struct Params {
    width: u32,
    height: u32,
    transfer: u32,
    alpha: u32,
}

// MTLSize is an ABI struct of three NSUInteger fields.
#[repr(C)]
struct MtlSize {
    width: usize,
    height: usize,
    depth: usize,
}

struct Obj(*mut Object);
impl Drop for Obj {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _: () = msg_send![self.0, release];
            }
        }
    }
}

pub(crate) struct ProResGpu<const SLOTS: usize> {
    pipeline: Obj,
    slots: Vec<Obj>,
    width: usize,
    height: usize,
    row_bytes: usize,
    is_4444: bool,
    params: Params,
}

impl<const SLOTS: usize> ProResGpu<SLOTS> {
    pub(crate) fn new(
        metal: &MetalContext,
        width: u32,
        height: u32,
        profile: ProRes,
        transfer: Option<Transfer>,
    ) -> Result<Self> {
        ensure!(width > 0 && height > 0, "invalid ProRes dimensions");
        let is_4444 = profile.is_4444();
        ensure!(
            is_4444 || width % 2 == 0,
            "x422 ProRes requires an even width"
        );
        // x422 stores one 16-bit Y per pixel and two 16-bit chroma samples
        // per pair. Both planes therefore have the same byte stride.
        let row_bytes = (width as usize)
            .checked_mul(if is_4444 { 8 } else { 2 })
            .context("ProRes row size overflow")?;
        let plane_bytes = row_bytes
            .checked_mul(height as usize)
            .context("ProRes plane size overflow")?;
        let output_size = plane_bytes
            .checked_mul(if is_4444 { 1 } else { 2 })
            .context("ProRes staging buffer size overflow")?;
        let (pipeline, slots) = objc::rc::autoreleasepool(|| unsafe {
            let device = metal.native_device() as *mut Object;
            let source = string(include_str!("apple_prores_gpu.metal"));
            let options = Obj(msg_send![class!(MTLCompileOptions), new]);
            let _: () = msg_send![options.0, setFastMathEnabled:objc::runtime::NO];
            let mut error: *mut Object = null_mut();
            let library = Obj(
                msg_send![device, newLibraryWithSource:source.0 options:options.0 error:&mut error],
            );
            ensure!(
                !library.0.is_null(),
                "Metal ProRes shader compilation failed: {}",
                describe_error(error)
            );
            let name = string(if is_4444 {
                "prores_y416"
            } else {
                "prores_x422"
            });
            let function = Obj(msg_send![library.0, newFunctionWithName:name.0]);
            ensure!(
                !function.0.is_null(),
                "Metal ProRes kernel function is missing"
            );
            error = null_mut();
            let pipeline = Obj(
                msg_send![device, newComputePipelineStateWithFunction:function.0 error:&mut error],
            );
            ensure!(
                !pipeline.0.is_null(),
                "Metal ProRes pipeline creation failed: {}",
                describe_error(error)
            );
            let mut slots = Vec::with_capacity(SLOTS);
            for _ in 0..SLOTS {
                let output = Obj(
                    msg_send![device, newBufferWithLength:output_size options:STORAGE_MODE_SHARED],
                );
                ensure!(
                    !output.0.is_null(),
                    "could not allocate a {output_size}-byte Metal ProRes staging buffer"
                );
                slots.push(output);
            }
            Ok((pipeline, slots))
        })?;
        Ok(Self {
            pipeline,
            slots,
            width: width as usize,
            height: height as usize,
            row_bytes,
            is_4444,
            params: Params {
                width,
                height,
                transfer: match transfer {
                    None => 0,
                    Some(Transfer::Pq) => 1,
                    Some(Transfer::Hlg) => 2,
                },
                alpha: u32::from(profile.alpha),
            },
        })
    }

    /// Enqueue conversion on the renderer's current command buffer. The caller
    /// must reserve the slot until the command completes and its output is copied.
    pub(crate) fn encode(
        &self,
        metal: &mut MetalContext,
        source: Texture,
        slot: usize,
    ) -> Result<()> {
        unsafe {
            let source = metal.native_texture(source).map_err(anyhow::Error::msg)? as *mut Object;
            let width: usize = msg_send![source, width];
            let height: usize = msg_send![source, height];
            let samples: usize = msg_send![source, sampleCount];
            let format: usize = msg_send![source, pixelFormat];
            let expected = if self.params.transfer == 0 {
                RGBA8_UNORM
            } else {
                RGBA16_FLOAT
            };
            ensure!(width == self.width && height == self.height && samples == 1 && format == expected,
                "invalid ProRes Metal source: {width}x{height}, {samples} samples, pixel format {format} (expected {}x{}, single-sample, format {expected})",
                self.width, self.height);
            // Miniquad creates command buffers with retained references, so the
            // render target remains valid after this borrow ends.
            let command = metal.native_command_buffer() as *mut Object;
            let encoder: *mut Object = msg_send![command, computeCommandEncoder];
            ensure!(!encoder.is_null(), "Metal compute encoder creation failed");
            let _: () = msg_send![encoder, setComputePipelineState:self.pipeline.0];
            let _: () = msg_send![encoder, setTexture:source atIndex:0usize];
            let _: () =
                msg_send![encoder, setBuffer:self.slots[slot].0 offset:0usize atIndex:0usize];
            let params = (&self.params as *const Params).cast::<c_void>();
            let _: () = msg_send![encoder, setBytes:params length:std::mem::size_of::<Params>() atIndex:1usize];
            let columns = if self.is_4444 { width } else { width / 2 };
            let threads = MtlSize {
                width: 16,
                height: 8,
                depth: 1,
            };
            let groups = MtlSize {
                width: columns.div_ceil(threads.width),
                height: height.div_ceil(threads.height),
                depth: 1,
            };
            let _: () =
                msg_send![encoder, dispatchThreadgroups:groups threadsPerThreadgroup:threads];
            let _: () = msg_send![encoder, endEncoding];
        }
        Ok(())
    }

    /// Copy completed shared-buffer rows into CoreVideo's potentially padded
    /// planes. Check dimensions and strides before copying, since CV decides the
    /// actual layout, and a pool may report different alignment across devices.
    pub(crate) fn copy_to_pixel(&self, slot: usize, pixel: &PixelBuffer) -> Result<()> {
        unsafe {
            let contents: *const u8 = msg_send![self.slots[slot].0, contents];
            ensure!(
                !contents.is_null(),
                "Metal shared staging buffer has no CPU mapping"
            );
            let buffer = pixel.as_ptr();
            let expected = if self.is_4444 { Y416 } else { X422 };
            ensure!(
                CVPixelBufferGetPixelFormatType(buffer) == expected
                    && CVPixelBufferGetWidth(buffer) == self.width
                    && CVPixelBufferGetHeight(buffer) == self.height,
                "wrong ProRes staging destination format or size"
            );
            let lock = CVPixelBufferLockBaseAddress(buffer, 0);
            ensure!(lock == 0, "ProRes pixel buffer lock failed: {lock}");
            let result = (|| {
                if self.is_4444 {
                    ensure!(
                        CVPixelBufferGetPlaneCount(buffer) == 0,
                        "invalid y416 plane count"
                    );
                    self.copy_rows(
                        contents,
                        CVPixelBufferGetBaseAddress(buffer).cast(),
                        CVPixelBufferGetBytesPerRow(buffer),
                    )?;
                } else {
                    ensure!(
                        CVPixelBufferGetPlaneCount(buffer) == 2,
                        "invalid x422 plane count"
                    );
                    for plane in 0..2 {
                        ensure!(
                            CVPixelBufferGetHeightOfPlane(buffer, plane) == self.height,
                            "invalid x422 plane height"
                        );
                        self.copy_rows(
                            contents.add(if plane == 0 {
                                0
                            } else {
                                self.row_bytes * self.height
                            }),
                            CVPixelBufferGetBaseAddressOfPlane(buffer, plane).cast(),
                            CVPixelBufferGetBytesPerRowOfPlane(buffer, plane),
                        )?;
                    }
                }
                Ok(())
            })();
            let unlock = CVPixelBufferUnlockBaseAddress(buffer, 0);
            result?;
            ensure!(unlock == 0, "ProRes pixel buffer unlock failed: {unlock}");
        }
        Ok(())
    }

    unsafe fn copy_rows(&self, source: *const u8, base: *mut u8, stride: usize) -> Result<()> {
        ensure!(
            !base.is_null() && stride >= self.row_bytes,
            "invalid ProRes plane layout"
        );
        if stride == self.row_bytes {
            copy_nonoverlapping(source, base, self.row_bytes * self.height);
        } else {
            for row in 0..self.height {
                copy_nonoverlapping(
                    source.add(row * self.row_bytes),
                    base.add(row * stride),
                    self.row_bytes,
                );
            }
        }
        Ok(())
    }
}

unsafe fn string(value: &str) -> Obj {
    let allocated: *mut Object = msg_send![class!(NSString), alloc];
    Obj(msg_send![allocated, initWithBytes:value.as_ptr() length:value.len() encoding:4usize])
}

unsafe fn describe_error(error: *mut Object) -> String {
    if error.is_null() {
        return "unknown Metal error".into();
    }
    let description: *mut Object = msg_send![error, description];
    let utf8: *const std::ffi::c_char = msg_send![description, UTF8String];
    if utf8.is_null() {
        "unknown Metal error".into()
    } else {
        CStr::from_ptr(utf8).to_string_lossy().into_owned()
    }
}

#[cfg_attr(
    any(target_os = "macos", target_os = "ios"),
    link(name = "CoreVideo", kind = "framework")
)]
extern "C" {
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
    fn CVPixelBufferGetHeightOfPlane(buffer: Ref, plane: usize) -> usize;
}
