//! `KHR_draco_mesh_compression`: primitives whose attributes and indices
//! are a Draco-compressed bufferView, decoded in workers (`decoders.js`) by
//! the Draco decoder the app points at (`setDracoDecoderPath`: a directory
//! holding `draco_wasm_wrapper.js` and `draco_decoder.wasm`, as three.js'
//! DRACOLoader takes — e.g. Google's
//! `https://www.gstatic.com/draco/versioned/decoders/1.5.7/`). Attributes
//! decode into their accessors' component types, as three.js'
//! GLTFDracoMeshCompressionExtension asks (MIT); the results go into one
//! new buffer, and the accessors are pointed at it in the JSON, so
//! everything after reads them like any other.

use js_sys::{Array, Object, Reflect, Uint8Array};
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::ktx2::{draco_decode, has_decoder, set_decoder_path};

/// Where the Draco decoder is: a directory URL holding
/// `draco_wasm_wrapper.js` and `draco_decoder.wasm`. Set before loading
/// models with Draco-compressed meshes (`KHR_draco_mesh_compression`);
/// without it they fail to load (reported). Fetched on first use;
/// decoding runs in workers.
#[wasm_bindgen(js_name = setDracoDecoderPath)]
pub fn set_draco_decoder_path(path: &str) {
    set_decoder_path("draco", path);
}

fn components(ty: &str) -> usize {
    match ty {
        "SCALAR" => 1,
        "VEC2" => 2,
        "VEC3" => 3,
        "VEC4" | "MAT2" => 4,
        "MAT3" => 9,
        "MAT4" => 16,
        _ => 0,
    }
}

fn component_size(ct: u64) -> usize {
    match ct {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        _ => 4,
    }
}

fn err(e: JsValue) -> String {
    e.dyn_ref::<js_sys::Error>()
        .map(|e| String::from(e.message()))
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"))
}

// Appends `data` to `out` at a 4-byte boundary; its offset.
fn append(out: &mut Vec<u8>, data: &[u8]) -> usize {
    out.resize(out.len().div_ceil(4) * 4, 0);
    let at = out.len();
    out.extend_from_slice(data);
    at
}

