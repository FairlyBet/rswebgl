// Pass execution: walks a Pass → Batch → Draw tree and issues the GL calls,
// diffing state between consecutive items. Resources are resolved from the
// context's pools by handle; a handle that went stale since the pass was built
// is reported once and its pass / batch / draw skipped.

use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::context::{Context, Resources, stale};
use crate::draw::Viewport;
use crate::framebuffer::{ClearMask, DefaultFramebuffer, InvalidateMask, framebuffer_status_str};
use rswebgl_foundation::pool::Pool;

use crate::ids::{FramebufferId, ProgramId, TextureId, UniformBufferId, VertexArrayId};
use crate::param::{GROUP_SLOTS, ParamGroupObj, ParamLayoutObj};
use crate::pass::Pass;
use crate::program::ProgramObj;
use crate::render_state::RenderState;
use crate::texture::TextureObj;
use crate::uniform_buffer::UniformBufferObj;

#[wasm_bindgen]
impl Context {
    /// Executes one render pass. State is tracked only for the duration of this
    /// call — nothing about the bound program/VAO/framebuffer is assumed on
    /// entry. Does nothing while the context is lost.
    pub fn render(&self, pass: &Pass) {
        self.render_passes(std::slice::from_ref(pass));
    }

    /// Executes several passes under one state tracker, so redundant binds across
    /// pass boundaries (same program/VAO/render state) are skipped. The JS array of
    /// passes is consumed. (Rust callers can use `render_passes(&[Pass])` to avoid
    /// giving up ownership.)
    pub fn render_all(&self, passes: Vec<Pass>) {
        self.render_passes(&passes);
    }

    /// Checks framebuffer completeness without disturbing rendering. Binds `fb`,
    /// reads the status, then restores the default framebuffer (the next `render`
    /// re-binds its own target anyway).
    pub fn check_framebuffer(&self, fb: FramebufferId) -> Result<(), String> {
        self.check_alive()?;
        let gl = self.gl();
        let res = self.res();
        let Some(f) = res.framebuffers.get(fb.0) else {
            return Err(format!("framebuffer {:?} is destroyed or invalid", fb.0));
        };
        gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&f.raw));
        let status = gl.check_framebuffer_status(WebGl2RenderingContext::FRAMEBUFFER);
        gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None);
        if status == WebGl2RenderingContext::FRAMEBUFFER_COMPLETE {
            Ok(())
        } else {
            Err(framebuffer_status_str(status).to_string())
        }
    }

    pub fn is_framebuffer_complete(&self, fb: FramebufferId) -> bool {
        self.check_framebuffer(fb).is_ok()
    }
}

