//! Asynchronous Vulkan staging of straight-alpha RGBA for ProRes.
use anyhow::{Context, Result};
use ash::{vk, vk::Handle};
use macroquad::{prelude::*, window::miniquad_context};

pub(crate) struct Packer {
    pub target: RenderTarget,
    material: Material,
    width: u32,
    height: u32,
}

impl Packer {
    pub(crate) fn new(width: u32, height: u32) -> Result<Self> {
        anyhow::ensure!(width > 0 && height > 0, "empty ProRes staging target");
        let ctx = miniquad_context();
        anyhow::ensure!(
            ctx.vulkan_context()
                .is_some_and(|v| width <= v.max_texture_size && height <= v.max_texture_size),
            "ProRes packing target exceeds device limits"
        );
        let material = load_material(VERTEX, FRAGMENT, MaterialParams::default())
            .map_err(|e| anyhow::anyhow!("ProRes alpha shader: {e:?}"))?;
        let target =
            super::render::new_plane(width, height, macroquad::miniquad::TextureFormat::RGBA8);
        Ok(Self {
            target,
            material,
            width,
            height,
        })
    }
    pub(crate) fn frame_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
    pub(crate) fn convert(&self, source: &RenderTarget) {
        push_camera_state();
        set_camera(&Camera2D {
            render_target: Some(self.target),
            ..Default::default()
        });
        gl_use_material(self.material);
        draw_texture_ex(
            source.texture,
            -1.,
            -1.,
            WHITE,
            DrawTextureParams {
                dest_size: Some(vec2(2., 2.)),
                ..Default::default()
            },
        );
        unsafe { get_internal_gl() }.flush();
        gl_use_default_material();
        pop_camera_state();
    }
    pub(crate) fn record(&self, staging: u64) {
        let width = self.target.texture.width() as u32;
        let height = self.target.texture.height() as u32;
        miniquad_context()
            .vulkan_context()
            .unwrap()
            .record_rgba_copy_to_staging(
                self.target
                    .texture
                    .raw_miniquad_texture_handle()
                    .gl_internal_id() as usize,
                staging,
                width,
                height,
            );
    }
}
impl Drop for Packer {
    fn drop(&mut self) {
        self.target.delete();
        self.material.delete();
    }
}

struct Slot {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    mapped: usize,
}
pub(crate) struct Readback {
    device: ash::Device,
    slots: Vec<Slot>,
    bytes: usize,
}
impl Readback {
    pub(crate) fn new(bytes: usize, count: usize) -> Result<Self> {
        let ctx = miniquad_context()
            .vulkan_context()
            .context("Vulkan readback unavailable")?;
        let entry = unsafe { ash::Entry::load()? };
        let instance = unsafe {
            ash::Instance::load(
                entry.static_fn(),
                vk::Instance::from_raw(unsafe { std::mem::transmute(ctx.instance.handle()) }),
            )
        };
        let device = unsafe {
            ash::Device::load(
                &instance.fp_v1_0(),
                vk::Device::from_raw(unsafe { std::mem::transmute(ctx.device.handle()) }),
            )
        };
        let memory = unsafe {
            instance.get_physical_device_memory_properties(vk::PhysicalDevice::from_raw(unsafe {
                std::mem::transmute(ctx.physical_device)
            }))
        };
        let mut result = Self {
            device,
            slots: Vec::new(),
            bytes,
        };
        for _ in 0..count {
            let buffer = unsafe {
                result.device.create_buffer(
                    &vk::BufferCreateInfo::default()
                        .size(bytes as u64)
                        .usage(vk::BufferUsageFlags::TRANSFER_DST)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE),
                    None,
                )?
            };
            let req = unsafe { result.device.get_buffer_memory_requirements(buffer) };
            let Some(index) = (0..memory.memory_type_count).find(|&i| {
                req.memory_type_bits & (1 << i) != 0
                    && memory.memory_types[i as usize].property_flags.contains(
                        vk::MemoryPropertyFlags::HOST_VISIBLE
                            | vk::MemoryPropertyFlags::HOST_COHERENT,
                    )
            }) else {
                unsafe { result.device.destroy_buffer(buffer, None) };
                anyhow::bail!("host-coherent readback memory unavailable");
            };
            let allocation = match unsafe {
                result.device.allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(index),
                    None,
                )
            } {
                Ok(m) => m,
                Err(e) => {
                    unsafe { result.device.destroy_buffer(buffer, None) };
                    return Err(e.into());
                }
            };
            result.slots.push(Slot {
                buffer,
                memory: allocation,
                mapped: 0,
            });
            unsafe {
                result.device.bind_buffer_memory(buffer, allocation, 0)?;
            }
            result.slots.last_mut().unwrap().mapped = unsafe {
                result
                    .device
                    .map_memory(allocation, 0, req.size, vk::MemoryMapFlags::empty())?
            } as usize;
        }
        Ok(result)
    }
    pub(crate) fn addresses(&self) -> Vec<usize> {
        self.slots.iter().map(|s| s.mapped).collect()
    }
    pub(crate) fn buffer(&self, slot: usize) -> u64 {
        self.slots[slot].buffer.as_raw()
    }
    pub(crate) fn frame_bytes(&self) -> usize {
        self.bytes
    }
}
impl Drop for Readback {
    fn drop(&mut self) {
        // Callers wait the render fences and join the writer before dropping storage.
        for slot in self.slots.drain(..) {
            unsafe {
                if slot.mapped != 0 {
                    self.device.unmap_memory(slot.memory);
                }
                self.device.destroy_buffer(slot.buffer, None);
                self.device.free_memory(slot.memory, None);
            }
        }
    }
}

