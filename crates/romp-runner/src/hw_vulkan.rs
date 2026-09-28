use ash::vk;
use parking_lot::lock_api::RawMutex as _;
use parking_lot::{Mutex, RawMutex};
use romp_libretro::{HwContextProvider, RETRO_HW_CONTEXT_VULKAN};
use std::cell::{OnceCell, RefCell};
use std::ffi::{c_char, c_uint, c_void, CStr};
use std::path::{Path, PathBuf};

const RETRO_HW_RENDER_INTERFACE_VULKAN: c_uint = 0;
const RETRO_HW_RENDER_INTERFACE_VULKAN_VERSION: c_uint = 5;
const RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN: c_uint = 0;
const NEGOTIATION_VERSION: c_uint = 1;

#[repr(C)]
struct RetroVulkanImage {
    image_view: vk::ImageView,
    image_layout: vk::ImageLayout,
    create_info: vk::ImageViewCreateInfo<'static>,
}

#[repr(C)]
#[derive(Default)]
struct RetroVulkanContext {
    gpu: vk::PhysicalDevice,
    device: vk::Device,
    queue: vk::Queue,
    queue_family_index: u32,
    presentation_queue: vk::Queue,
    presentation_queue_family_index: u32,
}

type CreateDevice = unsafe extern "C" fn(
    context: *mut RetroVulkanContext,
    instance: vk::Instance,
    gpu: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    required_device_extensions: *const *const c_char,
    num_required_device_extensions: c_uint,
    required_device_layers: *const *const c_char,
    num_required_device_layers: c_uint,
    required_features: *const vk::PhysicalDeviceFeatures,
) -> bool;

#[repr(C)]
struct Negotiation {
    interface_type: c_uint,
    interface_version: c_uint,
    get_application_info: Option<unsafe extern "C" fn() -> *const vk::ApplicationInfo<'static>>,
    create_device: Option<CreateDevice>,
    destroy_device: Option<unsafe extern "C" fn()>,
}

#[repr(C)]
struct RenderInterface {
    interface_type: c_uint,
    interface_version: c_uint,
    handle: *mut c_void,
    instance: vk::Instance,
    gpu: vk::PhysicalDevice,
    device: vk::Device,
    get_device_proc_addr: vk::PFN_vkGetDeviceProcAddr,
    get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    queue: vk::Queue,
    queue_index: c_uint,
    set_image:
        unsafe extern "C" fn(*mut c_void, *const RetroVulkanImage, u32, *const vk::Semaphore, u32),
    get_sync_index: unsafe extern "C" fn(*mut c_void) -> u32,
    get_sync_index_mask: unsafe extern "C" fn(*mut c_void) -> u32,
    set_command_buffers: unsafe extern "C" fn(*mut c_void, u32, *const vk::CommandBuffer),
    wait_sync_index: unsafe extern "C" fn(*mut c_void),
    lock_queue: unsafe extern "C" fn(*mut c_void),
    unlock_queue: unsafe extern "C" fn(*mut c_void),
    set_signal_semaphore: unsafe extern "C" fn(*mut c_void, vk::Semaphore),
}

#[derive(Clone, Copy)]
struct Image {
    image: vk::Image,
    layout: vk::ImageLayout,
    format: vk::Format,
}

#[derive(Default)]
struct Pending {
    image: Option<Image>,
    wait: Vec<vk::Semaphore>,
    commands: Vec<vk::CommandBuffer>,
    signal: Option<vk::Semaphore>,
}

struct Readback {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    size: u64,
    mapped: *const u8,
}

struct DeviceState {
    device: ash::Device,
    gpu: vk::PhysicalDevice,
    queue: vk::Queue,
    queue_family: u32,
    commands: vk::CommandBuffer,
    fence: vk::Fence,
    readback: Option<Readback>,
}

