use std::any::TypeId;
use std::collections::HashMap;

use rswebgl_foundation::pool::{Destroyed, Pool};

use crate::storage::{AnyStorage, Mut, Storage};
use crate::{Entity, Tick};

/// Entities and their components.
///
/// Any `'static` type is a component; its storage is created on first use.
/// Writes are stamped with the world's current tick, which is how systems find
/// what changed since they last ran (see `advance_tick`).
pub struct World {
    entities: Pool<()>,
    storages: Vec<Box<dyn AnyStorage>>,
    types: HashMap<TypeId, usize>,
    tick: Tick,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        Self {
            entities: Pool::new(),
            storages: Vec::new(),
            types: HashMap::new(),
            // Systems start with `last_run = 0`, so everything is new to them.
            tick: 1,
        }
    }

    // --- entities -----------------------------------------------------------

    /// A new entity with no components. Panics past ~1M live entities (see
    /// `try_spawn`).
    pub fn spawn(&mut self) -> Entity {
        self.try_spawn()
            .expect("rswebgl-ecs: entity pool exhausted (about 1M live entities)")
    }

    /// `spawn`, or None when the entity pool is exhausted.
    pub fn try_spawn(&mut self) -> Option<Entity> {
        self.entities.insert(()).ok().map(Entity)
    }

    /// Removes the entity and all its components (each storage logs it as
    /// removed). False if it was already gone.
    pub fn despawn(&mut self, e: Entity) -> bool {
        if !matches!(self.entities.destroy(e.0), Destroyed::Now(())) {
            return false;
        }
        for s in &mut self.storages {
            s.remove_entity(e);
        }
        true
    }

    pub fn is_alive(&self, e: Entity) -> bool {
        self.entities.contains(e.0)
    }

    /// Live entities.
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    // --- components ---------------------------------------------------------

    /// Adds or replaces `e`'s `T`, returning the previous one. Does nothing
    /// (and returns None) for a dead entity.
    pub fn insert<T: 'static>(&mut self, e: Entity, value: T) -> Option<T> {
        if !self.is_alive(e) {
            return None;
        }
        let tick = self.tick;
        self.storage_mut::<T>().insert(e, value, tick)
    }

    pub fn remove<T: 'static>(&mut self, e: Entity) -> Option<T> {
        self.storage_mut_if_any::<T>()?.remove(e)
    }

    pub fn has<T: 'static>(&self, e: Entity) -> bool {
        self.storage::<T>().is_some_and(|s| s.contains(e))
    }

    pub fn get<T: 'static>(&self, e: Entity) -> Option<&T> {
        self.storage::<T>()?.get(e)
    }

    /// Marks the component changed (at the current tick) when written
    /// through.
    pub fn get_mut<T: 'static>(&mut self, e: Entity) -> Option<Mut<'_, T>> {
        let tick = self.tick;
        self.storage_mut_if_any::<T>()?.get_mut(e, tick)
    }

    /// The storage of `T`, if any entity ever had one.
    pub fn storage<T: 'static>(&self) -> Option<&Storage<T>> {
        let i = *self.types.get(&TypeId::of::<T>())?;
        self.storages[i].as_any().downcast_ref()
    }

    /// The storage of `T`, created if needed.
    pub fn storage_mut<T: 'static>(&mut self) -> &mut Storage<T> {
        let i = self.storage_index::<T>();
        self.storages[i]
            .as_any_mut()
            .downcast_mut()
            .expect("storage type matches its TypeId")
    }

    /// Two storages at once — e.g. read `A`, write `B`. Panics if `A` and `B`
    /// are the same type.
    pub fn storages_mut<A: 'static, B: 'static>(&mut self) -> (&mut Storage<A>, &mut Storage<B>) {
        let (i, j) = (self.storage_index::<A>(), self.storage_index::<B>());
        let [a, b] = self
            .storages
            .get_disjoint_mut([i, j])
            .expect("storages_mut needs two different component types");
        (
            a.as_any_mut().downcast_mut().expect("type matches"),
            b.as_any_mut().downcast_mut().expect("type matches"),
        )
    }

    /// Entities that have both `A` and `B`. Walks `A`'s rows and looks each
    /// entity up in `B`, so put the rarer component first.
    pub fn join<A: 'static, B: 'static>(&self) -> impl Iterator<Item = (Entity, &A, &B)> {
        let pair = self.storage::<A>().zip(self.storage::<B>());
        pair.into_iter()
            .flat_map(|(a, b)| a.iter().filter_map(move |(e, va)| Some((e, va, b.get(e)?))))
    }

    fn storage_index<T: 'static>(&mut self) -> usize {
        let next = self.storages.len();
        let i = *self.types.entry(TypeId::of::<T>()).or_insert(next);
        if i == next {
            self.storages.push(Box::new(Storage::<T>::default()));
        }
        i
    }

    fn storage_mut_if_any<T: 'static>(&mut self) -> Option<&mut Storage<T>> {
        let i = *self.types.get(&TypeId::of::<T>())?;
        self.storages[i].as_any_mut().downcast_mut()
    }

    // --- ticks --------------------------------------------------------------

    /// The tick writes are stamped with now.
    pub fn tick(&self) -> Tick {
        self.tick
    }

    /// Starts a system run: returns the current tick and moves the world to
    /// the next, so writes made after the run are newer than it. A system
    /// keeps the returned tick as its `last_run`, processes rows changed after
    /// the previous `last_run`, and stamps its own writes with the returned
    /// tick — they're new to the systems after it, but not to itself next
    /// time.
    pub fn advance_tick(&mut self) -> Tick {
        let now = self.tick;
        self.tick += 1;
        now
    }

    /// Forgets which components were removed — call once per frame, after
    /// every system that reads `Storage::removed` has run.
    pub fn clear_trackers(&mut self) {
        for s in &mut self.storages {
            s.clear_removed();
        }
    }
}
