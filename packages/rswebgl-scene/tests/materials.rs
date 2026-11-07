// glTF material extensions through the built-in PBR: IOR, transmission,
// volume absorption, clearcoat. The environment is red toward the camera
// (+Z: what a surface facing it reflects) and blue behind the quads (−Z: what
// light passing straight through them shows). Run with:
// wasm-pack test --headless --chrome packages/rswebgl-scene
#![cfg(target_arch = "wasm32")]

use base64::Engine;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

use rswebgl::context::{Context, ContextOptions};
use rswebgl::framebuffer::ClearMask;
use rswebgl::pass::Pass;
use rswebgl_gltf::load_model;
use rswebgl_scene::{Camera, Scene, encode_hdr};

wasm_bindgen_test_configure!(run_in_browser);

// Quads in a row, one material each (x = −3 … 3), facing the camera.
const MATERIALS: [&str; 7] = [
    // 0: white, rough: diffuse light from the red side.
    r#"{ "pbrMetallicRoughness": { "metallicFactor": 0, "roughnessFactor": 1 } }"#,
    // 1: clear thin glass: the blue behind shows through.
    r#"{ "pbrMetallicRoughness": { "metallicFactor": 0, "roughnessFactor": 0 },
         "extensions": { "KHR_materials_transmission": { "transmissionFactor": 1 } } }"#,
    // 2: black glossy dielectric, IOR 1.5: a faint red reflection (F0 0.04).
    r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 0 } }"#,
    // 3: the same at IOR 2.4 (diamond): F0 0.17, a stronger reflection.
    r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 0 },
         "extensions": { "KHR_materials_ior": { "ior": 2.4 } } }"#,
    // 4: black rough dielectric under a glossy clearcoat.
    r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 1 },
         "extensions": { "KHR_materials_clearcoat": { "clearcoatFactor": 1, "clearcoatRoughnessFactor": 0 } } }"#,
    // 5: the same without the coat.
    r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 1 } }"#,
    // 6: glass with a volume that absorbs everything but red.
    r#"{ "pbrMetallicRoughness": { "metallicFactor": 0, "roughnessFactor": 0 },
         "extensions": { "KHR_materials_transmission": { "transmissionFactor": 1 },
                         "KHR_materials_volume": { "thicknessFactor": 1, "attenuationDistance": 0.1,
                                                   "attenuationColor": [1, 0, 0] } } }"#,
];

fn asset_url() -> String {
    let mut bin = Vec::new();
    #[rustfmt::skip]
    let floats = [
        -0.5f32, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, 0.5, 0.0,
        0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
    ];
    for v in floats {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u16, 1, 2, 0, 2, 3] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let meshes: Vec<String> = (0..MATERIALS.len())
        .map(|m| {
            format!(
                r#"{{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2, "material": {m} }}] }}"#
            )
        })
        .collect();
    let nodes: Vec<String> = (0..MATERIALS.len())
        .map(|i| {
            format!(
                r#"{{ "mesh": {i}, "translation": [{}, 0, 0] }}"#,
                i as i32 - 3
            )
        })
        .collect();
    let scene_nodes: Vec<String> = (0..MATERIALS.len()).map(|i| i.to_string()).collect();
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "extensionsUsed": ["KHR_materials_transmission", "KHR_materials_ior",
                     "KHR_materials_clearcoat", "KHR_materials_volume"],
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
  "scenes": [{{ "nodes": [{scene_nodes}] }}],
  "scene": 0
}}"#,
        len = bin.len(),
        bin = b64(&bin),
        materials = MATERIALS.join(","),
        meshes = meshes.join(","),
        nodes = nodes.join(","),
        scene_nodes = scene_nodes.join(","),
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

// Red where the direction points toward +Z, blue toward −Z (the bake's
// equirect mapping: u = atan(z, x) / 2π + 0.5).
fn red_front_blue_back() -> Vec<u8> {
    let (w, h) = (32u32, 16u32);
    let mut rgb = Vec::new();
    for _ in 0..h {
        for x in 0..w {
            let phi = ((x as f32 + 0.5) / w as f32 - 0.5) * 2.0 * std::f32::consts::PI;
            let toward_z = phi.sin() > 0.0;
            rgb.extend(if toward_z {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 0.0, 1.0]
            });
        }
    }
    encode_hdr(w, h, &rgb)
}

#[wasm_bindgen_test]
async fn extensions_shade_as_specified() {
    let n = MATERIALS.len() as i32;
    let (w, h) = (32 * n, 32);
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
    let camera = scene.spawn();
    scene.insert_camera(camera, Camera::orthographic(0.5, 1.0, 0.1, 100.0));
    scene.set_camera_aspect(camera, n as f32);
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    scene.add_model(load_model(&ctx, &asset_url()).await.expect("load"));
    scene
        .set_environment_hdr(&red_front_blue_back(), 64)
        .expect("bake");

    let mut pass = Pass::to_default();
    pass.set_clear(&ClearMask::color_depth());
    scene.render(camera, &mut pass);
    pass.sort();
    ctx.render(&pass);
    let mut px = vec![[0u8; 4]; n as usize];
    for (i, p) in px.iter_mut().enumerate() {
        gl.read_pixels_with_opt_u8_array(
            16 + 32 * i as i32,
            h / 2,
            1,
            1,
            Gl::RGBA,
            Gl::UNSIGNED_BYTE,
            Some(&mut p[..]),
        )
        .unwrap();
    }
    assert_eq!(gl.get_error(), Gl::NO_ERROR);
    let [rough, glass, ior15, ior24, coat, no_coat, absorbing] =
        <[[u8; 4]; 7]>::try_from(px).unwrap();

    assert!(
        rough[0] > rough[2] + 60,
        "diffuse sees the red side: {rough:?}"
    );
    assert!(
        glass[2] > glass[0] + 100,
        "glass shows the blue behind: {glass:?}"
    );
    assert!(
        ior24[0] > ior15[0] + 30,
        "higher IOR reflects more: {ior15:?} → {ior24:?}"
    );
    assert!(
        coat[0] > no_coat[0] + 20,
        "the coat reflects: {no_coat:?} → {coat:?}"
    );
    assert!(
        absorbing[2] + 100 < glass[2],
        "the volume absorbs blue: {glass:?} → {absorbing:?}"
    );
    // Four variants: plain, transmission, transmission + volume, clearcoat
    // (the IOR is a uniform, not a variant).
    assert_eq!(scene.shader_variant_count(), 4);
    scene.destroy();
}