pub struct HwVulkanContext {
    _entry: ash::Entry,
    instance: ash::Instance,
    negotiation: RefCell<*const Negotiation>,
    state: RefCell<Option<DeviceState>>,
    interface: OnceCell<Box<RenderInterface>>,
    pending: Mutex<Pending>,
    queue_lock: RawMutex,
}

// SAFETY: the core only reaches this from other threads through lock_queue and unlock_queue,
// which touch the raw mutex alone; everything else runs on the thread that drives the core.
unsafe impl Send for HwVulkanContext {}

pub fn library_path() -> PathBuf {
    if let Some(path) = std::env::var_os("ROMP_VULKAN_LIBRARY") {
        return path.into();
    }
    let name = if cfg!(target_os = "macos") {
        "libMoltenVK.dylib"
    } else if cfg!(windows) {
        "vulkan-1.dll"
    } else {
        "libvulkan.so.1"
    };
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(dir) = &exe_dir {
        candidates.push(dir.join("../Frameworks").join(name));
        candidates.push(dir.join(name));
    }
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from("/opt/homebrew/lib").join(name));
        candidates.push(PathBuf::from("/usr/local/lib").join(name));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| PathBuf::from(name))
}

impl HwVulkanContext {
    pub fn create(library: &Path) -> anyhow::Result<Self> {
        // SAFETY: loading the Vulkan library runs its initializers, as any Vulkan frontend does.
        let entry = unsafe { ash::Entry::load_from(library) }
            .map_err(|e| anyhow::anyhow!("load {}: {e}", library.display()))?;
        let version = unsafe { entry.try_enumerate_instance_version() }?
            .unwrap_or(vk::API_VERSION_1_0)
            .min(vk::API_VERSION_1_3);
        let available = unsafe { entry.enumerate_instance_extension_properties(None) }?;
        let has = |name: &CStr| {
            available
                .iter()
                .any(|e| e.extension_name_as_c_str() == Ok(name))
        };
        let mut extensions = Vec::new();
        let mut flags = vk::InstanceCreateFlags::empty();
        if has(ash::khr::get_physical_device_properties2::NAME) {
            extensions.push(ash::khr::get_physical_device_properties2::NAME.as_ptr());
        }
        if has(ash::khr::portability_enumeration::NAME) {
            extensions.push(ash::khr::portability_enumeration::NAME.as_ptr());
            flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
        }
        let app = vk::ApplicationInfo::default()
            .application_name(c"Romp")
            .engine_name(c"Romp")
            .api_version(version.max(vk::API_VERSION_1_1));
        let info = vk::InstanceCreateInfo::default()
            .application_info(&app)
            .enabled_extension_names(&extensions)
            .flags(flags);
        let instance = unsafe { entry.create_instance(&info, None) }?;
        Ok(Self {
            _entry: entry,
            instance,
            negotiation: RefCell::new(std::ptr::null()),
            state: RefCell::new(None),
            interface: OnceCell::new(),
            pending: Mutex::new(Pending::default()),
            queue_lock: RawMutex::INIT,
        })
    }

    fn create_device(&self) -> anyhow::Result<RetroVulkanContext> {
        let negotiation = *self.negotiation.borrow();
        // SAFETY: the core keeps its negotiation interface alive until it unloads.
        let create = unsafe { negotiation.as_ref() }.and_then(|n| n.create_device);
        if let Some(create) = create {
            let mut context = RetroVulkanContext::default();
            let features = vk::PhysicalDeviceFeatures::default();
            let created = unsafe {
                create(
                    &mut context,
                    self.instance.handle(),
                    vk::PhysicalDevice::null(),
                    vk::SurfaceKHR::null(),
                    self._entry.static_fn().get_instance_proc_addr,
                    std::ptr::null(),
                    0,
                    std::ptr::null(),
                    0,
                    &features,
                )
            };
            if !created || context.device == vk::Device::null() {
                anyhow::bail!("the core could not create its Vulkan device");
            }
            return Ok(context);
        }
        self.create_own_device()
    }

