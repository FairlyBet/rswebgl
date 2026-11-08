// Reading pixels back: synchronous and fenced (non-stalling), canvas and
// framebuffers, byte and float formats, and the error paths.
// Run with: wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

mod common;

use wasm_bindgen_test::*;

use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::{FramebufferId, ParamGroupId, ProgramId, StateId};
use rswebgl::pass::Pass;
use rswebgl::readback::ReadFormat;
use rswebgl::render_state::RenderState;
use rswebgl::renderbuffer::RenderbufferFormat;

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

// Draws the middle quad with `program` into `target`, cleared first.
fn draw_middle(f: &Fixture, target: FramebufferId, program: ProgramId) {
    let mut pass = Pass::new(target);
    pass.set_clear(&ClearMask::color());
    pass.push(
        program,
        state(f),
        f.quad,
        NULL,
        NULL,
        NULL,
        quad(f, 1),
        &quad_cmd(),
    );
    f.ctx.render(&pass);
}

// The center pixel of the middle quad, as read_quads samples it.
const MID: (i32, i32) = (W / 6 + W / 3, H / 2);

#[wasm_bindgen_test]
fn sync_read_of_the_canvas() {
    let f = fixture();
    draw_middle(&f, CANVAS, solid_program(&f, "1.0, 0.0, 0.0, 1.0"));
    let px = f
        .ctx
        .read_pixels(CANVAS, 0, MID.0, MID.1, 1, 1, ReadFormat::Rgba8)
        .unwrap();
    assert_eq!(px, RED);
    // A row: 3 pixels across the left edge of the middle quad's column.
    let row = f
        .ctx
        .read_pixels(CANVAS, 0, 0, MID.1, W, 1, ReadFormat::Rgba8)
        .unwrap();
    assert_eq!(row.len(), W as usize * 4);
    assert_eq!(&row[MID.0 as usize * 4..][..4], &RED);
    assert_eq!(&row[..4], &BLACK);
}

// The async read sees the canvas as it was when issued, even though the next
// frame repaints it before the result arrives.
#[wasm_bindgen_test]
async fn async_read_is_a_snapshot_at_issue_time() {
    let f = fixture();
    let red = solid_program(&f, "1.0, 0.0, 0.0, 1.0");
    let green = solid_program(&f, "0.0, 1.0, 0.0, 1.0");
    draw_middle(&f, CANVAS, red);

    let read = f
        .ctx
        .read_pixels_async(CANVAS, 0, MID.0, MID.1, 1, 1, ReadFormat::Rgba8);
    let promise = f
        .ctx
        .read_pixels_promise(CANVAS, 0, MID.0, MID.1, 1, 1, ReadFormat::Rgba8);
    // The next frame is drawn before either result is awaited.
    draw_middle(&f, CANVAS, green);
    assert_eq!(read.await.unwrap(), RED);
    let bytes = wasm_bindgen_futures::JsFuture::from(promise).await.unwrap();
    assert_eq!(js_sys::Uint8Array::new(&bytes).to_vec(), RED);
    assert_eq!(read_quads(&f.gl)[1], GREEN);
}

#[wasm_bindgen_test]
async fn async_read_of_a_float_framebuffer() {
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
    // Values a byte format couldn't hold.
    draw_middle(&f, fb, solid_program(&f, "0.25, -2.0, 1000.5, 1.0"));

    let bytes = f
        .ctx
        .read_pixels_async(fb, 0, MID.0, MID.1, 1, 1, ReadFormat::RgbaF32)
        .await
        .unwrap();
    let px: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    assert_eq!(px, [0.25, -2.0, 1000.5, 1.0]);
}

