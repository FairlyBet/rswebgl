// Parameter layouts and groups: slots, overrides, live values, per-program
// texture units, upload skipping, validation. Run with:
// wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::context::Context;
use rswebgl::ids::{ParamGroupId, ParamLayoutId, TextureId};
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::pass::Batch;
use rswebgl::render_state::RenderState;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};
use rswebgl::uniform_buffer::UniformType;

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const COLOR_FRAG: &str = r#"#version 300 es
precision highp float;
uniform vec4 u_color;
out vec4 frag_color;
void main() {
    frag_color = u_color;
}
"#;

// Red from u_a, green and blue from u_b.
const TWO_SAMPLER_FRAG: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_a;
uniform sampler2D u_b;
out vec4 frag_color;
void main() {
    vec4 b = texture(u_b, vec2(0.5));
    frag_color = vec4(texture(u_a, vec2(0.5)).r, b.g, b.b, 1.0);
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

fn color_layout(f: &Fixture) -> ParamLayoutId {
    f.ctx
        .create_param_layout(&ParamLayoutBuilder::new().uniform("u_color", UniformType::Vec4))
        .unwrap()
}

fn color_group(f: &Fixture, layout: ParamLayoutId, c: [u8; 4]) -> ParamGroupId {
    let g = f.ctx.create_param_group(layout).unwrap();
    set_color(f, g, c);
    g
}

fn set_color(f: &Fixture, g: ParamGroupId, c: [u8; 4]) {
    let v = c.map(|x| x as f32 / 255.0);
    f.ctx.param_set_floats_by_name(g, "u_color", &v);
}

// The batch's group applies to every draw; a draw's own group in that slot
// holds for that draw only — the next draw gets the batch's back.
#[wasm_bindgen_test]
fn draw_override_does_not_leak_into_next_draw() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, COLOR_FRAG).unwrap();
    let layout = color_layout(&f);
    let red = color_group(&f, layout, RED);
    let blue = color_group(&f, layout, BLUE);

    let mut batch = Batch::new(program, f.ctx.render_state(&RenderState::new()));
    batch.set_group(1, red);
    draw_quad(&mut batch, &f, 0);
    let mut groups = [ParamGroupId::NULL; 4];
    groups[1] = blue;
    groups[QUAD_SLOT as usize] = quad(&f, 1);
    batch.draw_with_groups(f.quad, groups, &quad_cmd());
    draw_quad(&mut batch, &f, 2);
    render(&f, vec![batch]);

    assert_eq!(read_quads(&f.gl), [RED, BLUE, RED]);
}

// Groups are referenced, not copied: writing after the pass is built is seen.
#[wasm_bindgen_test]
fn group_writes_after_building_are_seen() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, COLOR_FRAG).unwrap();
    let g = color_group(&f, color_layout(&f), RED);
    let mut batch = Batch::new(program, f.ctx.render_state(&RenderState::new()));
    batch.set_group(0, g);
    draw_quad(&mut batch, &f, 1);
    set_color(&f, g, GREEN);
    render(&f, vec![batch]);

    assert_eq!(read_quads(&f.gl), [BLACK, GREEN, BLACK]);
}

// A copy starts with the source's values and then goes its own way; it keeps
// working after the source is destroyed.
#[wasm_bindgen_test]
fn a_copied_group_is_independent() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, COLOR_FRAG).unwrap();
    let layout = color_layout(&f);
    let red = color_group(&f, layout, RED);
    let copy = f.ctx.copy_param_group(red).unwrap();
    let blue = f.ctx.copy_param_group(red).unwrap();
    set_color(&f, blue, BLUE);
    f.ctx.destroy_param_group(red);
    f.ctx.destroy_param_layout(layout);
    let mut a = Batch::new(program, f.ctx.render_state(&RenderState::new()));
    a.set_group(0, copy);
    draw_quad(&mut a, &f, 0);
    let mut b = Batch::new(program, f.ctx.render_state(&RenderState::new()));
    b.set_group(0, blue);
    draw_quad(&mut b, &f, 2);
    render(&f, vec![a, b]);
    assert_eq!(read_quads(&f.gl), [RED, BLACK, BLUE]);
    assert!(
        f.ctx.copy_param_group(red).is_err(),
        "copying a destroyed group"
    );

    // A copy made after the layout is gone keeps it alive on its own:
    // destroying the group it came from doesn't pull the layout away.
    let copy2 = f.ctx.copy_param_group(copy).unwrap();
    f.ctx.destroy_param_group(copy);
    f.ctx.destroy_param_group(blue); // copy2 is the layout's last holder
    set_color(&f, copy2, GREEN);
    let mut c = Batch::new(program, f.ctx.render_state(&RenderState::new()));
    c.set_group(0, copy2);
    draw_quad(&mut c, &f, 1);
    render(&f, vec![c]);
    assert_eq!(read_quads(&f.gl), [BLACK, GREEN, BLACK]);
}

