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

mod accessor;
mod animation;
mod buffers;
mod draco;
mod fetch;
mod geometry;
mod ktx2;
mod material;
mod meshopt;
mod pack;
mod scene;
mod textures;

pub use animation::{Animation, AnimationChannel, AnimationPath, Interpolation};
pub use draco::set_draco_decoder_path;
pub use ktx2::set_ktx2_transcoder_path;
pub use material::{
    AlphaMode, EXTERNAL_TEXTURE, MAP_COUNT, MAP_NAMES, MAP_SLOTS, MapSlot, Material, PROPERTIES,
    Property, UV_IDENTITY, default_material, render_state_for, uv_matrix,
};

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
/// (ior, transmission, emissive_strength and unlit are on the crate's own
/// list; specular and volume it parses but leaves off that list.)
const HANDLED_EXTENSIONS: &[&str] = &[
    "EXT_mesh_gpu_instancing",
    "KHR_texture_transform",
    "KHR_materials_specular",
    "KHR_materials_volume",
    "KHR_materials_clearcoat",
    "KHR_materials_sheen",
    "KHR_materials_iridescence",
    "KHR_materials_anisotropy",
    "KHR_materials_dispersion",
    "EXT_meshopt_compression",
    "KHR_meshopt_compression",
    "KHR_texture_basisu",
    "KHR_draco_mesh_compression",
    "KHR_mesh_quantization",
    "KHR_materials_variants",
    "KHR_materials_diffuse_transmission",
];

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
/// JOINTS_0 at location 6 and WEIGHTS_0 at 7: skinned (`DrawItem::skin`).
pub const ATTR_SKIN: u32 = 1 << 5;
/// Morph targets with position deltas (`DrawItem::morph_texture`).
pub const ATTR_MORPH_POSITION: u32 = 1 << 6;
/// Morph targets with normal deltas.
pub const ATTR_MORPH_NORMAL: u32 = 1 << 7;
/// Morph targets with tangent deltas.
pub const ATTR_MORPH_TANGENT: u32 = 1 << 8;
/// JOINTS_1 at location 8 and WEIGHTS_1 at 9: eight joint influences (with
/// `ATTR_SKIN`).
pub const ATTR_SKIN8: u32 = 1 << 9;

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
    // The node's skin (index into `Model::skins`), or -1.
    pub(crate) skin: i32,
    // Morph target deltas (NULL without), and how many targets.
    pub(crate) morph_texture: TextureId,
    pub(crate) morph_targets: u32,
    // KHR_materials_variants: (variant, material index).
    pub(crate) variants: Vec<(u32, i32)>,
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
    /// The material this item uses in KHR_materials_variants variant
    /// `variant` (an index into `materialVariants`): its own where the file
    /// maps none.
    #[wasm_bindgen(js_name = variantMaterialIndex)]
    pub fn variant_material_index(&self, variant: u32) -> i32 {
        self.variants
            .iter()
            .find(|(v, _)| *v == variant)
            .map_or(self.material, |(_, m)| *m)
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
    /// 4 TANGENT, 8 COLOR_0, 16 TEXCOORD_1, 32 skin (JOINTS_0 + WEIGHTS_0),
    /// 64 / 128 / 256 morph position / normal / tangent deltas (`ATTR_*`).
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

    /// The skin (index into `Model::skins`) of a skinned item: its vertices
    /// follow the skin's joints, and the node's own transform doesn't apply
    /// (glTF 2.0 §3.7.3).
    pub fn skin(&self) -> Option<usize> {
        usize::try_from(self.skin).ok()
    }

    /// The morph target texture (see `geometry::MorphGpu` for its layout)
    /// and the number of targets; None without targets.
    pub fn morph(&self) -> Option<(TextureId, u32)> {
        (!self.morph_texture.is_null()).then_some((self.morph_texture, self.morph_targets))
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
    /// The node's camera (glTF `cameras`), looking down its local −Z.
    pub camera: Option<NodeCamera>,
    /// The node's light (`KHR_lights_punctual`), shining down its local −Z.
    pub light: Option<NodeLight>,
    /// Morph target weights of the node's mesh: the node's own, else the
    /// mesh's defaults; None without (animations change them).
    pub weights: Option<Vec<f32>>,
}

/// A glTF skin: the joint nodes a skinned mesh follows and, per joint, the
/// inverse bind matrix (mesh space → the joint's space at bind time). A
/// vertex lands at Σ weight × world(joint) × inverse_bind(joint) × position.
#[derive(Clone, Debug)]
pub struct Skin {
    /// glTF node indices.
    pub joints: Vec<u32>,
    /// Column-major, one per joint (identity where the file has none).
    pub inverse_bind: Vec<[f32; 16]>,
}

/// A glTF camera, as the file has it (angles in radians, lengths in meters).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeCamera {
    /// `zfar` None: an infinite far plane. `aspect_ratio` is the author's
    /// hint; a viewer usually follows its canvas instead.
    Perspective {
        yfov: f32,
        aspect_ratio: Option<f32>,
        znear: f32,
        zfar: Option<f32>,
    },
    /// `xmag`, `ymag`: half the view volume's width and height.
    Orthographic {
        xmag: f32,
        ymag: f32,
        znear: f32,
        zfar: f32,
    },
}

