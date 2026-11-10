//! Translate a glTF material into a `Material` data record + a `RenderState`,
//! following the glTF 2.0 spec (§3.9 materials, alpha coverage, double-sided).
//!
//! Texture references are stored as indices into `Model::texture(i)`; `-1` means
//! "no texture". `RenderState` is derived per draw item (not per material),
//! because front-face winding depends on the node's world-transform determinant.

use rswebgl::context::Context;
use rswebgl::ids::ParamGroupId;
use rswebgl::render_state::{BlendFactor, CullFace, DepthFunc, FrontFace, RenderState};
use wasm_bindgen::prelude::*;

/// glTF `alphaMode`.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaMode {
    Opaque = 0,
    Mask = 1,
    Blend = 2,
}

/// PBR metallic-roughness material data (the subset we translate in v1).
#[wasm_bindgen]
#[derive(Clone, Debug)]
pub struct Material {
    pub(crate) name: Option<String>,
    pub(crate) base_color_factor: [f32; 4],
    pub(crate) metallic: f32,
    pub(crate) roughness: f32,
    pub(crate) emissive: [f32; 3],
    pub(crate) alpha_mode: AlphaMode,
    pub(crate) alpha_cutoff: f32,
    pub(crate) double_sided: bool,
    // Each map's (texture index or -1, TEXCOORD set), in `MAP_SLOTS` order.
    pub(crate) maps: [(i32, u32); MAP_COUNT],
    // Each map's KHR_texture_transform (`UV_IDENTITY` without one).
    pub(crate) uv_transforms: [[f32; 5]; MAP_COUNT],
    pub(crate) normal_scale: f32,
    pub(crate) occlusion_strength: f32,
    // KHR_materials_emissive_strength (1 without it).
    pub(crate) emissive_strength: f32,
    // KHR_materials_unlit.
    pub(crate) unlit: bool,
    // Extension factors (their textures aren't read yet): KHR_materials_ior,
    // _specular, _transmission, _volume (attenuation distance 0 = none),
    // _clearcoat, _dispersion. Defaults are the spec's "extension absent".
    pub(crate) ior: f32,
    pub(crate) specular: f32,
    pub(crate) specular_color: [f32; 3],
    pub(crate) transmission: f32,
    pub(crate) thickness: f32,
    pub(crate) attenuation_color: [f32; 3],
    pub(crate) attenuation_distance: f32,
    pub(crate) clearcoat: f32,
    pub(crate) clearcoat_roughness: f32,
    pub(crate) dispersion: f32,
    // OpenPBR parameters glTF doesn't have (OpenPBR's defaults, except coat_ior
    // 1.5 to match glTF's clearcoat).
    pub(crate) base_weight: f32,
    pub(crate) diffuse_roughness: f32,
    pub(crate) coat_ior: f32,
    pub(crate) coat_color: [f32; 3],
    pub(crate) coat_darkening: f32,
    // OpenPBR coat_roughness_anisotropy, and subsurface (weight, color, mean
    // free path radius and its per-channel scale).
    pub(crate) coat_anisotropy: f32,
    pub(crate) subsurface_weight: f32,
    pub(crate) subsurface_color: [f32; 3],
    pub(crate) subsurface_radius: f32,
    pub(crate) subsurface_radius_scale: [f32; 3],
    // KHR_materials_sheen (OpenPBR fuzz), _iridescence (thin film; thickness
    // in nm, the extension's maximum without a thickness texture),
    // _anisotropy (strength, rotation in radians).
    pub(crate) sheen_color: [f32; 3],
    pub(crate) sheen_roughness: f32,
    pub(crate) iridescence: f32,
    pub(crate) iridescence_ior: f32,
    pub(crate) iridescence_thickness: f32,
    pub(crate) anisotropy_strength: f32,
    pub(crate) anisotropy_rotation: f32,
    // clearcoatNormalTexture.scale; the iridescence thickness at texture
    // value 0 (the maximum is at 1).
    pub(crate) clearcoat_normal_scale: f32,
    pub(crate) iridescence_thickness_min: f32,
    // KHR_materials_diffuse_transmission: the share of diffuse light passed
    // through (thin), and its color.
    pub(crate) diffuse_transmission: f32,
    pub(crate) diffuse_transmission_color: [f32; 3],
    // OpenPBR's on-surface transmission tint (`transmissionTint`).
    pub(crate) transmission_tint: [f32; 3],
}

