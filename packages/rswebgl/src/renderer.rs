// Pass execution: walks a pass's draw list and issues the GL calls, diffing
// state between consecutive draws. Resources are resolved from the context's
// pools by handle; a handle that went stale since the pass was built is
// reported once and its pass / draw skipped.
//
// What the renderer leaves bound is remembered across calls (`GlState`, kept
// in the context), so a frame that starts with the program, VAO, render state
// and textures the previous one ended with re-issues none of them. Library
// code that binds outside the renderer resets the part it touched; code that
// changes GL state behind the library's back calls `Context::invalidate_state`.

use crate::error::{Error, err};
use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::context::{Context, Resources, stale};
use crate::draw::Viewport;
use crate::framebuffer::{ClearMask, DefaultFramebuffer, InvalidateMask, framebuffer_status_str};
use rswebgl_foundation::pool::Pool;

use crate::ids::{FramebufferId, ProgramId, StateId, TextureId, UniformBufferId, VertexArrayId};
use crate::param::{GROUP_SLOTS, ParamGroupObj, ParamLayoutObj};
use crate::pass::Pass;
use crate::program::ProgramObj;
use crate::render_state::RenderState;
use crate::texture::TextureObj;
use crate::uniform_buffer::UniformBufferObj;

/// What rendering cost since the last `Context::reset_frame_stats`: draws
/// issued and the GL state changes they needed. Call `reset_frame_stats` once
/// per frame and read this after rendering to see how well draws are sorted.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    pub passes: u32,
    /// Draw calls issued.
    pub draws: u32,
    /// Draws skipped: a stale handle or a program that failed to link.
    pub skipped: u32,
    /// useProgram calls.
    pub programs: u32,
    /// Render-state changes (each may be several GL calls).
    pub states: u32,
    /// bindVertexArray calls.
    #[wasm_bindgen(js_name = vertexArrays)]
    pub vertex_arrays: u32,
    /// bindTexture calls.
    pub textures: u32,
    /// bindBufferBase / bindBufferRange calls for uniform blocks.
    pub blocks: u32,
    /// uniform* calls (values that differed from what the program held).
    pub uniforms: u32,
}

#[wasm_bindgen]
impl Context {
    /// Executes one render pass. Does nothing while the context is lost.
    pub fn render(&self, pass: &Pass) {
        self.render_passes(std::slice::from_ref(pass));
    }

    /// Checks framebuffer completeness. Binds `fb` to read its status.
    #[wasm_bindgen(js_name = checkFramebuffer)]
    pub fn check_framebuffer(&self, fb: FramebufferId) -> Result<(), Error> {
        self.check_alive()?;
        let gl = self.gl();
        let res = self.res();
        let Some(f) = res.framebuffers.get(fb.0) else {
            return Err(err!("framebuffer {:?} is destroyed or invalid", fb.0));
        };
        self.gl_state().forget_target();
        gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&f.raw));
        let status = gl.check_framebuffer_status(WebGl2RenderingContext::FRAMEBUFFER);
        if status == WebGl2RenderingContext::FRAMEBUFFER_COMPLETE {
            Ok(())
        } else {
            Err(framebuffer_status_str(status).into())
        }
    }

    #[wasm_bindgen(js_name = isFramebufferComplete)]
    pub fn is_framebuffer_complete(&self, fb: FramebufferId) -> bool {
        self.check_framebuffer(fb).is_ok()
    }

    /// Forgets what the library believes is bound, so the next render binds
    /// everything afresh. Call after changing GL state directly (your own GL
    /// calls on the same context, or another library sharing it).
    #[wasm_bindgen(js_name = invalidateState)]
    pub fn invalidate_state(&self) {
        self.core.invalidate_state();
    }

    /// Counters since the last `reset_frame_stats`.
    #[wasm_bindgen(js_name = frameStats)]
    pub fn frame_stats(&self) -> FrameStats {
        self.gl_state().stats
    }

    #[wasm_bindgen(js_name = resetFrameStats)]
    pub fn reset_frame_stats(&self) {
        self.gl_state().stats = FrameStats::default();
    }
}