/// A `KHR_lights_punctual` light. `intensity` is lux for a directional light,
/// candela otherwise; `range` None means no cutoff.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeLight {
    pub kind: NodeLightKind,
    /// Linear RGB.
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeLightKind {
    Directional,
    Point,
    /// Cone angles in radians from the axis.
    Spot {
        inner_cone_angle: f32,
        outer_cone_angle: f32,
    },
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
    pub(crate) morph_textures: Vec<TextureId>,
    pub(crate) skins: Vec<Skin>,
    pub(crate) animations: Vec<Animation>,
    pub(crate) mesh_count: usize,
    pub(crate) center: [f32; 3],
    pub(crate) radius: f32,
    // Where it was loaded from, to reload it after a context loss.
    pub(crate) source: String,
    // The context generation its GPU handles belong to.
    pub(crate) generation: u32,
    // KHR_materials_variants: the variants' names, in index order.
    pub(crate) variants: Vec<String>,
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
    /// After a context loss the GPU objects are gone already: only the
    /// material groups and layout (CPU-side) are freed then.
    pub fn destroy(&self, ctx: &Context) {
        for &g in &self.material_groups {
            ctx.destroy_param_group(g);
        }
        ctx.destroy_param_layout(self.material_layout);
        if self.generation != ctx.generation() {
            return;
        }
        for &vao in &self.vertex_arrays {
            ctx.destroy_vertex_array(vao);
        }
        for &buf in &self.buffers {
            ctx.destroy_buffer(buf);
        }
        for &tex in &self.morph_textures {
            ctx.destroy_texture(tex);
        }
        for &tex in &self.textures {
            ctx.destroy_texture(tex);
        }
        for &s in &self.samplers {
            ctx.destroy_sampler(s);
        }
    }

    /// Takes over the GPU objects of `fresh` — the same file loaded again
    /// after a context loss — keeping this model's material groups and their
    /// values (so material handles held elsewhere, and edits made to them,
    /// stay valid). Every parameter group pointing at this model's old
    /// textures or samplers (clones included) is pointed at the new ones.
    /// `fresh`'s own groups are freed. An error (and nothing changed but
    /// `fresh` freed) if the file no longer matches.
    pub fn adopt(&mut self, ctx: &Context, fresh: Model) -> Result<(), Error> {
        let same = self.items.len() == fresh.items.len()
            && self.textures.len() == fresh.textures.len()
            && self.samplers.len() == fresh.samplers.len()
            && self.materials.len() == fresh.materials.len();
        if !same {
            fresh.destroy(ctx);
            return Err(format!("reloading {}: the file has changed", self.source).into());
        }
        let textures = self
            .textures
            .iter()
            .copied()
            .zip(fresh.textures.iter().copied());
        let samplers = self
            .samplers
            .iter()
            .copied()
            .zip(fresh.samplers.iter().copied());
        ctx.remap_group_handles(&textures.collect(), &samplers.collect());
        for (mine, theirs) in self.items.iter_mut().zip(&fresh.items) {
            mine.vao = theirs.vao;
            mine.state_id = theirs.state_id;
            mine.morph_texture = theirs.morph_texture;
        }
        for &g in &fresh.material_groups {
            ctx.destroy_param_group(g);
        }
        ctx.destroy_param_layout(fresh.material_layout);
        self.textures = fresh.textures;
        self.samplers = fresh.samplers;
        self.vertex_arrays = fresh.vertex_arrays;
        self.buffers = fresh.buffers;
        self.morph_textures = fresh.morph_textures;
        self.generation = fresh.generation;
        Ok(())
    }

    /// The URL the model was loaded from (what a reload after a context loss
    /// fetches again).
    pub fn source(&self) -> String {
        self.source.clone()
    }

    /// Drops the kept source URL (a `data:` URI can be the whole file):
    /// the model can't be reloaded after a context loss then.
    #[wasm_bindgen(js_name = forgetSource)]
    pub fn forget_source(&mut self) {
        self.source = String::new();
    }

    /// Whether its GPU objects belong to an earlier context generation (lost
    /// since): reload it and `adopt` the reload.
    #[wasm_bindgen(js_name = isLost)]
    pub fn is_lost(&self, ctx: &Context) -> bool {
        self.generation != ctx.generation()
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

    /// The material groups: one per material (same order as `materials`),
    /// then the default material's.
    pub fn material_groups(&self) -> &[ParamGroupId] {
        &self.material_groups
    }

    /// KHR_materials_variants: the variants' names, in index order (empty
    /// without the extension).
    pub fn material_variants(&self) -> &[String] {
        &self.variants
    }

    /// The default scene's nodes, parents before children.
    pub fn nodes(&self) -> &[SceneNode] {
        &self.nodes
    }

    /// The skins (`DrawItem::skin` indexes them).
    pub fn skins(&self) -> &[Skin] {
        &self.skins
    }

    /// The animations, in file order.
    pub fn animations(&self) -> &[Animation] {
        &self.animations
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
    // The file itself goes as soon as it's parsed: a GLB's binary chunk is
    // copied out, and holding both would double the peak memory.
    let (mut json, blob) = {
        let bytes = fetch::fetch_bytes(url).await.map_err(js_err)?;
        split(&bytes)?
    };
    patch_basisu_sources(&mut json);

    let base = fetch::absolute_base(url).map_err(js_err)?;
    let mut buffers = buffers::resolve_buffers(&json, blob, &base)
        .await
        .map_err(js_err)?;
    // Compressed geometry, decoded in place (meshopt) or into a buffer of
    // its own the accessors are pointed at (Draco).
    // KHR_materials_variants: the variants' names.
    let variants: Vec<String> = json
        .pointer("/extensions/KHR_materials_variants/variants")
        .and_then(|v| v.as_array())
        .map(|vs| {
            vs.iter()
                .enumerate()
                .map(|(i, v)| {
                    v["name"]
                        .as_str()
                        .map_or_else(|| format!("variant {i}"), str::to_string)
                })
                .collect()
        })
        .unwrap_or_default();
    // (On the JSON: such a file is only a valid document once decoded.)
    meshopt::decode_views(&json, &mut buffers)?;
    draco::decode(&mut json, &mut buffers).await?;
    let document = document(&json)?;

    // Assemble textures, packing separate occlusion + metallic-roughness maps
    // into one ORM texture where safe; `remap` rewrites material texture indices
    // to match (a packed pair, or two textures with the same image and sampler,
    // collapse into one).
    // The context may have been lost while the files downloaded.
    if ctx.is_lost() {
        return Err("WebGL context lost while the model loaded".into());
    }
    // Everything below is created synchronously, in this generation.
    let generation = ctx.generation();
    let set = pack::build_texture_set(ctx, &document, &buffers, &base)?;
    let mut materials: Vec<Material> = document
        .materials()
        .map(|m| material::translate(&m))
        .collect();
    // TEXCOORD_2 and up go into the shader's two UV slots, per material.
    let uv_sets: Vec<[u32; 2]> = materials
        .iter_mut()
        .map(material::assign_uv_slots)
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
    geometry.set_uv_sets(uv_sets);
    // Normal-mapped materials (map 2, after their UV slots are set).
    geometry.set_normal_slots(
        materials
            .iter()
            .map(|m| (m.maps()[2].0 >= 0).then_some(m.maps()[2].1))
            .collect(),
    );
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
        match build_material_groups(ctx, &materials, &map_samplers, &textures) {
            Ok(m) => m,
            Err(e) => {
                // Nothing of the model is kept.
                for v in geometry.vertex_arrays() {
                    ctx.destroy_vertex_array(v);
                }
                for b in geometry.buffers() {
                    ctx.destroy_buffer(b);
                }
                for t in geometry.morph_textures() {
                    ctx.destroy_texture(t);
                }
                for &t in &textures {
                    ctx.destroy_texture(t);
                }
                for &s in &set.samplers {
                    ctx.destroy_sampler(s);
                }
                return Err(e);
            }
        };
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
        morph_textures: geometry.morph_textures().collect(),
        skins: read_skins(&document, &buffers),
        animations: animation::read_animations(&document, &buffers),
        mesh_count: geometry.mesh_count(),
        center,
        radius,
        source: url.to_string(),
        generation,
        variants,
    })
}