    fn create_own_device(&self) -> anyhow::Result<RetroVulkanContext> {
        let gpu = *unsafe { self.instance.enumerate_physical_devices() }?
            .first()
            .ok_or_else(|| anyhow::anyhow!("no Vulkan device"))?;
        let family = unsafe {
            self.instance
                .get_physical_device_queue_family_properties(gpu)
        }
        .iter()
        .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS))
        .ok_or_else(|| anyhow::anyhow!("no Vulkan graphics queue"))? as u32;
        let supported = unsafe { self.instance.enumerate_device_extension_properties(gpu) }?;
        let portability = c"VK_KHR_portability_subset";
        let extensions: Vec<*const c_char> = supported
            .iter()
            .any(|e| e.extension_name_as_c_str() == Ok(portability))
            .then_some(portability.as_ptr())
            .into_iter()
            .collect();
        let priorities = [1.0];
        let queues = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&priorities)];
        let info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queues)
            .enabled_extension_names(&extensions);
        let device = unsafe { self.instance.create_device(gpu, &info, None) }?;
        let queue = unsafe { device.get_device_queue(family, 0) };
        Ok(RetroVulkanContext {
            gpu,
            device: device.handle(),
            queue,
            queue_family_index: family,
            presentation_queue: queue,
            presentation_queue_family_index: family,
        })
    }

    fn state_for(&self, context: &RetroVulkanContext) -> anyhow::Result<DeviceState> {
        let device = unsafe { ash::Device::load(self.instance.fp_v1_0(), context.device) };
        let pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(context.queue_family_index)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )
        }?;
        let commands = unsafe {
            device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .command_buffer_count(1),
            )
        }?[0];
        let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }?;
        Ok(DeviceState {
            device,
            gpu: context.gpu,
            queue: context.queue,
            queue_family: context.queue_family_index,
            commands,
            fence,
            readback: None,
        })
    }

    fn readback_buffer(&self, state: &mut DeviceState, size: u64) -> anyhow::Result<*const u8> {
        if let Some(readback) = &state.readback {
            if readback.size >= size {
                return Ok(readback.mapped);
            }
        }
        if let Some(old) = state.readback.take() {
            unsafe {
                state.device.destroy_buffer(old.buffer, None);
                state.device.free_memory(old.memory, None);
            }
        }
        let device = &state.device;
        let buffer = unsafe {
            device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(vk::BufferUsageFlags::TRANSFER_DST),
                None,
            )
        }?;
        let needs = unsafe { device.get_buffer_memory_requirements(buffer) };
        let memory_types = unsafe {
            self.instance
                .get_physical_device_memory_properties(state.gpu)
        };
        let wanted = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        let type_index = (0..memory_types.memory_type_count)
            .find(|&i| {
                needs.memory_type_bits & (1 << i) != 0
                    && memory_types.memory_types[i as usize]
                        .property_flags
                        .contains(wanted)
            })
            .ok_or_else(|| anyhow::anyhow!("no host-visible Vulkan memory"))?;
        let memory = unsafe {
            device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(needs.size)
                    .memory_type_index(type_index),
                None,
            )
        }?;
        unsafe { device.bind_buffer_memory(buffer, memory, 0) }?;
        let mapped =
            unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }?.cast();
        state.readback = Some(Readback {
            buffer,
            memory,
            size,
            mapped,
        });
        Ok(mapped)
    }

    fn copy_out(&self, image: Image, width: u32, height: u32) -> anyhow::Result<Vec<u8>> {
        let mut pending = self.pending.lock();
        let mut guard = self.state.borrow_mut();
        let state = guard
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no Vulkan device"))?;
        let size = u64::from(width) * u64::from(height) * 4;
        let mapped = self.readback_buffer(state, size)?;
        let buffer = state.readback.as_ref().expect("readback buffer").buffer;
        let device = &state.device;
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        let to_copy = vk::ImageMemoryBarrier::default()
            .image(image.image)
            .subresource_range(range)
            .old_layout(image.layout)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED);
        let back = vk::ImageMemoryBarrier::default()
            .image(image.image)
            .subresource_range(range)
            .old_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .new_layout(image.layout)
            .src_access_mask(vk::AccessFlags::TRANSFER_READ)
            .dst_access_mask(vk::AccessFlags::MEMORY_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED);
        let to_host = vk::BufferMemoryBarrier::default()
            .buffer(buffer)
            .size(vk::WHOLE_SIZE)
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::HOST_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED);
        let region = vk::BufferImageCopy::default()
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            });
        unsafe {
            device.reset_command_buffer(state.commands, vk::CommandBufferResetFlags::empty())?;
            device.begin_command_buffer(
                state.commands,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            device.cmd_pipeline_barrier(
                state.commands,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[to_copy],
            );
            device.cmd_copy_image_to_buffer(
                state.commands,
                image.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                buffer,
                &[region],
            );
            device.cmd_pipeline_barrier(
                state.commands,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS | vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &[],
                &[to_host],
                &[back],
            );
            device.end_command_buffer(state.commands)?;
        }
        let mut commands = std::mem::take(&mut pending.commands);
        commands.push(state.commands);
        let wait = std::mem::take(&mut pending.wait);
        let stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; wait.len()];
        let signal: Vec<vk::Semaphore> = pending.signal.take().into_iter().collect();
        let submit = vk::SubmitInfo::default()
            .command_buffers(&commands)
            .wait_semaphores(&wait)
            .wait_dst_stage_mask(&stages)
            .signal_semaphores(&signal);
        self.queue_lock.lock();
        let submitted = unsafe { device.queue_submit(state.queue, &[submit], state.fence) };
        // SAFETY: locked just above on this thread.
        unsafe { self.queue_lock.unlock() };
        submitted?;
        unsafe {
            device.wait_for_fences(&[state.fence], true, u64::MAX)?;
            device.reset_fences(&[state.fence])?;
        }
        // SAFETY: the mapping holds at least `size` bytes and the fence says the copy finished.
        let mut pixels = unsafe { std::slice::from_raw_parts(mapped, size as usize) }.to_vec();
        if matches!(
            image.format,
            vk::Format::R8G8B8A8_UNORM | vk::Format::R8G8B8A8_SRGB
        ) {
            for px in pixels.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
        }
        Ok(pixels)
    }

    fn render_interface_for(&self, state: &DeviceState) -> RenderInterface {
        RenderInterface {
            interface_type: RETRO_HW_RENDER_INTERFACE_VULKAN,
            interface_version: RETRO_HW_RENDER_INTERFACE_VULKAN_VERSION,
            handle: self as *const Self as *mut c_void,
            instance: self.instance.handle(),
            gpu: state.gpu,
            device: state.device.handle(),
            get_device_proc_addr: self.instance.fp_v1_0().get_device_proc_addr,
            get_instance_proc_addr: self._entry.static_fn().get_instance_proc_addr,
            queue: state.queue,
            queue_index: state.queue_family,
            set_image,
            get_sync_index,
            get_sync_index_mask,
            set_command_buffers,
            wait_sync_index,
            lock_queue,
            unlock_queue,
            set_signal_semaphore,
        }
    }
}

