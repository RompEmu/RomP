//! A core creates its own Vulkan device, through the function lookup RomP hands it. Standing in
//! that lookup lets RomP put right what the core asks the driver for.

use ash::vk;
use parking_lot::Mutex;
use std::ffi::{c_char, c_void, CStr};

const DEPTH_CLAMP_CONTROL: &CStr = c"VK_EXT_depth_clamp_control";
const DEPTH_CLAMP_CONTROL_FEATURES: vk::StructureType = vk::StructureType::from_raw(1_000_582_000);

/// `VkPhysicalDeviceDepthClampControlFeaturesEXT`, which is newer than ash's Vulkan headers.
#[repr(C)]
struct DepthClampControlFeatures {
    s_type: vk::StructureType,
    p_next: *mut c_void,
    depth_clamp_control: vk::Bool32,
}

struct Real {
    lookup: vk::PFN_vkGetInstanceProcAddr,
    instance: ash::Instance,
}

static REAL: Mutex<Option<Real>> = Mutex::new(None);

/// Makes `get_instance_proc_addr` answer for this instance.
pub fn stand_in(entry: &ash::Entry, instance: &ash::Instance) {
    *REAL.lock() = Some(Real {
        lookup: entry.static_fn().get_instance_proc_addr,
        instance: instance.clone(),
    });
}

/// The lookup cores get: the loader's own, except that devices are created through RomP.
///
/// # Safety
/// As `vkGetInstanceProcAddr`: `name` is null or a C string.
pub unsafe extern "system" fn get_instance_proc_addr(
    instance: vk::Instance,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    let lookup = REAL.lock().as_ref().map(|real| real.lookup)?;
    let found = unsafe { lookup(instance, name) };
    let creates_device = !name.is_null() && unsafe { CStr::from_ptr(name) } == c"vkCreateDevice";
    if found.is_some() && creates_device {
        // SAFETY: Vulkan hands every function out as this one type, and the caller casts it back.
        return Some(unsafe {
            std::mem::transmute::<vk::PFN_vkCreateDevice, unsafe extern "system" fn()>(
                create_device,
            )
        });
    }
    found
}

/// # Safety
/// `info` is a valid `VkDeviceCreateInfo`.
unsafe fn enabled_extensions<'a>(info: &vk::DeviceCreateInfo<'a>) -> Vec<&'a CStr> {
    if info.pp_enabled_extension_names.is_null() {
        return Vec::new();
    }
    let names = unsafe {
        std::slice::from_raw_parts(
            info.pp_enabled_extension_names,
            info.enabled_extension_count as usize,
        )
    };
    names
        .iter()
        .filter(|name| !name.is_null())
        .map(|&name| unsafe { CStr::from_ptr(name) })
        .collect()
}

/// # Safety
/// `next` heads a valid chain of Vulkan structures, or is null.
unsafe fn chained(mut next: *const c_void) -> Vec<vk::StructureType> {
    let mut types = Vec::new();
    while let Some(structure) = unsafe { next.cast::<vk::BaseInStructure>().as_ref() } {
        types.push(structure.s_type);
        next = structure.p_next.cast();
    }
    types
}

/// Dolphin uses depth clamp control in every pipeline, but turns on only the extension. Vulkan
/// wants the feature turned on as well, and a driver may lose the device over the difference.
fn lacks_depth_clamp_feature(enabled: &[&CStr], chained: &[vk::StructureType]) -> bool {
    enabled.contains(&DEPTH_CLAMP_CONTROL) && !chained.contains(&DEPTH_CLAMP_CONTROL_FEATURES)
}

fn depth_clamp_features(next: *mut c_void, on: bool) -> DepthClampControlFeatures {
    DepthClampControlFeatures {
        s_type: DEPTH_CLAMP_CONTROL_FEATURES,
        p_next: next,
        depth_clamp_control: on.into(),
    }
}

fn offers_depth_clamp_feature(instance: &ash::Instance, gpu: vk::PhysicalDevice) -> bool {
    let mut clamp = depth_clamp_features(std::ptr::null_mut(), false);
    let mut all = vk::PhysicalDeviceFeatures2 {
        p_next: (&raw mut clamp).cast(),
        ..Default::default()
    };
    unsafe { instance.get_physical_device_features2(gpu, &mut all) };
    clamp.depth_clamp_control == vk::TRUE
}

unsafe extern "system" fn create_device(
    gpu: vk::PhysicalDevice,
    create_info: *const vk::DeviceCreateInfo<'_>,
    allocator: *const vk::AllocationCallbacks<'_>,
    device: *mut vk::Device,
) -> vk::Result {
    let Some(instance) = REAL.lock().as_ref().map(|real| real.instance.clone()) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };
    let real = instance.fp_v1_0().create_device;
    let as_asked = || unsafe { real(gpu, create_info, allocator, device) };
    let Some(info) = (unsafe { create_info.as_ref() }) else {
        return as_asked();
    };
    let lacking = lacks_depth_clamp_feature(&unsafe { enabled_extensions(info) }, &unsafe {
        chained(info.p_next)
    });
    if !lacking || !offers_depth_clamp_feature(&instance, gpu) {
        return as_asked();
    }
    let mut clamp = depth_clamp_features(info.p_next.cast_mut(), true);
    let mut corrected = *info;
    corrected.p_next = (&raw mut clamp).cast();
    let result = unsafe { real(gpu, &corrected, allocator, device) };
    if result == vk::Result::SUCCESS {
        tracing::info!(
            "Vulkan device created with the depth clamp control feature the core left out"
        );
        return result;
    }
    tracing::warn!("no Vulkan device with the depth clamp control feature ({result}); creating it as the core asked");
    as_asked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feature_is_lacking_when_only_the_extension_is_on() {
        assert!(lacks_depth_clamp_feature(
            &[c"VK_KHR_swapchain", DEPTH_CLAMP_CONTROL],
            &[]
        ));
    }

    #[test]
    fn nothing_is_lacking_without_the_extension() {
        assert!(!lacks_depth_clamp_feature(&[c"VK_KHR_swapchain"], &[]));
    }

    #[test]
    fn a_core_that_turns_the_feature_on_is_left_alone() {
        assert!(!lacks_depth_clamp_feature(
            &[DEPTH_CLAMP_CONTROL],
            &[DEPTH_CLAMP_CONTROL_FEATURES]
        ));
    }

    #[test]
    fn extensions_and_chained_structures_are_read_from_the_request() {
        let mut clamp = depth_clamp_features(std::ptr::null_mut(), true);
        let all = vk::PhysicalDeviceFeatures2 {
            p_next: (&raw mut clamp).cast(),
            ..Default::default()
        };
        let names = [c"VK_KHR_swapchain".as_ptr(), DEPTH_CLAMP_CONTROL.as_ptr()];
        let mut info = vk::DeviceCreateInfo::default().enabled_extension_names(&names);
        info.p_next = (&raw const all).cast();
        assert_eq!(
            unsafe { enabled_extensions(&info) },
            [c"VK_KHR_swapchain", DEPTH_CLAMP_CONTROL]
        );
        assert_eq!(
            unsafe { chained(info.p_next) },
            [
                vk::StructureType::PHYSICAL_DEVICE_FEATURES_2,
                DEPTH_CLAMP_CONTROL_FEATURES
            ]
        );
    }

    #[test]
    fn a_request_with_no_extensions_reads_as_empty() {
        let info = vk::DeviceCreateInfo::default();
        assert!(unsafe { enabled_extensions(&info) }.is_empty());
        assert!(unsafe { chained(info.p_next) }.is_empty());
    }
}
