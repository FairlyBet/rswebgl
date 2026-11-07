// The JS-facing Scene API: materials by name and property, cloning one part's
// material, variants following properties, renderFrame, picking, scene
// shaders, option objects. Run with:
// wasm-pack test --headless --chrome packages/rswebgl-scene
#![cfg(target_arch = "wasm32")]

use base64::Engine;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext as Gl};

use rswebgl::context::{Context, ContextOptions};
use rswebgl::draw::{DrawCommand, DrawMode};
use rswebgl::ids::{ParamGroupId, VertexArrayId};
use rswebgl::render_state::RenderState;
use rswebgl::vao_builder::{AttrKind, VaoBuilder};
use rswebgl_ecs::Entity;
use rswebgl_gltf::load_model;
use rswebgl_scene::{Camera, Scene};

wasm_bindgen_test_configure!(run_in_browser);

const W: i32 = 96;
const H: i32 = 32;

fn json(s: &str) -> JsValue {
    js_sys::JSON::parse(s).expect("json")
}

// Three unit quads at x = −1, 0, +1 ("band", "stone_a", "stone_b"), unlit so
// colors read back exactly: "Gold" (yellow) on the band, "Stone" (white) on
// both stones.
fn asset_url() -> String {
    let mut bin = Vec::new();
    #[rustfmt::skip]
    let floats = [-0.5f32, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, 0.5, 0.0];
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
  "extensionsUsed": ["KHR_materials_unlit"],
  "buffers": [{{ "byteLength": {len}, "uri": "data:application/octet-stream;base64,{bin}" }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 12, "target": 34963 }}
  ],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
       "min": [-0.5, -0.5, 0], "max": [0.5, 0.5, 0] }},
    {{ "bufferView": 1, "componentType": 5123, "count": 6, "type": "SCALAR" }}
  ],
  "materials": [
    {{ "name": "Gold", "pbrMetallicRoughness": {{ "baseColorFactor": [1, 1, 0, 1] }},
       "extensions": {{ "KHR_materials_unlit": {{}} }} }},
    {{ "name": "Stone", "pbrMetallicRoughness": {{ "baseColorFactor": [1, 1, 1, 1] }},
       "extensions": {{ "KHR_materials_unlit": {{}} }} }}
  ],
  "meshes": [
    {{ "primitives": [{{ "attributes": {{ "POSITION": 0 }}, "indices": 1, "material": 0 }}] }},
    {{ "primitives": [{{ "attributes": {{ "POSITION": 0 }}, "indices": 1, "material": 1 }}] }}
  ],
  "nodes": [
    {{ "name": "band", "mesh": 0, "translation": [-1, 0, 0] }},
    {{ "name": "stone_a", "mesh": 1, "translation": [0, 0, 0] }},
    {{ "name": "stone_b", "mesh": 1, "translation": [1, 0, 0] }}
  ],
  "scenes": [{{ "nodes": [0, 1, 2] }}],
  "scene": 0
}}"#,
        len = bin.len(),
        bin = b64(&bin),
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

struct Setup {
    ctx: Context,
    gl: Gl,
    scene: Scene,
    camera: Entity,
    root: Entity,
}

