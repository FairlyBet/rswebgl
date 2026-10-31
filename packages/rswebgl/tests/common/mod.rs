// Shared fixture for the browser tests: a small offscreen canvas, a unit quad,
// and readback of the centers of three side-by-side quads.
#![allow(dead_code)]

use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use rswebgl::context::{Context, ContextOptions};
use rswebgl::draw::DrawCommand;
use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::VertexArrayId;
use rswebgl::ids::{ParamGroupId, ParamLayoutId};
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::pass::{Batch, Pass};
use rswebgl::uniform_buffer::UniformType;
use rswebgl::vao_builder::{AttrKind, VaoBuilder};

pub const W: i32 = 300;
pub const H: i32 = 100;

pub const BLACK: [u8; 4] = [0, 0, 0, 255];
pub const RED: [u8; 4] = [255, 0, 0, 255];
pub const GREEN: [u8; 4] = [0, 255, 0, 255];
pub const BLUE: [u8; 4] = [0, 0, 255, 255];

// Draws the unit quad into the rect `u_rect` (clip space: x0, y0, x1, y1).
pub const VERT: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_pos;
uniform vec4 u_rect;
void main() {
    gl_Position = vec4(mix(u_rect.xy, u_rect.zw, a_pos), 0.0, 1.0);
}
"#;

pub struct Fixture {
    pub ctx: Context,
    pub gl: WebGl2RenderingContext,
    pub quad: VertexArrayId,
    // `u_rect` (vec4): where a quad goes. Lives in group slot QUAD_SLOT.
    pub quad_layout: ParamLayoutId,
}

pub fn fixture() -> Fixture {
    let document = web_sys::window().unwrap().document().unwrap();
    let canvas: HtmlCanvasElement = document
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    canvas.set_width(W as u32);
    canvas.set_height(H as u32);

    let mut options = ContextOptions::new();
    options.antialias = false;
    let ctx = Context::from_canvas(&canvas, &options).expect("ctx");
    // Same context object the library uses, for readback only.
    let gl = canvas
        .get_context("webgl2")
        .unwrap()
        .unwrap()
        .dyn_into()
        .unwrap();

    let mut vb = VaoBuilder::new(&ctx);
    vb.add_f32(
        0,
        AttrKind::F32x2,
        &[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
    );
    vb.add_indices_u16(&[0, 1, 2, 0, 2, 3]);
    let quad = vb.build().expect("vao");
    let quad_layout = ctx
        .create_param_layout(&ParamLayoutBuilder::new().uniform("u_rect", UniformType::Vec4))
        .expect("quad layout");
    Fixture {
        ctx,
        gl,
        quad,
        quad_layout,
    }
}

/// Group slot the quad placement goes in (the tests use 0..2 for their own).
pub const QUAD_SLOT: u32 = 3;

// A group placing a quad at column `col` of three side-by-side quads.
pub fn quad(f: &Fixture, col: i32) -> ParamGroupId {
    let g = f.ctx.create_param_group(f.quad_layout).expect("quad group");
    let x0 = -1.0 + col as f32 * (2.0 / 3.0) + 0.05;
    f.ctx
        .param_set_floats_by_name(g, "u_rect", &[x0, -0.9, x0 + 2.0 / 3.0 - 0.1, 0.9]);
    g
}

// Adds a draw of the quad at column `col`.
pub fn draw_quad(batch: &mut Batch, f: &Fixture, col: i32) {
    let cmd = DrawCommand::elements(
        rswebgl::draw::DrawMode::Triangles,
        6,
        rswebgl::draw::IndexType::UnsignedShort,
        0,
    );
    batch.draw_with_group(f.quad, QUAD_SLOT, quad(f, col), cmd);
}

// One pass to the canvas, cleared to black first.
pub fn render(f: &Fixture, batches: Vec<Batch>) {
    let mut pass = Pass::to_default();
    pass.set_clear(ClearMask::color());
    for b in batches {
        pass.add(b);
    }
    f.ctx.render(&pass);
}

// Center pixel of each of the three quads. Also asserts no GL error is pending.
pub fn read_quads(gl: &WebGl2RenderingContext) -> [[u8; 4]; 3] {
    let mut out = [[0u8; 4]; 3];
    for (col, px) in out.iter_mut().enumerate() {
        gl.read_pixels_with_opt_u8_array(
            W / 6 + col as i32 * W / 3,
            H / 2,
            1,
            1,
            WebGl2RenderingContext::RGBA,
            WebGl2RenderingContext::UNSIGNED_BYTE,
            Some(&mut px[..]),
        )
        .unwrap();
    }
    assert_eq!(gl.get_error(), WebGl2RenderingContext::NO_ERROR);
    out
}

// Messages logged via `console.<method>` ("warn", "error") while `f` runs.
pub fn capture_console(method: &str, f: impl FnOnce()) -> Vec<String> {
    let console = js_sys::Reflect::get(&js_sys::global(), &"console".into()).unwrap();
    let original = js_sys::Reflect::get(&console, &method.into()).unwrap();
    let log = js_sys::Array::new();
    let push = js_sys::Function::new_with_args("m", "this.push(String(m))").bind0(&log);
    js_sys::Reflect::set(&console, &method.into(), &push).unwrap();
    f();
    js_sys::Reflect::set(&console, &method.into(), &original).unwrap();
    log.iter().filter_map(|v| v.as_string()).collect()
}

// The quad's draw command (6 indices).
pub fn quad_cmd() -> DrawCommand {
    DrawCommand::elements(
        rswebgl::draw::DrawMode::Triangles,
        6,
        rswebgl::draw::IndexType::UnsignedShort,
        0,
    )
}
