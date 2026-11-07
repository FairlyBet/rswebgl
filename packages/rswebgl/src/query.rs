// Query objects: occlusion tests and GPU timers.
//
// A query is attached to what it measures — an occlusion query to one draw
// (`Pass::set_last_draw_query`), a timer to a whole pass (`Pass::set_timer`) —
// and the renderer begins and ends it around that. Results come later: WebGL
// never makes them available in the frame (task) that ended the query, so
// they are polled (`query_result`, typically next frame) or awaited
// (`query_result_async`). A query keeps its last result until it is run again.
//
// Timers need `EXT_disjoint_timer_query_webgl2`, which browsers often withhold
// (it is a timing side channel); `can_time_gpu` says whether it's there. A
// timer whose measurement the GPU disturbed (`GPU_DISJOINT_EXT`: a power-state
// change, another app) has no result.

use std::time::Duration;

use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext as Gl, WebGlQuery};

use rswebgl_foundation::pool::{Destroyed, Pool};

use crate::console;
use crate::context::{Context, stale};
use crate::error::Error;
use crate::extension::Extension;
use crate::ids::QueryId;
use crate::readback::sleep;

const GPU_DISJOINT_EXT: u32 = 0x8FBB;

/// What a query measures.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryKind {
    /// Whether any sample of the draw passed the depth / stencil tests: 1 or 0.
    AnySamplesPassed = 0x8C2F,
    /// Same, but may answer 1 when nothing passed — cheaper on some GPUs.
    AnySamplesPassedConservative = 0x8D6A,
    /// GPU time a pass took, in nanoseconds. Needs
    /// `EXT_disjoint_timer_query_webgl2` (see `Context::can_time_gpu`).
    TimeElapsed = 0x88BF,
    /// Primitives a transform feedback recording wrote (see
    /// `Pass::set_transform_feedback_query`).
    TransformFeedbackPrimitivesWritten = 0x8C88,
}

