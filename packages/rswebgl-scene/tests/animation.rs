// Skinning, morph targets and animation, end to end: a quad skinned to a
// bone the animation moves, and a quad whose morph target the animation
// weighs in. Run with:
// wasm-pack test --headless --chrome packages/rswebgl-scene
#![cfg(target_arch = "wasm32")]

use base64::Engine;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

use rswebgl::context::{Context, ContextOptions};
use rswebgl_gltf::load_model;
use rswebgl_scene::Scene;

wasm_bindgen_test_configure!(run_in_browser);

const W: i32 = 96;
const H: i32 = 32;
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

// Nodes: 0 "skinned" (mesh 0, skin 0: every vertex on joint 0), 1 "bone"
// (the joint, at x = −1), 2 "morphing" (mesh 1 at x = +1, one target that
// folds it onto its right edge, weight 0). Animation "Move", linear over 1 s: the bone
// from x = −1 to 0, the target's weight from 0 to 1.
fn asset_url() -> String {
    let mut bin = Vec::new();
    let f = |bin: &mut Vec<u8>, v: &[f32]| v.iter().for_each(|x| bin.extend(x.to_le_bytes()));
    #[rustfmt::skip]
    f(&mut bin, &[-0.5, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, 0.5, 0.0]); // 0..48 quad
    for i in [0u16, 1, 2, 0, 2, 3] {
        bin.extend(i.to_le_bytes()); // 48..60 indices
    }
    bin.extend([0u8; 16]); // 60..76 joints: all joint 0
    f(&mut bin, &[1.0, 0.0, 0.0, 0.0].repeat(4)); // 76..140 weights
    // 140..188 target: the left vertices (0, 3) +1 x — the quad folds to
    // its right edge.
    f(&mut bin, &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    f(&mut bin, &[0.0, 1.0]); // 188..196 times
    f(&mut bin, &[-1.0, 0.0, 0.0, 0.0, 0.0, 0.0]); // 196..220 bone x
    f(&mut bin, &[0.0, 1.0]); // 220..228 weight
    f(&mut bin, &[0.0, 0.0, 1.0].repeat(4)); // 228..276 normals
    f(&mut bin, &[1.0, 0.0, 0.0, 1.0].repeat(4)); // 276..340 tangents
    f(&mut bin, &[0.0, 0.0, 0.0].repeat(4)); // 340..388 normal / tangent deltas
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "extensionsUsed": ["KHR_materials_unlit"],
  "buffers": [{{ "byteLength": {len}, "uri": "data:application/octet-stream;base64,{bin}" }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 12, "target": 34963 }},
    {{ "buffer": 0, "byteOffset": 60, "byteLength": 16 }},
    {{ "buffer": 0, "byteOffset": 76, "byteLength": 64 }},
    {{ "buffer": 0, "byteOffset": 140, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 188, "byteLength": 8 }},
    {{ "buffer": 0, "byteOffset": 196, "byteLength": 24 }},
    {{ "buffer": 0, "byteOffset": 220, "byteLength": 8 }},
    {{ "buffer": 0, "byteOffset": 228, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 276, "byteLength": 64 }},
    {{ "buffer": 0, "byteOffset": 340, "byteLength": 48 }}
  ],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
       "min": [-0.5, -0.5, 0], "max": [0.5, 0.5, 0] }},
    {{ "bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR" }},
    {{ "bufferView": 2, "componentType": 5121, "count": 4, "type": "VEC4" }},
    {{ "bufferView": 3, "componentType": 5126, "count": 4, "type": "VEC4" }},
    {{ "bufferView": 4, "componentType": 5126, "count": 4, "type": "VEC3",
       "min": [0, 0, 0], "max": [1, 0, 0] }},
    {{ "bufferView": 5, "componentType": 5126, "count": 2, "type": "SCALAR",
       "min": [0], "max": [1] }},
    {{ "bufferView": 6, "componentType": 5126, "count": 2, "type": "VEC3" }},
    {{ "bufferView": 7, "componentType": 5126, "count": 2, "type": "SCALAR" }},
    {{ "bufferView": 8, "componentType": 5126, "count": 4, "type": "VEC3" }},
    {{ "bufferView": 9, "componentType": 5126, "count": 4, "type": "VEC4" }},
    {{ "bufferView": 10, "componentType": 5126, "count": 4, "type": "VEC3" }}
  ],
  "materials": [
    {{ "pbrMetallicRoughness": {{ "baseColorFactor": [1, 0, 0, 1] }},
       "extensions": {{ "KHR_materials_unlit": {{}} }} }},
    {{ "pbrMetallicRoughness": {{ "baseColorFactor": [0, 1, 0, 1] }},
       "extensions": {{ "KHR_materials_unlit": {{}} }} }}
  ],
  "meshes": [
    {{ "primitives": [{{ "attributes": {{ "POSITION": 0, "JOINTS_0": 2, "WEIGHTS_0": 3 }},
                        "indices": 1, "material": 0 }}] }},
    {{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 8, "TANGENT": 9 }},
                        "indices": 1, "material": 1,
                        "targets": [{{ "POSITION": 4, "NORMAL": 10, "TANGENT": 10 }}] }}],
       "weights": [0] }}
  ],
  "skins": [{{ "joints": [1] }}],
  "animations": [{{
    "name": "Move",
    "samplers": [
      {{ "input": 5, "output": 6, "interpolation": "LINEAR" }},
      {{ "input": 5, "output": 7, "interpolation": "LINEAR" }}
    ],
    "channels": [
      {{ "sampler": 0, "target": {{ "node": 1, "path": "translation" }} }},
      {{ "sampler": 1, "target": {{ "node": 2, "path": "weights" }} }}
    ]
  }}],
  "nodes": [
    {{ "name": "skinned", "mesh": 0, "skin": 0 }},
    {{ "name": "bone", "translation": [-1, 0, 0] }},
    {{ "name": "morphing", "mesh": 1, "translation": [1, 0, 0] }}
  ],
  "scenes": [{{ "nodes": [0, 1, 2] }}],
  "scene": 0
}}"#,
        len = bin.len(),
        bin = b64(&bin),
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

