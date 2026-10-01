use glow::HasContext;
use librashader::presets::ShaderFeatures;
use librashader::runtime::gl::{FilterChain, GLImage};
use librashader::runtime::{FilterChainParameters, Size, Viewport};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The game's picture drawn through a RetroArch shader preset, under the window's own interface.
#[derive(Default)]
pub struct Shading {
    preset: Option<PathBuf>,
    params: Vec<(String, f32)>,
    gl: Option<Arc<glow::Context>>,
    chains: std::collections::HashMap<PathBuf, Option<FilterChain>>,
    input: Option<(glow::Texture, u32, u32)>,
    output: Option<(glow::Texture, u32, u32)>,
    read_fbo: Option<glow::Framebuffer>,
    frame: Vec<u8>,
    frame_size: (u32, u32),
    frame_dirty: bool,
    frame_count: usize,
    aspect: f32,
    failed: bool,
}

/// The rectangle the game fills, centred and kept at its shape, in pixels from the window's bottom left.
pub fn game_rect(window: (u32, u32), aspect: f32) -> (i32, i32, u32, u32) {
    let (ww, wh) = (window.0 as f32, window.1 as f32);
    let aspect = if aspect > 0.0 { aspect } else { 4.0 / 3.0 };
    let width = ww.min(wh * aspect);
    let height = width / aspect;
    let x = ((ww - width) / 2.0).round();
    let y = ((wh - height) / 2.0).round();
    (
        x as i32,
        y as i32,
        width.round().max(1.0) as u32,
        height.round().max(1.0) as u32,
    )
}

struct SavedState {
    program: Option<glow::Program>,
    vertex_array: Option<glow::VertexArray>,
    array_buffer: Option<glow::Buffer>,
    texture: Option<glow::Texture>,
    active_texture: u32,
    read_framebuffer: Option<glow::Framebuffer>,
    draw_framebuffer: Option<glow::Framebuffer>,
    viewport: [i32; 4],
    blend: bool,
    scissor: bool,
}

unsafe fn framebuffer_binding(gl: &glow::Context, which: u32) -> Option<glow::Framebuffer> {
    let id = unsafe { gl.get_parameter_i32(which) } as u32;
    std::num::NonZeroU32::new(id).map(glow::NativeFramebuffer)
}

impl SavedState {
    unsafe fn save(gl: &glow::Context) -> Self {
        unsafe {
            let mut viewport = [0; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            Self {
                program: std::num::NonZeroU32::new(
                    gl.get_parameter_i32(glow::CURRENT_PROGRAM) as u32
                )
                .map(glow::NativeProgram),
                vertex_array: std::num::NonZeroU32::new(
                    gl.get_parameter_i32(glow::VERTEX_ARRAY_BINDING) as u32,
                )
                .map(glow::NativeVertexArray),
                array_buffer: std::num::NonZeroU32::new(
                    gl.get_parameter_i32(glow::ARRAY_BUFFER_BINDING) as u32,
                )
                .map(glow::NativeBuffer),
                active_texture: gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32,
                texture: {
                    gl.active_texture(glow::TEXTURE0);
                    std::num::NonZeroU32::new(gl.get_parameter_i32(glow::TEXTURE_BINDING_2D) as u32)
                        .map(glow::NativeTexture)
                },
                read_framebuffer: framebuffer_binding(gl, glow::READ_FRAMEBUFFER_BINDING),
                draw_framebuffer: framebuffer_binding(gl, glow::DRAW_FRAMEBUFFER_BINDING),
                viewport,
                blend: gl.is_enabled(glow::BLEND),
                scissor: gl.is_enabled(glow::SCISSOR_TEST),
            }
        }
    }

    unsafe fn restore(&self, gl: &glow::Context) {
        unsafe {
            gl.use_program(self.program);
            gl.bind_vertex_array(self.vertex_array);
            gl.bind_buffer(glow::ARRAY_BUFFER, self.array_buffer);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, self.texture);
            gl.active_texture(self.active_texture);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.read_framebuffer);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.draw_framebuffer);
            let [x, y, w, h] = self.viewport;
            gl.viewport(x, y, w, h);
            let toggle = |cap, on| if on { gl.enable(cap) } else { gl.disable(cap) };
            toggle(glow::BLEND, self.blend);
            toggle(glow::SCISSOR_TEST, self.scissor);
        }
    }
}

impl Shading {
    /// Uses `preset`, or draws the plain picture when it's None.
    pub fn set_preset(&mut self, preset: Option<PathBuf>, params: Vec<(String, f32)>) {
        if self.preset != preset {
            self.failed = false;
        }
        self.preset = preset;
        self.params = params;
    }

    /// Whether the shader is drawing the game, so the window should not draw the plain picture.
    pub fn active(&self) -> bool {
        self.preset.is_some() && !self.failed
    }

    pub fn set_frame(&mut self, rgba: &[u8], width: u32, height: u32, aspect: f32) {
        let len = (width * height * 4) as usize;
        let Some(pixels) = rgba.get(..len) else {
            return;
        };
        self.frame.clear();
        self.frame.extend_from_slice(pixels);
        self.frame_size = (width, height);
        self.aspect = if aspect > 0.0 {
            aspect
        } else {
            width as f32 / height.max(1) as f32
        };
        self.frame_dirty = true;
    }

