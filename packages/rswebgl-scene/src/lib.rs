//! Scene components and systems on top of `rswebgl-ecs` (design:
//! `docs/design/ecs.md`).
//!
//! - [`Transform`]: an entity's local position, rotation and scale.
//! - [`Parent`]: makes the entity's transform relative to another entity's.
//! - [`WorldMatrix`]: the resulting world matrix, written by
//!   [`TransformSystem`] — never by hand.
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

mod hierarchy;
mod transform;

pub use hierarchy::{HierarchyError, TransformSystem, children, despawn_recursive, set_parent};
pub use transform::{Parent, Transform, WorldMatrix};
