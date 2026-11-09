//! The built-in glTF metallic-roughness shader (`shaders/pbr.{vert,frag}`) and
//! its variants: one program per combination of the primitive's attributes and
//! the material's maps, alpha mode and shading model, compiled on first use.

use std::collections::HashMap;
use std::fmt::Write as _;

use wasm_bindgen::prelude::*;

use crate::shader::shader_sources;
use rswebgl::context::Context;
use rswebgl::error::Error;
use rswebgl::ids::ProgramId;
use rswebgl::shader_source::ShaderSource;
use rswebgl_gltf::{
    ATTR_COLOR, ATTR_MORPH_NORMAL, ATTR_MORPH_POSITION, ATTR_MORPH_TANGENT, ATTR_NORMAL, ATTR_SKIN,
    ATTR_SKIN8, ATTR_TANGENT, ATTR_UV0, ATTR_UV1, AlphaMode, DrawItem, MAP_COUNT, MAP_SLOTS,
    Material, Model,
};

const VERT: &str = include_str!("../shaders/pbr.vert");
const FRAG: &str = include_str!("../shaders/pbr.frag");

// Key layout (u64): bits 0..5 the primitive's ATTR_* bits; 5..7 the alpha
// mode; 7 unlit; 8..12 transmission, volume, clearcoat, dispersion; 12 the
// glTF reference model; 13 EON diffuse; 14..17 sheen, thin film,
// anisotropy; 17.. which maps the material has (`MAP_SLOTS` order), then
// which of them read TEXCOORD_1, KHR_texture_transform, and the features
// after it (deformers, subsurface, eight joints, diffuse transmission).
const ATTRS: [(u64, &str); 5] = [
    (ATTR_NORMAL as u64, "HAS_NORMAL"),
    (ATTR_UV0 as u64, "HAS_UV0"),
    (ATTR_TANGENT as u64, "HAS_TANGENT"),
    (ATTR_COLOR as u64, "HAS_COLOR"),
    (ATTR_UV1 as u64, "HAS_UV1"),
];
const ALPHA_SHIFT: u64 = 5;
const UNLIT: u64 = 1 << 7;
const TRANSMISSION: u64 = 1 << 8;
const VOLUME: u64 = 1 << 9;
const CLEARCOAT: u64 = 1 << 10;
const DISPERSION: u64 = 1 << 11;
// The shading model: OpenPBR unless set (the glTF reference).
const MODEL_GLTF: u64 = 1 << 12;
// EON diffuse (base_diffuse_roughness > 0); Lambert otherwise.
const DIFFUSE_ROUGHNESS: u64 = 1 << 13;
// Layers: glTF sheen / OpenPBR fuzz, thin film, specular anisotropy.
const SHEEN: u64 = 1 << 14;
const IRIDESCENCE: u64 = 1 << 15;
const ANISOTROPY: u64 = 1 << 16;
const MAP_SHIFT: u64 = 17;
const UV1_SHIFT: u64 = MAP_SHIFT + MAP_COUNT as u64;
// KHR_texture_transform on any map in use: its `_UV` goes through a matrix.
const TEXTURE_TRANSFORM: u64 = 1 << (UV1_SHIFT + MAP_COUNT as u64);
// Geometry the vertex stage deforms: skinning (JOINTS_0 / WEIGHTS_0) and
// morph targets with position / normal / tangent deltas.
const SKIN: u64 = TEXTURE_TRANSFORM << 1;
const MORPH_POSITION: u64 = TEXTURE_TRANSFORM << 2;
const MORPH_NORMAL: u64 = TEXTURE_TRANSFORM << 3;
const MORPH_TANGENT: u64 = TEXTURE_TRANSFORM << 4;
const MORPH: u64 = MORPH_POSITION | MORPH_NORMAL | MORPH_TANGENT;
// OpenPBR subsurface (subsurface_weight > 0).
const SUBSURFACE: u64 = TEXTURE_TRANSFORM << 5;
// Eight joint influences (JOINTS_1 / WEIGHTS_1 as well).
const SKIN8: u64 = TEXTURE_TRANSFORM << 6;
// KHR_materials_diffuse_transmission.
const DIFFUSE_TRANSMISSION: u64 = TEXTURE_TRANSFORM << 7;
const TOP_BIT: u64 = DIFFUSE_TRANSMISSION;
// Feature defines, by bit.
const FEATURES: [(u64, &str); 18] = [
    (DIFFUSE_TRANSMISSION, "HAS_DIFFUSE_TRANSMISSION"),
    (SKIN8, "HAS_SKIN8"),
    (SUBSURFACE, "HAS_SUBSURFACE"),
    (TEXTURE_TRANSFORM, "HAS_TEXTURE_TRANSFORM"),
    (SKIN, "HAS_SKIN"),
    (MORPH_POSITION, "HAS_MORPH_POSITION"),
    (MORPH_NORMAL, "HAS_MORPH_NORMAL"),
    (MORPH_TANGENT, "HAS_MORPH_TANGENT"),
    (UNLIT, "MATERIAL_UNLIT"),
    (TRANSMISSION, "HAS_TRANSMISSION"),
    (VOLUME, "HAS_VOLUME"),
    (CLEARCOAT, "HAS_CLEARCOAT"),
    (DISPERSION, "HAS_DISPERSION"),
    (DIFFUSE_ROUGHNESS, "HAS_DIFFUSE_ROUGHNESS"),
    (SHEEN, "HAS_SHEEN"),
    (IRIDESCENCE, "HAS_IRIDESCENCE"),
    (ANISOTROPY, "HAS_ANISOTROPY"),
    (MODEL_GLTF, "MODEL_GLTF"),
];

