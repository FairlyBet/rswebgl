//! HDR output: the scene renders linear radiance into a half-float target
//! (multisampled when the device allows), and a display pass turns it into
//! the canvas's colors — exposure, tone mapping, sRGB, dither.
//!
//! Shading in linear HDR and finishing once at the end is what keeps
//! blending physically right (transparent surfaces mix radiance, not
//! tone-mapped colors), leaves the tone mapper a free choice, and gives later
//! passes (screen-space refraction, post effects) the scene as light.
//!
//! The target holds premultiplied radiance and coverage, cleared to the
//! canvas's clear color, so a canvas with a transparent clear color still
//! composites over the page.

use rswebgl::context::Context;
use rswebgl::draw::{DrawCommand, DrawMode, Viewport};
use rswebgl::error::Error;
use rswebgl::framebuffer::{ClearMask, InvalidateMask};
use rswebgl::ids::{
    FramebufferId, ParamGroupId, ParamLayoutId, ProgramId, RenderbufferId, StateId, TextureId,
    VertexArrayId,
};
use rswebgl::param::{GROUP_SLOTS, ParamField, ParamLayoutBuilder};
use rswebgl::pass::Pass;
use rswebgl::render_state::RenderState;
use rswebgl::renderbuffer::RenderbufferFormat;
use rswebgl::shader_source::ShaderSource;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};
use rswebgl::uniform_buffer::UniformType;
use wasm_bindgen::prelude::*;

use crate::environment::FULLSCREEN_VERT;
use crate::shader::chunk;

const PRESENT_FRAG: &str = include_str!("../shaders/present.frag");

/// How linear radiance is squeezed into the display's range.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToneMapping {
    /// Exposure and a clamp: what fits shows exactly, the rest clips.
    None = 0,
    /// Khronos PBR Neutral: base colors stay true up to ~0.8, highlights
    /// roll off to white. The default; made for product rendering.
    #[default]
    Neutral = 1,
    /// ACES filmic (Hill's fit, three.js' exposure lift): contrasty, filmic.
    Aces = 2,
    /// AgX: gentle, desaturates highlights like film.
    Agx = 3,
}

impl ToneMapping {
    /// By name: "none", "neutral", "aces", "agx".
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "none" => Self::None,
            "neutral" => Self::Neutral,
            "aces" => Self::Aces,
            "agx" => Self::Agx,
            _ => return None,
        })
    }
}

/// The scene's HDR target (sized to the canvas on each frame) and the pass
/// that displays it.
pub struct HdrOutput {
    // What the scene draws into: multisampled renderbuffers, or the texture
    // directly when not multisampling.
    fb: FramebufferId,
    color_rb: RenderbufferId,
    depth_rb: RenderbufferId,
    // The resolved (or directly rendered) radiance the display pass reads.
    resolve_fb: FramebufferId,
    color: TextureId,
    size: (i32, i32),
    samples: i32,
    reverse_z: bool,
    program: ProgramId,
    layout: ParamLayoutId,
    group: ParamGroupId,
    f_exposure: ParamField,
    f_tone_mapping: ParamField,
    f_dither: ParamField,
    state: StateId,
    generation: u32,
}

impl HdrOutput {
    /// Whether this device can render into half-float color.
    pub fn supported(ctx: &Context) -> bool {
        ctx.can_render_to(&TextureFormat::rgba16f())
    }

    /// `samples` per pixel for antialiasing (0 or 1 for none; lowered to what
    /// the device allows).
    pub fn new(ctx: &Context, samples: i32) -> Result<Self, Error> {
        if !Self::supported(ctx) {
            return Err("HDR output: this device can't render to half-float targets".into());
        }
        let prefix = "#version 300 es\n";
        let vert = ShaderSource::plain("fullscreen.vert", &format!("{prefix}{FULLSCREEN_VERT}"));
        let frag = ShaderSource::assemble(prefix, &[], "present.frag", PRESENT_FRAG, chunk)?;
        let program = ctx.create_program_from(&vert, &frag)?;
        let layout = ctx.create_param_layout(
            &ParamLayoutBuilder::new()
                .texture("u_hdr")
                .uniform("u_exposure", UniformType::F32)
                .uniform("u_tone_mapping", UniformType::I32)
                .uniform("u_dither", UniformType::F32),
        )?;
        let group = ctx.create_param_group(layout)?;
        let mut state = RenderState::new();
        state.depth_test = false;
        state.depth_mask = false;
        Ok(Self {
            fb: FramebufferId::NULL,
            color_rb: RenderbufferId::NULL,
            depth_rb: RenderbufferId::NULL,
            resolve_fb: FramebufferId::NULL,
            color: TextureId::NULL,
            size: (0, 0),
            samples: samples.clamp(0, rswebgl::limits::max_samples() as i32),
            reverse_z: false,
            program,
            layout,
            group,
            f_exposure: ctx.param_field(layout, "u_exposure")?,
            f_tone_mapping: ctx.param_field(layout, "u_tone_mapping")?,
            f_dither: ctx.param_field(layout, "u_dither")?,
            state: ctx.render_state(&state),
            generation: ctx.generation(),
        })
    }

    /// Whether its GPU objects belong to an earlier context generation.
    pub fn is_lost(&self, ctx: &Context) -> bool {
        self.generation != ctx.generation()
    }

    /// The radiance the last frame left (resolved), for passes that read
    /// the scene; NULL before the first frame.
    pub fn color_texture(&self) -> TextureId {
        self.color
    }

    /// Samples per pixel actually used (0: no multisampling).
    pub fn samples(&self) -> i32 {
        self.samples
    }

