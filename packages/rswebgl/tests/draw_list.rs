// The flat draw list (sort keys) and the GL state cache kept across frames:
// what's skipped, and that binds outside the renderer are noticed.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::buffer::BufferUsage;
use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::{ParamGroupId, ProgramId, StateId};
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::pass::{KEY_LAST, Pass, default_key};
use rswebgl::render_state::RenderState;
use rswebgl::renderbuffer::RenderbufferFormat;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const NULL: ParamGroupId = ParamGroupId::NULL;

fn solid_frag(c: [u8; 4]) -> String {
    let v = c.map(|x| x as f32 / 255.0);
    format!(
        "#version 300 es\nprecision highp float;\nout vec4 frag_color;\n\
         void main() {{ frag_color = vec4({:.1}, {:.1}, {:.1}, {:.1}); }}\n",
        v[0], v[1], v[2], v[3]
    )
}

const SAMPLER_FRAG: &str = r#"#version 300 es
precision highp float;
uniform sampler2D u_tex;
out vec4 frag_color;
void main() {
    frag_color = texture(u_tex, vec2(0.5));
}
"#;

fn solid_program(f: &Fixture, c: [u8; 4]) -> ProgramId {
    f.ctx.create_program(VERT, &solid_frag(c)).expect("program")
}

fn default_state(f: &Fixture) -> StateId {
    f.ctx.render_state(&RenderState::new())
}

fn canvas_pass() -> Pass {
    let mut pass = Pass::to_default();
    pass.set_clear(&ClearMask::color());
    pass
}

fn push_quad(f: &Fixture, pass: &mut Pass, program: ProgramId, state: StateId, col: i32) {
    pass.push(
        program,
        state,
        f.quad,
        NULL,
        NULL,
        NULL,
        quad(f, col),
        &quad_cmd(),
    );
}

// Starts counting from a known state: nothing assumed bound, counters zero.
fn fresh_frame(f: &Fixture) {
    f.ctx.invalidate_state();
    f.ctx.reset_frame_stats();
}

#[wasm_bindgen_test]
fn equal_render_states_intern_to_one_id() {
    let f = fixture();
    let a = f.ctx.render_state(&RenderState::new());
    let b = f.ctx.render_state(&RenderState::new());
    let mut depth = RenderState::new();
    depth.depth_test = true;
    let c = f.ctx.render_state(&depth);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// Interleaved programs: sorting by the default key groups them.
#[wasm_bindgen_test]
fn sorting_groups_draws_by_program() {
    let f = fixture();
    let red = solid_program(&f, RED);
    let green = solid_program(&f, GREEN);
    let state = default_state(&f);

    let mut pass = canvas_pass();
    for (program, col) in [(red, 0), (green, 1), (red, 2), (green, 1)] {
        push_quad(&f, &mut pass, program, state, col);
    }

    fresh_frame(&f);
    f.ctx.render(&pass);
    assert_eq!(f.ctx.frame_stats().programs, 4);
    assert_eq!(read_quads(&f.gl), [RED, GREEN, RED]);

    pass.sort();
    fresh_frame(&f);
    f.ctx.render(&pass);
    let stats = f.ctx.frame_stats();
    assert_eq!(stats.programs, 2);
    assert_eq!(stats.draws, 4);
    assert_eq!(read_quads(&f.gl), [RED, GREEN, RED]);
}

// KEY_LAST draws after every default-keyed draw, whatever the insertion order.
#[wasm_bindgen_test]
fn key_last_draws_after_default_keys() {
    let f = fixture();
    let red = solid_program(&f, RED);
    let green = solid_program(&f, GREEN);
    let state = default_state(&f);

    let mut pass = canvas_pass();
    let q = quad(&f, 1);
    let key = KEY_LAST | default_key(red, state, NULL, f.quad);
    let (hi, lo) = ((key >> 32) as u32, key as u32);
    pass.push_with_key(hi, lo, red, state, f.quad, NULL, NULL, NULL, q, &quad_cmd());
    push_quad(&f, &mut pass, green, state, 1);

    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl)[1], GREEN, "unsorted: insertion order");
    pass.sort();
    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl)[1], RED, "sorted: KEY_LAST last");
}

// A second identical frame re-issues no binds and no unchanged uniforms.
#[wasm_bindgen_test]
fn unchanged_frame_reissues_nothing() {
    let f = fixture();
    let red = solid_program(&f, RED);
    let state = default_state(&f);
    let mut pass = canvas_pass();
    push_quad(&f, &mut pass, red, state, 1);

    f.ctx.render(&pass);
    f.ctx.reset_frame_stats();
    f.ctx.render(&pass);
    let stats = f.ctx.frame_stats();
    assert_eq!(stats.draws, 1);
    assert_eq!(
        (
            stats.programs,
            stats.vertex_arrays,
            stats.states,
            stats.uniforms
        ),
        (0, 0, 0, 0),
        "{stats:?}"
    );
    assert_eq!(read_quads(&f.gl), [BLACK, RED, BLACK]);
}