#[wasm_bindgen]
impl Material {
    /// The glTF material name, if it has one.
    #[wasm_bindgen(getter)]
    pub fn name(&self) -> Option<String> {
        self.name.clone()
    }
    #[wasm_bindgen(getter, js_name = baseColorFactor)]
    pub fn base_color_factor(&self) -> Vec<f32> {
        self.base_color_factor.to_vec()
    }
    #[wasm_bindgen(getter)]
    pub fn metallic(&self) -> f32 {
        self.metallic
    }
    #[wasm_bindgen(getter)]
    pub fn roughness(&self) -> f32 {
        self.roughness
    }
    #[wasm_bindgen(getter, js_name = emissiveFactor)]
    pub fn emissive_factor(&self) -> Vec<f32> {
        self.emissive.to_vec()
    }
    #[wasm_bindgen(getter, js_name = alphaMode)]
    pub fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }
    #[wasm_bindgen(getter, js_name = alphaCutoff)]
    pub fn alpha_cutoff(&self) -> f32 {
        self.alpha_cutoff
    }
    #[wasm_bindgen(getter, js_name = doubleSided)]
    pub fn double_sided(&self) -> bool {
        self.double_sided
    }
    /// Index into `Model::texture(i)`, or `-1` if absent.
    #[wasm_bindgen(getter, js_name = baseColorTexture)]
    pub fn base_color_texture(&self) -> i32 {
        self.maps[0].0
    }
    /// TEXCOORD set index used by the base-color texture (`TEXCOORD_n`).
    #[wasm_bindgen(getter, js_name = baseColorUv)]
    pub fn base_color_uv(&self) -> u32 {
        self.maps[0].1
    }
    #[wasm_bindgen(getter, js_name = metallicRoughnessTexture)]
    pub fn metallic_roughness_texture(&self) -> i32 {
        self.maps[1].0
    }
    #[wasm_bindgen(getter, js_name = normalTexture)]
    pub fn normal_texture(&self) -> i32 {
        self.maps[2].0
    }
    #[wasm_bindgen(getter, js_name = occlusionTexture)]
    pub fn occlusion_texture(&self) -> i32 {
        self.maps[3].0
    }
    #[wasm_bindgen(getter, js_name = emissiveTexture)]
    pub fn emissive_texture(&self) -> i32 {
        self.maps[4].0
    }
    /// TEXCOORD set of each map (`TEXCOORD_n`); 0 when the map is absent.
    #[wasm_bindgen(getter, js_name = metallicRoughnessUv)]
    pub fn metallic_roughness_uv(&self) -> u32 {
        self.maps[1].1
    }
    #[wasm_bindgen(getter, js_name = normalUv)]
    pub fn normal_uv(&self) -> u32 {
        self.maps[2].1
    }
    #[wasm_bindgen(getter, js_name = occlusionUv)]
    pub fn occlusion_uv(&self) -> u32 {
        self.maps[3].1
    }
    #[wasm_bindgen(getter, js_name = emissiveUv)]
    pub fn emissive_uv(&self) -> u32 {
        self.maps[4].1
    }
    /// `normalTexture.scale` (1 without a normal map).
    #[wasm_bindgen(getter, js_name = normalScale)]
    pub fn normal_scale(&self) -> f32 {
        self.normal_scale
    }
    /// `occlusionTexture.strength` (1 without an occlusion map).
    #[wasm_bindgen(getter, js_name = occlusionStrength)]
    pub fn occlusion_strength(&self) -> f32 {
        self.occlusion_strength
    }
    /// `KHR_materials_emissive_strength` (1 without it).
    #[wasm_bindgen(getter, js_name = emissiveStrength)]
    pub fn emissive_strength(&self) -> f32 {
        self.emissive_strength
    }
    /// `KHR_materials_unlit`: shade with the base color only.
    #[wasm_bindgen(getter)]
    pub fn unlit(&self) -> bool {
        self.unlit
    }
    /// `KHR_materials_ior` (1.5 without it).
    #[wasm_bindgen(getter)]
    pub fn ior(&self) -> f32 {
        self.ior
    }
    /// `KHR_materials_specular` factor (1 without it).
    #[wasm_bindgen(getter)]
    pub fn specular(&self) -> f32 {
        self.specular
    }
    /// `KHR_materials_specular` color factor (white without it).
    #[wasm_bindgen(getter, js_name = specularColor)]
    pub fn specular_color(&self) -> Vec<f32> {
        self.specular_color.to_vec()
    }
    /// `KHR_materials_transmission` factor (0: opaque).
    #[wasm_bindgen(getter)]
    pub fn transmission(&self) -> f32 {
        self.transmission
    }
    /// `KHR_materials_volume` thickness in mesh units (0: thin-walled).
    #[wasm_bindgen(getter)]
    pub fn thickness(&self) -> f32 {
        self.thickness
    }
    /// `KHR_materials_volume`: the color white light turns into after
    /// `attenuationDistance` inside.
    #[wasm_bindgen(getter, js_name = attenuationColor)]
    pub fn attenuation_color(&self) -> Vec<f32> {
        self.attenuation_color.to_vec()
    }
    /// `KHR_materials_volume` attenuation distance; 0 = no absorption.
    #[wasm_bindgen(getter, js_name = attenuationDistance)]
    pub fn attenuation_distance(&self) -> f32 {
        self.attenuation_distance
    }
    /// `KHR_materials_clearcoat` factor (0: no coat).
    #[wasm_bindgen(getter)]
    pub fn clearcoat(&self) -> f32 {
        self.clearcoat
    }
    #[wasm_bindgen(getter, js_name = clearcoatRoughness)]
    pub fn clearcoat_roughness(&self) -> f32 {
        self.clearcoat_roughness
    }
    /// `KHR_materials_sheen` color (black: no sheen / fuzz).
    #[wasm_bindgen(getter, js_name = sheenColor)]
    pub fn sheen_color(&self) -> Vec<f32> {
        self.sheen_color.to_vec()
    }
    /// `KHR_materials_diffuse_transmission` factor (0: none).
    #[wasm_bindgen(getter, js_name = diffuseTransmission)]
    pub fn diffuse_transmission(&self) -> f32 {
        self.diffuse_transmission
    }

    /// `KHR_materials_iridescence` factor (0: no thin film).
    #[wasm_bindgen(getter)]
    pub fn iridescence(&self) -> f32 {
        self.iridescence
    }
    /// `KHR_materials_anisotropy` strength (0: isotropic).
    #[wasm_bindgen(getter, js_name = anisotropyStrength)]
    pub fn anisotropy_strength(&self) -> f32 {
        self.anisotropy_strength
    }
    /// OpenPBR `coat_roughness_anisotropy`.
    pub fn coat_anisotropy(&self) -> f32 {
        self.coat_anisotropy
    }

    /// OpenPBR `subsurface_weight` (0: none).
    pub fn subsurface_weight(&self) -> f32 {
        self.subsurface_weight
    }

    /// OpenPBR `base_diffuse_roughness` (0: Lambert).
    #[wasm_bindgen(getter, js_name = diffuseRoughness)]
    pub fn diffuse_roughness(&self) -> f32 {
        self.diffuse_roughness
    }
    /// `KHR_materials_dispersion` (0: none; 20/Abbe number otherwise).
    #[wasm_bindgen(getter)]
    pub fn dispersion(&self) -> f32 {
        self.dispersion
    }
}

/// A numeric material property: its glTF name (as in the spec and its
/// extensions: `baseColorFactor`, `roughnessFactor`, `ior`, …), the uniform
/// that carries it in a material group, and how many floats it has.
#[derive(Debug, Clone, Copy)]
pub struct Property {
    pub name: &'static str,
    pub uniform: &'static str,
    pub len: usize,
}

const fn prop(name: &'static str, uniform: &'static str, len: usize) -> Property {
    Property { name, uniform, len }
}

