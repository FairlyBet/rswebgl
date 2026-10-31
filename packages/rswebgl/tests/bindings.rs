// Renderer state-tracking tests against a real WebGL2 context.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::buffer::BufferUsage;
use rswebgl::context::Context;
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::ids::{ProgramId, TextureId, UniformBufferId};
use rswebgl::pass::Batch;
use rswebgl::render_state::RenderState;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};
use rswebgl::uniform_buffer::{Std140Type, UboLayout};
use rswebgl::uniform_values::UniformValues;

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

// Each program has a single sampler / block, so each puts it on unit 0 /
// point 0: switching programs puts different objects in the same slot.
const SAMPLER_A_FRAG: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_a;
out vec4 frag_color;
void main() {
    frag_color = texture(u_a, vec2(0.5));
}
"#;

const SAMPLER_B_FRAG: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_b;
out vec4 frag_color;
void main() {
    frag_color = texture(u_b, vec2(0.5));
}
"#;

const BLOCK_A_FRAG: &str = r#"#version 300 es
precision highp float;
uniform BlockA { vec4 a_color; };
out vec4 frag_color;
void main() {
    frag_color = a_color;
}
"#;

const BLOCK_B_FRAG: &str = r#"#version 300 es
precision highp float;
uniform BlockB { vec4 b_color; };
out vec4 frag_color;
void main() {
    frag_color = b_color;
}
"#;

fn solid_texture(ctx: &Context, c: [u8; 4]) -> TextureId {
    let t = ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )
        .expect("texture");
    ctx.texture_storage_2d(t, 1, &TextureFormat::rgba8(), 1, 1);
    ctx.texture_sub_image_2d(t, 0, 0, 0, 1, 1, &TextureFormat::rgba8(), &c);
    t
}

fn color_block(ctx: &Context, field: &str, c: [u8; 4]) -> UniformBufferId {
    let layout = UboLayout::new().field(field, Std140Type::Vec4);
    let ubo = ctx
        .create_uniform_buffer(&layout, BufferUsage::StaticDraw)
        .expect("ubo");
    let f = |x: u8| x as f32 / 255.0;
    ctx.ubo_set_vec4(ubo, field, f(c[0]), f(c[1]), f(c[2]), f(c[3]));
    ubo
}

// One batch drawing quad `col` with `program` and `u` (plus the quad rect).
fn quad_batch(f: &Fixture, program: ProgramId, col: i32, u: UniformValues) -> Batch {
    let mut b = Batch::new(program, &RenderState::new());
    b.set_uniforms(&u);
    let cmd = DrawCommand::elements(DrawMode::Triangles, 6, IndexType::UnsignedShort, 0);
    b.draw(f.quad, &quad_uniforms(col, 0), cmd);
    b
}

// A → B → A, all on unit 0. Switching back to A must rebind RED even though A's
// own uniforms didn't change: the unit's contents are global state.
#[wasm_bindgen_test]
fn texture_unit_rebinds_across_programs() {
    let f = fixture();
    let prog_a = f.ctx.create_program(VERT, SAMPLER_A_FRAG).expect("program");
    let prog_b = f.ctx.create_program(VERT, SAMPLER_B_FRAG).expect("program");
    let red = solid_texture(&f.ctx, RED);
    let blue = solid_texture(&f.ctx, BLUE);

    let a = UniformValues::new();
    a.set_sampler("u_a", red);
    let b = UniformValues::new();
    b.set_sampler("u_b", blue);
    render(
        &f,
        vec![
            quad_batch(&f, prog_a, 0, a.clone()),
            quad_batch(&f, prog_b, 1, b),
            quad_batch(&f, prog_a, 2, a),
        ],
    );

    assert_eq!(read_quads(&f.gl), [RED, BLUE, RED]);
}

// Same shape for uniform-buffer binding points.
#[wasm_bindgen_test]
fn block_point_rebinds_across_programs() {
    let f = fixture();
    let prog_a = f.ctx.create_program(VERT, BLOCK_A_FRAG).expect("program");
    let prog_b = f.ctx.create_program(VERT, BLOCK_B_FRAG).expect("program");
    let a_red = color_block(&f.ctx, "a_color", RED);
    let b_blue = color_block(&f.ctx, "b_color", BLUE);

    let a = UniformValues::new();
    a.set_uniform_block("BlockA", a_red);
    let b = UniformValues::new();
    b.set_uniform_block("BlockB", b_blue);
    render(
        &f,
        vec![
            quad_batch(&f, prog_a, 0, a.clone()),
            quad_batch(&f, prog_b, 1, b),
            quad_batch(&f, prog_a, 2, a),
        ],
    );

    assert_eq!(read_quads(&f.gl), [RED, BLUE, RED]);
}

// Two writes far apart in one block upload as separate runs; both must land.
#[wasm_bindgen_test]
fn scattered_ubo_writes_reach_the_gpu() {
    const FRAG: &str = r#"#version 300 es
precision highp float;
uniform Big { vec4 head; vec4 filler[60]; vec4 tail; };
out vec4 frag_color;
void main() {
    frag_color = head + tail + filler[0] * 0.0;
}
"#;
    let f = fixture();
    let program = f.ctx.create_program(VERT, FRAG).expect("program");
    let layout = UboLayout::new()
        .field("head", Std140Type::Vec4)
        .array("filler", Std140Type::Vec4, 60)
        .field("tail", Std140Type::Vec4);
    let ubo = f
        .ctx
        .create_uniform_buffer(&layout, BufferUsage::DynamicDraw)
        .expect("ubo");
    f.ctx.ubo_set_vec4(ubo, "head", 1.0, 0.0, 0.0, 0.5);
    f.ctx.ubo_set_vec4(ubo, "tail", 0.0, 0.0, 1.0, 0.5);

    let u = UniformValues::new();
    u.set_uniform_block("Big", ubo);
    render(&f, vec![quad_batch(&f, program, 1, u)]);

    assert_eq!(read_quads(&f.gl), [BLACK, [255, 0, 255, 255], BLACK]);
}
