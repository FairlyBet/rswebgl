// Image-based lighting against a real context: the equirect → cube bake keeps
// directions (seen through the background), rotation turns it, and a baked
// uniform environment lights like the built-in constant one (irradiance and
// prefilter normalization). Run with:
// wasm-pack test --headless --chrome packages/rswebgl-scene
#![cfg(target_arch = "wasm32")]

use base64::Engine;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

use rswebgl::context::{Context, ContextOptions};
use rswebgl::framebuffer::ClearMask;
use rswebgl::pass::Pass;
use rswebgl_ecs::Entity;
use rswebgl_gltf::load_model;
use rswebgl_scene::{Camera, Scene, encode_hdr};

wasm_bindgen_test_configure!(run_in_browser);

const SIZE: i32 = 64;

fn context() -> (Context, Gl) {
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
    let gl = canvas
        .get_context("webgl2")
        .unwrap()
        .unwrap()
        .dyn_into()
        .unwrap();
    (ctx, gl)
}

fn center(gl: &Gl) -> [u8; 4] {
    let mut px = [0u8; 4];
    gl.read_pixels_with_opt_u8_array(
        SIZE / 2,
        SIZE / 2,
        1,
        1,
        Gl::RGBA,
        Gl::UNSIGNED_BYTE,
        Some(&mut px[..]),
    )
    .unwrap();
    assert_eq!(gl.get_error(), Gl::NO_ERROR);
    px
}

// An equirect map that says where it is: red where the direction points up
// (+Y), green where it points to +X.
fn direction_map() -> Vec<u8> {
    let (w, h) = (64u32, 32u32);
    let mut rgb = Vec::new();
    for y in 0..h {
        let theta = (y as f32 + 0.5) / h as f32 * std::f32::consts::PI; // from +Y
        for x in 0..w {
            // u = atan(z, x) / 2π + 0.5 (the bake's mapping).
            let phi = ((x as f32 + 0.5) / w as f32 - 0.5) * 2.0 * std::f32::consts::PI;
            let dx = theta.sin() * phi.cos();
            let up = theta.cos() > 0.0;
            rgb.extend([
                if up { 1.0 } else { 0.0 },
                if dx > 0.0 { 1.0 } else { 0.0 },
                0.0,
            ]);
        }
    }
    encode_hdr(w, h, &rgb)
}

// The background seen by a camera looking from the origin toward (x, y, z).
fn look(scene: &mut Scene, ctx: &Context, gl: &Gl, camera: Entity, dir: [f32; 3]) -> [u8; 4] {
    // lookAt can't take an up-parallel direction; nudge it.
    scene.look_at(camera, &[0.0, 0.0, 0.0], &[dir[0], dir[1], dir[2] + 1e-3]);
    let mut pass = Pass::to_default();
    pass.set_clear(&ClearMask::color_depth());
    scene.render(camera, &mut pass);
    pass.sort();
    ctx.render(&pass);
    center(gl)
}

#[wasm_bindgen_test]
async fn baked_directions_match_the_equirect_map() {
    let (ctx, gl) = context();
    let mut scene = Scene::new(&ctx).expect("scene");
    let camera = scene.spawn();
    scene.insert_camera(camera, Camera::perspective(0.5, 1.0, 0.1, 10.0));
    scene
        .set_environment_hdr(&direction_map(), 64)
        .expect("bake");
    assert!(scene.has_environment());
    scene.set_background(0.0);
    ready(&scene).await;

    let red = |p: [u8; 4]| p[0] > 128;
    let green = |p: [u8; 4]| p[1] > 128;
    let up = look(&mut scene, &ctx, &gl, camera, [0.3, 1.0, 0.0]);
    assert!(red(up) && green(up), "up and toward +X: {up:?}");
    let down = look(&mut scene, &ctx, &gl, camera, [-0.3, -1.0, 0.0]);
    assert!(!red(down) && !green(down), "down and toward −X: {down:?}");
    for (dir, want_green) in [
        ([1.0, -0.3, 0.2], true),
        ([-1.0, -0.3, 0.2], false),
        ([1.0, -0.3, -0.2], true),
    ] {
        let px = look(&mut scene, &ctx, &gl, camera, dir);
        assert_eq!(green(px), want_green, "{dir:?}: {px:?}");
        assert!(!red(px), "below the horizon: {px:?}");
    }

    // Turning the environment half a turn swaps +X and −X.
    scene.set_environment_rotation(std::f32::consts::PI);
    assert!(!green(look(
        &mut scene,
        &ctx,
        &gl,
        camera,
        [1.0, -0.3, 0.2]
    )));
    assert!(green(look(
        &mut scene,
        &ctx,
        &gl,
        camera,
        [-1.0, -0.3, 0.2]
    )));

    // A file that isn't HDR is an error, and the environment stays.
    assert!(scene.set_environment_hdr(b"not an hdr", 64).is_err());
    assert!(scene.has_environment());
    scene.clear_environment();
    assert!(!scene.has_environment());
    scene.destroy();
}