/// Every numeric property a material group carries, by glTF name.
pub const PROPERTIES: [Property; 40] = [
    prop("baseColorFactor", "u_base_color_factor", 4),
    prop("metallicFactor", "u_metallic_factor", 1),
    prop("roughnessFactor", "u_roughness_factor", 1),
    prop("emissiveFactor", "u_emissive_factor", 3),
    prop("emissiveStrength", "u_emissive_strength", 1),
    prop("normalScale", "u_normal_scale", 1),
    prop("occlusionStrength", "u_occlusion_strength", 1),
    prop("alphaCutoff", "u_alpha_cutoff", 1),
    prop("ior", "u_ior", 1),
    prop("specularFactor", "u_specular_factor", 1),
    prop("specularColorFactor", "u_specular_color", 3),
    prop("transmissionFactor", "u_transmission", 1),
    prop("thicknessFactor", "u_thickness", 1),
    prop("attenuationColor", "u_attenuation_color", 3),
    prop("attenuationDistance", "u_attenuation_distance", 1),
    prop("clearcoatFactor", "u_clearcoat", 1),
    prop("clearcoatRoughnessFactor", "u_clearcoat_roughness", 1),
    prop("dispersion", "u_dispersion", 1),
    prop("baseWeight", "u_base_weight", 1),
    prop("diffuseRoughness", "u_diffuse_roughness", 1),
    prop("coatIor", "u_coat_ior", 1),
    prop("coatColor", "u_coat_color", 3),
    prop("coatDarkening", "u_coat_darkening", 1),
    prop("coatAnisotropy", "u_coat_anisotropy", 1),
    prop("subsurfaceWeight", "u_subsurface_weight", 1),
    prop("subsurfaceColor", "u_subsurface_color", 3),
    prop("subsurfaceRadius", "u_subsurface_radius", 1),
    prop("subsurfaceRadiusScale", "u_subsurface_radius_scale", 3),
    prop("sheenColorFactor", "u_sheen_color", 3),
    prop("sheenRoughnessFactor", "u_sheen_roughness", 1),
    prop("iridescenceFactor", "u_iridescence", 1),
    prop("iridescenceIor", "u_iridescence_ior", 1),
    prop("iridescenceThickness", "u_iridescence_thickness", 1),
    prop("anisotropyStrength", "u_anisotropy_strength", 1),
    prop("anisotropyRotation", "u_anisotropy_rotation", 1),
    prop("clearcoatNormalScale", "u_clearcoat_normal_scale", 1),
    prop(
        "iridescenceThicknessMinimum",
        "u_iridescence_thickness_min",
        1,
    ),
    prop("diffuseTransmissionFactor", "u_diffuse_transmission", 1),
    prop(
        "diffuseTransmissionColorFactor",
        "u_diffuse_transmission_color",
        3,
    ),
    // OpenPBR's transmission_color with transmission_depth 0 (no medium): a
    // tint on the surface. Not glTF's; white unless set through the
    // OpenPBR names.
    prop("transmissionTint", "u_transmission_tint", 3),
];

impl Material {
    // The storage of each of `PROPERTIES`, in the same order.
    fn values_mut(&mut self) -> [&mut [f32]; 40] {
        [
            &mut self.base_color_factor,
            std::slice::from_mut(&mut self.metallic),
            std::slice::from_mut(&mut self.roughness),
            &mut self.emissive,
            std::slice::from_mut(&mut self.emissive_strength),
            std::slice::from_mut(&mut self.normal_scale),
            std::slice::from_mut(&mut self.occlusion_strength),
            std::slice::from_mut(&mut self.alpha_cutoff),
            std::slice::from_mut(&mut self.ior),
            std::slice::from_mut(&mut self.specular),
            &mut self.specular_color,
            std::slice::from_mut(&mut self.transmission),
            std::slice::from_mut(&mut self.thickness),
            &mut self.attenuation_color,
            std::slice::from_mut(&mut self.attenuation_distance),
            std::slice::from_mut(&mut self.clearcoat),
            std::slice::from_mut(&mut self.clearcoat_roughness),
            std::slice::from_mut(&mut self.dispersion),
            std::slice::from_mut(&mut self.base_weight),
            std::slice::from_mut(&mut self.diffuse_roughness),
            std::slice::from_mut(&mut self.coat_ior),
            &mut self.coat_color,
            std::slice::from_mut(&mut self.coat_darkening),
            std::slice::from_mut(&mut self.coat_anisotropy),
            std::slice::from_mut(&mut self.subsurface_weight),
            &mut self.subsurface_color,
            std::slice::from_mut(&mut self.subsurface_radius),
            &mut self.subsurface_radius_scale,
            &mut self.sheen_color,
            std::slice::from_mut(&mut self.sheen_roughness),
            std::slice::from_mut(&mut self.iridescence),
            std::slice::from_mut(&mut self.iridescence_ior),
            std::slice::from_mut(&mut self.iridescence_thickness),
            std::slice::from_mut(&mut self.anisotropy_strength),
            std::slice::from_mut(&mut self.anisotropy_rotation),
            std::slice::from_mut(&mut self.clearcoat_normal_scale),
            std::slice::from_mut(&mut self.iridescence_thickness_min),
            std::slice::from_mut(&mut self.diffuse_transmission),
            &mut self.diffuse_transmission_color,
            &mut self.transmission_tint,
        ]
    }

    /// The property named `name` (see `PROPERTIES`), or None for an unknown
    /// name.
    pub fn property(&self, name: &str) -> Option<Vec<f32>> {
        let i = PROPERTIES.iter().position(|p| p.name == name)?;
        Some(self.clone().values_mut()[i].to_vec())
    }

    /// Sets a property; `values` must have its length (4 for
    /// `baseColorFactor`, 3 for colors, 1 otherwise). An infinite
    /// `attenuationDistance` means no absorption (stored as 0).
    pub fn set_property(&mut self, name: &str, values: &[f32]) -> Result<(), String> {
        let i = PROPERTIES
            .iter()
            .position(|p| p.name == name)
            .ok_or_else(|| format!("no material property \"{name}\""))?;
        let len = PROPERTIES[i].len;
        if values.len() != len {
            return Err(format!(
                "material property \"{name}\" takes {len} value(s), got {}",
                values.len()
            ));
        }
        let mut v = values.to_vec();
        if name == "attenuationDistance" && !v[0].is_finite() {
            v[0] = 0.0;
        }
        self.values_mut()[i].copy_from_slice(&v);
        Ok(())
    }

    /// Writes every numeric property into `group` (a group of a
    /// `Model::material_layout`).
    pub fn write_properties(&self, ctx: &Context, group: ParamGroupId) {
        for (p, v) in PROPERTIES.iter().zip(self.clone().values_mut()) {
            ctx.param_set_floats_by_name(group, p.uniform, v);
        }
    }

    /// Writes one property into `group`.
    pub fn write_property(&self, ctx: &Context, group: ParamGroupId, name: &str) {
        if let (Some(p), Some(v)) = (
            PROPERTIES.iter().find(|p| p.name == name),
            self.property(name),
        ) {
            ctx.param_set_floats_by_name(group, p.uniform, &v);
        }
    }
}

/// One kind of material map: its glTF name, the sampler uniform carrying it
/// in a material group, the shader's name for it (`HAS_<define>_MAP`,
/// `<define>_UV`), and whether it holds colors (decoded from sRGB).
#[derive(Debug, Clone, Copy)]
pub struct MapSlot {
    pub name: &'static str,
    pub uniform: &'static str,
    /// Its KHR_texture_transform matrix (mat3) in a material group.
    pub transform: &'static str,
    pub define: &'static str,
    pub srgb: bool,
}

