// GPU-side copies: the MSAA resolve as a pass store op, framebuffer blits,
// buffer-to-buffer and framebuffer-to-texture copies, buffer readback.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::buffer::BufferUsage;
use rswebgl::draw::Viewport;
use rswebgl::framebuffer::{ClearMask, InvalidateMask};
use rswebgl::ids::{FramebufferId, ParamGroupId, ProgramId, StateId, TextureId};
use rswebgl::pass::Pass;
use rswebgl::readback::ReadFormat;
use rswebgl::render_state::RenderState;
use rswebgl::renderbuffer::RenderbufferFormat;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};

use common::*;

wasm_bindgen_test_configure!(run_in_browser);

const NULL: ParamGroupId = ParamGroupId::NULL;
const CANVAS: FramebufferId = FramebufferId::NULL;

fn solid_program(f: &Fixture, rgba: &str) -> ProgramId {
    let frag = format!(
        "#version 300 es\nprecision highp float;\nout vec4 frag_color;\n\
         void main() {{ frag_color = vec4({rgba}); }}\n"
    );
    f.ctx.create_program(VERT, &frag).unwrap()
}

fn state(f: &Fixture) -> StateId {
    f.ctx.render_state(&RenderState::new())
}

fn quad_pass(
    f: &Fixture,
    target: FramebufferId,
    program: ProgramId,
    col: i32,
    clear: bool,
) -> Pass {
    let mut pass = Pass::new(target);
    if clear {
        pass.set_clear(&ClearMask::color());
    }
    pass.push(
        program,
        state(f),
        f.quad,
        NULL,
        NULL,
        NULL,
        quad(f, col),
        &quad_cmd(),
    );
    pass
}

fn msaa_framebuffer(f: &Fixture) -> FramebufferId {
    let fb = f.ctx.create_framebuffer(W, H).unwrap();
    let rb = f
        .ctx
        .create_renderbuffer_multisample(RenderbufferFormat::Rgba8, 4, W, H)
        .unwrap();
    f.ctx.framebuffer_attach_color_renderbuffer(fb, 0, rb);
    fb
}

fn texture_framebuffer(f: &Fixture, w: i32, h: i32) -> (FramebufferId, TextureId) {
    let tex = f
        .ctx
        .create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )
        .unwrap();
    f.ctx
        .texture_storage_2d(tex, 1, &TextureFormat::rgba8(), w, h);
    let fb = f.ctx.create_framebuffer(w, h).unwrap();
    f.ctx.framebuffer_attach_color_texture_2d(fb, 0, tex, 0);
    (fb, tex)
}

// Render into a multisampled framebuffer, resolve into the canvas, discard the
// samples. The next pass into the MSAA target must really draw there (the
// resolve rebound DRAW behind the state cache).
#[wasm_bindgen_test]
fn pass_resolves_msaa_into_the_canvas() {
    let f = fixture();
    let msaa = msaa_framebuffer(&f);
    let red = solid_program(&f, "1.0, 0.0, 0.0, 1.0");
    let green = solid_program(&f, "0.0, 1.0, 0.0, 1.0");

    let mut pass = quad_pass(&f, msaa, red, 1, true);
    pass.set_resolve(CANVAS);
    pass.set_invalidate(&InvalidateMask::all());
    f.ctx.render(&pass);
    assert_eq!(read_quads(&f.gl), [BLACK, RED, BLACK]);

    f.ctx.render(&quad_pass(&f, msaa, green, 0, false));
    // Through the context: a raw readPixels would now read the MSAA target.
    let left = f
        .ctx
        .read_pixels(CANVAS, 0, W / 6, H / 2, 1, 1, ReadFormat::Rgba8)
        .unwrap();
    assert_eq!(left, BLACK, "drew into the canvas");
}

#[wasm_bindgen_test]
fn pass_resolves_msaa_into_a_texture() {
    let f = fixture();
    let msaa = msaa_framebuffer(&f);
    let (resolved, _) = texture_framebuffer(&f, W, H);
    let mut pass = quad_pass(&f, msaa, solid_program(&f, "1.0, 0.0, 0.0, 1.0"), 1, true);
    pass.set_resolve(resolved);
    f.ctx.render(&pass);
    let px = f
        .ctx
        .read_pixels(resolved, 0, W / 2, H / 2, 1, 1, ReadFormat::Rgba8)
        .unwrap();
    assert_eq!(px, RED);
}