impl Context {
    /// Executes several passes in order. Rust-only: a `&[Pass]` doesn't cross
    /// the wasm_bindgen boundary.
    pub fn render_passes(&self, passes: &[Pass]) {
        let gl = self.gl();
        // Every call would be a no-op anyway, but program/framebuffer status
        // queries would report bogus failures (and mark programs invalid).
        if gl.is_context_lost() {
            return;
        }
        let res = &mut *self.res();
        let st = &mut *self.gl_state();
        for pass in passes {
            execute_pass(gl, &self.core.default_fb, res, pass, st);
        }
    }
}

// ---------------------------------------------------------------------------
// Pass execution
// ---------------------------------------------------------------------------

fn execute_pass(
    gl: &WebGl2RenderingContext,
    default_fb: &DefaultFramebuffer,
    res: &mut Resources,
    pass: &Pass,
    st: &mut GlState,
) {
    let Resources {
        framebuffers,
        programs,
        vertex_arrays,
        textures,
        uniform_buffers,
        param_layouts,
        param_groups,
        render_states,
        ..
    } = res;

    // Render target — bind whatever this pass draws into.
    let target = if pass.target.is_null() {
        None
    } else {
        match framebuffers.get(pass.target.0) {
            Some(f) => Some(f),
            None => {
                st.stats.skipped += pass.items.len() as u32;
                return stale("framebuffer", "render (pass target)", pass.target);
            }
        }
    };
    st.stats.passes += 1;
    st.bind_target(gl, pass.target, target.map(|f| &f.raw));

    // Viewport — the pass's override, else the target's full size.
    let vp = pass.viewport.unwrap_or_else(|| match target {
        Some(f) => f.viewport,
        None => default_fb.viewport(),
    });
    st.set_viewport(gl, vp);

    // Load op — clear before drawing. Clearing obeys the write masks and
    // scissor: open those the current render state keeps closed, clear, and
    // put them back. If the state is unknown, open all and let the next draw
    // apply its state in full.
    if pass.clear != ClearMask::none() {
        let cur = st.render_state.and_then(|id| render_states.get(id.0));
        let opened = open_for_clear(gl, pass.clear, cur);
        match target {
            Some(f) => f.clear(gl, pass.clear),
            None => clear_default(default_fb, gl, pass.clear),
        }
        match cur {
            Some(rs) => opened.restore(gl, rs),
            None => st.forget_render_state(),
        }
    }

    for d in &pass.items {
        let Some(program) = programs.get_mut(d.program.0) else {
            stale("program", "render (draw)", d.program);
            st.stats.skipped += 1;
            continue;
        };
        // A program whose link failed is skipped (its error is reported once).
        // One still linking in the background is waited on, not skipped, so a
        // one-shot pass (a bake, a pack) can't silently draw nothing; callers who
        // want to avoid the stall poll `program_is_ready` before submitting.
        if !program.ensure_linked(gl) {
            st.stats.skipped += 1;
            continue;
        }
        if !st.apply_render_state(gl, d.state, render_states) {
            stale("render state", "render (draw)", d.state);
            st.stats.skipped += 1;
            continue;
        }
        let vao = if d.vao.is_null() {
            None
        } else {
            match vertex_arrays.get(d.vao.0) {
                Some(v) => Some(&v.raw),
                None => {
                    stale("vertex array", "render (draw)", d.vao);
                    st.stats.skipped += 1;
                    continue;
                }
            }
        };
        st.use_program(gl, d.program, &program.raw);
        program.activate(gl);
        st.bind_vao(gl, d.vao, vao);

        // Values are compared against what the program holds, so unchanged ones
        // cost a compare, not a GL call.
        let mut used = [0usize; GROUP_SLOTS];
        let mut n_used = 0;
        for &gid in &d.groups {
            if gid.is_null() {
                continue;
            }
            let Some(group) = param_groups.get(gid.0) else {
                stale("param group", "render (draw)", gid);
                continue;
            };
            let layout = param_layouts
                .get(group.layout.0)
                .expect("a group keeps its layout alive");
            let m = program.mapping(group.layout, layout);
            apply_group(
                gl,
                program,
                m,
                group,
                layout,
                textures,
                uniform_buffers,
                &mut st.bindings,
                &mut st.stats,
            );
            used[n_used] = m;
            n_used += 1;
        }
        program.check_covered(&used[..n_used]);
        d.command.execute(gl);
        st.stats.draws += 1;
    }

    // Store op — discard attachments we won't read again.
    if pass.invalidate != InvalidateMask::none() {
        let arr = js_sys::Array::new();
        match target {
            Some(f) => f.collect_invalidate_attachments(&arr, pass.invalidate),
            None => {
                if pass.invalidate.color {
                    arr.push(&JsValue::from(WebGl2RenderingContext::COLOR));
                }
                if pass.invalidate.depth {
                    arr.push(&JsValue::from(WebGl2RenderingContext::DEPTH));
                }
                if pass.invalidate.stencil {
                    arr.push(&JsValue::from(WebGl2RenderingContext::STENCIL));
                }
            }
        }
        let _ = gl.invalidate_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, arr.as_ref());
    }
}

