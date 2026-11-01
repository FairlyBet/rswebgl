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

pub use material::{AlphaMode, Material, default_material};

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

// Optional vertex attributes a primitive can have (`DrawItem::attributes`).
/// NORMAL at location 1.
pub const ATTR_NORMAL: u32 = 1 << 0;
/// TEXCOORD_0 at location 2.
pub const ATTR_UV0: u32 = 1 << 1;
/// TANGENT at location 3.
pub const ATTR_TANGENT: u32 = 1 << 2;
/// COLOR_0 at location 4 (always read as RGBA).
pub const ATTR_COLOR: u32 = 1 << 3;
/// TEXCOORD_1 at location 5.
pub const ATTR_UV1: u32 = 1 << 4;

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
    // The EXT_mesh_gpu_instancing transform on top of the node (identity
    // without it), column-major.
    pub(crate) instance_matrix: [f32; 16],
    pub(crate) attributes: u32,
    // The primitive's POSITION bounds, in mesh space.
    pub(crate) aabb_min: [f32; 3],
    pub(crate) aabb_max: [f32; 3],
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
    /// Optional vertex attributes present: bits 1 NORMAL, 2 TEXCOORD_0,
    /// 4 TANGENT, 8 COLOR_0, 16 TEXCOORD_1 (`ATTR_*`).
    pub fn attributes(&self) -> u32 {
        self.attributes
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

    /// The instance transform relative to the node (identity unless the node
    /// uses `EXT_mesh_gpu_instancing`): `model = node_world * instance`.
    pub fn instance_cols(&self) -> [f32; 16] {
        self.instance_matrix
    }

    /// The primitive's bounding box in mesh space: `(min, max)`.
    pub fn local_bounds(&self) -> ([f32; 3], [f32; 3]) {
        (self.aabb_min, self.aabb_max)
    }
}

/// A node of the default scene: its local transform and parent, for whoever
/// rebuilds the hierarchy (the flat draw list already has world matrices).
#[derive(Clone, Debug)]
pub struct SceneNode {
    /// glTF node index (`DrawItem::node` refers to it).
    pub index: u32,
    /// Parent's glTF node index; None for a root of the scene.
    pub parent: Option<u32>,
    /// Local transform, column-major.
    pub matrix: [f32; 16],
    pub name: Option<String>,
}

/// A translated glTF asset: a flat draw list plus the GPU resources behind it.
///
/// Each material is a parameter group (layout `material_layout()`) with, by
/// convention: `u_base_color_factor` (vec4), `u_metallic_factor`,
/// `u_roughness_factor`, `u_emissive_factor` (vec3), `u_emissive_strength`,
/// `u_normal_scale`, `u_occlusion_strength`, `u_alpha_cutoff` (floats),
/// `u_alpha_mode` (int: 0=OPAQUE 1=MASK 2=BLEND), `u_has_base_color_tex`
/// (bool), the extension factors `u_ior`, `u_specular_factor`,
/// `u_specular_color` (vec3), `u_transmission`, `u_thickness`,
/// `u_attenuation_color` (vec3), `u_attenuation_distance`, `u_clearcoat`,
/// `u_clearcoat_roughness`, `u_dispersion`, and the maps `MAP_UNIFORMS` (samplers, each with the glTF
/// sampler's state). Which maps exist and their TEXCOORD sets are on
/// `Material`; which attributes a primitive has, on `DrawItem::attributes`.
/// The transform is the caller's job (it depends on the camera) — use
/// `DrawItem::model_cols()`.
///
/// The model owns its resources; `destroy` frees them all.
#[wasm_bindgen]
pub struct Model {
    pub(crate) items: Vec<DrawItem>,
    pub(crate) nodes: Vec<SceneNode>,
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

    /// The materials, by glTF index (`DrawItem::material_index`).
    pub fn materials(&self) -> &[Material] {
        &self.materials
    }

