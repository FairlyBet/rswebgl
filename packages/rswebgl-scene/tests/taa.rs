// Temporal antialiasing: a slanted edge of a white quad on black turns into
// in-between values over a few jittered frames (none without antialiasing),
// and moving the camera leaves no ghost where the quad was. Run with:
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

// One white unlit quad, turned 30° about z.
fn asset_url() -> String {
    quad_url(0.258_819, 0.965_925_8, false)
}

// A unit unlit quad turned by the z rotation (sin, cos of half the angle);
// `gradient`: black on the left to white on the right (vertex colors).
fn quad_url(qz: f32, qw: f32, gradient: bool) -> String {
    let mut bin = Vec::new();
    #[rustfmt::skip]
    let floats = [-0.5f32, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, 0.5, 0.0];
    for v in floats {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u16, 1, 2, 0, 2, 3] {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    #[rustfmt::skip]
    let colors = [0.0f32, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0];
    for v in colors {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let color = if gradient { r#", "COLOR_0": 2"# } else { "" };
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "extensionsUsed": ["KHR_materials_unlit"],
  "buffers": [{{ "byteLength": {len}, "uri": "data:application/octet-stream;base64,{bin}" }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 12, "target": 34963 }},
    {{ "buffer": 0, "byteOffset": 60, "byteLength": 48 }}
  ],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
       "min": [-0.5, -0.5, 0], "max": [0.5, 0.5, 0] }},
    {{ "bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR" }},
    {{ "bufferView": 2, "componentType": 5126, "count": 4, "type": "VEC3" }}
  ],
  "materials": [{{ "pbrMetallicRoughness": {{ "baseColorFactor": [1, 1, 1, 1] }},
                   "extensions": {{ "KHR_materials_unlit": {{}} }} }}],
  "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0{color} }}, "indices": 1, "material": 0 }}] }}],
  "nodes": [{{ "mesh": 0, "rotation": [0, 0, {qz}, {qw}] }}],
  "scenes": [{{ "nodes": [0] }}],
  "scene": 0
}}"#,
        len = bin.len(),
        bin = b64(&bin),
        color = color,
        qz = qz,
        qw = qw,
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

struct Setup {
    gl: Gl,
    scene: Scene,
    camera: Entity,
}

impl Setup {
    // The red channel of every pixel after `frames` frames.
    async fn frames(&mut self, frames: usize) -> Vec<u8> {
        wasm_bindgen_futures::JsFuture::from(self.scene.when_ready())
            .await
            .unwrap();
        for _ in 0..frames {
            self.scene.render_frame(self.camera);
        }
        let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
        self.gl
            .read_pixels_with_opt_u8_array(
                0,
                0,
                SIZE,
                SIZE,
                Gl::RGBA,
                Gl::UNSIGNED_BYTE,
                Some(&mut px),
            )
            .unwrap();
        px.chunks(4).map(|p| p[0]).collect()
    }
}

