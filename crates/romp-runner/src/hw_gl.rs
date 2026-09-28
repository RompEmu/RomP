use romp_libretro::{
    HwContextProvider, RETRO_HW_CONTEXT_OPENGL, RETRO_HW_CONTEXT_OPENGLES2,
    RETRO_HW_CONTEXT_OPENGLES3, RETRO_HW_CONTEXT_OPENGL_CORE,
};
use std::ffi::c_void;

#[allow(dead_code)]
mod gl {
    pub const TEXTURE_2D: u32 = 0x0DE1;
    pub const TEXTURE_MIN_FILTER: u32 = 0x2801;
    pub const TEXTURE_MAG_FILTER: u32 = 0x2800;
    pub const LINEAR: u32 = 0x2601;
    pub const RGBA: u32 = 0x1908;
    pub const BGRA: u32 = 0x80E1;
    pub const RGBA8: u32 = 0x8058;
    pub const UNSIGNED_BYTE: u32 = 0x1401;
    pub const FRAMEBUFFER: u32 = 0x8D40;
    pub const READ_FRAMEBUFFER: u32 = 0x8CA8;
    pub const COLOR_ATTACHMENT0: u32 = 0x8CE0;
    pub const DEPTH_ATTACHMENT: u32 = 0x8D00;
    pub const RENDERBUFFER: u32 = 0x8D41;
    pub const DEPTH_COMPONENT24: u32 = 0x81A5;
    pub const FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
    pub const PIXEL_PACK_BUFFER: u32 = 0x88EB;
    pub const STREAM_READ: u32 = 0x88E1;
    pub const READ_ONLY: u32 = 0x88B8;
}

type GenFn = unsafe extern "C" fn(n: i32, ids: *mut u32);
type BindFn = unsafe extern "C" fn(target: u32, id: u32);
type DeleteFn = unsafe extern "C" fn(n: i32, ids: *const u32);
type TexImage2DFn = unsafe extern "C" fn(
    target: u32,
    level: i32,
    internal_fmt: i32,
    w: i32,
    h: i32,
    border: i32,
    fmt: u32,
    typ: u32,
    data: *const c_void,
);
type TexParameteriFn = unsafe extern "C" fn(target: u32, pname: u32, param: i32);
type FbTexture2DFn =
    unsafe extern "C" fn(target: u32, attachment: u32, textarget: u32, tex: u32, level: i32);
type RbStorageFn = unsafe extern "C" fn(target: u32, internal_fmt: u32, w: i32, h: i32);
type FbRbFn = unsafe extern "C" fn(target: u32, attachment: u32, rbtarget: u32, rb: u32);
type CheckFbFn = unsafe extern "C" fn(target: u32) -> u32;
type ReadPixelsFn =
    unsafe extern "C" fn(x: i32, y: i32, w: i32, h: i32, fmt: u32, typ: u32, px: *mut c_void);
#[cfg(target_os = "linux")]
type BufferDataFn = unsafe extern "C" fn(target: u32, size: isize, data: *const c_void, usage: u32);
#[cfg(target_os = "linux")]
type MapBufferFn = unsafe extern "C" fn(target: u32, access: u32) -> *mut c_void;
#[cfg(target_os = "linux")]
type UnmapBufferFn = unsafe extern "C" fn(target: u32) -> u8;

struct GlVtable {
    gen_framebuffers: GenFn,
    bind_framebuffer: BindFn,
    delete_framebuffers: DeleteFn,
    gen_textures: GenFn,
    bind_texture: BindFn,
    delete_textures: DeleteFn,
    tex_image_2d: TexImage2DFn,
    tex_parameteri: TexParameteriFn,
    framebuffer_texture_2d: FbTexture2DFn,
    gen_renderbuffers: GenFn,
    bind_renderbuffer: BindFn,
    delete_renderbuffers: DeleteFn,
    renderbuffer_storage: RbStorageFn,
    framebuffer_renderbuffer: FbRbFn,
    check_framebuffer_status: CheckFbFn,
    read_pixels: ReadPixelsFn,
    #[cfg(target_os = "linux")]
    gen_buffers: GenFn,
    #[cfg(target_os = "linux")]
    bind_buffer: BindFn,
    #[cfg(target_os = "linux")]
    buffer_data: BufferDataFn,
    #[cfg(target_os = "linux")]
    map_buffer: MapBufferFn,
    #[cfg(target_os = "linux")]
    unmap_buffer: UnmapBufferFn,
    #[cfg(target_os = "linux")]
    delete_buffers: DeleteFn,
}