// The layer a map belongs to (0: always used): an extension's maps only
// count while its layer is on, so a texture without its factor costs nothing.
// Samplers every scene shader binds: environment diffuse, specular, the BRDF
// table, the scene copy, the peeled depth.
const FRAME_SAMPLERS: u32 = 5;

// Maps left out first when a variant would need more texture units than
// the device has: the subtlest effects first, the base color last.
const DROP_ORDER: [usize; MAP_COUNT] = [
    16, 15, 14, 13, 18, 7, 6, 10, 11, 17, 9, 12, 5, 8, 3, 4, 1, 2, 0,
];

const MAP_LAYERS: [u64; MAP_COUNT] = [
    0,
    0,
    0,
    0,
    0, // core
    CLEARCOAT,
    CLEARCOAT,
    CLEARCOAT, // clearcoat, its roughness, normal
    TRANSMISSION,
    VOLUME, // transmission, thickness
    0,
    0, // specular, specular color
    SHEEN,
    SHEEN, // sheen color, roughness
    IRIDESCENCE,
    IRIDESCENCE, // iridescence, its thickness
    ANISOTROPY,  // anisotropy
    DIFFUSE_TRANSMISSION,
    DIFFUSE_TRANSMISSION, // diffuse transmission, its color
];

/// A shader variant: which features one program is compiled with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PbrKey(u64);

impl PbrKey {
    /// The variant for a primitive with `attributes` (`ATTR_*` bits) drawn
    /// with `material`.
    pub fn new(attributes: u32, material: &Material) -> Self {
        let mut key = (attributes & 0x1f) as u64;
        for (attr, bit) in [
            (ATTR_SKIN, SKIN),
            (ATTR_SKIN8, SKIN8),
            (ATTR_MORPH_POSITION, MORPH_POSITION),
            (ATTR_MORPH_NORMAL, MORPH_NORMAL),
            (ATTR_MORPH_TANGENT, MORPH_TANGENT),
        ] {
            if attributes & attr != 0 {
                key |= bit;
            }
        }
        key |= (material.alpha_mode() as u64) << ALPHA_SHIFT;
        if material.unlit() {
            key |= UNLIT;
        }
        // Volume and dispersion only matter for light going through (the
        // volume also for diffuse transmission, which it attenuates).
        let transmissive = material.transmission() > 0.0;
        let diffuse_through = material.diffuse_transmission() > 0.0;
        let volume = (transmissive || diffuse_through) && material.thickness() > 0.0;
        let present = [
            (TRANSMISSION, transmissive),
            (VOLUME, volume),
            (CLEARCOAT, material.clearcoat() > 0.0),
            (
                DISPERSION,
                transmissive && volume && material.dispersion() > 0.0,
            ),
            (DIFFUSE_ROUGHNESS, material.diffuse_roughness() > 0.0),
            (SHEEN, material.sheen_color().iter().any(|&c| c > 0.0)),
            (IRIDESCENCE, material.iridescence() > 0.0),
            (ANISOTROPY, material.anisotropy_strength() != 0.0),
            // Through the base only: not under a full transmission.
            (SUBSURFACE, material.subsurface_weight() > 0.0),
            (DIFFUSE_TRANSMISSION, material.diffuse_transmission() > 0.0),
        ];
        for (bit, on) in present {
            if on {
                key |= bit;
            }
        }
        for (i, (tex, uv)) in material.maps().into_iter().enumerate() {
            let layer = MAP_LAYERS[i];
            if tex >= 0 && (layer == 0 || key & layer != 0) {
                key |= 1 << (MAP_SHIFT + i as u64);
                if uv == 1 {
                    key |= 1 << (UV1_SHIFT + i as u64);
                }
            }
        }
        if material.has_uv_transform() {
            key |= TEXTURE_TRANSFORM;
        }
        Self(key).fit_texture_units()
    }

