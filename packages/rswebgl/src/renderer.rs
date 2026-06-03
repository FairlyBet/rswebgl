use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::draw::Viewport;
use crate::framebuffer::{
    ClearMask, DefaultFramebuffer, Framebuffer, InvalidateMask, framebuffer_status_str,
};
use crate::pass::Pass;
use crate::program::Program;
use crate::render_state::RenderState;
use crate::uniform_values::UniformValues;
use crate::vao::VertexArray;

struct RendererInner {
    gl: WebGl2RenderingContext,
    default_fb: DefaultFramebuffer,
}

#[wasm_bindgen]
pub struct Renderer {
    inner: Rc<RefCell<RendererInner>>,
}

impl Renderer {
    pub(crate) fn new(gl: WebGl2RenderingContext, default_fb: DefaultFramebuffer) -> Self {
        Self {
            inner: Rc::new(RefCell::new(RendererInner { gl, default_fb })),
        }
    }

    pub(crate) fn handle(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

#[wasm_bindgen]
impl Renderer {
    /// Executes one render pass. State is tracked only for the duration of this
    /// call — nothing about the bound program/VAO/framebuffer is assumed on entry,
    /// so anything a user object did to global GL state in between can't desync us.
    pub fn render(&self, pass: &Pass) {
        let s = self.inner.borrow();
        let mut tracker = StateTracker::new(&s.gl);
        execute_pass(&s, pass, &mut tracker);
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
    pub fn check_framebuffer(&self, fb: &Framebuffer) -> Result<(), String> {
        let s = self.inner.borrow();
        let gl = &s.gl;
        gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&fb.raw_gl()));
        let status = gl.check_framebuffer_status(WebGl2RenderingContext::FRAMEBUFFER);
        gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None);
        if status == WebGl2RenderingContext::FRAMEBUFFER_COMPLETE {
            Ok(())
        } else {
            Err(framebuffer_status_str(status).to_string())
        }
    }

    pub fn is_framebuffer_complete(&self, fb: &Framebuffer) -> bool {
        self.check_framebuffer(fb).is_ok()
    }
}

impl Renderer {
    /// Executes several passes sharing one state tracker, so redundant binds
    /// across pass boundaries (same program/VAO/render state) are skipped.
    /// Rust-only: a `&[Pass]` doesn't cross the wasm_bindgen boundary.
    pub fn render_passes(&self, passes: &[Pass]) {
        let s = self.inner.borrow();
        let mut tracker = StateTracker::new(&s.gl);
        for pass in passes {
            execute_pass(&s, pass, &mut tracker);
        }
    }
}

// ---------------------------------------------------------------------------
// Pass execution
// ---------------------------------------------------------------------------

fn execute_pass(s: &RendererInner, pass: &Pass, t: &mut StateTracker) {
    let gl = &s.gl;

    // Render target — bind whatever this pass draws into.
    t.bind_target(&pass.target);

    // Viewport — the pass's override, else the target's full size.
    let vp = pass.viewport.unwrap_or_else(|| match &pass.target {
        Some(fb) => fb.viewport(),
        None => s.default_fb.viewport(),
    });
    t.set_viewport(vp);

    // Load op — clear before drawing. Clearing obeys the write masks and scissor,
    // so force them open first; that dirties render state, so the next batch is
    // made to re-apply it in full.
    if pass.clear != ClearMask::none() {
        t.open_for_clear(pass.clear);
        match &pass.target {
            Some(fb) => fb.clear(gl, pass.clear),
            None => clear_default(&s.default_fb, gl, pass.clear),
        }
        t.invalidate_render_state();
    }

    // Draw — each batch shares a program + render state across its draws.
    for batch in &pass.batches {
        t.apply_render_state(&batch.render_state);
        t.use_program(&batch.program);
        for d in &batch.draws {
            t.bind_vao(&d.vao);
            t.upload_uniforms(&d.uniforms, &batch.program);
            d.command.execute(gl);
        }
    }

    // Store op — discard attachments we won't read again.
    if pass.invalidate != InvalidateMask::none() {
        let arr = js_sys::Array::new();
        match &pass.target {
            Some(fb) => fb.collect_invalidate_attachments(&arr, pass.invalidate),
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
    // Outer Option = tracked yet; inner Option = a user FBO vs the default.
    target: Option<Option<Framebuffer>>,
    viewport: Option<Viewport>,
    render_state: Option<RenderState>,
    program: Option<Program>,
    // Outer = tracked yet; inner = bound VAO vs explicitly bound to None.
    vao: Option<Option<VertexArray>>,
    // What's currently set in the active program. Persisted across draws and diffed
    // in place (no per-draw clone); cleared on program switch since locations are
    // per-program.
    applied_uniforms: UniformValues,
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
        }
    }

    fn bind_target(&mut self, target: &Option<Framebuffer>) {
        let same = match &self.target {
            Some(cur) => fb_eq(cur, target),
            None => false,
        };
        if !same {
            match target {
                Some(fb) => self
                    .gl
                    .bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&fb.raw_gl())),
                None => self
                    .gl
                    .bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None),
            }
            self.target = Some(target.clone());
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

    fn use_program(&mut self, program: &Program) {
        if self.program.as_ref() != Some(program) {
            self.gl.use_program(Some(program.raw_gl()));
            self.program = Some(program.clone());
            // Uniform locations are per-program; drop the applied cache (in place,
            // keeping capacity) so the next draw re-uploads everything it needs.
            self.applied_uniforms.clear();
        }
    }

    fn bind_vao(&mut self, vao: &Option<VertexArray>) {
        let same = match (&self.vao, vao) {
            (Some(Some(prev)), Some(v)) => prev == v,
            (Some(None), None) => true,
            _ => false,
        };
        if !same {
            self.gl.bind_vertex_array(vao.as_ref().map(|v| v.raw_gl()));
            self.vao = Some(vao.clone());
        }
    }

    fn upload_uniforms(&mut self, uniforms: &UniformValues, program: &Program) {
        self.applied_uniforms.sync_from(uniforms, program);
    }
}

fn fb_eq(a: &Option<Framebuffer>, b: &Option<Framebuffer>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}