async fn setup() -> Setup {
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
    let camera = scene.spawn();
    // Orthographic, 1 unit tall: with the canvas's 3:1 aspect (set by
    // renderFrame) x spans −1.5…1.5 — quad centers at pixels 16, 48, 80.
    scene
        .set_camera(
            camera,
            json(r#"{ "type": "orthographic", "ymag": 0.5, "znear": 0.1, "zfar": 100 }"#),
        )
        .expect("camera");
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    let model = load_model(&ctx, &asset_url()).await.expect("load");
    let root = scene.add_model(model);
    Setup {
        ctx,
        gl,
        scene,
        camera,
        root,
    }
}

impl Setup {
    fn frame(&mut self) -> [[u8; 4]; 3] {
        self.scene.render_frame(self.camera);
        let mut out = [[0u8; 4]; 3];
        for (i, px) in out.iter_mut().enumerate() {
            self.gl
                .read_pixels_with_opt_u8_array(
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
        assert_eq!(self.gl.get_error(), Gl::NO_ERROR);
        out
    }
}

const YELLOW: [u8; 4] = [255, 255, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];

#[wasm_bindgen_test]
async fn materials_by_name_property_and_per_part_clones() {
    let mut s = setup().await;
    // renderFrame gave the camera the canvas's aspect.
    assert_eq!(s.frame(), [YELLOW, WHITE, WHITE]);
    assert_eq!(s.scene.world().get::<Camera>(s.camera).unwrap().aspect, 3.0);

    let gold = s.scene.find_material(s.root, "Gold");
    let stone = s.scene.find_material(s.root, "Stone");
    assert!(!gold.is_null() && !stone.is_null() && gold != stone);
    assert_eq!(s.scene.find_material(s.root, "Nope"), ParamGroupId::NULL);
    assert_eq!(
        s.scene.materials(s.root),
        [gold.to_raw(), stone.to_raw()],
        "distinct, in mesh order"
    );
    assert_eq!(s.scene.material_name(stone).as_deref(), Some("Stone"));
    let node = s.scene.find_by_name(s.root, "stone_b");
    assert_eq!(s.scene.mesh_material(node), stone, "a node's first mesh");

    // A property change reaches every mesh sharing the material.
    s.scene
        .set_material_property(stone, "baseColorFactor", json("[0, 0, 1, 1]"))
        .unwrap();
    assert_eq!(s.frame(), [YELLOW, BLUE, BLUE]);
    assert_eq!(
        s.scene.material_property(stone, "baseColorFactor"),
        Some(vec![0.0, 0.0, 1.0, 1.0])
    );

    // A clone set on one node changes that part only.
    let ruby = s.scene.clone_material(stone).unwrap();
    s.scene
        .set_material_property(ruby, "baseColorFactor", json("[1, 0, 0, 1]"))
        .unwrap();
    s.scene.set_material(node, ruby);
    assert_eq!(s.frame(), [YELLOW, BLUE, RED]);
    assert_eq!(s.scene.material_name(ruby).as_deref(), Some("Stone"));
    s.scene.set_material(node, stone);
    s.scene.destroy_material(ruby);
    assert_eq!(s.frame(), [YELLOW, BLUE, BLUE]);

    // Bad input throws instead of half-applying.
    assert!(
        s.scene
            .set_material_property(gold, "nope", json("1"))
            .is_err()
    );
    assert!(
        s.scene
            .set_material_property(gold, "baseColorFactor", json("1"))
            .is_err()
    );
    assert!(
        s.scene
            .set_material_property(gold, "ior", json(r#""x""#))
            .is_err()
    );
    assert!(
        s.scene
            .set_material_property(ParamGroupId::NULL, "ior", json("1"))
            .is_err()
    );
    s.scene.destroy();
}

#[wasm_bindgen_test]
async fn turning_an_extension_on_switches_the_variant() {
    let mut s = setup().await;
    s.frame();
    let stone = s.scene.find_material(s.root, "Stone");
    let mesh = |s: &Setup, name: &str| {
        let node = s.scene.find_by_name(s.root, name);
        let prim = rswebgl_scene::children(s.scene.world(), node)[0];
        s.scene
            .world()
            .get::<rswebgl_scene::MeshRenderer>(prim)
            .unwrap()
            .program
    };
    let before = (s.scene.shader_variant_count(), mesh(&s, "stone_a"));
    s.scene
        .set_material_property(stone, "clearcoatFactor", json("1"))
        .unwrap();
    assert_eq!(s.scene.shader_variant_count(), before.0 + 1);
    let coated = mesh(&s, "stone_a");
    assert_ne!(coated, before.1);
    assert_eq!(mesh(&s, "stone_b"), coated, "both users switch");
    assert_ne!(mesh(&s, "band"), coated, "other materials don't");
    // Back to zero: the original variant again, nothing new compiled.
    s.scene
        .set_material_property(stone, "clearcoatFactor", json("0"))
        .unwrap();
    assert_eq!(mesh(&s, "stone_a"), before.1);
    assert_eq!(s.scene.shader_variant_count(), before.0 + 1);
    s.scene.destroy();
}

#[wasm_bindgen_test]
async fn picking_hits_triangles_and_skips_hidden_parts() {
    let mut s = setup().await;
    s.frame();
    let node_of = |s: &Setup, hit: Entity| s.scene.parent(hit);
    let band = s.scene.find_by_name(s.root, "band");
    let stone_b = s.scene.find_by_name(s.root, "stone_b");

    let hit = s.scene.pick(s.camera, -2.0 / 3.0, 0.0); // x = −1 in the world
    assert_eq!(node_of(&s, hit), band);
    let p = s.scene.pick_point();
    assert!(
        (p[0] + 1.0).abs() < 1e-4 && p[1].abs() < 1e-4 && p[2].abs() < 1e-4,
        "{p:?}"
    );
    assert!(
        (s.scene.pick_distance() - 4.9).abs() < 1e-3,
        "near plane at z = 4.9"
    );

    let hit = s.scene.pick(s.camera, 2.0 / 3.0, 0.0);
    assert_eq!(node_of(&s, hit), stone_b);
    // Moved away, it isn't there to hit (picking sees the moved matrices
    // after the next frame / updateTransforms).
    s.scene.set_translation(stone_b, 1.0, 5.0, 0.0);
    s.scene.update_transforms();
    assert_eq!(
        s.scene.pick(s.camera, 2.0 / 3.0, 0.0),
        Entity::NULL,
        "nothing there"
    );
    s.scene.set_translation(stone_b, 1.0, 0.0, 0.0);
    s.scene.update_transforms();
    let hit = s.scene.pick(s.camera, 2.0 / 3.0, 0.0);
    assert_eq!(node_of(&s, hit), stone_b);
    s.scene.set_visible(stone_b, false);
    assert_eq!(
        s.scene.pick(s.camera, 2.0 / 3.0, 0.0),
        Entity::NULL,
        "hidden"
    );
    assert!(s.scene.pick_point().is_empty() && s.scene.pick_distance() < 0.0);
    s.scene.destroy();
}

#[wasm_bindgen_test]
async fn scene_shaders_and_options() {
    let mut s = setup().await;
    // A scene shader: no #version, no declarations — the prelude has them.
    let vert = "layout(location = 0) in vec3 a_position;\n\
                void main() { gl_Position = u_view_proj * u_model * vec4(a_position, 1.0); }";
    let frag = "out vec4 frag_color;\n\
                void main() { frag_color = vec4(scene_output(vec3(0.0, 0.0, 1.0)), 1.0); }";
    let program = s.scene.create_program(vert, frag).expect("program");
    assert!(
        s.ctx.program_is_valid(program) || {
            s.ctx.program_active_uniform_names(program);
            s.ctx.program_is_valid(program)
        }
    );
    // Compile errors point at the user's own lines.
    let err = match s
        .scene
        .create_program(vert, "out vec4 c;\nvoid main() { c = nope; }")
    {
        Err(e) => e.message().to_string(),
        Ok(p) => {
            s.ctx.program_active_uniform_names(p);
            format!("linked: {}", s.ctx.program_is_valid(p))
        }
    };
    assert!(
        err.contains("0:2"),
        "error on line 2 of the user's code: {err}"
    );

    // A mesh of our own through setMesh's options.
    let mut vb = VaoBuilder::new(&s.ctx);
    vb.add_f32(
        0,
        AttrKind::F32x3,
        &[
            -0.5, -0.5, 0.5, 0.5, -0.5, 0.5, 0.5, 0.5, 0.5, -0.5, -0.5, 0.5, 0.5, 0.5, 0.5, -0.5,
            0.5, 0.5,
        ],
    );
    let vao: VertexArrayId = vb.build().unwrap();
    let state = s.ctx.render_state(&RenderState::new());
    let e = s.scene.spawn();
    let opts = format!(
        r#"{{ "program": {}, "state": {}, "vao": {} }}"#,
        program.to_raw(),
        state.to_raw(),
        vao.to_raw()
    );
    let draw = DrawCommand::arrays(DrawMode::Triangles, 0, 6);
    s.scene.set_mesh(e, json(&opts), &draw).expect("mesh");
    // Blue through scene_output: tone mapping desaturates it a little.
    let px = s.frame()[1];
    assert!(
        px[2] > 220 && px[0] < 60 && px[0] == px[1],
        "drawn in front of stone_a: {px:?}"
    );
    assert!(
        s.scene.set_mesh(e, json("{}"), &draw).is_err(),
        "program and state required"
    );

    // Option objects are checked.
    assert!(
        s.scene
            .set_light(e, json(r#"{ "type": "laser" }"#))
            .is_err()
    );
    assert!(
        s.scene
            .set_light(e, json(r#"{ "type": "point", "color": [1, 2] }"#))
            .is_err()
    );
    assert!(
        s.scene
            .set_light(
                e,
                json(r#"{ "type": "spot", "innerConeAngle": 1, "outerConeAngle": 0.5 }"#)
            )
            .is_err()
    );
    s.scene
        .set_light(
            e,
            json(r#"{ "type": "point", "color": [1, 0.5, 0], "intensity": 3 }"#),
        )
        .expect("light");
    assert!(
        s.scene
            .set_camera(e, json(r#"{ "type": "fisheye" }"#))
            .is_err()
    );
    assert!(
        s.scene
            .set_camera(e, json(r#"{ "type": "perspective", "znear": 0 }"#))
            .is_err()
    );
    assert!(
        s.scene.set_camera(e, JsValue::from(3)).is_err(),
        "not an object"
    );

    // Reads into a buffer of the caller's.
    let mut m = [0.0f32; 16];
    let band = s.scene.find_by_name(s.root, "band");
    assert!(s.scene.copy_world_matrix(band, &mut m));
    assert_eq!(m[12], -1.0);
    assert_eq!(s.scene.world_position(band), [-1.0, 0.0, 0.0]);
    s.scene.destroy();
}
