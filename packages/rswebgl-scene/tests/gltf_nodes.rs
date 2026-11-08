// glTF cameras and KHR_lights_punctual lights: their nodes become camera and
// light entities that render and light like the app's own. Run with:
// wasm-pack test --headless --chrome packages/rswebgl-scene
#![cfg(target_arch = "wasm32")]

use base64::Engine;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

use rswebgl::context::{Context, ContextOptions};
use rswebgl_ecs::Entity;
use rswebgl_gltf::load_model;
use rswebgl_scene::Scene;

wasm_bindgen_test_configure!(run_in_browser);

const SIZE: i32 = 32;

// A white rough quad at the origin facing +Z; a camera 3 m in front of it
// with no zfar (infinite); a point light 1 m in front; a spot light far off
// to the side, aimed away; an orthographic camera under it.
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
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "extensionsUsed": ["KHR_lights_punctual"],
  "extensions": {{ "KHR_lights_punctual": {{ "lights": [
    {{ "type": "point", "intensity": 3, "color": [1, 1, 1] }},
    {{ "type": "spot", "intensity": 100, "range": 5, "spot": {{ "outerConeAngle": 0.3 }} }}
  ] }} }},
  "cameras": [
    {{ "type": "perspective", "perspective": {{ "yfov": 0.6, "znear": 0.1 }} }},
    {{ "type": "orthographic", "orthographic": {{ "xmag": 2, "ymag": 1, "znear": 0.1, "zfar": 50 }} }}
  ],
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
  "materials": [{{ "pbrMetallicRoughness": {{ "metallicFactor": 0, "roughnessFactor": 1 }} }}],
  "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2, "material": 0 }}] }}],
  "nodes": [
    {{ "name": "quad", "mesh": 0 }},
    {{ "name": "eye", "camera": 0, "translation": [0, 0, 3] }},
    {{ "name": "bulb", "translation": [0, 0, 1], "extensions": {{ "KHR_lights_punctual": {{ "light": 0 }} }} }},
    {{ "name": "spot", "translation": [30, 0, 0], "children": [4],
       "extensions": {{ "KHR_lights_punctual": {{ "light": 1 }} }} }},
    {{ "name": "ortho", "camera": 1 }}
  ],
  "scenes": [{{ "nodes": [0, 1, 2, 3] }}],
  "scene": 0
}}"#,
        len = bin.len(),
        bin = b64(&bin),
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

#[wasm_bindgen_test]
async fn gltf_cameras_and_lights_become_entities() {
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
    scene.set_dither(false);
    let root = scene.add_model(load_model(&ctx, &asset_url()).await.expect("load"));

    let cameras: Vec<Entity> = scene
        .cameras(root)
        .into_iter()
        .map(Entity::from_raw)
        .collect();
    let names: Vec<_> = cameras.iter().map(|&e| scene.name(e)).collect();
    assert_eq!(names, [Some("eye".into()), Some("ortho".into())]);
    let lights: Vec<Entity> = scene
        .lights(root)
        .into_iter()
        .map(Entity::from_raw)
        .collect();
    let names: Vec<_> = lights.iter().map(|&e| scene.name(e)).collect();
    assert_eq!(names, [Some("bulb".into()), Some("spot".into())]);

    let mut center = || {
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
    wasm_bindgen_futures::JsFuture::from(scene.when_ready())
        .await
        .unwrap();
    // Through the file's camera (infinite far plane): the quad, lit by the
    // file's point light…
    assert!(scene.render_frame(cameras[0]) > 0);
    let lit = center();
    // …and darker with that light off (only the dim default ambient left).
    scene.set_visible(lights[0], false);
    scene.render_frame(cameras[0]);
    let unlit = center();
    assert!(lit[0] > unlit[0] + 60, "lit {lit:?} vs unlit {unlit:?}");
    assert!(lit[0] == lit[1] && lit[1] == lit[2], "white light: {lit:?}");
    scene.destroy();
}
