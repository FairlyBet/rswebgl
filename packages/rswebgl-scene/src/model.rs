//! A loaded glTF model → entities: its node tree becomes a hierarchy under one
//! root entity, each primitive a child entity of its node that draws.

use std::collections::HashMap;
use std::rc::Rc;

use glam::{Mat4, Vec3};
use rswebgl::ids::ProgramId;
use rswebgl_ecs::{Entity, World};
use rswebgl_gltf::{DrawItem, Model, Triangles};

use crate::culling::Bounds;
use crate::render::MeshRenderer;
use crate::transform::{Parent, Transform};

/// What a glTF primitive entity keeps from the loader: its vertex attributes
/// (`ATTR_*`, to pick a PBR variant when its material changes) and its
/// triangles in mesh space (for picking), if it's a `TRIANGLES` primitive.
#[derive(Debug, Clone)]
pub struct Primitive {
    pub attributes: u32,
    pub triangles: Option<Rc<Triangles>>,
}

/// A name to find an entity by (glTF node names, or the app's own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name(pub String);

/// Spawns `model`'s default scene under a new root entity (identity
/// `Transform`) and returns the root:
///
/// - each glTF node → an entity with the node's local `Transform`, a `Parent`
///   (its parent node, or the root) and its `Name` if it has one;
/// - each primitive → a child entity of its node with a `MeshRenderer`
///   (the program `program_for` picks for it — e.g. `PbrShaders::program_for`
///   — and the primitive's state, vertex array, material group, draw command,
///   transparent if its material blends), its mesh-space `Bounds`, and the
///   `EXT_mesh_gpu_instancing` transform as its `Transform` (identity
///   without it), and a `Primitive`. A primitive `program_for` returns None
///   for is left out.
///
/// The entities refer to the model's GPU resources; the model still owns
/// them. Move or hide parts by their node entities (`find_by_name`).
///
/// Front-face winding is fixed at load time from the node's world matrix: a
/// part later mirrored by a negative scale draws inside out.
pub fn spawn_model(
    world: &mut World,
    model: &Model,
    mut program_for: impl FnMut(&DrawItem) -> Option<ProgramId>,
) -> Entity {
    let root = world.spawn();
    world.insert(root, Transform::IDENTITY);
    let mut by_index: HashMap<u32, Entity> = HashMap::with_capacity(model.nodes().len());
    // Parents come first in `nodes()`, so a parent's entity always exists.
    for node in model.nodes() {
        let e = world.spawn();
        world.insert(
            e,
            Transform::from_matrix(Mat4::from_cols_array(&node.matrix)),
        );
        let parent = node
            .parent
            .and_then(|p| by_index.get(&p).copied())
            .unwrap_or(root);
        world.insert(e, Parent(parent));
        if let Some(name) = &node.name {
            world.insert(e, Name(name.clone()));
        }
        by_index.insert(node.index, e);
    }
    for item in model.items() {
        let Some(&node) = by_index.get(&item.node()) else {
            continue;
        };
        let Some(program) = program_for(item) else {
            continue;
        };
        let e = world.spawn();
        world.insert(
            e,
            Transform::from_matrix(Mat4::from_cols_array(&item.instance_cols())),
        );
        world.insert(e, Parent(node));
        let (min, max) = item.local_bounds();
        world.insert(e, Bounds::new(Vec3::from(min), Vec3::from(max)));
        world.insert(
            e,
            MeshRenderer {
                program,
                state: item.state(),
                vao: item.vao(),
                material: item.material_group(),
                command: item.draw_command(),
                transparent: item.render_state().blend,
            },
        );
        world.insert(
            e,
            Primitive {
                attributes: item.attributes(),
                triangles: item.triangles(),
            },
        );
    }
    root
}

/// The first entity named `name` at or under `root`. Scans every `Name`, so
/// look parts up once and keep the entity.
pub fn find_by_name(world: &World, root: Entity, name: &str) -> Option<Entity> {
    let names = world.storage::<Name>()?;
    names
        .iter()
        .filter(|(_, n)| n.0 == name)
        .map(|(e, _)| e)
        .find(|&e| is_under(world, e, root))
}

// Whether `e` is `root` or a descendant of it. Bounded against a hand-made
// `Parent` cycle.
fn is_under(world: &World, mut e: Entity, root: Entity) -> bool {
    for _ in 0..=world.len() {
        if e == root {
            return true;
        }
        match world.get::<Parent>(e) {
            Some(p) => e = p.0,
            None => return false,
        }
    }
    false
}