#[wasm_bindgen_test]
fn resolve_of_a_different_size_is_reported_and_skipped() {
    let f = fixture();
    let msaa = msaa_framebuffer(&f);
    let (small, _) = texture_framebuffer(&f, 4, 4);
    let mut pass = quad_pass(&f, msaa, solid_program(&f, "1.0, 0.0, 0.0, 1.0"), 1, true);
    pass.set_resolve(small);
    let errors = capture_console("error", || f.ctx.render(&pass));
    assert!(
        errors.iter().any(|e| e.contains("sizes must match")),
        "{errors:?}"
    );
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

// The canvas scaled down to 3×1: each output pixel takes the centre of one
// third.
#[wasm_bindgen_test]
fn blit_scales_between_framebuffers() {
    let f = fixture();
    f.ctx.render(&quad_pass(
        &f,
        CANVAS,
        solid_program(&f, "1.0, 0.0, 0.0, 1.0"),
        1,
        true,
    ));
    let (small, _) = texture_framebuffer(&f, 3, 1);
    f.ctx.blit_framebuffer(
        CANVAS,
        &Viewport::new(0, 0, W, H),
        small,
        &Viewport::new(0, 0, 3, 1),
        &ClearMask::color(),
        TextureMagFilter::Nearest,
    );
    let px = f
        .ctx
        .read_pixels(small, 0, 0, 0, 3, 1, ReadFormat::Rgba8)
        .unwrap();
    assert_eq!(px, [BLACK, RED, BLACK].concat());

    // Depth with linear filtering isn't allowed: reported, nothing happens.
    let errors = capture_console("error", || {
        f.ctx.blit_framebuffer(
            CANVAS,
            &Viewport::new(0, 0, W, H),
            small,
            &Viewport::new(0, 0, 3, 1),
            &ClearMask::depth(),
            TextureMagFilter::Linear,
        )
    });
    assert!(errors.iter().any(|e| e.contains("Nearest")), "{errors:?}");
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

#[wasm_bindgen_test]
async fn buffer_copy_and_readback() {
    let f = fixture();
    let src = f
        .ctx
        .create_buffer(BufferUsage::StaticDraw, &(0u8..16).collect::<Vec<_>>())
        .unwrap();
    let dst = f
        .ctx
        .create_empty_buffer(BufferUsage::StaticCopy, 16)
        .unwrap();
    f.ctx.copy_buffer(src, 4, dst, 2, 8);

    let sync = f.ctx.buffer_read(dst, 0, 12).unwrap();
    assert_eq!(sync, [0, 0, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0]);

    // The async read is a snapshot: a write after issuing it doesn't show.
    let read = f.ctx.buffer_read_async(dst, 2, 4);
    f.ctx.buffer_write(dst, 2, &[99, 99, 99, 99]);
    assert_eq!(read.await.unwrap(), [4, 5, 6, 7]);

    // Overruns and overlaps are reported and do nothing.
    let errors = capture_console("error", || {
        f.ctx.copy_buffer(src, 12, dst, 0, 8);
        f.ctx.copy_buffer(src, 0, src, 4, 8);
    });
    assert_eq!(errors.len(), 2, "{errors:?}");
    assert!(f.ctx.buffer_read(src, 0, 20).is_err());
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

#[wasm_bindgen_test]
fn copy_from_the_canvas_into_a_texture() {
    let f = fixture();
    f.ctx.render(&quad_pass(
        &f,
        CANVAS,
        solid_program(&f, "0.0, 0.0, 1.0, 1.0"),
        1,
        true,
    ));
    let (fb, tex) = texture_framebuffer(&f, 2, 1);
    // Left texel from the black first column, right from the blue middle.
    f.ctx
        .copy_texture_sub_image_2d(tex, 0, 0, 0, CANVAS, 0, W / 6, H / 2, 1, 1);
    f.ctx
        .copy_texture_sub_image_2d(tex, 0, 1, 0, CANVAS, 0, W / 2, H / 2, 1, 1);
    let px = f
        .ctx
        .read_pixels(fb, 0, 0, 0, 2, 1, ReadFormat::Rgba8)
        .unwrap();
    assert_eq!(px, [BLACK, BLUE].concat());
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}
