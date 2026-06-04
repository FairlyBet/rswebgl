//! Walk the default scene's node tree, compose world transforms, and flatten
//! every mesh primitive into a draw list (and a world-space bounding sphere).
//!
//! Per glTF 2.0: a node's local transform is `T * R * S` (or an equivalent
//! matrix); a child's world transform is `parent_world * local`. The node
//! hierarchy is a set of disjoint trees rooted at `scene.nodes()`.

use glam::{Mat4, Vec3};

use rswebgl::context::Context;

use crate::DrawItem;
use crate::geometry::build_primitive;
use crate::material::{self, Material};

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
    materials: &[Material],
    out: &mut Vec<DrawItem>,
) -> ([f32; 3], f32) {
    let mut bounds = Aabb::new();
    if let Some(scene) = doc.default_scene().or_else(|| doc.scenes().next()) {
        for node in scene.nodes() {
            walk(
                ctx,
                &node,
                Mat4::IDENTITY,
                buffers,
                materials,
                out,
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
    node: &gltf::Node,
    parent_world: Mat4,
    buffers: &[Vec<u8>],
    materials: &[Material],
    out: &mut Vec<DrawItem>,
    bounds: &mut Aabb,
) {
    let local = Mat4::from_cols_array_2d(&node.transform().matrix());
    let world = parent_world * local;

    if let Some(mesh) = node.mesh() {
        let det_negative = world.determinant() < 0.0;
        for primitive in mesh.primitives() {
            let Some(geo) = build_primitive(ctx, &primitive, buffers) else {
                continue;
            };
            expand_world_aabb(bounds, &world, geo.aabb_min, geo.aabb_max);

            let mat_index = primitive.material().index().map(|i| i as i32).unwrap_or(-1);
            let mat = if mat_index >= 0 {
                materials[mat_index as usize].clone()
            } else {
                material::default_material()
            };
            let state = material::render_state_for(&mat, det_negative);
            out.push(DrawItem {
                vao: geo.vao,
                draw: geo.draw,
                state,
                material: mat_index,
                model: world.to_cols_array(),
            });
        }
    }

    for child in node.children() {
        walk(ctx, &child, world, buffers, materials, out, bounds);
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