/// # Safety
/// `handle` is the one handed out in the render interface, which outlives the core.
unsafe fn context<'a>(handle: *mut c_void) -> &'a HwVulkanContext {
    unsafe { &*(handle as *const HwVulkanContext) }
}

unsafe extern "C" fn set_image(
    handle: *mut c_void,
    image: *const RetroVulkanImage,
    num_semaphores: u32,
    semaphores: *const vk::Semaphore,
    _src_queue_family: u32,
) {
    let ctx = unsafe { context(handle) };
    let mut pending = ctx.pending.lock();
    pending.image = unsafe { image.as_ref() }.map(|i| Image {
        image: i.create_info.image,
        layout: i.image_layout,
        format: i.create_info.format,
    });
    pending.wait = if semaphores.is_null() {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(semaphores, num_semaphores as usize) }.to_vec()
    };
}

unsafe extern "C" fn get_sync_index(_handle: *mut c_void) -> u32 {
    0
}

unsafe extern "C" fn get_sync_index_mask(_handle: *mut c_void) -> u32 {
    1
}

unsafe extern "C" fn set_command_buffers(
    handle: *mut c_void,
    num: u32,
    commands: *const vk::CommandBuffer,
) {
    if commands.is_null() {
        return;
    }
    let ctx = unsafe { context(handle) };
    let commands = unsafe { std::slice::from_raw_parts(commands, num as usize) };
    ctx.pending.lock().commands = commands.to_vec();
}

