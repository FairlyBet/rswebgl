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

// A 1×1 RGBA PNG of one color: stored (uncompressed) deflate, so it's built
// here rather than pasted.
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
    let raw = [0, rgba[0], rgba[1], rgba[2], rgba[3]]; // filter byte + pixel
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

// One triangle with a material whose occlusion and metallic-roughness maps
// are separate textures (so they're packed into one ORM texture), the
// occlusion one given by `occlusion_uri`.
fn orm_asset_url(occlusion_uri: &str) -> String {
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let mut bin = Vec::new();
    for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        bin.extend_from_slice(&v.to_le_bytes());
    }
    let mr = format!("data:image/png;base64,{}", b64(&png([0, 128, 255, 255])));
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "buffers": [{{ "byteLength": 36, "uri": "data:application/octet-stream;base64,{bin}" }}],
  "bufferViews": [{{ "buffer": 0, "byteLength": 36 }}],
  "accessors": [{{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                   "min": [0, 0, 0], "max": [1, 1, 0] }}],
  "images": [{{ "uri": "{occlusion_uri}" }}, {{ "uri": "{mr}" }}],
  "textures": [{{ "source": 0 }}, {{ "source": 1 }}],
  "materials": [{{
    "occlusionTexture": {{ "index": 0 }},
    "pbrMetallicRoughness": {{ "metallicRoughnessTexture": {{ "index": 1 }} }}
  }}],
  "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0 }}, "material": 0 }}] }}],
  "nodes": [{{ "mesh": 0 }}],
  "scenes": [{{ "nodes": [0] }}]
}}"#,
        bin = b64(&bin)
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

// The packed texture's one texel, once the pack has run.
async fn read_orm(ctx: &Context, model: &Model) -> Vec<u8> {
    assert_eq!(
        model.texture_count(),
        1,
        "the two maps became one ORM texture"
    );
    let tex = model.texture(0);
    for _ in 0..50 {
        sleep(20).await;
        let fb = ctx.create_framebuffer(1, 1).unwrap();
        ctx.framebuffer_attach_color_texture_2d(fb, 0, tex, 0);
        let complete = ctx.is_framebuffer_complete(fb);
        let px = complete.then(|| {
            ctx.read_pixels(fb, 0, 0, 0, 1, 1, rswebgl::readback::ReadFormat::Rgba8)
                .unwrap()
        });
        ctx.destroy_framebuffer(fb);
        if let Some(px) = px {
            return px;
        }
    }
    panic!("the ORM texture was never filled");
}

#[wasm_bindgen_test]
async fn separate_occlusion_and_mr_maps_pack_into_one() {
    let ctx = context();
    let occlusion = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png([200, 0, 0, 255]))
    );
    let model = load_model(&ctx, &orm_asset_url(&occlusion))
        .await
        .expect("load");
    // R = occlusion, G = roughness, B = metallic.
    assert_eq!(read_orm(&ctx, &model).await, [200, 128, 255, 255]);
}

// An occlusion map that can't load doesn't stall the pack or blacken the
// model: it packs as "no occlusion" (1), and says so.
#[wasm_bindgen_test]
async fn a_failed_orm_source_packs_as_a_constant() {
    let ctx = context();
    let mut model = None;
    let errors = capture_errors(async {
        // Not base64: rejected before any request, which used to leave the
        // pack waiting forever.
        let m = load_model(&ctx, &orm_asset_url("data:image/png;base64,***"))
            .await
            .expect("load");
        let px = read_orm(&ctx, &m).await;
        model = Some((m, px));
    })
    .await;
    let (_, px) = model.unwrap();
    assert_eq!(px, [255, 128, 255, 255]);
    assert!(
        errors.iter().any(|e| e.contains("occlusion map failed")),
        "{errors:?}"
    );
}

// Losing the context while the model downloads is an error, not a panic.
#[wasm_bindgen_test]
async fn a_context_lost_during_load_is_an_error() {
    let ctx = context();
    let url = asset_url();
    let load = load_model(&ctx, &url);
    assert!(ctx.simulate_context_loss());
    sleep(20).await;
    let err = load.await.err().expect("an error");
    assert!(err.contains("lost"), "{err}");
}

// A sparse accessor with no bufferView: zeros underneath, three vertices
// substituted (glTF 2.0 §3.6.2.3); the uploaded positions must be them.
fn sparse_url() -> String {
    let mut bin = Vec::new();
    for i in [0u16, 1, 2, 0] {
        bin.extend_from_slice(&i.to_le_bytes()); // sparse indices (+pad): 0..8
    }
    for v in [-1.0f32, -2.0, 0.0, 3.0, -2.0, 0.0, 0.0, 4.0, 0.5] {
        bin.extend_from_slice(&v.to_le_bytes()); // sparse values: 8..44
    }
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let json = format!(
        r#"{{
  "asset": {{ "version": "2.0" }},
  "buffers": [{{ "byteLength": {len}, "uri": "data:application/octet-stream;base64,{bin}" }}],
  "bufferViews": [
    {{ "buffer": 0, "byteOffset": 0, "byteLength": 6 }},
    {{ "buffer": 0, "byteOffset": 8, "byteLength": 36 }}
  ],
  "accessors": [
    {{ "componentType": 5126, "count": 4, "type": "VEC3",
       "min": [-1, -2, 0], "max": [3, 4, 0.5],
       "sparse": {{ "count": 3,
         "indices": {{ "bufferView": 0, "componentType": 5123 }},
         "values": {{ "bufferView": 1 }} }} }}
  ],
  "meshes": [{{ "primitives": [{{ "attributes": {{ "POSITION": 0 }} }}] }}],
  "nodes": [{{ "mesh": 0 }}],
  "scenes": [{{ "nodes": [0] }}],
  "scene": 0
}}"#,
        len = bin.len(),
        bin = b64(&bin),
    );
    format!("data:model/gltf+json;base64,{}", b64(json.as_bytes()))
}

#[wasm_bindgen_test]
async fn sparse_accessors_without_a_view_load() {
    let ctx = context();
    let model = load_model(&ctx, &sparse_url()).await.expect("load");
    assert_eq!(model.draw_item_count(), 1);
    assert_eq!(model.items()[0].draw_command().count, 4);
    // Vertices 0..3 are the sparse values, vertex 3 the zero base.
    let bytes = ctx.buffer_read(model.buffers()[0], 0, 48).expect("read");
    let floats: Vec<f32> = bytes
        .chunks(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(
        floats,
        [
            -1.0, -2.0, 0.0, 3.0, -2.0, 0.0, 0.0, 4.0, 0.5, 0.0, 0.0, 0.0
        ]
    );
}
