// Transform feedback: vertex outputs captured into buffers, appended across a
// pass's draws, counted by a query, and every WebGL2 rule reported per draw
// instead of a GL error.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;
use web_sys::WebGl2RenderingContext as Gl;

use rswebgl::buffer::BufferUsage;
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::feedback::FeedbackMode;
use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::{BufferId, FeedbackId, FramebufferId, ParamGroupId, ProgramId, VertexArrayId};
use rswebgl::pass::Pass;
use rswebgl::query::QueryKind;
use rswebgl::render_state::RenderState;

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const NULL: ParamGroupId = ParamGroupId::NULL;
const FRAG: &str =
    "#version 300 es\nprecision highp float;\nout vec4 c;\nvoid main() { c = vec4(1.0); }\n";

// Each vertex writes (id, 2·id) and id + 0.5.
const VERT: &str = r#"#version 300 es
out vec2 v_pair;
out float v_half;
void main() {
    float id = float(gl_VertexID);
    v_pair = vec2(id, id * 2.0);
    v_half = id + 0.5;
    gl_Position = vec4(0.0, 0.0, 0.0, 1.0);
    gl_PointSize = 1.0;
}
"#;

fn program(f: &Fixture, varyings: &[&str], mode: FeedbackMode) -> ProgramId {
    let names = varyings.iter().map(|s| s.to_string()).collect();
    f.ctx
        .create_program_with_feedback(VERT, FRAG, names, mode)
        .unwrap()
}

fn buffer(f: &Fixture, bytes: u32) -> BufferId {
    f.ctx
        .create_empty_buffer(BufferUsage::StaticCopy, bytes)
        .unwrap()
}

