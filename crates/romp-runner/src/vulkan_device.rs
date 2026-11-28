//! A core creates its own Vulkan device, through the function lookup RomP hands it. Standing in
//! that lookup lets RomP put right what the core asks the driver for.

use ash::vk::{self, Handle as _};
use parking_lot::Mutex;
use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicU64, Ordering};

const DEVICE_FAULT: &CStr = ash::ext::device_fault::NAME;
/// The same reports under their newer name, which ash's Vulkan headers don't have yet.
const KHR_DEVICE_FAULT: &CStr = c"VK_KHR_device_fault";
const KHR_DEVICE_FAULT_FEATURES: vk::StructureType = vk::StructureType::from_raw(1_000_573_000);
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

/// The device created so that the driver keeps a report of what loses it, or null.
static REPORTS_FAULTS: AtomicU64 = AtomicU64::new(0);

/// Makes `get_instance_proc_addr` answer for this instance.
pub fn stand_in(entry: &ash::Entry, instance: &ash::Instance) {
    *REAL.lock() = Some(Real {
        lookup: entry.static_fn().get_instance_proc_addr,
        instance: instance.clone(),
    });
    REPORTS_FAULTS.store(0, Ordering::Relaxed);
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

/// A core that asks for fault reports itself is left to read them itself.
fn can_add_fault_reports(enabled: &[&CStr], chained: &[vk::StructureType]) -> bool {
    !enabled.contains(&DEVICE_FAULT)
        && !enabled.contains(&KHR_DEVICE_FAULT)
        && !chained.contains(&vk::StructureType::PHYSICAL_DEVICE_FAULT_FEATURES_EXT)
        && !chained.contains(&KHR_DEVICE_FAULT_FEATURES)
}

fn offers_fault_reports(instance: &ash::Instance, gpu: vk::PhysicalDevice) -> bool {
    let listed = unsafe { instance.enumerate_device_extension_properties(gpu) }
        .unwrap_or_default()
        .iter()
        .any(|e| e.extension_name_as_c_str() == Ok(DEVICE_FAULT));
    if !listed {
        return false;
    }
    let mut fault = vk::PhysicalDeviceFaultFeaturesEXT::default();
    let mut all = vk::PhysicalDeviceFeatures2::default().push_next(&mut fault);
    unsafe { instance.get_physical_device_features2(gpu, &mut all) };
    fault.device_fault == vk::TRUE
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
    let extensions = unsafe { enabled_extensions(info) };
    let chain = unsafe { chained(info.p_next) };
    let depth_clamp = lacks_depth_clamp_feature(&extensions, &chain)
        && offers_depth_clamp_feature(&instance, gpu);
    let fault_reports =
        can_add_fault_reports(&extensions, &chain) && offers_fault_reports(&instance, gpu);
    if !depth_clamp && !fault_reports {
        return as_asked();
    }
    let corrected = unsafe { Corrected::new(info, depth_clamp, fault_reports) };
    let result = unsafe { real(gpu, &corrected.info, allocator, device) };
    if result == vk::Result::SUCCESS {
        if fault_reports {
            REPORTS_FAULTS.store(unsafe { *device }.as_raw(), Ordering::Relaxed);
        }
        tracing::info!(
            depth_clamp_feature = depth_clamp,
            fault_reports,
            "Vulkan device created with more than the core asked for"
        );
        return result;
    }
    tracing::warn!(
        depth_clamp_feature = depth_clamp,
        fault_reports,
        "no Vulkan device with what RomP adds ({result}); creating it as the core asked"
    );
    as_asked()
}

/// A core's request with RomP's additions, which it points at and so have to outlive it.
struct Corrected<'a> {
    info: vk::DeviceCreateInfo<'a>,
    _names: Vec<*const c_char>,
    _clamp: Box<DepthClampControlFeatures>,
    _fault: Box<vk::PhysicalDeviceFaultFeaturesEXT<'static>>,
}

