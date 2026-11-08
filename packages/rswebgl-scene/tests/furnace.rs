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
const MATERIALS: [(&str, &str); 10] = [
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
    (
        "fuzz (sheen) over white",
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 0, "roughnessFactor": 0.5 },
          "extensions": { "KHR_materials_sheen": { "sheenColorFactor": [1, 1, 1], "sheenRoughnessFactor": 0.5 } } }"#,
    ),
    (
        "anisotropic metal",
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 1, "roughnessFactor": 0.4 },
          "extensions": { "KHR_materials_anisotropy": { "anisotropyStrength": 0.8 } } }"#,
    ),
    (
        "thin film over white",
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 0, "roughnessFactor": 0.3 },
          "extensions": { "KHR_materials_iridescence": { "iridescenceFactor": 1, "iridescenceThicknessMaximum": 300 } } }"#,
    ),
    (
        "thin film over metal",
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 1, "roughnessFactor": 0.3 },
          "extensions": { "KHR_materials_iridescence": { "iridescenceFactor": 1, "iridescenceIor": 1.0001, "iridescenceThicknessMaximum": 300 } } }"#,
    ),
];
// Turns about +Y per column: n·v = 1, 0.5, 0.2.
const ANGLES: [f32; 3] = [0.0, std::f32::consts::FRAC_PI_3, 1.3694384];

// A grid of unit quads: one row per material, one column per `ANGLES` turn.
fn grid_url(materials: &[&str]) -> String {
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
    let rows = materials.len();

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
  "extensionsUsed": ["KHR_materials_clearcoat", "KHR_materials_sheen", "KHR_materials_anisotropy", "KHR_materials_iridescence", "KHR_materials_transmission", "KHR_materials_unlit"],
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
  "images": [{{ "uri": "{black}" }}],
  "textures": [{{ "source": 0 }}],
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
        black = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png([0, 0, 0, 0]))
        ),
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
    // Row 5: EON at full diffuse roughness.
    let eon = scene.materials(root)[5];
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
            if model == "gltf" && (row == 5 || row >= 8) {
                // glTF has no rough diffuse; its thin film (Khronos rgb_mix)
                // isn't energy-conserving by construction.
                continue;
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
                // The film's image-based albedo is MaterialX's blend of the mirror
                // and rough values: a few percent off at grazing angles.
                let tolerance = if name.contains("film") { 0.03 } else { 0.02 };
                if worst > tolerance {
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
    for row in [0, 2] {
        let y = (rows as f32 - 1.0) / 2.0 - row as f32;
        let lamp = scene.spawn();
        scene.insert_light(
            lamp,
            rswebgl_scene::Light::point(glam::Vec3::ONE, 300.0, 0.0),
        );
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

fn asset_url() -> String {
    let materials: Vec<&str> = MATERIALS.iter().map(|(_, m)| *m).collect();
    grid_url(&materials)
}

// Layers show as what they are, in both models: sheen brightens a dark
// surface at grazing angles, a thin film colors a reflection, anisotropy
// reshapes a highlight.
#[wasm_bindgen_test]
async fn layers_show() {
    const LAYERS: [&str; 6] = [
        r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 0.6 } }"#,
        r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 0.6 },
             "extensions": { "KHR_materials_sheen": { "sheenColorFactor": [1, 1, 1], "sheenRoughnessFactor": 0.5 } } }"#,
        r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 0.1 } }"#,
        r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 0.1 },
             "extensions": { "KHR_materials_iridescence": { "iridescenceFactor": 1, "iridescenceIor": 1.8,
                                                            "iridescenceThicknessMaximum": 350 } } }"#,
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 1, "roughnessFactor": 0.3 } }"#,
        r#"{ "pbrMetallicRoughness": { "metallicFactor": 1, "roughnessFactor": 0.3 },
             "extensions": { "KHR_materials_anisotropy": { "anisotropyStrength": 0.9 } } }"#,
    ];
    let rows = LAYERS.len() as i32;
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
    scene.set_environment_intensity(1.0, 1.0, 1.0);
    scene.set_tone_mapping("none").unwrap();
    scene.set_dither(false);
    let camera = scene.spawn();
    scene.insert_camera(
        camera,
        Camera::orthographic(rows as f32 / 2.0, 0.5, 0.1, 100.0),
    );
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    scene.add_model(load_model(&ctx, &grid_url(&LAYERS)).await.expect("load"));
    // A lamp off to the side for the anisotropic highlight.
    let lamp = scene.spawn();
    scene.insert_light(
        lamp,
        rswebgl_scene::Light::point(glam::Vec3::ONE, 40.0, 0.0),
    );
    scene.set_translation(lamp, 1.0, -0.5, 3.0);

    for model in ["openpbr", "gltf"] {
        scene.set_shading_model(model).unwrap();
        for _ in 0..2 {
            wasm_bindgen_futures::JsFuture::from(scene.when_ready())
                .await
                .unwrap();
            scene.render_frame(camera);
        }
        let px = |row: i32, col: i32| {
            let mut p = [0u8; 4];
            gl.read_pixels_with_opt_u8_array(
                16 + 32 * col,
                h - 16 - 32 * row,
                1,
                1,
                Gl::RGBA,
                Gl::UNSIGNED_BYTE,
                Some(&mut p[..]),
            )
            .unwrap();
            p
        };
        let (plain, sheen) = (px(0, 2), px(1, 2));
        assert!(
            sheen[0] as i32 > plain[0] as i32 + 20,
            "{model}: sheen at grazing {plain:?} → {sheen:?}"
        );
        let (bare, film) = (px(2, 0), px(3, 0));
        let spread = |p: [u8; 4]| p[..3].iter().max().unwrap() - p[..3].iter().min().unwrap();
        assert!(
            spread(bare) <= 2,
            "{model}: a bare reflection is gray: {bare:?}"
        );
        assert!(spread(film) > 15, "{model}: the film colors it: {film:?}");
        // Anisotropy, lit by the lamp alone (the white environment would
        // saturate the metals): the highlight's shape changes what the cells
        // gather, summed over a 5×5 sample of each.
        scene.set_environment_intensity(0.0, 0.0, 0.0);
        scene.render_frame(camera);
        let cell = |row: i32| -> i32 {
            let mut sum = 0;
            for dy in -2..=2 {
                for dx in -2..=2 {
                    let mut p = [0u8; 4];
                    gl.read_pixels_with_opt_u8_array(
                        16 + dx * 5,
                        h - 16 - 32 * row + dy * 5,
                        1,
                        1,
                        Gl::RGBA,
                        Gl::UNSIGNED_BYTE,
                        Some(&mut p[..]),
                    )
                    .unwrap();
                    sum += p[0] as i32;
                }
            }
            sum
        };
        let (iso, aniso) = (cell(4), cell(5));
        assert!(
            iso > 0 && (iso - aniso).abs() * 10 > iso,
            "{model}: anisotropy {iso} → {aniso}"
        );
        scene.set_environment_intensity(1.0, 1.0, 1.0);
    }
    scene.destroy();
}

