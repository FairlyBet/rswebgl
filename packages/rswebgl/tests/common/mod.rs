// Shared fixture for the browser tests: a small offscreen canvas, a unit quad,
// and readback of the centers of three side-by-side quads.
#![allow(dead_code)]

use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use rswebgl::context::{Context, ContextOptions};
use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::VertexArrayId;
use rswebgl::pass::{Batch, Pass};
use rswebgl::uniform_values::UniformValues;
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
    Fixture { ctx, gl, quad }
}

// Uniforms for quad `col` of three side-by-side quads.
pub fn quad_uniforms(col: i32, mode: i32) -> UniformValues {
    let u = UniformValues::new();
    let x0 = -1.0 + col as f32 * (2.0 / 3.0) + 0.05;
    u.set_vec4("u_rect", x0, -0.9, x0 + 2.0 / 3.0 - 0.1, 0.9);
    u.set_int("u_mode", mode);
    u
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
