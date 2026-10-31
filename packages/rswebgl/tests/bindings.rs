// Renderer state-tracking tests against a real WebGL2 context.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::buffer::BufferUsage;
use rswebgl::context::Context;
use rswebgl::ids::{ParamGroupId, ProgramId, TextureId, UniformBufferId};
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::pass::Batch;
use rswebgl::render_state::RenderState;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};
use rswebgl::uniform_buffer::{UboLayout, UniformType};

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
    let layout = UboLayout::new().field(field, UniformType::Vec4);
    let ubo = ctx
        .create_uniform_buffer(&layout, BufferUsage::StaticDraw)
        .expect("ubo");
    let f = |x: u8| x as f32 / 255.0;
    ctx.ubo_set_vec4(ubo, field, f(c[0]), f(c[1]), f(c[2]), f(c[3]));
    ubo
}

// A single-texture / single-block parameter group.
fn texture_group(f: &Fixture, name: &str, tex: TextureId) -> ParamGroupId {
    let layout = f
        .ctx
        .create_param_layout(&ParamLayoutBuilder::new().texture(name))
        .unwrap();
    let g = f.ctx.create_param_group(layout).unwrap();
    f.ctx.param_set_texture_by_name(g, name, tex);
    g
}

fn block_group(f: &Fixture, name: &str, ubo: UniformBufferId) -> ParamGroupId {
    let layout = f
        .ctx
        .create_param_layout(&ParamLayoutBuilder::new().block(name))
        .unwrap();
    let g = f.ctx.create_param_group(layout).unwrap();
    f.ctx.param_set_block_by_name(g, name, ubo);
    g
}

// One batch drawing quad `col` with `program` and `group` in slot 0.
fn quad_batch(f: &Fixture, program: ProgramId, col: i32, group: ParamGroupId) -> Batch {
    let mut b = Batch::new(program, f.ctx.render_state(&RenderState::new()));
    b.set_group(0, group);
    draw_quad(&mut b, f, col);
    b
}

// A → B → A, all on unit 0. Switching back to A must rebind RED even though A's
// own parameters didn't change: the unit's contents are global state.
#[wasm_bindgen_test]
fn texture_unit_rebinds_across_programs() {
    let f = fixture();
    let prog_a = f.ctx.create_program(VERT, SAMPLER_A_FRAG).expect("program");
    let prog_b = f.ctx.create_program(VERT, SAMPLER_B_FRAG).expect("program");
    let a = texture_group(&f, "u_a", solid_texture(&f.ctx, RED));
    let b = texture_group(&f, "u_b", solid_texture(&f.ctx, BLUE));
    render(
        &f,
        vec![
            quad_batch(&f, prog_a, 0, a),
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
    let a = block_group(&f, "BlockA", color_block(&f.ctx, "a_color", RED));
    let b = block_group(&f, "BlockB", color_block(&f.ctx, "b_color", BLUE));
    render(
        &f,
        vec![
            quad_batch(&f, prog_a, 0, a),
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
        .field("head", UniformType::Vec4)
        .array("filler", UniformType::Vec4, 60)
        .field("tail", UniformType::Vec4);
    let ubo = f
        .ctx
        .create_uniform_buffer(&layout, BufferUsage::DynamicDraw)
        .expect("ubo");
    f.ctx.ubo_set_vec4(ubo, "head", 1.0, 0.0, 0.0, 0.5);
    f.ctx.ubo_set_vec4(ubo, "tail", 0.0, 0.0, 1.0, 0.5);

    let g = block_group(&f, "Big", ubo);
    render(&f, vec![quad_batch(&f, program, 1, g)]);

    assert_eq!(read_quads(&f.gl), [BLACK, [255, 0, 255, 255], BLACK]);
}
