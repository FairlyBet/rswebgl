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
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::error::Error;
use rswebgl::ids::{SamplerId, TextureId};
use rswebgl::sampler::SamplerDesc;
use rswebgl::texture::TextureTarget;
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
) -> Result<TextureSet, Error> {
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
    let mut textures: Vec<TextureId> = Vec::with_capacity(tex_count);

    // 0. Sampling lives in sampler objects, one per distinct glTF sampler, so
    // textures only differ by image and colour space.
    let (doc_samplers, samplers) = build_samplers(ctx, &docs);

    // 1. Every texture not consumed by an ORM pack: load normally — once per
    // (image, colour space); textures that repeat one share it. It gets
    // mipmaps if any texture over it samples with a mipmap filter.
    let mut needs_mips: HashMap<TextureKey, bool> = HashMap::new();
    for (i, t) in docs.iter().enumerate() {
        if !consumed.contains(&i) {
            let mips = min_filter(t.sampler().min_filter()).1;
            *needs_mips
                .entry(texture_key(t, srgb.contains(&i)))
                .or_default() |= mips;
        }
    }
    let mut seen: HashMap<TextureKey, i32> = HashMap::new();
    for (i, t) in docs.iter().enumerate() {
        if consumed.contains(&i) {
            continue;
        }
        let key = texture_key(t, srgb.contains(&i));
        if let Some(&j) = seen.get(&key) {
            remap[i] = j;
            continue;
        }
        let tex = build_texture(
            ctx,
            t,
            buffers,
            base,
            srgb.contains(&i),
            needs_mips[&key],
            empty_callback(),
        )?;
        remap[i] = textures.len() as i32;
        seen.insert(key, remap[i]);
        textures.push(tex);
    }

    // 2. One packed ORM texture per accepted pair; both source indices point here.
    if let Some(packer) = &packer {
        for &(occ, mr) in &accepted {
            let output = build_packed_orm(ctx, &docs, buffers, base, occ, mr, packer.clone())?;
            let p = textures.len() as i32;
            remap[occ] = p;
            remap[mr] = p;
            textures.push(output);
        }
    }

    Ok(TextureSet {
        textures,
        remap,
        doc_samplers,
        samplers,
    })
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
    ctx: Context,
    remaining: u32,
    w: i32,
    h: i32,
    occ: TextureId,
    mr: TextureId,
    // Whether each source decoded: a failed one is replaced by a constant.
    occ_ok: bool,
    mr_ok: bool,
    output: TextureId,
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
) -> Result<TextureId, Error> {
    // The output adopts the MR texture's sampler (wrap/filter); ORM is linear data.
    let mr_doc = &docs[mr_idx];
    let sampler = mr_doc.sampler();
    let (min, _) = min_filter(sampler.min_filter());
    let mag = mag_filter(sampler.mag_filter());
    let output = ctx.create_texture(TextureTarget::Texture2D, min, mag)?;
    ctx.texture_set_wrap_s(output, wrap(sampler.wrap_s()));
    ctx.texture_set_wrap_t(output, wrap(sampler.wrap_t()));

    let join = Rc::new(RefCell::new(OrmJoin {
        ctx: ctx.clone(),
        remaining: 2,
        w: 0,
        h: 0,
        occ: TextureId::NULL,
        mr: TextureId::NULL,
        occ_ok: false,
        mr_ok: false,
        output,
    }));

    // Sources are linear (occlusion/MR are data); load them as transient inputs.
    let occ_src = build_texture(
        ctx,
        &docs[occ_idx],
        buffers,
        base,
        false,
        false,
        source_callback(&join, &packer, Source::Occlusion),
    )?;
    let mr_src = build_texture(
        ctx,
        mr_doc,
        buffers,
        base,
        false,
        false,
        source_callback(&join, &packer, Source::MetallicRoughness),
    )?;

    // onload is always async, so the join's sources are in place before either
    // callback fires. The join (and thus the sources) stays alive via the
    // callbacks until both have run.
    {
        let mut j = join.borrow_mut();
        j.occ = occ_src;
        j.mr = mr_src;
    }

    Ok(output)
}

// Which source of an ORM pack a load callback is for.
#[derive(Clone, Copy)]
enum Source {
    Occlusion,
    MetallicRoughness,
}

/// A one-shot `on_load(width, height)` callback that advances the ORM join.
fn source_callback(
    join: &Rc<RefCell<OrmJoin>>,
    packer: &Rc<TexturePacker>,
    source: Source,
) -> js_sys::Function {
    let join = join.clone();
    let packer = packer.clone();
    Closure::once_into_js(move |w: f64, h: f64| {
        on_source_loaded(&join, &packer, source, w as i32, h as i32);
    })
    .unchecked_into()
}

