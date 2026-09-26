#![allow(non_camel_case_types, non_snake_case, dead_code)]

use std::ffi::{c_char, c_int, c_uint, c_void};

pub const RETRO_API_VERSION: c_uint = 1;

pub const RETRO_DEVICE_NONE: c_uint = 0;
pub const RETRO_DEVICE_JOYPAD: c_uint = 1;
pub const RETRO_DEVICE_MOUSE: c_uint = 2;
pub const RETRO_DEVICE_KEYBOARD: c_uint = 3;
pub const RETRO_DEVICE_ANALOG: c_uint = 5;
pub const RETRO_DEVICE_POINTER: c_uint = 6;

pub const RETRO_DEVICE_ID_POINTER_X: c_uint = 0;
pub const RETRO_DEVICE_ID_POINTER_Y: c_uint = 1;
pub const RETRO_DEVICE_ID_POINTER_PRESSED: c_uint = 2;
pub const RETRO_DEVICE_ID_POINTER_COUNT: c_uint = 3;

pub const RETRO_DEVICE_ID_MOUSE_X: c_uint = 0;
pub const RETRO_DEVICE_ID_MOUSE_Y: c_uint = 1;
pub const RETRO_DEVICE_ID_MOUSE_LEFT: c_uint = 2;
pub const RETRO_DEVICE_ID_MOUSE_RIGHT: c_uint = 3;
pub const RETRO_DEVICE_ID_MOUSE_WHEELUP: c_uint = 4;
pub const RETRO_DEVICE_ID_MOUSE_WHEELDOWN: c_uint = 5;
pub const RETRO_DEVICE_ID_MOUSE_MIDDLE: c_uint = 6;
pub const RETRO_DEVICE_ID_MOUSE_HORIZ_WHEELUP: c_uint = 7;
pub const RETRO_DEVICE_ID_MOUSE_HORIZ_WHEELDOWN: c_uint = 8;
pub const RETRO_DEVICE_ID_MOUSE_BUTTON_4: c_uint = 9;
pub const RETRO_DEVICE_ID_MOUSE_BUTTON_5: c_uint = 10;

pub const RETRO_DEVICE_ID_JOYPAD_B: c_uint = 0;
pub const RETRO_DEVICE_ID_JOYPAD_Y: c_uint = 1;
pub const RETRO_DEVICE_ID_JOYPAD_SELECT: c_uint = 2;
pub const RETRO_DEVICE_ID_JOYPAD_START: c_uint = 3;
pub const RETRO_DEVICE_ID_JOYPAD_UP: c_uint = 4;
pub const RETRO_DEVICE_ID_JOYPAD_DOWN: c_uint = 5;
pub const RETRO_DEVICE_ID_JOYPAD_LEFT: c_uint = 6;
pub const RETRO_DEVICE_ID_JOYPAD_RIGHT: c_uint = 7;
pub const RETRO_DEVICE_ID_JOYPAD_A: c_uint = 8;
pub const RETRO_DEVICE_ID_JOYPAD_X: c_uint = 9;
pub const RETRO_DEVICE_ID_JOYPAD_L: c_uint = 10;
pub const RETRO_DEVICE_ID_JOYPAD_R: c_uint = 11;
pub const RETRO_DEVICE_ID_JOYPAD_L2: c_uint = 12;
pub const RETRO_DEVICE_ID_JOYPAD_R2: c_uint = 13;
pub const RETRO_DEVICE_ID_JOYPAD_L3: c_uint = 14;
pub const RETRO_DEVICE_ID_JOYPAD_R3: c_uint = 15;

pub const RETRO_DEVICE_INDEX_ANALOG_LEFT: c_uint = 0;
pub const RETRO_DEVICE_INDEX_ANALOG_RIGHT: c_uint = 1;
pub const RETRO_DEVICE_INDEX_ANALOG_BUTTON: c_uint = 2;

pub const RETRO_DEVICE_ID_ANALOG_X: c_uint = 0;
pub const RETRO_DEVICE_ID_ANALOG_Y: c_uint = 1;

pub const RETRO_PIXEL_FORMAT_0RGB1555: c_int = 0;
pub const RETRO_PIXEL_FORMAT_XRGB8888: c_int = 1;
pub const RETRO_PIXEL_FORMAT_RGB565: c_int = 2;

pub const RETRO_MEMORY_SAVE_RAM: c_uint = 0;
pub const RETRO_MEMORY_RTC: c_uint = 1;
pub const RETRO_MEMORY_SYSTEM_RAM: c_uint = 2;
pub const RETRO_MEMORY_VIDEO_RAM: c_uint = 3;