const fn slot(
    name: &'static str,
    uniform: &'static str,
    transform: &'static str,
    define: &'static str,
    srgb: bool,
) -> MapSlot {
    MapSlot {
        name,
        uniform,
        transform,
        define,
        srgb,
    }
}

/// How many kinds of maps a material has.
pub const MAP_COUNT: usize = 19;

/// Every material map: the core five, then the extensions'
/// (KHR_materials_clearcoat, _transmission, _volume, _specular, _sheen,
/// _iridescence, _anisotropy). The channels each reads are the extensions'.
pub const MAP_SLOTS: [MapSlot; MAP_COUNT] = [
    slot(
        "baseColorTexture",
        "u_base_color_tex",
        "u_base_color_uv",
        "BASE_COLOR",
        true,
    ),
    slot(
        "metallicRoughnessTexture",
        "u_metallic_roughness_tex",
        "u_metallic_roughness_uv",
        "METALLIC_ROUGHNESS",
        false,
    ),
    slot(
        "normalTexture",
        "u_normal_tex",
        "u_normal_uv",
        "NORMAL",
        false,
    ),
    slot(
        "occlusionTexture",
        "u_occlusion_tex",
        "u_occlusion_uv",
        "OCCLUSION",
        false,
    ),
    slot(
        "emissiveTexture",
        "u_emissive_tex",
        "u_emissive_uv",
        "EMISSIVE",
        true,
    ),
    slot(
        "clearcoatTexture",
        "u_clearcoat_tex",
        "u_clearcoat_uv",
        "CLEARCOAT",
        false,
    ),
    slot(
        "clearcoatRoughnessTexture",
        "u_clearcoat_roughness_tex",
        "u_clearcoat_roughness_uv",
        "CLEARCOAT_ROUGHNESS",
        false,
    ),
    slot(
        "clearcoatNormalTexture",
        "u_clearcoat_normal_tex",
        "u_clearcoat_normal_uv",
        "CLEARCOAT_NORMAL",
        false,
    ),
    slot(
        "transmissionTexture",
        "u_transmission_tex",
        "u_transmission_uv",
        "TRANSMISSION",
        false,
    ),
    slot(
        "thicknessTexture",
        "u_thickness_tex",
        "u_thickness_uv",
        "THICKNESS",
        false,
    ),
    slot(
        "specularTexture",
        "u_specular_tex",
        "u_specular_uv",
        "SPECULAR",
        false,
    ),
    slot(
        "specularColorTexture",
        "u_specular_color_tex",
        "u_specular_color_uv",
        "SPECULAR_COLOR",
        true,
    ),
    slot(
        "sheenColorTexture",
        "u_sheen_color_tex",
        "u_sheen_color_uv",
        "SHEEN_COLOR",
        true,
    ),
    slot(
        "sheenRoughnessTexture",
        "u_sheen_roughness_tex",
        "u_sheen_roughness_uv",
        "SHEEN_ROUGHNESS",
        false,
    ),
    slot(
        "iridescenceTexture",
        "u_iridescence_tex",
        "u_iridescence_uv",
        "IRIDESCENCE",
        false,
    ),
    slot(
        "iridescenceThicknessTexture",
        "u_iridescence_thickness_tex",
        "u_iridescence_thickness_uv",
        "IRIDESCENCE_THICKNESS",
        false,
    ),
    slot(
        "anisotropyTexture",
        "u_anisotropy_tex",
        "u_anisotropy_uv",
        "ANISOTROPY",
        false,
    ),
    slot(
        "diffuseTransmissionTexture",
        "u_diffuse_transmission_tex",
        "u_diffuse_transmission_uv",
        "DIFFUSE_TRANSMISSION",
        false,
    ),
    slot(
        "diffuseTransmissionColorTexture",
        "u_diffuse_transmission_color_tex",
        "u_diffuse_transmission_color_uv",
        "DIFFUSE_TRANSMISSION_COLOR",
        true,
    ),
];

/// The material maps by glTF name (`MAP_SLOTS` order).
pub const MAP_NAMES: [&str; MAP_COUNT] = {
    let mut out = [""; MAP_COUNT];
    let mut i = 0;
    while i < MAP_COUNT {
        out[i] = MAP_SLOTS[i].name;
        i += 1;
    }
    out
};

/// The texture index of a map whose texture isn't one of the model's (set by
/// the app — a scene's `setMaterialTexture`).
pub const EXTERNAL_TEXTURE: i32 = i32::MAX;

impl Material {
    /// Map `slot` (an index into `MAP_SLOTS`): its texture index (−1 none,
    /// `EXTERNAL_TEXTURE` for the app's) and TEXCOORD set.
    pub fn map(&self, slot: usize) -> Option<(i32, u32)> {
        self.maps.get(slot).copied()
    }

    /// Sets map `slot`: `texture` as for `map`, `uv` its TEXCOORD set (0 or 1).
    pub fn set_map(&mut self, slot: usize, texture: i32, uv: u32) {
        if let Some(m) = self.maps.get_mut(slot) {
            *m = (texture, if texture < 0 { 0 } else { uv });
        }
    }

    /// Every map's (texture index or −1, TEXCOORD set), `MAP_SLOTS` order.
    pub fn maps(&self) -> [(i32, u32); MAP_COUNT] {
        self.maps
    }

    /// Map `slot`'s KHR_texture_transform: offset x, y, rotation, scale x, y.
    pub fn uv_transform(&self, slot: usize) -> [f32; 5] {
        self.uv_transforms.get(slot).copied().unwrap_or(UV_IDENTITY)
    }

    pub fn set_uv_transform(&mut self, slot: usize, t: [f32; 5]) {
        if let Some(u) = self.uv_transforms.get_mut(slot) {
            *u = t;
        }
    }

    /// Whether any map in use is transformed (the shader then applies them).
    pub fn has_uv_transform(&self) -> bool {
        self.maps
            .iter()
            .zip(&self.uv_transforms)
            .any(|((tex, _), t)| *tex >= 0 && *t != UV_IDENTITY)
    }
}

impl Material {
    /// Writes every map's KHR_texture_transform matrix into `group`.
    pub fn write_uv_transforms(&self, ctx: &Context, group: ParamGroupId) {
        for (slot, t) in MAP_SLOTS.iter().zip(self.uv_transforms) {
            ctx.param_set_floats_by_name(group, slot.transform, &uv_matrix(t));
        }
    }
}
/// A map's reference in the glTF document: the document texture and the
/// TEXCOORD set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DocMap {
    pub(crate) texture: usize,
    pub(crate) tex_coord: u32,
    // KHR_texture_transform: offset x, y, rotation, scale x, y.
    pub(crate) transform: [f32; 5],
}