fn clear_default(default_fb: &DefaultFramebuffer, gl: &WebGl2RenderingContext, mask: ClearMask) {
    if mask.color {
        let c = default_fb.clear_color_rgba();
        gl.clear_color(c[0], c[1], c[2], c[3]);
    }
    if mask.depth {
        gl.clear_depth(default_fb.clear_depth_value());
    }
    if mask.stencil {
        gl.clear_stencil(default_fb.clear_stencil_value());
    }
    gl.clear(mask.as_gl());
}

// What `open_for_clear` changed, to put back after the clear.
struct Opened {
    color: bool,
    depth: bool,
    stencil: bool,
    scissor: bool,
}

impl Opened {
    fn restore(&self, gl: &WebGl2RenderingContext, rs: &RenderState) {
        if self.color {
            gl.color_mask(
                rs.color_mask_r,
                rs.color_mask_g,
                rs.color_mask_b,
                rs.color_mask_a,
            );
        }
        if self.depth {
            gl.depth_mask(rs.depth_mask);
        }
        if self.stencil {
            gl.stencil_mask(rs.stencil_write_mask);
        }
        if self.scissor {
            gl.enable(WebGl2RenderingContext::SCISSOR_TEST);
        }
    }
}

// Opens the write masks and scissor the given clear touches (all of them if
// the current render state `cur` is unknown), so the clear isn't silently
// masked off or scissored down.
fn open_for_clear(
    gl: &WebGl2RenderingContext,
    mask: ClearMask,
    cur: Option<&RenderState>,
) -> Opened {
    let closed = |f: fn(&RenderState) -> bool| cur.is_none_or(f);
    let opened = Opened {
        color: mask.color
            && closed(|rs| {
                !(rs.color_mask_r && rs.color_mask_g && rs.color_mask_b && rs.color_mask_a)
            }),
        depth: mask.depth && closed(|rs| !rs.depth_mask),
        stencil: mask.stencil && closed(|rs| rs.stencil_write_mask != 0xFFFF_FFFF),
        scissor: closed(|rs| rs.scissor_test),
    };
    if opened.color {
        gl.color_mask(true, true, true, true);
    }
    if opened.depth {
        gl.depth_mask(true);
    }
    if opened.stencil {
        gl.stencil_mask(0xFFFF_FFFF);
    }
    if opened.scissor {
        gl.disable(WebGl2RenderingContext::SCISSOR_TEST);
    }
    opened
}

