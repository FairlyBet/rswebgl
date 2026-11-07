//! Create one GPU texture per glTF `texture` and fill it asynchronously.
//!
//! Each texture is created empty with its sampler parameters set immediately;
//! the image content arrives later through the browser's image decoder (rswebgl
//! `Context::texture_load_image` / `texture_load_bytes`), which sizes storage, uploads, and
//! generates mipmaps on `onload`. We deliberately don't await the images — the
//! model is usable right away and textures pop in as they decode.
//!
//! This module builds *single* textures and exposes the sampler/format helpers;
//! the higher-level assembly (including ORM channel packing) lives in `pack.rs`.

use std::collections::HashSet;

use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::error::Error;
use rswebgl::ids::TextureId;
use rswebgl::texture::{
    TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget, TextureWrap, failed_later,
};

use crate::buffers::{data_uri_mime, decode_data_uri};
use crate::fetch::resolve_url;

/// Create one GPU texture for `tex` and kick off its async image load.
///
/// `srgb` picks the internal format (sRGB for color roles, linear for data);
/// `force_mips` builds mipmaps even if this texture's own sampler doesn't
/// need them (another texture sharing the image does).
/// `on_load` is the rswebgl image-load callback (`on_load(width, height)`); pass
/// an empty function for fire-and-forget, or a real one to join on completion
/// (used by ORM packing to learn the decoded size). It comes exactly once and
/// never synchronously — with `(0, 0)` when the image can't even be requested
/// (a bad data URI or URL, a bufferView out of range). An error only when the
/// texture itself can't be created (the context is lost).
pub(crate) fn build_texture(
    ctx: &Context,
    tex: &gltf::Texture,
    buffers: &[Vec<u8>],
    base: &str,
    srgb: bool,
    force_mips: bool,
    on_load: js_sys::Function,
) -> Result<TextureId, Error> {
    let sampler = tex.sampler();
    let (min, mips) = min_filter(sampler.min_filter());
    let generate_mipmaps = mips || force_mips;
    let mag = mag_filter(sampler.mag_filter());

    let out = ctx.create_texture(TextureTarget::Texture2D, min, mag)?;
    ctx.texture_set_wrap_s(out, wrap(sampler.wrap_s()));
    ctx.texture_set_wrap_t(out, wrap(sampler.wrap_t()));

    // baseColor/emissive are authored in sRGB; data textures stay linear.
    let format = if srgb {
        TextureFormat::srgb8_alpha8()
    } else {
        TextureFormat::rgba8()
    };

    // glTF texture coordinates have their origin at the top-left, matching the
    // browser image's natural orientation and WebGL's default unpack — no flip.
    let flip_y = false;

    match tex.source().source() {
        gltf::image::Source::Uri { uri, mime_type } => {
            if let Some(decoded) = decode_data_uri(uri) {
                match decoded {
                    Ok(bytes) => {
                        let mime = mime_type
                            .map(str::to_string)
                            .or_else(|| data_uri_mime(uri))
                            .unwrap_or_else(|| "image/png".to_string());
                        ctx.texture_load_bytes(
                            out,
                            &bytes,
                            &mime,
                            &format,
                            generate_mipmaps,
                            flip_y,
                            on_load,
                        );
                    }
                    Err(e) => {
                        console::error(&format!("[rswebgl-gltf] image data URI: {e:?}"));
                        failed_later(on_load);
                    }
                }
            } else {
                match resolve_url(base, uri) {
                    Ok(abs) => ctx.texture_load_image(
                        out,
                        &abs,
                        &format,
                        generate_mipmaps,
                        flip_y,
                        on_load,
                    ),
                    Err(e) => {
                        console::error(&format!("[rswebgl-gltf] image URL: {e:?}"));
                        failed_later(on_load);
                    }
                }
            }
        }
        gltf::image::Source::View { view, mime_type } => {
            let buf = view.buffer().index();
            let start = view.offset();
            let end = start + view.length();
            match buffers.get(buf).and_then(|b| b.get(start..end)) {
                Some(data) => ctx.texture_load_bytes(
                    out,
                    data,
                    mime_type,
                    &format,
                    generate_mipmaps,
                    flip_y,
                    on_load,
                ),
                None => {
                    console::error("[rswebgl-gltf] image bufferView out of range");
                    failed_later(on_load);
                }
            }
        }
    }

    Ok(out)
}

/// Texture indices used in an sRGB role (baseColor, emissive) — those decode
/// from sRGB; every other texture (normal, metallic-roughness, occlusion) is
/// linear.
pub(crate) fn srgb_texture_set(doc: &gltf::Document) -> HashSet<usize> {
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
pub(crate) fn min_filter(f: Option<gltf::texture::MinFilter>) -> (TextureMinFilter, bool) {
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

pub(crate) fn mag_filter(f: Option<gltf::texture::MagFilter>) -> TextureMagFilter {
    match f {
        Some(gltf::texture::MagFilter::Nearest) => TextureMagFilter::Nearest,
        _ => TextureMagFilter::Linear,
    }
}

pub(crate) fn wrap(w: gltf::texture::WrappingMode) -> TextureWrap {
    use gltf::texture::WrappingMode as W;
    match w {
        W::ClampToEdge => TextureWrap::ClampToEdge,
        W::MirroredRepeat => TextureWrap::MirroredRepeat,
        W::Repeat => TextureWrap::Repeat,
    }
}
