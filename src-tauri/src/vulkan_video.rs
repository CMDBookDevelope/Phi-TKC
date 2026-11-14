use ash::vk;
use ash::vk::Handle;

use crate::vulkan_encode::{encode_fence_timeout_ns, VideoFns, VulkanVideoEncoder};

/// One NV12 image the renderer can fill while the encoder is busy with the
/// previous one.  The images are separate from the encoder session: a single
/// session encodes every frame in order, whichever source image it comes from.
struct SourceImage {
    image: vk::Image,
    view: vk::ImageView,
    /// Owns the image's memory; kept alive as long as the context.
    #[allow(dead_code)]
    memory: vk::DeviceMemory,
}

/// Frees a partially constructed source, unless its initialization submission
/// still owns the resources after a failed fence wait.
struct SourceAllocation<'a> {
    device: &'a ash::Device,
    command_pool: vk::CommandPool,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    command_buffer: vk::CommandBuffer,
    fence: vk::Fence,
    pending: bool,
}

impl Drop for SourceAllocation<'_> {
    fn drop(&mut self) {
        if self.pending {
            log::warn!("[vulkan-video] retaining source initialization resources: the GPU submission may still be running");
            return;
        }
        unsafe {
            if self.fence != vk::Fence::null() {
                self.device.destroy_fence(self.fence, None);
            }
            if self.command_buffer != vk::CommandBuffer::null() {
                self.device
                    .free_command_buffers(self.command_pool, &[self.command_buffer]);
            }
            if self.view != vk::ImageView::null() {
                self.device.destroy_image_view(self.view, None);
            }
            if self.image != vk::Image::null() {
                self.device.destroy_image(self.image, None);
            }
            if self.memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.memory, None);
            }
        }
    }
}

fn source_step<T>(name: &str, result: Result<T, vk::Result>) -> Result<T, bool> {
    result.map_err(|error| {
        log::warn!("[vulkan-video] source {name} failed: {error:?}");
        false
    })
}

unsafe fn destroy_source(device: &ash::Device, source: &SourceImage) {
    unsafe {
        device.destroy_image_view(source.view, None);
        device.destroy_image(source.image, None);
        device.free_memory(source.memory, None);
    }
}

pub struct VideoContext {
    pub device: ash::Device,
    pub instance: ash::Instance,
    pub fns: VideoFns,
    pub encoder: VulkanVideoEncoder,
    pub queue: vk::Queue,
    pub physical_device: vk::PhysicalDevice,
    pub command_pool: vk::CommandPool,
    sources: Vec<SourceImage>,
    pub width: u32,
    pub height: u32,
}