// ---------------------------------------------------------------------------
// GlState — what the renderer last left bound, kept in the context across
// render calls. Each slot starts as `None` ("unknown"), so the first use
// applies in full; thereafter only differences reach GL.
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct GlState {
    // FramebufferId::NULL = the default framebuffer.
    target: Option<FramebufferId>,
    viewport: Option<Viewport>,
    render_state: Option<StateId>,
    program: Option<ProgramId>,
    // VertexArrayId::NULL = explicitly no VAO.
    vao: Option<VertexArrayId>,
    // What's bound to each texture unit / uniform-buffer point. Global state, so
    // it's kept across program switches.
    bindings: GlBindings,
    stats: FrameStats,
}

impl GlState {
    /// Forgets everything bound (keeps the counters).
    pub(crate) fn reset(&mut self) {
        *self = Self {
            stats: self.stats,
            ..Self::default()
        };
    }

    /// Something bound a framebuffer.
    pub(crate) fn forget_target(&mut self) {
        self.target = None;
    }

    /// Something bound a vertex array.
    pub(crate) fn forget_vao(&mut self) {
        self.vao = None;
    }

    /// Something bound a texture on the active unit.
    pub(crate) fn forget_texture_unit(&mut self) {
        self.bindings.forget_active_unit();
    }

    fn forget_render_state(&mut self) {
        self.render_state = None;
    }

