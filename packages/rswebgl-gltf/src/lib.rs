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
//!     // the material's parameters are a ready-made group:
//!     batch.set_group(1, item.material_group());
//!     // set the transform from item.model_cols() in your own group, then draw
//!     // item.vao() with item.draw_command() under item.render_state()
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
use rswebgl::ids::{ParamGroupId, ParamLayoutId, TextureId, VertexArrayId};
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::render_state::RenderState;
use rswebgl::uniform_buffer::UniformType;
use wasm_bindgen::prelude::*;

/// One renderable primitive instance: its geometry, how to draw it, the
/// fixed-function state it expects, its material, and its world transform.
#[wasm_bindgen]
#[derive(Clone)]
pub struct DrawItem {
    pub(crate) vao: VertexArrayId,
    pub(crate) draw: DrawCommand,
    pub(crate) state: RenderState,
    pub(crate) material: i32, // index into Model materials, or -1 (default)
    pub(crate) material_group: ParamGroupId,
    pub(crate) model: [f32; 16], // world matrix, column-major
}

#[wasm_bindgen]
impl DrawItem {
    pub fn vao(&self) -> VertexArrayId {
        self.vao
    }
    #[wasm_bindgen(js_name = drawCommand)]
    pub fn draw_command(&self) -> DrawCommand {
        self.draw
    }
    #[wasm_bindgen(js_name = renderState)]
    pub fn render_state(&self) -> RenderState {
        self.state.clone()
    }
    /// Index into `Model::material(i)`, or `-1` for the default material.
    #[wasm_bindgen(js_name = materialIndex)]
    pub fn material_index(&self) -> i32 {
        self.material
    }
    /// World (model) matrix, 16 floats column-major.
    #[wasm_bindgen(js_name = modelMatrix)]
    pub fn model_matrix(&self) -> Vec<f32> {
        self.model.to_vec()
    }

    /// The parameter group holding this item's material (see `Model` for the
    /// uniform names). Put it in a batch slot, typically 1.
    #[wasm_bindgen(js_name = materialGroup)]
    pub fn material_group(&self) -> ParamGroupId {
        self.material_group
    }
}

impl DrawItem {
    /// World matrix as a fixed array (Rust convenience; wasm uses
    /// `model_matrix()`).
    pub fn model_cols(&self) -> [f32; 16] {
        self.model
    }
}

/// A translated glTF asset: a flat draw list plus its materials and textures.
///
/// Each material is a parameter group (layout `material_layout()`) with, by
/// convention: `u_base_color_factor` (vec4), `u_base_color_tex` (sampler) +
/// `u_has_base_color_tex` (bool), `u_alpha_cutoff` (float), `u_alpha_mode`
/// (int: 0=OPAQUE 1=MASK 2=BLEND). The transform is the caller's job (it
/// depends on the camera) — use `DrawItem::model_cols()`.
#[wasm_bindgen]
pub struct Model {
    pub(crate) items: Vec<DrawItem>,
    pub(crate) materials: Vec<Material>,
    pub(crate) material_layout: ParamLayoutId,
    // One per material, plus the default material's last.
    pub(crate) material_groups: Vec<ParamGroupId>,
    pub(crate) textures: Vec<TextureId>,
    pub(crate) center: [f32; 3],
    pub(crate) radius: f32,
}

#[wasm_bindgen]
impl Model {
    #[wasm_bindgen(js_name = drawItemCount)]
    pub fn draw_item_count(&self) -> usize {
        self.items.len()
    }
    #[wasm_bindgen(js_name = drawItem)]
    pub fn draw_item(&self, i: usize) -> Option<DrawItem> {
        self.items.get(i).cloned()
    }
    #[wasm_bindgen(js_name = materialCount)]
    pub fn material_count(&self) -> usize {
        self.materials.len()
    }
    pub fn material(&self, i: usize) -> Option<Material> {
        self.materials.get(i).cloned()
    }
    #[wasm_bindgen(js_name = textureCount)]
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }
    /// The texture at `i`, or `TextureId::NULL` if out of range.
    pub fn texture(&self, i: usize) -> TextureId {
        self.textures.get(i).copied().unwrap_or(TextureId::NULL)
    }
    /// World-space center of the model's bounding sphere (3 floats).
    pub fn center(&self) -> Vec<f32> {
        self.center.to_vec()
    }
    /// World-space radius of the model's bounding sphere.
    pub fn radius(&self) -> f32 {
        self.radius
    }

    /// The layout of the material groups.
    #[wasm_bindgen(js_name = materialLayout)]
    pub fn material_layout(&self) -> ParamLayoutId {
        self.material_layout
    }

    /// Destroys every GPU resource the model created: vertex arrays (with their
    /// buffers), textures, material groups and their layout. Handles from this
    /// model are stale afterwards.
    pub fn destroy(&self, ctx: &Context) {
        for item in &self.items {
            ctx.destroy_vertex_array(item.vao);
        }
        for &tex in &self.textures {
            ctx.destroy_texture(tex);
        }
        for &g in &self.material_groups {
            ctx.destroy_param_group(g);
        }
        ctx.destroy_param_layout(self.material_layout);
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

    let (material_layout, material_groups) = build_material_groups(ctx, &materials, &textures)?;
    for item in &mut items {
        let i = if item.material >= 0 {
            item.material as usize
        } else {
            materials.len()
        };
        item.material_group = material_groups[i];
    }

    Ok(Model {
        items,
        materials,
        material_layout,
        material_groups,
        textures,
        center,
        radius,
    })
}

fn js_err(e: JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

// One parameter group per material (plus the default material's, last).
fn build_material_groups(
    ctx: &Context,
    materials: &[Material],
    textures: &[TextureId],
) -> Result<(ParamLayoutId, Vec<ParamGroupId>), String> {
    let layout = ctx.create_param_layout(
        &ParamLayoutBuilder::new()
            .uniform("u_base_color_factor", UniformType::Vec4)
            .uniform("u_alpha_cutoff", UniformType::F32)
            .uniform("u_alpha_mode", UniformType::I32)
            .uniform("u_has_base_color_tex", UniformType::Bool)
            .texture("u_base_color_tex"),
    )?;
    let f_color = ctx.param_field(layout, "u_base_color_factor")?;
    let f_cutoff = ctx.param_field(layout, "u_alpha_cutoff")?;
    let f_mode = ctx.param_field(layout, "u_alpha_mode")?;
    let f_has_tex = ctx.param_field(layout, "u_has_base_color_tex")?;
    let f_tex = ctx.param_field(layout, "u_base_color_tex")?;

    let default = material::default_material();
    let mut groups = Vec::with_capacity(materials.len() + 1);
    for m in materials.iter().chain(std::iter::once(&default)) {
        let g = ctx.create_param_group(layout)?;
        ctx.param_set_floats(g, f_color, &m.base_color_factor);
        ctx.param_set_f32(g, f_cutoff, m.alpha_cutoff);
        ctx.param_set_i32(g, f_mode, m.alpha_mode as i32);
        let tex = usize::try_from(m.base_color_tex)
            .ok()
            .and_then(|i| textures.get(i).copied());
        ctx.param_set_bool(g, f_has_tex, tex.is_some());
        if let Some(tex) = tex {
            ctx.param_set_texture(g, f_tex, tex);
        }
        groups.push(g);
    }
    Ok((layout, groups))
}