// A .gltf / .glb file → its JSON (patched before validation) and the GLB
// binary chunk.
fn split(bytes: &[u8]) -> Result<(serde_json::Value, Option<Vec<u8>>), Error> {
    let (json, blob) = if bytes.starts_with(b"glTF") {
        let glb = gltf::Glb::from_slice(bytes).map_err(|e| format!("GLB parse failed: {e}"))?;
        let json = serde_json::from_slice(&glb.json);
        (json, glb.bin.map(|b| b.into_owned()))
    } else {
        (serde_json::from_slice(bytes), None)
    };
    let json = json.map_err(|e| format!("glTF parse failed: {e}"))?;
    Ok((json, blob))
}

// The validated document. Extensions we handle are dropped from
// `extensionsRequired` first, which would otherwise reject them as
// unsupported.
fn document(json: &serde_json::Value) -> Result<gltf::Document, Error> {
    let mut root: gltf::json::Root =
        serde_json::from_value(json.clone()).map_err(|e| format!("glTF parse failed: {e}"))?;
    root.extensions_required
        .retain(|e| !HANDLED_EXTENSIONS.contains(&e.as_str()));
    gltf::Document::from_json(root).map_err(|e| format!("glTF invalid: {e}").into())
}

// KHR_texture_basisu: with a transcoder, a texture's source becomes its
// KTX2 image (textures.rs knows it by its type); without, it keeps its
// PNG/JPEG fallback — or, with none (no `source`, which the gltf crate
// requires), gets the KTX2 image anyway, to fail visibly at load.
fn patch_basisu_sources(json: &mut serde_json::Value) {
    let prefer = ktx2::has_transcoder();
    let Some(textures) = json.get_mut("textures").and_then(|t| t.as_array_mut()) else {
        return;
    };
    for t in textures {
        let ext = t.pointer("/extensions/KHR_texture_basisu/source").cloned();
        if let (Some(source), Some(obj)) = (ext, t.as_object_mut()) {
            if prefer {
                obj.insert("source".into(), source);
            } else {
                obj.entry("source").or_insert(source);
            }
        }
    }
}

