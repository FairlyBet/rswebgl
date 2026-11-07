//! Slot pool behind generational handles.
//!
//! - A handle resolves only while its slot holds the same generation, so a
//!   handle to a destroyed object never reaches whatever reuses the slot.
//! - Freed slots are reused FIFO (as late as possible), and a slot whose
//!   generation is exhausted is retired for good instead of wrapping around: a
//!   stale handle can never match a newer object, by construction.
//! - Objects can be *in use* by others (a buffer shared by several meshes). The
//!   use count is a plain number in the slot, not shared ownership: destroying an
//!   object that's still in use makes its handle invalid immediately, and the
//!   value is handed back to the caller by the `release` that drops the last use.

use std::collections::VecDeque;

use crate::handle::{Handle, MAX_GENERATION, MAX_INDEX};

enum State<T> {
    Free,
    Live(T),
    // Destroyed while still in use: unreachable through handles, waiting for
    // the last `release`.
    Doomed(T),
}

struct Slot<T> {
    generation: u32,
    uses: u32,
    state: State<T>,
}

/// What `Pool::destroy` did.
#[derive(Debug, PartialEq, Eq)]
pub enum Destroyed<T> {
    /// The object was removed; here it is, for the caller to clean up.
    Now(T),
    /// The object is still in use. Its handle is already invalid; the value
    /// comes back from the `release` that drops the last use.
    Deferred,
    /// The handle didn't refer to a live object (stale, null or foreign).
    Invalid,
}

/// The pool has no slot left: every index is live or retired.
#[derive(Debug, PartialEq, Eq)]
pub struct PoolFull;

impl std::fmt::Display for PoolFull {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "handle pool is full ({} slots)", MAX_INDEX)
    }
}

pub struct Pool<T> {
    slots: Vec<Slot<T>>,
    free: VecDeque<u32>,
    live: usize,
    retired: usize,
}

impl<T> Default for Pool<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Pool<T> {
    pub fn new() -> Self {
        // Slot 0 is reserved so that the zero handle is always null.
        Self {
            slots: vec![Slot {
                generation: 0,
                uses: 0,
                state: State::Free,
            }],
            free: VecDeque::new(),
            live: 0,
            retired: 0,
        }
    }

    /// Stores `value` and returns its handle.
    pub fn insert(&mut self, value: T) -> Result<Handle, PoolFull> {
        let index = match self.free.pop_front() {
            Some(i) => i,
            None => {
                let i = self.slots.len() as u32;
                if i > MAX_INDEX {
                    return Err(PoolFull);
                }
                self.slots.push(Slot {
                    generation: 0,
                    uses: 0,
                    state: State::Free,
                });
                i
            }
        };
        let slot = &mut self.slots[index as usize];
        slot.state = State::Live(value);
        slot.uses = 0;
        self.live += 1;
        Ok(Handle::new(index, slot.generation))
    }

    fn slot(&self, h: Handle) -> Option<&Slot<T>> {
        let s = self.slots.get(h.index() as usize)?;
        (h.index() != 0 && s.generation == h.generation()).then_some(s)
    }

    fn slot_mut(&mut self, h: Handle) -> Option<&mut Slot<T>> {
        let s = self.slots.get_mut(h.index() as usize)?;
        (h.index() != 0 && s.generation == h.generation()).then_some(s)
    }

    pub fn get(&self, h: Handle) -> Option<&T> {
        match &self.slot(h)?.state {
            State::Live(v) => Some(v),
            _ => None,
        }
    }

    pub fn get_mut(&mut self, h: Handle) -> Option<&mut T> {
        match &mut self.slot_mut(h)?.state {
            State::Live(v) => Some(v),
            _ => None,
        }
    }

    /// Like `get`, but also reaches an object destroyed while still in use —
    /// for the users that keep it alive (a parameter group reading its
    /// layout after the app destroyed the layout).
    pub fn get_in_use(&self, h: Handle) -> Option<&T> {
        match &self.slot(h)?.state {
            State::Live(v) | State::Doomed(v) => Some(v),
            State::Free => None,
        }
    }

    pub fn contains(&self, h: Handle) -> bool {
        self.get(h).is_some()
    }