impl VideoContext {
    /// Create one H.264 encoder session plus `source_count` NV12 source images.
    ///
    /// Every frame must be encoded by the *same* session for P-frame
    /// prediction to make sense.  The images are what the renderer pipelines:
    /// it fills one while the encoder is still busy with another.  (Using one
    /// session per image would make each session see only every Nth frame,
    /// and its P frames would reference pictures the decoder has already
    /// dropped — so every frame had to be an IDR, which is what made renders
    /// enormous.)
    pub fn new(width: u32, height: u32, qp: u8, source_count: usize) -> Option<Self> {
        let context = macroquad::window::miniquad_context().vulkan_context()?;
        let queue_family = context.video_queue_family?;
        let graphics_family = context.queue_family_index;
        let families = if graphics_family == queue_family {
            vec![queue_family]
        } else {
            vec![graphics_family, queue_family]
        };
        let queue = vk::Queue::from_raw(unsafe { std::mem::transmute(context.video_queue) });
        let raw_instance: u64 = unsafe { std::mem::transmute(context.instance.handle()) };
        let raw_device: u64 = unsafe { std::mem::transmute(context.device.handle()) };
        let raw_physical: u64 = unsafe { std::mem::transmute(context.physical_device) };
        let entry = unsafe { ash::Entry::load().ok()? };
        let instance =
            unsafe { ash::Instance::load(entry.static_fn(), vk::Instance::from_raw(raw_instance)) };
        let device =
            unsafe { ash::Device::load(&instance.fp_v1_0(), vk::Device::from_raw(raw_device)) };
        let fns = unsafe { VideoFns::load(&entry, &instance, &device)? };
        log::debug!("[vulkan-video] creating H.264 session and DPB images");
        let mut encoder = unsafe {
            VulkanVideoEncoder::try_new_h264(
                &device,
                &instance,
                vk::PhysicalDevice::from_raw(raw_physical),
                &fns,
                queue_family,
                width,
                height,
                qp,
                false,
            )?
        };
        let physical_device = vk::PhysicalDevice::from_raw(raw_physical);
        let command_pool = match unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                    .queue_family_index(queue_family),
                None,
            )
        } {
            Ok(pool) => pool,
            Err(error) => {
                log::warn!("[vulkan-video] command pool creation failed: {error:?}");
                unsafe { encoder.destroy(&device, &fns) };
                return None;
            }
        };
        let mut sources = Vec::with_capacity(source_count.max(1));
        for slot in 0..source_count.max(1) {
            log::debug!("[vulkan-video] initializing NV12 source {slot}");
            let source = unsafe {
                create_source_image(
                    &device,
                    &instance,
                    physical_device,
                    queue,
                    command_pool,
                    &families,
                    width,
                    height,
                )
            };
            match source {
                Ok(source) => sources.push(source),
                Err(pending) => {
                    unsafe {
                        for source in &sources {
                            destroy_source(&device, source);
                        }
                        encoder.destroy(&device, &fns);
                        // A timed-out command buffer is still allocated from
                        // this pool. Destroying the pool would free it in flight.
                        if !pending {
                            device.destroy_command_pool(command_pool, None);
                        }
                    }
                    return None;
                }
            }
        }
        Some(Self {
            device,
            instance,
            fns,
            encoder,
            queue,
            physical_device,
            command_pool,
            sources,
            width,
            height,
        })
    }

    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    /// Raw `VkImage` handle for `slot`, for `copy_nv12_from_textures`.
    pub fn nv12_raw(&self, slot: usize) -> u64 {
        self.sources[slot].image.as_raw()
    }

    /// Encode the NV12 image in `slot`.  Must be called in output order on one
    /// thread; the session keeps the prediction state between calls.
    pub fn encode_slot(&mut self, slot: usize, force_keyframe: bool) -> Option<Vec<u8>> {
        let source = &self.sources[slot];
        unsafe {
            self.encoder
                .encode(
                    &self.device,
                    &self.fns,
                    self.queue,
                    self.command_pool,
                    source.image,
                    source.view,
                    force_keyframe,
                )
                .map(|(bitstream, _)| bitstream)
        }
    }

    /// Submit the NV12 image in `source_slot` without waiting for the encode
    /// fence.  The caller collects completed frames (and gets the source slot
    /// back) through [`Self::collect_encoded`].
    pub fn submit_encode_slot(
        &mut self,
        source_slot: usize,
        frame_index: u64,
        force_keyframe: bool,
    ) -> Option<()> {
        unsafe {
            self.encoder.submit_h264_async(
                &self.device,
                &self.fns,
                self.queue,
                self.command_pool,
                self.sources[source_slot].view,
                source_slot,
                frame_index,
                force_keyframe,
            )
        }
    }

    /// Wait for the oldest submitted frame.
    /// Returns `(frame index, bitstream, source image slot)`.
    pub fn collect_encoded(&mut self) -> Option<(u64, Vec<u8>, usize)> {
        unsafe { self.encoder.collect_h264_async(&self.device) }
    }

    pub fn encoded_in_flight(&self) -> usize {
        self.encoder.async_in_flight()
    }
}

