//! Assemble a glTF document's textures into the final `Model` texture list,
//! packing separate occlusion + metallic-roughness maps into one **ORM** texture
//! (`R = occlusion, G = roughness, B = metallic`) where it's safe to do so.
//!
//! Why: glTF samples occlusion from a texture's R and metallic-roughness from
//! another's G/B. Authoring tools often already share one image (then nothing to
//! do — we keep the single texture). When they're *separate*, combining them into
//! one texture means a material samples one texture/one unit instead of two, and
//! we hold one GPU texture instead of two.
//!
//! Packing is conservative — it only fires when a texture is used *exclusively*
//! as occlusion (resp. metallic-roughness) and the occlusion↔MR pairing is 1:1.
//! Anything ambiguous is left unpacked (still correct, just not combined).
//!
//! The combine runs on the GPU via [`rswebgl::texture_packer`] once both source
//! images have decoded (an async join on their load callbacks). The packed output
//! handle is created up front and returned in the texture list immediately; it
//! fills in when the join completes, like every other async texture.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::texture::{Texture, TextureMagFilter, TextureMinFilter, TextureTarget};
use rswebgl::texture_packer::{Channel, PackSpec, TexturePacker};

use crate::textures::{build_texture, mag_filter, min_filter, srgb_texture_set, wrap};

/// Build the final, index-remapped texture list for `doc`.
///
/// Returns the textures plus a `remap` from document texture index to final
/// `Model` texture index (occlusion and MR of a packed pair both map to the one
/// ORM texture). Apply `remap` to every material's texture indices
/// (`material::remap_textures`).
pub fn build_texture_set(
    ctx: &Context,
    doc: &gltf::Document,
    buffers: &[Vec<u8>],
    base: &str,
) -> (Vec<Texture>, Vec<i32>) {
    let docs: Vec<gltf::Texture> = doc.textures().collect();
    let tex_count = docs.len();

    let mut accepted = plan_orm_pairs(doc, tex_count);

    // ORM packing needs the packer's program; if it won't compile, fall back to
    // leaving every texture unpacked (still correct).
    let mut packer: Option<Rc<TexturePacker>> = None;
    if !accepted.is_empty() {
        match TexturePacker::new(ctx) {
            Ok(p) => packer = Some(Rc::new(p)),
            Err(e) => {
                console::error(&format!(
                    "[rswebgl-gltf] ORM packer unavailable ({e}); textures left unpacked"
                ));
                accepted.clear();
            }
        }
    }

    let consumed: HashSet<usize> = accepted.iter().flat_map(|&(o, m)| [o, m]).collect();
    let srgb = srgb_texture_set(doc);

    let mut remap = vec![-1i32; tex_count];
    let mut textures: Vec<Texture> = Vec::with_capacity(tex_count);

    // 1. Every texture not consumed by an ORM pack: load normally.
    for (i, t) in docs.iter().enumerate() {
        if consumed.contains(&i) {
            continue;
        }
        let tex = build_texture(ctx, t, buffers, base, srgb.contains(&i), empty_callback());
        remap[i] = textures.len() as i32;
        textures.push(tex);
    }

    // 2. One packed ORM texture per accepted pair; both source indices point here.
    if let Some(packer) = &packer {
        for &(occ, mr) in &accepted {
            let output = build_packed_orm(ctx, &docs, buffers, base, occ, mr, packer.clone());
            let p = textures.len() as i32;
            remap[occ] = p;
            remap[mr] = p;
            textures.push(output);
        }
    }

    (textures, remap)
}

/// Which roles a texture is used in across all materials.
#[derive(Clone, Copy, Default)]
struct Roles {
    base: bool,
    normal: bool,
    emissive: bool,
    occ: bool,
    mr: bool,
}

/// Decide which (occlusion, metallic-roughness) index pairs are safe to pack.
fn plan_orm_pairs(doc: &gltf::Document, tex_count: usize) -> Vec<(usize, usize)> {
    let mut roles = vec![Roles::default(); tex_count];
    let mut candidates: Vec<(usize, usize)> = Vec::new();

    for mat in doc.materials() {
        let pbr = mat.pbr_metallic_roughness();
        let bc = pbr.base_color_texture().map(|i| i.texture().index());
        let mr = pbr
            .metallic_roughness_texture()
            .map(|i| i.texture().index());
        let nm = mat.normal_texture().map(|i| i.texture().index());
        let oc = mat.occlusion_texture().map(|i| i.texture().index());
        let em = mat.emissive_texture().map(|i| i.texture().index());

        if let Some(i) = bc {
            roles[i].base = true;
        }
        if let Some(i) = mr {
            roles[i].mr = true;
        }
        if let Some(i) = nm {
            roles[i].normal = true;
        }
        if let Some(i) = oc {
            roles[i].occ = true;
        }
        if let Some(i) = em {
            roles[i].emissive = true;
        }
        if let (Some(o), Some(m)) = (oc, mr)
            && o != m
        {
            candidates.push((o, m));
        }
    }

    // A texture must be used *only* as occlusion (resp. only as MR) to be packed,
    // so repointing it doesn't disturb another role. And the occ↔MR pairing must
    // be 1:1: if either side ever pairs differently, reject both.
    let occ_only = |r: &Roles| r.occ && !r.base && !r.normal && !r.emissive && !r.mr;
    let mr_only = |r: &Roles| r.mr && !r.base && !r.normal && !r.emissive && !r.occ;

    let mut rejected: HashSet<usize> = HashSet::new();
    for &(o, m) in &candidates {
        if !occ_only(&roles[o]) || !mr_only(&roles[m]) {
            rejected.insert(o);
            rejected.insert(m);
        }
    }
    // Inconsistent pairings: an occ paired with two MRs (or vice versa).
    for (idx_a, &(o, m)) in candidates.iter().enumerate() {
        for &(o2, m2) in &candidates[idx_a + 1..] {
            if (o == o2) != (m == m2) {
                rejected.insert(o);
                rejected.insert(m);
                rejected.insert(o2);
                rejected.insert(m2);
            }
        }
    }

    let mut accepted: Vec<(usize, usize)> = Vec::new();
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for &(o, m) in &candidates {
        if rejected.contains(&o) || rejected.contains(&m) {
            continue;
        }
        if seen.insert((o, m)) {
            accepted.push((o, m));
        }
    }
    accepted
}

