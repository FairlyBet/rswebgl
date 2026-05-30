use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::draw::{DrawCommand, Viewport};
use crate::framebuffer::{ClearMask, DefaultFramebuffer};
use crate::program::Program;
use crate::render_state::RenderState;
use crate::uniform_values::UniformValues;
use crate::vao::VertexArray;

struct RendererInner {
    gl: WebGl2RenderingContext,
    default_fb: DefaultFramebuffer,
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
    pub fn clear(&self, mask: ClearMask) {
        let s = self.inner.borrow();
        let gl = &s.gl;
        let fb = &s.default_fb;

        if mask.color {
            let c = fb.clear_color_rgba();
            gl.clear_color(c[0], c[1], c[2], c[3]);
        }
        if mask.depth {
            gl.clear_depth(fb.clear_depth_value());
        }
        if mask.stencil {
            gl.clear_stencil(fb.clear_stencil_value());
        }
        gl.clear(mask.as_gl());
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

        // 1. Viewport
        let vp = viewport.unwrap_or_else(|| s.default_fb.viewport());
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