// Glass in front of a red wall, in a white environment: with HDR output
// the glass sees the wall (the copied opaque scene); drawing straight to
// the canvas it falls back to the environment.
#[wasm_bindgen_test]
async fn glass_sees_the_scene_behind_it() {
    let (w, h) = (96, 32);
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
    scene.set_environment_intensity(1.0, 1.0, 1.0);
    scene.set_tone_mapping("none").unwrap();
    scene.set_dither(false);
    let camera = scene.spawn();
    scene.insert_camera(camera, Camera::orthographic(0.5, 3.0, 0.1, 100.0));
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    let wall = r#"{ "pbrMetallicRoughness": { "baseColorFactor": [1, 0, 0, 1] },
                   "extensions": { "KHR_materials_unlit": {} } }"#;
    let glass = r#"{ "pbrMetallicRoughness": { "metallicFactor": 0, "roughnessFactor": 0 },
                    "extensions": { "KHR_materials_transmission": { "transmissionFactor": 1 } } }"#;
    scene.add_model(load_model(&ctx, &grid_url(&[wall])).await.expect("wall"));
    let front = scene.add_model(load_model(&ctx, &grid_url(&[glass])).await.expect("glass"));
    scene.set_translation(front, 0.0, 0.0, 1.0);

    for model in ["openpbr", "gltf"] {
        scene.set_shading_model(model).unwrap();
        wasm_bindgen_futures::JsFuture::from(scene.when_ready())
            .await
            .unwrap();
        let mut look = |hdr: bool| {
            assert_eq!(scene.set_hdr(hdr), hdr);
            scene.render_frame(camera);
            let mut p = [0u8; 4];
            // Column 0's quad (head-on): glass over the wall.
            gl.read_pixels_with_opt_u8_array(
                16,
                h / 2,
                1,
                1,
                Gl::RGBA,
                Gl::UNSIGNED_BYTE,
                Some(&mut p[..]),
            )
            .unwrap();
            p
        };
        look(true);
        let through = look(true);
        assert!(
            through[0] > 150 && through[1] < 60 && through[2] < 60,
            "{model}: the wall shows through: {through:?}"
        );
        let env = look(false);
        assert!(
            env[1] > 150 && env[2] > 150,
            "{model}: without HDR the environment: {env:?}"
        );
    }
    scene.destroy();
}