/// No KHR_texture_transform: offset 0, rotation 0, scale 1.
pub const UV_IDENTITY: [f32; 5] = [0.0, 0.0, 0.0, 1.0, 1.0];

// A KHR_texture_transform object (raw JSON) → (offset, rotation, scale) and
// its texCoord override.
fn uv_transform(v: Option<&gltf::json::Value>) -> ([f32; 5], Option<u32>) {
    let Some(v) = v else {
        return (UV_IDENTITY, None);
    };
    let pair = |key: &str, default: [f32; 2]| {
        v.get(key)
            .and_then(|a| a.as_array())
            .filter(|a| a.len() == 2)
            .map_or(default, |a| {
                let f = |i: usize| a[i].as_f64().map_or(default[i], |x| x as f32);
                [f(0), f(1)]
            })
    };
    let [ox, oy] = pair("offset", [0.0, 0.0]);
    let [sx, sy] = pair("scale", [1.0, 1.0]);
    let rotation = v.get("rotation").and_then(|r| r.as_f64()).unwrap_or(0.0) as f32;
    let tex_coord = v.get("texCoord").and_then(|t| t.as_u64()).map(|t| t as u32);
    ([ox, oy, rotation, sx, sy], tex_coord)
}

// A map reference with its transform (the transform's texCoord wins).
fn with_transform(texture: usize, tex_coord: u32, ext: Option<&gltf::json::Value>) -> DocMap {
    let (transform, over) = uv_transform(ext);
    DocMap {
        texture,
        tex_coord: over.unwrap_or(tex_coord),
        transform,
    }
}

/// KHR_texture_transform as a column-major mat3 (translation × rotation ×
/// scale, as the Khronos sample renderer builds it).
pub fn uv_matrix(t: [f32; 5]) -> [f32; 9] {
    let [ox, oy, r, sx, sy] = t;
    let (s, c) = r.sin_cos();
    [c * sx, -s * sx, 0.0, s * sy, c * sy, 0.0, ox, oy, 1.0]
}

/// The raw JSON of a texture-info object inside an extension the `gltf`
/// crate doesn't model.
fn ext_texture<'a>(mat: &'a gltf::Material, ext: &str, key: &str) -> Option<&'a gltf::json::Value> {
    mat.extension_value(ext)?.get(key)
}

fn doc_map_from_json(v: Option<&gltf::json::Value>) -> Option<DocMap> {
    let v = v?;
    Some(with_transform(
        v.get("index")?.as_u64()? as usize,
        v.get("texCoord").and_then(|t| t.as_u64()).unwrap_or(0) as u32,
        v.get("extensions")
            .and_then(|e| e.get("KHR_texture_transform")),
    ))
}

fn doc_map(info: Option<gltf::texture::Info>) -> Option<DocMap> {
    info.map(|i| {
        with_transform(
            i.texture().index(),
            i.tex_coord(),
            i.extension_value("KHR_texture_transform"),
        )
    })
}

/// Every map `mat` references, `MAP_SLOTS` order.
pub(crate) fn doc_maps(mat: &gltf::Material) -> [Option<DocMap>; MAP_COUNT] {
    let pbr = mat.pbr_metallic_roughness();
    let json = |ext: &str, key: &str| doc_map_from_json(ext_texture(mat, ext, key));
    [
        doc_map(pbr.base_color_texture()),
        doc_map(pbr.metallic_roughness_texture()),
        mat.normal_texture().map(|n| {
            with_transform(
                n.texture().index(),
                n.tex_coord(),
                n.extension_value("KHR_texture_transform"),
            )
        }),
        mat.occlusion_texture().map(|o| {
            with_transform(
                o.texture().index(),
                o.tex_coord(),
                o.extension_value("KHR_texture_transform"),
            )
        }),
        doc_map(mat.emissive_texture()),
        json("KHR_materials_clearcoat", "clearcoatTexture"),
        json("KHR_materials_clearcoat", "clearcoatRoughnessTexture"),
        json("KHR_materials_clearcoat", "clearcoatNormalTexture"),
        mat.transmission()
            .and_then(|t| doc_map(t.transmission_texture())),
        mat.volume().and_then(|v| doc_map(v.thickness_texture())),
        mat.specular().and_then(|s| doc_map(s.specular_texture())),
        mat.specular()
            .and_then(|s| doc_map(s.specular_color_texture())),
        json("KHR_materials_sheen", "sheenColorTexture"),
        json("KHR_materials_sheen", "sheenRoughnessTexture"),
        json("KHR_materials_iridescence", "iridescenceTexture"),
        json("KHR_materials_iridescence", "iridescenceThicknessTexture"),
        json("KHR_materials_anisotropy", "anisotropyTexture"),
        json(
            "KHR_materials_diffuse_transmission",
            "diffuseTransmissionTexture",
        ),
        json(
            "KHR_materials_diffuse_transmission",
            "diffuseTransmissionColorTexture",
        ),
    ]
}