// Every skin: its joints and inverse bind matrices (identity where the file
// gives none, glTF 2.0 §5.28).
fn read_skins(doc: &gltf::Document, buffers: &[Vec<u8>]) -> Vec<Skin> {
    doc.skins()
        .map(|skin| {
            let joints: Vec<u32> = skin.joints().map(|j| j.index() as u32).collect();
            let reader = skin.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));
            let mut inverse_bind: Vec<[f32; 16]> = reader
                .read_inverse_bind_matrices()
                .map(|m| {
                    m.map(|c| glam::Mat4::from_cols_array_2d(&c).to_cols_array())
                        .collect()
                })
                .unwrap_or_default();
            inverse_bind.resize(joints.len(), glam::Mat4::IDENTITY.to_cols_array());
            Skin {
                joints,
                inverse_bind,
            }
        })
        .collect()
}

fn js_err(e: JsValue) -> Error {
    e.as_string().unwrap_or_else(|| format!("{e:?}")).into()
}

// The material maps in `MAP_SLOTS` order: (texture index or -1, TEXCOORD set).
const MAPS: usize = MAP_COUNT;
fn material_maps(m: &Material) -> [(i32, u32); MAPS] {
    m.maps()
}

/// Sampler uniforms of the material maps, as named in a material group
/// (`MAP_SLOTS` order).
pub const MAP_UNIFORMS: [&str; MAPS] = {
    let mut out = [""; MAPS];
    let mut i = 0;
    while i < MAPS {
        out[i] = MAP_SLOTS[i].uniform;
        i += 1;
    }
    out
};

