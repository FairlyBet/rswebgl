//! A loaded glTF model → entities: its node tree becomes a hierarchy under one
//! root entity, each primitive a child entity of its node that draws.

use std::collections::HashMap;
use std::rc::Rc;

use glam::{Mat4, Vec3};
use rswebgl::ids::{ProgramId, TextureId};
use rswebgl_ecs::{Entity, World};
use rswebgl_gltf::{DrawItem, Model, NodeCamera, NodeLight, NodeLightKind};

use crate::animation::{ModelAnimations, MorphWeights};
use crate::camera::Camera;
use crate::culling::Bounds;
use crate::light::Light;
use crate::render::MeshRenderer;
use crate::transform::{Parent, Transform};

/// What a glTF primitive entity keeps from the loader: its vertex attributes
/// (`ATTR_*`), to pick a PBR variant when its material changes, and which of
/// the model's draw items it is (`Model::items`), to find its GPU data again
/// after a reload.
#[derive(Debug, Clone, Copy)]
pub struct Primitive {
    pub attributes: u32,
    pub item: u32,
}

/// A skinned primitive's skeleton: the joint entities its vertices follow and
/// each joint's inverse bind matrix. The renderer hands the shader
/// world(joint) × inverse_bind(joint) per joint; the primitive's own world
/// matrix doesn't apply (glTF 2.0 §3.7.3).
#[derive(Debug, Clone)]
pub struct Skin {
    pub joints: Vec<Entity>,
    pub inverse_bind: Rc<[Mat4]>,
}

/// A primitive's morph targets on the GPU (the model owns the texture);
/// their weights are its node's `MorphWeights`.
#[derive(Debug, Clone, Copy)]
pub struct Morph {
    pub texture: TextureId,
    pub targets: u32,
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
/// - a node with a glTF camera or a `KHR_lights_punctual` light also gets a
///   `Camera` (aspect from the file, or 1; `Scene.renderFrame` follows the
///   canvas) or a `Light`, which then lights the scene like any other.
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
        if let Some(camera) = &node.camera {
            world.insert(e, camera_from(camera));
        }
        if let Some(light) = &node.light {
            world.insert(e, light_from(light));
        }
        if let Some(w) = &node.weights {
            world.insert(e, MorphWeights(w.clone()));
        }
        by_index.insert(node.index, e);
    }
    // Skins, with joints as entities (a joint outside the scene: the root).
    let skins: Vec<Skin> = model
        .skins()
        .iter()
        .map(|s| Skin {
            joints: s
                .joints
                .iter()
                .map(|j| by_index.get(j).copied().unwrap_or(root))
                .collect(),
            inverse_bind: s.inverse_bind.iter().map(Mat4::from_cols_array).collect(),
        })
        .collect();
    for (index, item) in model.items().iter().enumerate() {
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
        // A skinned mesh moves with its joints: its rest bounds don't hold,
        // so it isn't culled.
        match item.skin().and_then(|s| skins.get(s)) {
            Some(skin) => {
                world.insert(e, skin.clone());
            }
            None => {
                let (min, max) = item.local_bounds();
                world.insert(e, Bounds::new(Vec3::from(min), Vec3::from(max)));
            }
        }
        if let Some((texture, targets)) = item.morph() {
            world.insert(e, Morph { texture, targets });
        }
        world.insert(
            e,
            MeshRenderer {
                program,
                state: item.state(),
                vao: item.vao(),
                material: item.material_group(),
                command: item.draw_command(),
                transparent: item.render_state().blend,
                transmissive: usize::try_from(item.material_index())
                    .ok()
                    .and_then(|i| model.materials().get(i))
                    .is_some_and(|m| m.transmission() > 0.0),
            },
        );
        world.insert(
            e,
            Primitive {
                attributes: item.attributes(),
                item: index as u32,
            },
        );
    }
    if !model.animations().is_empty() {
        world.insert(
            root,
            ModelAnimations {
                clips: model.animations().into(),
                nodes: by_index,
                playing: None,
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

// A glTF camera as a scene camera: the file's aspect until the renderer
// sets the canvas's; a missing zfar is an infinite far plane.
fn camera_from(camera: &NodeCamera) -> Camera {
    match *camera {
        NodeCamera::Perspective {
            yfov,
            aspect_ratio,
            znear,
            zfar,
        } => Camera::perspective(
            yfov,
            aspect_ratio.unwrap_or(1.0),
            znear,
            zfar.unwrap_or(f32::INFINITY),
        ),
        NodeCamera::Orthographic {
            xmag,
            ymag,
            znear,
            zfar,
        } => Camera::orthographic(ymag, xmag / ymag, znear, zfar),
    }
}

fn light_from(light: &NodeLight) -> Light {
    let color = Vec3::from(light.color);
    let range = light.range.unwrap_or(0.0);
    match light.kind {
        NodeLightKind::Directional => Light::directional(color, light.intensity),
        NodeLightKind::Point => Light::point(color, light.intensity, range),
        NodeLightKind::Spot {
            inner_cone_angle,
            outer_cone_angle,
        } => Light::spot(
            color,
            light.intensity,
            range,
            inner_cone_angle,
            outer_cone_angle,
        ),
    }
}
