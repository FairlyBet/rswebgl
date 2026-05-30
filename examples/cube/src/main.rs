use std::cell::RefCell;
use std::rc::Rc;

use glam::{Mat4, Vec3};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use rswebgl::buffer::{BufferTarget, BufferUsage};
use rswebgl::context::Context;
use rswebgl::draw::{DrawCommand, DrawMode, IndexType, Viewport};
use rswebgl::render_state::{DepthFunc, RenderState};
use rswebgl::uniform_values::UniformValues;
use rswebgl::vao::VertexAttr;

const VERT: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_color;
uniform mat4 u_mvp;
out vec3 v_color;
void main() {
    gl_Position = u_mvp * vec4(a_pos, 1.0);
    v_color = a_color;
}
"#;

const FRAG: &str = r#"#version 300 es
precision highp float;
in vec3 v_color;
out vec4 frag_color;
void main() {
    frag_color = vec4(v_color, 1.0);
}
"#;

fn bytes<T>(slice: &[T]) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(
            slice.as_ptr() as *const u8,
            std::mem::size_of_val(slice),
        )
    }
}

fn main() {
    let window = web_sys::window().expect("no window");
    let document = window.document().expect("no document");
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("c")
        .expect("missing #c canvas")
        .dyn_into()
        .expect("not a canvas");

    let w = canvas.width() as i32;
    let h = canvas.height() as i32;

    let ctx = Context::from_canvas(&canvas).expect("ctx");
    let mut program = ctx.create_program(VERT, FRAG).expect("program");

    #[rustfmt::skip]
    let verts: [f32; 48] = [
        // pos              // color
        -0.5, -0.5, -0.5,   0.0, 0.0, 0.0,
         0.5, -0.5, -0.5,   1.0, 0.0, 0.0,
         0.5,  0.5, -0.5,   1.0, 1.0, 0.0,
        -0.5,  0.5, -0.5,   0.0, 1.0, 0.0,
        -0.5, -0.5,  0.5,   0.0, 0.0, 1.0,
         0.5, -0.5,  0.5,   1.0, 0.0, 1.0,
         0.5,  0.5,  0.5,   1.0, 1.0, 1.0,
        -0.5,  0.5,  0.5,   0.0, 1.0, 1.0,
    ];

    #[rustfmt::skip]
    let indices: [u16; 36] = [
        0,2,1, 0,3,2, // back  (-z)
        4,5,6, 4,6,7, // front (+z)
        0,4,7, 0,7,3, // left  (-x)
        1,2,6, 1,6,5, // right (+x)
        0,1,5, 0,5,4, // bottom(-y)
        3,7,6, 3,6,2, // top   (+y)
    ];

    let vbo = ctx
        .create_buffer(BufferTarget::Array, BufferUsage::StaticDraw, bytes(&verts))
        .expect("vbo");
    let ibo = ctx
        .create_buffer(
            BufferTarget::ElementArray,
            BufferUsage::StaticDraw,
            bytes(&indices),
        )
        .expect("ibo");

    let mut vao = ctx.create_vertex_array().expect("vao");
    let stride = (6 * std::mem::size_of::<f32>()) as i32;

    let mut pos = VertexAttr::float(3);
    pos.stride = stride;
    pos.offset = 0;
    let mut col = VertexAttr::float(3);
    col.stride = stride;
    col.offset = (3 * std::mem::size_of::<f32>()) as i32;

    vao.attr(0, &vbo, &pos);
    vao.attr(1, &vbo, &col);
    vao.set_index_buffer(&ibo);

    let mut render_state = RenderState::new();
    render_state.depth_test = true;
    render_state.depth_func = DepthFunc::Less;

    let renderer = ctx.renderer();
    renderer.set_default_viewport(Viewport::new(0, 0, w, h));

    let aspect = w as f32 / h as f32;
    let proj = Mat4::perspective_rh_gl(60f32.to_radians(), aspect, 0.1, 100.0);
    let view = Mat4::from_translation(Vec3::new(0.0, 0.0, -3.0));

    let draw_cmd = DrawCommand::elements(DrawMode::Triangles, 36, IndexType::UnsignedShort, 0);

    let gl = ctx.gl();
    let mut uniforms = UniformValues::new();

    // requestAnimationFrame loop with self-reference via Rc<RefCell<Option<Closure>>>.
    let f: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    let win = window.clone();

    *g.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::new(move |time: f64| {
        let t = time as f32 / 1000.0;
        let model = Mat4::from_rotation_y(t) * Mat4::from_rotation_x(t * 0.7);
        let mvp = proj * view * model;
        uniforms.set_mat4("u_mvp", false, &mvp.to_cols_array());

        gl.clear_color(0.05, 0.05, 0.08, 1.0);
        gl.clear(
            WebGl2RenderingContext::COLOR_BUFFER_BIT | WebGl2RenderingContext::DEPTH_BUFFER_BIT,
        );

        renderer.draw(
            &render_state,
            &mut program,
            Some(vao.clone()),
            &uniforms,
            draw_cmd,
            None,
        );

        let _ = win.request_animation_frame(
            f.borrow().as_ref().unwrap().as_ref().unchecked_ref(),
        );
    }));

    let _ = window
        .request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref());
}