// One parameter group per material (plus the default material's, last).
fn build_material_groups(
    ctx: &Context,
    materials: &[Material],
    map_samplers: &[[SamplerId; MAPS]],
    textures: &[TextureId],
) -> Result<(ParamLayoutId, Vec<ParamGroupId>), Error> {
    let mut builder = ParamLayoutBuilder::new()
        .uniform("u_alpha_mode", UniformType::I32)
        .uniform("u_has_base_color_tex", UniformType::Bool);
    for p in material::PROPERTIES {
        let ty = match p.len {
            1 => UniformType::F32,
            3 => UniformType::Vec3,
            _ => UniformType::Vec4,
        };
        builder = builder.uniform(p.uniform, ty);
    }
    for name in MAP_UNIFORMS {
        builder = builder.texture(name);
    }
    for slot in MAP_SLOTS {
        builder = builder.uniform(slot.transform, UniformType::Mat3);
    }
    let layout = ctx.create_param_layout(&builder)?;
    // A failure part way frees the layout and the groups made.
    let mut groups = Vec::with_capacity(materials.len() + 1);
    let made = fill_material_groups(ctx, layout, materials, map_samplers, textures, &mut groups);
    if let Err(e) = made {
        for g in groups {
            ctx.destroy_param_group(g);
        }
        ctx.destroy_param_layout(layout);
        return Err(e);
    }
    Ok((layout, groups))
}

fn fill_material_groups(
    ctx: &Context,
    layout: ParamLayoutId,
    materials: &[Material],
    map_samplers: &[[SamplerId; MAPS]],
    textures: &[TextureId],
    groups: &mut Vec<ParamGroupId>,
) -> Result<(), Error> {
    let field = |name| ctx.param_field(layout, name);
    let f_mode = field("u_alpha_mode")?;
    let f_has_tex = field("u_has_base_color_tex")?;
    let f_maps: Vec<_> = MAP_UNIFORMS
        .iter()
        .map(|name| field(name))
        .collect::<Result<_, _>>()?;

    let default = material::default_material();
    for (i, m) in materials
        .iter()
        .chain(std::iter::once(&default))
        .enumerate()
    {
        let g = ctx.create_param_group(layout)?;
        m.write_properties(ctx, g);
        m.write_uv_transforms(ctx, g);
        ctx.param_set_i32(g, f_mode, m.alpha_mode as i32);
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every extension we implement may be required; an unknown one still fails.
    #[test]
    fn required_extensions_we_implement_load() {
        let doc = |name: &str| {
            format!(
                r#"{{"asset": {{"version": "2.0"}}, "extensionsUsed": ["{name}"], "extensionsRequired": ["{name}"]}}"#
            )
        };
        let crate_parsed = [
            "KHR_materials_ior",
            "KHR_materials_transmission",
            "KHR_materials_emissive_strength",
            "KHR_materials_unlit",
            "KHR_lights_punctual",
        ];
        for name in HANDLED_EXTENSIONS.iter().chain(&crate_parsed) {
            assert!(
                document(&split(doc(name).as_bytes()).unwrap().0).is_ok(),
                "{name}"
            );
        }
        let unknown = split(doc("EXT_unknown_extension").as_bytes()).unwrap().0;
        assert!(document(&unknown).is_err());
    }
}
