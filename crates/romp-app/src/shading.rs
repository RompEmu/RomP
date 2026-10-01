use glow::HasContext;
use librashader::presets::ShaderFeatures;
use librashader::runtime::gl::{FilterChain, GLImage};
use librashader::runtime::{FilterChainParameters, Size, Viewport};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One RetroArch shader preset the picture goes through, with its settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub preset: PathBuf,
    pub params: Vec<(String, f32)>,
    /// Whether it draws at the size the game fills in the window, rather than the game's own size.
    pub to_screen: bool,
}

type Target = (glow::Texture, u32, u32);

/// The game's picture drawn through RetroArch shader presets, one after another, into a texture
/// the window shows.
#[derive(Default)]
pub struct Shading {
    stages: Vec<Stage>,
    gl: Option<Arc<glow::Context>>,
    chains: std::collections::HashMap<PathBuf, Option<FilterChain>>,
    input: Option<Target>,
    targets: Vec<Option<Target>>,
    opaque_fbo: Option<glow::Framebuffer>,
    frame: Vec<u8>,
    frame_size: (u32, u32),
    frame_dirty: bool,
    frame_count: usize,
    aspect: f32,
    failed: bool,
    redraw: bool,
    shown: bool,
    settling: u32,
}

/// How many more times the shaders run after the picture last changed. Ghosting and afterglow
/// build up over frames, and a still screen sends no new ones.
const SETTLE_FRAMES: u32 = 60;

/// A texture the shader drew, for the window to show in place of the plain picture.
pub struct Drawn {
    pub texture: std::num::NonZeroU32,
    pub width: u32,
    pub height: u32,
}

/// The size the game fills in the window, kept at its shape, in pixels.
pub fn game_size(window: (u32, u32), aspect: f32) -> (u32, u32) {
    let (ww, wh) = (window.0 as f32, window.1 as f32);
    let aspect = if aspect > 0.0 { aspect } else { 4.0 / 3.0 };
    let width = ww.min(wh * aspect);
    let height = width / aspect;
    (
        width.round().max(1.0) as u32,
        height.round().max(1.0) as u32,
    )
}

/// The texture units a shader pass may bind its inputs and samplers to.
const TEXTURE_UNITS: u32 = 16;

/// The OpenGL state the window's own drawing relies on, put back after the shader runs.
struct SavedState {
    program: Option<glow::Program>,
    vertex_array: Option<glow::VertexArray>,
    array_buffer: Option<glow::Buffer>,
    units: Vec<(Option<glow::Texture>, Option<glow::Sampler>)>,
    active_texture: u32,
    read_framebuffer: Option<glow::Framebuffer>,
    draw_framebuffer: Option<glow::Framebuffer>,
    unpack_alignment: i32,
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
                units: (0..TEXTURE_UNITS)
                    .map(|unit| {
                        gl.active_texture(glow::TEXTURE0 + unit);
                        (
                            std::num::NonZeroU32::new(
                                gl.get_parameter_i32(glow::TEXTURE_BINDING_2D) as u32,
                            )
                            .map(glow::NativeTexture),
                            std::num::NonZeroU32::new(
                                gl.get_parameter_i32(glow::SAMPLER_BINDING) as u32
                            )
                            .map(glow::NativeSampler),
                        )
                    })
                    .collect(),
                read_framebuffer: framebuffer_binding(gl, glow::READ_FRAMEBUFFER_BINDING),
                draw_framebuffer: framebuffer_binding(gl, glow::DRAW_FRAMEBUFFER_BINDING),
                unpack_alignment: gl.get_parameter_i32(glow::UNPACK_ALIGNMENT),
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
            for (unit, (texture, sampler)) in (0..).zip(&self.units) {
                gl.active_texture(glow::TEXTURE0 + unit);
                gl.bind_texture(glow::TEXTURE_2D, *texture);
                gl.bind_sampler(unit, *sampler);
            }
            gl.active_texture(self.active_texture);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.read_framebuffer);
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.draw_framebuffer);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, self.unpack_alignment);
            let [x, y, w, h] = self.viewport;
            gl.viewport(x, y, w, h);
            let toggle = |cap, on| if on { gl.enable(cap) } else { gl.disable(cap) };
            toggle(glow::BLEND, self.blend);
            toggle(glow::SCISSOR_TEST, self.scissor);
        }
    }
}

impl Shading {
    /// Draws the picture through `stages`, or plainly when there are none.
    pub fn set_stages(&mut self, stages: Vec<Stage>) {
        let presets =
            |stages: &[Stage]| stages.iter().map(|s| s.preset.clone()).collect::<Vec<_>>();
        if presets(&self.stages) != presets(&stages) {
            self.failed = false;
            self.shown = false;
        }
        self.stages = stages;
        self.redraw = true;
    }

    /// Whether the shaders still need to run on the same picture, so the window should redraw.
    pub fn settling(&self) -> bool {
        self.active() && self.settling > 0
    }

