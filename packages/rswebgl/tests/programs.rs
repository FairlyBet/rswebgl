// Program compile/link error reporting against a real WebGL2 context.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::extension::Extension;
use rswebgl::pass::Batch;
use rswebgl::render_state::RenderState;

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const RED_FRAG: &str = r#"#version 300 es
precision highp float;
out vec4 frag_color;
void main() {
    frag_color = vec4(1.0, 0.0, 0.0, 1.0);
}
"#;

// Assigns a float to a vec4: a compile error with a recognizable log.
const BROKEN_FRAG: &str = r#"#version 300 es
precision highp float;
out vec4 frag_color;
void main() {
    frag_color = 1.0;
}
"#;

fn cmd() -> DrawCommand {
    DrawCommand::elements(DrawMode::Triangles, 6, IndexType::UnsignedShort, 0)
}

// Without parallel compile the error comes straight back from create_program,
// with the fragment shader's compile log in it.
#[wasm_bindgen_test]
fn create_program_returns_compile_log() {
    let f = fixture();
    let err = f
        .ctx
        .create_program(VERT, BROKEN_FRAG)
        .expect_err("broken shader must not link");
    assert!(err.contains("link failed"), "{err}");
    assert!(err.contains("fragment shader:"), "{err}");
}

// With parallel compile create_program can't know yet, so it succeeds; the
// renderer then skips the broken batch (no GL error) and still draws the rest.
#[wasm_bindgen_test]
fn renderer_skips_program_that_failed_to_link() {
    let mut f = fixture();
    if !f.ctx.enable_extension(Extension::KhrParallelShaderCompile) {
        return; // Nothing parallel to test on this browser.
    }
    let broken = f
        .ctx
        .create_program(VERT, BROKEN_FRAG)
        .expect("deferred: no error until the link completes");
    let good = f.ctx.create_program(VERT, RED_FRAG).expect("program");

    let mut bad_batch = Batch::new(&broken, &RenderState::new());
    bad_batch.draw(&f.quad, &quad_uniforms(0, 0), cmd());
    let mut good_batch = Batch::new(&good, &RenderState::new());
    good_batch.draw(&f.quad, &quad_uniforms(1, 0), cmd());
    render(&f, vec![bad_batch, good_batch]);

    assert_eq!(read_quads(&f.gl), [BLACK, RED, BLACK]);
    assert!(broken.is_ready());
    assert!(!broken.is_valid());
    assert!(good.is_valid());
}
