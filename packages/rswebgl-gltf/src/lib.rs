//! Translate glTF 2.0 assets into rswebgl primitives (VAOs, textures, draw
//! commands, render state, per-node transforms).
//!
//! This is a thin, specialized translator built only on rswebgl's public API —
//! it does not own a renderer, shader, or scene graph. The output `Model` is a
//! flat draw list you iterate to render; it can later feed a real material/scene
//! system. Loading is async (it fetches external buffers/images); textures fill
//! in after the call returns.
//!
//! ```ignore
//! let model = rswebgl_gltf::load_model(&ctx, "./model.glb").await?;
//! for i in 0..model.draw_item_count() {
//!     let item = model.draw_item(i).unwrap();
//!     // set MVP from item.model_cols(), then:
//!     item.write_uniforms(&model, &uniforms);
//!     // draw item.vao() with item.draw_command() under item.render_state()
//! }
//! ```

mod buffers;
mod fetch;
mod geometry;
mod material;
mod pack;
mod scene;
mod textures;

pub use material::{AlphaMode, Material};

use rswebgl::context::Context;
use rswebgl::draw::DrawCommand;
use rswebgl::render_state::RenderState;
use rswebgl::texture::Texture;
use rswebgl::uniform_values::UniformValues;
use rswebgl::vao::VertexArray;
use wasm_bindgen::prelude::*;

/// One renderable primitive instance: its geometry, how to draw it, the
/// fixed-function state it expects, its material, and its world transform.
#[wasm_bindgen]
#[derive(Clone)]
pub struct DrawItem {
    pub(crate) vao: VertexArray,
    pub(crate) draw: DrawCommand,
    pub(crate) state: RenderState,
    pub(crate) material: i32, // index into Model materials, or -1 (default)
    pub(crate) model: [f32; 16], // world matrix, column-major
}

#[wasm_bindgen]
impl DrawItem {
    pub fn vao(&self) -> VertexArray {
        self.vao.clone()
    }
    pub fn draw_command(&self) -> DrawCommand {
        self.draw
    }
    pub fn render_state(&self) -> RenderState {
        self.state.clone()
    }
    /// Index into `Model::material(i)`, or `-1` for the default material.
    pub fn material_index(&self) -> i32 {
        self.material
    }
    /// World (model) matrix, 16 floats column-major.
    pub fn model_matrix(&self) -> Vec<f32> {
        self.model.to_vec()
    }

    /// Fill `uniforms` with this item's material, by a documented convention:
    /// `u_base_color_factor` (vec4), `u_base_color_tex` (sampler) +
    /// `u_has_base_color_tex` (bool), `u_alpha_cutoff` (float), `u_alpha_mode`
    /// (int: 0=OPAQUE 1=MASK 2=BLEND). The MVP/model matrix is the caller's job
    /// (it depends on the camera) — use `model_matrix()` / `model_cols()`.
    pub fn write_uniforms(&self, model: &Model, uniforms: &UniformValues) {
        let m = self.material_of(model);
        let c = m.base_color_factor;
        uniforms.set_vec4("u_base_color_factor", c[0], c[1], c[2], c[3]);
        uniforms.set_float("u_alpha_cutoff", m.alpha_cutoff);
        uniforms.set_int("u_alpha_mode", m.alpha_mode as i32);
        if m.base_color_tex >= 0 {
            if let Some(tex) = model.textures.get(m.base_color_tex as usize) {
                uniforms.set_sampler("u_base_color_tex", tex);
            }
            uniforms.set_bool("u_has_base_color_tex", true);
        } else {
            uniforms.set_bool("u_has_base_color_tex", false);
        }
    }
}

impl DrawItem {
    /// World matrix as a fixed array (Rust convenience; wasm uses
    /// `model_matrix()`).
    pub fn model_cols(&self) -> [f32; 16] {
        self.model
    }

    fn material_of(&self, model: &Model) -> Material {
        if self.material >= 0
            && let Some(m) = model.materials.get(self.material as usize)
        {
            return m.clone();
        }
        material::default_material()
    }
}

/// A translated glTF asset: a flat draw list plus its materials and textures.
#[wasm_bindgen]
pub struct Model {
    pub(crate) items: Vec<DrawItem>,
    pub(crate) materials: Vec<Material>,
    pub(crate) textures: Vec<Texture>,
    pub(crate) center: [f32; 3],
    pub(crate) radius: f32,
}

#[wasm_bindgen]
impl Model {
    pub fn draw_item_count(&self) -> usize {
        self.items.len()
    }
    pub fn draw_item(&self, i: usize) -> Option<DrawItem> {
        self.items.get(i).cloned()
    }
    pub fn material_count(&self) -> usize {
        self.materials.len()
    }
    pub fn material(&self, i: usize) -> Option<Material> {
        self.materials.get(i).cloned()
    }
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }
    pub fn texture(&self, i: usize) -> Option<Texture> {
        self.textures.get(i).cloned()
    }
    /// World-space center of the model's bounding sphere (3 floats).
    pub fn center(&self) -> Vec<f32> {
        self.center.to_vec()
    }
    /// World-space radius of the model's bounding sphere.
    pub fn radius(&self) -> f32 {
        self.radius
    }
}

impl Model {
    /// All draw items (Rust convenience; JS uses `draw_item_count`/`draw_item`).
    pub fn items(&self) -> &[DrawItem] {
        &self.items
    }
}

/// Load and translate a glTF/GLB asset at `url` into GPU primitives.
///
/// Geometry, draw commands, render state and node transforms are ready when this
/// resolves; textures are created empty and fill in asynchronously as their
/// images decode (sampling one before it arrives shows black, then it pops in).
///
/// Not exported to JS directly: an exported async entry would need to hold the
/// `Context` borrow across `await`, which wasm-bindgen disallows. Call it from
/// Rust (e.g. `wasm_bindgen_futures::spawn_local`); a JS-facing wrapper that
/// takes the context by value can be added later.
pub async fn load_model(ctx: &Context, url: &str) -> Result<Model, String> {
    let bytes = fetch::fetch_bytes(url).await.map_err(js_err)?;
    let g = gltf::Gltf::from_slice(&bytes).map_err(|e| format!("glTF parse failed: {e}"))?;
    let document = g.document;
    let blob = g.blob;

    let base = fetch::absolute_base(url).map_err(js_err)?;
    let buffers = buffers::resolve_buffers(&document, blob, &base)
        .await
        .map_err(js_err)?;

    // Assemble textures, packing separate occlusion + metallic-roughness maps
    // into one ORM texture where safe; `remap` rewrites material texture indices
    // to match (a packed pair collapses two indices into one).
    let (textures, remap) = pack::build_texture_set(ctx, &document, &buffers, &base);
    let mut materials: Vec<Material> = document
        .materials()
        .map(|m| material::translate(&m))
        .collect();
    for m in &mut materials {
        material::remap_textures(m, &remap);
    }
    let mut items = Vec::new();
    let (center, radius) = scene::build_draw_list(ctx, &document, &buffers, &materials, &mut items);

    Ok(Model {
        items,
        materials,
        textures,
        center,
        radius,
    })
}

fn js_err(e: JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}