    fn bind_target(
        &mut self,
        gl: &WebGl2RenderingContext,
        id: FramebufferId,
        raw: Option<&web_sys::WebGlFramebuffer>,
    ) {
        if self.target != Some(id) {
            gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, raw);
            self.target = Some(id);
        }
    }

    fn set_viewport(&mut self, gl: &WebGl2RenderingContext, vp: Viewport) {
        if self.viewport != Some(vp) {
            gl.viewport(vp.x, vp.y, vp.width, vp.height);
            self.viewport = Some(vp);
        }
    }

    // Applies interned state `id`, diffing against the one applied last.
    // False if `id` doesn't name a state.
    fn apply_render_state(
        &mut self,
        gl: &WebGl2RenderingContext,
        id: StateId,
        states: &Pool<RenderState>,
    ) -> bool {
        if self.render_state == Some(id) {
            return true;
        }
        let Some(rs) = states.get(id.0) else {
            return false;
        };
        match self.render_state.and_then(|prev| states.get(prev.0)) {
            Some(prev) => rs.apply_diff(prev, gl),
            None => rs.apply(gl),
        }
        self.render_state = Some(id);
        self.stats.states += 1;
        true
    }

    fn use_program(
        &mut self,
        gl: &WebGl2RenderingContext,
        id: ProgramId,
        raw: &web_sys::WebGlProgram,
    ) {
        if self.program != Some(id) {
            gl.use_program(Some(raw));
            self.program = Some(id);
            self.stats.programs += 1;
        }
    }

    fn bind_vao(
        &mut self,
        gl: &WebGl2RenderingContext,
        id: VertexArrayId,
        raw: Option<&web_sys::WebGlVertexArrayObject>,
    ) {
        if self.vao != Some(id) {
            gl.bind_vertex_array(raw);
            self.vao = Some(id);
            self.stats.vertex_arrays += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Parameter groups
// ---------------------------------------------------------------------------

// Applies one group to the program in use through mapping `m`: changed plain
// values, its textures (on the units the program assigned), its uniform
// buffers (flushed, on the program's binding points).
#[allow(clippy::too_many_arguments)]
fn apply_group(
    gl: &WebGl2RenderingContext,
    program: &mut ProgramObj,
    m: usize,
    group: &ParamGroupObj,
    layout: &ParamLayoutObj,
    textures: &Pool<TextureObj>,
    uniform_buffers: &mut Pool<UniformBufferObj>,
    bindings: &mut GlBindings,
    stats: &mut FrameStats,
) {
    for (i, f) in layout.plain.iter().enumerate() {
        if program.upload_field(gl, m, i, f, group) {
            stats.uniforms += 1;
        }
    }

    for i in 0..layout.textures.len() {
        let Some(unit) = program.mapping_at(m).textures[i] else {
            continue;
        };
        let tex = group.textures[i];
        if tex.is_null() {
            continue;
        }
        match textures.get(tex.0) {
            Some(obj) => {
                if bindings.bind_texture(gl, unit, tex, obj) {
                    stats.textures += 1;
                }
            }
            None => stale(
                "texture",
                &format!("texture \"{}\"", layout.textures[i]),
                tex,
            ),
        }
    }

    for i in 0..layout.blocks.len() {
        let Some(bi) = program.mapping_at(m).blocks[i] else {
            continue;
        };
        let (ubo, range) = group.blocks[i];
        if ubo.is_null() {
            continue;
        }
        let Some(obj) = uniform_buffers.get_mut(ubo.0) else {
            stale(
                "uniform buffer",
                &format!("block \"{}\"", layout.blocks[i]),
                ubo,
            );
            continue;
        };
        // The UBO's bytes can be dirty while the binding is unchanged, so flush
        // unconditionally (a no-op when clean).
        obj.flush(gl);
        let size = range.map_or(obj.byte_size(), |(_, s)| s as u32);
        let point = program.block_point(bi, size);
        if bindings.bind_block(gl, point, ubo, range, obj) {
            stats.blocks += 1;
        }
    }
}

// What's actually bound to each texture unit / uniform-buffer binding point.
// These are *global* GL state, not program state: different programs (and
// draws) put different textures on the same unit, so this is keyed by
// unit/point. It survives program switches, so an unchanged binding isn't
// re-issued after `useProgram`.
//
// Handles carry a generation, so a destroyed object (which GL unbinds) never
// compares equal to whatever reuses its slot.
#[derive(Default)]
struct GlBindings {
    active_unit: Option<u32>,
    units: Vec<TextureId>,
    blocks: Vec<Option<BoundBlock>>,
}

// A buffer bound to a uniform-buffer point, with its sub-range (None = whole).
type BoundBlock = (UniformBufferId, Option<(i32, i32)>);

impl GlBindings {
    fn forget_active_unit(&mut self) {
        match self.active_unit {
            Some(u) => {
                if let Some(t) = self.units.get_mut(u as usize) {
                    *t = TextureId::NULL;
                }
            }
            None => self.units.clear(),
        }
    }

    // True if it issued a bind.
    fn bind_texture(
        &mut self,
        gl: &WebGl2RenderingContext,
        unit: u32,
        id: TextureId,
        obj: &TextureObj,
    ) -> bool {
        let idx = unit as usize;
        if self.units.get(idx) == Some(&id) {
            return false;
        }
        if self.active_unit != Some(unit) {
            gl.active_texture(WebGl2RenderingContext::TEXTURE0 + unit);
            self.active_unit = Some(unit);
        }
        gl.bind_texture(obj.target_gl(), Some(&obj.raw));
        if idx >= self.units.len() {
            self.units.resize(idx + 1, TextureId::NULL);
        }
        self.units[idx] = id;
        true
    }

    // True if it issued a bind.
    fn bind_block(
        &mut self,
        gl: &WebGl2RenderingContext,
        point: u32,
        id: UniformBufferId,
        range: Option<(i32, i32)>,
        obj: &UniformBufferObj,
    ) -> bool {
        let idx = point as usize;
        if self.blocks.get(idx) == Some(&Some((id, range))) {
            return false;
        }
        match range {
            Some((offset, size)) => gl.bind_buffer_range_with_i32_and_i32(
                WebGl2RenderingContext::UNIFORM_BUFFER,
                point,
                Some(&obj.raw),
                offset,
                size,
            ),
            None => gl.bind_buffer_base(
                WebGl2RenderingContext::UNIFORM_BUFFER,
                point,
                Some(&obj.raw),
            ),
        }
        if idx >= self.blocks.len() {
            self.blocks.resize(idx + 1, None);
        }
        self.blocks[idx] = Some((id, range));
        true
    }
}