unsafe extern "C" fn wait_sync_index(_handle: *mut c_void) {}

unsafe extern "C" fn lock_queue(handle: *mut c_void) {
    unsafe { context(handle) }.queue_lock.lock();
}

unsafe extern "C" fn unlock_queue(handle: *mut c_void) {
    // SAFETY: the core pairs every unlock with its own earlier lock_queue.
    unsafe { context(handle).queue_lock.unlock() };
}

unsafe extern "C" fn set_signal_semaphore(handle: *mut c_void, semaphore: vk::Semaphore) {
    unsafe { context(handle) }.pending.lock().signal = Some(semaphore);
}

impl HwContextProvider for HwVulkanContext {
    fn get_proc_address(&self, _sym: &str) -> *const c_void {
        std::ptr::null()
    }

    fn get_current_framebuffer(&self) -> usize {
        0
    }

    fn supports_context_type(&self, ctx_type: u32) -> bool {
        ctx_type == RETRO_HW_CONTEXT_VULKAN
    }

    fn make_current(&self) {}

    fn readback_bgra(&self, width: u32, height: u32) -> Vec<u8> {
        let image = self.pending.lock().image;
        let Some(image) = image else {
            return vec![0; width as usize * height as usize * 4];
        };
        self.copy_out(image, width, height).unwrap_or_else(|e| {
            tracing::warn!("Vulkan readback: {e}");
            vec![0; width as usize * height as usize * 4]
        })
    }

    fn preferred_context_type(&self) -> u32 {
        RETRO_HW_CONTEXT_VULKAN
    }

    fn negotiation_version(&self, interface_type: u32) -> Option<u32> {
        (interface_type == RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN)
            .then_some(NEGOTIATION_VERSION)
    }

    unsafe fn set_negotiation_interface(&self, iface: *const c_void) -> bool {
        let negotiation = iface as *const Negotiation;
        // SAFETY: the core hands over a negotiation interface it keeps alive until it unloads.
        let Some(n) = (unsafe { negotiation.as_ref() }) else {
            return false;
        };
        if n.interface_type != RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN {
            return false;
        }
        *self.negotiation.borrow_mut() = negotiation;
        true
    }

    fn prepare(&self) -> Result<(), String> {
        if self.interface.get().is_some() {
            return Ok(());
        }
        let state = self
            .create_device()
            .and_then(|context| self.state_for(&context))
            .map_err(|e| e.to_string())?;
        let interface = self.render_interface_for(&state);
        *self.state.borrow_mut() = Some(state);
        let _ = self.interface.set(Box::new(interface));
        Ok(())
    }