#[wasm_bindgen_test]
fn draw_with_unknown_state_is_skipped() {
    let f = fixture();
    let red = solid_program(&f, RED);
    let mut pass = canvas_pass();
    push_quad(&f, &mut pass, red, StateId::NULL, 1);
    f.ctx.reset_frame_stats();
    let errors = capture_console("error", || f.ctx.render(&pass));
    assert!(
        errors.iter().any(|e| e.contains("render state")),
        "{errors:?}"
    );
    let stats = f.ctx.frame_stats();
    assert_eq!((stats.draws, stats.skipped), (0, 1));
    assert_eq!(read_quads(&f.gl), [BLACK; 3]);
}

// --- binds outside the renderer reset the cache ------------------------------

// Creating an index buffer unbinds the VAO; the next frame must rebind it.
#[wasm_bindgen_test]
fn index_buffer_upload_between_frames_rebinds_vao() {
    let f = fixture();
    let red = solid_program(&f, RED);
    let state = default_state(&f);
    let mut pass = canvas_pass();
    push_quad(&f, &mut pass, red, state, 1);
    f.ctx.render(&pass);

    f.ctx
        .create_index_buffer(BufferUsage::StaticDraw, &[0, 0, 1, 0, 2, 0])
        .unwrap();
    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl), [BLACK, RED, BLACK]);
}

// A texture upload rebinds the active unit; the next frame must rebind the
// texture it samples.
#[wasm_bindgen_test]
fn texture_upload_between_frames_rebinds_unit() {
    let f = fixture();
    let program = f.ctx.create_program(VERT, SAMPLER_FRAG).unwrap();
    let state = default_state(&f);
    let tex = f
        .ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )
        .unwrap();
    f.ctx
        .texture_storage_2d(tex, 1, &TextureFormat::rgba8(), 1, 1);
    f.ctx
        .texture_sub_image_2d(tex, 0, 0, 0, 1, 1, &TextureFormat::rgba8(), &RED);
    let layout = f
        .ctx
        .create_param_layout(&ParamLayoutBuilder::new().texture("u_tex"))
        .unwrap();
    let g = f.ctx.create_param_group(layout).unwrap();
    f.ctx.param_set_texture_by_name(g, "u_tex", tex);

    let mut pass = canvas_pass();
    pass.push(
        program,
        state,
        f.quad,
        g,
        NULL,
        NULL,
        quad(&f, 1),
        &quad_cmd(),
    );
    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl)[1], RED);

    f.ctx
        .texture_sub_image_2d(tex, 0, 0, 0, 1, 1, &TextureFormat::rgba8(), &GREEN);
    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl)[1], GREEN);
}

// Attaching to a framebuffer binds it; the next canvas pass must rebind the
// default framebuffer.
#[wasm_bindgen_test]
fn framebuffer_setup_between_frames_rebinds_target() {
    let f = fixture();
    let red = solid_program(&f, RED);
    let green = solid_program(&f, GREEN);
    let state = default_state(&f);
    let mut pass = canvas_pass();
    push_quad(&f, &mut pass, red, state, 1);
    f.ctx.render(&pass);

    let fb = f.ctx.create_framebuffer(4, 4).unwrap();
    let rb = f
        .ctx
        .create_renderbuffer(RenderbufferFormat::Rgba8, 4, 4)
        .unwrap();
    f.ctx.framebuffer_attach_color_renderbuffer(fb, 0, rb);

    let mut pass = canvas_pass();
    push_quad(&f, &mut pass, green, state, 1);
    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl), [BLACK, GREEN, BLACK]);
}

// Raw GL calls behind the library's back: `invalidate_state` resyncs.
#[wasm_bindgen_test]
fn invalidate_state_after_raw_gl() {
    let f = fixture();
    let red = solid_program(&f, RED);
    let state = default_state(&f);
    let mut pass = canvas_pass();
    push_quad(&f, &mut pass, red, state, 1);
    f.ctx.render(&pass);

    f.gl.bind_vertex_array(None);
    f.gl.use_program(None);
    f.ctx.invalidate_state();
    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl), [BLACK, RED, BLACK]);
}

// A clear opens what the current state masks off, and puts it back: the next
// frame's masked draw still writes nothing, the clear still clears.
#[wasm_bindgen_test]
fn clear_opens_and_restores_masks() {
    let f = fixture();
    let red = solid_program(&f, RED);
    let mut masked = RenderState::new();
    masked.color_mask_r = false;
    masked.color_mask_g = false;
    masked.color_mask_b = false;
    masked.color_mask_a = false;
    masked.scissor_test = true;
    masked.scissor_width = 1;
    masked.scissor_height = 1;
    let masked = f.ctx.render_state(&masked);
    let open = default_state(&f);

    let mut pass = canvas_pass();
    push_quad(&f, &mut pass, red, open, 0);
    push_quad(&f, &mut pass, red, masked, 1);
    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl), [RED, BLACK, BLACK]);

    // Frame 2 starts with `masked` current: the clear must still reach the
    // whole canvas, and the masked draw must still write nothing.
    let mut pass = canvas_pass();
    push_quad(&f, &mut pass, red, masked, 0);
    f.ctx.reset_frame_stats();
    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl), [BLACK; 3]);
    assert_eq!(f.ctx.frame_stats().states, 0);
}
