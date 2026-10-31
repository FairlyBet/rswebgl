// Context loss / restore against a real WebGL2 context, driven by
// WEBGL_lose_context. Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;
use web_sys::WebGl2RenderingContext;

use rswebgl::extension::Extension;
use rswebgl::pass::Batch;
use rswebgl::render_state::RenderState;
use rswebgl::vao_builder::{AttrKind, VaoBuilder};

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const RED_FRAG: &str = r#"#version 300 es
precision highp float;
out vec4 frag_color;
void main() {
    frag_color = vec4(1.0, 0.0, 0.0, 1.0);
}
"#;

async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    });
    JsFuture::from(p).await.unwrap();
}

// Polls `cond` for up to ~2s; loss/restore events arrive asynchronously.
async fn wait_for(cond: impl Fn() -> bool) -> bool {
    for _ in 0..100 {
        if cond() {
            return true;
        }
        sleep(20).await;
    }
    cond()
}

fn red_middle_quad(f: &Fixture, program: rswebgl::ids::ProgramId) -> Batch {
    let mut b = Batch::new(program, f.ctx.render_state(&RenderState::new()));

    draw_quad(&mut b, f, 1);
    b
}

#[wasm_bindgen_test]
async fn lose_and_restore_then_recreate() {
    let mut f = fixture();
    // Enabled before the loss, so the restore must re-enable it: the parallel
    // program below polls COMPLETION_STATUS_KHR, an INVALID_ENUM without it.
    if !f.ctx.enable_extension(Extension::KhrParallelShaderCompile) {
        return;
    }

    let lost = Rc::new(Cell::new(0));
    let restored = Rc::new(Cell::new(0));
    let l = Rc::clone(&lost);
    f.ctx.on_lost(move |_| l.set(l.get() + 1));
    let r = Rc::clone(&restored);
    f.ctx.on_restored(move |_| r.set(r.get() + 1));

    let old_program = f.ctx.create_program(VERT, RED_FRAG).expect("program");
    render(&f, vec![red_middle_quad(&f, old_program)]);
    assert_eq!(read_quads(&f.gl), [BLACK, RED, BLACK]);

    // --- lose ---
    assert!(f.ctx.simulate_context_loss());
    assert!(wait_for(|| lost.get() == 1).await, "on_lost not called");
    assert!(f.ctx.is_lost());
    // Every handle is stale from the loss on; rendering is a no-op, and
    // creation fails cleanly.
    assert!(!f.ctx.program_is_valid(old_program));
    render(&f, vec![red_middle_quad(&f, old_program)]);
    let err = f.ctx.create_program(VERT, RED_FRAG).expect_err("lost");
    assert!(err.contains("lost"), "{err}");
    assert_eq!(restored.get(), 0);

    // --- restore ---
    assert!(f.ctx.simulate_context_restore());
    assert!(
        wait_for(|| restored.get() == 1).await,
        "on_restored not called"
    );
    assert!(!f.ctx.is_lost());
    assert_eq!(lost.get(), 1);

    // Handles from before the loss are stale: queries say so, and destroying
    // one is reported (by the library) rather than reaching GL.
    assert!(!f.ctx.program_is_valid(old_program));
    let errors = capture_console("error", || f.ctx.destroy_program(old_program));
    assert!(
        errors.iter().any(|e| e.contains("destroyed or invalid")),
        "{errors:?}"
    );
    let mut vb = VaoBuilder::new(&f.ctx);
    vb.add_f32(
        0,
        AttrKind::F32x2,
        &[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
    );
    vb.add_indices_u16(&[0, 1, 2, 0, 2, 3]);
    f.quad = vb.build().expect("vao");
    assert_eq!(f.gl.get_error(), WebGl2RenderingContext::NO_ERROR);

    // Recreated resources render normally.
    let program = f.ctx.create_program(VERT, RED_FRAG).expect("program");
    assert!(wait_for(|| f.ctx.program_is_ready(program)).await);
    render(&f, vec![red_middle_quad(&f, program)]);
    assert_eq!(read_quads(&f.gl), [BLACK, RED, BLACK]);
}