fn floats(f: &Fixture, b: BufferId, bytes: u32) -> Vec<f32> {
    f.ctx
        .buffer_read(b, 0, bytes)
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

fn recording(tf: FeedbackId, mode: DrawMode) -> Pass {
    let mut pass = Pass::new(FramebufferId::NULL);
    pass.set_transform_feedback(tf, mode, true);
    pass
}

fn push(f: &Fixture, pass: &mut Pass, program: ProgramId, cmd: &DrawCommand) {
    let state = f.ctx.render_state(&RenderState::new());
    pass.push(
        program,
        state,
        VertexArrayId::NULL,
        NULL,
        NULL,
        NULL,
        NULL,
        cmd,
    );
}

fn points(first: i32, count: i32) -> DrawCommand {
    DrawCommand::arrays(DrawMode::Points, first, count)
}

// Two draws append to one buffer; the query counts the primitives; the
// recording leaves no state behind.
#[wasm_bindgen_test]
async fn draws_append_their_outputs_into_the_buffer() {
    let f = fixture();
    let p = program(&f, &["v_pair"], FeedbackMode::Interleaved);
    let out = buffer(&f, 32);
    let tf = f.ctx.create_transform_feedback().unwrap();
    f.ctx.transform_feedback_set_buffer(tf, 0, out);
    let written = f
        .ctx
        .create_query(QueryKind::TransformFeedbackPrimitivesWritten)
        .unwrap();

    let mut pass = recording(tf, DrawMode::Points);
    pass.set_transform_feedback_query(written);
    push(&f, &mut pass, p, &points(0, 2));
    push(&f, &mut pass, p, &points(2, 2));
    f.ctx.render(&pass);

    assert_eq!(
        floats(&f, out, 32),
        [0.0, 0.0, 1.0, 2.0, 2.0, 4.0, 3.0, 6.0]
    );
    assert_eq!(f.ctx.query_result_async(written).await.unwrap(), 4.0);
    assert!(!f.gl.is_enabled(Gl::RASTERIZER_DISCARD));
    assert!(
        f.gl.get_parameter(Gl::TRANSFORM_FEEDBACK_BINDING)
            .unwrap()
            .is_null()
    );
    assert_eq!(f.gl.get_error(), Gl::NO_ERROR);

    // The buffer is free again: an ordinary pass right after draws.
    let red = f
        .ctx
        .create_program(
            common::VERT,
            "#version 300 es\nprecision highp float;\nout vec4 c;\nvoid main() { c = vec4(1.0, 0.0, 0.0, 1.0); }\n",
        )
        .unwrap();
    let mut normal = Pass::new(FramebufferId::NULL);
    normal.set_clear(&ClearMask::color());
    normal.push(
        red,
        f.ctx.render_state(&RenderState::new()),
        f.quad,
        NULL,
        NULL,
        NULL,
        quad(&f, 1),
        &quad_cmd(),
    );
    f.ctx.render(&normal);
    assert_eq!(read_quads(&f.gl), [BLACK, RED, BLACK]);
}

#[wasm_bindgen_test]
fn separate_mode_writes_one_buffer_per_varying() {
    let f = fixture();
    let p = program(&f, &["v_pair", "v_half"], FeedbackMode::Separate);
    let (pairs, halves) = (buffer(&f, 24), buffer(&f, 64));
    let tf = f.ctx.create_transform_feedback().unwrap();
    f.ctx.transform_feedback_set_buffer(tf, 0, pairs);
    // A range: 12 bytes from offset 4.
    f.ctx
        .transform_feedback_set_buffer_range(tf, 1, halves, 4, 12);
    let mut pass = recording(tf, DrawMode::Points);
    push(&f, &mut pass, p, &points(0, 3));
    f.ctx.render(&pass);
    assert_eq!(floats(&f, pairs, 24), [0.0, 0.0, 1.0, 2.0, 2.0, 4.0]);
    assert_eq!(floats(&f, halves, 20), [0.0, 0.5, 1.5, 2.5, 0.0]);
    assert_eq!(f.gl.get_error(), Gl::NO_ERROR);
}

// Each broken rule: reported, the draw skipped, no GL error, nothing written.
#[wasm_bindgen_test]
fn draws_that_cant_be_recorded_are_reported_and_skipped() {
    let f = fixture();
    let p = program(&f, &["v_pair"], FeedbackMode::Interleaved);
    let plain = f.ctx.create_program(VERT, FRAG).unwrap();
    let separate = program(&f, &["v_pair", "v_half"], FeedbackMode::Separate);
    let out = buffer(&f, 16); // two vertices
    let tf = f.ctx.create_transform_feedback().unwrap();
    f.ctx.transform_feedback_set_buffer(tf, 0, out);

    let cases: [(&str, ProgramId, DrawCommand); 6] = [
        (
            "isn't the recording's",
            p,
            DrawCommand::arrays(DrawMode::Lines, 0, 2),
        ),
        (
            "indexed",
            p,
            DrawCommand::elements(DrawMode::Points, 2, IndexType::UnsignedShort, 0),
        ),
        ("captures no varyings", plain, points(0, 1)),
        ("none is bound there", separate, points(0, 1)),
        ("holds 16", p, points(0, 3)),
        ("one program per recording", separate, points(0, 1)),
    ];
    for (why, program, cmd) in cases {
        let mut pass = recording(tf, DrawMode::Points);
        if why == "one program per recording" {
            push(&f, &mut pass, p, &points(0, 1));
        }
        push(&f, &mut pass, program, &cmd);
        let errors = capture_console("error", || f.ctx.render(&pass));
        assert!(errors.iter().any(|e| e.contains(why)), "{why}: {errors:?}");
        assert_eq!(f.gl.get_error(), Gl::NO_ERROR, "{why}");
    }

    let errors = capture_console("error", || {
        let mut pass = Pass::new(FramebufferId::NULL);
        pass.set_transform_feedback(tf, DrawMode::TriangleStrip, true);
        let index = f
            .ctx
            .create_index_buffer(BufferUsage::StaticDraw, &[0, 0, 1, 0])
            .unwrap();
        f.ctx.transform_feedback_set_buffer(tf, 1, index);
        f.ctx.transform_feedback_set_buffer_range(tf, 1, out, 2, 8);
    });
    assert_eq!(errors.len(), 3, "{errors:?}");
}

// A buffer destroyed while bound stays alive for the object; destroying the
// object then frees it.
#[wasm_bindgen_test]
fn the_object_keeps_its_buffers_alive() {
    let f = fixture();
    let p = program(&f, &["v_pair"], FeedbackMode::Interleaved);
    let out = buffer(&f, 16);
    let tf = f.ctx.create_transform_feedback().unwrap();
    f.ctx.transform_feedback_set_buffer(tf, 0, out);
    f.ctx.destroy_buffer(out);
    assert!(!f.ctx.buffer_is_valid(out));
    let mut pass = recording(tf, DrawMode::Points);
    push(&f, &mut pass, p, &points(0, 2));
    f.ctx.render(&pass);
    f.ctx.destroy_transform_feedback(tf);
    assert!(!f.ctx.transform_feedback_is_valid(tf));
    assert_eq!(f.gl.get_error(), Gl::NO_ERROR);
}