// A 1×1 RGBA PNG of one color (stored deflate).
fn png(rgba: [u8; 4]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend(kind);
        out.extend(data);
        let crc = crc32(&out[start..]);
        out.extend(crc.to_be_bytes());
    }
    let raw = [0, rgba[0], rgba[1], rgba[2], rgba[3]];
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    let mut zlib = vec![0x78, 0x01, 1, raw.len() as u8, 0, !(raw.len() as u8), 0xff];
    zlib.extend(raw);
    zlib.extend(((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

// An extension's map multiplies its factor: a black clearcoat, sheen color
// or transmission map turns that layer off, in both models. (The grid's
// texture 0 is a 1×1 black, transparent PNG; the quads have no UVs, so
// every pixel reads it.)
#[wasm_bindgen_test]
async fn extension_maps_scale_their_factors() {
    const ROWS: [&str; 6] = [
        // A rough black base under a glossy coat, then the coat masked off,
        // then no coat at all.
        r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 1 },
             "extensions": { "KHR_materials_clearcoat": { "clearcoatFactor": 1 } } }"#,
        r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 1 },
             "extensions": { "KHR_materials_clearcoat": { "clearcoatFactor": 1, "clearcoatTexture": { "index": 0 } } } }"#,
        r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 1 } }"#,
        // White sheen on black, and the same with its color map black.
        r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 1 },
             "extensions": { "KHR_materials_sheen": { "sheenColorFactor": [1, 1, 1], "sheenRoughnessFactor": 0.5 } } }"#,
        r#"{ "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 1 },
             "extensions": { "KHR_materials_sheen": { "sheenColorFactor": [1, 1, 1], "sheenRoughnessFactor": 0.5,
                                                      "sheenColorTexture": { "index": 0 } } } }"#,
        // Red glass with its transmission map black: an opaque red dielectric.
        r#"{ "pbrMetallicRoughness": { "baseColorFactor": [1, 0, 0, 1], "metallicFactor": 0, "roughnessFactor": 0.5 },
             "extensions": { "KHR_materials_transmission": { "transmissionFactor": 1, "transmissionTexture": { "index": 0 } } } }"#,
    ];
    let rows = ROWS.len() as i32;
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
    scene.set_environment_intensity(1.0, 1.0, 1.0);
    scene.set_tone_mapping("none").unwrap();
    scene.set_dither(false);
    let camera = scene.spawn();
    scene.insert_camera(
        camera,
        Camera::orthographic(rows as f32 / 2.0, 0.5, 0.1, 100.0),
    );
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    scene.add_model(load_model(&ctx, &grid_url(&ROWS)).await.expect("load"));
    // The image decodes asynchronously.
    sleep_ms(100).await;

    for model in ["openpbr", "gltf"] {
        scene.set_shading_model(model).unwrap();
        for _ in 0..2 {
            wasm_bindgen_futures::JsFuture::from(scene.when_ready())
                .await
                .unwrap();
            scene.render_frame(camera);
        }
        let px = |row: i32, col: i32| {
            let mut p = [0u8; 4];
            gl.read_pixels_with_opt_u8_array(
                16 + 32 * col,
                h - 16 - 32 * row,
                1,
                1,
                Gl::RGBA,
                Gl::UNSIGNED_BYTE,
                Some(&mut p[..]),
            )
            .unwrap();
            p
        };
        let (coated, masked, bare) = (px(0, 0), px(1, 0), px(2, 0));
        assert!(
            coated[0] > bare[0] + 10,
            "{model}: the coat shows: {bare:?} → {coated:?}"
        );
        assert!(
            masked[0].abs_diff(bare[0]) <= 2,
            "{model}: masked coat = none: {masked:?} vs {bare:?}"
        );
        let (sheen, no_sheen) = (px(3, 2), px(4, 2));
        assert!(
            sheen[0] > no_sheen[0] + 20,
            "{model}: a black sheen map: {sheen:?} → {no_sheen:?}"
        );
        let opaque = px(5, 0);
        assert!(
            opaque[0] > opaque[2] + 40,
            "{model}: no transmission, red shows: {opaque:?}"
        );
    }
    scene.destroy();
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
