//! `KHR_texture_basisu`: KTX2 textures (Basis Universal ETC1S or UASTC),
//! transcoded in workers (`decoders.js`) by the Basis Universal transcoder
//! the app points at (`setKtx2TranscoderPath`: a directory holding
//! `basis_transcoder.js` and `.wasm`, as three.js' KTX2Loader takes — from
//! BinomialLLC's basis_universal or three.js' `examples/jsm/libs/basis/`),
//! to the best compressed format the device takes (three.js' choice, with
//! sRGB formats for color maps), then uploaded here.

use js_sys::{Object, Reflect, Uint8Array};
use rswebgl::compressed_format::CompressedFormat;
use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::extension::Extension;
use rswebgl::ids::TextureId;
use rswebgl::texture::TextureFormat;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/src/decoders.js")]
extern "C" {
    #[wasm_bindgen(js_name = setDecoderPath)]
    pub(crate) fn set_decoder_path(kind: &str, path: &str);
    #[wasm_bindgen(js_name = hasDecoder)]
    pub(crate) fn has_decoder(kind: &str) -> bool;
    #[wasm_bindgen(js_name = ktx2Transcode)]
    fn ktx2_transcode(bytes: &Uint8Array, srgb: bool, config: &JsValue) -> js_sys::Promise;
    #[wasm_bindgen(js_name = dracoDecode)]
    pub(crate) fn draco_decode(bytes: &Uint8Array, attributes: &JsValue) -> js_sys::Promise;
}

/// Where the Basis Universal transcoder is: a directory URL holding
/// `basis_transcoder.js` and `basis_transcoder.wasm`. Set before loading
/// models with KTX2 textures (`KHR_texture_basisu`); without it such a
/// texture loads its PNG/JPEG fallback if the file has one, else stays
/// empty (reported). Fetched on first use; transcoding runs in workers.
#[wasm_bindgen(js_name = setKtx2TranscoderPath)]
pub fn set_ktx2_transcoder_path(path: &str) {
    set_decoder_path("ktx2", path);
}

/// Whether a transcoder path is set.
pub(crate) fn has_transcoder() -> bool {
    has_decoder("ktx2")
}

// Basis Universal's transcoder_texture_format values (three.js'
// KTX2Loader.TranscoderFormat).
const TF_ETC1: u32 = 0;
const TF_ETC2: u32 = 1;
const TF_BC1: u32 = 2;
const TF_BC3: u32 = 3;
const TF_BC7_M5: u32 = 7;
const TF_ASTC_4X4: u32 = 10;

// The GL format a transcoder format uploads as (linear, sRGB); None:
// uncompressed RGBA8.
fn gl_format(tf: u32, srgb: bool) -> Option<u32> {
    let (linear, s) = match tf {
        TF_ASTC_4X4 => (0x93B0, 0x93D0),
        TF_BC7_M5 => (0x8E8C, 0x8E8D),
        TF_BC1 => (0x83F1, 0x8C4D),
        TF_BC3 => (0x83F3, 0x8C4F),
        TF_ETC1 => (0x9274, 0x9275),
        TF_ETC2 => (0x9278, 0x9279),
        _ => return None,
    };
    Some(if srgb { s } else { linear })
}

// What the device takes, for the worker's choice (enabling it here).
fn config(ctx: &Context) -> JsValue {
    let c = Object::new();
    let set = |k: &str, e: Extension| {
        let _ = Reflect::set(&c, &k.into(), &ctx.enable_extension_quietly(e).into());
    };
    set("astc", Extension::WebglCompressedTextureAstc);
    set("bptc", Extension::ExtTextureCompressionBptc);
    set("dxt", Extension::WebglCompressedTextureS3tc);
    set("dxtSrgb", Extension::WebglCompressedTextureS3tcSrgb);
    set("etc2", Extension::WebglCompressedTextureEtc);
    c.into()
}

/// Transcodes the KTX2 file `bytes` into `out` (created empty, its
/// sampler set) in a worker, then calls `on_load(width, height)`, or
/// `(0, 0)` when it fails (reported). `srgb`: a color map; `mips`: its
/// sampler filters across mips (a file without them gets them generated
/// when uncompressed, else samples its one level).
pub(crate) fn load(
    ctx: &Context,
    out: TextureId,
    bytes: Vec<u8>,
    srgb: bool,
    mips: bool,
    on_load: js_sys::Function,
) {
    let ctx = ctx.clone();
    let config = config(&ctx);
    let job = ktx2_transcode(&Uint8Array::from(bytes.as_slice()), srgb, &config);
    drop(bytes);
    wasm_bindgen_futures::spawn_local(async move {
        let result = match JsFuture::from(job).await {
            Ok(r) => upload(&ctx, out, &r, srgb, mips),
            Err(e) => Err(e),
        };
        let (w, h) = result.unwrap_or_else(|e| {
            let msg = e
                .dyn_ref::<js_sys::Error>()
                .map(|e| String::from(e.message()))
                .or_else(|| e.as_string())
                .unwrap_or_else(|| format!("{e:?}"));
            console::error(&format!("[rswebgl-gltf] KTX2: {msg}"));
            (0, 0)
        });
        let _ = on_load.call2(&JsValue::NULL, &w.into(), &h.into());
    });
}

fn number(v: &JsValue, k: &str) -> Result<i32, JsValue> {
    Ok(Reflect::get(v, &k.into())?.as_f64().unwrap_or(0.0) as i32)
}

// The worker's levels into `out`.
fn upload(
    ctx: &Context,
    out: TextureId,
    r: &JsValue,
    srgb: bool,
    mips: bool,
) -> Result<(i32, i32), JsValue> {
    if ctx.is_lost() {
        return Err("the context was lost".into());
    }
    let (width, height) = (number(r, "width")?, number(r, "height")?);
    let tf = number(r, "format")? as u32;
    let levels: js_sys::Array = Reflect::get(r, &"levels".into())?.dyn_into()?;
    let count = levels.length() as i32;
    let compressed = gl_format(tf, srgb).map(CompressedFormat::from_internal);
    let generate = compressed.is_none() && mips && count == 1;
    let storage_levels = if generate {
        32 - (width.max(height) as u32).leading_zeros() as i32
    } else {
        count
    };
    let rgba = if srgb {
        TextureFormat::srgb8_alpha8()
    } else {
        TextureFormat::rgba8()
    };
    match &compressed {
        Some(f) => ctx.texture_compressed_storage_2d(out, storage_levels, f, width, height),
        None => ctx.texture_storage_2d(out, storage_levels, &rgba, width, height),
    }
    for (mip, level) in levels.iter().enumerate() {
        let (w, h) = (number(&level, "width")?, number(&level, "height")?);
        let data: Uint8Array = Reflect::get(&level, &"data".into())?.dyn_into()?;
        let data = data.to_vec();
        let mip = mip as i32;
        match &compressed {
            Some(f) => ctx.texture_compressed_sub_image_2d(out, mip, 0, 0, w, h, f, &data),
            None => ctx.texture_sub_image_2d(out, mip, 0, 0, w, h, &rgba, &data),
        }
    }
    if generate {
        ctx.texture_generate_mipmaps(out);
    }
    Ok((width, height))
}