/// State shared between the two source-load callbacks of one ORM pack.
struct OrmJoin {
    remaining: u32,
    w: i32,
    h: i32,
    occ: Option<Texture>,
    mr: Option<Texture>,
    output: Texture,
}

/// Create the (empty) ORM output texture and kick the two source loads; the pack
/// runs once both have decoded.
fn build_packed_orm(
    ctx: &Context,
    docs: &[gltf::Texture],
    buffers: &[Vec<u8>],
    base: &str,
    occ_idx: usize,
    mr_idx: usize,
    packer: Rc<TexturePacker>,
) -> Texture {
    // The output adopts the MR texture's sampler (wrap/filter); ORM is linear data.
    let mr_doc = &docs[mr_idx];
    let sampler = mr_doc.sampler();
    let (min, _) = min_filter(sampler.min_filter());
    let mag = mag_filter(sampler.mag_filter());
    let output = ctx
        .create_texture(TextureTarget::Texture2D, min, mag)
        .expect("create_texture");
    output.set_wrap_s(wrap(sampler.wrap_s()));
    output.set_wrap_t(wrap(sampler.wrap_t()));

    let join = Rc::new(RefCell::new(OrmJoin {
        remaining: 2,
        w: 0,
        h: 0,
        occ: None,
        mr: None,
        output: output.clone(),
    }));

    // Sources are linear (occlusion/MR are data); load them as transient inputs.
    let occ_src = build_texture(
        ctx,
        &docs[occ_idx],
        buffers,
        base,
        false,
        source_callback(&join, &packer),
    );
    let mr_src = build_texture(
        ctx,
        mr_doc,
        buffers,
        base,
        false,
        source_callback(&join, &packer),
    );

    // onload is always async, so the join's sources are in place before either
    // callback fires. The join (and thus the sources) stays alive via the
    // callbacks until both have run.
    {
        let mut j = join.borrow_mut();
        j.occ = Some(occ_src);
        j.mr = Some(mr_src);
    }

    output
}

/// A one-shot `on_load(width, height)` callback that advances the ORM join.
fn source_callback(join: &Rc<RefCell<OrmJoin>>, packer: &Rc<TexturePacker>) -> js_sys::Function {
    let join = join.clone();
    let packer = packer.clone();
    Closure::once_into_js(move |w: f64, h: f64| {
        on_source_loaded(&join, &packer, w as i32, h as i32);
    })
    .unchecked_into()
}

fn on_source_loaded(join: &Rc<RefCell<OrmJoin>>, packer: &TexturePacker, w: i32, h: i32) {
    let mut j = join.borrow_mut();
    if w > 0 && h > 0 {
        // Pack at the larger of the two source sizes; the GPU resamples the rest.
        j.w = j.w.max(w);
        j.h = j.h.max(h);
    }
    j.remaining -= 1;
    if j.remaining != 0 {
        return;
    }

    let (Some(occ), Some(mr), w, h, output) =
        (j.occ.clone(), j.mr.clone(), j.w, j.h, j.output.clone())
    else {
        console::error("[rswebgl-gltf] ORM join missing a source texture");
        return;
    };
    drop(j); // release the borrow before the GPU work

    if w <= 0 || h <= 0 {
        console::error("[rswebgl-gltf] ORM sources failed to load; packed texture left empty");
        return;
    }

    // These sources are transient — packed once here, then dropped — so forcing
    // NEAREST is free and gives an exact texel copy when the output matches the
    // source size (the common case; we never minify, since the output is the
    // larger of the two). Revisit once sampler objects let the blit override
    // filtering without mutating the texture (PROGRESS: "Sampler object ❌").
    for t in [&occ, &mr] {
        t.set_min_filter(TextureMinFilter::Nearest);
        t.set_mag_filter(TextureMagFilter::Nearest);
    }

    let mut spec = PackSpec::new();
    spec.set_source(Channel::R, &occ, Channel::R); // occlusion -> R
    spec.set_source(Channel::G, &mr, Channel::G); // roughness  -> G
    spec.set_source(Channel::B, &mr, Channel::B); // metallic   -> B
    spec.set_constant(Channel::A, 1.0);

    if let Err(e) = packer.pack_into(&output, &spec, w, h, false, true) {
        console::error(&format!("[rswebgl-gltf] ORM pack failed: {e}"));
    }
}

/// A nullary image-load callback for fire-and-forget textures.
fn empty_callback() -> js_sys::Function {
    js_sys::Function::new_no_args("")
}
