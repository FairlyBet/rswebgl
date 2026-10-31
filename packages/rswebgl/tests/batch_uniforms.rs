// Batch-level uniforms, per-program sampler units, and the unset-uniform check.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use rswebgl::context::Context;
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::pass::Batch;
use rswebgl::render_state::RenderState;
use rswebgl::texture::{Texture, TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};
use rswebgl::uniform_values::UniformValues;

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

fn cmd() -> DrawCommand {
    DrawCommand::elements(DrawMode::Triangles, 6, IndexType::UnsignedShort, 0)
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

fn color(u: &UniformValues, c: [u8; 4]) {
    let f = |x: u8| x as f32 / 255.0;
    u.set_vec4("u_color", f(c[0]), f(c[1]), f(c[2]), f(c[3]));
}

// The batch value applies to every draw; a draw's override holds for that draw
// only — the next draw gets the batch value back, not the override.
#[wasm_bindgen_test]
fn draw_override_does_not_leak_into_next_draw() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, COLOR_FRAG).expect("program");

    let shared = UniformValues::new();
    color(&shared, RED);
    let mut batch = Batch::new(&program, &RenderState::new());
    batch.set_uniforms(&shared);
    batch.draw(&f.quad, &quad_uniforms(0, 0), cmd());
    let blue = quad_uniforms(1, 0);
    color(&blue, BLUE);
    batch.draw(&f.quad, &blue, cmd());
    batch.draw(&f.quad, &quad_uniforms(2, 0), cmd());
    render(&f, vec![batch]);

    assert_eq!(read_quads(&f.gl), [RED, BLUE, RED]);
}

// The batch handle stays live: changing it after the batch is built is seen.
#[wasm_bindgen_test]
fn batch_uniforms_are_a_live_handle() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, COLOR_FRAG).expect("program");

    let shared = UniformValues::new();
    color(&shared, RED);
    let mut batch = Batch::new(&program, &RenderState::new());
    batch.set_uniforms(&shared);
    batch.draw(&f.quad, &quad_uniforms(1, 0), cmd());
    color(&shared, GREEN);
    render(&f, vec![batch]);

    assert_eq!(read_quads(&f.gl), [BLACK, GREEN, BLACK]);
}

// A sampler on the batch and one on the draw get distinct units (both used to
// be unit 0, assigned independently per UniformValues).
#[wasm_bindgen_test]
fn batch_and_draw_samplers_get_distinct_units() {
    let f = fixture();
    let program = f
        .ctx
        .create_program(VERT, TWO_SAMPLER_FRAG)
        .expect("program");
    let red = solid_texture(&f.ctx, RED);
    let green = solid_texture(&f.ctx, GREEN);
    let blue = solid_texture(&f.ctx, BLUE);

    let shared = UniformValues::new();
    shared.set_sampler("u_a", &red);
    let mut batch = Batch::new(&program, &RenderState::new());
    batch.set_uniforms(&shared);
    for (col, tex) in [(0, &blue), (1, &green), (2, &blue)] {
        let u = quad_uniforms(col, 0);
        u.set_sampler("u_b", tex);
        batch.draw(&f.quad, &u, cmd());
    }
    render(&f, vec![batch]);

    let magenta = [255, 0, 255, 255];
    let yellow = [255, 255, 0, 255];
    assert_eq!(read_quads(&f.gl), [magenta, yellow, magenta]);
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
        program.active_uniform_names(),
        ["u_rect", "u_tex", "u_tint"]
    );
}

// Captures console.warn messages while `f` runs.
fn capture_warnings(f: impl FnOnce()) -> Vec<String> {
    let console = js_sys::Reflect::get(&js_sys::global(), &"console".into()).unwrap();
    let original = js_sys::Reflect::get(&console, &"warn".into()).unwrap();
    let log = js_sys::Array::new();
    let push = js_sys::Function::new_with_args("m", "this.push(String(m))").bind0(&log);
    js_sys::Reflect::set(&console, &"warn".into(), &push).unwrap();
    f();
    js_sys::Reflect::set(&console, &"warn".into(), &original).unwrap();
    log.iter().filter_map(|v: JsValue| v.as_string()).collect()
}

// A uniform set by neither the batch nor the draw is reported, once.
#[wasm_bindgen_test]
fn unset_uniform_is_reported_once() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, COLOR_FRAG).expect("program");
    let mut batch = Batch::new(&program, &RenderState::new());
    let first = quad_uniforms(0, 0);
    color(&first, RED);
    batch.draw(&f.quad, &first, cmd());
    // These two rely on the leaked u_color.
    batch.draw(&f.quad, &quad_uniforms(1, 0), cmd());
    batch.draw(&f.quad, &quad_uniforms(2, 0), cmd());

    let warnings = capture_warnings(|| render(&f, vec![batch]));
    let about_color: Vec<_> = warnings.iter().filter(|w| w.contains("u_color")).collect();
    assert_eq!(about_color.len(), 1, "{warnings:?}");
}
