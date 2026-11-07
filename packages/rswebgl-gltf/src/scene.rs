//! Walk the default scene's node tree, compose world transforms, and flatten
//! every mesh primitive into a draw list (and a world-space bounding sphere).
//!
//! Per glTF 2.0: a node's local transform is `T * R * S` (or an equivalent
//! matrix); a child's world transform is `parent_world * local`. The node
//! hierarchy is a set of disjoint trees rooted at `scene.nodes()`.
//!
//! A node with `EXT_mesh_gpu_instancing` stands for N copies of its mesh,
//! each `node_world * TRS_i`; they share the geometry and come out as N draw
//! items (drawing them in one instanced call is a renderer matter, not done
//! yet).

use glam::{Mat4, Quat, Vec3};

use rswebgl::console;
use rswebgl::context::Context;

use crate::geometry::GeometryCache;
use crate::material::{self, Material};
use crate::{DrawItem, SceneNode};

/// Growing axis-aligned bounds in world space.
struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    fn new() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        }
    }
    fn expand(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
}

/// Append a `DrawItem` for every primitive reachable from the default scene,
/// returning the model's bounding-sphere `(center, radius)` for camera framing.
pub fn build_draw_list(
    ctx: &Context,
    doc: &gltf::Document,
    buffers: &[Vec<u8>],
    geometry: &mut GeometryCache,
    materials: &[Material],
    out: &mut Vec<DrawItem>,
    nodes: &mut Vec<SceneNode>,
) -> ([f32; 3], f32) {
    let mut bounds = Aabb::new();
    if let Some(scene) = doc.default_scene().or_else(|| doc.scenes().next()) {
        for node in scene.nodes() {
            walk(
                ctx,
                doc,
                &node,
                None,
                Mat4::IDENTITY,
                buffers,
                geometry,
                materials,
                out,
                nodes,
                &mut bounds,
            );
        }
    }

    if bounds.is_empty() {
        ([0.0, 0.0, 0.0], 1.0)
    } else {
        let center = (bounds.min + bounds.max) * 0.5;
        let radius = ((bounds.max - center).length()).max(1e-4);
        (center.to_array(), radius)
    }
}

#[allow(clippy::too_many_arguments)]
fn walk(
    ctx: &Context,
    doc: &gltf::Document,
    node: &gltf::Node,
    parent: Option<u32>,
    parent_world: Mat4,
    buffers: &[Vec<u8>],
    geometry: &mut GeometryCache,
    materials: &[Material],
    out: &mut Vec<DrawItem>,
    nodes: &mut Vec<SceneNode>,
    bounds: &mut Aabb,
) {
    let local = Mat4::from_cols_array_2d(&node.transform().matrix());
    let world = parent_world * local;
    nodes.push(SceneNode {
        index: node.index() as u32,
        parent,
        matrix: local.to_cols_array(),
        name: node.name().map(str::to_owned),
    });

    if let Some(mesh) = node.mesh() {
        let default = material::default_material();
        // EXT_mesh_gpu_instancing: the node stands for N copies, each with its
        // own TRS on top of the node's transform. Without it, one copy.
        let instances = gpu_instances(doc, node, buffers).unwrap_or_else(|| vec![Mat4::IDENTITY]);
        // Built once per mesh; every further node or instance just reuses it.
        let prims = geometry.mesh(ctx, &mesh, buffers);
        for (instance, local_instance) in instances.iter().enumerate() {
            let world = world * *local_instance;
            let det_negative = world.determinant() < 0.0;
            for geo in prims {
                expand_world_aabb(bounds, &world, geo.aabb_min, geo.aabb_max);
                let mat = usize::try_from(geo.material).map_or(&default, |i| &materials[i]);
                let state = material::render_state_for(mat, det_negative);
                out.push(DrawItem {
                    vao: geo.vao,
                    draw: geo.draw,
                    state_id: ctx.render_state(&state),
                    state,
                    material: geo.material,
                    material_group: rswebgl::ids::ParamGroupId::NULL,
                    mesh: mesh.index() as u32,
                    node: node.index() as u32,
                    instance: instance as u32,
                    model: world.to_cols_array(),
                    instance_matrix: local_instance.to_cols_array(),
                    attributes: geo.attributes,
                    aabb_min: geo.aabb_min,
                    aabb_max: geo.aabb_max,
                    triangles: geo.triangles.clone(),
                });
            }
        }
    }

    for child in node.children() {
        walk(
            ctx,
            doc,
            &child,
            Some(node.index() as u32),
            world,
            buffers,
            geometry,
            materials,
            out,
            nodes,
            bounds,
        );
    }
}

