//! Translate a glTF material into a `Material` data record + a `RenderState`,
//! following the glTF 2.0 spec (§3.9 materials, alpha coverage, double-sided).
//!
//! Texture references are stored as indices into `Model::texture(i)`; `-1` means
//! "no texture". `RenderState` is derived per draw item (not per material),
//! because front-face winding depends on the node's world-transform determinant.

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
    pub(crate) normal_tex: i32,
    pub(crate) occlusion_tex: i32,
    pub(crate) emissive_tex: i32,
}

#[wasm_bindgen]
impl Material {
    #[wasm_bindgen(getter)]
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
    #[wasm_bindgen(getter)]
    pub fn emissive_factor(&self) -> Vec<f32> {
        self.emissive.to_vec()
    }
    #[wasm_bindgen(getter)]
    pub fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }
    #[wasm_bindgen(getter)]
    pub fn alpha_cutoff(&self) -> f32 {
        self.alpha_cutoff
    }
    #[wasm_bindgen(getter)]
    pub fn double_sided(&self) -> bool {
        self.double_sided
    }
    /// Index into `Model::texture(i)`, or `-1` if absent.
    #[wasm_bindgen(getter)]
    pub fn base_color_texture(&self) -> i32 {
        self.base_color_tex
    }
    /// TEXCOORD set index used by the base-color texture (`TEXCOORD_n`).
    #[wasm_bindgen(getter)]
    pub fn base_color_uv(&self) -> u32 {
        self.base_color_uv
    }
    #[wasm_bindgen(getter)]
    pub fn metallic_roughness_texture(&self) -> i32 {
        self.metallic_roughness_tex
    }
    #[wasm_bindgen(getter)]
    pub fn normal_texture(&self) -> i32 {
        self.normal_tex
    }
    #[wasm_bindgen(getter)]
    pub fn occlusion_texture(&self) -> i32 {
        self.occlusion_tex
    }
    #[wasm_bindgen(getter)]
    pub fn emissive_texture(&self) -> i32 {
        self.emissive_tex
    }
}

/// Translate one glTF material's data.
pub fn translate(mat: &gltf::Material) -> Material {
    let pbr = mat.pbr_metallic_roughness();
    Material {
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
        base_color_uv: pbr.base_color_texture().map(|i| i.tex_coord()).unwrap_or(0),
        metallic_roughness_tex: tex_index(pbr.metallic_roughness_texture().map(|i| i.texture())),
        normal_tex: tex_index(mat.normal_texture().map(|i| i.texture())),
        occlusion_tex: tex_index(mat.occlusion_texture().map(|i| i.texture())),
        emissive_tex: tex_index(mat.emissive_texture().map(|i| i.texture())),
    }
}

/// The glTF default material (used for primitives that reference none).
pub fn default_material() -> Material {
    Material {
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
        normal_tex: -1,
        occlusion_tex: -1,
        emissive_tex: -1,
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
            { "name": "mask", "alphaMode": "MASK", "alphaCutoff": 0.25 }
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
    fn mask_cutoff() {
        let m = &materials()[2];
        assert_eq!(m.alpha_mode, AlphaMode::Mask);
        assert_eq!(m.alpha_cutoff, 0.25);
        let s = render_state_for(m, false);
        assert!(!s.blend); // MASK uses discard, not blending
        assert!(s.depth_mask);
    }
}