    /// Records one more user of a live object. Fails for a stale or destroyed
    /// handle, so nothing new can start using an object that's on its way out.
    pub fn retain(&mut self, h: Handle) -> bool {
        match self.slot_mut(h) {
            Some(s) if matches!(s.state, State::Live(_)) => {
                s.uses += 1;
                true
            }
            _ => false,
        }
    }

    /// Drops one use. Returns the value if this was the last use of an object
    /// that was already destroyed (it's removed now, for the caller to clean
    /// up). Works on destroyed-but-in-use objects, which `get` no longer sees.
    pub fn release(&mut self, h: Handle) -> Option<T> {
        let s = self.slot_mut(h)?;
        if matches!(s.state, State::Free) || s.uses == 0 {
            return None;
        }
        s.uses -= 1;
        if s.uses == 0 && matches!(s.state, State::Doomed(_)) {
            return self.remove(h.index());
        }
        None
    }

    /// Destroys the object. See [`Destroyed`] for the in-use case.
    pub fn destroy(&mut self, h: Handle) -> Destroyed<T> {
        let Some(s) = self.slot_mut(h) else {
            return Destroyed::Invalid;
        };
        if !matches!(s.state, State::Live(_)) {
            return Destroyed::Invalid;
        }
        if s.uses > 0 {
            let State::Live(v) = std::mem::replace(&mut s.state, State::Free) else {
                unreachable!()
            };
            s.state = State::Doomed(v);
            self.live -= 1;
            return Destroyed::Deferred;
        }
        self.live -= 1;
        match self.remove(h.index()) {
            Some(v) => Destroyed::Now(v),
            None => Destroyed::Invalid,
        }
    }

    // Frees a slot: bumps its generation, then recycles it — or retires it if
    // the generation is exhausted.
    fn remove(&mut self, index: u32) -> Option<T> {
        let s = &mut self.slots[index as usize];
        let value = match std::mem::replace(&mut s.state, State::Free) {
            State::Live(v) | State::Doomed(v) => v,
            State::Free => return None,
        };
        s.uses = 0;
        if s.generation == MAX_GENERATION {
            self.retired += 1;
        } else {
            s.generation += 1;
            self.free.push_back(index);
        }
        Some(value)
    }

    /// Number of live objects (destroyed-but-in-use ones not included).
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Slots permanently taken out of use because their generation ran out.
    pub fn retired(&self) -> usize {
        self.retired
    }