impl GlVtable {
    fn load(get: &dyn Fn(&str) -> *const c_void) -> anyhow::Result<Self> {
        macro_rules! sym {
            ($name:expr, $ty:ty) => {{
                let p = get($name);
                if p.is_null() {
                    anyhow::bail!("GL symbol '{}' not found", $name);
                }
                // SAFETY: we verify the pointer is non-null before transmuting to fn ptr
                unsafe { std::mem::transmute::<*const c_void, $ty>(p) }
            }};
        }
        Ok(Self {
            gen_framebuffers: sym!("glGenFramebuffers", GenFn),
            bind_framebuffer: sym!("glBindFramebuffer", BindFn),
            delete_framebuffers: sym!("glDeleteFramebuffers", DeleteFn),
            gen_textures: sym!("glGenTextures", GenFn),
            bind_texture: sym!("glBindTexture", BindFn),
            delete_textures: sym!("glDeleteTextures", DeleteFn),
            tex_image_2d: sym!("glTexImage2D", TexImage2DFn),
            tex_parameteri: sym!("glTexParameteri", TexParameteriFn),
            framebuffer_texture_2d: sym!("glFramebufferTexture2D", FbTexture2DFn),
            gen_renderbuffers: sym!("glGenRenderbuffers", GenFn),
            bind_renderbuffer: sym!("glBindRenderbuffer", BindFn),
            delete_renderbuffers: sym!("glDeleteRenderbuffers", DeleteFn),
            renderbuffer_storage: sym!("glRenderbufferStorage", RbStorageFn),
            framebuffer_renderbuffer: sym!("glFramebufferRenderbuffer", FbRbFn),
            check_framebuffer_status: sym!("glCheckFramebufferStatus", CheckFbFn),
            read_pixels: sym!("glReadPixels", ReadPixelsFn),
            #[cfg(target_os = "linux")]
            gen_buffers: sym!("glGenBuffers", GenFn),
            #[cfg(target_os = "linux")]
            bind_buffer: sym!("glBindBuffer", BindFn),
            #[cfg(target_os = "linux")]
            buffer_data: sym!("glBufferData", BufferDataFn),
            #[cfg(target_os = "linux")]
            map_buffer: sym!("glMapBuffer", MapBufferFn),
            #[cfg(target_os = "linux")]
            unmap_buffer: sym!("glUnmapBuffer", UnmapBufferFn),
            #[cfg(target_os = "linux")]
            delete_buffers: sym!("glDeleteBuffers", DeleteFn),
        })
    }
}

pub struct HwGlContext {
    platform: Platform,
    gl: GlVtable,
    fbo: u32,
    texture: u32,
    depth_rb: u32,
    fbo_width: u32,
    fbo_height: u32,
    #[cfg(target_os = "linux")]
    pbo: [u32; 2],
    #[cfg(target_os = "linux")]
    pbo_index: std::cell::Cell<usize>,
    #[cfg(target_os = "linux")]
    pbo_primed: std::cell::Cell<bool>,
}

impl HwGlContext {
    pub fn create(width: u32, height: u32) -> anyhow::Result<Self> {
        let platform = Platform::create()?;
        platform.make_current();
        let gl = GlVtable::load(&|name| platform.get_proc_address(name))?;
        let mut ctx = Self {
            platform,
            gl,
            fbo: 0,
            texture: 0,
            depth_rb: 0,
            fbo_width: 0,
            fbo_height: 0,
            #[cfg(target_os = "linux")]
            pbo: [0, 0],
            #[cfg(target_os = "linux")]
            pbo_index: std::cell::Cell::new(0),
            #[cfg(target_os = "linux")]
            pbo_primed: std::cell::Cell::new(false),
        };
        ctx.alloc_fbo(width, height)?;
        Ok(ctx)
    }

