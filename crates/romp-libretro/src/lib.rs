use libloading::Library;
use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_uint, c_void, CStr, CString};
use std::path::Path;
use std::ptr;
use thiserror::Error;

mod sys;

// Unlike eprintln!, never panics when stderr is closed; a panic here would abort inside a core callback.
macro_rules! log {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

pub use sys::{
    RETRO_DEVICE_ANALOG, RETRO_DEVICE_ID_ANALOG_X, RETRO_DEVICE_ID_ANALOG_Y,
    RETRO_DEVICE_ID_JOYPAD_A, RETRO_DEVICE_ID_JOYPAD_B, RETRO_DEVICE_ID_JOYPAD_DOWN,
    RETRO_DEVICE_ID_JOYPAD_L, RETRO_DEVICE_ID_JOYPAD_L2, RETRO_DEVICE_ID_JOYPAD_L3,
    RETRO_DEVICE_ID_JOYPAD_LEFT, RETRO_DEVICE_ID_JOYPAD_R, RETRO_DEVICE_ID_JOYPAD_R2,
    RETRO_DEVICE_ID_JOYPAD_R3, RETRO_DEVICE_ID_JOYPAD_RIGHT, RETRO_DEVICE_ID_JOYPAD_SELECT,
    RETRO_DEVICE_ID_JOYPAD_START, RETRO_DEVICE_ID_JOYPAD_UP, RETRO_DEVICE_ID_JOYPAD_X,
    RETRO_DEVICE_ID_JOYPAD_Y, RETRO_DEVICE_ID_LIGHTGUN_AUX_A, RETRO_DEVICE_ID_LIGHTGUN_AUX_B,
    RETRO_DEVICE_ID_LIGHTGUN_IS_OFFSCREEN, RETRO_DEVICE_ID_LIGHTGUN_PAUSE,
    RETRO_DEVICE_ID_LIGHTGUN_RELOAD, RETRO_DEVICE_ID_LIGHTGUN_SCREEN_X,
    RETRO_DEVICE_ID_LIGHTGUN_SCREEN_Y, RETRO_DEVICE_ID_LIGHTGUN_SELECT,
    RETRO_DEVICE_ID_LIGHTGUN_START, RETRO_DEVICE_ID_LIGHTGUN_TRIGGER, RETRO_DEVICE_ID_LIGHTGUN_X,
    RETRO_DEVICE_ID_LIGHTGUN_Y, RETRO_DEVICE_ID_MOUSE_BUTTON_4, RETRO_DEVICE_ID_MOUSE_BUTTON_5,
    RETRO_DEVICE_ID_MOUSE_LEFT, RETRO_DEVICE_ID_MOUSE_MIDDLE, RETRO_DEVICE_ID_MOUSE_RIGHT,
    RETRO_DEVICE_ID_MOUSE_WHEELDOWN, RETRO_DEVICE_ID_MOUSE_WHEELUP, RETRO_DEVICE_ID_MOUSE_X,
    RETRO_DEVICE_ID_MOUSE_Y, RETRO_DEVICE_ID_POINTER_COUNT, RETRO_DEVICE_ID_POINTER_PRESSED,
    RETRO_DEVICE_ID_POINTER_X, RETRO_DEVICE_ID_POINTER_Y, RETRO_DEVICE_INDEX_ANALOG_BUTTON,
    RETRO_DEVICE_INDEX_ANALOG_LEFT, RETRO_DEVICE_INDEX_ANALOG_RIGHT, RETRO_DEVICE_JOYPAD,
    RETRO_DEVICE_KEYBOARD, RETRO_DEVICE_LIGHTGUN, RETRO_DEVICE_MASK, RETRO_DEVICE_MOUSE,
    RETRO_DEVICE_NONE, RETRO_DEVICE_POINTER, RETRO_HW_CONTEXT_NONE, RETRO_HW_CONTEXT_OPENGL,
    RETRO_HW_CONTEXT_OPENGLES2, RETRO_HW_CONTEXT_OPENGLES3, RETRO_HW_CONTEXT_OPENGLES_VERSION,
    RETRO_HW_CONTEXT_OPENGL_CORE, RETRO_HW_CONTEXT_VULKAN, RETRO_MEMORY_RTC, RETRO_MEMORY_SAVE_RAM,
    RETRO_MEMORY_SYSTEM_RAM, RETRO_MEMORY_VIDEO_RAM,
};

pub trait HwContextProvider {
    fn get_proc_address(&self, sym: &str) -> *const c_void;
    fn get_current_framebuffer(&self) -> usize;
    fn supports_context_type(&self, ctx_type: u32) -> bool;
    fn make_current(&self);
    fn readback_bgra(&self, width: u32, height: u32) -> Vec<u8>;

    fn preferred_context_type(&self) -> u32 {
        RETRO_HW_CONTEXT_OPENGL_CORE
    }

    /// Returns the negotiation interface version supported for `interface_type`, if any.
    fn negotiation_version(&self, _interface_type: u32) -> Option<u32> {
        None
    }

    /// # Safety
    /// `iface` points to a negotiation interface the core keeps alive until it unloads.
    unsafe fn set_negotiation_interface(&self, _iface: *const c_void) -> bool {
        false
    }

    /// Creates whatever the context needs from the core before `context_reset` runs.
    fn prepare(&self) -> Result<(), String> {
        Ok(())
    }

    fn render_interface(&self) -> *const c_void {
        ptr::null()
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("failed to load core: {0}")]
    Load(#[from] libloading::Error),
    #[error("core api version mismatch: expected {expected}, got {got}")]
    ApiVersion { expected: u32, got: u32 },
    #[error("core failed to load game")]
    GameLoad,
    #[error("invalid path: {0}")]
    InvalidPath(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb1555,
    Xrgb8888,
    Rgb565,
}

impl PixelFormat {
    fn from_raw(v: c_int) -> Option<Self> {
        match v {
            sys::RETRO_PIXEL_FORMAT_0RGB1555 => Some(Self::Rgb1555),
            sys::RETRO_PIXEL_FORMAT_XRGB8888 => Some(Self::Xrgb8888),
            sys::RETRO_PIXEL_FORMAT_RGB565 => Some(Self::Rgb565),
            _ => None,
        }
    }

    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgb1555 | Self::Rgb565 => 2,
            Self::Xrgb8888 => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Geometry {
    pub base_width: u32,
    pub base_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub aspect_ratio: f32,
}

impl From<&sys::retro_game_geometry> for Geometry {
    fn from(g: &sys::retro_game_geometry) -> Self {
        Self {
            base_width: g.base_width,
            base_height: g.base_height,
            max_width: g.max_width,
            max_height: g.max_height,
            aspect_ratio: g.aspect_ratio,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    pub fps: f64,
    pub sample_rate: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AvInfo {
    pub geometry: Geometry,
    pub timing: Timing,
}

#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub library_name: String,
    pub library_version: String,
    pub valid_extensions: String,
    pub need_fullpath: bool,
    pub block_extract: bool,
}

pub struct VideoFrame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub pitch: usize,
}

pub trait Frontend {
    fn video_refresh(&mut self, frame: Option<VideoFrame<'_>>);
    fn video_refresh_hw(&mut self, width: u32, height: u32);
    fn audio_sample_batch(&mut self, samples: &[i16]) -> usize;
    fn input_poll(&mut self);
    fn input_state(&mut self, port: u32, device: u32, index: u32, id: u32) -> i16;
    fn set_pixel_format(&mut self, fmt: PixelFormat) -> bool;
    fn set_geometry(&mut self, _geometry: Geometry) {}
    fn shutdown(&mut self);
}

pub struct GameInfo<'a> {
    pub path: Option<&'a Path>,
    pub data: Option<&'a [u8]>,
}

struct Symbols {
    init: unsafe extern "C" fn(),
    deinit: unsafe extern "C" fn(),
    api_version: unsafe extern "C" fn() -> c_uint,
    get_system_info: unsafe extern "C" fn(*mut sys::retro_system_info),
    get_system_av_info: unsafe extern "C" fn(*mut sys::retro_system_av_info),
    set_environment: unsafe extern "C" fn(sys::retro_environment_t),
    set_video_refresh: unsafe extern "C" fn(sys::retro_video_refresh_t),
    set_audio_sample: unsafe extern "C" fn(sys::retro_audio_sample_t),
    set_audio_sample_batch: unsafe extern "C" fn(sys::retro_audio_sample_batch_t),
    set_input_poll: unsafe extern "C" fn(sys::retro_input_poll_t),
    set_input_state: unsafe extern "C" fn(sys::retro_input_state_t),
    load_game: unsafe extern "C" fn(*const sys::retro_game_info) -> bool,
    unload_game: unsafe extern "C" fn(),
    run: unsafe extern "C" fn(),
    set_controller_port_device: unsafe extern "C" fn(c_uint, c_uint),
    reset: unsafe extern "C" fn(),
    serialize_size: unsafe extern "C" fn() -> usize,
    serialize: unsafe extern "C" fn(*mut c_void, usize) -> bool,
    unserialize: unsafe extern "C" fn(*const c_void, usize) -> bool,
    get_memory_data: unsafe extern "C" fn(c_uint) -> *mut c_void,
    get_memory_size: unsafe extern "C" fn(c_uint) -> usize,
}

pub struct Core {
    _lib: Library,
    syms: Symbols,
    initialized: bool,
    game_loaded: bool,
}

impl Core {
    /// # Safety
    /// Loading runs the library's initializers in this process; `path` must be a libretro core.
    pub unsafe fn load(path: &Path) -> Result<Self, Error> {
        let lib = unsafe { Library::new(path) }?;
        let syms = unsafe { resolve(&lib) }?;
        let api = unsafe { (syms.api_version)() };
        if api != sys::RETRO_API_VERSION {
            return Err(Error::ApiVersion {
                expected: sys::RETRO_API_VERSION,
                got: api,
            });
        }
        unsafe {
            (syms.set_environment)(env_trampoline);
            (syms.set_video_refresh)(video_trampoline);
            (syms.set_audio_sample)(audio_sample_trampoline);
            (syms.set_audio_sample_batch)(audio_sample_batch_trampoline);
            (syms.set_input_poll)(input_poll_trampoline);
            (syms.set_input_state)(input_state_trampoline);
        }
        Ok(Self {
            _lib: lib,
            syms,
            initialized: false,
            game_loaded: false,
        })
    }

    pub fn system_info(&self) -> SystemInfo {
        let mut info = sys::retro_system_info {
            library_name: ptr::null(),
            library_version: ptr::null(),
            valid_extensions: ptr::null(),
            need_fullpath: false,
            block_extract: false,
        };
        unsafe { (self.syms.get_system_info)(&mut info) };
        unsafe {
            SystemInfo {
                library_name: cstr_to_string(info.library_name),
                library_version: cstr_to_string(info.library_version),
                valid_extensions: cstr_to_string(info.valid_extensions),
                need_fullpath: info.need_fullpath,
                block_extract: info.block_extract,
            }
        }
    }

    pub fn init<F: Frontend>(&mut self, frontend: &mut F) {
        if let Some(fmt) = PENDING_PIXEL_FORMAT.with(|p| p.borrow_mut().take()) {
            frontend.set_pixel_format(fmt);
        }
        let _g = FrontendGuard::install(frontend);
        unsafe { (self.syms.init)() };
        self.initialized = true;
    }

    pub fn av_info<F: Frontend>(&mut self, frontend: &mut F) -> AvInfo {
        let _g = FrontendGuard::install(frontend);
        let mut info = sys::retro_system_av_info::default();
        unsafe { (self.syms.get_system_av_info)(&mut info) };
        AvInfo {
            geometry: Geometry::from(&info.geometry),
            timing: Timing {
                fps: info.timing.fps,
                sample_rate: info.timing.sample_rate,
            },
        }
    }

    pub fn load_game<F: Frontend>(
        &mut self,
        info: GameInfo<'_>,
        frontend: &mut F,
    ) -> Result<(), Error> {
        let _g = FrontendGuard::install(frontend);
        let path_c = match info.path {
            Some(p) => {
                let s = p
                    .to_str()
                    .ok_or_else(|| Error::InvalidPath(p.to_string_lossy().into_owned()))?;
                Some(CString::new(s.as_bytes()).map_err(|_| Error::InvalidPath(s.to_owned()))?)
            }
            None => None,
        };
        let g = sys::retro_game_info {
            path: path_c.as_ref().map(|c| c.as_ptr()).unwrap_or(ptr::null()),
            data: info
                .data
                .map(|d| d.as_ptr().cast::<c_void>())
                .unwrap_or(ptr::null()),
            size: info.data.map(|d| d.len()).unwrap_or(0),
            meta: ptr::null(),
        };
        let ok = unsafe { (self.syms.load_game)(&g) };
        if !ok {
            return Err(Error::GameLoad);
        }
        self.game_loaded = true;
        Ok(())
    }

    pub fn load_no_game<F: Frontend>(&mut self, frontend: &mut F) -> Result<(), Error> {
        let _g = FrontendGuard::install(frontend);
        let ok = unsafe { (self.syms.load_game)(ptr::null()) };
        if !ok {
            return Err(Error::GameLoad);
        }
        self.game_loaded = true;
        Ok(())
    }

    pub fn run<F: Frontend>(&mut self, frontend: &mut F) {
        let _g = FrontendGuard::install(frontend);
        unsafe { (self.syms.run)() };
    }

    pub fn set_controller_port_device<F: Frontend>(
        &mut self,
        port: u32,
        device: u32,
        frontend: &mut F,
    ) {
        let _g = FrontendGuard::install(frontend);
        unsafe { (self.syms.set_controller_port_device)(port, device) };
    }

    pub fn reset<F: Frontend>(&mut self, frontend: &mut F) {
        let _g = FrontendGuard::install(frontend);
        unsafe { (self.syms.reset)() };
    }

    pub fn unload_game<F: Frontend>(&mut self, frontend: &mut F) {
        if !self.game_loaded {
            return;
        }
        let _g = FrontendGuard::install(frontend);
        unsafe { (self.syms.unload_game)() };
        self.game_loaded = false;
    }

    pub fn serialize_size<F: Frontend>(&mut self, frontend: &mut F) -> usize {
        let _g = FrontendGuard::install(frontend);
        unsafe { (self.syms.serialize_size)() }
    }

    pub fn serialize<F: Frontend>(&mut self, frontend: &mut F) -> Result<Vec<u8>, Error> {
        let _g = FrontendGuard::install(frontend);
        let size = unsafe { (self.syms.serialize_size)() };
        if size == 0 {
            return Err(Error::GameLoad);
        }
        let mut buf = vec![0u8; size];
        let ok = unsafe { (self.syms.serialize)(buf.as_mut_ptr().cast::<c_void>(), size) };
        if !ok {
            return Err(Error::GameLoad);
        }
        Ok(buf)
    }

    pub fn unserialize<F: Frontend>(&mut self, data: &[u8], frontend: &mut F) -> Result<(), Error> {
        let _g = FrontendGuard::install(frontend);
        let ok = unsafe { (self.syms.unserialize)(data.as_ptr().cast::<c_void>(), data.len()) };
        if !ok {
            return Err(Error::GameLoad);
        }
        Ok(())
    }

    /// # Safety
    /// The slice aliases core-owned memory and must not outlive the loaded game.
    pub unsafe fn memory_region<F: Frontend>(
        &mut self,
        kind: c_uint,
        frontend: &mut F,
    ) -> Option<&mut [u8]> {
        let _g = FrontendGuard::install(frontend);
        let size = unsafe { (self.syms.get_memory_size)(kind) };
        if size == 0 {
            return None;
        }
        let ptr = unsafe { (self.syms.get_memory_data)(kind) };
        if ptr.is_null() {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts_mut(ptr.cast::<u8>(), size) })
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        struct Noop;
        impl Frontend for Noop {
            fn video_refresh(&mut self, _: Option<VideoFrame<'_>>) {}
            fn video_refresh_hw(&mut self, _: u32, _: u32) {}
            fn audio_sample_batch(&mut self, s: &[i16]) -> usize {
                s.len() / 2
            }
            fn input_poll(&mut self) {}
            fn input_state(&mut self, _: u32, _: u32, _: u32, _: u32) -> i16 {
                0
            }
            fn set_pixel_format(&mut self, _: PixelFormat) -> bool {
                false
            }
            fn shutdown(&mut self) {}
        }
        let mut noop = Noop;
        let _g = FrontendGuard::install(&mut noop);
        if self.game_loaded {
            unsafe { (self.syms.unload_game)() };
        }
        if self.initialized {
            unsafe { (self.syms.deinit)() };
        }
    }
}

unsafe fn resolve(lib: &Library) -> Result<Symbols, Error> {
    unsafe {
        Ok(Symbols {
            init: *lib.get(b"retro_init\0")?,
            deinit: *lib.get(b"retro_deinit\0")?,
            api_version: *lib.get(b"retro_api_version\0")?,
            get_system_info: *lib.get(b"retro_get_system_info\0")?,
            get_system_av_info: *lib.get(b"retro_get_system_av_info\0")?,
            set_environment: *lib.get(b"retro_set_environment\0")?,
            set_video_refresh: *lib.get(b"retro_set_video_refresh\0")?,
            set_audio_sample: *lib.get(b"retro_set_audio_sample\0")?,
            set_audio_sample_batch: *lib.get(b"retro_set_audio_sample_batch\0")?,
            set_input_poll: *lib.get(b"retro_set_input_poll\0")?,
            set_input_state: *lib.get(b"retro_set_input_state\0")?,
            load_game: *lib.get(b"retro_load_game\0")?,
            unload_game: *lib.get(b"retro_unload_game\0")?,
            run: *lib.get(b"retro_run\0")?,
            set_controller_port_device: *lib.get(b"retro_set_controller_port_device\0")?,
            reset: *lib.get(b"retro_reset\0")?,
            serialize_size: *lib.get(b"retro_serialize_size\0")?,
            serialize: *lib.get(b"retro_serialize\0")?,
            unserialize: *lib.get(b"retro_unserialize\0")?,
            get_memory_data: *lib.get(b"retro_get_memory_data\0")?,
            get_memory_size: *lib.get(b"retro_get_memory_size\0")?,
        })
    }
}

unsafe fn cstr_to_string(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

thread_local! {
    static CURRENT_FRONTEND: RefCell<Option<*mut dyn Frontend>> =
        const { RefCell::new(None) };
    static PENDING_PIXEL_FORMAT: RefCell<Option<PixelFormat>> =
        const { RefCell::new(None) };
}

struct FrontendGuard<'a> {
    prev: Option<*mut dyn Frontend>,
    _marker: std::marker::PhantomData<&'a mut ()>,
}

impl<'a> FrontendGuard<'a> {
    fn install<F: Frontend + 'a>(f: &'a mut F) -> Self {
        let raw: *mut (dyn Frontend + 'a) = f;
        let raw_static: *mut (dyn Frontend + 'static) = unsafe { std::mem::transmute(raw) };
        let prev = CURRENT_FRONTEND.with(|c| c.replace(Some(raw_static)));
        Self {
            prev,
            _marker: std::marker::PhantomData,
        }
    }
}

impl Drop for FrontendGuard<'_> {
    fn drop(&mut self) {
        let prev = self.prev.take();
        CURRENT_FRONTEND.with(|c| *c.borrow_mut() = prev);
    }
}

pub type InputSource = std::sync::Arc<dyn Fn(u32, u32, u32, u32) -> i16 + Send + Sync>;

static INPUT_SOURCE: std::sync::RwLock<Option<InputSource>> = std::sync::RwLock::new(None);

pub type AudioSink = std::sync::Arc<dyn Fn(&[i16]) -> usize + Send + Sync>;

static AUDIO_SINK: std::sync::RwLock<Option<AudioSink>> = std::sync::RwLock::new(None);

/// Takes audio that cores produce from their own threads, outside any `Core` call.
pub fn set_audio_sink(sink: Option<AudioSink>) {
    *AUDIO_SINK.write().unwrap() = sink;
}

fn send_to_sink(samples: &[i16]) -> usize {
    let sink = AUDIO_SINK.read().unwrap().clone();
    sink.map_or(samples.len() / 2, |s| s(samples))
}

/// Answers input reads that cores make from their own threads, outside any `Core` call.
pub fn set_input_source(source: Option<InputSource>) {
    *INPUT_SOURCE.write().unwrap() = source;
}

/// Runs a call into the core outside the `Core` methods, such as a hardware context reset,
/// with `frontend` receiving any callbacks the core makes during it.
pub fn with_frontend_installed<F: Frontend, R>(frontend: &mut F, call: impl FnOnce() -> R) -> R {
    let _g = FrontendGuard::install(frontend);
    call()
}

fn try_frontend<R>(f: impl FnOnce(&mut dyn Frontend) -> R) -> Option<R> {
    let ptr = CURRENT_FRONTEND.with(|c| *c.borrow())?;
    Some(unsafe { f(&mut *ptr) })
}

#[track_caller]
fn with_frontend<R: Default>(f: impl FnOnce(&mut dyn Frontend) -> R) -> R {
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let caller = std::panic::Location::caller();
    try_frontend(f).unwrap_or_else(|| {
        if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log!("[libretro] ignoring a callback ({caller}) made outside a core call");
        }
        R::default()
    })
}

thread_local! {
    static SYSTEM_DIR: RefCell<Option<CString>> = const { RefCell::new(None) };
    static SAVE_DIR:   RefCell<Option<CString>> = const { RefCell::new(None) };
}

struct RawProvider(*mut dyn HwContextProvider);
// SAFETY: install_hw_provider's caller keeps the pointee valid while installed.
unsafe impl Send for RawProvider {}

static HW_PROVIDER: std::sync::Mutex<Option<RawProvider>> = std::sync::Mutex::new(None);
static HW_CONTEXT_RESET: std::sync::Mutex<Option<unsafe extern "C" fn()>> =
    std::sync::Mutex::new(None);
static HW_CONTEXT_DESTROY: std::sync::Mutex<Option<unsafe extern "C" fn()>> =
    std::sync::Mutex::new(None);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerType {
    pub name: String,
    pub id: u32,
}

static ROTATION: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn rotation() -> u32 {
    ROTATION.load(std::sync::atomic::Ordering::Relaxed)
}

static CONTROLLER_INFO: std::sync::Mutex<Vec<Vec<ControllerType>>> =
    std::sync::Mutex::new(Vec::new());

pub fn controller_info() -> Vec<Vec<ControllerType>> {
    CONTROLLER_INFO.lock().unwrap().clone()
}

/// # Safety
/// `info` must point to an array of `retro_controller_info` terminated by an entry with null
/// `types`, as the libretro API requires.
unsafe fn parse_controller_info(
    info: *const sys::retro_controller_info,
) -> Vec<Vec<ControllerType>> {
    const MAX_PORTS: usize = 16;
    let mut ports = Vec::new();
    for i in 0..MAX_PORTS {
        let port = unsafe { &*info.add(i) };
        if port.types.is_null() {
            break;
        }
        let types = unsafe { std::slice::from_raw_parts(port.types, port.num_types as usize) };
        ports.push(
            types
                .iter()
                .filter(|t| !t.desc.is_null())
                .map(|t| ControllerType {
                    name: unsafe { cstr_to_string(t.desc) },
                    id: t.id,
                })
                .collect(),
        );
    }
    ports
}

static KEYBOARD_CALLBACK: std::sync::Mutex<sys::retro_keyboard_event_t> =
    std::sync::Mutex::new(None);

pub fn invoke_keyboard_callback(down: bool, key: u32, character: u32, modifiers: u16) {
    let cb = *KEYBOARD_CALLBACK.lock().unwrap();
    if let Some(cb) = cb {
        unsafe { cb(down, key, character, modifiers) };
    }
}

pub fn keyboard_callback_set() -> bool {
    KEYBOARD_CALLBACK.lock().unwrap().is_some()
}

#[allow(dead_code)]
#[derive(Default, Clone, Copy)]
struct DiskControlVTable {
    set_eject_state: Option<sys::retro_set_eject_state_t>,
    get_eject_state: Option<sys::retro_get_eject_state_t>,
    get_image_index: Option<sys::retro_get_image_index_t>,
    set_image_index: Option<sys::retro_set_image_index_t>,
    get_num_images: Option<sys::retro_get_num_images_t>,
    replace_image_index: Option<sys::retro_replace_image_index_t>,
    add_image_index: Option<sys::retro_add_image_index_t>,
    set_initial_image: Option<sys::retro_set_initial_image_t>,
    get_image_path: Option<sys::retro_get_image_path_t>,
    get_image_label: Option<sys::retro_get_image_label_t>,
}

unsafe impl Send for DiskControlVTable {}

static DISK_CONTROL: std::sync::Mutex<Option<DiskControlVTable>> = std::sync::Mutex::new(None);

// Programmatic swaps crash some cores (PUAE); disabled until gated per system.
pub fn disk_control_swap(_index: u32) -> bool {
    false
}

/// # Safety
/// The provider must stay valid until `uninstall_hw_provider` is called.
pub unsafe fn install_hw_provider(p: *mut dyn HwContextProvider) {
    *HW_PROVIDER.lock().unwrap() = Some(RawProvider(p));
}

pub fn uninstall_hw_provider() {
    *HW_PROVIDER.lock().unwrap() = None;
}

pub fn take_pending_hw_reset() -> Option<unsafe extern "C" fn()> {
    HW_CONTEXT_RESET.lock().unwrap().take()
}

pub fn set_core_dirs(system_dir: Option<&std::path::Path>, save_dir: Option<&std::path::Path>) {
    let to_cstring = |p: &std::path::Path| p.to_str().and_then(|s| CString::new(s).ok());
    SYSTEM_DIR.with(|s| *s.borrow_mut() = system_dir.and_then(to_cstring));
    SAVE_DIR.with(|s| *s.borrow_mut() = save_dir.and_then(to_cstring));
}

#[derive(Clone, Debug)]
pub struct CoreOptionDef {
    pub key: String,
    pub label: String,
    pub info: String,
    pub category: String,
    pub default_value: String,
    pub values: Vec<CoreOptionValue>,
}

#[derive(Clone, Debug)]
pub struct CoreOptionValue {
    pub value: String,
    pub label: String,
}

#[derive(Default)]
struct CoreOptionsState {
    defs: Vec<CoreOptionDef>,
    values: std::collections::HashMap<String, CString>,
    dirty: bool,
    retired: Vec<CString>,
}

static CORE_OPTIONS: std::sync::Mutex<Option<CoreOptionsState>> = std::sync::Mutex::new(None);

fn with_core_options<R>(f: impl FnOnce(&mut CoreOptionsState) -> R) -> R {
    let mut guard = CORE_OPTIONS.lock().unwrap();
    f(guard.get_or_insert_with(CoreOptionsState::default))
}

fn store_core_option_defs(defs: Vec<CoreOptionDef>) {
    with_core_options(|st| {
        st.values = defs
            .iter()
            .filter_map(|d| {
                CString::new(d.default_value.as_str())
                    .ok()
                    .map(|c| (d.key.clone(), c))
            })
            .collect();
        st.defs = defs;
        st.dirty = true;
    });
}

pub fn core_option_defs() -> Vec<CoreOptionDef> {
    with_core_options(|st| st.defs.clone())
}

pub fn set_core_option_values(values: &[(String, String)]) {
    with_core_options(|st| {
        for (key, value) in values {
            if st.defs.iter().any(|d| &d.key == key) {
                if let Ok(c) = CString::new(value.as_str()) {
                    if let Some(old) = st.values.insert(key.clone(), c) {
                        st.retired.push(old);
                    }
                }
            }
        }
        st.dirty = true;
    });
}

pub fn update_core_option(key: &str, value: &str) {
    with_core_options(|st| {
        if let Ok(c) = CString::new(value) {
            if let Some(old) = st.values.insert(key.to_string(), c) {
                st.retired.push(old);
            }
            st.dirty = true;
        }
    });
}

pub fn clear_core_options() {
    with_core_options(|st| {
        st.defs.clear();
        st.values.clear();
        st.retired.clear();
        st.dirty = false;
    });
}

unsafe fn parse_option_values(values: &[sys::retro_core_option_value]) -> Vec<CoreOptionValue> {
    let mut out = Vec::new();
    for v in values {
        if v.value.is_null() {
            break;
        }
        let value = unsafe { cstr_to_string(v.value) };
        let label = if v.label.is_null() {
            value.clone()
        } else {
            unsafe { cstr_to_string(v.label) }
        };
        out.push(CoreOptionValue { value, label });
    }
    out
}

unsafe fn parse_v2_definitions(
    defs: *const sys::retro_core_option_v2_definition,
) -> Vec<CoreOptionDef> {
    let mut out = Vec::new();
    if defs.is_null() {
        return out;
    }
    let mut i = 0usize;
    loop {
        let d = unsafe { &*defs.add(i) };
        if d.key.is_null() {
            break;
        }
        out.push(CoreOptionDef {
            key: unsafe { cstr_to_string(d.key) },
            label: unsafe { cstr_to_string(d.desc) },
            info: unsafe { cstr_to_string(d.info) },
            category: unsafe { cstr_to_string(d.category_key) },
            default_value: unsafe { cstr_to_string(d.default_value) },
            values: unsafe { parse_option_values(&d.values) },
        });
        i += 1;
    }
    out
}

unsafe fn parse_v1_definitions(
    defs: *const sys::retro_core_option_definition,
) -> Vec<CoreOptionDef> {
    let mut out = Vec::new();
    if defs.is_null() {
        return out;
    }
    let mut i = 0usize;
    loop {
        let d = unsafe { &*defs.add(i) };
        if d.key.is_null() {
            break;
        }
        out.push(CoreOptionDef {
            key: unsafe { cstr_to_string(d.key) },
            label: unsafe { cstr_to_string(d.desc) },
            info: unsafe { cstr_to_string(d.info) },
            category: String::new(),
            default_value: unsafe { cstr_to_string(d.default_value) },
            values: unsafe { parse_option_values(&d.values) },
        });
        i += 1;
    }
    out
}

unsafe fn parse_variables(vars: *const sys::retro_variable) -> Vec<CoreOptionDef> {
    let mut out = Vec::new();
    if vars.is_null() {
        return out;
    }
    let mut i = 0usize;
    loop {
        let v = unsafe { &*vars.add(i) };
        if v.key.is_null() {
            break;
        }
        let key = unsafe { cstr_to_string(v.key) };
        let raw = unsafe { cstr_to_string(v.value) };
        let (label, choices) = match raw.split_once(';') {
            Some((d, c)) => (d.trim().to_string(), c),
            None => (raw.clone(), ""),
        };
        let values: Vec<CoreOptionValue> = choices
            .split('|')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| CoreOptionValue {
                value: s.to_string(),
                label: s.to_string(),
            })
            .collect();
        let default_value = values
            .first()
            .map(|first| first.value.clone())
            .unwrap_or_default();
        out.push(CoreOptionDef {
            key,
            label,
            info: String::new(),
            category: String::new(),
            default_value,
            values,
        });
        i += 1;
    }
    out
}

unsafe extern "C" fn env_trampoline(cmd: c_uint, data: *mut c_void) -> bool {
    match cmd {
        sys::RETRO_ENVIRONMENT_GET_CAN_DUPE => {
            if data.is_null() {
                return false;
            }
            unsafe { *(data as *mut bool) = true };
            true
        }
        sys::RETRO_ENVIRONMENT_GET_FASTFORWARDING => {
            if data.is_null() {
                return false;
            }
            unsafe { *(data as *mut bool) = false };
            true
        }
        sys::RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE => {
            if data.is_null() {
                return false;
            }
            let dirty = with_core_options(|st| std::mem::replace(&mut st.dirty, false));
            unsafe { *(data as *mut bool) = dirty };
            true
        }
        sys::RETRO_ENVIRONMENT_SET_MESSAGE => {
            if !data.is_null() {
                let msg = unsafe { &*(data as *const sys::retro_message) };
                if !msg.msg.is_null() {
                    let text = unsafe { CStr::from_ptr(msg.msg) }
                        .to_string_lossy()
                        .into_owned();
                    log!("[core OSD] {text} (frames={})", msg.frames);
                }
            }
            true
        }
        sys::RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION => {
            if data.is_null() {
                return false;
            }
            unsafe { *(data as *mut c_uint) = 2 };
            true
        }
        sys::RETRO_ENVIRONMENT_GET_VARIABLE => {
            if data.is_null() {
                return false;
            }
            let var = unsafe { &mut *(data as *mut sys::retro_variable) };
            if var.key.is_null() {
                return false;
            }
            let key = unsafe { CStr::from_ptr(var.key) }
                .to_string_lossy()
                .into_owned();
            with_core_options(|st| match st.values.get(&key) {
                Some(c) => {
                    var.value = c.as_ptr();
                    true
                }
                None => false,
            })
        }
        sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2 => {
            if data.is_null() {
                return false;
            }
            let opts = unsafe { &*(data as *const sys::retro_core_options_v2) };
            store_core_option_defs(unsafe { parse_v2_definitions(opts.definitions) });
            true
        }
        sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL => {
            if data.is_null() {
                return false;
            }
            let intl = unsafe { &*(data as *const sys::retro_core_options_v2_intl) };
            if intl.us.is_null() {
                return false;
            }
            let opts = unsafe { &*intl.us };
            store_core_option_defs(unsafe { parse_v2_definitions(opts.definitions) });
            true
        }
        sys::RETRO_ENVIRONMENT_SET_CORE_OPTIONS => {
            if data.is_null() {
                return false;
            }
            store_core_option_defs(unsafe {
                parse_v1_definitions(data as *const sys::retro_core_option_definition)
            });
            true
        }
        sys::RETRO_ENVIRONMENT_SET_VARIABLES => {
            if data.is_null() {
                return false;
            }
            store_core_option_defs(unsafe { parse_variables(data as *const sys::retro_variable) });
            true
        }
        sys::RETRO_ENVIRONMENT_SET_GEOMETRY => {
            if data.is_null() {
                return false;
            }
            let geometry = Geometry::from(unsafe { &*(data as *const sys::retro_game_geometry) });
            with_frontend(|f| f.set_geometry(geometry));
            true
        }
        sys::RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO => {
            if data.is_null() {
                return false;
            }
            let info = unsafe { &*(data as *const sys::retro_system_av_info) };
            with_frontend(|f| f.set_geometry(Geometry::from(&info.geometry)));
            true
        }
        sys::RETRO_ENVIRONMENT_SET_PIXEL_FORMAT => {
            if data.is_null() {
                return false;
            }
            let raw = unsafe { *(data as *const c_int) };
            let Some(fmt) = PixelFormat::from_raw(raw) else {
                return false;
            };
            if CURRENT_FRONTEND.with(|c| c.borrow().is_some()) {
                with_frontend(|f| f.set_pixel_format(fmt))
            } else {
                PENDING_PIXEL_FORMAT.with(|p| *p.borrow_mut() = Some(fmt));
                true
            }
        }
        sys::RETRO_ENVIRONMENT_SET_KEYBOARD_CALLBACK => {
            if data.is_null() {
                return false;
            }
            let cb = unsafe { &*(data as *const sys::retro_keyboard_callback) };
            *KEYBOARD_CALLBACK.lock().unwrap() = cb.callback;
            true
        }
        sys::RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE => {
            if data.is_null() {
                return false;
            }
            // Some cores pass this struct misaligned.
            let cb: sys::retro_disk_control_callback = unsafe {
                std::ptr::read_unaligned(data as *const sys::retro_disk_control_callback)
            };
            *DISK_CONTROL.lock().unwrap() = Some(DiskControlVTable {
                set_eject_state: cb.set_eject_state,
                get_eject_state: cb.get_eject_state,
                get_image_index: cb.get_image_index,
                set_image_index: cb.set_image_index,
                get_num_images: cb.get_num_images,
                replace_image_index: cb.replace_image_index,
                add_image_index: cb.add_image_index,
                set_initial_image: None,
                get_image_path: None,
                get_image_label: None,
            });
            true
        }
        sys::RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE => {
            if data.is_null() {
                return false;
            }
            let cb: sys::retro_disk_control_ext_callback = unsafe {
                std::ptr::read_unaligned(data as *const sys::retro_disk_control_ext_callback)
            };
            *DISK_CONTROL.lock().unwrap() = Some(DiskControlVTable {
                set_eject_state: cb.set_eject_state,
                get_eject_state: cb.get_eject_state,
                get_image_index: cb.get_image_index,
                set_image_index: cb.set_image_index,
                get_num_images: cb.get_num_images,
                replace_image_index: cb.replace_image_index,
                add_image_index: cb.add_image_index,
                set_initial_image: cb.set_initial_image,
                get_image_path: cb.get_image_path,
                get_image_label: cb.get_image_label,
            });
            true
        }
        sys::RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME => true,
        sys::RETRO_ENVIRONMENT_SET_ROTATION => {
            if data.is_null() {
                return false;
            }
            let quarter_turns = unsafe { *(data as *const c_uint) } % 4;
            ROTATION.store(quarter_turns, std::sync::atomic::Ordering::Relaxed);
            true
        }
        sys::RETRO_ENVIRONMENT_SET_CONTROLLER_INFO => {
            if data.is_null() {
                return false;
            }
            let ports = unsafe { parse_controller_info(data as *const sys::retro_controller_info) };
            *CONTROLLER_INFO.lock().unwrap() = ports;
            true
        }
        sys::RETRO_ENVIRONMENT_GET_LANGUAGE => {
            if data.is_null() {
                return false;
            }
            unsafe { *(data as *mut c_uint) = 0 };
            true
        }
        sys::RETRO_ENVIRONMENT_GET_INPUT_BITMASKS => {
            let _ = data;
            false
        }
        sys::RETRO_ENVIRONMENT_SHUTDOWN => {
            if CURRENT_FRONTEND.with(|c| c.borrow().is_some()) {
                with_frontend(|f| f.shutdown());
            }
            true
        }
        sys::RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY => {
            if data.is_null() {
                return false;
            }
            SYSTEM_DIR.with(|s| match s.borrow().as_ref() {
                Some(dir) => {
                    unsafe { *(data as *mut *const c_char) = dir.as_ptr() };
                    true
                }
                None => false,
            })
        }
        sys::RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY => {
            if data.is_null() {
                return false;
            }
            SAVE_DIR.with(|s| match s.borrow().as_ref() {
                Some(dir) => {
                    unsafe { *(data as *mut *const c_char) = dir.as_ptr() };
                    true
                }
                None => false,
            })
        }
        sys::RETRO_ENVIRONMENT_GET_LOG_INTERFACE => {
            if data.is_null() {
                return false;
            }
            let cb = unsafe { &mut *(data as *mut sys::retro_log_callback) };
            cb.log = romp_core_log;
            true
        }
        sys::RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER => {
            if data.is_null() {
                return false;
            }
            let preferred = with_provider(|p| p.preferred_context_type())
                .unwrap_or(sys::RETRO_HW_CONTEXT_OPENGL_CORE);
            unsafe { *(data as *mut c_uint) = preferred };
            true
        }
        sys::RETRO_ENVIRONMENT_GET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE_SUPPORT => {
            if data.is_null() {
                return false;
            }
            let query =
                unsafe { &mut *(data as *mut sys::retro_hw_render_context_negotiation_interface) };
            match with_provider(|p| p.negotiation_version(query.interface_type)).flatten() {
                Some(version) => {
                    query.interface_version = version;
                    true
                }
                None => false,
            }
        }
        sys::RETRO_ENVIRONMENT_SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE => {
            if data.is_null() {
                return false;
            }
            with_provider(|p| unsafe { p.set_negotiation_interface(data) }).unwrap_or(false)
        }
        sys::RETRO_ENVIRONMENT_GET_HW_RENDER_INTERFACE => {
            if data.is_null() {
                return false;
            }
            let iface = with_provider(|p| p.render_interface()).unwrap_or(ptr::null());
            unsafe { *(data as *mut *const c_void) = iface };
            !iface.is_null()
        }
        sys::RETRO_ENVIRONMENT_SET_HW_RENDER => {
            if data.is_null() {
                return false;
            }
            let cb = unsafe { &mut *(data as *mut sys::retro_hw_render_callback) };
            log!("[env] SET_HW_RENDER context_type={}", cb.context_type);
            let raw = HW_PROVIDER.lock().unwrap().as_ref().map(|r| r.0);
            if let Some(p) = raw {
                let provider = unsafe { &*p };
                if provider.supports_context_type(cb.context_type) {
                    cb.get_proc_address = Some(hw_get_proc_address);
                    cb.get_current_framebuffer = Some(hw_get_current_framebuffer);
                    *HW_CONTEXT_RESET.lock().unwrap() = cb.context_reset;
                    *HW_CONTEXT_DESTROY.lock().unwrap() = cb.context_destroy;
                    true
                } else {
                    log!(
                        "[env] SET_HW_RENDER unsupported context_type={}",
                        cb.context_type
                    );
                    false
                }
            } else {
                log!("[env] SET_HW_RENDER no HW provider");
                false
            }
        }
        _ => {
            log!("[env] unhandled cmd={}", cmd);
            false
        }
    }
}

extern "C" {
    fn romp_core_log(level: c_uint, fmt: *const c_char, ...);
}

fn with_provider<R>(f: impl FnOnce(&dyn HwContextProvider) -> R) -> Option<R> {
    let raw = HW_PROVIDER.lock().unwrap().as_ref().map(|r| r.0);
    // SAFETY: install_hw_provider's caller keeps the provider valid while it is installed.
    raw.map(|p| f(unsafe { &*p }))
}

unsafe extern "C" fn hw_get_proc_address(sym: *const c_char) -> *const c_void {
    if sym.is_null() {
        return ptr::null();
    }
    let name = unsafe { CStr::from_ptr(sym) }.to_string_lossy();
    let raw = HW_PROVIDER.lock().unwrap().as_ref().map(|r| r.0);
    raw.map_or(ptr::null(), |p| unsafe { (*p).get_proc_address(&name) })
}

unsafe extern "C" fn hw_get_current_framebuffer() -> usize {
    let raw = HW_PROVIDER.lock().unwrap().as_ref().map(|r| r.0);
    raw.map_or(0, |p| unsafe { (*p).get_current_framebuffer() })
}

unsafe extern "C" fn video_trampoline(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
) {
    if data == sys::RETRO_HW_FRAME_BUFFER_VALID {
        with_frontend(|f| f.video_refresh_hw(width, height));
        return;
    }
    if data.is_null() {
        with_frontend(|f| f.video_refresh(None));
        return;
    }
    let len = pitch.saturating_mul(height as usize);
    let slice = unsafe { std::slice::from_raw_parts(data as *const u8, len) };
    let frame = VideoFrame {
        data: slice,
        width,
        height,
        pitch,
    };
    with_frontend(|f| f.video_refresh(Some(frame)));
}

unsafe extern "C" fn audio_sample_trampoline(left: i16, right: i16) {
    let buf = [left, right];
    if try_frontend(|f| f.audio_sample_batch(&buf)).is_none() {
        send_to_sink(&buf);
    }
}

unsafe extern "C" fn audio_sample_batch_trampoline(data: *const i16, frames: usize) -> usize {
    let slice = unsafe { std::slice::from_raw_parts(data, frames * 2) };
    try_frontend(|f| f.audio_sample_batch(slice)).unwrap_or_else(|| send_to_sink(slice))
}

unsafe extern "C" fn input_poll_trampoline() {
    let _ = try_frontend(|f| f.input_poll());
}

unsafe extern "C" fn input_state_trampoline(
    port: c_uint,
    device: c_uint,
    index: c_uint,
    id: c_uint,
) -> i16 {
    try_frontend(|f| f.input_state(port, device, index, id)).unwrap_or_else(|| {
        let source = INPUT_SOURCE.read().unwrap().clone();
        source.map_or(0, |s| s(port, device, index, id))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_info_lists_every_port_until_the_terminator() {
        let pad = CString::new("RetroPad").unwrap();
        let mouse = CString::new("SNES Mouse").unwrap();
        let scope = CString::new("Super Scope").unwrap();
        let port1 = [
            sys::retro_controller_description {
                desc: pad.as_ptr(),
                id: 1,
            },
            sys::retro_controller_description {
                desc: mouse.as_ptr(),
                id: 0x102,
            },
        ];
        let port2 = [sys::retro_controller_description {
            desc: scope.as_ptr(),
            id: 0x104,
        }];
        let info = [
            sys::retro_controller_info {
                types: port1.as_ptr(),
                num_types: 2,
            },
            sys::retro_controller_info {
                types: port2.as_ptr(),
                num_types: 1,
            },
            sys::retro_controller_info {
                types: ptr::null(),
                num_types: 0,
            },
        ];
        let ports = unsafe { parse_controller_info(info.as_ptr()) };
        assert_eq!(ports.len(), 2);
        assert_eq!(
            ports[0][1],
            ControllerType {
                name: "SNES Mouse".into(),
                id: 0x102
            }
        );
        assert_eq!(ports[1][0].name, "Super Scope");
    }

    #[derive(Default)]
    struct Recorder {
        geometry: Option<Geometry>,
        hw_frame: Option<(u32, u32)>,
    }

    impl Frontend for Recorder {
        fn video_refresh(&mut self, _: Option<VideoFrame<'_>>) {}
        fn video_refresh_hw(&mut self, width: u32, height: u32) {
            self.hw_frame = Some((width, height));
        }
        fn audio_sample_batch(&mut self, s: &[i16]) -> usize {
            s.len() / 2
        }
        fn input_poll(&mut self) {}
        fn input_state(&mut self, _: u32, _: u32, _: u32, _: u32) -> i16 {
            0
        }
        fn set_pixel_format(&mut self, _: PixelFormat) -> bool {
            true
        }
        fn shutdown(&mut self) {}
        fn set_geometry(&mut self, geometry: Geometry) {
            self.geometry = Some(geometry);
        }
    }

    fn raw_geometry(aspect_ratio: f32) -> sys::retro_game_geometry {
        sys::retro_game_geometry {
            base_width: 320,
            base_height: 240,
            max_width: 640,
            max_height: 480,
            aspect_ratio,
        }
    }

    static GLOBAL_CALLBACKS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn input_read_from_a_core_thread_uses_the_input_source() {
        let _serial = GLOBAL_CALLBACKS.lock().unwrap();
        set_input_source(Some(std::sync::Arc::new(|port, device, _, id| {
            (port * 100 + device * 10 + id) as i16
        })));
        let value = std::thread::spawn(|| unsafe {
            input_state_trampoline(1, sys::RETRO_DEVICE_JOYPAD, 0, 8)
        })
        .join()
        .unwrap();
        assert_eq!(value, 118);
        set_input_source(None);
    }

    #[test]
    fn audio_from_a_core_thread_goes_to_the_audio_sink() {
        let _serial = GLOBAL_CALLBACKS.lock().unwrap();
        let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        set_audio_sink(Some(std::sync::Arc::new({
            let heard = heard.clone();
            move |samples: &[i16]| {
                heard.lock().unwrap().extend_from_slice(samples);
                samples.len() / 2
            }
        })));
        let samples = [1i16, 2, 3, 4];
        let accepted = std::thread::spawn(move || unsafe {
            audio_sample_batch_trampoline(samples.as_ptr(), 2)
        })
        .join()
        .unwrap();
        set_audio_sink(None);
        assert_eq!(accepted, 2);
        assert_eq!(*heard.lock().unwrap(), [1, 2, 3, 4]);
    }

    #[test]
    fn stray_callbacks_from_core_threads_are_ignored() {
        let _serial = GLOBAL_CALLBACKS.lock().unwrap();
        let samples = [0i16; 4];
        let accepted = std::thread::spawn(move || unsafe {
            video_trampoline(sys::RETRO_HW_FRAME_BUFFER_VALID, 640, 480, 0);
            input_poll_trampoline();
            audio_sample_batch_trampoline(samples.as_ptr(), 2)
        })
        .join()
        .unwrap();
        assert_eq!(accepted, 2);
    }

    #[test]
    fn frames_presented_during_a_context_reset_reach_the_frontend() {
        let mut recorder = Recorder::default();
        with_frontend_installed(&mut recorder, || unsafe {
            video_trampoline(sys::RETRO_HW_FRAME_BUFFER_VALID, 640, 528, 0)
        });
        assert_eq!(recorder.hw_frame, Some((640, 528)));
    }

    #[test]
    fn set_geometry_reaches_the_frontend() {
        let mut recorder = Recorder::default();
        let mut raw = raw_geometry(1.5);
        let handled = {
            let _g = FrontendGuard::install(&mut recorder);
            unsafe {
                env_trampoline(
                    sys::RETRO_ENVIRONMENT_SET_GEOMETRY,
                    &mut raw as *mut _ as *mut c_void,
                )
            }
        };
        assert!(handled);
        let g = recorder.geometry.expect("geometry forwarded");
        assert_eq!(
            (g.base_width, g.base_height, g.aspect_ratio),
            (320, 240, 1.5)
        );
    }

    #[test]
    fn set_system_av_info_forwards_its_geometry() {
        let mut recorder = Recorder::default();
        let mut raw = sys::retro_system_av_info {
            geometry: raw_geometry(4.0 / 3.0),
            ..Default::default()
        };
        let handled = {
            let _g = FrontendGuard::install(&mut recorder);
            unsafe {
                env_trampoline(
                    sys::RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO,
                    &mut raw as *mut _ as *mut c_void,
                )
            }
        };
        assert!(handled);
        assert_eq!(recorder.geometry.unwrap().aspect_ratio, 4.0 / 3.0);
    }
}
