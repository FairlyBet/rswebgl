// Renderer state-tracking tests against a real WebGL2 context.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use rswebgl::buffer::BufferUsage;
use rswebgl::context::{Context, ContextOptions};
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::framebuffer::ClearMask;
use rswebgl::pass::{Batch, Pass};
use rswebgl::render_state::RenderState;
use rswebgl::texture::{Texture, TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};
use rswebgl::uniform_buffer::{Std140Type, UboLayout, UniformBuffer};
use rswebgl::uniform_values::UniformValues;
use rswebgl::vao::VertexArray;
use rswebgl::vao_builder::{AttrKind, VaoBuilder};

wasm_bindgen_test_configure!(run_in_browser);

const W: i32 = 300;
const H: i32 = 100;

const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

// Draws a unit quad into the rect `u_rect` (clip space: x0, y0, x1, y1).
const VERT: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec2 a_pos;
uniform vec4 u_rect;
void main() {
    gl_Position = vec4(mix(u_rect.xy, u_rect.zw, a_pos), 0.0, 1.0);
}
"#;

const SAMPLER_FRAG: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_a;
uniform sampler2D u_b;
uniform int u_mode;
out vec4 frag_color;
void main() {
    frag_color = u_mode == 0 ? texture(u_a, vec2(0.5)) : texture(u_b, vec2(0.5));
}
"#;

const BLOCK_FRAG: &str = r#"#version 300 es
precision highp float;
uniform BlockA { vec4 a_color; };
uniform BlockB { vec4 b_color; };
uniform int u_mode;
out vec4 frag_color;
void main() {
    frag_color = u_mode == 0 ? a_color : b_color;
}
"#;

struct Fixture {
    ctx: Context,
    gl: WebGl2RenderingContext,
    quad: VertexArray,
}

fn fixture() -> Fixture {
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

fn solid_texture(ctx: &Context, c: [u8; 4]) -> Texture {
    let t = ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )
        .expect("texture");
    t.storage_2d(1, &TextureFormat::rgba8(), 1, 1);
    t.sub_image_2d(0, 0, 0, 1, 1, &TextureFormat::rgba8(), &c);
    t
}

fn color_block(ctx: &Context, field: &str, c: [u8; 4]) -> UniformBuffer {
    let layout = UboLayout::new().field(field, Std140Type::Vec4);
    let ubo = ctx
        .create_uniform_buffer(&layout, BufferUsage::StaticDraw)
        .expect("ubo");
    let f = |x: u8| x as f32 / 255.0;
    ubo.set_vec4(field, f(c[0]), f(c[1]), f(c[2]), f(c[3]));
    ubo
}

// Uniforms for quad `col` of three side-by-side quads.
fn quad_uniforms(col: i32, mode: i32) -> UniformValues {
    let u = UniformValues::new();
    let x0 = -1.0 + col as f32 * (2.0 / 3.0) + 0.05;
    u.set_vec4("u_rect", x0, -0.9, x0 + 2.0 / 3.0 - 0.1, 0.9);
    u.set_int("u_mode", mode);
    u
}

fn render(f: &Fixture, batch: Batch) {
    let mut pass = Pass::to_default();
    pass.set_clear(ClearMask::color());
    pass.add(batch);
    f.ctx.renderer().render(&pass);
}

// Center pixel of each of the three quads.
fn read_quads(gl: &WebGl2RenderingContext) -> [[u8; 4]; 3] {
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

// Units are assigned per UniformValues, so the same unit ends up holding
// different textures under different names across draws. The third draw must
// rebind RED to unit 0 even though "u_a"@0=RED is what an earlier draw set.
#[wasm_bindgen_test]
fn sampler_units_rebind_across_draws() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, SAMPLER_FRAG).expect("program");
    let red = solid_texture(&f.ctx, RED);
    let green = solid_texture(&f.ctx, GREEN);
    let blue = solid_texture(&f.ctx, BLUE);
    let cmd = DrawCommand::elements(DrawMode::Triangles, 6, IndexType::UnsignedShort, 0);

    let mut batch = Batch::new(&program, &RenderState::new());
    let u = quad_uniforms(0, 0);
    u.set_sampler("u_a", &red); // unit 0
    u.set_sampler("u_b", &green); // unit 1
    batch.draw(&f.quad, &u, cmd);
    let u = quad_uniforms(1, 1);
    u.set_sampler("u_b", &blue); // unit 0
    batch.draw(&f.quad, &u, cmd);
    let u = quad_uniforms(2, 0);
    u.set_sampler("u_a", &red); // unit 0
    batch.draw(&f.quad, &u, cmd);
    render(&f, batch);

    assert_eq!(read_quads(&f.gl), [RED, BLUE, RED]);
}

// Same shape for uniform-buffer binding points.
#[wasm_bindgen_test]
fn block_points_rebind_across_draws() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, BLOCK_FRAG).expect("program");
    let a_red = color_block(&f.ctx, "a_color", RED);
    let b_green = color_block(&f.ctx, "b_color", GREEN);
    let b_blue = color_block(&f.ctx, "b_color", BLUE);
    let cmd = DrawCommand::elements(DrawMode::Triangles, 6, IndexType::UnsignedShort, 0);

    let mut batch = Batch::new(&program, &RenderState::new());
    let u = quad_uniforms(0, 0);
    u.set_uniform_block("BlockA", &a_red); // point 0
    u.set_uniform_block("BlockB", &b_green); // point 1
    batch.draw(&f.quad, &u, cmd);
    let u = quad_uniforms(1, 1);
    u.set_uniform_block("BlockB", &b_blue); // point 0
    batch.draw(&f.quad, &u, cmd);
    let u = quad_uniforms(2, 0);
    u.set_uniform_block("BlockA", &a_red); // point 0
    batch.draw(&f.quad, &u, cmd);
    render(&f, batch);

    assert_eq!(read_quads(&f.gl), [RED, BLUE, RED]);
}