impl QueryKind {
    pub(crate) fn is_occlusion(self) -> bool {
        matches!(
            self,
            QueryKind::AnySamplesPassed | QueryKind::AnySamplesPassedConservative
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum QueryState {
    // Never run.
    Unused,
    Pending,
    Ready(f64),
    // A timer the GPU disturbed: no result.
    Disjoint,
}

pub(crate) struct QueryObj {
    raw: WebGlQuery,
    kind: QueryKind,
    state: QueryState,
    // Bumped by every run, so a wait can tell its run from a later one.
    run: u32,
    // A timer the GPU was disturbed during (seen by any timer's read of the
    // context-wide `GPU_DISJOINT_EXT`, which reading clears).
    disturbed: bool,
}

// `GPU_DISJOINT_EXT` is one flag for the whole context, cleared by reading it:
// when it's set, every timer running at the time is disturbed, not just the
// one whose read happened to see it.
fn check_disjoint(gl: &Gl, queries: &mut Pool<QueryObj>) {
    if gl
        .get_parameter(GPU_DISJOINT_EXT)
        .is_ok_and(|v| v.is_truthy())
    {
        for q in queries.values_mut() {
            if q.kind == QueryKind::TimeElapsed && q.state == QueryState::Pending {
                q.disturbed = true;
            }
        }
    }
}

// Moves pending query `q` on if GL has its result.
fn poll(gl: &Gl, queries: &mut Pool<QueryObj>, q: QueryId) {
    let Some(obj) = queries.get(q.0) else {
        return;
    };
    if obj.state != QueryState::Pending
        || gl.is_context_lost()
        || !gl
            .get_query_parameter(&obj.raw, Gl::QUERY_RESULT_AVAILABLE)
            .is_truthy()
    {
        return;
    }
    let value = gl
        .get_query_parameter(&obj.raw, Gl::QUERY_RESULT)
        .as_f64()
        .unwrap_or(0.0);
    let timer = obj.kind == QueryKind::TimeElapsed;
    if timer {
        check_disjoint(gl, queries);
    }
    let obj = queries.get_mut(q.0).expect("looked up above");
    obj.state = if timer && obj.disturbed {
        QueryState::Disjoint
    } else {
        QueryState::Ready(value)
    };
}

/// What the renderer wants a query for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum QueryUse {
    Draw,
    Pass,
    Feedback,
}

/// Begins query `q` for `usage`; returns the GL target to end it on, or None
/// (reported) if the handle is stale or the kind doesn't fit.
pub(crate) fn begin(
    gl: &Gl,
    queries: &mut Pool<QueryObj>,
    q: crate::ids::QueryId,
    usage: QueryUse,
) -> Option<u32> {
    // Clear the context's disjoint flag before a timer starts, so it only
    // reports what happens during this run (and owns up to earlier ones).
    if queries
        .get(q.0)
        .is_some_and(|o| o.kind == QueryKind::TimeElapsed)
    {
        check_disjoint(gl, queries);
    }
    let Some(obj) = queries.get_mut(q.0) else {
        stale("query", "render", q);
        return None;
    };
    let fits = match usage {
        QueryUse::Draw => obj.kind.is_occlusion(),
        QueryUse::Pass => obj.kind == QueryKind::TimeElapsed,
        QueryUse::Feedback => obj.kind == QueryKind::TransformFeedbackPrimitivesWritten,
    };
    if !fits {
        let (what, want) = match usage {
            QueryUse::Draw => ("a draw", "an occlusion kind"),
            QueryUse::Pass => ("a pass timer", "TimeElapsed"),
            QueryUse::Feedback => (
                "a transform feedback recording",
                "TransformFeedbackPrimitivesWritten",
            ),
        };
        console::error_once(&format!(
            "[rswebgl] render: {what} needs a query of {want}, got {:?}; not measured",
            obj.kind
        ));
        return None;
    }
    let target = obj.kind as u32;
    gl.begin_query(target, &obj.raw);
    obj.state = QueryState::Pending;
    obj.run = obj.run.wrapping_add(1);
    obj.disturbed = false;
    Some(target)
}

#[wasm_bindgen]
impl Context {
    /// Creates a query. A `TimeElapsed` one is an error where GPU timing isn't
    /// available (see `can_time_gpu`).
    #[wasm_bindgen(js_name = createQuery)]
    pub fn create_query(&self, kind: QueryKind) -> Result<QueryId, Error> {
        self.check_alive()?;
        if kind == QueryKind::TimeElapsed && !self.can_time_gpu() {
            return Err("GPU timer queries need EXT_disjoint_timer_query_webgl2, \
                        which this browser doesn't expose"
                .into());
        }
        let raw = self.gl().create_query().ok_or("createQuery failed")?;
        self.res()
            .queries
            .insert(QueryObj {
                raw,
                kind,
                state: QueryState::Unused,
                run: 0,
                disturbed: false,
            })
            .map(Into::into)
            .map_err(|e| Error::from(e.to_string()))
    }

    #[wasm_bindgen(js_name = destroyQuery)]
    pub fn destroy_query(&self, q: QueryId) {
        let destroyed = self.res().queries.destroy(q.0);
        match destroyed {
            Destroyed::Now(obj) => self.gl().delete_query(Some(&obj.raw)),
            Destroyed::Deferred => {}
            Destroyed::Invalid => stale("query", "destroy_query", q),
        }
    }

    #[wasm_bindgen(js_name = queryIsValid)]
    pub fn query_is_valid(&self, q: QueryId) -> bool {
        self.res().queries.get(q.0).is_some()
    }

    /// Whether GPU timer queries are available
    /// (`EXT_disjoint_timer_query_webgl2`, enabled on first ask).
    #[wasm_bindgen(js_name = canTimeGpu)]
    pub fn can_time_gpu(&self) -> bool {
        self.enable_extension_quietly(Extension::ExtDisjointTimerQueryWebgl2)
    }

    /// Whether the query was run and its result hasn't arrived yet.
    #[wasm_bindgen(js_name = queryPending)]
    pub fn query_pending(&self, q: QueryId) -> bool {
        matches!(self.poll_query(q), Some((QueryState::Pending, _)))
    }

    /// The result of the query's last run: 1 / 0 for occlusion, nanoseconds
    /// for a timer. None while pending, if never run, for a disturbed timer
    /// (`GPU_DISJOINT_EXT`), or for a stale handle. Never available in the
    /// frame the query ran in — poll next frame.
    #[wasm_bindgen(js_name = queryResult)]
    pub fn query_result(&self, q: QueryId) -> Option<f64> {
        match self.poll_query(q)?.0 {
            QueryState::Ready(v) => Some(v),
            _ => None,
        }
    }

    /// JS: `query_result_async` as a Promise<number>.
    #[wasm_bindgen(js_name = queryResultAsync)]
    pub fn query_result_promise(&self, q: QueryId) -> js_sys::Promise {
        let result = self.query_result_async(q);
        wasm_bindgen_futures::future_to_promise(async move { Ok(result.await?.into()) })
    }
}

impl Context {
    // The query's state (polled) and run number.
    fn poll_query(&self, q: QueryId) -> Option<(QueryState, u32)> {
        let mut res = self.res();
        if res.queries.get(q.0).is_none() {
            stale("query", "query_result", q);
            return None;
        }
        poll(self.gl(), &mut res.queries, q);
        res.queries.get(q.0).map(|o| (o.state, o.run))
    }

    /// Waits for the result of the query's current run. An error if it was
    /// never run, the timer was disturbed, the query is run again before the
    /// result arrives (a query re-run every frame: poll `query_result` instead,
    /// or wait on a query run once), or the query or context goes away
    /// meanwhile.
    pub fn query_result_async(
        &self,
        q: QueryId,
    ) -> impl Future<Output = Result<f64, Error>> + 'static {
        let ctx = self.clone();
        let started = self.poll_query(q).map(|(_, run)| run);
        let epoch = self.core.epoch.get();
        // Submit the queued commands, or the result may not come before the
        // next frame flushes them (or ever, without one).
        self.gl().flush();
        async move {
            let run = started.ok_or("query: destroyed or invalid")?;
            loop {
                if ctx.is_lost() || ctx.core.epoch.get() != epoch {
                    return Err("query: the WebGL context was lost".into());
                }
                let state = match ctx.poll_query(q) {
                    None => return Err("query: destroyed or invalid".into()),
                    Some((_, r)) if r != run => {
                        return Err("query: run again before its result arrived".into());
                    }
                    Some((state, _)) => state,
                };
                match state {
                    QueryState::Unused => return Err("query: never run".into()),
                    QueryState::Disjoint => {
                        return Err("query: the GPU timer was disturbed (disjoint)".into());
                    }
                    QueryState::Ready(v) => return Ok(v),
                    QueryState::Pending => sleep(Duration::from_millis(1)).await,
                }
            }
        }
    }
}
