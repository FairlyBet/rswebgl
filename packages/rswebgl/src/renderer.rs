// Pass execution: walks a Pass → Batch → Draw tree and issues the GL calls,
// diffing state between consecutive items. Resources are resolved from the
// context's pools by handle; a handle that went stale since the pass was built
// is reported once and its pass / batch / draw skipped.

use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::context::{Context, Resources, stale};
use crate::draw::Viewport;
use crate::framebuffer::{ClearMask, DefaultFramebuffer, InvalidateMask, framebuffer_status_str};
use crate::ids::{FramebufferId, ProgramId, VertexArrayId};
use crate::pass::Pass;
use crate::render_state::RenderState;
use crate::uniform_values::{GlBindings, SyncTarget, UniformValues};

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
            // Batch first, then the draw's own (overrides win). Re-syncing the
            // batch per draw restores any value the previous draw overrode;
            // unchanged ones cost a compare, not a GL call.
            let mut sync = SyncTarget {
                gl,
                program,
                textures,
                uniform_buffers,
            };
            t.upload_uniforms(&batch.uniforms, &mut sync);
            t.upload_uniforms(&d.uniforms, &mut sync);
            program.check_uniforms_set(gl, &[&batch.uniforms, &d.uniforms]);
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
    // What's currently set in the active program. Persisted across draws and diffed
    // in place (no per-draw clone); cleared on program switch since locations are
    // per-program.
    applied_uniforms: UniformValues,
    // What's bound to each texture unit / uniform-buffer point. Global state, so
    // unlike `applied_uniforms` it's kept across program switches.
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
            applied_uniforms: UniformValues::new(),
            bindings: GlBindings::new(),
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
            // Uniform locations are per-program; drop the applied cache (in place,
            // keeping capacity) so the next draw re-uploads everything it needs.
            self.applied_uniforms.clear();
        }
    }

    fn bind_vao(&mut self, id: VertexArrayId, raw: Option<&web_sys::WebGlVertexArrayObject>) {
        if self.vao != Some(id) {
            self.gl.bind_vertex_array(raw);
            self.vao = Some(id);
        }
    }

    fn upload_uniforms(&mut self, uniforms: &UniformValues, sync: &mut SyncTarget<'_>) {
        self.applied_uniforms
            .sync_from(uniforms, sync, &mut self.bindings);
    }
}