fn on_source_loaded(
    join: &Rc<RefCell<OrmJoin>>,
    packer: &TexturePacker,
    source: Source,
    w: i32,
    h: i32,
) {
    let mut j = join.borrow_mut();
    if w > 0 && h > 0 {
        match source {
            Source::Occlusion => j.occ_ok = true,
            Source::MetallicRoughness => j.mr_ok = true,
        }
        // Pack at the larger of the two source sizes; the GPU resamples the rest.
        j.w = j.w.max(w);
        j.h = j.h.max(h);
    }
    j.remaining -= 1;
    if j.remaining != 0 {
        return;
    }

    let (ctx, occ, mr, w, h, output) = (j.ctx.clone(), j.occ, j.mr, j.w, j.h, j.output);
    let (occ_ok, mr_ok) = (j.occ_ok, j.mr_ok);
    drop(j); // release the borrow before the GPU work

    // The sources are transient: only the packed output is kept.
    // (Unless already gone with a lost context: nothing to free, nothing to
    // report.)
    let free_sources = || {
        for t in [occ, mr] {
            if ctx.texture_is_valid(t) {
                ctx.destroy_texture(t);
            }
        }
    };

    // The model was destroyed while the sources loaded: nothing to fill.
    if !ctx.texture_is_valid(output) {
        free_sources();
        return;
    }
    if w <= 0 || h <= 0 {
        console::error("[rswebgl-gltf] ORM sources failed to load; packed texture left empty");
        free_sources();
        return;
    }

    // A source that failed reads as 1 — what glTF means by "no texture" for
    // occlusion and for the roughness / metallic factors' multiplier — rather
    // than as an empty texture (black: fully occluded, or a smooth dielectric).
    let mut spec = PackSpec::new();
    if occ_ok {
        spec.set_source(Channel::R, occ, Channel::R); // occlusion -> R
    } else {
        console::error("[rswebgl-gltf] occlusion map failed to load; packed as no occlusion");
        spec.set_constant(Channel::R, 1.0);
    }
    if mr_ok {
        spec.set_source(Channel::G, mr, Channel::G); // roughness  -> G
        spec.set_source(Channel::B, mr, Channel::B); // metallic   -> B
    } else {
        console::error(
            "[rswebgl-gltf] metallic-roughness map failed to load; packed as the factors alone",
        );
        spec.set_constant(Channel::G, 1.0);
        spec.set_constant(Channel::B, 1.0);
    }
    spec.set_constant(Channel::A, 1.0);

    if let Err(e) = packer.pack_into(output, &spec, w, h, false, true) {
        console::error(&format!("[rswebgl-gltf] ORM pack failed: {e}"));
    }
    free_sources();
}

/// A nullary image-load callback for fire-and-forget textures.
fn empty_callback() -> js_sys::Function {
    js_sys::Function::new_no_args("")
}

// What makes two glTF textures the same GPU texture: the image and whether it
// decodes as sRGB (sampling is in sampler objects).
type TextureKey = (usize, bool);

fn texture_key(t: &gltf::Texture, srgb: bool) -> TextureKey {
    (t.source().index(), srgb)
}

/// The final texture list and how the document maps onto it.
pub struct TextureSet {
    pub textures: Vec<TextureId>,
    /// Document texture index → `textures` index (-1: none).
    pub remap: Vec<i32>,
    /// Document texture index → the sampler to read it through.
    pub doc_samplers: Vec<SamplerId>,
    /// Every distinct sampler created (to destroy with the model).
    pub samplers: Vec<SamplerId>,
}

// One sampler per distinct glTF sampler (min, mag, wrap S/T); returns the
// sampler of each document texture and the distinct list.
fn build_samplers(ctx: &Context, docs: &[gltf::Texture]) -> (Vec<SamplerId>, Vec<SamplerId>) {
    let mut by_desc: Vec<(SamplerDesc, SamplerId)> = Vec::new();
    let per_doc = docs
        .iter()
        .map(|t| {
            let s = t.sampler();
            let mut desc = SamplerDesc::new();
            desc.min_filter = min_filter(s.min_filter()).0;
            desc.mag_filter = mag_filter(s.mag_filter());
            desc.wrap_s = wrap(s.wrap_s());
            desc.wrap_t = wrap(s.wrap_t());
            if let Some((_, id)) = by_desc.iter().find(|(d, _)| *d == desc) {
                return *id;
            }
            match ctx.create_sampler(&desc) {
                Ok(id) => {
                    by_desc.push((desc, id));
                    id
                }
                Err(e) => {
                    console::error(&format!("[rswebgl-gltf] sampler: {e}"));
                    SamplerId::NULL
                }
            }
        })
        .collect();
    (per_doc, by_desc.into_iter().map(|(_, id)| id).collect())
}
