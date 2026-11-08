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
    pub(crate) base_color_tex: i32,
    pub(crate) base_color_uv: u32,
    pub(crate) metallic_roughness_tex: i32,
    pub(crate) metallic_roughness_uv: u32,
    pub(crate) normal_tex: i32,
    pub(crate) normal_uv: u32,
    pub(crate) normal_scale: f32,
    pub(crate) occlusion_tex: i32,
    pub(crate) occlusion_uv: u32,
    pub(crate) occlusion_strength: f32,
    pub(crate) emissive_tex: i32,
    pub(crate) emissive_uv: u32,
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
        self.base_color_tex
    }
    /// TEXCOORD set index used by the base-color texture (`TEXCOORD_n`).
    #[wasm_bindgen(getter, js_name = baseColorUv)]
    pub fn base_color_uv(&self) -> u32 {
        self.base_color_uv
    }
    #[wasm_bindgen(getter, js_name = metallicRoughnessTexture)]
    pub fn metallic_roughness_texture(&self) -> i32 {
        self.metallic_roughness_tex
    }
    #[wasm_bindgen(getter, js_name = normalTexture)]
    pub fn normal_texture(&self) -> i32 {
        self.normal_tex
    }
    #[wasm_bindgen(getter, js_name = occlusionTexture)]
    pub fn occlusion_texture(&self) -> i32 {
        self.occlusion_tex
    }
    #[wasm_bindgen(getter, js_name = emissiveTexture)]
    pub fn emissive_texture(&self) -> i32 {
        self.emissive_tex
    }
    /// TEXCOORD set of each map (`TEXCOORD_n`); 0 when the map is absent.
    #[wasm_bindgen(getter, js_name = metallicRoughnessUv)]
    pub fn metallic_roughness_uv(&self) -> u32 {
        self.metallic_roughness_uv
    }
    #[wasm_bindgen(getter, js_name = normalUv)]
    pub fn normal_uv(&self) -> u32 {
        self.normal_uv
    }
    #[wasm_bindgen(getter, js_name = occlusionUv)]
    pub fn occlusion_uv(&self) -> u32 {
        self.occlusion_uv
    }
    #[wasm_bindgen(getter, js_name = emissiveUv)]
    pub fn emissive_uv(&self) -> u32 {
        self.emissive_uv
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
pub const PROPERTIES: [Property; 30] = [
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
    prop("sheenColorFactor", "u_sheen_color", 3),
    prop("sheenRoughnessFactor", "u_sheen_roughness", 1),
    prop("iridescenceFactor", "u_iridescence", 1),
    prop("iridescenceIor", "u_iridescence_ior", 1),
    prop("iridescenceThickness", "u_iridescence_thickness", 1),
    prop("anisotropyStrength", "u_anisotropy_strength", 1),
    prop("anisotropyRotation", "u_anisotropy_rotation", 1),
];

impl Material {
    // The storage of each of `PROPERTIES`, in the same order.
    fn values_mut(&mut self) -> [&mut [f32]; 30] {
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
            &mut self.sheen_color,
            std::slice::from_mut(&mut self.sheen_roughness),
            std::slice::from_mut(&mut self.iridescence),
            std::slice::from_mut(&mut self.iridescence_ior),
            std::slice::from_mut(&mut self.iridescence_thickness),
            std::slice::from_mut(&mut self.anisotropy_strength),
            std::slice::from_mut(&mut self.anisotropy_rotation),
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

/// The material maps by glTF name, in the order of `MAP_UNIFORMS`.
pub const MAP_NAMES: [&str; 5] = [
    "baseColorTexture",
    "metallicRoughnessTexture",
    "normalTexture",
    "occlusionTexture",
    "emissiveTexture",
];

/// The texture index of a map whose texture isn't one of the model's (set by
/// the app — a scene's `setMaterialTexture`).
pub const EXTERNAL_TEXTURE: i32 = i32::MAX;

impl Material {
    // Each map's (texture index, TEXCOORD set), in `MAP_NAMES` order.
    fn maps_mut(&mut self) -> [(&mut i32, &mut u32); 5] {
        [
            (&mut self.base_color_tex, &mut self.base_color_uv),
            (
                &mut self.metallic_roughness_tex,
                &mut self.metallic_roughness_uv,
            ),
            (&mut self.normal_tex, &mut self.normal_uv),
            (&mut self.occlusion_tex, &mut self.occlusion_uv),
            (&mut self.emissive_tex, &mut self.emissive_uv),
        ]
    }

    /// Map `slot` (an index into `MAP_NAMES`): its texture index (−1 none,
    /// `EXTERNAL_TEXTURE` for the app's) and TEXCOORD set.
    pub fn map(&self, slot: usize) -> Option<(i32, u32)> {
        let mut m = self.clone();
        m.maps_mut().into_iter().nth(slot).map(|(t, uv)| (*t, *uv))
    }

    /// Sets map `slot`: `texture` as for `map`, `uv` its TEXCOORD set (0 or 1).
    pub fn set_map(&mut self, slot: usize, texture: i32, uv: u32) {
        if let Some((t, u)) = self.maps_mut().into_iter().nth(slot) {
            *t = texture;
            *u = if texture < 0 { 0 } else { uv };
        }
    }
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
        base_color_tex: tex_index(pbr.base_color_texture().map(|i| i.texture())),
        base_color_uv: pbr.base_color_texture().map_or(0, |i| i.tex_coord()),
        metallic_roughness_tex: tex_index(pbr.metallic_roughness_texture().map(|i| i.texture())),
        metallic_roughness_uv: pbr
            .metallic_roughness_texture()
            .map_or(0, |i| i.tex_coord()),
        normal_tex: tex_index(mat.normal_texture().map(|i| i.texture())),
        normal_uv: mat.normal_texture().map_or(0, |i| i.tex_coord()),
        normal_scale: mat.normal_texture().map_or(1.0, |i| i.scale()),
        occlusion_tex: tex_index(mat.occlusion_texture().map(|i| i.texture())),
        occlusion_uv: mat.occlusion_texture().map_or(0, |i| i.tex_coord()),
        occlusion_strength: mat.occlusion_texture().map_or(1.0, |i| i.strength()),
        emissive_tex: tex_index(mat.emissive_texture().map(|i| i.texture())),
        emissive_uv: mat.emissive_texture().map_or(0, |i| i.tex_coord()),
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
        coat_darkening: 1.0,
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
    m.base_color_tex = map(m.base_color_tex);
    m.metallic_roughness_tex = map(m.metallic_roughness_tex);
    m.normal_tex = map(m.normal_tex);
    m.occlusion_tex = map(m.occlusion_tex);
    m.emissive_tex = map(m.emissive_tex);
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
        base_color_tex: -1,
        base_color_uv: 0,
        metallic_roughness_tex: -1,
        metallic_roughness_uv: 0,
        normal_tex: -1,
        normal_uv: 0,
        normal_scale: 1.0,
        occlusion_tex: -1,
        occlusion_uv: 0,
        occlusion_strength: 1.0,
        emissive_tex: -1,
        emissive_uv: 0,
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
        sheen_color: [0.0; 3],
        sheen_roughness: 0.0,
        iridescence: 0.0,
        iridescence_ior: 1.3,
        iridescence_thickness: 400.0,
        anisotropy_strength: 0.0,
        anisotropy_rotation: 0.0,
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

fn tex_index(t: Option<gltf::Texture>) -> i32 {
    t.map(|t| t.index() as i32).unwrap_or(-1)
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
}
