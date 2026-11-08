// One timer per context for everything that polls the GPU without blocking
// (async reads, queries, parallel shader links).
//
// Each waiter awaits `Poller::tick`: the first of a round schedules a single
// timer; when it fires, every waiter registered meanwhile wakes and checks its
// own condition, then waits on the next round if it still isn't done. So many
// reads in flight cost one timer per round, not one each.
//
// A timer rather than requestAnimationFrame: it keeps polling in background
// tabs and workers, and the wait is usually a frame or two.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context as TaskContext, Poll, Waker};

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

// How long a round waits. Browsers clamp nested timers to ~4 ms anyway.
const ROUND_MS: f64 = 1.0;

#[derive(Default)]
pub(crate) struct Poller {
    // Bumped by every fired round.
    round: Cell<u64>,
    scheduled: Cell<bool>,
    wakers: RefCell<Vec<Waker>>,
}

impl Poller {
    /// Resolves after the next round fires.
    pub(crate) fn tick(self: &Rc<Self>) -> Tick {
        Tick {
            poller: self.clone(),
            round: None,
        }
    }

    /// Timers scheduled and not yet fired (0 or 1). Diagnostics, tests.
    pub(crate) fn pending_timers(&self) -> usize {
        self.scheduled.get() as usize
    }

    fn register(self: &Rc<Self>, waker: &Waker) {
        let mut wakers = self.wakers.borrow_mut();
        if !wakers.iter().any(|w| w.will_wake(waker)) {
            wakers.push(waker.clone());
        }
        drop(wakers);
        if !self.scheduled.replace(true) {
            self.schedule();
        }
    }

    fn schedule(self: &Rc<Self>) {
        let me = self.clone();
        let fire = Closure::once_into_js(move || me.fire());
        let global = js_sys::global();
        let set_timeout = js_sys::Reflect::get(&global, &"setTimeout".into())
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
        let scheduled = set_timeout
            .map(|f| f.call2(&global, &fire, &ROUND_MS.into()).is_ok())
            .unwrap_or(false);
        if !scheduled {
            // No timers here (an odd global): fire on the microtask queue.
            let me = self.clone();
            wasm_bindgen_futures::spawn_local(async move { me.fire() });
        }
    }

    fn fire(&self) {
        self.scheduled.set(false);
        self.round.set(self.round.get() + 1);
        let wakers = std::mem::take(&mut *self.wakers.borrow_mut());
        for w in wakers {
            w.wake();
        }
    }
}

pub(crate) struct Tick {
    poller: Rc<Poller>,
    round: Option<u64>,
}

impl Future for Tick {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<()> {
        let now = self.poller.round.get();
        match self.round {
            Some(r) if r != now => Poll::Ready(()),
            _ => {
                self.round = Some(now);
                self.poller.register(cx.waker());
                Poll::Pending
            }
        }
    }
}

impl crate::context::Context {
    /// Whether a polling timer is scheduled: one per context, shared by every
    /// async read, query and shader link in flight. Diagnostics.
    #[doc(hidden)]
    pub fn poll_timers_pending(&self) -> usize {
        self.core.poller.pending_timers()
    }
}
