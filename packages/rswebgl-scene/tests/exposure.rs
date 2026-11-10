// Auto exposure: a frame far too bright for exposure 1 (an emissive quad
// filling the view at strength 100) clips to white without it, and with it
// shows its brightness as the grey it targets (0.4 linear, ~170 in sRGB)
// from the first frame (no previous to ease from: set at once). Run with:
// wasm-pack test --headless --chrome packages/rswebgl-scene
#![cfg(target_arch = "wasm32")]

use base64::Engine;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

use rswebgl::context::{Context, ContextOptions};
use rswebgl_gltf::load_model;
use rswebgl_scene::Scene;

wasm_bindgen_test_configure!(run_in_browser);

const SIZE: i32 = 16;

// A 4×4 quad, white emissive × 100 (KHR_materials_emissive_strength), unlit
// black base: it covers the whole orthographic view.
fn asset_url() -> String {
    let mut bin = Vec::new();
    #[rustfmt::skip]
    let floats = [-2.0f32, -2.0, 0.0, 2.0, -2.0, 0.0, 2.0, 2.0, 0.0, -2.0, 2.0, 0.0];
    for v in floats {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u16, 1, 2, 0, 2, 3] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "extensionsUsed": ["KHR_materials_emissive_strength"],
  "buffers": [{{ "byteLength": {len}, "uri": "data:application/octet-stream;base64,{bin}" }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 12, "target": 34963 }}
  ],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
       "min": [-2, -2, 0], "max": [2, 2, 0] }},
    {{ "bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR" }}
  ],
  "materials": [{{ "pbrMetallicRoughness": {{ "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0 }},
                   "emissiveFactor": [1, 1, 1],
                   "extensions": {{ "KHR_materials_emissive_strength": {{ "emissiveStrength": 100 }} }} }}],
  "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0 }}, "indices": 1, "material": 0 }}] }}],
  "nodes": [{{ "mesh": 0 }}],
  "scenes": [{{ "nodes": [0] }}],
  "scene": 0
}}"#,
        len = bin.len(),
        bin = b64(&bin),
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

// The red channel at the center after `frames` frames.
async fn center(auto: bool, frames: usize) -> Option<u8> {
    let canvas: HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    canvas.set_width(SIZE as u32);
    canvas.set_height(SIZE as u32);
    let mut options = ContextOptions::new();
    options.antialias = false;
    let ctx = Context::from_canvas(&canvas, &options).expect("ctx");
    let gl: Gl = canvas
        .get_context("webgl2")
        .unwrap()
        .unwrap()
        .dyn_into()
        .unwrap();
    let mut scene = Scene::new(&ctx).expect("scene");
    if !scene.is_hdr() {
        assert!(
            !scene.set_auto_exposure(true),
            "auto exposure needs the HDR target"
        );
        return None;
    }
    scene.set_tone_mapping("none").unwrap();
    scene.set_dither(false);
    scene.set_samples(0);
    scene.set_environment_intensity(0.0, 0.0, 0.0);
    assert_eq!(scene.set_auto_exposure(auto), auto);
    let camera = scene.spawn();
    let cam =
        js_sys::JSON::parse(r#"{ "type": "orthographic", "ymag": 1, "znear": 0.1, "zfar": 100 }"#)
            .unwrap();
    scene.set_camera(camera, cam).unwrap();
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    scene.add_model(load_model(&ctx, &asset_url()).await.expect("load"));
    wasm_bindgen_futures::JsFuture::from(scene.when_ready())
        .await
        .unwrap();
    for _ in 0..frames {
        scene.render_frame(camera);
    }
    let mut px = [0u8; 4];
    gl.read_pixels_with_opt_u8_array(
        SIZE / 2,
        SIZE / 2,
        1,
        1,
        Gl::RGBA,
        Gl::UNSIGNED_BYTE,
        Some(&mut px),
    )
    .unwrap();
    scene.destroy();
    Some(px[0])
}

#[wasm_bindgen_test]
async fn auto_exposure_brings_a_bright_frame_to_its_grey() {
    let Some(clipped) = center(false, 1).await else {
        return;
    };
    assert_eq!(clipped, 255, "exposure 1: clipped");
    let shown = center(true, 1).await.unwrap();
    // 0.4 linear → 170 sRGB.
    assert!((160..=180).contains(&shown), "auto exposed: {shown}");
    // Stays there.
    let later = center(true, 5).await.unwrap();
    assert!((160..=180).contains(&later), "after a few frames: {later}");
}
