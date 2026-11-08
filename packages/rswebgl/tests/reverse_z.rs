// Reverse-Z (EXT_clip_control): depth funcs and depth clears flip so states
// keep meaning "nearer", and far-away surfaces keep their order.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::{FramebufferId, ParamGroupId};
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::pass::Pass;
use rswebgl::readback::ReadFormat;
use rswebgl::render_state::{DepthFunc, RenderState};
use rswebgl::renderbuffer::RenderbufferFormat;
use rswebgl::uniform_buffer::UniformType;

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const NULL: ParamGroupId = ParamGroupId::NULL;

// A full-screen plane `u_dist` away, through `u_proj` (90° field of view).
const PLANE_VERT: &str = "#version 300 es\nprecision highp float;\n\
    layout(location = 0) in vec2 a_pos;\nuniform mat4 u_proj;\nuniform float u_dist;\n\
    void main() { vec2 xy = (a_pos * 2.0 - 1.0) * u_dist * 2.0;\n\
      gl_Position = u_proj * vec4(xy, -u_dist, 1.0); }\n";
const COLOR_FRAG: &str = "#version 300 es\nprecision highp float;\nuniform vec4 u_color;\n\
    out vec4 o;\nvoid main() { o = u_color; }\n";

const NEAR: f32 = 0.001;

// Column-major projections, 90° field of view, square.
fn gl_perspective(far: f32) -> [f32; 16] {
    let (n, f) = (NEAR, far);
    [
        1.0,
        0.0,
        0.0,
        0.0, //
        0.0,
        1.0,
        0.0,
        0.0, //
        0.0,
        0.0,
        (f + n) / (n - f),
        -1.0, //
        0.0,
        0.0,
        2.0 * f * n / (n - f),
        0.0,
    ]
}

// Reverse, infinite, zero-to-one: depth = near / distance.
fn reverse_infinite() -> [f32; 16] {
    [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 0.0, -1.0, //
        0.0, 0.0, NEAR, 0.0,
    ]
}

fn target(f: &Fixture) -> FramebufferId {
    let fb = f.ctx.create_framebuffer(W, H).unwrap();
    let color = f
        .ctx
        .create_renderbuffer(RenderbufferFormat::Rgba8, W, H)
        .unwrap();
    let depth = f
        .ctx
        .create_renderbuffer(RenderbufferFormat::Depth32f, W, H)
        .unwrap();
    f.ctx.framebuffer_attach_color_renderbuffer(fb, 0, color);
    f.ctx.framebuffer_attach_depth_renderbuffer(fb, depth);
    fb
}

// Blue at 10000.01, then red at 10000 (in front), depth-tested with LESS; the
// pixel in the middle.
fn far_planes(f: &Fixture, fb: FramebufferId, proj: [f32; 16]) -> [u8; 4] {
    let program = f.ctx.create_program(PLANE_VERT, COLOR_FRAG).unwrap();
    let layout = f
        .ctx
        .create_param_layout(
            &ParamLayoutBuilder::new()
                .uniform("u_proj", UniformType::Mat4)
                .uniform("u_dist", UniformType::F32)
                .uniform("u_color", UniformType::Vec4),
        )
        .unwrap();
    let plane = |dist: f32, color: [f32; 4]| {
        let g = f.ctx.create_param_group(layout).unwrap();
        f.ctx.param_set_floats_by_name(g, "u_proj", &proj);
        f.ctx.param_set_floats_by_name(g, "u_dist", &[dist]);
        f.ctx.param_set_floats_by_name(g, "u_color", &color);
        g
    };
    let mut rs = RenderState::new();
    rs.depth_test = true;
    rs.depth_func = DepthFunc::Less;
    let state = f.ctx.render_state(&rs);
    let mut pass = Pass::new(fb);
    pass.set_clear(&ClearMask::color_depth());
    for g in [
        plane(10_000.01, [0.0, 0.0, 1.0, 1.0]),
        plane(10_000.0, [1.0, 0.0, 0.0, 1.0]),
    ] {
        pass.push(program, state, f.quad, g, NULL, NULL, NULL, &quad_cmd());
    }
    f.ctx.render(&pass);
    let px = f
        .ctx
        .read_pixels(fb, 0, W / 2, H / 2, 1, 1, ReadFormat::Rgba8)
        .unwrap();
    [px[0], px[1], px[2], px[3]]
}

#[wasm_bindgen_test]
fn far_surfaces_keep_their_order_under_reverse_z() {
    let f = fixture();
    if !f.ctx.can_reverse_z() {
        assert!(!f.ctx.set_reverse_z(true), "not available: refused");
        assert!(!f.ctx.reverse_z());
        return;
    }
    let fb = target(&f);
    assert!(f.ctx.set_reverse_z(true));
    assert!(f.ctx.reverse_z());
    // The same LESS state and default (1 = far) clear, flipped by the renderer.
    assert_eq!(
        far_planes(&f, fb, reverse_infinite()),
        [255, 0, 0, 255],
        "the nearer one"
    );

    // Off again: a regular projection draws as before (near-plane order).
    assert!(f.ctx.set_reverse_z(false));
    let near_scene = far_planes(&f, fb, gl_perspective(100_000.0));
    // Regular Z can't separate planes 0.01 apart at 10000 with a 0.001 near
    // plane: whichever came first stays. Reverse-Z is what fixed it above.
    assert!(near_scene == [0, 0, 255, 255] || near_scene == [255, 0, 0, 255]);
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}