impl<'a> Corrected<'a> {
    /// The additions go in front of the core's own chain, which stays as it was.
    ///
    /// # Safety
    /// `asked` is a valid `VkDeviceCreateInfo`.
    unsafe fn new(
        asked: &vk::DeviceCreateInfo<'a>,
        depth_clamp: bool,
        fault_reports: bool,
    ) -> Self {
        let mut info = *asked;
        let mut extensions = unsafe { enabled_extensions(asked) };
        let mut clamp = Box::new(depth_clamp_features(info.p_next.cast_mut(), true));
        if depth_clamp {
            info.p_next = (&raw mut *clamp).cast();
        }
        let mut fault = Box::new(vk::PhysicalDeviceFaultFeaturesEXT::default().device_fault(true));
        fault.p_next = info.p_next.cast_mut();
        if fault_reports {
            info.p_next = (&raw mut *fault).cast();
            extensions.push(DEVICE_FAULT);
        }
        let names: Vec<*const c_char> = extensions.iter().map(|name| name.as_ptr()).collect();
        info.enabled_extension_count = names.len() as u32;
        info.pp_enabled_extension_names = names.as_ptr();
        Self {
            info,
            _names: names,
            _clamp: clamp,
            _fault: fault,
        }
    }
}

fn describe_fault(
    description: &str,
    addresses: &[vk::DeviceFaultAddressInfoEXT],
    vendors: &[vk::DeviceFaultVendorInfoEXT],
) -> String {
    let mut report = if description.is_empty() {
        "no description".to_owned()
    } else {
        description.to_owned()
    };
    for a in addresses {
        report += &format!(
            "; {:?} at {:#x} (precision {:#x})",
            a.address_type, a.reported_address, a.address_precision
        );
    }
    for v in vendors {
        let what = v.description_as_c_str().unwrap_or_default();
        report += &format!(
            "; driver: {} (code {:#x}, data {:#x})",
            what.to_string_lossy(),
            v.vendor_fault_code,
            v.vendor_fault_data
        );
    }
    report
}

