// The built-in PBR shader: every feature compiles and links, lights and
// ambient behave, unlit / emissive / exposure, transparent draws sort back to
// front. Run with:
// wasm-pack test --headless --chrome packages/rswebgl-scene
#![cfg(target_arch = "wasm32")]

use base64::Engine;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

use glam::Vec3;
use rswebgl::context::{Context, ContextOptions};
use rswebgl::framebuffer::ClearMask;
use rswebgl::pass::Pass;
use rswebgl_ecs::Entity;
use rswebgl_gltf::load_model;
use rswebgl_scene::{Camera, Light, PbrKey, PbrShaders, Scene, ShadingModel};

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

#[wasm_bindgen_test]
fn every_feature_compiles_and_links() {
    let (ctx, _) = context();
    let mut shaders = PbrShaders::new();
    // Bits (see pbr.rs): 0..5 attributes, 5..7 alpha, 7 unlit, 8..12
    // transmission/volume/clearcoat/dispersion, 12 glTF model, 13 EON,
    // 14..17 sheen/film/anisotropy, 17..34 maps, 34..51 maps on UV1, 51
    // texture transform, 52 skin, 53..56 morph position/normal/tangent.
    let attrs: u64 = 0x1f;
    let maps: u64 = 0x1ffff << 17;
    let uv1: u64 = 0x1ffff << 34;
    let layers: u64 = 0xf << 8 | 0xf << 13;
    // Each bit alone, every alpha mode, and everything at once.
    let mut keys: Vec<u64> = (0..56).map(|b| 1u64 << b).collect();
    let deform: u64 = 0xf << 52;
    keys.extend([
        0,
        2 << 5,
        attrs | maps,
        attrs | maps | uv1,
        maps,
        attrs | maps | layers,
        attrs | maps | layers | 2 << 5,
        attrs | maps | 1 << 51,
        attrs | maps | uv1 | layers | 1 << 51,
        attrs | deform,
        attrs | maps | layers | deform,
    ]);
    for model in [ShadingModel::OpenPbr, ShadingModel::Gltf] {
        shaders.set_model(model);
        for &bits in &keys {
            let key = PbrKey::from_bits(bits).fit_texture_units();
            let p = shaders.program(&ctx, key).expect("program");
            // Waits for the link; then validity is known.
            ctx.program_active_uniform_names(p);
            assert!(
                ctx.program_is_valid(p),
                "{model:?} variant {bits:#x} doesn't link:\n{}",
                key.with_model(model).defines()
            );
        }
    }
    // A known key comes from the cache.
    let first = shaders.program(&ctx, PbrKey::from_bits(0)).unwrap();
    let count = shaders.len();
    assert_eq!(shaders.program(&ctx, PbrKey::from_bits(0)).unwrap(), first);
    assert_eq!(shaders.len(), count);
    shaders.destroy(&ctx);
}

// Quads facing +Z (with normals), one node each. Materials:
//   0 "white": dielectric, rough        1 "red": unlit
//   2 "glow": black, emissive green     3 "near"/4 "far": blended, unlit, α 0.5
fn asset_url() -> String {
    let mut bin = Vec::new();
    #[rustfmt::skip]
    let floats = [
        -0.5f32, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, 0.5, 0.0, // positions 0..48
        0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,         // normals 48..96
    ];
    for v in floats {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    for i in [0u16, 1, 2, 0, 2, 3] {
        bin.extend_from_slice(&i.to_le_bytes()); // 96..108
    }
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let mesh = |m: u32| {
        format!(
            r#"{{ "primitives": [{{ "attributes": {{ "POSITION": 0, "NORMAL": 1 }}, "indices": 2, "material": {m} }}] }}"#
        )
    };
    let meshes: Vec<String> = (0..5).map(mesh).collect();
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "extensionsUsed": ["KHR_materials_unlit", "KHR_materials_emissive_strength"],
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
    {{ "name": "white", "pbrMetallicRoughness": {{ "metallicFactor": 0, "roughnessFactor": 1 }} }},
    {{ "name": "red", "pbrMetallicRoughness": {{ "baseColorFactor": [1, 0, 0, 1] }},
       "extensions": {{ "KHR_materials_unlit": {{}} }} }},
    {{ "name": "glow", "pbrMetallicRoughness": {{ "baseColorFactor": [0, 0, 0, 1] }},
       "emissiveFactor": [0, 1, 0],
       "extensions": {{ "KHR_materials_emissive_strength": {{ "emissiveStrength": 4 }} }} }},
    {{ "name": "near", "alphaMode": "BLEND", "pbrMetallicRoughness": {{ "baseColorFactor": [1, 0, 0, 0.5] }},
       "extensions": {{ "KHR_materials_unlit": {{}} }} }},
    {{ "name": "far", "alphaMode": "BLEND", "pbrMetallicRoughness": {{ "baseColorFactor": [0, 0, 1, 0.5] }},
       "extensions": {{ "KHR_materials_unlit": {{}} }} }}
  ],
  "meshes": [{meshes}],
  "nodes": [
    {{ "name": "white", "mesh": 0, "translation": [-1, 0, 0] }},
    {{ "name": "red", "mesh": 1, "translation": [0, 0, 0] }},
    {{ "name": "glow", "mesh": 2, "translation": [1, 0, 0] }},
    {{ "name": "near", "mesh": 3, "translation": [0, 0, 1] }},
    {{ "name": "far", "mesh": 4, "translation": [0, 0, -1] }}
  ],
  "scenes": [{{ "nodes": [0, 1, 2, 3, 4] }}],
  "scene": 0
}}"#,
        len = bin.len(),
        bin = b64(&bin),
        meshes = meshes.join(","),
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