    fn alloc_fbo(&mut self, width: u32, height: u32) -> anyhow::Result<()> {
        self.platform.make_current();
        unsafe {
            (self.gl.gen_framebuffers)(1, &mut self.fbo);
            (self.gl.bind_framebuffer)(gl::FRAMEBUFFER, self.fbo);

            (self.gl.gen_textures)(1, &mut self.texture);
            (self.gl.bind_texture)(gl::TEXTURE_2D, self.texture);
            (self.gl.tex_image_2d)(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                width as i32,
                height as i32,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                std::ptr::null(),
            );
            (self.gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            (self.gl.tex_parameteri)(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            (self.gl.framebuffer_texture_2d)(
                gl::FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                self.texture,
                0,
            );

            (self.gl.gen_renderbuffers)(1, &mut self.depth_rb);
            (self.gl.bind_renderbuffer)(gl::RENDERBUFFER, self.depth_rb);
            (self.gl.renderbuffer_storage)(
                gl::RENDERBUFFER,
                gl::DEPTH_COMPONENT24,
                width as i32,
                height as i32,
            );
            (self.gl.framebuffer_renderbuffer)(
                gl::FRAMEBUFFER,
                gl::DEPTH_ATTACHMENT,
                gl::RENDERBUFFER,
                self.depth_rb,
            );

            let status = (self.gl.check_framebuffer_status)(gl::FRAMEBUFFER);
            if status != gl::FRAMEBUFFER_COMPLETE {
                anyhow::bail!("GL FBO incomplete: 0x{:x}", status);
            }
        }
        self.fbo_width = width;
        self.fbo_height = height;
        #[cfg(target_os = "linux")]
        unsafe {
            let size = (width as usize) * (height as usize) * 4;
            (self.gl.gen_buffers)(2, self.pbo.as_mut_ptr());
            for &pbo in &self.pbo {
                (self.gl.bind_buffer)(gl::PIXEL_PACK_BUFFER, pbo);
                (self.gl.buffer_data)(
                    gl::PIXEL_PACK_BUFFER,
                    size as isize,
                    std::ptr::null(),
                    gl::STREAM_READ,
                );
            }
            (self.gl.bind_buffer)(gl::PIXEL_PACK_BUFFER, 0);
            self.pbo_index.set(0);
            self.pbo_primed.set(false);
        }
        Ok(())
    }
}

impl HwContextProvider for HwGlContext {
    fn get_proc_address(&self, sym: &str) -> *const c_void {
        self.platform.get_proc_address(sym)
    }

    fn get_current_framebuffer(&self) -> usize {
        self.fbo as usize
    }

    fn supports_context_type(&self, ctx_type: u32) -> bool {
        matches!(
            ctx_type,
            RETRO_HW_CONTEXT_OPENGL
                | RETRO_HW_CONTEXT_OPENGL_CORE
                | RETRO_HW_CONTEXT_OPENGLES2
                | RETRO_HW_CONTEXT_OPENGLES3
        )
    }

    fn make_current(&self) {
        self.platform.make_current();
    }

    #[cfg(not(target_os = "linux"))]
    fn readback_bgra(&self, width: u32, height: u32) -> Vec<u8> {
        let w = width.min(self.fbo_width);
        let h = height.min(self.fbo_height);
        let stride = (w * 4) as usize;
        let mut pixels = vec![0u8; stride * h as usize];

        self.platform.make_current();
        unsafe {
            (self.gl.bind_framebuffer)(gl::READ_FRAMEBUFFER, self.fbo);
            (self.gl.read_pixels)(
                0,
                0,
                w as i32,
                h as i32,
                gl::BGRA,
                gl::UNSIGNED_BYTE,
                pixels.as_mut_ptr() as *mut c_void,
            );
        }
        flip_rows(&mut pixels, stride, h as usize);
        pixels
    }

    #[cfg(target_os = "linux")]
    fn readback_bgra(&self, width: u32, height: u32) -> Vec<u8> {
        let w = width.min(self.fbo_width);
        let h = height.min(self.fbo_height);
        let stride = (w * 4) as usize;
        let size = stride * h as usize;
        let mut pixels = vec![0u8; size];

        self.platform.make_current();
        let cur = self.pbo_index.get();
        let prev = 1 - cur;
        unsafe {
            (self.gl.bind_framebuffer)(gl::READ_FRAMEBUFFER, self.fbo);
            (self.gl.bind_buffer)(gl::PIXEL_PACK_BUFFER, self.pbo[cur]);
            (self.gl.read_pixels)(
                0,
                0,
                w as i32,
                h as i32,
                gl::BGRA,
                gl::UNSIGNED_BYTE,
                std::ptr::null_mut(),
            );
            if self.pbo_primed.get() {
                (self.gl.bind_buffer)(gl::PIXEL_PACK_BUFFER, self.pbo[prev]);
                let ptr = (self.gl.map_buffer)(gl::PIXEL_PACK_BUFFER, gl::READ_ONLY) as *const u8;
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(ptr, pixels.as_mut_ptr(), size);
                    (self.gl.unmap_buffer)(gl::PIXEL_PACK_BUFFER);
                }
                flip_rows(&mut pixels, stride, h as usize);
            } else {
                self.pbo_primed.set(true);
            }
            (self.gl.bind_buffer)(gl::PIXEL_PACK_BUFFER, 0);
        }
        self.pbo_index.set(prev);
        pixels
    }
}

fn flip_rows(pixels: &mut [u8], stride: usize, rows: usize) {
    for y in 0..rows / 2 {
        let (a, b) = pixels.split_at_mut((rows - 1 - y) * stride);
        a[y * stride..(y + 1) * stride].swap_with_slice(&mut b[..stride]);
    }
}

impl Drop for HwGlContext {
    fn drop(&mut self) {
        self.platform.make_current();
        unsafe {
            if self.depth_rb != 0 {
                (self.gl.delete_renderbuffers)(1, &self.depth_rb);
            }
            if self.texture != 0 {
                (self.gl.delete_textures)(1, &self.texture);
            }
            if self.fbo != 0 {
                (self.gl.delete_framebuffers)(1, &self.fbo);
            }
            #[cfg(target_os = "linux")]
            if self.pbo[0] != 0 {
                (self.gl.delete_buffers)(2, self.pbo.as_ptr());
            }
        }
    }
}

#[cfg(target_os = "macos")]
#[allow(non_upper_case_globals)]
mod platform_impl {
    use std::ffi::{c_void, CString};

