// Implementation-limit checks and report-once warnings.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;
use web_sys::WebGl2RenderingContext;

use rswebgl::buffer::BufferUsage;
use rswebgl::limits;
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
    let errors = capture_console("error", || {
        f.ctx
            .vertex_array_attr(vao, max, buf, &VertexAttr::float(4))
    });
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
        f.ctx
            .texture_storage_2d(tex, 1, &TextureFormat::rgba8(), too_big, 4);
        // 4x4 has 3 mip levels at most.
        f.ctx
            .texture_storage_2d(tex, 4, &TextureFormat::rgba8(), 4, 4);
    });
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(errors[0].contains("MAX_TEXTURE_SIZE"), "{errors:?}");
    assert!(errors[1].contains("mip levels"), "{errors:?}");
    assert_no_gl_error(&f);

    // A valid request still goes through.
    f.ctx
        .texture_storage_2d(tex, 3, &TextureFormat::rgba8(), 4, 4);
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
    let errors = capture_console("error", || {
        f.ctx.framebuffer_attach_color_renderbuffer(fb, index, rb)
    });
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

// A problem hit every frame (here: a wrong-type parameter write) is reported once.
#[wasm_bindgen_test]
fn wrong_type_write_is_reported_once_across_frames() {
    let f = fixture();
    let layout = f
        .ctx
        .create_param_layout(
            &rswebgl::param::ParamLayoutBuilder::new()
                .uniform("u_speed", rswebgl::uniform_buffer::UniformType::F32),
        )
        .unwrap();
    let g = f.ctx.create_param_group(layout).unwrap();
    let field = f.ctx.param_field(layout, "u_speed").unwrap();

    // An int write to a float field, every frame.
    let warnings = capture_console("warn", || {
        for frame in 0..3 {
            f.ctx.param_set_ints(g, field, &[frame]);
        }
    });
    let about: Vec<_> = warnings.iter().filter(|w| w.contains("u_speed")).collect();
    assert_eq!(about.len(), 1, "{warnings:?}");
}

// A sub-image region outside its level is reported and skipped, not a bare
// GL error; one inside still uploads.
#[wasm_bindgen_test]
fn sub_image_outside_its_level_is_rejected() {
    let f = fixture();
    let t = f
        .ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )
        .unwrap();
    let rgba = TextureFormat::rgba8();
    f.ctx.texture_storage_2d(t, 2, &rgba, 4, 4);
    for (level, x, w, why) in [
        (0, 2, 4, "outside level 0"),
        (1, 0, 4, "outside level 1"),
        (2, 0, 1, "level 2"),
    ] {
        let errors = capture_console("error", || {
            f.ctx
                .texture_sub_image_2d(t, level, x, 0, w, 1, &rgba, &vec![0u8; (w * 4) as usize])
        });
        assert!(errors.iter().any(|e| e.contains(why)), "{why}: {errors:?}");
        assert_no_gl_error(&f);
    }
    let errors = capture_console("error", || {
        f.ctx
            .texture_sub_image_2d(t, 1, 0, 0, 2, 2, &rgba, &[0u8; 16])
    });
    assert!(errors.is_empty(), "{errors:?}");
    assert_no_gl_error(&f);
}

// Multisampling is bounded by what the format takes, not only MAX_SAMPLES.
#[wasm_bindgen_test]
fn renderbuffer_samples_are_bounded_per_format() {
    let f = fixture();
    let most = f.ctx.max_samples_for(RenderbufferFormat::Rgba8);
    assert!(most >= 4, "RGBA8 takes {most}");
    assert!(
        f.ctx
            .create_renderbuffer_multisample(RenderbufferFormat::Rgba8, most, 4, 4)
            .is_ok()
    );
    let err = f
        .ctx
        .create_renderbuffer_multisample(RenderbufferFormat::Rgba8, most + 1, 4, 4)
        .expect_err("past the format's samples");
    assert!(err.to_string().contains("samples"), "{err}");
    assert_no_gl_error(&f);
}
