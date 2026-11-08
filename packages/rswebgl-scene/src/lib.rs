//! Scene components and systems on top of `rswebgl-ecs` (design:
//! `docs/design/ecs.md`).
//!
//! - [`Transform`]: an entity's local position, rotation and scale.
//! - [`Parent`]: makes the entity's transform relative to another entity's.
//! - [`WorldMatrix`]: the resulting world matrix, written by
//!   [`TransformSystem`] — never by hand.
//! - [`Camera`], [`Bounds`], [`MeshRenderer`], [`Visible`]: what
//!   [`SceneRenderer`] needs to cull the scene and push its draws into a
//!   `Pass`.
//!
//! ```
//! use glam::Vec3;
//! use rswebgl_ecs::World;
//! use rswebgl_scene::{Transform, TransformSystem, WorldMatrix, set_parent};
//!
//! let mut world = World::new();
//! let ring = world.spawn();
//! world.insert(ring, Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
//! let stone = world.spawn();
//! world.insert(stone, Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
//! set_parent(&mut world, stone, Some(ring)).unwrap();
//!
//! let mut transforms = TransformSystem::new();
//! transforms.update(&mut world);
//! let m = world.get::<WorldMatrix>(stone).unwrap().0;
//! assert_eq!(m.w_axis.truncate(), Vec3::new(1.0, 2.0, 0.0));
//! world.clear_trackers();
//! ```

mod brdf_lut;
mod camera;
mod culling;
mod environment;
mod gpu_pick;
mod hdr;
mod hierarchy;
mod js;
mod light;
mod model;
mod pbr;
mod pick;
mod render;
mod shader;
mod transform;

pub use camera::{Camera, Projection};
pub use culling::{Bounds, Frustum};
pub use environment::Environment;
pub use hdr::{HdrImage, decode_hdr, encode_hdr};
pub use hierarchy::{
    HierarchyError, TransformSystem, children, descendants, despawn_recursive, set_parent,
};
pub use js::Scene;
pub use light::{Light, LightKind};
pub use model::{Name, Primitive, find_by_name, spawn_model};
pub use pbr::{PbrKey, PbrShaders};
pub use pick::{Hit, Ray, pick_bounds, ray_aabb};
pub use render::{
    FRAME_BLOCK, FRAME_SLOT, LIGHTING_BLOCK, MATERIAL_SLOT, MAX_LIGHTS, MODEL_UNIFORM,
    MeshRenderer, NORMAL_MATRIX_UNIFORM, OBJECT_SLOT, Reloading, RenderStats, SceneRenderer,
    Visible,
};
pub use shader::{CHUNKS, FRAG_PRELUDE, VERT_PRELUDE, chunk, create_scene_program, shader_sources};
pub use transform::{Parent, Transform, WorldMatrix};
