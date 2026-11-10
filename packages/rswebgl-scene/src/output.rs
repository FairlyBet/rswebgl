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
use rswebgl::profiler::GpuProfiler;
use rswebgl::render_state::RenderState;
use rswebgl::renderbuffer::RenderbufferFormat;
use rswebgl::shader_source::ShaderSource;
use rswebgl::texture::{
    TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget, TextureWrap,
};
use rswebgl::uniform_buffer::UniformType;
use wasm_bindgen::prelude::*;

use crate::environment::FULLSCREEN_VERT;
use crate::shader::chunk;

const PRESENT_FRAG: &str = include_str!("../shaders/present.frag");
const TAA_FRAG: &str = include_str!("../shaders/taa.frag");
const FXAA_FRAG: &str = include_str!("../shaders/fxaa.frag");
const GRAB_DOWN_FRAG: &str = include_str!("../shaders/grab_down.frag");
/// The velocity target's clear value: no motion vector here (taa.frag
/// reprojects the depth instead).
const VELOCITY_NONE: f32 = 1000.0;

/// Temporal antialiasing (ported from Bevy, see taa.frag): the scene renders
/// jittered without MSAA, its depth into a texture; each frame resolves
/// into the next of two history textures, which the display shows.
struct Taa {
    program: ProgramId,
    layout: ParamLayoutId,
    group: ParamGroupId,
    f_reproject: ParamField,
    f_taa: ParamField,
    f_jitter: ParamField,
    f_reactive: ParamField,
    // Sized with the target: (texture, framebuffer) each.
    history: [(TextureId, FramebufferId); 2],
    // The motion each history's resolve used, per pixel (its second color
    // attachment): the next resolve's velocity disocclusion.
    motions: [TextureId; 2],
    depth: TextureId,
    // Moving meshes' motion (uv now − uv last frame; elsewhere the clear
    // value, VELOCITY_NONE).
    velocity: (TextureId, FramebufferId),
    // The opaque scene alone, copied before transmissive and transparent
    // meshes draw (made on first need), and whether this frame copied it:
    // the resolve's reactive mask compares it with the final frame.
    opaque: (TextureId, FramebufferId),
    opaque_kept: bool,
    // The history the last resolve wrote; whether the next starts over.
    current: usize,
    reset: bool,
}

/// FXAA (ported from three.js, see fxaa.frag): the display pass draws into
/// an 8-bit texture the canvas's size, which this pass filters onto the
/// canvas.
struct Fxaa {
    program: ProgramId,
    layout: ParamLayoutId,
    group: ParamGroupId,
    f_texel: ParamField,
    // The display pass's output (made at the canvas's size on need).
    ldr: (TextureId, FramebufferId),
    size: (i32, i32),
}

impl Fxaa {
    fn free_target(&mut self, ctx: &Context) {
        let (t, fb) = std::mem::replace(&mut self.ldr, (TextureId::NULL, FramebufferId::NULL));
        if !fb.is_null() {
            ctx.destroy_framebuffer(fb);
        }
        if !t.is_null() {
            ctx.destroy_texture(t);
        }
        self.size = (0, 0);
    }

    // The display pass's target at `w`×`h` (remade when the canvas resized).
    fn ensure_target(&mut self, ctx: &Context, w: i32, h: i32) -> Result<FramebufferId, Error> {
        if self.size == (w, h) && !self.ldr.1.is_null() {
            return Ok(self.ldr.1);
        }
        self.free_target(ctx);
        let t = ctx.create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Linear,
            TextureMagFilter::Linear,
        )?;
        ctx.texture_storage_2d(t, 1, &TextureFormat::rgba8(), w, h);
        ctx.texture_set_wrap_s(t, TextureWrap::ClampToEdge);
        ctx.texture_set_wrap_t(t, TextureWrap::ClampToEdge);
        let fb = match ctx.create_framebuffer(w, h) {
            Ok(fb) => fb,
            Err(e) => {
                ctx.destroy_texture(t);
                return Err(e);
            }
        };
        ctx.framebuffer_attach_color_texture_2d(fb, 0, t, 0);
        self.ldr = (t, fb);
        self.size = (w, h);
        Ok(fb)
    }
}

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
    // A copy of the opaque scene with mips, for transmission (`grab_scale` of the size).
    grab_fb: FramebufferId,
    // Its level 1 (drawn by `grab_down`, firefly-suppressed).
    grab_fb1: FramebufferId,
    grab: TextureId,
    grab_size: (i32, i32),
    // The copy's share of the target's size (`set_transmission_scale`).
    grab_scale: f32,
    size: (i32, i32),
    samples: i32,
    reverse_z: bool,
    program: ProgramId,
    layout: ParamLayoutId,
    group: ParamGroupId,
    f_exposure: ParamField,
    f_tone_mapping: ParamField,
    f_dither: ParamField,
    f_sharpness: ParamField,
    // The scene copy's first mip: program, its layout and group.
    grab_down: (ProgramId, ParamLayoutId, ParamGroupId),
    state: StateId,
    generation: u32,
    // Depth peeling's targets, made on first use (`peel`).
    peel: Option<PeelTargets>,
    // Temporal antialiasing, when on (instead of multisampling).
    taa: Option<Taa>,
    // FXAA after the display pass, when on.
    fxaa: Option<Fxaa>,
}