/// Decodes every Draco-compressed primitive into a new buffer (appended to
/// `buffers` and to `json`'s), pointing its accessors there. Returns
/// whether there were any (the document is then rebuilt from `json`).
pub async fn decode(json: &mut Value, buffers: &mut Vec<Vec<u8>>) -> Result<bool, String> {
    // (bufferView, attributes: [(accessor, unique id)], indices accessor)
    let mut jobs = Vec::new();
    let meshes = json["meshes"].as_array().map_or(&[][..], Vec::as_slice);
    for mesh in meshes {
        let prims = mesh["primitives"].as_array().map_or(&[][..], Vec::as_slice);
        for p in prims {
            let Some(ext) = p.pointer("/extensions/KHR_draco_mesh_compression") else {
                continue;
            };
            let view = ext
                .get("bufferView")
                .and_then(Value::as_u64)
                .ok_or("Draco: a primitive without its bufferView")?;
            // The primitive's own attribute → accessor map, by the names the
            // extension uses.
            let own = &p["attributes"];
            let mut attributes = Vec::new();
            if let Some(map) = ext.get("attributes").and_then(Value::as_object) {
                for (name, id) in map {
                    if let (Some(id), Some(acc)) = (id.as_u64(), own[name].as_u64()) {
                        attributes.push((acc as usize, id));
                    }
                }
            }
            let indices = p["indices"].as_u64().map(|a| a as usize);
            jobs.push((view as usize, attributes, indices));
        }
    }
    if jobs.is_empty() {
        return Ok(false);
    }
    if !has_decoder("draco") {
        return Err("Draco-compressed meshes need the decoder: setDracoDecoderPath".into());
    }
    let accessor = |i: usize| -> (u64, usize) {
        let a = &json["accessors"][i];
        (
            a["componentType"].as_u64().unwrap_or(5126),
            components(a["type"].as_str().unwrap_or("")),
        )
    };
    // All jobs go out at once: the pool spreads them over its workers.
    let mut pending = Vec::new();
    for (view, attributes, _) in &jobs {
        let v = &json["bufferViews"][*view];
        let start = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = v["byteLength"].as_u64().unwrap_or(0) as usize;
        let data = buffers
            .get(v["buffer"].as_u64().unwrap_or(u64::MAX) as usize)
            .and_then(|b| b.get(start..start + len))
            .ok_or("Draco: bufferView out of range")?;
        let asks = Array::new();
        for &(acc, id) in attributes {
            let o = Object::new();
            let _ = Reflect::set(&o, &"id".into(), &(id as f64).into());
            let _ = Reflect::set(
                &o,
                &"componentType".into(),
                &(accessor(acc).0 as f64).into(),
            );
            asks.push(&o);
        }
        pending.push(JsFuture::from(draco_decode(&Uint8Array::from(data), &asks)));
    }

    let mut out = Vec::new();
    let mut views = Vec::new(); // (accessor, offset, length, stride)
    for ((_, attributes, indices), job) in jobs.iter().zip(pending) {
        let r = job.await.map_err(|e| format!("Draco: {}", err(e)))?;
        let decoded: Array = Reflect::get(&r, &"attributes".into())
            .and_then(|a| a.dyn_into())
            .map_err(|_| "Draco: bad decoder result")?;
        let count = Reflect::get(&r, &"count".into())
            .ok()
            .and_then(|c| c.as_f64())
            .unwrap_or(0.0) as usize;
        for (&(acc, _), d) in attributes.iter().zip(decoded.iter()) {
            let data: Uint8Array = Reflect::get(&d, &"data".into())
                .and_then(|a| a.dyn_into())
                .map_err(|_| "Draco: bad attribute")?;
            let data = data.to_vec();
            let (ct, n) = accessor(acc);
            // Vertex attributes start every element on 4 bytes (glTF §3.6.2.4):
            // pad as three.js' decodeAttribute does.
            let src = n * component_size(ct);
            let stride = src.div_ceil(4) * 4;
            let bytes = if src == stride || src == 0 {
                data
            } else {
                let mut padded = vec![0u8; count * stride];
                for (i, e) in data.chunks_exact(src).enumerate() {
                    padded[i * stride..i * stride + src].copy_from_slice(e);
                }
                padded
            };
            let at = append(&mut out, &bytes);
            views.push((acc, at, bytes.len(), (stride != src).then_some(stride)));
        }
        if let Some(acc) = indices {
            let index: js_sys::Uint32Array = Reflect::get(&r, &"index".into())
                .and_then(|a| a.dyn_into())
                .map_err(|_| "Draco: bad index")?;
            let index = index.to_vec();
            let bytes: Vec<u8> = match accessor(*acc).0 {
                5121 => index.iter().map(|&i| i as u8).collect(),
                5123 => index
                    .iter()
                    .flat_map(|&i| (i as u16).to_le_bytes())
                    .collect(),
                _ => index.iter().flat_map(|&i| i.to_le_bytes()).collect(),
            };
            let at = append(&mut out, &bytes);
            views.push((*acc, at, bytes.len(), None));
        }
    }

    // The new buffer and a view per accessor, in the JSON.
    let buffer = buffers.len();
    let length = out.len();
    buffers.push(out);
    let root = json.as_object_mut().ok_or("glTF: not an object")?;
    root.entry("buffers")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("glTF: bad buffers")?
        .push(json!({ "byteLength": length }));
    let view_list = root
        .entry("bufferViews")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or("glTF: bad bufferViews")?;
    let mut targets = Vec::new();
    for (acc, at, len, stride) in views {
        let mut v = json!({ "buffer": buffer, "byteOffset": at, "byteLength": len });
        if let Some(s) = stride {
            v["byteStride"] = json!(s);
        }
        targets.push((acc, view_list.len()));
        view_list.push(v);
    }
    let accessors = json["accessors"]
        .as_array_mut()
        .ok_or("glTF: bad accessors")?;
    for (acc, view) in targets {
        if let Some(a) = accessors.get_mut(acc).and_then(Value::as_object_mut) {
            a.insert("bufferView".into(), json!(view));
            a.insert("byteOffset".into(), json!(0));
        }
    }
    Ok(true)
}
