// Implementation-limit checks and report-once warnings.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;
use web_sys::WebGl2RenderingContext;

use rswebgl::buffer::BufferUsage;
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::limits;
use rswebgl::pass::Batch;
use rswebgl::render_state::RenderState;
use rswebgl::renderbuffer::RenderbufferFormat;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};

use rswebgl::vao::VertexAttr;
use rswebgl::vao_builder::{AttrKind, VaoBuilder};

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

fn assert_no_gl_error(f: &Fixture) {
    assert_eq!(f.gl.get_error(), WebGl2RenderingContext::NO_ERROR);
}

#[wasm_bindgen_test]
fn limits_are_queried_from_the_context() {
    let _f = fixture();
    // Every WebGL2 implementation is at least at the spec minimums.
    assert!(limits::max_vertex_attribs() >= 16);
    assert!(limits::max_texture_size() >= 2048);
    assert!(limits::max_color_attachments() >= 4);
    assert!(limits::max_samples() >= 4);
}

#[wasm_bindgen_test]
fn vertex_attr_beyond_limit_is_rejected() {
    let f = fixture();
    let vao = f.ctx.create_vertex_array().unwrap();
    let buf = f
        .ctx
        .create_buffer(BufferUsage::StaticDraw, &[0; 16])
        .unwrap();
    let max = limits::max_vertex_attribs();
    let errors = capture_console("error", || vao.attr(max, &buf, &VertexAttr::float(4)));
    assert!(
        errors.iter().any(|e| e.contains("MAX_VERTEX_ATTRIBS")),
        "{errors:?}"
    );
    assert_no_gl_error(&f);

    let mut vb = VaoBuilder::new(&f.ctx);
    vb.add_f32(max, AttrKind::F32x2, &[0.0, 0.0]);
    let err = vb.build().expect_err("index beyond limit");
    assert!(err.contains("MAX_VERTEX_ATTRIBS"), "{err}");
}

#[wasm_bindgen_test]
fn texture_storage_beyond_limit_is_rejected() {
    let f = fixture();
    let tex = f
        .ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )
        .unwrap();
    let too_big = limits::max_texture_size() as i32 + 1;
    let errors = capture_console("error", || {
        tex.storage_2d(1, &TextureFormat::rgba8(), too_big, 4);
        // 4x4 has 3 mip levels at most.
        tex.storage_2d(4, &TextureFormat::rgba8(), 4, 4);
    });
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].contains("MAX_TEXTURE_SIZE"), "{errors:?}");
    assert!(errors[1].contains("mip levels"), "{errors:?}");
    assert_no_gl_error(&f);

    // A valid request still goes through.
    tex.storage_2d(3, &TextureFormat::rgba8(), 4, 4);
    assert_no_gl_error(&f);
}

#[wasm_bindgen_test]
fn color_attachment_beyond_limit_is_rejected() {
    let f = fixture();
    let fb = f.ctx.create_framebuffer(4, 4).unwrap();
    let rb = f
        .ctx
        .create_renderbuffer(RenderbufferFormat::Rgba8, 4, 4)
        .unwrap();
    let index = limits::max_color_attachments().min(limits::max_draw_buffers());
    let errors = capture_console("error", || fb.attach_color_renderbuffer(index, &rb));
    assert!(
        errors.iter().any(|e| e.contains("MAX_COLOR_ATTACHMENTS")),
        "{errors:?}"
    );
    assert_no_gl_error(&f);
}

#[wasm_bindgen_test]
fn renderbuffer_beyond_limit_fails_to_create() {
    let f = fixture();
    let too_big = limits::max_renderbuffer_size() as i32 + 1;
    let err = f
        .ctx
        .create_renderbuffer(RenderbufferFormat::Rgba8, too_big, 4)
        .expect_err("too big");
    assert!(err.contains("MAX_RENDERBUFFER_SIZE"), "{err}");

    let too_many = limits::max_samples() as i32 + 1;
    let err = f
        .ctx
        .create_renderbuffer_multisample(RenderbufferFormat::Rgba8, too_many, 4, 4)
        .expect_err("too many samples");
    assert!(err.contains("MAX_SAMPLES"), "{err}");
    assert_no_gl_error(&f);
}

// An unknown uniform name set every frame is reported once, not per frame.
#[wasm_bindgen_test]
fn unknown_uniform_is_reported_once_across_frames() {
    const FRAG: &str = r#"#version 300 es
precision highp float;
out vec4 frag_color;
void main() {
    frag_color = vec4(1.0);
}
"#;
    let f = fixture();
    let program = f.ctx.create_program(VERT, FRAG).unwrap();
    let u = quad_uniforms(1, 0);
    u.set_float("u_no_such_uniform", 1.0);
    let cmd = DrawCommand::elements(DrawMode::Triangles, 6, IndexType::UnsignedShort, 0);

    let warnings = capture_console("warn", || {
        for frame in 0..3 {
            u.set_float("u_no_such_uniform", frame as f32);
            let mut batch = Batch::new(&program, &RenderState::new());
            batch.draw(&f.quad, &u, cmd);
            render(&f, vec![batch]);
        }
    });
    let about: Vec<_> = warnings
        .iter()
        .filter(|w| w.contains("u_no_such_uniform"))
        .collect();
    assert_eq!(about.len(), 1, "{warnings:?}");
}