// Three quads facing +Z: "rough" (white dielectric, roughness 1) at x = −1,
// "metal" (white metal, roughness 0.6: all specular, prefiltered) at 0 and
// "mirror" (white metal, roughness 0) at x = +1.
fn quads_url() -> String {
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
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
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
    {{ "pbrMetallicRoughness": {{ "metallicFactor": 0, "roughnessFactor": 1 }} }},
    {{ "pbrMetallicRoughness": {{ "metallicFactor": 1, "roughnessFactor": 0 }} }},
    {{ "pbrMetallicRoughness": {{ "metallicFactor": 1, "roughnessFactor": 0.6 }} }}
  ],
  "meshes": [
    {{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2, "material": 0 }}] }},
    {{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2, "material": 1 }}] }},
    {{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2, "material": 2 }}] }}
  ],
  "nodes": [
    {{ "name": "rough", "mesh": 0, "translation": [-1, 0, 0] }},
    {{ "name": "mirror", "mesh": 1, "translation": [1, 0, 0] }},
    {{ "name": "metal", "mesh": 2, "translation": [0, 0, 0] }}
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
async fn a_baked_uniform_environment_lights_like_the_constant_one() {
    let (ctx, gl) = context();
    let mut scene = Scene::new(&ctx).expect("scene");
    let camera = scene.spawn();
    scene.insert_camera(camera, Camera::orthographic(2.0, 1.0, 0.1, 100.0));
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    scene.add_model(load_model(&ctx, &quads_url()).await.expect("load"));
    ready(&scene).await;

    let frame = |scene: &mut Scene| -> [[u8; 4]; 3] {
        let mut pass = Pass::to_default();
        pass.set_clear(&ClearMask::color_depth());
        scene.render(camera, &mut pass);
        pass.sort();
        ctx.render(&pass);
        let mut out = [[0u8; 4]; 3];
        for (px, x) in out.iter_mut().zip([SIZE / 4, SIZE / 2, SIZE * 3 / 4]) {
            gl.read_pixels_with_opt_u8_array(
                x,
                SIZE / 2,
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

    // The built-in constant white environment at intensity 0.5…
    scene.set_environment_intensity(0.5, 0.5, 0.5);
    let constant = frame(&mut scene);
    // …and a baked environment that is 0.5 everywhere.
    let flat = encode_hdr(16, 8, &[0.5; 16 * 8 * 3]);
    scene.set_environment_hdr(&flat, 64).expect("bake");
    let baked = frame(&mut scene);
    for (c, b) in constant.iter().zip(&baked) {
        for i in 0..3 {
            assert!(
                (c[i] as i32 - b[i] as i32).abs() <= 3,
                "constant {constant:?} vs baked {baked:?}"
            );
        }
    }
    // All lit: the rough one by the diffuse term, the metals by the
    // prefiltered specular.
    assert!(baked.iter().all(|p| p[0] > 60), "{baked:?}");

    // A sun brighter than half float's max (65504; real HDRIs reach 2e5)
    // must not turn into infinities that the prefilter spreads as NaN (black
    // here): the rough and the metal, which gather the whole sky, stay lit,
    // and nothing tints. (The mirror sees only the dim sky: dark either way.)
    let mut sunny = [0.5; 16 * 8 * 3];
    sunny[(2 * 16 + 5) * 3..][..3].copy_from_slice(&[2.0e5; 3]);
    scene
        .set_environment_hdr(&encode_hdr(16, 8, &sunny), 64)
        .expect("bake");
    scene.set_environment_intensity(0.01, 0.01, 0.01);
    let sun = frame(&mut scene);
    assert!(sun[0][0] > 60 && sun[1][0] > 30, "{sun:?}");
    for p in &sun {
        assert!(
            p[0].abs_diff(p[1]) <= 3 && p[0].abs_diff(p[2]) <= 3,
            "{sun:?}"
        );
    }
    scene.destroy();
}

// Shaders link in the background; wait for them before reading pixels.
async fn ready(scene: &Scene) {
    wasm_bindgen_futures::JsFuture::from(scene.when_ready())
        .await
        .unwrap();
}