#[wasm_bindgen_test]
async fn bad_reads_are_errors() {
    let f = fixture();
    let out_of_bounds = f
        .ctx
        .read_pixels(CANVAS, 0, W - 1, 0, 2, 1, ReadFormat::Rgba8);
    assert!(out_of_bounds.unwrap_err().contains("outside"));

    let fb = f.ctx.create_framebuffer(4, 4).unwrap();
    let no_attachment = f.ctx.read_pixels(fb, 0, 0, 0, 1, 1, ReadFormat::Rgba8);
    assert!(no_attachment.unwrap_err().contains("no color attachment"));

    // JS gets a rejected promise carrying an Error.
    let promise = f
        .ctx
        .read_pixels_promise(CANVAS, 1, 0, 0, 1, 1, ReadFormat::Rgba8);
    let e = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .unwrap_err();
    assert!(wasm_bindgen::JsCast::is_instance_of::<js_sys::Error>(&e));
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

// A read still in flight when the context goes fails instead of hanging.
#[wasm_bindgen_test]
async fn context_loss_fails_a_pending_read() {
    let f = fixture();
    draw_middle(&f, CANVAS, solid_program(&f, "1.0, 0.0, 0.0, 1.0"));
    let promise = f
        .ctx
        .read_pixels_promise(CANVAS, 0, MID.0, MID.1, 1, 1, ReadFormat::Rgba8);
    assert!(f.ctx.simulate_context_loss());
    let e = wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .unwrap_err();
    let msg = js_sys::Error::from(e).message();
    assert!(String::from(msg).contains("lost"));
}

// WebGL reports these as GL errors, not exceptions: without a check they'd
// "succeed" with zeros.
#[wasm_bindgen_test]
async fn wrong_format_and_multisampled_reads_are_errors() {
    let f = fixture();
    let err = f
        .ctx
        .read_pixels(CANVAS, 0, 0, 0, 1, 1, ReadFormat::RgbaF32)
        .unwrap_err();
    assert!(err.contains("reads as Rgba8"), "{err}");

    if f.ctx
        .can_render_to_renderbuffer(RenderbufferFormat::Rgba32f)
    {
        let fb = f.ctx.create_framebuffer(4, 4).unwrap();
        let rb = f
            .ctx
            .create_renderbuffer(RenderbufferFormat::Rgba32f, 4, 4)
            .unwrap();
        f.ctx.framebuffer_attach_color_renderbuffer(fb, 0, rb);
        let err = f
            .ctx
            .read_pixels_async(fb, 0, 0, 0, 1, 1, ReadFormat::Rgba8)
            .await
            .unwrap_err();
        assert!(err.contains("reads as RgbaF32"), "{err}");
    }

    let msaa = f.ctx.create_framebuffer(4, 4).unwrap();
    let rb = f
        .ctx
        .create_renderbuffer_multisample(RenderbufferFormat::Rgba8, 4, 4, 4)
        .unwrap();
    f.ctx.framebuffer_attach_color_renderbuffer(msaa, 0, rb);
    let err = f
        .ctx
        .read_pixels(msaa, 0, 0, 0, 1, 1, ReadFormat::Rgba8)
        .unwrap_err();
    assert!(err.contains("multisampled"), "{err}");
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

// Lost and restored while a read waits: the read fails as lost, and nothing
// of the old context is touched on the new one (no GL errors left behind).
#[wasm_bindgen_test]
async fn a_read_across_loss_and_restore_fails_cleanly() {
    let f = fixture();
    draw_middle(&f, CANVAS, solid_program(&f, "1.0, 0.0, 0.0, 1.0"));
    let read = f
        .ctx
        .read_pixels_async(CANVAS, 0, MID.0, MID.1, 1, 1, ReadFormat::Rgba8);
    assert!(f.ctx.simulate_context_loss());
    // Let the loss event arrive, then restore before the read is polled.
    sleep_ms(20).await;
    assert!(f.ctx.simulate_context_restore());
    sleep_ms(20).await;
    assert!(!f.ctx.is_lost());
    let err = read.await.unwrap_err();
    assert!(err.contains("lost"), "{err}");
    assert_eq!(f.gl.get_error(), web_sys::WebGl2RenderingContext::NO_ERROR);
}

async fn sleep_ms(ms: i32) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    });
    wasm_bindgen_futures::JsFuture::from(p).await.unwrap();
}

// Many reads in flight poll on one shared timer, not one each.
#[wasm_bindgen_test]
async fn reads_in_flight_share_one_timer() {
    let f = fixture();
    let red = solid_program(&f, "1.0, 0.0, 0.0, 1.0");
    draw_middle(&f, CANVAS, red);
    // Count setTimeout calls while the reads are pending.
    let restore: js_sys::Function = js_sys::Function::new_no_args(
        "const o = globalThis.setTimeout; globalThis.__timers = 0; \
         globalThis.setTimeout = function (...a) { globalThis.__timers++; return o.apply(this, a); }; \
         return () => { globalThis.setTimeout = o; };",
    )
    .call0(&wasm_bindgen::JsValue::NULL)
    .unwrap()
    .into();
    const N: usize = 20;
    let reads: js_sys::Array = (0..N)
        .map(|_| {
            f.ctx
                .read_pixels_promise(CANVAS, 0, MID.0, MID.1, 1, 1, ReadFormat::Rgba8)
        })
        .collect();
    let all = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::all(&reads)).await;
    restore.call0(&wasm_bindgen::JsValue::NULL).unwrap();
    let all: js_sys::Array = all.unwrap().into();
    for bytes in all.iter() {
        assert_eq!(js_sys::Uint8Array::new(&bytes).to_vec(), RED);
    }
    let timers = js_sys::Reflect::get(&js_sys::global(), &"__timers".into())
        .unwrap()
        .as_f64()
        .unwrap() as usize;
    assert!(timers < N, "{timers} timers for {N} reads");
    assert_eq!(f.ctx.poll_timers_pending(), 0);
}