// A sampler in the batch's group and one in each draw's group get distinct
// units (the program assigns them per sampler name).
#[wasm_bindgen_test]
fn batch_and_draw_samplers_get_distinct_units() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, TWO_SAMPLER_FRAG).unwrap();
    let a_layout = f
        .ctx
        .create_param_layout(&ParamLayoutBuilder::new().texture("u_a"))
        .unwrap();
    let b_layout = f
        .ctx
        .create_param_layout(&ParamLayoutBuilder::new().texture("u_b"))
        .unwrap();
    let shared = f.ctx.create_param_group(a_layout).unwrap();
    f.ctx
        .param_set_texture_by_name(shared, "u_a", solid_texture(&f.ctx, RED));

    let green = solid_texture(&f.ctx, GREEN);
    let blue = solid_texture(&f.ctx, BLUE);
    let mut batch = Batch::new(program, f.ctx.render_state(&RenderState::new()));
    batch.set_group(0, shared);
    for (col, tex) in [(0, blue), (1, green), (2, blue)] {
        let g = f.ctx.create_param_group(b_layout).unwrap();
        f.ctx.param_set_texture_by_name(g, "u_b", tex);
        let mut groups = [ParamGroupId::NULL; 4];
        groups[1] = g;
        groups[QUAD_SLOT as usize] = quad(&f, col);
        batch.draw_with_groups(f.quad, groups, &quad_cmd());
    }
    render(&f, vec![batch]);

    let magenta = [255, 0, 255, 255];
    let yellow = [255, 255, 0, 255];
    assert_eq!(read_quads(&f.gl), [magenta, yellow, magenta]);
}

// The same value in consecutive draws is uploaded once, not per draw.
#[wasm_bindgen_test]
fn unchanged_values_are_not_reuploaded() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, COLOR_FRAG).unwrap();
    let layout = color_layout(&f);
    let red = color_group(&f, layout, RED);
    let also_red = color_group(&f, layout, RED);
    let mut batch = Batch::new(program, f.ctx.render_state(&RenderState::new()));
    batch.set_group(0, red);
    draw_quad(&mut batch, &f, 0);
    draw_quad(&mut batch, &f, 1);
    // A different group holding the same value: compared by content.
    let mut groups = [ParamGroupId::NULL; 4];
    groups[0] = also_red;
    groups[QUAD_SLOT as usize] = quad(&f, 2);
    batch.draw_with_groups(f.quad, groups, &quad_cmd());

    // Count uniform4fv calls on this context (u_color and u_rect are vec4s).
    let counter = js_sys::Function::new_with_args(
        "gl",
        "const orig = gl.uniform4fv; const c = { n: 0 }; \
         gl.uniform4fv = function(...a) { c.n++; return orig.apply(this, a); }; \
         return c;",
    );
    let c = counter.call1(&wasm_bindgen::JsValue::NULL, &f.gl).unwrap();
    render(&f, vec![batch]);
    let n = js_sys::Reflect::get(&c, &"n".into())
        .unwrap()
        .as_f64()
        .unwrap();

    assert_eq!(read_quads(&f.gl), [RED, RED, RED]);
    // 1 for u_color + 3 for the three distinct u_rect values.
    assert_eq!(n, 4.0);
}

#[wasm_bindgen_test]
fn active_uniform_names_skip_block_members() {
    let f = fixture();
    const FRAG: &str = r#"#version 300 es
precision highp float;
uniform Light { vec4 light_color; };
uniform vec4 u_tint[2];
uniform sampler2D u_tex;
out vec4 frag_color;
void main() {
    frag_color = light_color * u_tint[1] * texture(u_tex, vec2(0.5));
}
"#;
    let program = f.ctx.create_program(VERT, FRAG).expect("program");
    assert_eq!(
        f.ctx.program_active_uniform_names(program),
        ["u_rect", "u_tex", "u_tint"]
    );
}