pub const RETRO_ENVIRONMENT_GET_OVERSCAN: c_uint = 2;
pub const RETRO_ENVIRONMENT_GET_CAN_DUPE: c_uint = 3;
pub const RETRO_ENVIRONMENT_SET_MESSAGE: c_uint = 6;
pub const RETRO_ENVIRONMENT_SHUTDOWN: c_uint = 7;
pub const RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY: c_uint = 9;
pub const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
pub const RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS: c_uint = 11;
pub const RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK: c_uint = 12;
pub const RETRO_ENVIRONMENT_GET_VARIABLE: c_uint = 15;
pub const RETRO_ENVIRONMENT_SET_VARIABLES: c_uint = 16;
pub const RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE: c_uint = 17;
pub const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: c_uint = 18;
pub const RETRO_ENVIRONMENT_GET_LIBRETRO_PATH: c_uint = 19;
pub const RETRO_ENVIRONMENT_SET_CONTROLLER_INFO: c_uint = 35;
pub const RETRO_ENVIRONMENT_SET_ROTATION: c_uint = 1;
pub const RETRO_ENVIRONMENT_GET_LANGUAGE: c_uint = 39;
pub const RETRO_ENVIRONMENT_GET_INPUT_BITMASKS: c_uint = 51 | 0x10000;
pub const RETRO_ENVIRONMENT_SET_HW_RENDER: c_uint = 14;
pub const RETRO_ENVIRONMENT_GET_LOG_INTERFACE: c_uint = 27;
pub const RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY: c_uint = 31;
pub const RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO: c_uint = 32;
pub const RETRO_ENVIRONMENT_SET_GEOMETRY: c_uint = 37;
pub const RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER: c_uint = 56;
pub const RETRO_ENVIRONMENT_GET_FASTFORWARDING: c_uint = 49 | 0x10000;
pub const RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION: c_uint = 52;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS: c_uint = 53;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2: c_uint = 67;
pub const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL: c_uint = 68;
pub const RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE: c_uint = 24;
pub const RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE: c_uint = 57;

pub const RETRO_HW_CONTEXT_NONE: u32 = 0;
pub const RETRO_HW_CONTEXT_OPENGL: u32 = 1;
pub const RETRO_HW_CONTEXT_OPENGLES2: u32 = 2;
pub const RETRO_HW_CONTEXT_OPENGL_CORE: u32 = 3;
pub const RETRO_HW_CONTEXT_OPENGLES3: u32 = 4;
pub const RETRO_HW_CONTEXT_OPENGLES_VERSION: u32 = 5;
pub const RETRO_HW_CONTEXT_VULKAN: u32 = 6;

pub const RETRO_HW_FRAME_BUFFER_VALID: *const c_void = usize::MAX as *const c_void;

pub type retro_hw_context_reset_t = Option<unsafe extern "C" fn()>;
pub type retro_hw_get_current_framebuffer_t = Option<unsafe extern "C" fn() -> usize>;
pub type retro_hw_get_proc_address_t =
    Option<unsafe extern "C" fn(sym: *const c_char) -> *const c_void>;

pub type retro_keyboard_event_t =
    Option<unsafe extern "C" fn(down: bool, keycode: c_uint, character: u32, key_modifiers: u16)>;

#[repr(C)]
pub struct retro_message {
    pub msg: *const c_char,
    pub frames: c_uint,
}

pub const RETRO_DEVICE_LIGHTGUN: c_uint = 4;
pub const RETRO_DEVICE_MASK: c_uint = 0xff;
pub const RETRO_DEVICE_ID_LIGHTGUN_X: c_uint = 0;
pub const RETRO_DEVICE_ID_LIGHTGUN_Y: c_uint = 1;
pub const RETRO_DEVICE_ID_LIGHTGUN_TRIGGER: c_uint = 2;
pub const RETRO_DEVICE_ID_LIGHTGUN_AUX_A: c_uint = 3;
pub const RETRO_DEVICE_ID_LIGHTGUN_AUX_B: c_uint = 4;
pub const RETRO_DEVICE_ID_LIGHTGUN_PAUSE: c_uint = 5;
pub const RETRO_DEVICE_ID_LIGHTGUN_START: c_uint = 6;
pub const RETRO_DEVICE_ID_LIGHTGUN_SELECT: c_uint = 7;
pub const RETRO_DEVICE_ID_LIGHTGUN_SCREEN_X: c_uint = 13;
pub const RETRO_DEVICE_ID_LIGHTGUN_SCREEN_Y: c_uint = 14;
pub const RETRO_DEVICE_ID_LIGHTGUN_IS_OFFSCREEN: c_uint = 15;
pub const RETRO_DEVICE_ID_LIGHTGUN_RELOAD: c_uint = 16;

#[repr(C)]
pub struct retro_controller_description {
    pub desc: *const c_char,
    pub id: c_uint,
}

#[repr(C)]
pub struct retro_controller_info {
    pub types: *const retro_controller_description,
    pub num_types: c_uint,
}

#[repr(C)]
pub struct retro_keyboard_callback {
    pub callback: retro_keyboard_event_t,
}

pub type retro_set_eject_state_t = unsafe extern "C" fn(ejected: bool) -> bool;
pub type retro_get_eject_state_t = unsafe extern "C" fn() -> bool;
pub type retro_get_image_index_t = unsafe extern "C" fn() -> c_uint;
pub type retro_set_image_index_t = unsafe extern "C" fn(index: c_uint) -> bool;
pub type retro_get_num_images_t = unsafe extern "C" fn() -> c_uint;
pub type retro_replace_image_index_t =
    unsafe extern "C" fn(index: c_uint, info: *const retro_game_info) -> bool;