    #[link(name = "OpenGL", kind = "framework")]
    extern "C" {
        fn CGLChoosePixelFormat(attribs: *const i32, pix: *mut *mut c_void, npix: *mut i32) -> i32;
        fn CGLCreateContext(pix: *mut c_void, share: *mut c_void, ctx: *mut *mut c_void) -> i32;
        fn CGLSetCurrentContext(ctx: *mut c_void) -> i32;
        fn CGLDestroyContext(ctx: *mut c_void) -> i32;
        fn CGLDestroyPixelFormat(pix: *mut c_void) -> i32;
    }

    extern "C" {
        fn dlopen(path: *const i8, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, sym: *const i8) -> *mut c_void;
    }
    const RTLD_NOW: i32 = 2;

    // kCGLPFAOpenGLProfile = 99, kCGLOGLPVersion_3_2_Core = 0x3200
    const KCGLPFAOpenGLProfile: i32 = 99;
    const KCGLOGLPVersion_3_2_Core: i32 = 0x3200;
    const KCGLPFAAccelerated: i32 = 73;
    const KCGLPFAAllowOfflineRenderers: i32 = 96;

    pub struct Platform {
        pix: *mut c_void,
        ctx: *mut c_void,
        gl_lib: *mut c_void,
    }

    // SAFETY: all GL/CGL calls happen on the thread that installed the context.
    unsafe impl Send for Platform {}

