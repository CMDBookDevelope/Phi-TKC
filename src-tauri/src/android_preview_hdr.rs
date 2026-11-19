use jni::{objects::JObject, JNIEnv};
use std::{ffi::c_void, ptr, slice};

const RGBA_1010102: u32 = 0x2b;
const CPU_WRITE_OFTEN: u64 = 3 << 4;
const LUT_SIZE: usize = 4096;

#[repr(C)]
#[derive(Default)]
struct BufferDescription {
    width: u32,
    height: u32,
    layers: u32,
    format: u32,
    usage: u64,
    stride: u32,
    reserved: u32,
    reserved2: [u32; 2],
}

#[link(name = "android")]
extern "C" {
    fn AHardwareBuffer_fromHardwareBuffer(
        env: *mut jni::sys::JNIEnv,
        buffer: jni::sys::jobject,
    ) -> *mut c_void;
    fn AHardwareBuffer_describe(buffer: *const c_void, desc: *mut BufferDescription);
    fn AHardwareBuffer_lock(
        buffer: *mut c_void,
        usage: u64,
        fence: i32,
        rect: *const c_void,
        address: *mut *mut c_void,
    ) -> i32;
    fn AHardwareBuffer_unlock(buffer: *mut c_void, fence: *mut i32) -> i32;
}

pub(crate) struct Converter {
    linear: [f32; 256],
    transfer: Box<[u16]>,
    pq: bool,
}

impl Converter {
    pub(crate) fn new(color_space: &str) -> Option<Self> {
        let pq = match color_space {
            "rec2100_pq" | "rec2100-pq" => true,
            "rec2100_hlg" | "rec2100-hlg" => false,
            _ => return None,
        };
        let linear = std::array::from_fn(|index| {
            let value = index as f32 / 255.;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        });
        let transfer = (0..LUT_SIZE)
            .map(|index| {
                let value = index as f32 / (LUT_SIZE - 1) as f32;
                // The SDR scene has a 100-nit reference white. PQ is absolute.
                // The HLG conversion uses a 1000-nit reference display.
                let signal = if pq {
                    let power = value.powf(2610. / 16384.);
                    ((3424. / 4096. + 2413. / 128. * power) / (1. + 2392. / 128. * power))
                        .powf(2523. / 32.)
                } else {
                    let scene = value;
                    if scene <= 1. / 12. {
                        (3. * scene).sqrt()
                    } else {
                        0.17883277 * (12. * scene - 0.28466892).ln() + 0.55991073
                    }
                };
                (signal * 1023.).round() as u16
            })
            .collect();
        Some(Self {
            linear,
            transfer,
            pq,
        })
    }

    fn sdr_rgb(&self, rgba: &[u8]) -> [f32; 3] {
        let [r, g, b] = [rgba[0], rgba[1], rgba[2]].map(|channel| self.linear[channel as usize]);
        [
            0.627404 * r + 0.329283 * g + 0.043313 * b,
            0.069097 * r + 0.919540 * g + 0.011362 * b,
            0.016391 * r + 0.088013 * g + 0.895595 * b,
        ]
    }

    fn pixel(&self, rgb: [f32; 3]) -> u32 {
        let rgb = if self.pq {
            rgb.map(|value| value / 100.)
        } else {
            let luma = (0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]).max(1e-8);
            let gain = 0.1f32.powf(1. / 1.2) * luma.powf(1. / 1.2 - 1.);
            rgb.map(|value| value * gain)
        };
        let [r, g, b] = rgb.map(|linear| {
            let index = (linear.clamp(0., 1.) * (LUT_SIZE - 1) as f32).round() as usize;
            u32::from(self.transfer[index])
        });
        r | (g << 10) | (b << 20) | (3 << 30)
    }
}

pub(crate) unsafe fn write_buffer(
    env: JNIEnv<'_>,
    buffer: JObject<'_>,
    source: &[u8],
    converter: &Converter,
    hdr: bool,
) -> Result<(), ()> {
    let buffer = AHardwareBuffer_fromHardwareBuffer(env.get_native_interface(), buffer.as_raw());
    if buffer.is_null() {
        return Err(());
    }
    let mut desc = BufferDescription::default();
    AHardwareBuffer_describe(buffer, &mut desc);
    let (width, height, stride) = (
        desc.width as usize,
        desc.height as usize,
        desc.stride as usize,
    );
    let bytes_per_pixel = if hdr { 8 } else { 4 };
    if desc.layers != 1
        || desc.format != RGBA_1010102
        || desc.usage & CPU_WRITE_OFTEN == 0
        || width == 0
        || height == 0
        || stride < width
        || width
            .checked_mul(height)
            .and_then(|count| count.checked_mul(bytes_per_pixel))
            != Some(source.len())
        || stride
            .checked_mul(height)
            .and_then(|count| count.checked_mul(4))
            .is_none()
    {
        return Err(());
    }
    let mut address = ptr::null_mut();
    if AHardwareBuffer_lock(buffer, CPU_WRITE_OFTEN, -1, ptr::null(), &mut address) != 0 {
        return Err(());
    }
    let written = if address.is_null() {
        false
    } else {
        let output = slice::from_raw_parts_mut(address.cast::<u32>(), stride * height);
        for (row, pixels) in source
            .chunks_exact(width * bytes_per_pixel)
            .zip(output.chunks_exact_mut(stride))
        {
            for (pixel, destination) in row.chunks_exact(bytes_per_pixel).zip(pixels) {
                let rgb = if hdr {
                    crate::native_preview::read_linear_bt2020(pixel)
                } else {
                    converter.sdr_rgb(pixel)
                };
                *destination = converter.pixel(rgb);
            }
        }
        true
    };
    let unlocked = AHardwareBuffer_unlock(buffer, ptr::null_mut()) == 0;
    if written && unlocked {
        Ok(())
    } else {
        Err(())
    }
}