    fn render_interface(&self) -> *const c_void {
        self.interface.get().map_or(std::ptr::null(), |i| {
            &**i as *const RenderInterface as *const c_void
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn context() -> Option<(std::sync::MutexGuard<'static, ()>, Box<HwVulkanContext>)> {
        let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let ctx = HwVulkanContext::create(&library_path()).ok()?;
        Some((guard, Box::new(ctx)))
    }

    fn prepared() -> Option<(std::sync::MutexGuard<'static, ()>, Box<HwVulkanContext>)> {
        context().filter(|(_, ctx)| ctx.prepare().is_ok())
    }

    fn cleared_image(ctx: &HwVulkanContext, rgba: [f32; 4]) -> vk::Image {
        let guard = ctx.state.borrow();
        let state = guard.as_ref().unwrap();
        let device = &state.device;
        let image = unsafe {
            device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(vk::Format::R8G8B8A8_UNORM)
                    .extent(vk::Extent3D {
                        width: 4,
                        height: 2,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::TRANSFER_SRC),
                None,
            )
        }
        .unwrap();
        let needs = unsafe { device.get_image_memory_requirements(image) };
        let types = unsafe {
            ctx.instance
                .get_physical_device_memory_properties(state.gpu)
        };
        let index = (0..types.memory_type_count)
            .find(|&i| needs.memory_type_bits & (1 << i) != 0)
            .unwrap();
        let memory = unsafe {
            device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(needs.size)
                    .memory_type_index(index),
                None,
            )
        }
        .unwrap();
        unsafe { device.bind_image_memory(image, memory, 0) }.unwrap();
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);
        unsafe {
            device
                .begin_command_buffer(state.commands, &vk::CommandBufferBeginInfo::default())
                .unwrap();
            device.cmd_pipeline_barrier(
                state.commands,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(image)
                    .subresource_range(range)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)],
            );
            device.cmd_clear_color_image(
                state.commands,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &vk::ClearColorValue { float32: rgba },
                &[range],
            );
            device.cmd_pipeline_barrier(
                state.commands,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(image)
                    .subresource_range(range)
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)],
            );
            device.end_command_buffer(state.commands).unwrap();
            let commands = [state.commands];
            device
                .queue_submit(
                    state.queue,
                    &[vk::SubmitInfo::default().command_buffers(&commands)],
                    state.fence,
                )
                .unwrap();
            device
                .wait_for_fences(&[state.fence], true, u64::MAX)
                .unwrap();
            device.reset_fences(&[state.fence]).unwrap();
        }
        image
    }

    #[test]
    fn a_frame_set_by_the_core_reads_back_as_bgra() {
        let Some((_guard, ctx)) = prepared() else {
            eprintln!("no Vulkan device here, skipping");
            return;
        };
        let iface = unsafe { &*(ctx.render_interface() as *const RenderInterface) };
        assert_eq!(iface.interface_type, RETRO_HW_RENDER_INTERFACE_VULKAN);
        assert_eq!(unsafe { (iface.get_sync_index_mask)(iface.handle) }, 1);

        let image = cleared_image(&ctx, [1.0, 0.0, 0.0, 1.0]);
        let frame = RetroVulkanImage {
            image_view: vk::ImageView::null(),
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            create_info: vk::ImageViewCreateInfo::default()
                .image(image)
                .format(vk::Format::R8G8B8A8_UNORM),
        };
        unsafe { (iface.set_image)(iface.handle, &frame, 0, std::ptr::null(), 0) };
        let pixels = ctx.readback_bgra(4, 2);
        assert_eq!(pixels.len(), 4 * 2 * 4);
        assert!(
            pixels.chunks(4).all(|p| p == [0, 0, 255, 255]),
            "{pixels:?}"
        );
    }

    #[test]
    fn no_frame_reads_back_black() {
        let Some((_guard, ctx)) = prepared() else {
            return;
        };
        assert!(ctx.readback_bgra(2, 2).iter().all(|&b| b == 0));
    }

    #[test]
    fn only_vulkan_negotiation_is_offered() {
        let Some((_guard, ctx)) = context() else {
            return;
        };
        assert_eq!(
            ctx.negotiation_version(RETRO_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_VULKAN),
            Some(NEGOTIATION_VERSION)
        );
        assert_eq!(ctx.negotiation_version(7), None);
        assert!(ctx.supports_context_type(RETRO_HW_CONTEXT_VULKAN));
        assert!(!ctx.supports_context_type(romp_libretro::RETRO_HW_CONTEXT_OPENGL_CORE));
    }
}