/// Depth peeling (Everitt 2001): the opaque scene's depth, single-sampled,
/// then per layer a depth texture holding the k-th nearest transmissive
/// surface (or the opaque one where there is none). Transmissive layers are
/// shaded back to front into the resolved color, each seeing everything
/// behind it; transparent meshes then draw over the nearest layer.
struct PeelTargets {
    layers: Vec<(TextureId, FramebufferId)>,
    opaque_depth: TextureId,
    opaque_fb: FramebufferId,
    // The resolved color alone (layers shade into it), and with the
    // nearest layer's depth (transparent meshes test against it).
    glass_fb: FramebufferId,
    transparent_fb: FramebufferId,
}

impl PeelTargets {
    fn free(self, ctx: &Context) {
        for (t, fb) in self.layers {
            ctx.destroy_framebuffer(fb);
            ctx.destroy_texture(t);
        }
        for fb in [self.opaque_fb, self.glass_fb, self.transparent_fb] {
            if !fb.is_null() {
                ctx.destroy_framebuffer(fb);
            }
        }
        if !self.opaque_depth.is_null() {
            ctx.destroy_texture(self.opaque_depth);
        }
    }
}

impl HdrOutput {
    /// Whether this device can render into half-float color.
    pub fn supported(ctx: &Context) -> bool {
        ctx.can_render_to(&TextureFormat::rgba16f())
    }

