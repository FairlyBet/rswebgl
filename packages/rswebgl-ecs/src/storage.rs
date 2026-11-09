//! Sparse-set storage for one component type.
//!
//! `sparse` maps an entity's slot index to a row; rows are dense, so iteration
//! walks plain arrays with no holes. Removing a row moves the last row into its
//! place: O(1), and entities never move between tables. Memory is proportional
//! to how many entities have the component, not to the largest entity index
//! (apart from 4 bytes per index in `sparse`).

use std::ops::{Deref, DerefMut};

use crate::{Entity, Tick};

const NONE: u32 = u32::MAX;

/// The components of one type, with the tick each was last changed at.
pub struct Storage<T> {
    sparse: Vec<u32>,
    entities: Vec<Entity>,
    data: Vec<T>,
    changed: Vec<Tick>,
    // Entities that lost the component (or were despawned) since the last
    // `World::clear_trackers`.
    removed: Vec<Entity>,
    // Removals ever (never cleared): lets a consumer notice one even after
    // the trackers were cleared.
    removals: u64,
}

impl<T> Default for Storage<T> {
    fn default() -> Self {
        Self {
            sparse: Vec::new(),
            entities: Vec::new(),
            data: Vec::new(),
            changed: Vec::new(),
            removed: Vec::new(),
            removals: 0,
        }
    }
}

impl<T> Storage<T> {
    /// The row of `e`'s component, if it has one. A stale entity (despawned,
    /// slot reused) has none.
    pub fn row(&self, e: Entity) -> Option<u32> {
        let row = *self.sparse.get(e.0.index() as usize)?;
        (row != NONE && self.entities[row as usize] == e).then_some(row)
    }

    pub fn contains(&self, e: Entity) -> bool {
        self.row(e).is_some()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    pub fn get(&self, e: Entity) -> Option<&T> {
        self.row(e).map(|r| &self.data[r as usize])
    }

    /// Mutable access that marks the component changed only if it is actually
    /// written through.
    pub fn get_mut(&mut self, e: Entity, tick: Tick) -> Option<Mut<'_, T>> {
        let r = self.row(e)? as usize;
        Some(Mut {
            value: &mut self.data[r],
            changed: &mut self.changed[r],
            tick,
        })
    }

    /// Adds or replaces `e`'s component; either way it counts as changed.
    /// Returns the previous value.
    pub(crate) fn insert(&mut self, e: Entity, value: T, tick: Tick) -> Option<T> {
        if let Some(r) = self.row(e) {
            self.changed[r as usize] = tick;
            return Some(std::mem::replace(&mut self.data[r as usize], value));
        }
        let idx = e.0.index() as usize;
        if idx >= self.sparse.len() {
            self.sparse.resize(idx + 1, NONE);
        }
        self.sparse[idx] = self.entities.len() as u32;
        self.entities.push(e);
        self.data.push(value);
        self.changed.push(tick);
        None
    }

    pub(crate) fn remove(&mut self, e: Entity) -> Option<T> {
        let r = self.row(e)? as usize;
        let last = self.entities.len() - 1;
        if r != last {
            let moved = self.entities[last];
            self.sparse[moved.0.index() as usize] = r as u32;
        }
        self.sparse[e.0.index() as usize] = NONE;
        self.entities.swap_remove(r);
        self.changed.swap_remove(r);
        self.removed.push(e);
        self.removals += 1;
        Some(self.data.swap_remove(r))
    }

    /// Entities in row order.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    /// Components in row order.
    pub fn data(&self) -> &[T] {
        &self.data
    }

    /// Components in row order, writable without marking anything changed —
    /// for bulk writes; report them with `mark_changed`.
    pub fn data_mut_untracked(&mut self) -> &mut [T] {
        &mut self.data
    }

    pub fn mark_changed(&mut self, row: u32, tick: Tick) {
        self.changed[row as usize] = tick;
    }

    /// The tick row `row` was last changed (or added) at.
    pub fn changed_tick(&self, row: u32) -> Tick {
        self.changed[row as usize]
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = (Entity, &T)> {
        self.entities.iter().copied().zip(&self.data)
    }

    /// Components changed (or added) after tick `since`.
    pub fn iter_changed(&self, since: Tick) -> impl Iterator<Item = (Entity, &T)> {
        self.iter()
            .zip(&self.changed)
            .filter(move |(_, t)| **t > since)
            .map(|(item, _)| item)
    }

    /// Every component, each marking itself changed only if written through.
    pub fn iter_mut(&mut self, tick: Tick) -> impl ExactSizeIterator<Item = (Entity, Mut<'_, T>)> {
        self.entities
            .iter()
            .copied()
            .zip(self.data.iter_mut().zip(self.changed.iter_mut()))
            .map(move |(e, (value, changed))| {
                (
                    e,
                    Mut {
                        value,
                        changed,
                        tick,
                    },
                )
            })
    }

    /// Entities that lost this component since the last
    /// `World::clear_trackers` (despawned ones included). An entity can be
    /// listed and have the component again if it was re-added.
    pub fn removed(&self) -> &[Entity] {
        &self.removed
    }

    /// How many times the component was removed (despawns included) since
    /// the storage was made; unlike `removed`, `clear_trackers` doesn't reset
    /// it.
    pub fn removal_count(&self) -> u64 {
        self.removals
    }

    pub(crate) fn clear_removed(&mut self) {
        self.removed.clear();
    }
}

/// A component borrowed for writing: marks it changed on the first mutable
/// access, so reading through it costs nothing.
pub struct Mut<'a, T> {
    value: &'a mut T,
    changed: &'a mut Tick,
    tick: Tick,
}

impl<T> Mut<'_, T> {
    /// Writes only if the value differs, so an unchanged write doesn't mark
    /// the component changed.
    pub fn set_if_neq(&mut self, value: T)
    where
        T: PartialEq,
    {
        if *self.value != value {
            *self.value = value;
            *self.changed = self.tick;
        }
    }
}

impl<T> Deref for Mut<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value
    }
}

impl<T> DerefMut for Mut<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        *self.changed = self.tick;
        self.value
    }
}

// Type-erased view the world uses for despawn and tracker clearing.
pub(crate) trait AnyStorage: 'static {
    fn remove_entity(&mut self, e: Entity);
    fn clear_removed(&mut self);
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

impl<T: 'static> AnyStorage for Storage<T> {
    fn remove_entity(&mut self, e: Entity) {
        self.remove(e);
    }
    fn clear_removed(&mut self) {
        Storage::clear_removed(self);
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