/// What the driver says lost the device, when the device was created to keep such a report.
/// Only for a device that is lost.
pub fn fault_report(instance: &ash::Instance, device: &ash::Device) -> Option<String> {
    if REPORTS_FAULTS.load(Ordering::Relaxed) != device.handle().as_raw() {
        return None;
    }
    let get = ash::ext::device_fault::Device::new(instance, device)
        .fp()
        .get_device_fault_info_ext;
    let mut counts = vk::DeviceFaultCountsEXT::default();
    let counted = unsafe { get(device.handle(), &mut counts, std::ptr::null_mut()) };
    if counted != vk::Result::SUCCESS {
        return Some(format!("nothing ({counted})"));
    }
    // The driver's own dump can be large and only its maker can read it.
    counts.vendor_binary_size = 0;
    let mut addresses =
        vec![vk::DeviceFaultAddressInfoEXT::default(); counts.address_info_count as usize];
    let mut vendors =
        vec![vk::DeviceFaultVendorInfoEXT::default(); counts.vendor_info_count as usize];
    let mut info = vk::DeviceFaultInfoEXT {
        p_address_infos: addresses.as_mut_ptr(),
        p_vendor_infos: vendors.as_mut_ptr(),
        ..Default::default()
    };
    let read = unsafe { get(device.handle(), &mut counts, &mut info) };
    if read != vk::Result::SUCCESS && read != vk::Result::INCOMPLETE {
        return Some(format!("nothing ({read})"));
    }
    addresses.truncate(counts.address_info_count as usize);
    vendors.truncate(counts.vendor_info_count as usize);
    let description = info.description_as_c_str().unwrap_or_default();
    Some(describe_fault(
        &description.to_string_lossy(),
        &addresses,
        &vendors,
    ))
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

    #[test]
    fn fault_reports_are_added_unless_the_core_asks_for_them() {
        assert!(can_add_fault_reports(&[c"VK_KHR_swapchain"], &[]));
        assert!(!can_add_fault_reports(&[DEVICE_FAULT], &[]));
        assert!(!can_add_fault_reports(&[KHR_DEVICE_FAULT], &[]));
        assert!(!can_add_fault_reports(&[], &[KHR_DEVICE_FAULT_FEATURES]));
        assert!(!can_add_fault_reports(
            &[],
            &[vk::StructureType::PHYSICAL_DEVICE_FAULT_FEATURES_EXT]
        ));
    }

    #[test]
    fn a_fault_reads_as_one_line() {
        let address = vk::DeviceFaultAddressInfoEXT::default()
            .address_type(vk::DeviceFaultAddressTypeEXT::WRITE_INVALID)
            .reported_address(0x1000)
            .address_precision(0x1000);
        let vendor = vk::DeviceFaultVendorInfoEXT::default()
            .description(c"page fault")
            .unwrap()
            .vendor_fault_code(13)
            .vendor_fault_data(2);
        assert_eq!(
            describe_fault("engine reset", &[address], &[vendor]),
            "engine reset; WRITE_INVALID at 0x1000 (precision 0x1000); \
             driver: page fault (code 0xd, data 0x2)"
        );
        assert_eq!(describe_fault("", &[], &[]), "no description");
    }

    /// A request as Dolphin makes it: the extension on, and a chain of its own.
    fn asked<'a>(
        names: &'a [*const c_char],
        own: &'a vk::PhysicalDeviceFeatures2<'a>,
    ) -> vk::DeviceCreateInfo<'a> {
        let mut info = vk::DeviceCreateInfo::default().enabled_extension_names(names);
        info.p_next = (&raw const *own).cast();
        info
    }

    #[test]
    fn additions_go_in_front_of_the_cores_own_chain() {
        let own = vk::PhysicalDeviceFeatures2::default();
        let names = [DEPTH_CLAMP_CONTROL.as_ptr()];
        let info = asked(&names, &own);
        let corrected = unsafe { Corrected::new(&info, true, true) };
        assert_eq!(
            unsafe { chained(corrected.info.p_next) },
            [
                vk::StructureType::PHYSICAL_DEVICE_FAULT_FEATURES_EXT,
                DEPTH_CLAMP_CONTROL_FEATURES,
                vk::StructureType::PHYSICAL_DEVICE_FEATURES_2
            ]
        );
        assert_eq!(
            unsafe { enabled_extensions(&corrected.info) },
            [DEPTH_CLAMP_CONTROL, DEVICE_FAULT]
        );
        assert_eq!(corrected._clamp.depth_clamp_control, vk::TRUE);
        assert_eq!(corrected._fault.device_fault, vk::TRUE);
        assert_eq!(corrected._clamp.p_next.cast_const(), info.p_next);
    }

    #[test]
    fn only_what_is_wanted_is_added() {
        let own = vk::PhysicalDeviceFeatures2::default();
        let names = [DEPTH_CLAMP_CONTROL.as_ptr()];
        let info = asked(&names, &own);
        let clamp_only = unsafe { Corrected::new(&info, true, false) };
        assert_eq!(
            unsafe { chained(clamp_only.info.p_next) },
            [
                DEPTH_CLAMP_CONTROL_FEATURES,
                vk::StructureType::PHYSICAL_DEVICE_FEATURES_2
            ]
        );
        assert_eq!(
            unsafe { enabled_extensions(&clamp_only.info) },
            [DEPTH_CLAMP_CONTROL]
        );
        let fault_only = unsafe { Corrected::new(&info, false, true) };
        assert_eq!(
            unsafe { chained(fault_only.info.p_next) },
            [
                vk::StructureType::PHYSICAL_DEVICE_FAULT_FEATURES_EXT,
                vk::StructureType::PHYSICAL_DEVICE_FEATURES_2
            ]
        );
        assert_eq!(
            unsafe { enabled_extensions(&fault_only.info) },
            [DEPTH_CLAMP_CONTROL, DEVICE_FAULT]
        );
    }
}