/// Translate one glTF material's data.
pub fn translate(mat: &gltf::Material) -> Material {
    let pbr = mat.pbr_metallic_roughness();
    Material {
        name: mat.name().map(str::to_owned),
        base_color_factor: pbr.base_color_factor(),
        metallic: pbr.metallic_factor(),
        roughness: pbr.roughness_factor(),
        emissive: mat.emissive_factor(),
        alpha_mode: match mat.alpha_mode() {
            gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
            gltf::material::AlphaMode::Mask => AlphaMode::Mask,
            gltf::material::AlphaMode::Blend => AlphaMode::Blend,
        },
        alpha_cutoff: mat.alpha_cutoff().unwrap_or(0.5),
        double_sided: mat.double_sided(),
        maps: doc_maps(mat).map(|m| m.map_or((-1, 0), |m| (m.texture as i32, m.tex_coord))),
        uv_transforms: doc_maps(mat).map(|m| m.map_or(UV_IDENTITY, |m| m.transform)),
        normal_scale: mat.normal_texture().map_or(1.0, |i| i.scale()),
        occlusion_strength: mat.occlusion_texture().map_or(1.0, |i| i.strength()),
        emissive_strength: mat.emissive_strength().unwrap_or(1.0),
        unlit: mat.unlit(),
        ior: mat.ior().unwrap_or(1.5),
        specular: mat.specular().map_or(1.0, |s| s.specular_factor()),
        specular_color: mat
            .specular()
            .map_or([1.0; 3], |s| s.specular_color_factor()),
        transmission: mat.transmission().map_or(0.0, |t| t.transmission_factor()),
        thickness: mat.volume().map_or(0.0, |v| v.thickness_factor()),
        attenuation_color: mat.volume().map_or([1.0; 3], |v| v.attenuation_color()),
        attenuation_distance: mat
            .volume()
            .map(|v| v.attenuation_distance())
            .filter(|d| d.is_finite())
            .unwrap_or(0.0),
        clearcoat: ext_f32(mat, "KHR_materials_clearcoat", "clearcoatFactor", 0.0),
        clearcoat_roughness: ext_f32(
            mat,
            "KHR_materials_clearcoat",
            "clearcoatRoughnessFactor",
            0.0,
        ),
        dispersion: ext_f32(mat, "KHR_materials_dispersion", "dispersion", 0.0),
        base_weight: 1.0,
        diffuse_roughness: 0.0,
        coat_ior: 1.5,
        coat_color: [1.0; 3],
        coat_darkening: 0.0, // glTF's clearcoat leaves the base unchanged (Khronos)
        coat_anisotropy: 0.0,
        subsurface_weight: 0.0,
        subsurface_color: [0.8; 3],
        subsurface_radius: 1.0,
        subsurface_radius_scale: [1.0, 0.5, 0.25],
        sheen_color: ext_vec3(mat, "KHR_materials_sheen", "sheenColorFactor", [0.0; 3]),
        sheen_roughness: ext_f32(mat, "KHR_materials_sheen", "sheenRoughnessFactor", 0.0),
        iridescence: ext_f32(mat, "KHR_materials_iridescence", "iridescenceFactor", 0.0),
        iridescence_ior: ext_f32(mat, "KHR_materials_iridescence", "iridescenceIor", 1.3),
        iridescence_thickness: ext_f32(
            mat,
            "KHR_materials_iridescence",
            "iridescenceThicknessMaximum",
            400.0,
        ),
        anisotropy_strength: ext_f32(mat, "KHR_materials_anisotropy", "anisotropyStrength", 0.0),
        anisotropy_rotation: ext_f32(mat, "KHR_materials_anisotropy", "anisotropyRotation", 0.0),
        clearcoat_normal_scale: ext_texture(
            mat,
            "KHR_materials_clearcoat",
            "clearcoatNormalTexture",
        )
        .and_then(|t| t.get("scale").and_then(|s| s.as_f64()))
        .map_or(1.0, |s| s as f32),
        iridescence_thickness_min: ext_f32(
            mat,
            "KHR_materials_iridescence",
            "iridescenceThicknessMinimum",
            100.0,
        ),
        diffuse_transmission: ext_f32(
            mat,
            "KHR_materials_diffuse_transmission",
            "diffuseTransmissionFactor",
            0.0,
        ),
        diffuse_transmission_color: ext_vec3(
            mat,
            "KHR_materials_diffuse_transmission",
            "diffuseTransmissionColorFactor",
            [1.0; 3],
        ),
        transmission_tint: [1.0; 3],
    }
}

/// Remap every texture index in `m` through `remap` (document texture index →
/// final `Model` texture index). Called after textures are assembled, since
/// ORM-packing can collapse two source textures into one and reorder the rest.
pub fn remap_textures(m: &mut Material, remap: &[i32]) {
    let map = |i: i32| -> i32 {
        if i >= 0 {
            remap.get(i as usize).copied().unwrap_or(-1)
        } else {
            -1
        }
    };
    for (tex, _) in &mut m.maps {
        if *tex != EXTERNAL_TEXTURE {
            *tex = map(*tex);
        }
    }
}

/// The glTF default material (used for primitives that reference none):
/// white, fully metallic and rough, opaque.
pub fn default_material() -> Material {
    Material {
        name: None,
        base_color_factor: [1.0, 1.0, 1.0, 1.0],
        metallic: 1.0,
        roughness: 1.0,
        emissive: [0.0, 0.0, 0.0],
        alpha_mode: AlphaMode::Opaque,
        alpha_cutoff: 0.5,
        double_sided: false,
        maps: [(-1, 0); MAP_COUNT],
        uv_transforms: [UV_IDENTITY; MAP_COUNT],
        normal_scale: 1.0,
        occlusion_strength: 1.0,
        emissive_strength: 1.0,
        unlit: false,
        ior: 1.5,
        specular: 1.0,
        specular_color: [1.0; 3],
        transmission: 0.0,
        thickness: 0.0,
        attenuation_color: [1.0; 3],
        attenuation_distance: 0.0,
        clearcoat: 0.0,
        clearcoat_roughness: 0.0,
        dispersion: 0.0,
        base_weight: 1.0,
        diffuse_roughness: 0.0,
        coat_ior: 1.5,
        coat_color: [1.0; 3],
        coat_darkening: 1.0,
        coat_anisotropy: 0.0,
        subsurface_weight: 0.0,
        subsurface_color: [0.8; 3],
        subsurface_radius: 1.0,
        subsurface_radius_scale: [1.0, 0.5, 0.25],
        sheen_color: [0.0; 3],
        sheen_roughness: 0.0,
        iridescence: 0.0,
        iridescence_ior: 1.3,
        iridescence_thickness: 400.0,
        anisotropy_strength: 0.0,
        anisotropy_rotation: 0.0,
        clearcoat_normal_scale: 1.0,
        iridescence_thickness_min: 100.0,
        diffuse_transmission: 0.0,
        diffuse_transmission_color: [1.0; 3],
        transmission_tint: [1.0; 3],
    }
}

/// Fixed-function state for a draw using `m`. `world_det_negative` flips the
/// front face to CW (a mirroring node transform reverses triangle winding, per
/// spec). Culling is off when the material is double-sided.
pub fn render_state_for(m: &Material, world_det_negative: bool) -> RenderState {
    let mut s = RenderState::new();
    s.depth_test = true;
    s.depth_func = DepthFunc::Less;
    s.cull_face = !m.double_sided;
    s.cull_mode = CullFace::Back;
    s.front_face = if world_det_negative {
        FrontFace::Cw
    } else {
        FrontFace::Ccw
    };
    if m.alpha_mode == AlphaMode::Blend {
        s.blend = true;
        s.blend_src_rgb = BlendFactor::SrcAlpha;
        s.blend_dst_rgb = BlendFactor::OneMinusSrcAlpha;
        s.blend_src_alpha = BlendFactor::One;
        s.blend_dst_alpha = BlendFactor::OneMinusSrcAlpha;
        // Transparent surfaces shouldn't occlude what's behind them in the
        // depth buffer (proper sorting is the app's job).
        s.depth_mask = false;
    }
    s
}

// A number in an extension the `gltf` crate doesn't model (read from its raw
// JSON), or `default` when the extension or the property is absent.
fn ext_f32(mat: &gltf::Material, ext: &str, key: &str, default: f32) -> f32 {
    mat.extension_value(ext)
        .and_then(|e| e.get(key))
        .and_then(|v| v.as_f64())
        .map_or(default, |v| v as f32)
}

