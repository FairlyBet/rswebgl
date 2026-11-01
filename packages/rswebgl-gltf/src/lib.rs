//! Translate glTF 2.0 assets into rswebgl resources (vertex arrays, textures,
//! material parameter groups, interned render states) and a flat list of what
//! to draw where.
//!
//! This is a translator built only on rswebgl's public API — it does not own a
//! renderer, shader, or scene graph. The output `Model` is a flat draw list
//! (primitive × instance, each with its world matrix) meant to feed whatever
//! owns the scene. GPU data is shared wherever the document shares it: one
//! buffer per accessor, one set of vertex arrays per mesh however many nodes
//! (or `EXT_mesh_gpu_instancing` instances) use it, one texture per image and
//! colour space, one sampler object per distinct glTF sampler.
//!
//! Loading is async (it fetches external buffers/images); textures fill in
//! after the call returns.
//!
//! ```ignore
//! let model = rswebgl_gltf::load_model(&ctx, "./model.glb").await?;
//! for item in model.items() {
//!     pass.push(program, item.state(), item.vao(),
//!               frame, item.material_group(), object_group_for(item), NULL,
//!               &item.draw_command());
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
use rswebgl::error::Error;
use rswebgl::ids::{
    BufferId, ParamGroupId, ParamLayoutId, SamplerId, StateId, TextureId, VertexArrayId,
};
use rswebgl::param::ParamLayoutBuilder;
use rswebgl::render_state::RenderState;
use rswebgl::uniform_buffer::UniformType;
use wasm_bindgen::prelude::*;

/// Extensions this crate handles itself; a file that *requires* one of them
/// still loads (the `gltf` crate would reject it as unsupported).
const HANDLED_EXTENSIONS: &[&str] = &["EXT_mesh_gpu_instancing"];

/// One thing to draw: a primitive of a mesh, at one instance of it. Items of
/// the same mesh share their vertex array.
#[wasm_bindgen]
#[derive(Clone)]
pub struct DrawItem {
    pub(crate) vao: VertexArrayId,
    pub(crate) draw: DrawCommand,
    pub(crate) state: RenderState,
    pub(crate) state_id: StateId,
    pub(crate) material: i32, // index into Model materials, or -1 (default)
    pub(crate) material_group: ParamGroupId,
    pub(crate) mesh: u32,
    pub(crate) node: u32,
    pub(crate) instance: u32,
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
    /// The interned render state, ready for `Pass::push`.
    pub fn state(&self) -> StateId {
        self.state_id
    }
    /// The render state itself, to inspect (e.g. `blend` for sorting).
    #[wasm_bindgen(js_name = renderState)]
    pub fn render_state(&self) -> RenderState {
        self.state.clone()
    }
    /// Index into `Model::material(i)`, or `-1` for the default material.
    #[wasm_bindgen(js_name = materialIndex)]
    pub fn material_index(&self) -> i32 {
        self.material
    }
    /// The parameter group holding this item's material (see `Model` for the
    /// uniform names). Put it in a group slot, typically 1.
    #[wasm_bindgen(js_name = materialGroup)]
    pub fn material_group(&self) -> ParamGroupId {
        self.material_group
    }
    /// glTF mesh index. Items with the same mesh share geometry.
    pub fn mesh(&self) -> u32 {
        self.mesh
    }
    /// glTF node index this item came from.
    pub fn node(&self) -> u32 {
        self.node
    }
    /// Which `EXT_mesh_gpu_instancing` instance of the node (0 without it).
    pub fn instance(&self) -> u32 {
        self.instance
    }
    /// World (model) matrix, 16 floats column-major.
    #[wasm_bindgen(js_name = modelMatrix)]
    pub fn model_matrix(&self) -> Vec<f32> {
        self.model.to_vec()
    }
}

impl DrawItem {
    /// World matrix as a fixed array (Rust convenience; wasm uses
    /// `model_matrix()`).
    pub fn model_cols(&self) -> [f32; 16] {
        self.model
    }
}

/// A translated glTF asset: a flat draw list plus the GPU resources behind it.
///
/// Each material is a parameter group (layout `material_layout()`) with, by
/// convention: `u_base_color_factor` (vec4), `u_base_color_tex` (sampler) +
/// `u_has_base_color_tex` (bool), `u_alpha_cutoff` (float), `u_alpha_mode`
/// (int: 0=OPAQUE 1=MASK 2=BLEND). The transform is the caller's job (it
/// depends on the camera) — use `DrawItem::model_cols()`.
///
/// The model owns its resources; `destroy` frees them all.
#[wasm_bindgen]
pub struct Model {
    pub(crate) items: Vec<DrawItem>,
    pub(crate) materials: Vec<Material>,
    pub(crate) material_layout: ParamLayoutId,
    // One per material, plus the default material's last.
    pub(crate) material_groups: Vec<ParamGroupId>,
    pub(crate) textures: Vec<TextureId>,
    pub(crate) samplers: Vec<SamplerId>,
    pub(crate) vertex_arrays: Vec<VertexArrayId>,
    pub(crate) buffers: Vec<BufferId>,
    pub(crate) mesh_count: usize,
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
    /// Distinct meshes on the GPU (each drawn by one or more items).
    #[wasm_bindgen(js_name = meshCount)]
    pub fn mesh_count(&self) -> usize {
        self.mesh_count
    }
    /// Vertex arrays created: one per primitive of each distinct mesh.
    #[wasm_bindgen(js_name = vertexArrayCount)]
    pub fn vertex_array_count(&self) -> usize {
        self.vertex_arrays.len()
    }
    /// Samplers created: one per distinct glTF sampler.
    #[wasm_bindgen(js_name = samplerCount)]
    pub fn sampler_count(&self) -> usize {
        self.samplers.len()
    }
    /// GPU buffers created: one per distinct accessor.
    #[wasm_bindgen(js_name = bufferCount)]
    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
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