/// Allocate one NV12 image, make it usable by both the graphics queue (the
/// renderer copies into it) and the video queue, and put it in `GENERAL`.
/// On failure, `true` means a submitted command still owns the source and pool.
unsafe fn create_source_image(
    device: &ash::Device,
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    queue: vk::Queue,
    command_pool: vk::CommandPool,
    families: &[u32],
    width: u32,
    height: u32,
) -> Result<SourceImage, bool> {
    let mut allocation = SourceAllocation {
        device,
        command_pool,
        image: vk::Image::null(),
        memory: vk::DeviceMemory::null(),
        view: vk::ImageView::null(),
        command_buffer: vk::CommandBuffer::null(),
        fence: vk::Fence::null(),
        pending: false,
    };
    // VIDEO_ENCODE_SRC images must declare the same supported profile as the
    // encoder session (VUID-VkImageCreateInfo-usage-04816).
    let mut h264_profile = vk::VideoEncodeH264ProfileInfoKHR::default()
        .std_profile_idc(vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_HIGH);
    let profile = vk::VideoProfileInfoKHR::default()
        .video_codec_operation(vk::VideoCodecOperationFlagsKHR::ENCODE_H264)
        .chroma_subsampling(vk::VideoChromaSubsamplingFlagsKHR::TYPE_420)
        .luma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
        .chroma_bit_depth(vk::VideoComponentBitDepthFlagsKHR::TYPE_8)
        .push_next(&mut h264_profile);
    let mut profile_list =
        vk::VideoProfileListInfoKHR::default().profiles(std::slice::from_ref(&profile));
    let mut image_create = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::VIDEO_ENCODE_SRC_KHR | vk::ImageUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .push_next(&mut profile_list);
    // CONCURRENT requires at least two distinct queue families (00942).
    if families.len() > 1 {
        image_create = image_create
            .sharing_mode(vk::SharingMode::CONCURRENT)
            .queue_family_indices(families);
    }
    let image = source_step("image creation", unsafe {
        device.create_image(&image_create, None)
    })?;
    allocation.image = image;
    let requirements = unsafe { device.get_image_memory_requirements(image) };
    let memory_properties =
        unsafe { instance.get_physical_device_memory_properties(physical_device) };
    let memory_type = (0..memory_properties.memory_type_count)
        .find(|index| {
            requirements.memory_type_bits & (1 << index) != 0
                && memory_properties.memory_types[*index as usize]
                    .property_flags
                    .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        })
        .ok_or_else(|| {
            log::warn!("[vulkan-video] no device-local memory type for the NV12 source");
            false
        })?;
    let memory = source_step("memory allocation", unsafe {
        device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(requirements.size)
                .memory_type_index(memory_type),
            None,
        )
    })?;
    allocation.memory = memory;
    source_step("memory binding", unsafe {
        device.bind_image_memory(image, memory, 0)
    })?;
    let view = source_step("image view creation", unsafe {
        device.create_image_view(
            &vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                }),
            None,
        )
    })?;
    allocation.view = view;

    let init = source_step("command buffer allocation", unsafe {
        device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default()
                .command_pool(command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1),
        )
    })?[0];
    allocation.command_buffer = init;
    unsafe {
        source_step(
            "begin command buffer",
            device.begin_command_buffer(
                init,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            ),
        )?;
        let barrier = vk::ImageMemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::empty())
            .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });
        device.cmd_pipeline_barrier(
            init,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
        source_step("end command buffer", device.end_command_buffer(init))?;
        allocation.fence = source_step(
            "fence creation",
            device.create_fence(&vk::FenceCreateInfo::default(), None),
        )?;
        let command_buffers = [init];
        let submit = vk::SubmitInfo::default().command_buffers(&command_buffers);
        source_step(
            "queue submission",
            device.queue_submit(queue, &[submit], allocation.fence),
        )?;
        allocation.pending = true;
        let timeout = encode_fence_timeout_ns();
        if let Err(error) = device.wait_for_fences(&[allocation.fence], true, timeout) {
            log::warn!(
                "[vulkan-video] NV12 source initialization fence failed after {} ms: {error:?}",
                timeout / 1_000_000
            );
            return Err(true);
        }
        allocation.pending = false;
    }
    allocation.image = vk::Image::null();
    allocation.memory = vk::DeviceMemory::null();
    allocation.view = vk::ImageView::null();
    Ok(SourceImage {
        image,
        memory,
        view,
    })
}