struct Setup {
    ctx: Context,
    gl: Gl,
    scene: Scene,
    pass: Pass,
    camera: Entity,
    root: Entity,
}

async fn setup() -> Setup {
    let (ctx, gl) = context();
    let mut scene = Scene::new(&ctx).expect("scene");
    let camera = scene.spawn();
    // 4 units across, looking down −Z from z = 5: x = −1, 0, +1 at 16, 32, 48.
    scene.insert_camera(camera, Camera::orthographic(2.0, 1.0, 0.1, 100.0));
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    let model = load_model(&ctx, &asset_url()).await.expect("load");
    let root = scene.add_model(model);
    wasm_bindgen_futures::JsFuture::from(scene.when_ready())
        .await
        .unwrap();
    let mut pass = Pass::to_default();
    pass.set_clear(&ClearMask::color_depth());
    Setup {
        ctx,
        gl,
        scene,
        pass,
        camera,
        root,
    }
}

impl Setup {
    fn frame(&mut self) {
        self.pass.clear_draws();
        self.scene.render(self.camera, &mut self.pass);
        self.pass.sort();
        self.ctx.render(&self.pass);
    }

    fn pixel(&self, x: i32) -> [u8; 4] {
        let mut px = [0u8; 4];
        self.gl
            .read_pixels_with_opt_u8_array(
                x,
                SIZE / 2,
                1,
                1,
                Gl::RGBA,
                Gl::UNSIGNED_BYTE,
                Some(&mut px[..]),
            )
            .unwrap();
        assert_eq!(self.gl.get_error(), Gl::NO_ERROR);
        px
    }

    fn hide(&mut self, name: &str) {
        let e = self.scene.find_by_name(self.root, name);
        self.scene.set_visible(e, false);
    }
}

#[wasm_bindgen_test]
async fn lights_ambient_unlit_emissive_and_exposure() {
    let mut s = setup().await;
    s.hide("near");
    s.hide("far");
    let (white, red, glow) = (16, 32, 48);

    // Ambient only (default 0.2): a dim gray.
    s.frame();
    let ambient = s.pixel(white);
    assert!(ambient[0] > 60 && ambient[0] < 160, "{ambient:?}");
    assert_eq!(ambient[0], ambient[2], "gray");
    assert_eq!(
        s.pixel(red),
        [255, 0, 0, 255],
        "unlit: the base color as is"
    );
    let green = s.pixel(glow);
    // So bright that the tone mapper desaturates it toward white (by design),
    // but green dominates.
    assert!(
        green[1] > 240 && green[1] > green[0] + 60 && green[0] == green[2],
        "emissive ×4: {green:?}"
    );

    // A directional light down −Z (the spawn orientation) hits the quads
    // head on.
    let sun = s.scene.spawn();
    s.scene
        .insert_light(sun, Light::directional(Vec3::new(1.0, 1.0, 1.0), 2.0));
    s.frame();
    let lit = s.pixel(white);
    assert!(
        lit[0] > ambient[0] + 40,
        "lit {lit:?} vs ambient {ambient:?}"
    );

    // Turned away, it stops lighting the front faces.
    s.scene
        .set_rotation_euler(sun, 0.0, std::f32::consts::PI, 0.0);
    s.frame();
    assert_eq!(s.pixel(white), ambient);

    // A point light in front, and the colour of the light shows.
    s.scene.remove_light(sun);
    let lamp = s.scene.spawn();
    s.scene.set_translation(lamp, -1.0, 0.0, 1.0);
    s.scene
        .insert_light(lamp, Light::point(Vec3::new(0.0, 0.0, 1.0), 2.0, 0.0));
    s.frame();
    let blue = s.pixel(white);
    assert!(blue[2] > blue[0] + 40, "blue light: {blue:?}");
    // Out of range: no contribution.
    s.scene
        .insert_light(lamp, Light::point(Vec3::new(0.0, 0.0, 1.0), 2.0, 0.5));
    s.frame();
    assert_eq!(s.pixel(white), ambient);

    // Exposure 0: everything lit goes black; unlit doesn't care.
    s.scene.set_exposure(0.0);
    s.frame();
    assert_eq!(s.pixel(white), [0, 0, 0, 255]);
    assert_eq!(s.pixel(glow), [0, 0, 0, 255]);
    assert_eq!(s.pixel(red), [255, 0, 0, 255]);

    // Variants: lit opaque, unlit opaque, unlit blended (hidden but built).
    assert_eq!(s.scene.shader_variant_count(), 3);
    s.scene.destroy();
}

#[wasm_bindgen_test]
async fn transparent_draws_go_back_to_front() {
    let mut s = setup().await;
    for name in ["white", "red", "glow"] {
        s.hide(name);
    }
    // "near" (red, z = 1) comes before "far" (blue, z = −1) in the model, so
    // insertion order alone would draw it first.
    s.frame();
    // Far first: blue·½ over black, then red·½ over that.
    let px = s.pixel(32);
    assert!(
        (px[0] as i32 - 128).abs() <= 2 && px[1] == 0 && (px[2] as i32 - 64).abs() <= 2,
        "{px:?}"
    );

    // Swap their depths: the order follows.
    let near = s.scene.find_by_name(s.root, "near");
    let far = s.scene.find_by_name(s.root, "far");
    s.scene.set_translation(near, 0.0, 0.0, -1.0);
    s.scene.set_translation(far, 0.0, 0.0, 1.0);
    s.frame();
    let px = s.pixel(32);
    assert!(
        (px[0] as i32 - 64).abs() <= 2 && (px[2] as i32 - 128).abs() <= 2,
        "{px:?}"
    );
    s.scene.destroy();
}
