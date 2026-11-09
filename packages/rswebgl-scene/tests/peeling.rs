// Depth peeling: glass behind glass. Two thin-walled panes (transmission 1,
// tinted by their base color) in front of a white unlit wall. Without
// peeling the near pane refracts only the opaque scene; with it, the far
// pane too. Run with:
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

const SIZE: i32 = 32;

// Nodes: "near" (red pane, z = 0.5), "far" (green pane, z = 0), "wall"
// (white, unlit, z = −0.5, twice as large).
fn asset_url() -> String {
    let mut bin = Vec::new();
    #[rustfmt::skip]
    let floats = [-0.5f32, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, 0.5, 0.0,
                  0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    for v in floats {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u16, 1, 2, 0, 2, 3] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let pane = |r: f32, g: f32| {
        format!(
            r#"{{ "pbrMetallicRoughness": {{ "baseColorFactor": [{r}, {g}, 0, 1], "metallicFactor": 0, "roughnessFactor": 0 }},
               "extensions": {{ "KHR_materials_transmission": {{ "transmissionFactor": 1 }} }} }}"#
        )
    };
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "extensionsUsed": ["KHR_materials_transmission", "KHR_materials_unlit"],
  "buffers": [{{ "byteLength": {len}, "uri": "data:application/octet-stream;base64,{bin}" }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 96, "byteLength": 12, "target": 34963 }}
  ],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
       "min": [-0.5, -0.5, 0], "max": [0.5, 0.5, 0] }},
    {{ "bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC3" }},
    {{ "bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR" }}
  ],
  "materials": [
    {red},
    {green},
    {{ "pbrMetallicRoughness": {{ "baseColorFactor": [1, 1, 1, 1] }},
       "extensions": {{ "KHR_materials_unlit": {{}} }} }}
  ],
  "meshes": [
    {{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2, "material": 0 }}] }},
    {{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2, "material": 1 }}] }},
    {{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2, "material": 2 }}] }}
  ],
  "nodes": [
    {{ "name": "near", "mesh": 0, "translation": [0, 0, 0.5] }},
    {{ "name": "far", "mesh": 1, "translation": [0, 0, 0] }},
    {{ "name": "wall", "mesh": 2, "translation": [0, 0, -0.5], "scale": [3, 3, 1] }}
  ],
  "scenes": [{{ "nodes": [0, 1, 2] }}],
  "scene": 0
}}"#,
        red = pane(1.0, 0.0),
        green = pane(0.0, 1.0),
        len = bin.len(),
        bin = b64(&bin),
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

#[wasm_bindgen_test]
async fn peeled_glass_sees_the_glass_behind_it() {
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
        return; // peeling needs the HDR target
    }
    scene.set_tone_mapping("none").unwrap();
    scene.set_dither(false);
    // No environment light: the panes only tint what they see.
    scene.set_environment_intensity(0.0, 0.0, 0.0);
    let camera = scene.spawn();
    let cam =
        js_sys::JSON::parse(r#"{ "type": "orthographic", "ymag": 1, "znear": 0.1, "zfar": 100 }"#)
            .unwrap();
    scene.set_camera(camera, cam).unwrap();
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    scene.add_model(load_model(&ctx, &asset_url()).await.expect("load"));

    let center = async |scene: &mut Scene| -> [u8; 4] {
        wasm_bindgen_futures::JsFuture::from(scene.when_ready())
            .await
            .unwrap();
        scene.render_frame(camera);
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
        px
    };
    // One layer: the near pane tints the white wall red.
    let single = center(&mut scene).await;
    assert!(single[0] > 150 && single[1] < 40, "{single:?}");
    // Peeled: through the green pane too — red × green, dark.
    scene.set_transmission_layers(2);
    let peeled = center(&mut scene).await;
    assert!(
        peeled[0] < 40 && peeled[1] < 40,
        "{peeled:?} (single {single:?})"
    );
    assert_eq!(scene.transmission_layers(), 2);
    // Under reverse-Z (flipped comparisons, a float depth buffer) too.
    if scene.set_reverse_z(true) {
        let reversed = center(&mut scene).await;
        assert!(
            reversed[0] < 40 && reversed[1] < 40,
            "reverse-Z: {reversed:?}"
        );
        scene.set_reverse_z(false);
    }
    // And back.
    scene.set_transmission_layers(0);
    assert_eq!(center(&mut scene).await, single);
    scene.destroy();
}