fn ext_vec3(mat: &gltf::Material, ext: &str, key: &str, default: [f32; 3]) -> [f32; 3] {
    mat.extension_value(ext)
        .and_then(|e| e.get(key))
        .and_then(|v| v.as_array())
        .filter(|a| a.len() == 3)
        .and_then(|a| {
            let c: Vec<f32> = a
                .iter()
                .filter_map(|x| x.as_f64())
                .map(|x| x as f32)
                .collect();
            <[f32; 3]>::try_from(c).ok()
        })
        .unwrap_or(default)
}

/// Puts the TEXCOORD sets `m`'s maps read into the shader's two UV slots
/// (locations 2 and 5) and points its maps at the slots: returns the sets,
/// [slot 0, slot 1]. Sets 0 and 1 stay where they are when used; others
/// (TEXCOORD_2 and up) take a free slot. A material reading more than two
/// sets keeps the first two (reported); its other maps read slot 0.
pub(crate) fn assign_uv_slots(m: &mut Material) -> [u32; 2] {
    let mut used: Vec<u32> = m
        .maps
        .iter()
        .filter(|(tex, _)| *tex >= 0)
        .map(|(_, set)| *set)
        .collect();
    used.sort_unstable();
    used.dedup();
    if used.iter().all(|&s| s < 2) {
        return [0, 1];
    }
    let mut slots = [u32::MAX; 2];
    for &s in used.iter().filter(|&&s| s < 2) {
        slots[s as usize] = s;
    }
    let mut dropped = false;
    for &s in used.iter().filter(|&&s| s >= 2) {
        match slots.iter_mut().find(|x| **x == u32::MAX) {
            Some(free) => *free = s,
            None => dropped = true,
        }
    }
    if dropped {
        rswebgl::console::warn_once(
            "[rswebgl-gltf] a material reads more than two TEXCOORD sets; the shader has \
             two: maps on the others read the first",
        );
    }
    let slots = slots.map(|s| if s == u32::MAX { 0 } else { s });
    for (tex, set) in &mut m.maps {
        if *tex >= 0 {
            *set = slots.iter().position(|&s| s == *set).unwrap_or(0) as u32;
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    use rswebgl::render_state::{BlendFactor, FrontFace};

    // Three materials covering opaque/blend/mask + double-sided + factors.
    const DOC: &str = r#"{
        "asset": { "version": "2.0" },
        "materials": [
            { "name": "opaque" },
            {
                "name": "blend",
                "alphaMode": "BLEND",
                "doubleSided": true,
                "pbrMetallicRoughness": {
                    "baseColorFactor": [0.1, 0.2, 0.3, 0.5],
                    "metallicFactor": 0.25,
                    "roughnessFactor": 0.75
                }
            },
            { "name": "mask", "alphaMode": "MASK", "alphaCutoff": 0.25 },
            {
                "name": "gem",
                "extensions": {
                    "KHR_materials_ior": { "ior": 2.42 },
                    "KHR_materials_specular": { "specularFactor": 0.5, "specularColorFactor": [1, 0.5, 0.25] },
                    "KHR_materials_transmission": { "transmissionFactor": 1 },
                    "KHR_materials_volume": { "thicknessFactor": 0.3, "attenuationDistance": 2, "attenuationColor": [0.9, 0.8, 0.7] },
                    "KHR_materials_clearcoat": { "clearcoatFactor": 0.75, "clearcoatRoughnessFactor": 0.1 },
                    "KHR_materials_dispersion": { "dispersion": 0.4 }
                }
            }
        ]
    }"#;

    fn materials() -> Vec<Material> {
        let g = gltf::Gltf::from_slice(DOC.as_bytes()).expect("parse");
        g.document.materials().map(|m| translate(&m)).collect()
    }

    #[test]
    fn opaque_defaults() {
        let m = &materials()[0];
        assert_eq!(m.alpha_mode, AlphaMode::Opaque);
        assert!(!m.double_sided);
        assert_eq!(m.base_color_factor, [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(m.metallic, 1.0);
        assert_eq!(m.roughness, 1.0);
        assert_eq!(m.alpha_cutoff, 0.5); // glTF default

        let s = render_state_for(m, false);
        assert!(s.depth_test);
        assert!(s.cull_face); // single-sided culls
        assert_eq!(s.front_face, FrontFace::Ccw);
        assert!(!s.blend);
        assert!(s.depth_mask);
    }

    #[test]
    fn blend_double_sided_and_mirror() {
        let m = &materials()[1];
        assert_eq!(m.alpha_mode, AlphaMode::Blend);
        assert!(m.double_sided);
        assert_eq!(m.base_color_factor, [0.1, 0.2, 0.3, 0.5]);
        assert_eq!(m.metallic, 0.25);
        assert_eq!(m.roughness, 0.75);

        let s = render_state_for(m, false);
        assert!(!s.cull_face); // double-sided: no culling
        assert!(s.blend);
        assert_eq!(s.blend_src_rgb, BlendFactor::SrcAlpha);
        assert_eq!(s.blend_dst_rgb, BlendFactor::OneMinusSrcAlpha);
        assert!(!s.depth_mask); // transparent: don't write depth

        // A mirroring (negative-determinant) node flips the front face to CW.
        let mirrored = render_state_for(m, true);
        assert_eq!(mirrored.front_face, FrontFace::Cw);
    }

    #[test]
    fn extension_factors() {
        let plain = &materials()[0];
        assert_eq!(plain.ior, 1.5);
        assert_eq!((plain.specular, plain.specular_color), (1.0, [1.0; 3]));
        assert_eq!(
            (plain.transmission, plain.thickness, plain.clearcoat),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(plain.attenuation_distance, 0.0, "infinite = none");
        assert_eq!(plain.dispersion, 0.0);

        let gem = &materials()[3];
        assert_eq!(gem.ior, 2.42);
        assert_eq!((gem.specular, gem.specular_color), (0.5, [1.0, 0.5, 0.25]));
        assert_eq!(gem.transmission, 1.0);
        assert_eq!(gem.thickness, 0.3);
        assert_eq!(gem.attenuation_distance, 2.0);
        assert_eq!(gem.attenuation_color, [0.9, 0.8, 0.7]);
        assert_eq!((gem.clearcoat, gem.clearcoat_roughness), (0.75, 0.1));
        assert_eq!(gem.dispersion, 0.4);
    }

    #[test]
    fn properties_by_gltf_name() {
        let mut m = materials()[1].clone();
        assert_eq!(m.name.as_deref(), Some("blend"));
        assert_eq!(
            m.property("baseColorFactor"),
            Some(vec![0.1, 0.2, 0.3, 0.5])
        );
        assert_eq!(m.property("roughnessFactor"), Some(vec![0.75]));
        assert_eq!(m.property("nope"), None);

        m.set_property("baseColorFactor", &[1.0, 0.8, 0.3, 1.0])
            .unwrap();
        m.set_property("transmissionFactor", &[1.0]).unwrap();
        assert_eq!(m.base_color_factor, [1.0, 0.8, 0.3, 1.0]);
        assert_eq!(m.transmission, 1.0);
        m.set_property("attenuationDistance", &[f32::INFINITY])
            .unwrap();
        assert_eq!(m.attenuation_distance, 0.0, "infinite = none");
        assert!(m.set_property("baseColorFactor", &[1.0]).is_err(), "length");
        assert!(m.set_property("nope", &[1.0]).is_err());
        // Every property reads back.
        for p in PROPERTIES {
            assert_eq!(
                m.property(p.name).map(|v| v.len()),
                Some(p.len),
                "{}",
                p.name
            );
        }
    }

    #[test]
    fn mask_cutoff() {
        let m = &materials()[2];
        assert_eq!(m.alpha_mode, AlphaMode::Mask);
        assert_eq!(m.alpha_cutoff, 0.25);
        let s = render_state_for(m, false);
        assert!(!s.blend); // MASK uses discard, not blending
        assert!(s.depth_mask);
    }

    #[test]
    fn sheen_iridescence_anisotropy_are_read() {
        let doc = r#"{
            "asset": { "version": "2.0" },
            "materials": [{
                "extensions": {
                    "KHR_materials_sheen": { "sheenColorFactor": [1, 0.5, 0.25], "sheenRoughnessFactor": 0.3 },
                    "KHR_materials_iridescence": { "iridescenceFactor": 1, "iridescenceIor": 1.8,
                                                   "iridescenceThicknessMaximum": 550 },
                    "KHR_materials_anisotropy": { "anisotropyStrength": 0.6, "anisotropyRotation": 1.5 }
                }
            }, {}]
        }"#;
        let gltf = gltf::Gltf::from_slice(doc.as_bytes()).unwrap();
        let mats: Vec<Material> = gltf.document.materials().map(|m| translate(&m)).collect();
        let m = &mats[0];
        assert_eq!(m.sheen_color, [1.0, 0.5, 0.25]);
        assert_eq!(m.sheen_roughness, 0.3);
        assert_eq!(
            (m.iridescence, m.iridescence_ior, m.iridescence_thickness),
            (1.0, 1.8, 550.0)
        );
        assert_eq!((m.anisotropy_strength, m.anisotropy_rotation), (0.6, 1.5));
        // Absent: the extensions' defaults.
        let d = &mats[1];
        assert_eq!(d.sheen_color, [0.0; 3]);
        assert_eq!(
            (d.iridescence, d.iridescence_ior, d.iridescence_thickness),
            (0.0, 1.3, 400.0)
        );
        assert_eq!(d.anisotropy_strength, 0.0);
    }

    #[test]
    fn extension_maps_and_texture_transforms_are_read() {
        let doc = r#"{
            "asset": { "version": "2.0" },
            "textures": [{}, {}, {}],
            "materials": [{
                "normalTexture": { "index": 1, "extensions": { "KHR_texture_transform":
                    { "offset": [0.5, 0.25], "scale": [2, 3], "texCoord": 1 } } },
                "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } },
                "extensions": {
                    "KHR_materials_clearcoat": { "clearcoatFactor": 1,
                        "clearcoatTexture": { "index": 2, "texCoord": 1 },
                        "clearcoatNormalTexture": { "index": 1, "scale": 0.5,
                            "extensions": { "KHR_texture_transform": { "rotation": 1.5 } } } },
                    "KHR_materials_sheen": { "sheenColorTexture": { "index": 0 } }
                }
            }]
        }"#;
        let gltf = gltf::Gltf::from_slice_without_validation(doc.as_bytes()).unwrap();
        let mat = gltf.document.materials().next().unwrap();
        let m = translate(&mat);
        let slot = |name: &str| MAP_NAMES.iter().position(|n| *n == name).unwrap();
        assert_eq!(m.map(slot("baseColorTexture")), Some((0, 0)));
        // The transform's texCoord overrides the texture info's.
        assert_eq!(m.map(slot("normalTexture")), Some((1, 1)));
        assert_eq!(
            m.uv_transform(slot("normalTexture")),
            [0.5, 0.25, 0.0, 2.0, 3.0]
        );
        assert_eq!(m.map(slot("clearcoatTexture")), Some((2, 1)));
        assert_eq!(m.map(slot("clearcoatNormalTexture")), Some((1, 0)));
        assert_eq!(m.uv_transform(slot("clearcoatNormalTexture"))[2], 1.5);
        assert_eq!(m.clearcoat_normal_scale, 0.5);
        assert_eq!(m.map(slot("sheenColorTexture")), Some((0, 0)));
        assert_eq!(m.map(slot("anisotropyTexture")), Some((-1, 0)));
        assert!(m.has_uv_transform());
        // The sRGB set: base color and sheen color (texture 0), not 1 or 2.
        let srgb = crate::textures::srgb_texture_set(&gltf.document);
        assert!(srgb.contains(&0) && !srgb.contains(&1) && !srgb.contains(&2));
    }

    #[test]
    fn uv_matrix_is_translation_rotation_scale() {
        // Column-major; a point (1, 0) turned a quarter, scaled ×2 in u,
        // moved by (0.5, 0): (c·2, −s·2) + offset = (0.5, −2).
        let m = uv_matrix([0.5, 0.0, std::f32::consts::FRAC_PI_2, 2.0, 1.0]);
        let (u, v) = (
            m[0] * 1.0 + m[3] * 0.0 + m[6],
            m[1] * 1.0 + m[4] * 0.0 + m[7],
        );
        assert!((u - 0.5).abs() < 1e-6 && (v + 2.0).abs() < 1e-6, "{u} {v}");
        assert_eq!(
            uv_matrix(UV_IDENTITY),
            [1.0, -0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]
        );
    }

    // TEXCOORD sets past 1 take a free UV slot; the maps follow.
    #[test]
    fn uv_sets_past_one_take_a_free_slot() {
        let mut m = default_material();
        m.maps[0] = (0, 2);
        m.maps[2] = (1, 0);
        assert_eq!(assign_uv_slots(&mut m), [0, 2]);
        assert_eq!((m.maps[0], m.maps[2]), ((0, 1), (1, 0)));
        let mut plain = default_material();
        plain.maps[0] = (0, 1);
        assert_eq!(assign_uv_slots(&mut plain), [0, 1]);
        assert_eq!(plain.maps[0], (0, 1));
    }
}
