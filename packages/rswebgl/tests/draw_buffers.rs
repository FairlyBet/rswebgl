// Per-target blending (OES_draw_buffers_indexed): each color attachment of
// an MRT pass blends its own way, and the state cache keeps it straight
// when states switch. Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::{FramebufferId, ParamGroupId, StateId};
use rswebgl::pass::Pass;
use rswebgl::readback::ReadFormat;
use rswebgl::render_state::{BlendFactor, RenderState, TargetBlend};
use rswebgl::renderbuffer::RenderbufferFormat;

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const NULL: ParamGroupId = ParamGroupId::NULL;

// Writes 0.5 gray to both outputs.
const MRT_FRAG: &str = "#version 300 es\nprecision highp float;\n\
    layout(location = 0) out vec4 a;\nlayout(location = 1) out vec4 b;\n\
    void main() { a = vec4(0.5); b = vec4(0.5); }\n";

fn mrt(f: &Fixture) -> FramebufferId {
    let fb = f.ctx.create_framebuffer(W, H).unwrap();
    for i in 0..2 {
        let rb = f
            .ctx
            .create_renderbuffer(RenderbufferFormat::Rgba8, W, H)
            .unwrap();
        f.ctx.framebuffer_attach_color_renderbuffer(fb, i, rb);
        // Both start at 0.25 gray.
        f.ctx
            .framebuffer_set_color_clear_f32(fb, i, 0.25, 0.25, 0.25, 0.25);
    }
    fb
}

// Draws the middle quad with `state` over a cleared target; reads both
// attachments' red there.
fn draw(f: &Fixture, fb: FramebufferId, state: StateId) -> [u8; 2] {
    let program = f.ctx.create_program(VERT, MRT_FRAG).unwrap();
    let mut pass = Pass::new(fb);
    pass.set_clear(&ClearMask::color());
    pass.push(
        program,
        state,
        f.quad,
        NULL,
        NULL,
        NULL,
        quad(f, 1),
        &quad_cmd(),
    );
    f.ctx.render(&pass);
    let read = |i| {
        f.ctx
            .read_pixels(fb, i, W / 2, H / 2, 1, 1, ReadFormat::Rgba8)
            .unwrap()[0]
    };
    [read(0), read(1)]
}

// Target 0 adds, target 1 replaces.
fn adding_on_zero(f: &Fixture) -> StateId {
    let mut add = TargetBlend::new();
    add.blend = true;
    add.src_rgb = BlendFactor::One;
    add.dst_rgb = BlendFactor::One;
    let mut rs = RenderState::new();
    rs.set_target_blend(0, &add);
    f.ctx.render_state(&rs)
}

#[wasm_bindgen_test]
fn each_target_blends_its_own_way() {
    let f = fixture();
    let fb = mrt(&f);
    let plain = f.ctx.render_state(&RenderState::new());
    // 0.25 + 0.5 = 0.75 → 191; replaced: 0.5 → 128.
    if !f.ctx.can_blend_per_target() {
        // Without the extension the override is dropped (reported): both
        // targets follow the state's own (non-)blending.
        let errors = capture_console("error", || {
            assert_near(draw(&f, fb, adding_on_zero(&f)), [128, 128]);
        });
        assert!(
            errors
                .iter()
                .any(|e| e.contains("OES_draw_buffers_indexed"))
        );
        return;
    }
    assert_near(draw(&f, fb, plain), [128, 128]);
    let per_target = adding_on_zero(&f);
    assert_near(draw(&f, fb, per_target), [191, 128]);
    // Switching back to a state without overrides undoes them (the diff
    // can't leave target 0 blending)…
    assert_near(draw(&f, fb, plain), [128, 128]);
    // …and a global blend still reaches every target.
    let mut all = RenderState::new();
    all.blend = true;
    all.blend_src_rgb = BlendFactor::One;
    all.blend_dst_rgb = BlendFactor::One;
    let all = f.ctx.render_state(&all);
    assert_near(draw(&f, fb, all), [191, 191]);
    assert_near(draw(&f, fb, per_target), [191, 128]);
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

#[wasm_bindgen_test]
fn overrides_past_the_draw_buffers_limit_are_dropped() {
    let f = fixture();
    if !f.ctx.can_blend_per_target() {
        return;
    }
    let mut rs = RenderState::new();
    rs.set_target_blend(1000, &TargetBlend::new());
    let errors = capture_console("error", || {
        let id = f.ctx.render_state(&rs);
        assert_eq!(
            id,
            f.ctx.render_state(&RenderState::new()),
            "same as no overrides"
        );
    });
    assert!(
        errors.iter().any(|e| e.contains("MAX_DRAW_BUFFERS")),
        "{errors:?}"
    );
}

// GPUs round 0.5 to 127 or 128.
#[track_caller]
fn assert_near(got: [u8; 2], want: [u8; 2]) {
    assert!(
        got.iter()
            .zip(want)
            .all(|(g, w)| (*g as i32 - w as i32).abs() <= 1),
        "{got:?} vs {want:?}"
    );
}