    /// Live objects with their handles.
    pub fn iter(&self) -> impl Iterator<Item = (Handle, &T)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| match &s.state {
                State::Live(v) => Some((Handle::new(i as u32, s.generation), v)),
                _ => None,
            })
    }

    /// Removes everything, returning the values (live and destroyed-but-in-use)
    /// for cleanup. Every outstanding handle becomes stale: the slots move to
    /// the next generation as if each object had been destroyed. Used when the
    /// GL context is lost and all GPU objects die at once.
    pub fn drain(&mut self) -> Vec<T> {
        let mut out = Vec::with_capacity(self.live);
        for i in 1..self.slots.len() as u32 {
            if let Some(v) = self.remove(i) {
                out.push(v);
            }
        }
        self.live = 0;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_get_destroy() {
        let mut p = Pool::new();
        let a = p.insert("a").unwrap();
        let b = p.insert("b").unwrap();
        assert_eq!(p.get(a), Some(&"a"));
        assert_eq!(p.get(b), Some(&"b"));
        assert_eq!(p.len(), 2);
        assert_eq!(p.destroy(a), Destroyed::Now("a"));
        assert_eq!(p.get(a), None);
        assert_eq!(p.destroy(a), Destroyed::Invalid);
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn users_still_reach_a_destroyed_object_they_hold() {
        let mut p = Pool::new();
        let a = p.insert("a").unwrap();
        assert!(p.retain(a));
        assert_eq!(p.destroy(a), Destroyed::Deferred);
        assert_eq!(p.get(a), None, "gone for everyone else");
        assert_eq!(p.get_in_use(a), Some(&"a"), "the holder still reads it");
        assert_eq!(p.release(a), Some("a"));
        assert_eq!(p.get_in_use(a), None);
    }

    #[test]
    fn null_and_foreign_handles_never_resolve() {
        let mut p = Pool::new();
        p.insert(1).unwrap();
        assert_eq!(p.get(Handle::NULL), None);
        assert_eq!(p.get(Handle::from_raw(u32::MAX)), None);
        assert_eq!(p.destroy(Handle::NULL), Destroyed::Invalid);
    }

    #[test]
    fn stale_handle_does_not_reach_reused_slot() {
        let mut p = Pool::new();
        let a = p.insert(1).unwrap();
        p.destroy(a);
        let b = p.insert(2).unwrap();
        assert_eq!(b.index(), a.index());
        assert_ne!(b, a);
        assert_eq!(p.get(a), None);
        assert_eq!(p.get(b), Some(&2));
    }

    #[test]
    fn free_slots_are_reused_fifo() {
        let mut p = Pool::new();
        let hs: Vec<_> = (0..3).map(|i| p.insert(i).unwrap()).collect();
        p.destroy(hs[0]);
        p.destroy(hs[1]);
        assert_eq!(p.insert(10).unwrap().index(), hs[0].index());
        assert_eq!(p.insert(11).unwrap().index(), hs[1].index());
    }

    #[test]
    fn exhausted_slot_retires_instead_of_wrapping() {
        let mut p = Pool::new();
        let first = p.insert(0u32).unwrap();
        let mut h = first;
        // Cycle the only slot through every generation.
        for i in 1..=MAX_GENERATION {
            p.destroy(h);
            h = p.insert(i).unwrap();
            assert_eq!(h.index(), first.index());
            assert_eq!(h.generation(), i);
        }
        p.destroy(h);
        assert_eq!(p.retired(), 1);
        // The next insert takes a fresh slot; the original one is gone for good.
        let next = p.insert(0).unwrap();
        assert_ne!(next.index(), first.index());
        assert_eq!(p.get(first), None);
        assert_eq!(p.get(h), None);
    }

    #[test]
    fn destroy_in_use_defers_until_last_release() {
        let mut p = Pool::new();
        let buf = p.insert("buffer").unwrap();
        assert!(p.retain(buf));
        assert!(p.retain(buf));

        assert_eq!(p.destroy(buf), Destroyed::Deferred);
        // Unreachable right away, and nothing new can start using it.
        assert_eq!(p.get(buf), None);
        assert!(!p.retain(buf));
        assert_eq!(p.len(), 0);

        assert_eq!(p.release(buf), None);
        assert_eq!(p.release(buf), Some("buffer"));
        // The slot is recycled only now.
        let other = p.insert("other").unwrap();
        assert_eq!(other.index(), buf.index());
        assert_eq!(p.get(buf), None);
    }

    #[test]
    fn release_without_destroy_keeps_object() {
        let mut p = Pool::new();
        let h = p.insert(5).unwrap();
        assert!(p.retain(h));
        assert_eq!(p.release(h), None);
        assert_eq!(p.release(h), None); // no uses left: no-op
        assert_eq!(p.get(h), Some(&5));
        assert_eq!(p.destroy(h), Destroyed::Now(5));
    }

    #[test]
    fn drain_invalidates_every_handle() {
        let mut p = Pool::new();
        let a = p.insert(1).unwrap();
        let b = p.insert(2).unwrap();
        p.retain(b);
        p.destroy(b); // doomed, still drained
        let mut vals = p.drain();
        vals.sort();
        assert_eq!(vals, vec![1, 2]);
        assert!(p.is_empty());
        assert_eq!(p.get(a), None);
        assert_eq!(p.release(b), None);
        let c = p.insert(3).unwrap();
        assert_ne!(c, a);
        assert_ne!(c, b);
    }

    #[test]
    fn iter_yields_live_only() {
        let mut p = Pool::new();
        let a = p.insert('a').unwrap();
        let b = p.insert('b').unwrap();
        p.destroy(a);
        let items: Vec<_> = p.iter().collect();
        assert_eq!(items, vec![(b, &'b')]);
    }
}
