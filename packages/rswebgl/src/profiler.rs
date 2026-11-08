// GPU time per pass, without stalling: each label keeps a small ring of
// timer queries. `time` attaches the next free one to a pass; `update` (once
// a frame) collects the results that have arrived — WebGL hands them over a
// frame or more after the pass ran — into a smoothed average per label. A
// query still waiting for its result is never reused, so a slow GPU just
// grows the ring (up to `RING_MAX`) and then skips timing a frame.
//
// Needs `EXT_disjoint_timer_query_webgl2`, which browsers often withhold;
// without it every call is a no-op and there are no timings. Timings the GPU
// disturbed (`GPU_DISJOINT_EXT`) are dropped.

use wasm_bindgen::prelude::*;

use crate::context::Context;
use crate::ids::QueryId;
use crate::pass::Pass;
use crate::query::QueryKind;

// Queries in flight per label at most.
const RING_MAX: usize = 8;
// Weight of a new sample in the running average (≈ the last 10 frames).
const SMOOTHING: f64 = 0.1;

struct Slot {
    query: QueryId,
    // Attached to a pass and not yet collected.
    awaiting: bool,
}

struct Timer {
    label: String,
    slots: Vec<Slot>,
    // Smoothed and latest, in milliseconds; None before the first result.
    average: Option<f64>,
    last: f64,
}

/// GPU time per pass, averaged over recent frames (see the module notes).
#[wasm_bindgen]
pub struct GpuProfiler {
    ctx: Context,
    timers: Vec<Timer>,
    available: bool,
    // The context generation the queries belong to: a loss takes them.
    generation: u32,
}

#[wasm_bindgen]
impl GpuProfiler {
    /// A profiler on `ctx`; inert where GPU timing isn't available
    /// (`available`).
    #[wasm_bindgen(constructor)]
    pub fn new(ctx: &Context) -> GpuProfiler {
        GpuProfiler {
            ctx: ctx.clone(),
            timers: Vec::new(),
            available: ctx.can_time_gpu(),
            generation: ctx.generation(),
        }
    }

    /// Whether this browser and GPU time passes at all.
    pub fn available(&self) -> bool {
        self.available
    }

    /// Times `pass` under `label` (several passes may share a label: their
    /// times add up per frame only if collected together — give each its
    /// own). Call before rendering the pass.
    pub fn time(&mut self, pass: &mut Pass, label: &str) {
        if let Some(q) = self.next_query(label) {
            pass.set_timer(q);
        }
    }

    /// Collects the results that have arrived. Call once a frame.
    pub fn update(&mut self) {
        let ctx = &self.ctx;
        for timer in &mut self.timers {
            for slot in &mut timer.slots {
                if !slot.awaiting || ctx.query_pending(slot.query) {
                    continue;
                }
                slot.awaiting = false;
                // None: disturbed (disjoint) — dropped.
                if let Some(ns) = ctx.query_result(slot.query) {
                    let ms = ns / 1.0e6;
                    timer.last = ms;
                    timer.average = Some(match timer.average {
                        Some(avg) => avg + (ms - avg) * SMOOTHING,
                        None => ms,
                    });
                }
            }
        }
    }

    /// The smoothed GPU time of `label`'s passes in milliseconds; undefined
    /// before its first result (or without GPU timing).
    pub fn milliseconds(&self, label: &str) -> Option<f64> {
        self.timers.iter().find(|t| t.label == label)?.average
    }

    /// The latest single measurement of `label`, in milliseconds.
    #[wasm_bindgen(js_name = lastMilliseconds)]
    pub fn last_milliseconds(&self, label: &str) -> Option<f64> {
        let t = self.timers.iter().find(|t| t.label == label)?;
        t.average.map(|_| t.last)
    }

    /// The labels timed so far, in first-use order.
    pub fn labels(&self) -> Vec<String> {
        self.timers.iter().map(|t| t.label.clone()).collect()
    }

    /// Frees the queries.
    pub fn destroy(&mut self) {
        for timer in self.timers.drain(..) {
            for slot in timer.slots {
                self.ctx.destroy_query(slot.query);
            }
        }
    }
}

impl GpuProfiler {
    // A query for `label` not waiting on a result: a free slot, else a new
    // one (up to RING_MAX); None if all are in flight or timing is out.
    fn next_query(&mut self, label: &str) -> Option<QueryId> {
        if !self.available || self.ctx.is_lost() {
            return None;
        }
        if self.generation != self.ctx.generation() {
            // Restored after a loss: the old queries are gone with it.
            self.timers.clear();
            self.generation = self.ctx.generation();
            self.available = self.ctx.can_time_gpu();
        }
        let i = match self.timers.iter().position(|t| t.label == label) {
            Some(i) => i,
            None => {
                self.timers.push(Timer {
                    label: label.to_owned(),
                    slots: Vec::new(),
                    average: None,
                    last: 0.0,
                });
                self.timers.len() - 1
            }
        };
        let timer = &mut self.timers[i];
        // Results that arrived but weren't collected yet would be lost on
        // reuse: free slots only.
        if let Some(slot) = timer.slots.iter_mut().find(|s| !s.awaiting) {
            slot.awaiting = true;
            return Some(slot.query);
        }
        if timer.slots.len() >= RING_MAX {
            return None;
        }
        let query = self.ctx.create_query(QueryKind::TimeElapsed).ok()?;
        timer.slots.push(Slot {
            query,
            awaiting: true,
        });
        Some(query)
    }
}