    /// The default scene's nodes, parents before children.
    pub fn nodes(&self) -> &[SceneNode] {
        &self.nodes
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
    let map_samplers: Vec<[SamplerId; MAPS]> = materials
        .iter()
        .map(|m| material_maps(m).map(|(tex, _)| sampler_of(tex)))
        .collect();
    for m in &mut materials {
        material::remap_textures(m, &set.remap);
    }
    let textures = set.textures;
    let mut items = Vec::new();
    let mut nodes = Vec::new();
    let mut geometry = geometry::GeometryCache::default();
    let (center, radius) = scene::build_draw_list(
        ctx,
        &document,
        &buffers,
        &mut geometry,
        &materials,
        &mut items,
        &mut nodes,
    );

    let (material_layout, material_groups) =
        build_material_groups(ctx, &materials, &map_samplers, &textures)?;
    for item in &mut items {
        let i = usize::try_from(item.material).unwrap_or(materials.len());
        item.material_group = material_groups[i];
    }

    Ok(Model {
        items,
        nodes,
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

// The material maps in the order of `MAP_UNIFORMS`: (texture index or -1,
// TEXCOORD set).
const MAPS: usize = 5;
fn material_maps(m: &Material) -> [(i32, u32); MAPS] {
    [
        (m.base_color_tex, m.base_color_uv),
        (m.metallic_roughness_tex, m.metallic_roughness_uv),
        (m.normal_tex, m.normal_uv),
        (m.occlusion_tex, m.occlusion_uv),
        (m.emissive_tex, m.emissive_uv),
    ]
}

/// Sampler uniforms of the material maps, as named in a material group.
pub const MAP_UNIFORMS: [&str; MAPS] = [
    "u_base_color_tex",
    "u_metallic_roughness_tex",
    "u_normal_tex",
    "u_occlusion_tex",
    "u_emissive_tex",
];

// One parameter group per material (plus the default material's, last).
fn build_material_groups(
    ctx: &Context,
    materials: &[Material],
    map_samplers: &[[SamplerId; MAPS]],
    textures: &[TextureId],
) -> Result<(ParamLayoutId, Vec<ParamGroupId>), Error> {
    let mut builder = ParamLayoutBuilder::new()
        .uniform("u_base_color_factor", UniformType::Vec4)
        .uniform("u_metallic_factor", UniformType::F32)
        .uniform("u_roughness_factor", UniformType::F32)
        .uniform("u_emissive_factor", UniformType::Vec3)
        .uniform("u_emissive_strength", UniformType::F32)
        .uniform("u_normal_scale", UniformType::F32)
        .uniform("u_occlusion_strength", UniformType::F32)
        .uniform("u_alpha_cutoff", UniformType::F32)
        .uniform("u_alpha_mode", UniformType::I32)
        .uniform("u_has_base_color_tex", UniformType::Bool)
        .uniform("u_ior", UniformType::F32)
        .uniform("u_specular_factor", UniformType::F32)
        .uniform("u_specular_color", UniformType::Vec3)
        .uniform("u_transmission", UniformType::F32)
        .uniform("u_thickness", UniformType::F32)
        .uniform("u_attenuation_color", UniformType::Vec3)
        .uniform("u_attenuation_distance", UniformType::F32)
        .uniform("u_clearcoat", UniformType::F32)
        .uniform("u_clearcoat_roughness", UniformType::F32)
        .uniform("u_dispersion", UniformType::F32);
    for name in MAP_UNIFORMS {
        builder = builder.texture(name);
    }
    let layout = ctx.create_param_layout(&builder)?;
    let field = |name| ctx.param_field(layout, name);
    let f_color = field("u_base_color_factor")?;
    let f_metallic = field("u_metallic_factor")?;
    let f_roughness = field("u_roughness_factor")?;
    let f_emissive = field("u_emissive_factor")?;
    let f_emissive_strength = field("u_emissive_strength")?;
    let f_normal_scale = field("u_normal_scale")?;
    let f_occlusion = field("u_occlusion_strength")?;
    let f_cutoff = field("u_alpha_cutoff")?;
    let f_mode = field("u_alpha_mode")?;
    let f_has_tex = field("u_has_base_color_tex")?;
    let f_ior = field("u_ior")?;
    let f_specular = field("u_specular_factor")?;
    let f_specular_color = field("u_specular_color")?;
    let f_transmission = field("u_transmission")?;
    let f_thickness = field("u_thickness")?;
    let f_attenuation_color = field("u_attenuation_color")?;
    let f_attenuation_distance = field("u_attenuation_distance")?;
    let f_clearcoat = field("u_clearcoat")?;
    let f_clearcoat_roughness = field("u_clearcoat_roughness")?;
    let f_dispersion = field("u_dispersion")?;
    let f_maps: Vec<_> = MAP_UNIFORMS
        .iter()
        .map(|name| field(name))
        .collect::<Result<_, _>>()?;

    let default = material::default_material();
    let mut groups = Vec::with_capacity(materials.len() + 1);
    for (i, m) in materials
        .iter()
        .chain(std::iter::once(&default))
        .enumerate()
    {
        let g = ctx.create_param_group(layout)?;
        ctx.param_set_floats(g, f_color, &m.base_color_factor);
        ctx.param_set_f32(g, f_metallic, m.metallic);
        ctx.param_set_f32(g, f_roughness, m.roughness);
        ctx.param_set_floats(g, f_emissive, &m.emissive);
        ctx.param_set_f32(g, f_emissive_strength, m.emissive_strength);
        ctx.param_set_f32(g, f_normal_scale, m.normal_scale);
        ctx.param_set_f32(g, f_occlusion, m.occlusion_strength);
        ctx.param_set_f32(g, f_cutoff, m.alpha_cutoff);
        ctx.param_set_i32(g, f_mode, m.alpha_mode as i32);
        ctx.param_set_f32(g, f_ior, m.ior);
        ctx.param_set_f32(g, f_specular, m.specular);
        ctx.param_set_floats(g, f_specular_color, &m.specular_color);
        ctx.param_set_f32(g, f_transmission, m.transmission);
        ctx.param_set_f32(g, f_thickness, m.thickness);
        ctx.param_set_floats(g, f_attenuation_color, &m.attenuation_color);
        ctx.param_set_f32(g, f_attenuation_distance, m.attenuation_distance);
        ctx.param_set_f32(g, f_clearcoat, m.clearcoat);
        ctx.param_set_f32(g, f_clearcoat_roughness, m.clearcoat_roughness);
        ctx.param_set_f32(g, f_dispersion, m.dispersion);
        let samplers = map_samplers.get(i).copied().unwrap_or_default();
        for (map, ((tex, _), f)) in material_maps(m).into_iter().zip(&f_maps).enumerate() {
            let Some(tex) = usize::try_from(tex).ok().and_then(|t| textures.get(t)) else {
                continue;
            };
            ctx.param_set_texture(g, *f, *tex);
            ctx.param_set_sampler(g, *f, samplers[map]);
            if map == 0 {
                ctx.param_set_bool(g, f_has_tex, true);
            }
        }
        groups.push(g);
    }
    Ok((layout, groups))
}