/// Expand `bounds` by the 8 world-space corners of a local AABB.
fn expand_world_aabb(bounds: &mut Aabb, world: &Mat4, min: [f32; 3], max: [f32; 3]) {
    for &x in &[min[0], max[0]] {
        for &y in &[min[1], max[1]] {
            for &z in &[min[2], max[2]] {
                bounds.expand(world.transform_point3(Vec3::new(x, y, z)));
            }
        }
    }
}

/// The per-instance transforms of an `EXT_mesh_gpu_instancing` node, relative
/// to the node. `None` if the node has no such extension (or it can't be
/// read — reported, and the node is drawn once).
fn gpu_instances(
    doc: &gltf::Document,
    node: &gltf::Node,
    buffers: &[Vec<u8>],
) -> Option<Vec<Mat4>> {
    let ext = node.extension_value("EXT_mesh_gpu_instancing")?;
    let attrs = ext.get("attributes")?.as_object()?;
    let doc_accessor = |name: &str| -> Option<gltf::Accessor> {
        let i = attrs.get(name)?.as_u64()? as usize;
        doc.accessors().nth(i)
    };
    let get = |b: gltf::Buffer| buffers.get(b.index()).map(Vec::as_slice);

    // Each attribute is optional; all present ones share one count.
    let t = doc_accessor("TRANSLATION");
    let r = doc_accessor("ROTATION");
    let s = doc_accessor("SCALE");
    let count = [&t, &r, &s]
        .iter()
        .filter_map(|a| a.as_ref())
        .map(|a| a.count())
        .max()?;
    for a in [&t, &r, &s].into_iter().flatten() {
        if a.data_type() != gltf::accessor::DataType::F32 || a.count() != count {
            console::warn(&format!(
                "[rswebgl-gltf] node {}: EXT_mesh_gpu_instancing with non-float or \
                 mismatched attributes isn't supported; drawn once",
                node.index()
            ));
            return None;
        }
    }
    let read3 = |a: &Option<gltf::Accessor>, default: [f32; 3]| -> Vec<[f32; 3]> {
        a.clone()
            .and_then(|a| gltf::accessor::Iter::<[f32; 3]>::new(a, get))
            .map_or_else(|| vec![default; count], Iterator::collect)
    };
    let translations = read3(&t, [0.0; 3]);
    let scales = read3(&s, [1.0; 3]);
    let rotations: Vec<[f32; 4]> = r
        .and_then(|a| gltf::accessor::Iter::<[f32; 4]>::new(a, get))
        .map_or_else(|| vec![[0.0, 0.0, 0.0, 1.0]; count], Iterator::collect);
    if translations.len() != count || scales.len() != count || rotations.len() != count {
        console::warn(&format!(
            "[rswebgl-gltf] node {}: EXT_mesh_gpu_instancing data out of range; drawn once",
            node.index()
        ));
        return None;
    }
    Some(
        (0..count)
            .map(|i| {
                Mat4::from_scale_rotation_translation(
                    Vec3::from(scales[i]),
                    Quat::from_array(rotations[i]).normalize(),
                    Vec3::from(translations[i]),
                )
            })
            .collect(),
    )
}
