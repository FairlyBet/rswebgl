// Auto-resize of the default framebuffer: the drawing buffer follows the
// canvas's CSS size, and dropping the context stops the ResizeObserver
// (otherwise it calls a dropped closure). Run with:
// wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;
use web_sys::HtmlCanvasElement;

use rswebgl::context::{Context, ContextOptions};

wasm_bindgen_test_configure!(run_in_browser);

// Lets the browser lay out and deliver resize observations.
async fn settle() {
    let p: js_sys::Promise = js_sys::eval("new Promise(r => setTimeout(r, 150))")
        .unwrap()
        .unchecked_into();
    JsFuture::from(p).await.unwrap();
}

#[wasm_bindgen_test]
async fn auto_resize_follows_the_canvas_and_stops_with_the_context() {
    let document = web_sys::window().unwrap().document().unwrap();
    let canvas: HtmlCanvasElement = document
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    canvas
        .set_attribute("style", "display: block; width: 40px; height: 30px")
        .unwrap();
    document.body().unwrap().append_child(&canvas).unwrap();
    js_sys::eval(
        "window.__resize_errors = []; \
         addEventListener('error', e => __resize_errors.push(String(e.message)))",
    )
    .unwrap();

    let ctx = Context::from_canvas(&canvas, &ContextOptions::new()).expect("ctx");
    let fb = ctx.default_framebuffer();
    fb.enable_auto_resize().expect("auto-resize");
    settle().await;
    let dpr = web_sys::window().unwrap().device_pixel_ratio();
    assert_eq!(fb.width(), (40.0 * dpr).round() as i32);
    assert_eq!(canvas.width(), (40.0 * dpr).round() as u32);

    drop(fb);
    drop(ctx);
    canvas
        .set_attribute("style", "display: block; width: 64px; height: 30px")
        .unwrap();
    settle().await;
    let errors = js_sys::eval("__resize_errors.join('; ')").unwrap();
    assert_eq!(errors.as_string().unwrap(), "");
    // Nothing resizes it any more.
    assert_eq!(canvas.width(), (40.0 * dpr).round() as u32);
    canvas.remove();
}