    /// Destroys every GPU resource the model created: vertex arrays, buffers,
    /// textures (a texture still loading is dropped when its image arrives),
    /// material groups and their layout. Handles from this model are stale
    /// afterwards.
    pub fn destroy(&self, ctx: &Context) {
        for &vao in &self.vertex_arrays {
            ctx.destroy_vertex_array(vao);
        }
        for &buf in &self.buffers {
            ctx.destroy_buffer(buf);
        }
        for &tex in &self.textures {
            ctx.destroy_texture(tex);
        }
        for &s in &self.samplers {
            ctx.destroy_sampler(s);
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

    /// The GPU buffers the model created (one per distinct accessor).
    pub fn buffers(&self) -> &[BufferId] {
        &self.buffers
    }
}

/// `loadModel(ctx, url)` for JS: a promise of a `Model`. (An exported async fn
/// can't borrow the context across `await`; this clones it first.)
#[wasm_bindgen(js_name = loadModel)]
pub fn load_model_js(ctx: &Context, url: String) -> js_sys::Promise {
    let ctx = ctx.clone();
    wasm_bindgen_futures::future_to_promise(async move {
        load_model(&ctx, &url)
            .await
            .map(JsValue::from)
            .map_err(JsValue::from)
    })
}

/// Load and translate a glTF/GLB asset at `url` into GPU resources.
///
/// Geometry, draw commands, render state and node transforms are ready when this
/// resolves; textures are created empty and fill in asynchronously as their
/// images decode (sampling one before it arrives shows black, then it pops in).
pub async fn load_model(ctx: &Context, url: &str) -> Result<Model, Error> {
    let bytes = fetch::fetch_bytes(url).await.map_err(js_err)?;
    let (document, blob) = parse(&bytes)?;

    let base = fetch::absolute_base(url).map_err(js_err)?;
    let buffers = buffers::resolve_buffers(&document, blob, &base)
        .await
        .map_err(js_err)?;

    // Assemble textures, packing separate occlusion + metallic-roughness maps
    // into one ORM texture where safe; `remap` rewrites material texture indices
    // to match (a packed pair, or two textures with the same image and sampler,
    // collapse into one).
    let set = pack::build_texture_set(ctx, &document, &buffers, &base);
    let mut materials: Vec<Material> = document
        .materials()
        .map(|m| material::translate(&m))
        .collect();
    // Samplers follow the document texture, so pick them before remapping.
    let sampler_of = |doc_tex: i32| {
        usize::try_from(doc_tex)
            .ok()
            .and_then(|i| set.doc_samplers.get(i).copied())
            .unwrap_or(SamplerId::NULL)
    };
    let base_color_samplers: Vec<SamplerId> = materials
        .iter()
        .map(|m| sampler_of(m.base_color_tex))
        .collect();
    for m in &mut materials {
        material::remap_textures(m, &set.remap);
    }
    let textures = set.textures;
    let mut items = Vec::new();
    let mut geometry = geometry::GeometryCache::default();
    let (center, radius) = scene::build_draw_list(
        ctx,
        &document,
        &buffers,
        &mut geometry,
        &materials,
        &mut items,
    );

    let (material_layout, material_groups) =
        build_material_groups(ctx, &materials, &base_color_samplers, &textures)?;
    for item in &mut items {
        let i = usize::try_from(item.material).unwrap_or(materials.len());
        item.material_group = material_groups[i];
    }

    Ok(Model {
        items,
        materials,
        material_layout,
        material_groups,
        textures,
        samplers: set.samplers,
        vertex_arrays: geometry.vertex_arrays().collect(),
        buffers: geometry.buffers().collect(),
        mesh_count: geometry.mesh_count(),
        center,
        radius,
    })
}

// A .gltf / .glb file → the validated document and the GLB binary chunk.
// Extensions we handle are dropped from `extensionsRequired` before
// validation, which would otherwise reject them as unsupported.
fn parse(bytes: &[u8]) -> Result<(gltf::Document, Option<Vec<u8>>), Error> {
    let (json, blob) = if bytes.starts_with(b"glTF") {
        let glb = gltf::Glb::from_slice(bytes).map_err(|e| format!("GLB parse failed: {e}"))?;
        (glb.json.into_owned(), glb.bin.map(|b| b.into_owned()))
    } else {
        (bytes.to_vec(), None)
    };
    let mut root =
        gltf::json::Root::from_slice(&json).map_err(|e| format!("glTF parse failed: {e}"))?;
    root.extensions_required
        .retain(|e| !HANDLED_EXTENSIONS.contains(&e.as_str()));
    let document = gltf::Document::from_json(root).map_err(|e| format!("glTF invalid: {e}"))?;
    Ok((document, blob))
}

fn js_err(e: JsValue) -> Error {
    e.as_string().unwrap_or_else(|| format!("{e:?}")).into()
}

// One parameter group per material (plus the default material's, last).
fn build_material_groups(
    ctx: &Context,
    materials: &[Material],
    base_color_samplers: &[SamplerId],
    textures: &[TextureId],
) -> Result<(ParamLayoutId, Vec<ParamGroupId>), Error> {
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
    for (i, m) in materials
        .iter()
        .chain(std::iter::once(&default))
        .enumerate()
    {
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
            let sampler = base_color_samplers
                .get(i)
                .copied()
                .unwrap_or(SamplerId::NULL);
            ctx.param_set_sampler(g, f_tex, sampler);
        }
        groups.push(g);
    }
    Ok((layout, groups))
}