    /// Whether the shaders are drawing the game, so the window should not show the plain picture.
    pub fn active(&self) -> bool {
        !self.stages.is_empty() && !self.failed
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

    /// The last frame from the game, for showing it plainly once the shaders are off.
    pub fn frame(&self) -> Option<(&[u8], u32, u32, f32)> {
        let (width, height) = self.frame_size;
        (width > 0).then_some((&self.frame, width, height, self.aspect))
    }

    pub fn setup(&mut self, get_proc_address: &dyn Fn(&std::ffi::CStr) -> *const std::ffi::c_void) {
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| get_proc_address(name)) };
        self.gl = Some(Arc::new(gl));
    }

    pub fn teardown(&mut self) {
        self.chains.clear();
        if let Some(gl) = &self.gl {
            unsafe {
                for (t, ..) in self
                    .input
                    .take()
                    .into_iter()
                    .chain(self.targets.drain(..).flatten())
                {
                    gl.delete_texture(t);
                }
                if let Some(fbo) = self.opaque_fbo.take() {
                    gl.delete_framebuffer(fbo);
                }
            }
        }
        self.gl = None;
        self.shown = false;
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

    /// Keeps `slot` a texture of the given size; returns whether it had to be made anew.
    unsafe fn ensure(
        gl: &glow::Context,
        slot: &mut Option<Target>,
        width: u32,
        height: u32,
    ) -> bool {
        if slot.is_some_and(|(_, w, h)| (w, h) == (width, height)) {
            return false;
        }
        unsafe {
            if let Some((t, ..)) = slot.take() {
                gl.delete_texture(t);
            }
            *slot = Some((Self::texture(gl, width, height), width, height));
        }
        true
    }

    /// Sets the finished picture's alpha to opaque. Shaders written for RetroArch may leave it
    /// partly transparent, which RetroArch ignores but the window would blend with its background.
    unsafe fn make_opaque(&mut self, gl: &glow::Context, texture: glow::Texture) {
        unsafe {
            let fbo = *self
                .opaque_fbo
                .get_or_insert_with(|| gl.create_framebuffer().expect("framebuffer"));
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(fbo));
            gl.framebuffer_texture_2d(
                glow::DRAW_FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );
            gl.disable(glow::SCISSOR_TEST);
            gl.color_mask(false, false, false, true);
            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.color_mask(true, true, true, true);
        }
    }

    /// Runs the shaders on the latest frame for the window about to render. Returns the texture
    /// when the window must be told to show it, and None when it already does or nothing changed.
    pub fn draw(&mut self, window: (u32, u32)) -> Option<Drawn> {
        let gl = self.gl.clone()?;
        if self.stages.is_empty() || self.failed || self.frame_size.0 == 0 {
            return None;
        }
        let screen = game_size(window, self.aspect);
        let (fw, fh) = self.frame_size;
        unsafe {
            let saved = SavedState::save(&gl);
            let stages = self.stages.clone();
            if !stages.iter().all(|stage| self.load(&gl, &stage.preset)) {
                saved.restore(&gl);
                self.failed = true;
                return None;
            }
            let mut changed = Self::ensure(&gl, &mut self.input, fw, fh);
            let input = self.input.expect("input texture").0;
            if std::mem::take(&mut self.frame_dirty) || changed {
                changed = true;
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
            let keep = stages.len().min(self.targets.len());
            for (texture, ..) in self.targets.drain(keep..).flatten() {
                gl.delete_texture(texture);
            }
            self.targets.resize(stages.len(), None);
            let mut size = (fw, fh);
            for (i, stage) in stages.iter().enumerate() {
                if stage.to_screen {
                    size = screen;
                }
                if Self::ensure(&gl, &mut self.targets[i], size.0, size.1) {
                    changed = true;
                    if i + 1 == stages.len() {
                        self.shown = false;
                    }
                }
            }
            let mut drawn = Ok(());
            if changed || std::mem::take(&mut self.redraw) {
                self.settling = SETTLE_FRAMES;
            }
            if self.settling > 0 {
                self.settling -= 1;
                self.frame_count = self.frame_count.wrapping_add(1);
                let mut source = (input, fw, fh);
                for (i, stage) in stages.iter().enumerate() {
                    let target = self.targets[i].expect("stage texture");
                    let chain = self
                        .chains
                        .get_mut(&stage.preset)
                        .and_then(Option::as_mut)
                        .expect("chain loaded");
                    for (name, value) in &stage.params {
                        chain.parameters().set_parameter_value(name, *value);
                    }
                    let image = |(handle, w, h): Target| GLImage {
                        handle: Some(handle),
                        format: glow::RGBA8,
                        size: Size::new(w, h),
                    };
                    let output = image(target);
                    let viewport = Viewport {
                        x: 0.0,
                        y: 0.0,
                        mvp: None,
                        output: &output,
                        size: output.size,
                    };
                    drawn = chain.frame(&image(source), &viewport, self.frame_count, None);
                    if let Err(e) = &drawn {
                        tracing::warn!("shader {} failed to draw: {e}", stage.preset.display());
                        break;
                    }
                    source = target;
                }
                if drawn.is_ok() {
                    let last = self
                        .targets
                        .last()
                        .copied()
                        .flatten()
                        .expect("stage texture");
                    self.make_opaque(&gl, last.0);
                }
            }
            saved.restore(&gl);
            if drawn.is_err() {
                self.failed = true;
                return None;
            }
            if std::mem::replace(&mut self.shown, true) {
                return None;
            }
            let (texture, width, height) = self.targets.last().copied().flatten()?;
            Some(Drawn {
                texture: texture.0,
                width,
                height,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_keeps_its_shape() {
        assert_eq!(game_size((1600, 900), 4.0 / 3.0), (1200, 900));
        assert_eq!(game_size((800, 1200), 4.0 / 3.0), (800, 600));
        assert_eq!(game_size((640, 480), 0.0), (640, 480));
    }
}