#[wasm_bindgen_test]
async fn skins_and_morph_targets_follow_the_animation() {
    let canvas: HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    canvas.set_width(W as u32);
    canvas.set_height(H as u32);
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
    scene.set_tone_mapping("none").unwrap();
    scene.set_dither(false);
    let camera = scene.spawn();
    let cam = js_sys::JSON::parse(
        r#"{ "type": "orthographic", "ymag": 0.5, "znear": 0.1, "zfar": 100 }"#,
    )
    .unwrap();
    scene.set_camera(camera, cam).unwrap();
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    let root = scene.add_model(load_model(&ctx, &asset_url()).await.expect("load"));
    assert_eq!(scene.animations(root), ["Move"]);
    assert!((scene.animation_duration(root, 0) - 1.0).abs() < 1e-6);

    let mut frame = async |scene: &mut Scene| -> [[u8; 4]; 3] {
        wasm_bindgen_futures::JsFuture::from(scene.when_ready())
            .await
            .unwrap();
        scene.render_frame(camera);
        let mut out = [[0u8; 4]; 3];
        for (i, px) in out.iter_mut().enumerate() {
            gl.read_pixels_with_opt_u8_array(
                16 + 32 * i as i32,
                H / 2,
                1,
                1,
                Gl::RGBA,
                Gl::UNSIGNED_BYTE,
                Some(&mut px[..]),
            )
            .unwrap();
        }
        out
    };
    // At rest: the skinned quad where its bone is, the morph target off.
    assert_eq!(frame(&mut scene).await, [RED, BLACK, GREEN]);
    scene.play_animation(root, 0, false).unwrap();
    assert_eq!(frame(&mut scene).await, [RED, BLACK, GREEN], "t = 0");
    // Halfway the morph quad straddles x = 1.5 and its center moved off.
    scene.advance_animations(1.0);
    assert_eq!(scene.animation_time(root), Some(1.0));
    // The bone at x = 0 carries the quad; the target folded the green quad
    // onto its right edge.
    assert_eq!(frame(&mut scene).await, [BLACK, RED, BLACK], "t = 1");
    // Not looping: it holds the last pose.
    scene.advance_animations(0.5);
    assert_eq!(scene.animation_time(root), Some(1.0));
    assert!(scene.play_animation(root, 3, true).is_err());
    let _ = JsValue::NULL;
    scene.destroy();
}
