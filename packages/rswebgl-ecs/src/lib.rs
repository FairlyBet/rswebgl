//! A small ECS for rswebgl scenes (design: `docs/design/ecs.md`).
//!
//! - **Entities** are generational `u32` handles, the same kind as renderer
//!   resources: a despawned entity's handle never reaches whatever reuses its
//!   slot, and in JS it is a plain `number`.
//! - **Components** are any `'static` type, each kept in its own sparse set:
//!   dense rows (no holes to skip), O(1) add and remove, no entity moving
//!   between tables.
//! - **Change ticks** tell a system what changed since it last ran, so a frame
//!   where nothing moved costs next to nothing.
//!
//! There is no scheduler: systems are plain functions the app calls in the
//! order it wants.
//!
//! ```
//! use rswebgl_ecs::World;
//!
//! struct Position(f32);
//! struct Speed(f32);
//!
//! let mut world = World::new();
//! let e = world.spawn();
//! world.insert(e, Position(0.0));
//! world.insert(e, Speed(2.0));
//!
//! // A system: move everything that has a speed.
//! let now = world.advance_tick();
//! let (speeds, positions) = world.storages_mut::<Speed, Position>();
//! for (e, speed) in speeds.iter() {
//!     if let Some(mut p) = positions.get_mut(e, now) {
//!         p.0 += speed.0;
//!     }
//! }
//! assert_eq!(world.get::<Position>(e).unwrap().0, 2.0);
//! ```

mod storage;
mod world;

pub use storage::{Mut, Storage};
pub use world::World;

rswebgl_foundation::typed_handle!(
    /// An entity: a generational handle. `Entity::NULL` is never alive.
    pub struct Entity
);

/// A point in the world's change history; see `World::advance_tick`.
pub type Tick = u64;