    /// This variant with the least important maps dropped until its samplers
    /// fit the device (`MAX_TEXTURE_IMAGE_UNITS`, the frame's own four
    /// included): a material using every extension map would otherwise fail
    /// to link on 16-unit devices. Reported once.
    pub fn fit_texture_units(self) -> Self {
        let units = rswebgl::limits::max_texture_image_units();
        let (fit, dropped) = self.within_texture_units(units);
        for slot in dropped {
            rswebgl::console::warn_once(&format!(
                "[rswebgl-scene] a material uses more maps than this device can sample \
                 ({units} texture units); its {} is left out",
                MAP_SLOTS[slot].name
            ));
        }
        fit
    }

    // `fit_texture_units` for `units`, and the map slots it dropped.
    fn within_texture_units(self, units: u32) -> (Self, Vec<usize>) {
        let mut key = self.0;
        let used =
            |k: u64| ((k >> MAP_SHIFT) & ((1 << MAP_COUNT) - 1)).count_ones() + FRAME_SAMPLERS;
        let mut dropped = Vec::new();
        for slot in DROP_ORDER {
            if used(key) <= units {
                break;
            }
            let bit = 1 << (MAP_SHIFT + slot as u64);
            if key & bit != 0 {
                key &= !(bit | 1 << (UV1_SHIFT + slot as u64));
                dropped.push(slot);
            }
        }
        (Self(key), dropped)
    }

    /// A key from raw bits (see the layout above). For tools and tests;
    /// models go through `new`.
    pub fn from_bits(bits: u64) -> Self {
        Self(bits & ((TOP_BIT << 1) - 1))
    }

    pub fn bits(&self) -> u64 {
        self.0
    }

    /// This variant shaded by `model`.
    pub fn with_model(self, model: ShadingModel) -> Self {
        match model {
            ShadingModel::OpenPbr => Self(self.0 & !MODEL_GLTF),
            ShadingModel::Gltf => Self(self.0 | MODEL_GLTF),
        }
    }

    /// The `#define` lines for this variant.
    pub fn defines(&self) -> String {
        let k = self.0;
        let mut s = String::new();
        for (bit, name) in ATTRS {
            if k & bit != 0 {
                let _ = writeln!(s, "#define {name}");
            }
        }
        for (i, slot) in MAP_SLOTS.iter().enumerate() {
            if k & (1 << (MAP_SHIFT + i as u64)) != 0 {
                let uv = if k & (1 << (UV1_SHIFT + i as u64)) != 0 {
                    "v_uv1"
                } else {
                    "v_uv0"
                };
                let name = slot.define;
                let _ = writeln!(s, "#define HAS_{name}_MAP");
                if k & TEXTURE_TRANSFORM != 0 {
                    let m = slot.transform;
                    let _ = writeln!(s, "#define {name}_UV ({m} * vec3({uv}, 1.0)).xy");
                } else {
                    let _ = writeln!(s, "#define {name}_UV {uv}");
                }
            }
        }
        let alpha = match (k >> ALPHA_SHIFT) & 3 {
            1 => "MASK",
            2 => "BLEND",
            _ => "OPAQUE",
        };
        let _ = writeln!(s, "#define ALPHAMODE_{alpha}");
        for (bit, name) in FEATURES {
            if k & bit != 0 {
                let _ = writeln!(s, "#define {name}");
            }
        }
        if k & MORPH != 0 {
            let _ = writeln!(s, "#define HAS_MORPH");
            let kinds = (k & MORPH).count_ones();
            let _ = writeln!(s, "#define MORPH_KINDS {kinds}");
        }
        s
    }