// An input no group provides is reported, once.
#[wasm_bindgen_test]
fn input_in_no_group_is_reported_once() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, COLOR_FRAG).unwrap();
    let mut batch = Batch::new(program, f.ctx.render_state(&RenderState::new()));
    for col in 0..3 {
        draw_quad(&mut batch, &f, col);
    }
    let warnings = capture_console("warn", || render(&f, vec![batch]));
    let about: Vec<_> = warnings.iter().filter(|w| w.contains("u_color")).collect();
    assert_eq!(about.len(), 1, "{warnings:?}");
}

// A layout type that doesn't match the shader is reported and not uploaded.
#[wasm_bindgen_test]
fn type_mismatch_is_reported_and_skipped() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, COLOR_FRAG).unwrap();
    let layout = f
        .ctx
        .create_param_layout(&ParamLayoutBuilder::new().uniform("u_color", UniformType::Vec3))
        .unwrap();
    let g = f.ctx.create_param_group(layout).unwrap();
    f.ctx
        .param_set_floats_by_name(g, "u_color", &[1.0, 1.0, 1.0]);
    let mut batch = Batch::new(program, f.ctx.render_state(&RenderState::new()));
    batch.set_group(0, g);
    draw_quad(&mut batch, &f, 1);
    let warnings = capture_console("warn", || render(&f, vec![batch]));

    assert!(
        warnings
            .iter()
            .any(|w| w.contains("u_color") && w.contains("Vec3")),
        "{warnings:?}"
    );
    assert_eq!(read_quads(&f.gl), [BLACK, [0, 0, 0, 0], BLACK]);
}

#[wasm_bindgen_test]
fn fields_resolve_by_name_once() {
    let f = fixture();
    let layout = color_layout(&f);
    assert!(f.ctx.param_field(layout, "u_color").is_ok());
    let err = f.ctx.param_field(layout, "u_nope").unwrap_err();
    assert!(err.contains("u_nope"), "{err}");
    let dup = f.ctx.create_param_layout(
        &ParamLayoutBuilder::new()
            .uniform("x", UniformType::F32)
            .texture("x"),
    );
    assert!(dup.is_err());
}

// A draw whose group (or a texture in it) was destroyed is skipped, not drawn
// with whatever an earlier draw left in the program.
#[wasm_bindgen_test]
fn a_draw_with_a_destroyed_group_or_texture_is_skipped() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, COLOR_FRAG).unwrap();
    let layout = color_layout(&f);
    let red = color_group(&f, layout, RED);
    let gone = color_group(&f, layout, GREEN);
    let state = f.ctx.render_state(&RenderState::new());
    let batch = |g| {
        let mut b = Batch::new(program, state);
        b.set_group(0, g);
        b
    };
    // Red at 0 leaves red in the program; the destroyed group's quad at 1
    // would come out red too if it were drawn.
    let mut a = batch(red);
    draw_quad(&mut a, &f, 0);
    let mut b = batch(gone);
    draw_quad(&mut b, &f, 1);
    f.ctx.destroy_param_group(gone);
    f.ctx.reset_frame_stats();
    let errors = capture_console("error", || render(&f, vec![a, b]));
    assert_eq!(read_quads(&f.gl), [RED, BLACK, BLACK]);
    assert_eq!(f.ctx.frame_stats().skipped, 1);
    assert!(
        errors.iter().any(|e| e.contains("param group")),
        "{errors:?}"
    );

    // The same for a texture the program samples.
    let tex_program = f.ctx.create_program(VERT, TWO_SAMPLER_FRAG).unwrap();
    let tex_layout = f
        .ctx
        .create_param_layout(&ParamLayoutBuilder::new().texture("u_a").texture("u_b"))
        .unwrap();
    let g = f.ctx.create_param_group(tex_layout).unwrap();
    let t = solid_texture(&f.ctx, [255, 255, 255, 255]);
    f.ctx.param_set_texture_by_name(g, "u_a", t);
    f.ctx.param_set_texture_by_name(g, "u_b", t);
    f.ctx.destroy_texture(t);
    let mut c = Batch::new(tex_program, state);
    c.set_group(0, g);
    draw_quad(&mut c, &f, 2);
    f.ctx.reset_frame_stats();
    capture_console("error", || render(&f, vec![c]));
    assert_eq!(read_quads(&f.gl)[2], BLACK);
    assert_eq!(f.ctx.frame_stats().skipped, 1);
}