impl Context {
    /// Executes several passes sharing one state tracker, so redundant binds
    /// across pass boundaries (same program/VAO/render state) are skipped.
    /// Rust-only: a `&[Pass]` doesn't cross the wasm_bindgen boundary.
    pub fn render_passes(&self, passes: &[Pass]) {
        let gl = self.gl();
        // Every call would be a no-op anyway, but program/framebuffer status
        // queries would report bogus failures (and mark programs invalid).
        if gl.is_context_lost() {
            return;
        }
        let res = &mut *self.res();
        let mut tracker = StateTracker::new(gl);
        for pass in passes {
            execute_pass(gl, &self.core.default_fb, res, pass, &mut tracker);
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
    t: &mut StateTracker,
) {
    let Resources {
        framebuffers,
        programs,
        vertex_arrays,
        textures,
        uniform_buffers,
        param_layouts,
        param_groups,
        ..
    } = res;

    // Render target — bind whatever this pass draws into.
    let target = if pass.target.is_null() {
        None
    } else {
        match framebuffers.get(pass.target.0) {
            Some(f) => Some(f),
            None => return stale("framebuffer", "render (pass target)", pass.target),
        }
    };
    t.bind_target(pass.target, target.map(|f| &f.raw));

    // Viewport — the pass's override, else the target's full size.
    let vp = pass.viewport.unwrap_or_else(|| match target {
        Some(f) => f.viewport,
        None => default_fb.viewport(),
    });
    t.set_viewport(vp);

    // Load op — clear before drawing. Clearing obeys the write masks and scissor,
    // so force them open first; that dirties render state, so the next batch is
    // made to re-apply it in full.
    if pass.clear != ClearMask::none() {
        t.open_for_clear(pass.clear);
        match target {
            Some(f) => f.clear(gl, pass.clear),
            None => clear_default(default_fb, gl, pass.clear),
        }
        t.invalidate_render_state();
    }

    // Draw — each batch shares a program + render state across its draws.
    for batch in &pass.batches {
        let Some(program) = programs.get_mut(batch.program.0) else {
            stale("program", "render (batch)", batch.program);
            continue;
        };
        // A program whose link failed is skipped (its error is reported once).
        // One still linking in the background is waited on, not skipped, so a
        // one-shot pass (a bake, a pack) can't silently draw nothing; callers who
        // want to avoid the stall poll `program_is_ready` before submitting.
        if !program.ensure_linked(gl) {
            continue;
        }
        t.apply_render_state(&batch.render_state);
        t.use_program(batch.program, &program.raw);
        program.activate(gl);
        for d in &batch.draws {
            let vao = if d.vao.is_null() {
                None
            } else {
                match vertex_arrays.get(d.vao.0) {
                    Some(v) => Some(&v.raw),
                    None => {
                        stale("vertex array", "render (draw)", d.vao);
                        continue;
                    }
                }
            };
            t.bind_vao(d.vao, vao);
            // Each slot: the draw's group, else the batch's. Values are compared
            // against what the program holds, so unchanged ones cost a compare,
            // not a GL call.
            let mut used = [0usize; GROUP_SLOTS];
            let mut n_used = 0;
            for slot in 0..GROUP_SLOTS {
                let gid = if d.groups[slot].is_null() {
                    batch.groups[slot]
                } else {
                    d.groups[slot]
                };
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
                    &mut t.bindings,
                );
                used[n_used] = m;
                n_used += 1;
            }
            program.check_covered(&used[..n_used]);
            d.command.execute(gl);
        }
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

// ---------------------------------------------------------------------------
// StateTracker — caches applied GL state for the lifetime of a single
// render/render_passes call. Each slot starts as `None` ("nothing applied yet"),
// so the first use forces a full apply; thereafter only differences hit GL.
// ---------------------------------------------------------------------------

struct StateTracker<'a> {
    gl: &'a WebGl2RenderingContext,
    // FramebufferId::NULL = the default framebuffer.
    target: Option<FramebufferId>,
    viewport: Option<Viewport>,
    render_state: Option<RenderState>,
    program: Option<ProgramId>,
    // VertexArrayId::NULL = explicitly no VAO.
    vao: Option<VertexArrayId>,
    // What's bound to each texture unit / uniform-buffer point. Global state, so
    // it's kept across program switches.
    bindings: GlBindings,
}

impl<'a> StateTracker<'a> {
    fn new(gl: &'a WebGl2RenderingContext) -> Self {
        Self {
            gl,
            target: None,
            viewport: None,
            render_state: None,
            program: None,
            vao: None,
            bindings: GlBindings::default(),
        }
    }

    fn bind_target(&mut self, id: FramebufferId, raw: Option<&web_sys::WebGlFramebuffer>) {
        if self.target != Some(id) {
            self.gl
                .bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, raw);
            self.target = Some(id);
        }
    }

    fn set_viewport(&mut self, vp: Viewport) {
        if self.viewport != Some(vp) {
            self.gl.viewport(vp.x, vp.y, vp.width, vp.height);
            self.viewport = Some(vp);
        }
    }

    // Opens the write masks and scissor the given clear touches, so the clear
    // isn't silently masked off or scissored down by leftover state.
    fn open_for_clear(&self, mask: ClearMask) {
        if mask.color {
            self.gl.color_mask(true, true, true, true);
        }
        if mask.depth {
            self.gl.depth_mask(true);
        }
        if mask.stencil {
            self.gl.stencil_mask(0xFFFF_FFFF);
        }
        self.gl.disable(WebGl2RenderingContext::SCISSOR_TEST);
    }

    // Forces the next render-state apply to be a full one (used after a clear,
    // which mutates masks/scissor behind the tracker's back).
    fn invalidate_render_state(&mut self) {
        self.render_state = None;
    }

    fn apply_render_state(&mut self, rs: &RenderState) {
        match &self.render_state {
            Some(prev) => rs.apply_diff(prev, self.gl),
            None => rs.apply(self.gl),
        }
        self.render_state = Some(rs.clone());
    }

    fn use_program(&mut self, id: ProgramId, raw: &web_sys::WebGlProgram) {
        if self.program != Some(id) {
            self.gl.use_program(Some(raw));
            self.program = Some(id);
        }
    }

    fn bind_vao(&mut self, id: VertexArrayId, raw: Option<&web_sys::WebGlVertexArrayObject>) {
        if self.vao != Some(id) {
            self.gl.bind_vertex_array(raw);
            self.vao = Some(id);
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
) {
    for (i, f) in layout.plain.iter().enumerate() {
        program.upload_field(gl, m, i, f, group);
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
            Some(obj) => bindings.bind_texture(gl, unit, tex, obj),
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
        bindings.bind_block(gl, point, ubo, range, obj);
    }
}

// What's actually bound to each texture unit / uniform-buffer binding point, for
// the lifetime of one render call. These are *global* GL state, not program
// state: different programs (and draws) put different textures on the same unit,
// so this is keyed by unit/point. It survives program switches, so an unchanged
// binding isn't re-issued after `useProgram`.
#[derive(Default)]
struct GlBindings {
    active_unit: Option<u32>,
    units: Vec<TextureId>,
    blocks: Vec<Option<BoundBlock>>,
}

// A buffer bound to a uniform-buffer point, with its sub-range (None = whole).
type BoundBlock = (UniformBufferId, Option<(i32, i32)>);

impl GlBindings {
    fn bind_texture(
        &mut self,
        gl: &WebGl2RenderingContext,
        unit: u32,
        id: TextureId,
        obj: &TextureObj,
    ) {
        let idx = unit as usize;
        if self.units.get(idx) == Some(&id) {
            return;
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
    }

    fn bind_block(
        &mut self,
        gl: &WebGl2RenderingContext,
        point: u32,
        id: UniformBufferId,
        range: Option<(i32, i32)>,
        obj: &UniformBufferObj,
    ) {
        let idx = point as usize;
        if self.blocks.get(idx) == Some(&Some((id, range))) {
            return;
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
    }
}
