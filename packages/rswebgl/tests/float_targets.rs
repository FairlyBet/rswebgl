// Float render targets: the context enables what the device has, float
// textures and renderbuffers render HDR values, and what can't work is said
// up front.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::extension::Extension;
use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::{FramebufferId, ParamGroupId};
use rswebgl::pass::Pass;
use rswebgl::readback::ReadFormat;
use rswebgl::render_state::RenderState;
use rswebgl::renderbuffer::RenderbufferFormat;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const NULL: ParamGroupId = ParamGroupId::NULL;

fn supported(f: &Fixture, name: &str) -> bool {
    f.gl.get_supported_extensions()
        .is_some_and(|list| list.includes(&name.into(), 0))
}

// Draws the middle quad in `rgba` into `fb` and reads its centre as floats.
fn draw_and_read(f: &Fixture, fb: FramebufferId, rgba: &str) -> Vec<f32> {
    let frag = format!(
        "#version 300 es\nprecision highp float;\nout vec4 frag_color;\n\
         void main() {{ frag_color = vec4({rgba}); }}\n"
    );
    let program = f.ctx.create_program(VERT, &frag).unwrap();
    let mut pass = Pass::new(fb);
    pass.set_clear(&ClearMask::color());
    pass.push(
        program,
        f.ctx.render_state(&RenderState::new()),
        f.quad,
        NULL,
        NULL,
        NULL,
        quad(f, 1),
        &quad_cmd(),
    );
    f.ctx.render(&pass);
    let bytes = f
        .ctx
        .read_pixels(fb, 0, W / 2, H / 2, 1, 1, ReadFormat::RgbaF32)
        .unwrap();
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

#[wasm_bindgen_test]
fn the_context_enables_float_extensions_the_device_has() {
    let f = fixture();
    for ext in [
        Extension::ExtColorBufferFloat,
        Extension::ExtColorBufferHalfFloat,
        Extension::OesTextureFloatLinear,
        Extension::ExtFloatBlend,
    ] {
        assert_eq!(
            f.ctx.is_extension_enabled(ext.clone()),
            supported(&f, ext.name()),
            "{}",
            ext.name()
        );
    }
    let half =
        supported(&f, "EXT_color_buffer_float") || supported(&f, "EXT_color_buffer_half_float");
    assert_eq!(f.ctx.can_render_to(&TextureFormat::rgba16f()), half);
    assert_eq!(
        f.ctx
            .can_render_to_renderbuffer(RenderbufferFormat::Rgba32f),
        supported(&f, "EXT_color_buffer_float")
    );
    assert!(f.ctx.can_render_to(&TextureFormat::rgba8()));
}

// Values outside [0, 1] survive: what HDR and bloom need.
#[wasm_bindgen_test]
fn renders_hdr_into_a_half_float_texture() {
    let f = fixture();
    if !f.ctx.can_render_to(&TextureFormat::rgba16f()) {
        return;
    }
    let tex = f
        .ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Linear,
            TextureMagFilter::Linear,
        )
        .unwrap();
    f.ctx
        .texture_storage_2d(tex, 1, &TextureFormat::rgba16f(), W, H);
    let fb = f.ctx.create_framebuffer(W, H).unwrap();
    f.ctx.framebuffer_attach_color_texture_2d(fb, 0, tex, 0);
    f.ctx.check_framebuffer(fb).unwrap();
    assert_eq!(
        draw_and_read(&f, fb, "4.0, -0.5, 0.125, 1.0"),
        [4.0, -0.5, 0.125, 1.0]
    );
}

#[wasm_bindgen_test]
fn renders_into_a_float32_renderbuffer() {
    let f = fixture();
    if !f
        .ctx
        .can_render_to_renderbuffer(RenderbufferFormat::Rgba32f)
    {
        return;
    }
    let fb = f.ctx.create_framebuffer(W, H).unwrap();
    let rb = f
        .ctx
        .create_renderbuffer(RenderbufferFormat::Rgba32f, W, H)
        .unwrap();
    f.ctx.framebuffer_attach_color_renderbuffer(fb, 0, rb);
    f.ctx.check_framebuffer(fb).unwrap();
    assert_eq!(
        draw_and_read(&f, fb, "1000.25, -3.0, 0.1, 1.0"),
        [1000.25, -3.0, 0.1, 1.0]
    );
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

// A linearly filtered 32F texture is fine with OES_texture_float_linear and
// warned about without it (it would sample as black).
#[wasm_bindgen_test]
fn linear_float32_textures_warn_only_without_the_extension() {
    let f = fixture();
    let make = |min, mag| {
        let tex = f
            .ctx
            .create_texture(TextureTarget::Texture2D, min, mag)
            .unwrap();
        f.ctx
            .texture_storage_2d(tex, 1, &TextureFormat::rgba32f(), 4, 4);
    };
    let linear = capture_console("warn", || {
        make(TextureMinFilter::Linear, TextureMagFilter::Linear)
    });
    let nearest = capture_console("warn", || {
        make(TextureMinFilter::Nearest, TextureMagFilter::Nearest)
    });
    assert!(nearest.is_empty(), "{nearest:?}");
    assert_eq!(linear.is_empty(), f.ctx.can_filter_float32(), "{linear:?}");
}
