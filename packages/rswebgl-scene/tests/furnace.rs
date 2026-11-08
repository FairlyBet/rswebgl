// The white furnace: inside a uniform white environment of radiance 1, a
// white surface that conserves energy and doesn't absorb shows exactly 1 —
// whatever its roughness, metalness, coat or view angle. Lost energy (single
// scattering without compensation, layers that don't pass on what they
// don't reflect) shows as darkening, gained energy as > 1. Checked for both
// shading models. Run with:
// wasm-pack test --headless --chrome packages/rswebgl-scene
#![cfg(target_arch = "wasm32")]

use base64::Engine;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

use rswebgl::context::{Context, ContextOptions};
use rswebgl_gltf::load_model;
use rswebgl_scene::{Camera, Scene};

wasm_bindgen_test_configure!(run_in_browser);

// One white material per row.
const MATERIALS: [(&str, &str); 6] = [
    (
        "glossy dielectric",
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 0, "roughnessFactor": 0.3 } }"#,
    ),
    (
        "rough dielectric",
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 0, "roughnessFactor": 1 } }"#,
    ),
    (
        "glossy metal",
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 1, "roughnessFactor": 0.3 } }"#,
    ),
    (
        "rough metal",
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 1, "roughnessFactor": 1 } }"#,
    ),
    (
        "coated",
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 0, "roughnessFactor": 0.5 },
          "extensions": { "KHR_materials_clearcoat": { "clearcoatFactor": 1, "clearcoatRoughnessFactor": 0.2 } } }"#,
    ),
    (
        "rough diffuse (EON)",
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 0, "roughnessFactor": 0.6 } }"#,
    ),
];
// Turns about +Y per column: n·v = 1, 0.5, 0.2.
const ANGLES: [f32; 3] = [0.0, std::f32::consts::FRAC_PI_3, 1.3694384];

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
    let rows = MATERIALS.len();
    let materials: Vec<&str> = MATERIALS.iter().map(|(_, m)| *m).collect();
    let meshes: Vec<String> = (0..rows)
        .map(|i| {
            format!(
                r#"{{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2, "material": {i} }}] }}"#
            )
        })
        .collect();
    let mut nodes = Vec::new();
    for row in 0..rows {
        for (col, angle) in ANGLES.iter().enumerate() {
            let (s, c) = (angle * 0.5).sin_cos();
            nodes.push(format!(
                r#"{{ "mesh": {row}, "translation": [{x}, {y}, 0], "rotation": [0, {s}, 0, {c}] }}"#,
                x = col as f32 - 1.0,
                y = (rows as f32 - 1.0) / 2.0 - row as f32,
            ));
        }
    }
    let node_list: Vec<String> = (0..nodes.len()).map(|i| i.to_string()).collect();
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "extensionsUsed": ["KHR_materials_clearcoat"],
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
  "materials": [{materials}],
  "meshes": [{meshes}],
  "nodes": [{nodes}],
  "scenes": [{{ "nodes": [{node_list}] }}],
  "scene": 0
}}"#,
        len = bin.len(),
        bin = b64(&bin),
        materials = materials.join(","),
        meshes = meshes.join(","),
        nodes = nodes.join(","),
        node_list = node_list.join(","),
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