    pub fn setup(&mut self, get_proc_address: &dyn Fn(&std::ffi::CStr) -> *const std::ffi::c_void) {
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| get_proc_address(name)) };
        self.gl = Some(Arc::new(gl));
    }

    pub fn teardown(&mut self) {
        self.chains.clear();
        if let Some(gl) = &self.gl {
            unsafe {
                if let Some((t, ..)) = self.input.take() {
                    gl.delete_texture(t);
                }
                if let Some((t, ..)) = self.output.take() {
                    gl.delete_texture(t);
                }
                if let Some(f) = self.read_fbo.take() {
                    gl.delete_framebuffer(f);
                }
            }
        }
        self.gl = None;
    }

    /// Builds a preset once and keeps it, so switching back to a look is instant.
    fn load(&mut self, gl: &Arc<glow::Context>, preset: &Path) -> bool {
        self.chains
            .entry(preset.to_path_buf())
            .or_insert_with(|| {
                match unsafe {
                    FilterChain::load_from_path(preset, ShaderFeatures::NONE, gl.clone(), None)
                } {
                    Ok(chain) => Some(chain),
                    Err(e) => {
                        tracing::warn!("shader {} could not load: {e}", preset.display());
                        None
                    }
                }
            })
            .is_some()
    }

    unsafe fn clear_opaque(gl: &glow::Context) {
        unsafe {
            gl.disable(glow::SCISSOR_TEST);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
        }
    }

    unsafe fn texture(gl: &glow::Context, width: u32, height: u32) -> glow::Texture {
        unsafe {
            let texture = gl.create_texture().expect("texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_storage_2d(
                glow::TEXTURE_2D,
                1,
                glow::RGBA8,
                width as i32,
                height as i32,
            );
            texture
        }
    }

    /// Draws the game for the frame the window is about to render; returns false when it could not.
    pub fn draw(&mut self, window: (u32, u32)) -> bool {
        let (Some(gl), Some(preset)) = (self.gl.clone(), self.preset.clone()) else {
            return false;
        };
        if self.failed {
            return false;
        }
        if !self.load(&gl, &preset) {
            self.failed = true;
            return false;
        }
        if self.frame_size.0 == 0 {
            unsafe {
                let saved = SavedState::save(&gl);
                Self::clear_opaque(&gl);
                saved.restore(&gl);
            }
            return true;
        }
        let (x, y, width, height) = game_rect(window, self.aspect);
        unsafe {
            let saved = SavedState::save(&gl);
            Self::clear_opaque(&gl);

            let (fw, fh) = self.frame_size;
            if self.input.is_none_or(|(_, w, h)| (w, h) != (fw, fh)) {
                if let Some((t, ..)) = self.input.take() {
                    gl.delete_texture(t);
                }
                self.input = Some((Self::texture(&gl, fw, fh), fw, fh));
                self.frame_dirty = true;
            }
            let input = self.input.expect("input texture").0;
            if std::mem::take(&mut self.frame_dirty) {
                gl.bind_texture(glow::TEXTURE_2D, Some(input));
                gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    0,
                    0,
                    fw as i32,
                    fh as i32,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&self.frame)),
                );
            }
            if self
                .output
                .is_none_or(|(_, w, h)| (w, h) != (width, height))
            {
                if let Some((t, ..)) = self.output.take() {
                    gl.delete_texture(t);
                }
                self.output = Some((Self::texture(&gl, width, height), width, height));
            }
            let output = self.output.expect("output texture").0;
            let chain = self
                .chains
                .get_mut(&preset)
                .and_then(Option::as_mut)
                .expect("chain loaded");
            for (name, value) in &self.params {
                chain.parameters().set_parameter_value(name, *value);
            }
            let source = GLImage {
                handle: Some(input),
                format: glow::RGBA8,
                size: Size::new(fw, fh),
            };
            let target = GLImage {
                handle: Some(output),
                format: glow::RGBA8,
                size: Size::new(width, height),
            };
            let viewport = Viewport {
                x: 0.0,
                y: 0.0,
                mvp: None,
                output: &target,
                size: Size::new(width, height),
            };
            self.frame_count = self.frame_count.wrapping_add(1);
            let drawn = chain.frame(&source, &viewport, self.frame_count, None);
            if let Err(e) = &drawn {
                tracing::warn!("shader {} failed to draw: {e}", preset.display());
                self.failed = true;
            }
            if drawn.is_ok() {
                let read = *self
                    .read_fbo
                    .get_or_insert_with(|| gl.create_framebuffer().expect("framebuffer"));
                gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(read));
                gl.framebuffer_texture_2d(
                    glow::READ_FRAMEBUFFER,
                    glow::COLOR_ATTACHMENT0,
                    glow::TEXTURE_2D,
                    Some(output),
                    0,
                );
                gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, saved.draw_framebuffer);
                // The game's top row is the texture's first, which OpenGL counts from the bottom.
                gl.blit_framebuffer(
                    0,
                    0,
                    width as i32,
                    height as i32,
                    x,
                    y + height as i32,
                    x + width as i32,
                    y,
                    glow::COLOR_BUFFER_BIT,
                    glow::NEAREST,
                );
            }
            // Shaders may leave alpha at zero, which a see-through window would show the desktop through.
            gl.color_mask(false, false, false, true);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.color_mask(true, true, true, true);
            saved.restore(&gl);
            drawn.is_ok()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_is_centred_at_its_shape() {
        assert_eq!(game_rect((1600, 900), 4.0 / 3.0), (200, 0, 1200, 900));
        assert_eq!(game_rect((800, 1200), 4.0 / 3.0), (0, 300, 800, 600));
        assert_eq!(game_rect((640, 480), 0.0), (0, 0, 640, 480));
    }
}