    /// Points `pass` at the HDR target (cleared; resolved and discarded
    /// after drawing when multisampled), sized `w`×`h` (the canvas):
    /// (re)allocates the target when the size or depth mode changed.
    pub fn prepare(&mut self, ctx: &Context, w: i32, h: i32, pass: &mut Pass) -> Result<(), Error> {
        let (w, h) = (w.max(1), h.max(1));
        let reverse_z = ctx.reverse_z();
        if self.size != (w, h) || self.reverse_z != reverse_z || self.fb.is_null() {
            self.allocate(ctx, w, h, reverse_z)?;
        }
        // The canvas's clear color, as premultiplied linear radiance.
        let [r, g, b, a] = ctx.default_framebuffer().clear_color_rgba();
        let lin = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        ctx.framebuffer_set_color_clear_f32(self.fb, 0, lin(r) * a, lin(g) * a, lin(b) * a, a);
        pass.set_target(self.fb);
        pass.set_clear(&ClearMask::color_depth());
        let mut discard = InvalidateMask::none();
        // Depth isn't read after the frame; neither is the multisampled color
        // once resolved. Telling tiled (mobile) GPUs saves writing them back.
        discard.depth = true;
        if self.fb != self.resolve_fb {
            pass.set_resolve(self.resolve_fb);
            discard.color = true;
        } else {
            pass.clear_resolve();
        }
        pass.set_invalidate(&discard);
        Ok(())
    }

    /// Draws the target to the canvas.
    pub fn present(&self, ctx: &Context, exposure: f32, tone_mapping: ToneMapping, dither: bool) {
        ctx.param_set_texture_by_name(self.group, "u_hdr", self.color);
        ctx.param_set_f32(self.group, self.f_exposure, exposure);
        ctx.param_set_i32(self.group, self.f_tone_mapping, tone_mapping as i32);
        ctx.param_set_f32(self.group, self.f_dither, if dither { 1.0 } else { 0.0 });
        let mut pass = Pass::to_default();
        let fb = ctx.default_framebuffer();
        pass.set_viewport(&Viewport::new(0, 0, fb.width(), fb.height()));
        let mut groups = [ParamGroupId::NULL; GROUP_SLOTS];
        groups[0] = self.group;
        pass.push_groups_with_key(
            0,
            self.program,
            self.state,
            VertexArrayId::NULL,
            groups,
            &DrawCommand::arrays(DrawMode::Triangles, 0, 3),
        );
        ctx.render(&pass);
    }

    fn allocate(&mut self, ctx: &Context, w: i32, h: i32, reverse_z: bool) -> Result<(), Error> {
        self.free_target(ctx);
        self.size = (w, h);
        self.reverse_z = reverse_z;
        let depth = if reverse_z {
            RenderbufferFormat::Depth32f
        } else {
            RenderbufferFormat::Depth24
        };
        let color = ctx.create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )?;
        ctx.texture_storage_2d(color, 1, &TextureFormat::rgba16f(), w, h);
        self.color = color;
        let resolve_fb = ctx.create_framebuffer(w, h)?;
        ctx.framebuffer_attach_color_texture_2d(resolve_fb, 0, color, 0);
        self.resolve_fb = resolve_fb;

        // Multisampled if the device takes it for this format; else straight
        // into the texture.
        while self.samples > 1 {
            let fb = ctx.create_framebuffer(w, h)?;
            let color_rb = ctx.create_renderbuffer_multisample(
                RenderbufferFormat::Rgba16f,
                self.samples,
                w,
                h,
            )?;
            let depth_rb = ctx.create_renderbuffer_multisample(depth, self.samples, w, h)?;
            ctx.framebuffer_attach_color_renderbuffer(fb, 0, color_rb);
            ctx.framebuffer_attach_depth_renderbuffer(fb, depth_rb);
            if ctx.is_framebuffer_complete(fb) {
                (self.fb, self.color_rb, self.depth_rb) = (fb, color_rb, depth_rb);
                return Ok(());
            }
            ctx.destroy_framebuffer(fb);
            ctx.destroy_renderbuffer(color_rb);
            ctx.destroy_renderbuffer(depth_rb);
            self.samples /= 2;
        }
        self.samples = 0;
        let depth_rb = ctx.create_renderbuffer(depth, w, h)?;
        ctx.framebuffer_attach_depth_renderbuffer(resolve_fb, depth_rb);
        self.depth_rb = depth_rb;
        self.fb = resolve_fb;
        if !ctx.is_framebuffer_complete(resolve_fb) {
            return Err("HDR output: the half-float target is incomplete on this device".into());
        }
        Ok(())
    }

    fn free_target(&mut self, ctx: &Context) {
        if self.fb != self.resolve_fb && !self.fb.is_null() {
            ctx.destroy_framebuffer(self.fb);
        }
        if !self.resolve_fb.is_null() {
            ctx.destroy_framebuffer(self.resolve_fb);
        }
        for rb in [self.color_rb, self.depth_rb] {
            if !rb.is_null() {
                ctx.destroy_renderbuffer(rb);
            }
        }
        if !self.color.is_null() {
            ctx.destroy_texture(self.color);
        }
        self.fb = FramebufferId::NULL;
        self.resolve_fb = FramebufferId::NULL;
        self.color_rb = RenderbufferId::NULL;
        self.depth_rb = RenderbufferId::NULL;
        self.color = TextureId::NULL;
    }

    /// Frees it (after a context loss only the CPU-side group is left).
    pub fn destroy(mut self, ctx: &Context) {
        ctx.destroy_param_group(self.group);
        ctx.destroy_param_layout(self.layout);
        if self.generation != ctx.generation() {
            return;
        }
        self.free_target(ctx);
        ctx.destroy_program(self.program);
    }
}