#[wasm_bindgen_test]
async fn taa_smooths_edges_and_follows_the_camera() {
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
        return; // TAA needs the HDR target
    }
    scene.set_tone_mapping("none").unwrap();
    scene.set_dither(false);
    scene.set_samples(0);
    let camera = scene.spawn();
    let cam =
        js_sys::JSON::parse(r#"{ "type": "orthographic", "ymag": 1, "znear": 0.1, "zfar": 100 }"#)
            .unwrap();
    scene.set_camera(camera, cam).unwrap();
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    scene.add_model(load_model(&ctx, &asset_url()).await.expect("load"));
    let mut s = Setup { gl, scene, camera };
    let between = |img: &[u8]| img.iter().filter(|&&v| v > 40 && v < 215).count();

    // No antialiasing: every pixel all in or all out.
    let aliased = s.frames(1).await;
    assert_eq!(between(&aliased), 0);

    assert!(s.scene.set_taa(true));
    let smooth = s.frames(16).await;
    assert!(
        between(&smooth) > 8,
        "edge pixels in between: {}",
        between(&smooth)
    );
    // The interior stays white, the outside black.
    assert!(smooth[(SIZE / 2 * SIZE + SIZE / 2) as usize] > 240);
    assert!(smooth[0] < 15);

    // The camera moves: no trace of the quad where it was.
    s.scene
        .look_at(s.camera, &[1.2, 0.0, 5.0], &[1.2, 0.0, 0.0]);
    let moved = s.frames(1).await;
    let right_edge = (SIZE / 2 * SIZE + SIZE - 3) as usize;
    assert!(moved[right_edge] < 40, "ghost: {}", moved[right_edge]);

    assert!(!s.scene.set_taa(false));
    s.scene
        .look_at(s.camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    assert_eq!(between(&s.frames(1).await), 0, "off again");
    s.scene.destroy();
}

// A mesh that moves gets motion vectors (one motion draw a frame while it
// moves, none at rest), and its moving edges stay smooth: the history
// follows it rather than staying where it was.
#[wasm_bindgen_test]
async fn taa_follows_moving_meshes() {
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
        return;
    }
    scene.set_tone_mapping("none").unwrap();
    scene.set_dither(false);
    let camera = scene.spawn();
    let cam =
        js_sys::JSON::parse(r#"{ "type": "orthographic", "ymag": 1, "znear": 0.1, "zfar": 100 }"#)
            .unwrap();
    scene.set_camera(camera, cam).unwrap();
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    let root = scene.add_model(load_model(&ctx, &asset_url()).await.expect("load"));
    assert!(scene.set_taa(true));
    let mut s = Setup { gl, scene, camera };
    let between = |img: &[u8]| img.iter().filter(|&&v| v > 40 && v < 215).count();

    s.frames(16).await;
    assert_eq!(s.scene.moving_meshes(), 0, "at rest");

    // Sliding right a pixel a frame (32 px over 2 units).
    let mut img = Vec::new();
    for i in 1..=12 {
        s.scene.set_translation(root, i as f32 / 16.0, 0.0, 0.0);
        img = s.frames(1).await;
        assert_eq!(s.scene.moving_meshes(), 1, "frame {i}");
    }
    assert!(
        between(&img) > 8,
        "moving edge pixels in between: {}",
        between(&img)
    );
    // Where the quad's left part was: background again, no trail.
    let left = (SIZE / 2 * SIZE + 9) as usize;
    assert!(img[left] < 40, "trail: {}", img[left]);

    s.frames(1).await;
    assert_eq!(s.scene.moving_meshes(), 0, "stopped");
    s.scene.destroy();
}

// Detail on a moving mesh lands where the mesh is: a gradient quad sliding a
// pixel a frame matches the same frame rendered without TAA. Without its
// motion vectors the history would lag behind within the neighborhood's
// range (the clip can't tell).
#[wasm_bindgen_test]
async fn taa_moving_detail_follows_motion_vectors() {
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
        return;
    }
    scene.set_tone_mapping("none").unwrap();
    scene.set_dither(false);
    scene.set_samples(0);
    let camera = scene.spawn();
    let cam =
        js_sys::JSON::parse(r#"{ "type": "orthographic", "ymag": 1, "znear": 0.1, "zfar": 100 }"#)
            .unwrap();
    scene.set_camera(camera, cam).unwrap();
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    let root = scene.add_model(
        load_model(&ctx, &quad_url(0.0, 1.0, true))
            .await
            .expect("load"),
    );
    scene.set_translation(root, -0.5, 0.0, 0.0);
    let mut s = Setup { gl, scene, camera };
    // Where it ends, without TAA.
    s.scene.set_translation(root, 0.0, 0.0, 0.0);
    let reference = s.frames(1).await;
    s.scene.set_translation(root, -0.5, 0.0, 0.0);

    assert!(s.scene.set_taa(true));
    s.frames(16).await;
    let mut img = Vec::new();
    for i in 1..=8 {
        s.scene
            .set_translation(root, -0.5 + i as f32 / 16.0, 0.0, 0.0);
        img = s.frames(1).await;
    }
    // The quad's inner pixels on the middle row (its edges antialias).
    let row = (SIZE / 2 * SIZE) as usize;
    let error: i32 = (10..24)
        .map(|x| (img[row + x] as i32 - reference[row + x] as i32).abs())
        .max()
        .unwrap();
    assert!(
        error <= 4,
        "off by {error}: {:?} vs {:?}",
        &img[row + 8..row + 24],
        &reference[row + 8..row + 24]
    );
    s.scene.destroy();
}
