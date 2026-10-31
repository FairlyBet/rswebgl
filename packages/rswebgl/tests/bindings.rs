// Renderer state-tracking tests against a real WebGL2 context.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::buffer::BufferUsage;
use rswebgl::context::Context;
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::pass::Batch;
use rswebgl::render_state::RenderState;
use rswebgl::texture::{Texture, TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};
use rswebgl::uniform_buffer::{Std140Type, UboLayout, UniformBuffer};

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

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
    render(&f, vec![batch]);

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
    render(&f, vec![batch]);

    assert_eq!(read_quads(&f.gl), [RED, BLUE, RED]);
}
