//! The built-in glTF metallic-roughness shader (`shaders/pbr.{vert,frag}`) and
//! its variants: one program per combination of the primitive's attributes and
//! the material's maps, alpha mode and shading model, compiled on first use.

use std::collections::HashMap;
use std::fmt::Write as _;

use rswebgl::context::Context;
use rswebgl::error::Error;
use rswebgl::ids::ProgramId;
use rswebgl_gltf::{
    ATTR_COLOR, ATTR_NORMAL, ATTR_TANGENT, ATTR_UV0, ATTR_UV1, AlphaMode, DrawItem, Material, Model,
};

const VERT: &str = include_str!("../shaders/pbr.vert");
const FRAG: &str = include_str!("../shaders/pbr.frag");

// Key layout: bits 0..5 the primitive's ATTR_* bits; 5..10 which maps the
// material has (`MAPS` order); 10..15 which of them read TEXCOORD_1; 15..17
// the alpha mode; 17 unlit.
const MAPS: [&str; 5] = [
    "BASE_COLOR",
    "METALLIC_ROUGHNESS",
    "NORMAL",
    "OCCLUSION",
    "EMISSIVE",
];
const ATTRS: [(u32, &str); 5] = [
    (ATTR_NORMAL, "HAS_NORMAL"),
    (ATTR_UV0, "HAS_UV0"),
    (ATTR_TANGENT, "HAS_TANGENT"),
    (ATTR_COLOR, "HAS_COLOR"),
    (ATTR_UV1, "HAS_UV1"),
];
const MAP_SHIFT: u32 = 5;
const UV1_SHIFT: u32 = 10;
const ALPHA_SHIFT: u32 = 15;
const UNLIT: u32 = 1 << 17;

/// A shader variant: which features one program is compiled with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PbrKey(u32);

impl PbrKey {
    /// The variant for a primitive with `attributes` (`ATTR_*` bits) drawn
    /// with `material`.
    pub fn new(attributes: u32, material: &Material) -> Self {
        let maps = [
            (material.base_color_texture(), material.base_color_uv()),
            (
                material.metallic_roughness_texture(),
                material.metallic_roughness_uv(),
            ),
            (material.normal_texture(), material.normal_uv()),
            (material.occlusion_texture(), material.occlusion_uv()),
            (material.emissive_texture(), material.emissive_uv()),
        ];
        let mut key = attributes & 0x1f;
        for (i, (tex, uv)) in maps.into_iter().enumerate() {
            if tex >= 0 {
                key |= 1 << (MAP_SHIFT + i as u32);
                if uv == 1 {
                    key |= 1 << (UV1_SHIFT + i as u32);
                }
            }
        }
        key |= (material.alpha_mode() as u32) << ALPHA_SHIFT;
        if material.unlit() {
            key |= UNLIT;
        }
        Self(key)
    }

    /// A key from raw bits: 0..5 `ATTR_*`, 5..10 maps (base color, metallic-
    /// roughness, normal, occlusion, emissive), 10..15 which maps read
    /// TEXCOORD_1, 15..17 alpha mode, 17 unlit. For tools and tests; models
    /// go through `new`.
    pub fn from_bits(bits: u32) -> Self {
        Self(bits & ((UNLIT << 1) - 1))
    }

    pub fn bits(&self) -> u32 {
        self.0
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
        for (i, name) in MAPS.iter().enumerate() {
            if k & (1 << (MAP_SHIFT + i as u32)) != 0 {
                let uv = if k & (1 << (UV1_SHIFT + i as u32)) != 0 {
                    "v_uv1"
                } else {
                    "v_uv0"
                };
                let _ = writeln!(s, "#define HAS_{name}_MAP\n#define {name}_UV {uv}");
            }
        }
        let alpha = match (k >> ALPHA_SHIFT) & 3 {
            1 => "MASK",
            2 => "BLEND",
            _ => "OPAQUE",
        };
        let _ = writeln!(s, "#define ALPHAMODE_{alpha}");
        if k & UNLIT != 0 {
            s.push_str("#define MATERIAL_UNLIT\n");
        }
        s
    }

    /// Vertex and fragment source of this variant.
    pub fn sources(&self) -> (String, String) {
        let head = format!("#version 300 es\n{}", self.defines());
        (format!("{head}{VERT}"), format!("{head}{FRAG}"))
    }
}

/// glTF materials need this many alpha modes in the key; checked here so a new
/// variant can't silently overflow its two bits.
const _: () = assert!(AlphaMode::Blend as u32 <= 3);

/// Compiled PBR programs by variant. Programs are created on first use and
/// kept until `destroy`.
#[derive(Default)]
pub struct PbrShaders {
    programs: HashMap<PbrKey, ProgramId>,
}

impl PbrShaders {
    pub fn new() -> Self {
        Self::default()
    }

    /// The program for `key`, compiling it on first use.
    pub fn program(&mut self, ctx: &Context, key: PbrKey) -> Result<ProgramId, Error> {
        if let Some(&p) = self.programs.get(&key) {
            return Ok(p);
        }
        let (vert, frag) = key.sources();
        let p = ctx.create_program(&vert, &frag)?;
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