pub type retro_add_image_index_t = unsafe extern "C" fn() -> bool;
pub type retro_set_initial_image_t =
    unsafe extern "C" fn(index: c_uint, path: *const c_char) -> bool;
pub type retro_get_image_path_t =
    unsafe extern "C" fn(index: c_uint, path: *mut c_char, len: usize) -> bool;
pub type retro_get_image_label_t =
    unsafe extern "C" fn(index: c_uint, label: *mut c_char, len: usize) -> bool;

#[repr(C)]
pub struct retro_disk_control_callback {
    pub set_eject_state: Option<retro_set_eject_state_t>,
    pub get_eject_state: Option<retro_get_eject_state_t>,
    pub get_image_index: Option<retro_get_image_index_t>,
    pub set_image_index: Option<retro_set_image_index_t>,
    pub get_num_images: Option<retro_get_num_images_t>,
    pub replace_image_index: Option<retro_replace_image_index_t>,
    pub add_image_index: Option<retro_add_image_index_t>,
}

#[repr(C)]
pub struct retro_disk_control_ext_callback {
    pub set_eject_state: Option<retro_set_eject_state_t>,
    pub get_eject_state: Option<retro_get_eject_state_t>,
    pub get_image_index: Option<retro_get_image_index_t>,
    pub set_image_index: Option<retro_set_image_index_t>,
    pub get_num_images: Option<retro_get_num_images_t>,
    pub replace_image_index: Option<retro_replace_image_index_t>,
    pub add_image_index: Option<retro_add_image_index_t>,
    pub set_initial_image: Option<retro_set_initial_image_t>,
    pub get_image_path: Option<retro_get_image_path_t>,
    pub get_image_label: Option<retro_get_image_label_t>,
}

#[repr(C)]
pub struct retro_hw_render_callback {
    pub context_type: u32,
    pub context_reset: retro_hw_context_reset_t,
    pub get_current_framebuffer: retro_hw_get_current_framebuffer_t,
    pub get_proc_address: retro_hw_get_proc_address_t,
    pub depth: bool,
    pub stencil: bool,
    pub bottom_left_origin: bool,
    pub version_major: c_uint,
    pub version_minor: c_uint,
    pub cache_context: bool,
    pub context_destroy: retro_hw_context_reset_t,
    pub debug_context: bool,
}

#[repr(C)]
pub struct retro_system_info {
    pub library_name: *const c_char,
    pub library_version: *const c_char,
    pub valid_extensions: *const c_char,
    pub need_fullpath: bool,
    pub block_extract: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct retro_game_geometry {
    pub base_width: c_uint,
    pub base_height: c_uint,
    pub max_width: c_uint,
    pub max_height: c_uint,
    pub aspect_ratio: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct retro_system_timing {
    pub fps: f64,
    pub sample_rate: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct retro_system_av_info {
    pub geometry: retro_game_geometry,
    pub timing: retro_system_timing,
}

#[repr(C)]
pub struct retro_game_info {
    pub path: *const c_char,
    pub data: *const c_void,
    pub size: usize,
    pub meta: *const c_char,
}

#[repr(C)]
pub struct retro_variable {
    pub key: *const c_char,
    pub value: *const c_char,
}

pub const RETRO_NUM_CORE_OPTION_VALUES_MAX: usize = 128;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct retro_core_option_value {
    pub value: *const c_char,
    pub label: *const c_char,
}

#[repr(C)]
pub struct retro_core_option_definition {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub info: *const c_char,
    pub values: [retro_core_option_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    pub default_value: *const c_char,
}

#[repr(C)]
pub struct retro_core_option_v2_definition {
    pub key: *const c_char,
    pub desc: *const c_char,
    pub desc_categorized: *const c_char,
    pub info: *const c_char,
    pub info_categorized: *const c_char,
    pub category_key: *const c_char,
    pub values: [retro_core_option_value; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    pub default_value: *const c_char,
}

#[repr(C)]
pub struct retro_core_options_v2 {
    pub categories: *mut c_void,
    pub definitions: *mut retro_core_option_v2_definition,
}

#[repr(C)]
pub struct retro_core_options_v2_intl {
    pub us: *mut retro_core_options_v2,
    pub local: *mut retro_core_options_v2,
}

#[repr(C)]
pub struct retro_log_callback {
    pub log: retro_log_printf_t,
}

pub type retro_environment_t = unsafe extern "C" fn(cmd: c_uint, data: *mut c_void) -> bool;
pub type retro_video_refresh_t =
    unsafe extern "C" fn(data: *const c_void, width: c_uint, height: c_uint, pitch: usize);
pub type retro_audio_sample_t = unsafe extern "C" fn(left: i16, right: i16);
pub type retro_audio_sample_batch_t =
    unsafe extern "C" fn(data: *const i16, frames: usize) -> usize;
pub type retro_input_poll_t = unsafe extern "C" fn();
pub type retro_input_state_t =
    unsafe extern "C" fn(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16;
pub type retro_log_printf_t = unsafe extern "C" fn(level: c_uint, fmt: *const c_char, ...);