/// Diagnostic exercised by the release binary on the actual Vulkan device.
pub(crate) fn smoke() -> Result<()> {
    let target = super::render::new_plane(8, 4, macroquad::miniquad::TextureFormat::RGBA8);
    let packer = Packer::new(8, 4)?;
    let staging = Readback::new(packer.frame_bytes(), 1)?;
    miniquad_context()
        .vulkan_context()
        .unwrap()
        .begin_offscreen_frame();
    let mut camera = Camera2D::from_display_rect(Rect::new(0., 0., 8., 4.));
    camera.render_target = Some(target);
    set_camera(&camera);
    clear_background(BLANK);
    draw_rectangle(0., 0., 2., 2., Color::new(1., 0., 0., 0.5));
    draw_rectangle(2., 0., 2., 2., Color::new(0., 1., 0., 1.));
    draw_rectangle(0., 2., 2., 2., Color::new(0., 0., 1., 0.25));
    draw_rectangle(2., 2., 2., 2., Color::new(1., 1., 1., 1.));
    unsafe { get_internal_gl() }.flush();
    packer.convert(&target);
    packer.record(staging.buffer(0));
    miniquad_context()
        .vulkan_context()
        .unwrap()
        .finish_offscreen_frame();
    miniquad_context()
        .vulkan_context()
        .unwrap()
        .wait_offscreen_idle();
    let bytes = unsafe {
        std::slice::from_raw_parts(staging.addresses()[0] as *const u8, staging.frame_bytes())
    };
    let mut rgba = vec![0; 8 * 4 * 4];
    target
        .texture
        .raw_miniquad_texture_handle()
        .read_pixels(miniquad_context(), &mut rgba);
    for (index, (actual, source)) in bytes.chunks_exact(4).zip(rgba.chunks_exact(4)).enumerate() {
        anyhow::ensure!(actual[3] == source[3], "ProRes alpha changed at {index}");
        for channel in 0..3 {
            let a = u32::from(source[3]);
            let expected = if a == 0 {
                0
            } else {
                ((u32::from(source[channel]) * 255 + a / 2) / a).min(255) as u8
            };
            anyhow::ensure!(
                actual[channel].abs_diff(expected) <= 1,
                "ProRes RGB or row orientation changed at {index}"
            );
        }
    }
    anyhow::ensure!(
        bytes[..4] == [255, 0, 0, 127] && bytes[16 * 4..17 * 4] == [0, 0, 255, 63],
        "asymmetric alpha fixture changed"
    );
    target.delete();
    println!("ProRes asynchronous RGBA orientation, straight RGB and alpha PASS");
    Ok(())
}

const VERTEX: &str = r#"#version 450 core
layout(location=0) in vec3 position;
void main() { gl_Position = vec4(position.xy, 0.0, 1.0); }
"#;
const FRAGMENT: &str = r#"#version 450 core
layout(set=0,binding=1) uniform sampler2D Texture;
layout(location=0) out vec4 fragColor;
void main() {
    ivec2 pos = ivec2(gl_FragCoord.xy);
    pos.y = textureSize(Texture, 0).y - 1 - pos.y;
    vec4 p = texelFetch(Texture, pos, 0);
    fragColor = vec4(p.a > 0.0 ? clamp(p.rgb / p.a, 0.0, 1.0) : vec3(0.0), p.a);
}
"#;