    /// `samples` per pixel for antialiasing (0 or 1 for none; lowered to what
    /// the device allows).
    pub fn new(ctx: &Context, samples: i32, taa: bool) -> Result<Self, Error> {
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
                .texture("u_coverage")
                .uniform("u_exposure", UniformType::F32)
                .uniform("u_tone_mapping", UniformType::I32)
                .uniform("u_dither", UniformType::F32)
                .uniform("u_sharpness", UniformType::F32),
        )?;
        let group = ctx.create_param_group(layout)?;
        let taa = if taa {
            let frag = ShaderSource::plain("taa.frag", &format!("{prefix}{TAA_FRAG}"));
            let program = ctx.create_program_from(&vert, &frag)?;
            let layout = ctx.create_param_layout(
                &ParamLayoutBuilder::new()
                    .texture("u_current")
                    .texture("u_history")
                    .texture("u_prev_motion")
                    .texture("u_depth")
                    .texture("u_velocity")
                    .texture("u_opaque")
                    .uniform("u_reproject", UniformType::Mat4)
                    .uniform("u_taa", UniformType::Vec4)
                    .uniform("u_jitter", UniformType::Vec2)
                    .uniform("u_reactive", UniformType::Vec2),
            )?;
            Some(Taa {
                program,
                layout,
                group: ctx.create_param_group(layout)?,
                f_reproject: ctx.param_field(layout, "u_reproject")?,
                f_taa: ctx.param_field(layout, "u_taa")?,
                f_jitter: ctx.param_field(layout, "u_jitter")?,
                f_reactive: ctx.param_field(layout, "u_reactive")?,
                history: [(TextureId::NULL, FramebufferId::NULL); 2],
                motions: [TextureId::NULL; 2],
                depth: TextureId::NULL,
                velocity: (TextureId::NULL, FramebufferId::NULL),
                opaque: (TextureId::NULL, FramebufferId::NULL),
                opaque_kept: false,
                current: 0,
                reset: true,
            })
        } else {
            None
        };
        let grab_down = {
            let frag = ShaderSource::plain("grab_down.frag", &format!("{prefix}{GRAB_DOWN_FRAG}"));
            let program = ctx.create_program_from(&vert, &frag)?;
            let layout = ctx.create_param_layout(&ParamLayoutBuilder::new().texture("u_source"))?;
            (program, layout, ctx.create_param_group(layout)?)
        };
        let mut state = RenderState::new();
        state.depth_test = false;
        state.depth_mask = false;
        Ok(Self {
            fb: FramebufferId::NULL,
            color_rb: RenderbufferId::NULL,
            depth_rb: RenderbufferId::NULL,
            resolve_fb: FramebufferId::NULL,
            grab_fb: FramebufferId::NULL,
            grab: TextureId::NULL,
            grab_fb1: FramebufferId::NULL,
            grab_size: (0, 0),
            grab_scale: 1.0,
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
            f_sharpness: ctx.param_field(layout, "u_sharpness")?,
            grab_down,
            state: ctx.render_state(&state),
            generation: ctx.generation(),
            peel: None,
            taa,
            fxaa: None,
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

    /// Whether temporal antialiasing is on.
    pub fn taa(&self) -> bool {
        self.taa.is_some()
    }

    /// The size of the opaque-scene copy transmission sees through, as a
    /// share of the target (0.25…1, default 1): lower saves memory and the
    /// copy's time, blurring what shows through glass.
    pub fn set_transmission_scale(&mut self, scale: f32) {
        let scale = if scale.is_finite() {
            scale.clamp(0.25, 1.0)
        } else {
            1.0
        };
        if scale != self.grab_scale {
            self.grab_scale = scale;
            self.size = (0, 0); // remade at the next prepare
        }
    }

    /// Whether FXAA runs after the display pass.
    pub fn fxaa(&self) -> bool {
        self.fxaa.is_some()
    }

    /// FXAA on (its shader compiled now) or off (its program and target
    /// freed).
    pub fn set_fxaa(&mut self, ctx: &Context, on: bool) -> Result<(), Error> {
        if on == self.fxaa.is_some() {
            return Ok(());
        }
        if let Some(mut f) = self.fxaa.take() {
            f.free_target(ctx);
            ctx.destroy_param_group(f.group);
            ctx.destroy_param_layout(f.layout);
            ctx.destroy_program(f.program);
            return Ok(());
        }
        let prefix = "#version 300 es\n";
        let vert = ShaderSource::plain("fullscreen.vert", &format!("{prefix}{FULLSCREEN_VERT}"));
        let frag = ShaderSource::plain("fxaa.frag", &format!("{prefix}{FXAA_FRAG}"));
        let program = ctx.create_program_from(&vert, &frag)?;
        let layout = ctx.create_param_layout(
            &ParamLayoutBuilder::new()
                .texture("u_ldr")
                .uniform("u_texel", UniformType::Vec2),
        )?;
        let group = ctx.create_param_group(layout)?;
        self.fxaa = Some(Fxaa {
            program,
            layout,
            group,
            f_texel: ctx.param_field(layout, "u_texel")?,
            ldr: (TextureId::NULL, FramebufferId::NULL),
            size: (0, 0),
        });
        Ok(())
    }

    /// Starts the TAA history over (a camera cut: nothing to reuse).
    pub fn reset_taa(&mut self) {
        if let Some(taa) = &mut self.taa {
            taa.reset = true;
        }
    }

    /// With TAA, a pass into the velocity target (cleared to "no motion
    /// vector"), and the scene's depth to test motion draws against.
    pub fn motion_pass(&self, ctx: &Context) -> Option<(Pass, TextureId)> {
        let taa = self.taa.as_ref()?;
        let (_, fb) = taa.velocity;
        if fb.is_null() {
            return None;
        }
        ctx.framebuffer_set_clear_color(fb, VELOCITY_NONE, VELOCITY_NONE, 0.0, 0.0);
        let mut pass = Pass::new(fb);
        pass.set_viewport(&Viewport::new(0, 0, self.size.0, self.size.1));
        pass.set_clear(&ClearMask {
            color: true,
            depth: false,
            stencil: false,
        });
        Some((pass, taa.depth))
    }

    /// With TAA, copies the opaque scene (after the opaque pass, before
    /// transmissive and transparent meshes draw): this frame's resolve
    /// then knows where they changed it (its reactive mask).
    pub fn keep_opaque_color(&mut self, ctx: &Context) {
        let (w, h) = self.size;
        let Some(taa) = &mut self.taa else {
            return;
        };
        if taa.opaque.1.is_null() {
            let made = (|| -> Result<(TextureId, FramebufferId), Error> {
                let t = ctx.create_texture(
                    TextureTarget::Texture2D,
                    TextureMinFilter::Nearest,
                    TextureMagFilter::Nearest,
                )?;
                ctx.texture_storage_2d(t, 1, &TextureFormat::rgba16f(), w, h);
                let fb = match ctx.create_framebuffer(w, h) {
                    Ok(fb) => fb,
                    Err(e) => {
                        ctx.destroy_texture(t);
                        return Err(e);
                    }
                };
                ctx.framebuffer_attach_color_texture_2d(fb, 0, t, 0);
                Ok((t, fb))
            })();
            match made {
                Ok(o) => taa.opaque = o,
                Err(e) => {
                    rswebgl::console::error_once(&format!(
                        "[rswebgl-scene] TAA reactive mask: {e}"
                    ));
                    return;
                }
            }
        }
        let r = Viewport::new(0, 0, w, h);
        ctx.blit_framebuffer(
            self.resolve_fb,
            &r,
            taa.opaque.1,
            &r,
            &ClearMask::color(),
            TextureMagFilter::Nearest,
        );
        taa.opaque_kept = true;
    }

    /// Resolves this frame into the TAA history: the resolved color, the
    /// last history reprojected — by the velocity target where `velocity`
    /// (`motion_pass` drew into it this frame), else by `reproject` (clip
    /// now, unjittered → clip last frame) through the depth — and clipped.
    /// `jitter` is this frame's projection offset (NDC), `exposure` the
    /// display's. Where transmissive or transparent meshes changed the
    /// opaque scene (`keep_opaque_color` this frame), the history counts
    /// less while the pixel moves, or anywhere if `late_moved` (one of them
    /// moved). After the scene's passes, before `present`.
    pub fn taa_resolve(
        &mut self,
        ctx: &Context,
        reproject: &glam::Mat4,
        jitter: glam::Vec2,
        exposure: f32,
        velocity: bool,
        late_moved: bool,
    ) {
        let (w, h) = self.size;
        let reverse = self.reverse_z;
        let color = self.color;
        let Some(taa) = &mut self.taa else {
            return;
        };
        let next = 1 - taa.current;
        let (out_tex, out_fb) = taa.history[next];
        let _ = out_tex;
        ctx.param_set_texture_by_name(taa.group, "u_current", color);
        ctx.param_set_texture_by_name(taa.group, "u_history", taa.history[taa.current].0);
        ctx.param_set_texture_by_name(taa.group, "u_prev_motion", taa.motions[taa.current]);
        ctx.param_set_texture_by_name(taa.group, "u_depth", taa.depth);
        ctx.param_set_texture_by_name(taa.group, "u_velocity", taa.velocity.0);
        ctx.param_set_floats(taa.group, taa.f_reproject, &reproject.to_cols_array());
        let flags = [
            if taa.reset { 1.0 } else { 0.0 },
            if reverse { 1.0 } else { 0.0 },
            if velocity { 1.0 } else { 0.0 },
            exposure.max(1e-6),
        ];
        ctx.param_set_floats(taa.group, taa.f_taa, &flags);
        ctx.param_set_floats(taa.group, taa.f_jitter, &[jitter.x, jitter.y]);
        // The copy's texture is bound either way (a sampler needs one).
        let opaque = if taa.opaque.0.is_null() {
            color
        } else {
            taa.opaque.0
        };
        ctx.param_set_texture_by_name(taa.group, "u_opaque", opaque);
        let reactive = [
            if taa.opaque_kept { 1.0 } else { 0.0 },
            if late_moved { 1.0 } else { 0.0 },
        ];
        ctx.param_set_floats(taa.group, taa.f_reactive, &reactive);
        taa.opaque_kept = false;
        let mut pass = Pass::new(out_fb);
        pass.set_viewport(&Viewport::new(0, 0, w, h));
        let mut groups = [ParamGroupId::NULL; GROUP_SLOTS];
        groups[0] = taa.group;
        pass.push_groups_with_key(
            0,
            taa.program,
            self.state,
            VertexArrayId::NULL,
            groups,
            &DrawCommand::arrays(DrawMode::Triangles, 0, 3),
        );
        ctx.render(&pass);
        taa.current = next;
        taa.reset = false;
    }

    /// Samples per pixel actually used (0: no multisampling).
    pub fn samples(&self) -> i32 {
        self.samples
    }

    /// Points `pass` at the HDR target (cleared; resolved and discarded
    /// after drawing when multisampled), sized `w`×`h` (the canvas):
    /// (re)allocates the target when the size or depth mode changed.
    pub fn prepare(&mut self, ctx: &Context, w: i32, h: i32, pass: &mut Pass) -> Result<(), Error> {
        self.prepare_split(ctx, w, h, pass, true)
    }

    /// `prepare` for the first of two passes into the target (`last`
    /// false): it resolves but keeps the multisampled data for the next
    /// (`prepare_late`).
    pub fn prepare_split(
        &mut self,
        ctx: &Context,
        w: i32,
        h: i32,
        pass: &mut Pass,
        last: bool,
    ) -> Result<(), Error> {
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
        self.finish_pass(pass, last);
        Ok(())
    }

    /// The second pass into the target: no clear, drawn over the first.
    pub fn prepare_late(&self, pass: &mut Pass) {
        pass.set_target(self.fb);
        pass.set_clear(&ClearMask::none());
        self.finish_pass(pass, true);
    }

    fn finish_pass(&self, pass: &mut Pass, last: bool) {
        let mut discard = InvalidateMask::none();
        // After the last pass depth isn't read again (except by TAA: its
        // motion pass and resolve), nor the multisampled color once
        // resolved: telling tiled (mobile) GPUs saves writing them back.
        discard.depth = last && self.taa.is_none();
        if self.fb != self.resolve_fb {
            pass.set_resolve(self.resolve_fb);
            discard.color = last;
        } else {
            pass.clear_resolve();
        }
        pass.set_invalidate(&discard);
    }

    /// The scene copy transmission reads, and its last mip — valid once
    /// `grab` has run this frame.
    pub fn scene_color(&self) -> Option<(TextureId, u32)> {
        let levels = mip_levels(self.grab_size.0, self.grab_size.1);
        (!self.grab.is_null()).then_some((self.grab, levels - 1))
    }

    /// Copies what the target holds (the resolved opaque scene) into the
    /// mipmapped texture transmissive surfaces see through.
    pub fn grab(&self, ctx: &Context) {
        let (w, h) = self.size;
        let (gw, gh) = self.grab_size;
        ctx.blit_framebuffer(
            self.resolve_fb,
            &Viewport::new(0, 0, w, h),
            self.grab_fb,
            &Viewport::new(0, 0, gw, gh),
            &ClearMask::color(),
            TextureMagFilter::Linear,
        );
        let levels = mip_levels(gw, gh) as i32;
        if levels < 2 || self.grab_fb1.is_null() {
            ctx.texture_generate_mipmaps(self.grab);
            return;
        }
        // Level 1 from level 0, fireflies suppressed (grab_down.frag); the
        // texture sampled at level 0 alone meanwhile (no feedback loop).
        let (program, _, group) = self.grab_down;
        ctx.texture_set_base_level(self.grab, 0);
        ctx.texture_set_max_level(self.grab, 0);
        ctx.param_set_texture_by_name(group, "u_source", self.grab);
        let mut pass = Pass::new(self.grab_fb1);
        pass.set_viewport(&Viewport::new(0, 0, (gw / 2).max(1), (gh / 2).max(1)));
        pass.set_clear(&ClearMask::color()); // every texel written
        let mut groups = [ParamGroupId::NULL; GROUP_SLOTS];
        groups[0] = group;
        pass.push_groups_with_key(
            0,
            program,
            self.state,
            VertexArrayId::NULL,
            groups,
            &DrawCommand::arrays(DrawMode::Triangles, 0, 3),
        );
        ctx.render(&pass);
        // The rest from level 1 (box), then sampled whole again.
        ctx.texture_set_base_level(self.grab, 1);
        ctx.texture_set_max_level(self.grab, levels - 1);
        ctx.texture_generate_mipmaps(self.grab);
        ctx.texture_set_base_level(self.grab, 0);
    }

    // The depth format of the target (what depth blits copy between).
    fn depth_format(&self) -> TextureFormat {
        if self.reverse_z {
            TextureFormat::depth32f()
        } else {
            TextureFormat::depth24()
        }
    }

    /// Frees the peeling targets (peeling turned off).
    pub fn release_peel(&mut self, ctx: &Context) {
        if let Some(peel) = self.peel.take() {
            peel.free(ctx);
        }
    }

    /// Makes (or keeps) `layers` peeling targets at the current size.
    pub fn ensure_peel(&mut self, ctx: &Context, layers: usize) -> Result<(), Error> {
        if self.peel.as_ref().is_some_and(|p| p.layers.len() == layers) {
            return Ok(());
        }
        if let Some(peel) = self.peel.take() {
            peel.free(ctx);
        }
        let (w, h) = self.size;
        let format = self.depth_format();
        let depth = |ctx: &Context| -> Result<(TextureId, FramebufferId), Error> {
            let t = ctx.create_texture(
                TextureTarget::Texture2D,
                TextureMinFilter::Nearest,
                TextureMagFilter::Nearest,
            )?;
            ctx.texture_storage_2d(t, 1, &format, w, h);
            let fb = ctx.create_framebuffer(w, h)?;
            ctx.framebuffer_attach_depth_texture(fb, t, 0);
            Ok((t, fb))
        };
        // Built as it goes, so a failure part way frees what was made.
        let mut peel = PeelTargets {
            layers: Vec::with_capacity(layers),
            opaque_depth: TextureId::NULL,
            opaque_fb: FramebufferId::NULL,
            glass_fb: FramebufferId::NULL,
            transparent_fb: FramebufferId::NULL,
        };
        let made = (|| -> Result<(), Error> {
            (peel.opaque_depth, peel.opaque_fb) = depth(ctx)?;
            for _ in 0..layers {
                peel.layers.push(depth(ctx)?);
            }
            peel.glass_fb = ctx.create_framebuffer(w, h)?;
            ctx.framebuffer_attach_color_texture_2d(peel.glass_fb, 0, self.color, 0);
            peel.transparent_fb = ctx.create_framebuffer(w, h)?;
            ctx.framebuffer_attach_color_texture_2d(peel.transparent_fb, 0, self.color, 0);
            ctx.framebuffer_attach_depth_texture(peel.transparent_fb, peel.layers[0].0, 0);
            Ok(())
        })();
        if let Err(e) = made {
            peel.free(ctx);
            return Err(e);
        }
        self.peel = Some(peel);
        Ok(())
    }

    /// Copies the opaque pass's depth (resolved from the multisampled
    /// target) for peeling. After the opaque pass, before `peel_layer`.
    pub fn keep_opaque_depth(&self, ctx: &Context) {
        if let Some(p) = &self.peel {
            let (w, h) = self.size;
            let r = Viewport::new(0, 0, w, h);
            ctx.blit_framebuffer(
                self.fb,
                &r,
                p.opaque_fb,
                &r,
                &ClearMask::depth(),
                TextureMagFilter::Nearest,
            );
        }
    }

    /// Readies layer `k`'s depth target, starting from the opaque depth,
    /// and returns it with the previous layer's depth texture (NULL for
    /// the first): what the peel pass discards against.
    pub fn peel_layer(&self, ctx: &Context, k: usize) -> Option<(FramebufferId, TextureId)> {
        let p = self.peel.as_ref()?;
        let (_, fb) = *p.layers.get(k)?;
        let (w, h) = self.size;
        let r = Viewport::new(0, 0, w, h);
        ctx.blit_framebuffer(
            p.opaque_fb,
            &r,
            fb,
            &r,
            &ClearMask::depth(),
            TextureMagFilter::Nearest,
        );
        let previous = k.checked_sub(1).map_or(TextureId::NULL, |j| p.layers[j].0);
        Some((fb, previous))
    }

    /// Layer `k`'s depth texture (what its shading pass matches).
    pub fn layer_depth(&self, k: usize) -> TextureId {
        self.peel
            .as_ref()
            .and_then(|p| p.layers.get(k))
            .map_or(TextureId::NULL, |l| l.0)
    }

    /// Where peeled layers shade (the resolved color, no depth), and where
    /// transparent meshes draw after them (with the nearest layer's depth).
    pub fn peel_color_targets(&self) -> Option<(FramebufferId, FramebufferId)> {
        self.peel.as_ref().map(|p| (p.glass_fb, p.transparent_fb))
    }

    /// Draws the target to the canvas (through FXAA when on).
    pub fn present(
        &mut self,
        ctx: &Context,
        exposure: f32,
        tone_mapping: ToneMapping,
        dither: bool,
        sharpness: f32,
        profiler: Option<&mut GpuProfiler>,
    ) {
        // With TAA the display shows the history (the coverage stays this
        // frame's).
        let shown = match &self.taa {
            Some(taa) if !taa.history[taa.current].0.is_null() => taa.history[taa.current].0,
            _ => self.color,
        };
        ctx.param_set_texture_by_name(self.group, "u_hdr", shown);
        ctx.param_set_texture_by_name(self.group, "u_coverage", self.color);
        ctx.param_set_f32(self.group, self.f_exposure, exposure);
        ctx.param_set_i32(self.group, self.f_tone_mapping, tone_mapping as i32);
        ctx.param_set_f32(self.group, self.f_dither, if dither { 1.0 } else { 0.0 });
        ctx.param_set_f32(self.group, self.f_sharpness, sharpness.clamp(0.0, 1.0));
        let fb = ctx.default_framebuffer();
        let (cw, ch) = (fb.width().max(1), fb.height().max(1));
        // With FXAA the display pass draws into its texture (it failing to
        // allocate: straight to the canvas, unfiltered).
        let ldr = match &mut self.fxaa {
            Some(f) => match f.ensure_target(ctx, cw, ch) {
                Ok(fb) => Some(fb),
                Err(e) => {
                    rswebgl::console::error_once(&format!("[rswebgl-scene] FXAA target: {e}"));
                    None
                }
            },
            None => None,
        };
        let mut pass = ldr.map_or_else(Pass::to_default, Pass::new);
        pass.set_viewport(&Viewport::new(0, 0, cw, ch));
        if ldr.is_some() {
            // Every pixel is written: nothing to load on tiled GPUs.
            pass.set_clear(&ClearMask::color());
        }
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
        let mut profiler = profiler;
        if let Some(p) = profiler.as_deref_mut() {
            p.time(&mut pass, "present");
        }
        ctx.render(&pass);
        let (Some(_), Some(f)) = (ldr, &self.fxaa) else {
            return;
        };
        ctx.param_set_texture_by_name(f.group, "u_ldr", f.ldr.0);
        ctx.param_set_floats(f.group, f.f_texel, &[1.0 / cw as f32, 1.0 / ch as f32]);
        let mut pass = Pass::to_default();
        pass.set_viewport(&Viewport::new(0, 0, cw, ch));
        let mut groups = [ParamGroupId::NULL; GROUP_SLOTS];
        groups[0] = f.group;
        pass.push_groups_with_key(
            0,
            f.program,
            self.state,
            VertexArrayId::NULL,
            groups,
            &DrawCommand::arrays(DrawMode::Triangles, 0, 3),
        );
        if let Some(p) = profiler {
            p.time(&mut pass, "fxaa");
        }
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

        let gw = ((w as f32 * self.grab_scale).round() as i32).max(1);
        let gh = ((h as f32 * self.grab_scale).round() as i32).max(1);
        let grab = ctx.create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::LinearMipmapLinear,
            TextureMagFilter::Linear,
        )?;
        ctx.texture_storage_2d(
            grab,
            mip_levels(gw, gh) as i32,
            &TextureFormat::rgba16f(),
            gw,
            gh,
        );
        ctx.texture_set_wrap_s(grab, TextureWrap::ClampToEdge);
        ctx.texture_set_wrap_t(grab, TextureWrap::ClampToEdge);
        let grab_fb = ctx.create_framebuffer(gw, gh)?;
        ctx.framebuffer_attach_color_texture_2d(grab_fb, 0, grab, 0);
        (self.grab, self.grab_fb, self.grab_size) = (grab, grab_fb, (gw, gh));
        if mip_levels(gw, gh) > 1 {
            let (w1, h1) = ((gw / 2).max(1), (gh / 2).max(1));
            let fb1 = ctx.create_framebuffer(w1, h1)?;
            ctx.framebuffer_attach_color_texture_2d(fb1, 0, grab, 1);
            self.grab_fb1 = fb1;
        }

        // Multisampled if the device takes it for this format; else straight
        // into the texture. No more samples than both formats take.
        if self.samples > 1 {
            let most = ctx
                .max_samples_for(RenderbufferFormat::Rgba16f)
                .min(ctx.max_samples_for(depth));
            self.samples = self.samples.min(most);
        }
        while self.samples > 1 && self.taa.is_none() {
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
        if let Some(taa) = &mut self.taa {
            let tex = |format: &TextureFormat, filter| -> Result<TextureId, Error> {
                let t = ctx.create_texture(
                    TextureTarget::Texture2D,
                    filter,
                    TextureMagFilter::Nearest,
                )?;
                ctx.texture_storage_2d(t, 1, format, w, h);
                Ok(t)
            };
            let depth_format = if reverse_z {
                TextureFormat::depth32f()
            } else {
                TextureFormat::depth24()
            };
            taa.depth = tex(&depth_format, TextureMinFilter::Nearest)?;
            ctx.framebuffer_attach_depth_texture(resolve_fb, taa.depth, 0);
            for h_slot in &mut taa.history {
                let t = ctx.create_texture(
                    TextureTarget::Texture2D,
                    TextureMinFilter::Linear,
                    TextureMagFilter::Linear,
                )?;
                ctx.texture_storage_2d(t, 1, &TextureFormat::rgba16f(), w, h);
                ctx.texture_set_wrap_s(t, TextureWrap::ClampToEdge);
                ctx.texture_set_wrap_t(t, TextureWrap::ClampToEdge);
                let fb = ctx.create_framebuffer(w, h)?;
                ctx.framebuffer_attach_color_texture_2d(fb, 0, t, 0);
                *h_slot = (t, fb);
            }
            // Each resolve's motion next to its history (renderable with
            // the half-float color the HDR target needs).
            for i in 0..2 {
                let m = tex(&TextureFormat::rg16f(), TextureMinFilter::Nearest)?;
                ctx.framebuffer_attach_color_texture_2d(taa.history[i].1, 1, m, 0);
                taa.motions[i] = m;
            }
            let v = tex(&TextureFormat::rg16f(), TextureMinFilter::Nearest)?;
            let fb = ctx.create_framebuffer(w, h)?;
            ctx.framebuffer_attach_color_texture_2d(fb, 0, v, 0);
            if ctx.is_framebuffer_complete(fb) {
                taa.velocity = (v, fb);
            } else {
                // Moving meshes then lean on the clip alone.
                ctx.destroy_framebuffer(fb);
                ctx.destroy_texture(v);
            }
            taa.reset = true;
        } else {
            let depth_rb = ctx.create_renderbuffer(depth, w, h)?;
            ctx.framebuffer_attach_depth_renderbuffer(resolve_fb, depth_rb);
            self.depth_rb = depth_rb;
        }
        self.fb = resolve_fb;
        if !ctx.is_framebuffer_complete(resolve_fb) {
            return Err("HDR output: the half-float target is incomplete on this device".into());
        }
        Ok(())
    }

    fn free_target(&mut self, ctx: &Context) {
        if let Some(taa) = &mut self.taa {
            for (t, fb) in std::mem::replace(
                &mut taa.history,
                [(TextureId::NULL, FramebufferId::NULL); 2],
            ) {
                if !fb.is_null() {
                    ctx.destroy_framebuffer(fb);
                }
                if !t.is_null() {
                    ctx.destroy_texture(t);
                }
            }
            if !taa.depth.is_null() {
                ctx.destroy_texture(std::mem::replace(&mut taa.depth, TextureId::NULL));
            }
            for m in std::mem::replace(&mut taa.motions, [TextureId::NULL; 2]) {
                if !m.is_null() {
                    ctx.destroy_texture(m);
                }
            }
            let (v, fb) =
                std::mem::replace(&mut taa.velocity, (TextureId::NULL, FramebufferId::NULL));
            if !fb.is_null() {
                ctx.destroy_framebuffer(fb);
            }
            if !v.is_null() {
                ctx.destroy_texture(v);
            }
            let (o, fb) =
                std::mem::replace(&mut taa.opaque, (TextureId::NULL, FramebufferId::NULL));
            if !fb.is_null() {
                ctx.destroy_framebuffer(fb);
            }
            if !o.is_null() {
                ctx.destroy_texture(o);
            }
            taa.opaque_kept = false;
        }
        if let Some(peel) = self.peel.take() {
            peel.free(ctx);
        }
        if self.fb != self.resolve_fb && !self.fb.is_null() {
            ctx.destroy_framebuffer(self.fb);
        }
        if !self.resolve_fb.is_null() {
            ctx.destroy_framebuffer(self.resolve_fb);
        }
        if !self.grab_fb.is_null() {
            ctx.destroy_framebuffer(self.grab_fb);
        }
        if !self.grab_fb1.is_null() {
            ctx.destroy_framebuffer(self.grab_fb1);
        }
        self.grab_fb1 = FramebufferId::NULL;
        if !self.grab.is_null() {
            ctx.destroy_texture(self.grab);
        }
        self.grab_fb = FramebufferId::NULL;
        self.grab = TextureId::NULL;
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
        ctx.destroy_param_group(self.grab_down.2);
        ctx.destroy_param_layout(self.grab_down.1);
        if self.generation == ctx.generation() {
            ctx.destroy_program(self.grab_down.0);
        }
        if let Some(taa) = &self.taa {
            ctx.destroy_param_group(taa.group);
            ctx.destroy_param_layout(taa.layout);
            if self.generation == ctx.generation() {
                ctx.destroy_program(taa.program);
            }
        }
        if let Some(mut f) = self.fxaa.take() {
            ctx.destroy_param_group(f.group);
            ctx.destroy_param_layout(f.layout);
            if self.generation == ctx.generation() {
                f.free_target(ctx);
                ctx.destroy_program(f.program);
            }
        }
        if self.generation != ctx.generation() {
            return;
        }
        self.free_target(ctx);
        ctx.destroy_program(self.program);
    }
}

fn mip_levels(w: i32, h: i32) -> u32 {
    32 - (w.max(h).max(1) as u32).leading_zeros()
}
