// GPU resource sharing and cleanup for a loaded model, against a real WebGL2
// context. The asset is assembled here and loaded from a data: URL.
// Run with: wasm-pack test --headless --chrome packages/rswebgl-gltf
#![cfg(target_arch = "wasm32")]

use base64::Engine;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::*;

use rswebgl::context::{Context, ContextOptions};
use rswebgl_gltf::{Model, load_model};

wasm_bindgen_test_configure!(run_in_browser);

// A 1×1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

const INSTANCE_X: [f32; 3] = [10.0, 20.0, 30.0];

// One triangle mesh with two primitives sharing its POSITION and index
// accessors (two materials). Node 0 and node 1 both use the mesh; node 2 uses
// it through EXT_mesh_gpu_instancing (3 instances, marked *required*). The two
// materials' base-color textures are two glTF textures over one image, with
// different samplers.
fn asset_url() -> String {
    let mut bin = Vec::new();
    for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&v.to_le_bytes()); // positions: 0..36
    }
    for i in [0u16, 1, 2, 0] {
        bin.extend_from_slice(&i.to_le_bytes()); // indices (+pad): 36..44
    }
    for x in INSTANCE_X {
        for v in [x, 0.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes()); // translations: 44..80
        }
    }
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "extensionsUsed": ["EXT_mesh_gpu_instancing"],
  "extensionsRequired": ["EXT_mesh_gpu_instancing"],
  "buffers": [{{ "byteLength": {len}, "uri": "data:application/octet-stream;base64,{bin}" }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }},
    {{ "buffer": 0, "byteOffset": 36, "byteLength": 6, "target": 34963 }},
    {{ "buffer": 0, "byteOffset": 44, "byteLength": 36 }}
  ],
  "accessors": [
    {{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
       "min": [0, 0, 0], "max": [1, 1, 0] }},
    {{ "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR" }},
    {{ "bufferView": 2, "componentType": 5126, "count": 3, "type": "VEC3" }}
  ],
  "images": [{{ "uri": "data:image/png;base64,{PNG}" }}],
  "samplers": [{{ "wrapS": 33071 }}],
  "textures": [{{ "source": 0 }}, {{ "source": 0, "sampler": 0 }}],
  "materials": [
    {{ "pbrMetallicRoughness": {{ "baseColorTexture": {{ "index": 0 }} }} }},
    {{ "pbrMetallicRoughness": {{ "baseColorTexture": {{ "index": 1 }} }} }}
  ],
  "meshes": [{{ "primitives": [
    {{ "attributes": {{ "POSITION": 0 }}, "indices": 1, "material": 0 }},
    {{ "attributes": {{ "POSITION": 0 }}, "indices": 1, "material": 1 }}
  ] }}],
  "nodes": [
    {{ "mesh": 0 }},
    {{ "mesh": 0, "translation": [5, 0, 0] }},
    {{ "mesh": 0, "translation": [0, 1, 0],
       "extensions": {{ "EXT_mesh_gpu_instancing": {{ "attributes": {{ "TRANSLATION": 2 }} }} }} }}
  ],
  "scenes": [{{ "nodes": [0, 1, 2] }}],
  "scene": 0
}}"#,
        len = bin.len(),
        bin = b64(&bin),
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

fn context() -> Context {
    let canvas: web_sys::HtmlCanvasElement = web_sys::window()
        .unwrap()
        .document()
        .unwrap()
        .create_element("canvas")
        .unwrap()
        .dyn_into()
        .unwrap();
    Context::from_canvas(&canvas, &ContextOptions::new()).expect("ctx")
}

async fn load(ctx: &Context) -> Model {
    load_model(ctx, &asset_url()).await.expect("load")
}

async fn sleep(ms: i32) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        web_sys::window()
            .unwrap()
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .unwrap();
    });
    JsFuture::from(p).await.unwrap();
}

// Messages logged via console.error while the future runs.
async fn capture_errors<F: std::future::Future<Output = ()>>(f: F) -> Vec<String> {
    let console = js_sys::Reflect::get(&js_sys::global(), &"console".into()).unwrap();
    let original = js_sys::Reflect::get(&console, &"error".into()).unwrap();
    let log = js_sys::Array::new();
    let push = js_sys::Function::new_with_args("m", "this.push(String(m))").bind0(&log);
    js_sys::Reflect::set(&console, &"error".into(), &push).unwrap();
    f.await;
    js_sys::Reflect::set(&console, &"error".into(), &original).unwrap();
    log.iter().filter_map(|v| v.as_string()).collect()
}

#[wasm_bindgen_test]
async fn shared_data_is_uploaded_once() {
    let ctx = context();
    let model = load(&ctx).await;

    // 2 primitives × (node 0 + node 1 + 3 instances of node 2).
    assert_eq!(model.draw_item_count(), 10);
    // …all drawing one mesh: 2 vertex arrays over 2 buffers.
    assert_eq!(model.mesh_count(), 1);
    assert_eq!(model.vertex_array_count(), 2);
    assert_eq!(model.buffer_count(), 2);
    let vaos: std::collections::HashSet<_> = model.items().iter().map(|i| i.vao()).collect();
    assert_eq!(vaos.len(), 2);
    // Two glTF textures over one image, sampled two ways: one GPU texture,
    // two samplers.
    assert_eq!(model.texture_count(), 1);
    assert_eq!(model.sampler_count(), 2);
    // Indices stay 16-bit.
    let cmd = model.items()[0].draw_command();
    assert_eq!(cmd.index_type, rswebgl::draw::IndexType::UnsignedShort);
}

#[wasm_bindgen_test]
async fn gpu_instances_compose_with_the_node() {
    let ctx = context();
    let model = load(&ctx).await;
    let mut xs: Vec<(u32, f32, f32)> = model
        .items()
        .iter()
        .filter(|i| i.node() == 2)
        .map(|i| (i.instance(), i.model_cols()[12], i.model_cols()[13]))
        .collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    xs.dedup();
    // Instance i at node translation (0, 1, 0) + (INSTANCE_X[i], 0, 0).
    assert_eq!(xs, vec![(0, 10.0, 1.0), (1, 20.0, 1.0), (2, 30.0, 1.0)],);
}

// Destroying right after load, while the image is still decoding: everything
// goes, and the late image is dropped without a stale-handle error.
#[wasm_bindgen_test]
async fn destroy_frees_everything_even_mid_load() {
    let ctx = context();
    let errors = capture_errors(async {
        let model = load(&ctx).await;
        let tex = model.texture(0);
        let vao = model.items()[0].vao();
        model.destroy(&ctx);
        assert!(!ctx.texture_is_valid(tex));
        assert!(!ctx.vertex_array_is_valid(vao));
        assert!(model.buffers().iter().all(|&b| !ctx.buffer_is_valid(b)));
        sleep(200).await; // let the image load land
    })
    .await;
    assert!(errors.is_empty(), "{errors:?}");
}
