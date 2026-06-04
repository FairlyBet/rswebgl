//! Create one GPU texture per glTF `texture` and fill it asynchronously.
//!
//! Each texture is created empty with its sampler parameters set immediately;
//! the image content arrives later through the browser's image decoder (rswebgl
//! `Texture::load_image` / `load_bytes`), which sizes storage, uploads, and
//! generates mipmaps on `onload`. We deliberately don't await the images — the
//! model is usable right away and textures pop in as they decode.

use std::collections::HashSet;

use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::texture::{
    Texture, TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget, TextureWrap,
};

use crate::buffers::{data_uri_mime, decode_data_uri};
use crate::fetch::resolve_url;

/// One `Texture` per `document.textures()`, index-aligned.
pub fn build_textures(
    ctx: &Context,
    doc: &gltf::Document,
    buffers: &[Vec<u8>],
    base: &str,
) -> Vec<Texture> {
    let srgb = srgb_texture_set(doc);
    doc.textures()
        .map(|t| build_texture(ctx, &t, buffers, base, srgb.contains(&t.index())))
        .collect()
}

fn build_texture(
    ctx: &Context,
    tex: &gltf::Texture,
    buffers: &[Vec<u8>],
    base: &str,
    srgb: bool,
) -> Texture {
    let sampler = tex.sampler();
    let (min, generate_mipmaps) = min_filter(sampler.min_filter());
    let mag = mag_filter(sampler.mag_filter());

    // create_texture only fails on a lost context, in which case the whole load
    // is doomed; index alignment with document.textures() must be preserved.
    let out = ctx
        .create_texture(TextureTarget::Texture2D, min, mag)
        .expect("create_texture");
    out.set_wrap_s(wrap(sampler.wrap_s()));
    out.set_wrap_t(wrap(sampler.wrap_t()));

    // baseColor/emissive are authored in sRGB; data textures stay linear.
    let format = if srgb {
        TextureFormat::srgb8_alpha8()
    } else {
        TextureFormat::rgba8()
    };

    // glTF texture coordinates have their origin at the top-left, matching the
    // browser image's natural orientation and WebGL's default unpack — no flip.
    let flip_y = false;
    let on_load = js_sys::Function::new_no_args("");

    match tex.source().source() {
        gltf::image::Source::Uri { uri, mime_type } => {
            if let Some(decoded) = decode_data_uri(uri) {
                match decoded {
                    Ok(bytes) => {
                        let mime = mime_type
                            .map(str::to_string)
                            .or_else(|| data_uri_mime(uri))
                            .unwrap_or_else(|| "image/png".to_string());
                        out.load_bytes(&bytes, &mime, &format, generate_mipmaps, flip_y, on_load);
                    }
                    Err(e) => console::error(&format!("[rswebgl-gltf] image data URI: {e:?}")),
                }
            } else {
                match resolve_url(base, uri) {
                    Ok(abs) => out.load_image(&abs, &format, generate_mipmaps, flip_y, on_load),
                    Err(e) => console::error(&format!("[rswebgl-gltf] image URL: {e:?}")),
                }
            }
        }
        gltf::image::Source::View { view, mime_type } => {
            let buf = view.buffer().index();
            let start = view.offset();
            let end = start + view.length();
            match buffers.get(buf).and_then(|b| b.get(start..end)) {
                Some(data) => {
                    out.load_bytes(data, mime_type, &format, generate_mipmaps, flip_y, on_load)
                }
                None => console::error("[rswebgl-gltf] image bufferView out of range"),
            }
        }
    }

    out
}

/// Texture indices used in an sRGB role (baseColor, emissive) — those decode
/// from sRGB; every other texture (normal, metallic-roughness, occlusion) is
/// linear.
fn srgb_texture_set(doc: &gltf::Document) -> HashSet<usize> {
    let mut set = HashSet::new();
    for mat in doc.materials() {
        if let Some(info) = mat.pbr_metallic_roughness().base_color_texture() {
            set.insert(info.texture().index());
        }
        if let Some(info) = mat.emissive_texture() {
            set.insert(info.texture().index());
        }
    }
    set
}

/// glTF min filter → (`TextureMinFilter`, build-mipmaps?). `None` = "auto".
fn min_filter(f: Option<gltf::texture::MinFilter>) -> (TextureMinFilter, bool) {
    use gltf::texture::MinFilter as M;
    match f {
        Some(M::Nearest) => (TextureMinFilter::Nearest, false),
        Some(M::Linear) => (TextureMinFilter::Linear, false),
        Some(M::NearestMipmapNearest) => (TextureMinFilter::NearestMipmapNearest, true),
        Some(M::LinearMipmapNearest) => (TextureMinFilter::LinearMipmapNearest, true),
        Some(M::NearestMipmapLinear) => (TextureMinFilter::NearestMipmapLinear, true),
        Some(M::LinearMipmapLinear) => (TextureMinFilter::LinearMipmapLinear, true),
        None => (TextureMinFilter::LinearMipmapLinear, true),
    }
}

fn mag_filter(f: Option<gltf::texture::MagFilter>) -> TextureMagFilter {
    match f {
        Some(gltf::texture::MagFilter::Nearest) => TextureMagFilter::Nearest,
        _ => TextureMagFilter::Linear,
    }
}

fn wrap(w: gltf::texture::WrappingMode) -> TextureWrap {
    use gltf::texture::WrappingMode as W;
    match w {
        W::ClampToEdge => TextureWrap::ClampToEdge,
        W::MirroredRepeat => TextureWrap::MirroredRepeat,
        W::Repeat => TextureWrap::Repeat,
    }
}