#[wasm_bindgen_test]
async fn white_furnace() {
    let rows = MATERIALS.len() as i32;
    let (w, h) = (96, 32 * rows);
    let canvas: HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    canvas.set_width(w as u32);
    canvas.set_height(h as u32);
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
    // Radiance 1 from everywhere, shown as is.
    scene.set_environment_intensity(1.0, 1.0, 1.0);
    scene.set_tone_mapping("none").unwrap();
    // Half exposure: the expected 0.5 leaves room to see overshoot too.
    scene.set_exposure(0.5);
    scene.set_dither(false);
    let camera = scene.spawn();
    scene.insert_camera(
        camera,
        Camera::orthographic(rows as f32 / 2.0, 0.5, 0.1, 100.0),
    );
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    let root = scene.add_model(load_model(&ctx, &asset_url()).await.expect("load"));
    // The last row: EON at full diffuse roughness.
    let eon = scene.materials(root)[MATERIALS.len() - 1];
    scene
        .set_material_property(
            rswebgl::ids::ParamGroupId::from_raw(eon),
            "diffuseRoughness",
            wasm_bindgen::JsValue::from(1.0),
        )
        .unwrap();

    let mut report = Vec::new();
    for model in ["openpbr", "gltf"] {
        scene.set_shading_model(model).unwrap();
        // Two frames: the first finishes any variant switch.
        for _ in 0..2 {
            wasm_bindgen_futures::JsFuture::from(scene.when_ready())
                .await
                .unwrap();
            scene.render_frame(camera);
        }
        for (row, (name, _)) in MATERIALS.iter().enumerate() {
            if model == "gltf" && row == MATERIALS.len() - 1 {
                continue; // glTF has no rough diffuse
            }
            for col in 0..ANGLES.len() {
                let mut px = [0u8; 4];
                gl.read_pixels_with_opt_u8_array(
                    16 + 32 * col as i32,
                    h - 16 - 32 * row as i32,
                    1,
                    1,
                    Gl::RGBA,
                    Gl::UNSIGNED_BYTE,
                    Some(&mut px[..]),
                )
                .unwrap();
                // Linear 0.5 ± 4% at half exposure (0.5 is sRGB 188).
                let lin = |c: u8| {
                    let c = c as f32 / 255.0;
                    if c <= 0.04045 {
                        c / 12.92
                    } else {
                        ((c + 0.055) / 1.055).powf(2.4)
                    }
                };
                let worst = px[..3]
                    .iter()
                    .map(|&c| (lin(c) - 0.5).abs())
                    .fold(0.0, f32::max);
                if worst > 0.02 {
                    let white = lin(px[0]) * 2.0;
                    report.push(format!("{model} {name} n·v #{col}: {px:?} = {white:.3}"));
                }
            }
        }
    }
    assert!(
        report.is_empty(),
        "not white (1 expected):\n{}",
        report.join("\n")
    );
    scene.destroy();
}

// A mirror-smooth surface lit along the view (n·h = 1, where GGX peaks)
// gets a finite, bright highlight in both models — not 0/0 or Inf in fp32,
// which shows as black.
#[wasm_bindgen_test]
async fn a_mirror_lit_head_on_is_bright_not_nan() {
    let rows = MATERIALS.len() as i32;
    let (w, h) = (96, 32 * rows);
    let canvas: HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    canvas.set_width(w as u32);
    canvas.set_height(h as u32);
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
    let camera = scene.spawn();
    scene.insert_camera(
        camera,
        Camera::orthographic(rows as f32 / 2.0, 0.5, 0.1, 100.0),
    );
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    let root = scene.add_model(load_model(&ctx, &asset_url()).await.expect("load"));
    // Rows 0 (dielectric) and 2 (metal) as mirrors.
    let materials = scene.materials(root);
    for row in [0, 2] {
        scene
            .set_material_property(
                rswebgl::ids::ParamGroupId::from_raw(materials[row]),
                "roughnessFactor",
                wasm_bindgen::JsValue::from(0.0),
            )
            .unwrap();
    }
    // For each checked pixel P (column 0, rows 0 and 2) a point light at the
    // mirror of the eye (0, 0, 5) about the surface: L = (2P.x, 2P.y, 5), so
    // h = n exactly there.
    for y in [2.5f32, 0.5] {
        let lamp = scene.spawn();
        scene.insert_light(lamp, rswebgl_scene::Light::point(glam::Vec3::ONE, 300.0, 0.0));
        scene.set_translation(lamp, -2.0, 2.0 * y, 5.0);
    }
    for model in ["openpbr", "gltf"] {
        scene.set_shading_model(model).unwrap();
        for _ in 0..2 {
            wasm_bindgen_futures::JsFuture::from(scene.when_ready())
                .await
                .unwrap();
            scene.render_frame(camera);
        }
        for row in [0, 2] {
            let mut px = [0u8; 4];
            gl.read_pixels_with_opt_u8_array(
                16,
                h - 16 - 32 * row,
                1,
                1,
                Gl::RGBA,
                Gl::UNSIGNED_BYTE,
                Some(&mut px[..]),
            )
            .unwrap();
            assert!(px[0] > 200, "{model} row {row}: {px:?}");
        }
    }
    scene.destroy();
}
