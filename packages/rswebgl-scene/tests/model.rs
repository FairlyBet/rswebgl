// A glTF model through the JS-facing `Scene`: node tree → entities, names,
// moving and hiding parts, cleanup on despawn. Run with:
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
use rswebgl_scene::{Camera, Parent, Scene};

wasm_bindgen_test_configure!(run_in_browser);

const SIZE: i32 = 64;

// Model shaders: the scene's frame block and object matrix, the loader's
// material factor.
const VERT: &str = r#"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(std140) uniform Frame {
    mat4 u_view;
    mat4 u_proj;
    mat4 u_view_proj;
    vec4 u_camera_pos;
};
uniform mat4 u_model;
void main() { gl_Position = u_view_proj * u_model * vec4(a_position, 1.0); }
"#;
const FRAG: &str = r#"#version 300 es
precision highp float;
uniform vec4 u_base_color_factor;
out vec4 frag_color;
void main() { frag_color = u_base_color_factor; }
"#;

const BLACK: [u8; 4] = [0, 0, 0, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

// A unit quad mesh (two primitives: one triangle each), used by two nodes:
//   "ring" (x = −1) ─ "stone" (+2, so x = +1 in the world)
//   "side" (x = −1, a root)
fn asset_url() -> String {
    let mut bin = Vec::new();
    #[rustfmt::skip]
    let positions = [-0.5f32, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, 0.5, 0.0];
    for v in positions {
        bin.extend_from_slice(&v.to_le_bytes()); // 0..48
    }
    for i in [0u16, 1, 2, 0, 2, 3] {
        bin.extend_from_slice(&i.to_le_bytes()); // 48..60
    }
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "buffers": [{{ "byteLength": {len}, "uri": "data:application/octet-stream;base64,{bin}" }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 48 }},
    {{ "buffer": 0, "byteOffset": 48, "byteLength": 6, "target": 34963 }},
    {{ "buffer": 0, "byteOffset": 54, "byteLength": 6, "target": 34963 }}
  ],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3",
       "min": [-0.5, -0.5, 0], "max": [0.5, 0.5, 0] }},
    {{ "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR" }},
    {{ "bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR" }}
  ],
  "meshes": [{{ "primitives": [
    {{ "attributes": {{ "POSITION": 0 }}, "indices": 1 }},
    {{ "attributes": {{ "POSITION": 0 }}, "indices": 2 }}
  ] }}],
  "nodes": [
    {{ "name": "ring", "translation": [-1, 0, 0], "children": [1] }},
    {{ "name": "stone", "mesh": 0, "translation": [2, 0, 0] }},
    {{ "name": "side", "mesh": 0, "translation": [-1, 0, 0] }}
  ],
  "scenes": [{{ "nodes": [0, 2] }}],
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
    pass: Pass,
    camera: Entity,
}

fn setup() -> Setup {
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
    // 4 units across, looking down −Z: x = −1, 0, +1 at pixels 16, 32, 48.
    let camera = scene.spawn();
    scene.insert_camera(camera, Camera::orthographic(2.0, 1.0, 0.1, 100.0));
    scene.look_at(camera, &[0.0, 0.0, 5.0], &[0.0, 0.0, 0.0]);
    let mut pass = Pass::to_default();
    // The loader's render states test depth: clear it too, or the next
    // frame in the same task fails every fragment.
    pass.set_clear(&ClearMask::color_depth());
    Setup {
        ctx,
        gl,
        scene,
        pass,
        camera,
    }
}

impl Setup {
    fn frame(&mut self) -> usize {
        self.pass.clear_draws();
        let drawn = self.scene.render(self.camera, &mut self.pass);
        self.pass.sort();
        self.ctx.render(&self.pass);
        drawn
    }