    impl Platform {
        pub fn create() -> anyhow::Result<Self> {
            let attribs: [i32; 5] = [
                KCGLPFAOpenGLProfile,
                KCGLOGLPVersion_3_2_Core,
                KCGLPFAAccelerated,
                KCGLPFAAllowOfflineRenderers,
                0,
            ];
            let mut pix: *mut c_void = std::ptr::null_mut();
            let mut npix: i32 = 0;
            let err = unsafe { CGLChoosePixelFormat(attribs.as_ptr(), &mut pix, &mut npix) };
            if err != 0 || pix.is_null() {
                anyhow::bail!("CGLChoosePixelFormat failed: {err}");
            }
            let mut ctx: *mut c_void = std::ptr::null_mut();
            let err = unsafe { CGLCreateContext(pix, std::ptr::null_mut(), &mut ctx) };
            if err != 0 || ctx.is_null() {
                unsafe { CGLDestroyPixelFormat(pix) };
                anyhow::bail!("CGLCreateContext failed: {err}");
            }
            let err = unsafe { CGLSetCurrentContext(ctx) };
            if err != 0 {
                unsafe { CGLDestroyContext(ctx) };
                unsafe { CGLDestroyPixelFormat(pix) };
                anyhow::bail!("CGLSetCurrentContext failed: {err}");
            }
            let path = c"/System/Library/Frameworks/OpenGL.framework/OpenGL";
            let gl_lib = unsafe { dlopen(path.as_ptr().cast(), RTLD_NOW) };
            if gl_lib.is_null() {
                unsafe { CGLDestroyContext(ctx) };
                unsafe { CGLDestroyPixelFormat(pix) };
                anyhow::bail!("dlopen OpenGL.framework failed");
            }
            Ok(Self { pix, ctx, gl_lib })
        }

        pub fn make_current(&self) {
            unsafe { CGLSetCurrentContext(self.ctx) };
        }

        pub fn get_proc_address(&self, name: &str) -> *const c_void {
            let Ok(c_name) = CString::new(name) else {
                return std::ptr::null();
            };
            unsafe { dlsym(self.gl_lib, c_name.as_ptr().cast()) as *const c_void }
        }
    }