    /// Vertex and fragment source of this variant.
    pub fn sources(&self) -> Result<(ShaderSource, ShaderSource), Error> {
        shader_sources(VERT, FRAG, &self.defines())
    }
}

/// glTF materials need this many alpha modes in the key; checked here so a new
/// variant can't silently overflow its two bits.
const _: () = assert!(AlphaMode::Blend as u32 <= 3);

/// Which shading model PBR variants use.
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShadingModel {
    /// OpenPBR Surface (the default).
    #[default]
    OpenPbr = 0,
    /// The glTF reference (what Khronos' sample renderer shows), to compare.
    Gltf = 1,
}

/// Compiled PBR programs by variant. Programs are created on first use and
/// kept until `destroy`.
#[derive(Default)]
pub struct PbrShaders {
    programs: HashMap<PbrKey, ProgramId>,
    model: ShadingModel,
}

impl PbrShaders {
    pub fn new() -> Self {
        Self::default()
    }

    /// The shading model new programs use (`program` applies it to keys).
    pub fn set_model(&mut self, model: ShadingModel) {
        self.model = model;
    }

    pub fn model(&self) -> ShadingModel {
        self.model
    }

    /// The program for `key` under the current shading model, compiling it
    /// on first use.
    pub fn program(&mut self, ctx: &Context, key: PbrKey) -> Result<ProgramId, Error> {
        let key = key.with_model(self.model);
        if let Some(&p) = self.programs.get(&key) {
            return Ok(p);
        }
        let (vert, frag) = key.sources()?;
        let p = ctx.create_program_from(&vert, &frag)?;
        self.programs.insert(key, p);
        Ok(p)
    }

    /// The program for one of `model`'s draw items.
    pub fn program_for(
        &mut self,
        ctx: &Context,
        model: &Model,
        item: &DrawItem,
    ) -> Result<ProgramId, Error> {
        let material = usize::try_from(item.material_index())
            .ok()
            .and_then(|i| model.materials().get(i));
        let key = match material {
            Some(m) => PbrKey::new(item.attributes(), m),
            None => PbrKey::new(item.attributes(), &rswebgl_gltf::default_material()),
        };
        self.program(ctx, key)
    }

    /// Whether `program` is one of these variants.
    pub fn is_pbr(&self, program: ProgramId) -> bool {
        self.programs.values().any(|&p| p == program)
    }

    /// Variants compiled so far.
    pub fn len(&self) -> usize {
        self.programs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.programs.is_empty()
    }

    pub fn destroy(self, ctx: &Context) {
        for p in self.programs.into_values() {
            ctx.destroy_program(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn too_many_maps_drop_the_subtlest_first() {
        let mask = (1u64 << MAP_COUNT) - 1;
        let all = PbrKey::from_bits(mask << MAP_SHIFT | mask << UV1_SHIFT);
        // 19 maps + the frame's 5 on 16 units: 8 go.
        let (fit, dropped) = all.within_texture_units(16);
        assert_eq!(dropped, [16, 15, 14, 13, 18, 7, 6, 10]);
        let maps = |k: PbrKey| (k.0 >> MAP_SHIFT) & mask;
        assert_eq!(maps(fit).count_ones() + FRAME_SAMPLERS, 16);
        for gone in [16, 15, 14, 13, 18, 7, 6, 10] {
            assert_eq!(maps(fit) & 1 << gone, 0, "{}", MAP_SLOTS[gone].name);
            assert_eq!(fit.0 & 1 << (UV1_SHIFT + gone as u64), 0, "its UV bit too");
        }
        assert_ne!(maps(fit) & 1, 0, "base color stays");
        // Within the limit nothing changes.
        assert_eq!(all.within_texture_units(32), (all, vec![]));
    }
}
