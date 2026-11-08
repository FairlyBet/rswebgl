// Query objects: occlusion around a draw, a GPU timer around a pass, results
// a frame later, and the misuse paths.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::{FramebufferId, ParamGroupId, ProgramId, StateId};
use rswebgl::pass::Pass;
use rswebgl::query::QueryKind;
use rswebgl::render_state::RenderState;
use rswebgl::renderbuffer::RenderbufferFormat;

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const NULL: ParamGroupId = ParamGroupId::NULL;

fn red(f: &Fixture) -> ProgramId {
    let frag = "#version 300 es\nprecision highp float;\nout vec4 frag_color;\n\
                void main() { frag_color = vec4(1.0, 0.0, 0.0, 1.0); }\n";
    f.ctx.create_program(VERT, frag).unwrap()
}

fn depth_tested(f: &Fixture) -> StateId {
    let mut rs = RenderState::new();
    rs.depth_test = true;
    f.ctx.render_state(&rs)
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

fn depth_framebuffer(f: &Fixture) -> FramebufferId {
    let fb = f.ctx.create_framebuffer(W, H).unwrap();
    let color = f
        .ctx
        .create_renderbuffer(RenderbufferFormat::Rgba8, W, H)
        .unwrap();
    let depth = f
        .ctx
        .create_renderbuffer(RenderbufferFormat::Depth24, W, H)
        .unwrap();
    f.ctx.framebuffer_attach_color_renderbuffer(fb, 0, color);
    f.ctx.framebuffer_attach_depth_renderbuffer(fb, depth);
    fb
}

// A probe behind an occluder reports 0, one in the open reports 1 — and
// neither is available in the frame that drew them.
#[wasm_bindgen_test]
async fn occlusion_queries_report_whether_a_draw_was_visible() {
    let f = fixture();
    let fb = depth_framebuffer(&f);
    let program = red(&f);
    let state = depth_tested(&f);
    let hidden = f.ctx.create_query(QueryKind::AnySamplesPassed).unwrap();
    let visible = f
        .ctx
        .create_query(QueryKind::AnySamplesPassedConservative)
        .unwrap();
    assert!(!f.ctx.query_pending(hidden), "never run");

    let mut pass = Pass::new(fb);
    pass.set_clear(&ClearMask::color_depth());
    push_quad(&f, &mut pass, program, state, 1); // the occluder
    // Same depth as the occluder: LESS fails everywhere.
    push_quad(&f, &mut pass, program, state, 1);
    pass.set_last_draw_query(hidden);
    push_quad(&f, &mut pass, program, state, 2);
    pass.set_last_draw_query(visible);
    f.ctx.render(&pass);

    assert!(f.ctx.query_pending(hidden));
    assert_eq!(f.ctx.query_result(hidden), None, "not in the same frame");
    assert_eq!(f.ctx.query_result_async(hidden).await.unwrap(), 0.0);
    assert_eq!(f.ctx.query_result_async(visible).await.unwrap(), 1.0);
    // The result stays until the query runs again.
    assert_eq!(f.ctx.query_result(visible), Some(1.0));
    assert!(!f.ctx.query_pending(visible));
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

#[wasm_bindgen_test]
async fn a_pass_timer_measures_gpu_time_where_available() {
    let f = fixture();
    if !f.ctx.can_time_gpu() {
        let e = f.ctx.create_query(QueryKind::TimeElapsed).unwrap_err();
        assert!(e.contains("EXT_disjoint_timer_query_webgl2"), "{}", &*e);
        return;
    }
    let timer = f.ctx.create_query(QueryKind::TimeElapsed).unwrap();
    let mut pass = Pass::new(FramebufferId::NULL);
    pass.set_clear(&ClearMask::color());
    push_quad(&f, &mut pass, red(&f), depth_tested(&f), 1);
    pass.set_timer(timer);
    f.ctx.render(&pass);
    match f.ctx.query_result_async(timer).await {
        Ok(ns) => assert!(ns >= 0.0),
        Err(e) => assert!(e.contains("disjoint"), "{}", &*e),
    }
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

// The wrong kind of query is reported once and the work still happens,
// unmeasured; an unused or destroyed query has no result.
#[wasm_bindgen_test]
async fn misused_queries_are_reported() {
    let f = fixture();
    let occlusion = f.ctx.create_query(QueryKind::AnySamplesPassed).unwrap();
    let mut pass = Pass::new(FramebufferId::NULL);
    let errors = capture_console("error", || pass.set_last_draw_query(occlusion));
    assert!(errors.iter().any(|e| e.contains("no draws")), "{errors:?}");

    pass.set_clear(&ClearMask::color());
    push_quad(&f, &mut pass, red(&f), depth_tested(&f), 1);
    pass.set_timer(occlusion);
    let errors = capture_console("error", || f.ctx.render(&pass));
    assert!(
        errors.iter().any(|e| e.contains("TimeElapsed")),
        "{errors:?}"
    );
    assert_eq!(read_quads(&f.gl), [BLACK, RED, BLACK], "drawn anyway");
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);

    let never = f.ctx.query_result_async(occlusion).await.unwrap_err();
    assert!(never.contains("never run"), "{}", &*never);
    f.ctx.destroy_query(occlusion);
    assert!(!f.ctx.query_is_valid(occlusion));
    assert!(f.ctx.query_result_async(occlusion).await.is_err());
}

// Awaiting a query that the next frame runs again ends in an error instead
// of waiting on ever-newer runs.
#[wasm_bindgen_test]
async fn awaiting_a_query_run_again_fails_instead_of_hanging() {
    let f = fixture();
    let fb = depth_framebuffer(&f);
    let program = red(&f);
    let state = depth_tested(&f);
    let q = f.ctx.create_query(QueryKind::AnySamplesPassed).unwrap();
    let frame = || {
        let mut pass = Pass::new(fb);
        pass.set_clear(&ClearMask::color_depth());
        push_quad(&f, &mut pass, program, state, 1);
        pass.set_last_draw_query(q);
        f.ctx.render(&pass);
    };
    frame();
    let wait = f.ctx.query_result_async(q);
    frame(); // same task: the first run's result can't be in yet
    let err = wait.await.unwrap_err();
    assert!(err.contains("run again"), "{err}");
    // Waiting on the latest run works.
    assert_eq!(f.ctx.query_result_async(q).await.unwrap(), 1.0);
}

// The profiler times passes by label, a frame or more late, without
// reusing a query whose result hasn't been collected.
#[wasm_bindgen_test]
async fn the_profiler_times_passes_by_label() {
    let f = fixture();
    let mut profiler = rswebgl::profiler::GpuProfiler::new(&f.ctx);
    if !profiler.available() {
        return; // no EXT_disjoint_timer_query_webgl2 here
    }
    let program = red(&f);
    let state = f.ctx.render_state(&RenderState::new());
    let mut got = None;
    for _ in 0..60 {
        for label in ["scene", "post"] {
            let mut pass = Pass::to_default();
            pass.set_clear(&ClearMask::color());
            push_quad(&f, &mut pass, program, state, 1);
            profiler.time(&mut pass, label);
            f.ctx.render(&pass);
        }
        profiler.update();
        got = profiler.milliseconds("scene").zip(profiler.milliseconds("post"));
        if got.is_some() {
            break;
        }
        let p = js_sys::Promise::new(&mut |resolve, _| {
            web_sys::window()
                .unwrap()
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 16)
                .unwrap();
        });
        wasm_bindgen_futures::JsFuture::from(p).await.unwrap();
    }
    let (scene, post) = got.expect("timings within 60 frames");
    assert!(scene >= 0.0 && post >= 0.0 && scene < 1000.0, "{scene} {post}");
    assert_eq!(profiler.labels(), ["scene", "post"]);
    assert!(profiler.last_milliseconds("scene").is_some());
    assert_eq!(profiler.milliseconds("other"), None);
    profiler.destroy();
}
