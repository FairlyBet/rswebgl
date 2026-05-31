use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::draw::{DrawCommand, Viewport};
use crate::framebuffer::{
    ClearMask, DefaultFramebuffer, Framebuffer, InvalidateMask, framebuffer_status_str,
};
use crate::program::Program;
use crate::render_state::RenderState;
use crate::uniform_values::UniformValues;
use crate::vao::VertexArray;

struct RendererInner {
    gl: WebGl2RenderingContext,
    default_fb: DefaultFramebuffer,
    // None = the default (canvas) framebuffer is bound; Some = a user FBO.
    bound_fb: Option<Framebuffer>,
    prev_program: Option<Program>,
    prev_render_state: Option<RenderState>,
    prev_vao: Option<Option<VertexArray>>,
    prev_uniforms: Option<UniformValues>,
    prev_viewport: Option<Viewport>,
}

#[wasm_bindgen]
pub struct Renderer {
    inner: Rc<RefCell<RendererInner>>,
}

impl Renderer {
    pub(crate) fn new(gl: WebGl2RenderingContext, default_fb: DefaultFramebuffer) -> Self {
        Self {
            inner: Rc::new(RefCell::new(RendererInner {
                gl,
                default_fb,
                bound_fb: None,
                prev_program: None,
                prev_render_state: None,
                prev_vao: None,
                prev_uniforms: None,
                prev_viewport: None,
            })),
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
    /// Binds a render target. `None` restores the default (canvas) framebuffer.
    /// The bound target is state-tracked, so redundant calls are skipped, and
    /// `clear` / `invalidate` / `draw` all act on whatever is bound here.
    pub fn set_framebuffer(&self, fb: Option<Framebuffer>) {
        let mut s = self.inner.borrow_mut();
        let same = match (&s.bound_fb, &fb) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if !same {
            match &fb {
                Some(f) => {
                    s.gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&f.raw_gl()))
                }
                None => {
                    s.gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None)
                }
            }
            s.bound_fb = fb;
        }
        // Apply any attachment changes recorded since the last bind — the FBO is
        // now bound, so this is where lazy attach actually hits GL.
        if let Some(f) = &s.bound_fb {
            f.realize_if_dirty(&s.gl);
        }
    }

    /// Checks framebuffer completeness. Binds `fb`, realizes pending attachments,
    /// reads the status, then restores the previously bound target — so it never
    /// disturbs the Renderer's tracked binding.
    pub fn check_framebuffer(&self, fb: &Framebuffer) -> Result<(), String> {
        let s = self.inner.borrow();
        let gl = s.gl.clone();
        gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&fb.raw_gl()));
        fb.realize_if_dirty(&gl);
        let status = gl.check_framebuffer_status(WebGl2RenderingContext::FRAMEBUFFER);
        match &s.bound_fb {
            Some(p) => gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&p.raw_gl())),
            None => gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, None),
        }
        if status == WebGl2RenderingContext::FRAMEBUFFER_COMPLETE {
            Ok(())
        } else {
            Err(framebuffer_status_str(status).to_string())
        }
    }

    pub fn is_framebuffer_complete(&self, fb: &Framebuffer) -> bool {
        self.check_framebuffer(fb).is_ok()
    }

    pub fn clear(&self, mask: ClearMask) {
        let s = self.inner.borrow();
        let gl = &s.gl;

        match &s.bound_fb {
            // User FBO: per-attachment, correctly typed clearBuffer* calls.
            Some(fb) => {
                fb.realize_if_dirty(gl);
                fb.clear(gl, mask);
            }
            // Default (canvas) framebuffer: single color buffer, classic path.
            None => {
                let c = s.default_fb.clear_color_rgba();
                if mask.color {
                    gl.clear_color(c[0], c[1], c[2], c[3]);
                }
                if mask.depth {
                    gl.clear_depth(s.default_fb.clear_depth_value());
                }
                if mask.stencil {
                    gl.clear_stencil(s.default_fb.clear_stencil_value());
                }
                gl.clear(mask.as_gl());
            }
        }
    }

    /// Discards the selected attachments of the currently bound framebuffer.
    /// Only attachments that actually exist are invalidated, and the correct
    /// attachment-point enums are chosen automatically (default vs user FBO).
    pub fn invalidate(&self, mask: InvalidateMask) {
        let s = self.inner.borrow();
        if let Some(fb) = &s.bound_fb {
            fb.realize_if_dirty(&s.gl);
        }
        let arr = js_sys::Array::new();
        match &s.bound_fb {
            Some(fb) => fb.collect_invalidate_attachments(&arr, mask),
            None => {
                if mask.color {
                    arr.push(&JsValue::from(WebGl2RenderingContext::COLOR));
                }
                if mask.depth {
                    arr.push(&JsValue::from(WebGl2RenderingContext::DEPTH));
                }
                if mask.stencil {
                    arr.push(&JsValue::from(WebGl2RenderingContext::STENCIL));
                }
            }
        }
        let _ =
            s.gl.invalidate_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, arr.as_ref());
    }

    pub fn draw(
        &self,
        render_state: &RenderState,
        program: &mut Program,
        vao: Option<VertexArray>,
        uniforms: &UniformValues,
        draw: DrawCommand,
        viewport: Option<Viewport>,
    ) {
        let mut s = self.inner.borrow_mut();
        let gl = s.gl.clone();

        // 0. Realize any lazily-recorded attachments on the bound FBO.
        if let Some(fb) = &s.bound_fb {
            fb.realize_if_dirty(&gl);
        }

        // 1. Viewport — defaults to the full size of the bound render target.
        let vp = viewport.unwrap_or_else(|| match &s.bound_fb {
            Some(fb) => fb.viewport(),
            None => s.default_fb.viewport(),
        });
        if s.prev_viewport != Some(vp) {
            gl.viewport(vp.x, vp.y, vp.width, vp.height);
            s.prev_viewport = Some(vp);
        }

        // 2. Render state
        match &s.prev_render_state {
            Some(p) => render_state.apply_diff(p, &gl),
            None => render_state.apply(&gl),
        }
        s.prev_render_state = Some(render_state.clone());

        // 3. Program (must precede uniform uploads)
        if s.prev_program.as_ref() != Some(program) {
            gl.use_program(Some(program.raw_gl()));
            s.prev_program = Some(program.clone());
        }

        // 4. VAO — None must be bound explicitly (e.g. procedural vertices from
        // gl_VertexID), so we track "never bound" separately from "bound to None".
        let vao_same = match (&s.prev_vao, &vao) {
            (Some(Some(p)), Some(v)) => p == v,
            (Some(None), None) => true,
            _ => false,
        };
        if !vao_same {
            gl.bind_vertex_array(vao.as_ref().map(|v| v.raw_gl()));
            s.prev_vao = Some(vao);
        }

        // 5. Uniforms (after useProgram)
        match &s.prev_uniforms {
            Some(prev) => uniforms.upload_diff(prev, program),
            None => uniforms.upload(program),
        }
        // TODO(perf): UniformValues::clone is O(N) — clones Vec + each Box<str> + each
        // SmallVec. For typical 30-50 uniforms this is meaningful per-draw. Consider
        // Rc<UniformValues> for cheap pointer-eq fast path, or a version counter on
        // UniformValues to detect "same data passed again" without comparison.
        s.prev_uniforms = Some(uniforms.clone());

        // 6. Draw
        draw.execute(&gl);
    }
}