    fn read(&self) -> [[u8; 4]; 3] {
        let mut out = [[0u8; 4]; 3];
        for (i, px) in out.iter_mut().enumerate() {
            self.gl
                .read_pixels_with_opt_u8_array(
                    SIZE / 4 + i as i32 * SIZE / 4,
                    SIZE / 2,
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

#[wasm_bindgen_test]
async fn a_model_becomes_a_hierarchy_of_entities() {
    let mut s = setup();
    let model = load_model(&s.ctx, &asset_url()).await.expect("load");
    let buffers = model.buffers().to_vec();
    let program = s.ctx.create_program(VERT, FRAG).expect("program");
    let root = s.scene.add_model_with_program(model, program);

    // camera + root + 3 nodes + 2 primitives × 2 nodes with the mesh.
    assert_eq!(s.scene.entity_count(), 1 + 1 + 3 + 4);
    assert_eq!(s.frame(), 4);
    assert_eq!(s.read(), [WHITE, BLACK, WHITE]);

    let ring = s.scene.find_by_name(root, "ring");
    let stone = s.scene.find_by_name(root, "stone");
    assert!(s.scene.is_alive(ring) && s.scene.is_alive(stone));
    assert_eq!(s.scene.parent(stone), ring);
    assert_eq!(s.scene.parent(ring), root);
    assert_eq!(s.scene.name(stone).as_deref(), Some("stone"));
    assert_eq!(s.scene.find_by_name(root, "nope"), Entity::NULL);
    assert_eq!(
        s.scene.find_by_name(s.camera, "stone"),
        Entity::NULL,
        "not under the camera"
    );
    assert_eq!(&s.scene.world_matrix(stone)[12..15], &[1.0, 0.0, 0.0]);
    assert_eq!(s.scene.translation(stone), [2.0, 0.0, 0.0], "local");
    let bounds = s.scene.model_bounds(root);
    assert_eq!(bounds.len(), 4);

    // Moving a node moves its primitives.
    s.scene.set_translation(ring, -2.0, 0.0, 0.0);
    s.frame();
    assert_eq!(s.read(), [WHITE, WHITE, BLACK]);
    // Moving the root moves everything.
    s.scene.set_translation(root, 1.0, 0.0, 0.0);
    s.frame();
    assert_eq!(s.read(), [BLACK, WHITE, WHITE]);
    s.scene.set_translation(root, 0.0, 0.0, 0.0);
    s.scene.set_translation(ring, -1.0, 0.0, 0.0);

    // Hiding a node hides its primitives (children), and a hidden ancestor
    // wins over a visible child.
    s.scene.set_visible(stone, false);
    assert_eq!(s.frame(), 2);
    assert_eq!(s.read(), [WHITE, BLACK, BLACK]);
    s.scene.set_visible(stone, true);
    s.scene.set_visible(root, false);
    assert_eq!(s.frame(), 0);
    s.scene.set_visible(root, true);
    assert_eq!(s.frame(), 4);

    // Despawning the root takes the whole model, GPU resources included.
    assert_eq!(s.scene.despawn(root), 1 + 3 + 4);
    assert_eq!(s.scene.entity_count(), 1);
    assert!(buffers.iter().all(|&b| !s.ctx.buffer_is_valid(b)));
    assert_eq!(s.frame(), 0);
    assert_eq!(s.read(), [BLACK, BLACK, BLACK]);
    s.scene.destroy();
}

#[wasm_bindgen_test]
fn scene_setters_reject_dead_entities_and_cycles() {
    let mut s = setup();
    let a = s.scene.spawn();
    let b = s.scene.spawn();
    assert!(s.scene.set_parent(b, a));
    assert!(!s.scene.set_parent(a, b), "cycle");
    assert_eq!(s.scene.world().get::<Parent>(a), None);
    s.scene.clear_parent(b);
    assert_eq!(s.scene.parent(b), Entity::NULL);

    s.scene.despawn(a);
    s.scene.set_translation(a, 1.0, 2.0, 3.0);
    assert!(s.scene.translation(a).is_empty(), "no-op on a dead entity");
    assert!(!s.scene.is_alive(a));

    s.scene.set_rotation(b, 0.0, 0.0, 2.0, 0.0);
    assert_eq!(s.scene.rotation(b), [0.0, 0.0, 1.0, 0.0], "normalized");
    s.scene.set_local_matrix(b, &[0.0; 3]);
    assert_eq!(
        s.scene.translation(b),
        [0.0, 0.0, 0.0],
        "wrong length ignored"
    );
    s.scene.update_transforms();
    assert_eq!(s.scene.world_matrix(b).len(), 16);
    s.scene.destroy();
}