    impl Drop for Platform {
        fn drop(&mut self) {
            unsafe {
                CGLSetCurrentContext(std::ptr::null_mut());
                CGLDestroyContext(self.ctx);
                CGLDestroyPixelFormat(self.pix);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform_impl {
    use std::ffi::{c_void, CString};

    const EGL_DEFAULT_DISPLAY: *mut c_void = std::ptr::null_mut();
    const EGL_NO_CONTEXT: *mut c_void = std::ptr::null_mut();
    const EGL_NO_SURFACE: *mut c_void = std::ptr::null_mut();
    const EGL_TRUE: u32 = 1;
    const EGL_SURFACE_TYPE: i32 = 0x3033;
    const EGL_PBUFFER_BIT: i32 = 0x0001;
    const EGL_RENDERABLE_TYPE: i32 = 0x3040;
    const EGL_OPENGL_BIT: i32 = 0x0008;
    const EGL_NONE: i32 = 0x3038;
    const EGL_OPENGL_API: u32 = 0x30A2;
    const EGL_CONTEXT_MAJOR_VERSION: i32 = 0x3098;
    const EGL_CONTEXT_MINOR_VERSION: i32 = 0x30FB;
    const EGL_CONTEXT_OPENGL_PROFILE_MASK: i32 = 0x30FD;
    const EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT: i32 = 0x0001;
    const EGL_WIDTH: i32 = 0x3057;
    const EGL_HEIGHT: i32 = 0x3056;

    #[link(name = "EGL")]
    extern "C" {
        fn eglGetDisplay(native_display: *mut c_void) -> *mut c_void;
        fn eglInitialize(dpy: *mut c_void, major: *mut i32, minor: *mut i32) -> u32;
        fn eglBindAPI(api: u32) -> u32;
        fn eglChooseConfig(
            dpy: *mut c_void,
            attrib_list: *const i32,
            configs: *mut *mut c_void,
            config_size: i32,
            num_config: *mut i32,
        ) -> u32;
        fn eglCreateContext(
            dpy: *mut c_void,
            config: *mut c_void,
            share: *mut c_void,
            attrib_list: *const i32,
        ) -> *mut c_void;
        fn eglCreatePbufferSurface(
            dpy: *mut c_void,
            config: *mut c_void,
            attrib_list: *const i32,
        ) -> *mut c_void;
        fn eglMakeCurrent(
            dpy: *mut c_void,
            draw: *mut c_void,
            read: *mut c_void,
            ctx: *mut c_void,
        ) -> u32;
        fn eglGetProcAddress(procname: *const i8) -> *const c_void;
        fn eglDestroyContext(dpy: *mut c_void, ctx: *mut c_void) -> u32;
        fn eglDestroySurface(dpy: *mut c_void, surface: *mut c_void) -> u32;
        fn eglTerminate(dpy: *mut c_void) -> u32;
    }

    pub struct Platform {
        display: *mut c_void,
        context: *mut c_void,
        surface: *mut c_void,
    }

    unsafe impl Send for Platform {}

    impl Platform {
        pub fn create() -> anyhow::Result<Self> {
            let display = unsafe { eglGetDisplay(EGL_DEFAULT_DISPLAY) };
            if display.is_null() {
                anyhow::bail!("eglGetDisplay failed");
            }
            let (mut major, mut minor) = (0i32, 0i32);
            if unsafe { eglInitialize(display, &mut major, &mut minor) } != EGL_TRUE {
                anyhow::bail!("eglInitialize failed");
            }
            if unsafe { eglBindAPI(EGL_OPENGL_API) } != EGL_TRUE {
                anyhow::bail!("eglBindAPI(EGL_OPENGL_API) failed");
            }
            let cfg_attribs = [
                EGL_SURFACE_TYPE,
                EGL_PBUFFER_BIT,
                EGL_RENDERABLE_TYPE,
                EGL_OPENGL_BIT,
                EGL_NONE,
            ];
            let mut config: *mut c_void = std::ptr::null_mut();
            let mut num_config = 0i32;
            if unsafe {
                eglChooseConfig(
                    display,
                    cfg_attribs.as_ptr(),
                    &mut config,
                    1,
                    &mut num_config,
                )
            } != EGL_TRUE
                || num_config == 0
            {
                anyhow::bail!("eglChooseConfig failed");
            }
            let ctx_attribs = [
                EGL_CONTEXT_MAJOR_VERSION,
                3,
                EGL_CONTEXT_MINOR_VERSION,
                1,
                EGL_CONTEXT_OPENGL_PROFILE_MASK,
                EGL_CONTEXT_OPENGL_CORE_PROFILE_BIT,
                EGL_NONE,
            ];
            let context =
                unsafe { eglCreateContext(display, config, EGL_NO_CONTEXT, ctx_attribs.as_ptr()) };
            if context == EGL_NO_CONTEXT {
                anyhow::bail!("eglCreateContext failed");
            }
            let pb_attribs = [EGL_WIDTH, 1, EGL_HEIGHT, 1, EGL_NONE];
            let surface = unsafe { eglCreatePbufferSurface(display, config, pb_attribs.as_ptr()) };
            if surface == EGL_NO_SURFACE {
                unsafe { eglDestroyContext(display, context) };
                anyhow::bail!("eglCreatePbufferSurface failed");
            }
            if unsafe { eglMakeCurrent(display, surface, surface, context) } != EGL_TRUE {
                anyhow::bail!("initial eglMakeCurrent failed");
            }
            Ok(Self {
                display,
                context,
                surface,
            })
        }

        pub fn make_current(&self) {
            unsafe {
                eglMakeCurrent(self.display, self.surface, self.surface, self.context);
            }
        }

        pub fn get_proc_address(&self, name: &str) -> *const c_void {
            let Ok(c_name) = CString::new(name) else {
                return std::ptr::null();
            };
            unsafe { eglGetProcAddress(c_name.as_ptr().cast()) }
        }
    }

    impl Drop for Platform {
        fn drop(&mut self) {
            unsafe {
                eglMakeCurrent(self.display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
                eglDestroyContext(self.display, self.context);
                eglDestroySurface(self.display, self.surface);
                eglTerminate(self.display);
            }
        }
    }
}

#[cfg(windows)]
mod platform_impl {
    use std::ffi::{c_void, CString};
    use windows_sys::Win32::Foundation::{HMODULE, HWND};
    use windows_sys::Win32::Graphics::Gdi::{GetDC, ReleaseDC, HDC};
    use windows_sys::Win32::Graphics::OpenGL::{
        wglCreateContext, wglDeleteContext, wglGetProcAddress, wglMakeCurrent, ChoosePixelFormat,
        SetPixelFormat, HGLRC, PFD_DOUBLEBUFFER, PFD_DRAW_TO_WINDOW, PFD_MAIN_PLANE,
        PFD_SUPPORT_OPENGL, PFD_TYPE_RGBA, PIXELFORMATDESCRIPTOR,
    };
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow};

    pub struct Platform {
        window: HWND,
        dc: HDC,
        ctx: HGLRC,
        gl_lib: HMODULE,
    }

    // SAFETY: all GL/WGL calls happen on the thread that installed the context.
    unsafe impl Send for Platform {}

    impl Platform {
        pub fn create() -> anyhow::Result<Self> {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let window = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    std::ptr::null(),
                    0,
                    0,
                    0,
                    1,
                    1,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            };
            if window.is_null() {
                anyhow::bail!(
                    "CreateWindowExW failed: {}",
                    std::io::Error::last_os_error()
                );
            }
            let dc = unsafe { GetDC(window) };
            let mut platform = Self {
                window,
                dc,
                ctx: std::ptr::null_mut(),
                gl_lib: std::ptr::null_mut(),
            };
            let pfd = PIXELFORMATDESCRIPTOR {
                nSize: std::mem::size_of::<PIXELFORMATDESCRIPTOR>() as u16,
                nVersion: 1,
                dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
                iPixelType: PFD_TYPE_RGBA,
                cColorBits: 32,
                cAlphaBits: 8,
                cDepthBits: 24,
                cStencilBits: 8,
                iLayerType: PFD_MAIN_PLANE as u8,
                // SAFETY: PIXELFORMATDESCRIPTOR is plain data; zero is valid for the remaining fields.
                ..unsafe { std::mem::zeroed() }
            };
            let format = unsafe { ChoosePixelFormat(dc, &pfd) };
            if format == 0 || unsafe { SetPixelFormat(dc, format, &pfd) } == 0 {
                anyhow::bail!(
                    "no OpenGL pixel format: {}",
                    std::io::Error::last_os_error()
                );
            }
            platform.ctx = unsafe { wglCreateContext(dc) };
            if platform.ctx.is_null() {
                anyhow::bail!(
                    "wglCreateContext failed: {}",
                    std::io::Error::last_os_error()
                );
            }
            if unsafe { wglMakeCurrent(dc, platform.ctx) } == 0 {
                anyhow::bail!("wglMakeCurrent failed: {}", std::io::Error::last_os_error());
            }
            platform.gl_lib = unsafe { LoadLibraryA(c"opengl32.dll".as_ptr().cast()) };
            if platform.gl_lib.is_null() {
                anyhow::bail!("could not load opengl32.dll");
            }
            Ok(platform)
        }

        pub fn make_current(&self) {
            unsafe { wglMakeCurrent(self.dc, self.ctx) };
        }

        pub fn get_proc_address(&self, name: &str) -> *const c_void {
            let Ok(c_name) = CString::new(name) else {
                return std::ptr::null();
            };
            // wglGetProcAddress only knows extension and post-1.1 functions, and may return small sentinels.
            let proc = unsafe { wglGetProcAddress(c_name.as_ptr().cast()) }
                .map(|f| f as usize)
                .filter(|&addr| !matches!(addr as isize, -1..=3));
            match proc {
                Some(addr) => addr as *const c_void,
                None => unsafe { GetProcAddress(self.gl_lib, c_name.as_ptr().cast()) }
                    .map_or(std::ptr::null(), |f| f as *const c_void),
            }
        }
    }

    impl Drop for Platform {
        fn drop(&mut self) {
            unsafe {
                if !self.ctx.is_null() {
                    wglMakeCurrent(std::ptr::null_mut(), std::ptr::null_mut());
                    wglDeleteContext(self.ctx);
                }
                ReleaseDC(self.window, self.dc);
                DestroyWindow(self.window);
            }
        }
    }
}

use platform_impl::Platform;

pub const FBO_WIDTH: u32 = 1920;
pub const FBO_HEIGHT: u32 = 1080;

pub fn hw_frame_size(width: u32, height: u32) -> (u32, u32) {
    (width.min(FBO_WIDTH), height.min(FBO_HEIGHT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hw_frame_size_is_clamped_to_the_fbo() {
        assert_eq!(hw_frame_size(640, 480), (640, 480));
        assert_eq!(hw_frame_size(2560, 1440), (FBO_WIDTH, FBO_HEIGHT));
        assert_eq!(hw_frame_size(1920, 1088), (1920, FBO_HEIGHT));
    }
}
