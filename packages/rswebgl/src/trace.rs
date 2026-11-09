//! CPU profiling: [`tracing`] spans recorded into a trace that Perfetto
//! (<https://ui.perfetto.dev>) and `chrome://tracing` open — the Chrome
//! trace event format, as JSON.
//!
//! The library marks its stages with `tracing` spans (`renderFrame`, the
//! passes, culling, uploads…); JS adds its own (`Trace.begin` / `end`),
//! counters and spans with explicit times (long animation frames, say).
//! Nothing is recorded until `Trace.start()`; afterwards `Trace.report()`
//! returns the JSON. While not recording a span costs a flag check: the
//! subscriber is installed by the first `start` and stays, answering
//! "not interested" until the next one.
//!
//! ```js
//! Trace.start();
//! // ... frames ...
//! Trace.stop();
//! const json = Trace.report(); // save it, open it in ui.perfetto.dev
//! ```
//!
//! Times come from `performance.now()`, which browsers coarsen (to 100 µs in
//! Chrome, 5 µs on a cross-origin isolated page) — plenty for frames.
//!
//! The subscriber is one per page (the wasm module is single-threaded); a
//! span's id is its callsite, so entering costs no allocation. Spans must
//! nest (enter / exit in stack order), which `tracing`'s guards do.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write as _;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Event, Metadata, Subscriber};
use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext as Gl, WebGlSync};

use crate::context::Context;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;
}

/// Events kept at most per recording (about 100 bytes each in memory, a
/// little more as JSON); recording stops when full.
pub const DEFAULT_MAX_EVENTS: usize = 1_000_000;

/// Tracks (Perfetto rows): spans the library and JS record go to `MAIN`.
pub const TRACK_MAIN: u32 = 1;
/// The GPU progress probes (`gpu_fence`).
pub const TRACK_GPU: u32 = 2;

#[derive(Clone)]
enum Arg {
    I(i64),
    U(u64),
    F(f64),
    B(bool),
    S(String),
}

#[derive(Clone, Copy)]
enum Name {
    Static(&'static str),
    // An index into `State::names` (names from JS).
    Interned(u32),
}

enum Phase {
    Complete { dur: f64 },
    Instant,
    Counter(f64),
}

struct Rec {
    name: Name,
    phase: Phase,
    track: u32,
    // Microseconds since the recording started.
    ts: f64,
    args: Vec<(&'static str, Arg)>,
}

struct Open {
    // The callsite (spans) or 0 (JS `begin`).
    id: u64,
    name: Name,
    start: f64,
    args: Vec<(&'static str, Arg)>,
}

#[derive(Default)]
struct State {
    recording: bool,
    installed: bool,
    // performance.now() at `start`, in milliseconds.
    origin: f64,
    max_events: usize,
    full: bool,
    events: Vec<Rec>,
    stack: Vec<Open>,
    // Span names by callsite, and fields given at creation waiting for the
    // span to be entered.
    span_names: HashMap<u64, &'static str>,
    pending_args: HashMap<u64, Vec<(&'static str, Arg)>>,
    names: Vec<String>,
    name_index: HashMap<String, u32>,
    tracks: Vec<(u32, String)>,
    metadata: Vec<(String, String)>,
    // GPU progress probes (`gpu_fence`): a fence per frame and when it was
    // set (µs since start), oldest first.
    fences: Vec<(WebGlSync, f64)>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

impl State {
    fn now_us(&self) -> f64 {
        (performance_now() - self.origin) * 1000.0
    }

    fn push(&mut self, rec: Rec) {
        if self.events.len() >= self.max_events {
            self.full = true;
            self.recording = false;
            return;
        }
        self.events.push(rec);
    }

    fn intern(&mut self, name: &str) -> Name {
        if let Some(&i) = self.name_index.get(name) {
            return Name::Interned(i);
        }
        let i = self.names.len() as u32;
        self.names.push(name.to_string());
        self.name_index.insert(name.to_string(), i);
        Name::Interned(i)
    }

    fn name_str(&self, name: Name) -> &str {
        match name {
            Name::Static(s) => s,
            Name::Interned(i) => &self.names[i as usize],
        }
    }

    fn close_top(&mut self) {
        if let Some(open) = self.stack.pop() {
            let now = self.now_us();
            self.push(Rec {
                name: open.name,
                phase: Phase::Complete {
                    dur: (now - open.start).max(0.0),
                },
                track: TRACK_MAIN,
                ts: open.start,
                args: open.args,
            });
        }
    }
}

// Field values into args.
struct Args<'a>(&'a mut Vec<(&'static str, Arg)>, Option<&'a mut String>);

impl Visit for Args<'_> {
    fn record_i64(&mut self, f: &Field, v: i64) {
        self.0.push((f.name(), Arg::I(v)));
    }
    fn record_u64(&mut self, f: &Field, v: u64) {
        self.0.push((f.name(), Arg::U(v)));
    }
    fn record_f64(&mut self, f: &Field, v: f64) {
        self.0.push((f.name(), Arg::F(v)));
    }
    fn record_bool(&mut self, f: &Field, v: bool) {
        self.0.push((f.name(), Arg::B(v)));
    }
    fn record_str(&mut self, f: &Field, v: &str) {
        self.0.push((f.name(), Arg::S(v.to_string())));
    }
    fn record_debug(&mut self, f: &Field, v: &dyn std::fmt::Debug) {
        let s = format!("{v:?}");
        match (&mut self.1, f.name()) {
            (Some(message), "message") => **message = s,
            _ => self.0.push((f.name(), Arg::S(s))),
        }
    }
}

fn callsite(meta: &'static Metadata<'static>) -> u64 {
    meta as *const Metadata<'static> as usize as u64
}

struct Recorder;

impl Subscriber for Recorder {
    // Asked every time (not cached): recording comes and goes.
    fn register_callsite(&self, _: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, _: &Metadata<'_>) -> bool {
        STATE.with(|s| s.try_borrow().is_ok_and(|s| s.recording))
    }

    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let meta = attrs.metadata();
        let id = callsite(meta);
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.span_names.entry(id).or_insert(meta.name());
            if !attrs.fields().is_empty() {
                let mut args = Vec::new();
                attrs.record(&mut Args(&mut args, None));
                s.pending_args.insert(id, args);
            }
        });
        Id::from_u64(id)
    }

    fn record(&self, span: &Id, values: &Record<'_>) {
        let id = span.into_u64();
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if let Some(open) = s.stack.iter_mut().rev().find(|o| o.id == id) {
                values.record(&mut Args(&mut open.args, None));
            }
        });
    }

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if !s.recording {
                return;
            }
            let mut args = Vec::new();
            let mut message = String::new();
            event.record(&mut Args(&mut args, Some(&mut message)));
            let name = if message.is_empty() {
                Name::Static(event.metadata().name())
            } else {
                s.intern(&message)
            };
            let ts = s.now_us();
            s.push(Rec {
                name,
                phase: Phase::Instant,
                track: TRACK_MAIN,
                ts,
                args,
            });
        });
    }

    fn enter(&self, span: &Id) {
        let id = span.into_u64();
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if !s.recording {
                return;
            }
            let name = Name::Static(s.span_names.get(&id).copied().unwrap_or("span"));
            let args = s.pending_args.remove(&id).unwrap_or_default();
            let start = s.now_us();
            s.stack.push(Open {
                id,
                name,
                start,
                args,
            });
        });
    }

    fn exit(&self, span: &Id) {
        let id = span.into_u64();
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            // A span entered before `start` isn't on the stack.
            if s.stack.last().is_some_and(|o| o.id == id) {
                s.close_top();
            }
        });
    }
}

// --- Rust API ------------------------------------------------------------------

/// Starts a recording (dropping the last one), of at most `max_events`.
pub fn start(max_events: usize) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.installed {
            s.installed = tracing::subscriber::set_global_default(Recorder).is_ok();
            if !s.installed {
                crate::console::warn(
                    "[rswebgl] Trace: another tracing subscriber is installed; \
                     only JS events are recorded",
                );
            }
        }
        s.events.clear();
        s.stack.clear();
        s.pending_args.clear();
        s.full = false;
        s.max_events = max_events.max(1);
        s.origin = performance_now();
        s.recording = true;
    });
}

/// Stops recording; what was recorded stays for [`report`]. Open spans are
/// closed now.
pub fn stop() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        while !s.stack.is_empty() {
            s.close_top();
        }
        s.recording = false;
    });
}

/// Probes the GPU's progress: call once a frame, after its commands. Sets a
/// fence and checks the earlier ones without waiting; each one found passed
/// becomes a span on the "GPU (fences)" track from when it was set to when
/// it was seen passed — an upper bound on that frame's GPU work, as fine as
/// this is called often. `gpu frames in flight` counts the fences not
/// passed yet: it climbs while the GPU falls behind. Works where timer
/// queries don't (most phones). Does nothing while not recording.
pub fn gpu_fence(ctx: &Context) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let gl = ctx.gl();
        if !s.recording || gl.is_context_lost() {
            for (sync, _) in s.fences.drain(..) {
                gl.delete_sync(Some(&sync));
            }
            return;
        }
        let now = s.now_us();
        let mut passed = 0;
        for (sync, _) in &s.fences {
            match gl.client_wait_sync_with_u32(sync, 0, 0) {
                Gl::ALREADY_SIGNALED | Gl::CONDITION_SATISFIED => passed += 1,
                _ => break,
            }
        }
        let done: Vec<_> = s.fences.drain(..passed).collect();
        for (sync, set) in done {
            gl.delete_sync(Some(&sync));
            s.push(Rec {
                name: Name::Static("gpu frame"),
                phase: Phase::Complete { dur: now - set },
                track: TRACK_GPU,
                ts: set,
                args: Vec::new(),
            });
        }
        let in_flight = s.fences.len() as f64;
        s.push(Rec {
            name: Name::Static("gpu frames in flight"),
            phase: Phase::Counter(in_flight),
            track: TRACK_MAIN,
            ts: now,
            args: Vec::new(),
        });
        if let Some(sync) = gl.fence_sync(Gl::SYNC_GPU_COMMANDS_COMPLETE, 0) {
            gl.flush();
            s.fences.push((sync, now));
        }
    });
}

pub fn is_recording() -> bool {
    STATE.with(|s| s.borrow().recording)
}

/// A counter's value now (a graph row in Perfetto).
pub fn counter(name: &str, value: f64) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if !s.recording {
            return;
        }
        let name = s.intern(name);
        let ts = s.now_us();
        s.push(Rec {
            name,
            phase: Phase::Counter(value),
            track: TRACK_MAIN,
            ts,
            args: Vec::new(),
        });
    });
}

/// The recording as Chrome trace event JSON.
pub fn report() -> String {
    STATE.with(|s| write_json(&s.borrow()))
}

fn write_json(s: &State) -> String {
    let mut out = String::with_capacity(s.events.len() * 96 + 1024);
    out.push_str("{\"displayTimeUnit\":\"ms\",\"traceEvents\":[\n");
    let mut first = true;
    let mut sep = |out: &mut String| {
        if !first {
            out.push_str(",\n");
        }
        first = false;
    };
    // Track names.
    sep(&mut out);
    out.push_str(r#"{"name":"process_name","ph":"M","pid":1,"tid":1,"args":{"name":"page"}}"#);
    let mut tracks = vec![
        (TRACK_MAIN, "main thread".to_string()),
        (TRACK_GPU, "GPU (fences)".to_string()),
    ];
    tracks.extend(s.tracks.iter().cloned());
    for (tid, name) in &tracks {
        sep(&mut out);
        let _ = write!(
            out,
            r#"{{"name":"thread_name","ph":"M","pid":1,"tid":{tid},"args":{{"name":{}}}}}"#,
            json_str(name)
        );
        sep(&mut out);
        let _ = write!(
            out,
            r#"{{"name":"thread_sort_index","ph":"M","pid":1,"tid":{tid},"args":{{"sort_index":{tid}}}}}"#
        );
    }
    for e in &s.events {
        sep(&mut out);
        let name = json_str(s.name_str(e.name));
        let ts = e.ts;
        let tid = e.track;
        match e.phase {
            Phase::Complete { dur } => {
                let _ = write!(
                    out,
                    r#"{{"name":{name},"ph":"X","pid":1,"tid":{tid},"ts":{ts:.1},"dur":{dur:.1}"#
                );
            }
            Phase::Instant => {
                let _ = write!(
                    out,
                    r#"{{"name":{name},"ph":"i","s":"t","pid":1,"tid":{tid},"ts":{ts:.1}"#
                );
            }
            Phase::Counter(v) => {
                let _ = write!(
                    out,
                    r#"{{"name":{name},"ph":"C","pid":1,"tid":{tid},"ts":{ts:.1},"args":{{"value":{}}}}}"#,
                    json_num(v)
                );
                continue;
            }
        }
        if !e.args.is_empty() {
            out.push_str(r#","args":{"#);
            for (i, (k, v)) in e.args.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&json_str(k));
                out.push(':');
                match v {
                    Arg::I(v) => {
                        let _ = write!(out, "{v}");
                    }
                    Arg::U(v) => {
                        let _ = write!(out, "{v}");
                    }
                    Arg::F(v) => out.push_str(&json_num(*v)),
                    Arg::B(v) => {
                        let _ = write!(out, "{v}");
                    }
                    Arg::S(v) => out.push_str(&json_str(v)),
                }
            }
            out.push('}');
        }
        out.push('}');
    }
    out.push_str("\n],\"metadata\":{");
    let mut meta = s.metadata.clone();
    meta.push(("events".into(), s.events.len().to_string()));
    meta.push(("truncated".into(), s.full.to_string()));
    for (i, m) in crate::console::recent().into_iter().enumerate() {
        meta.push((format!("console {i:02}"), m));
    }
    for (i, (k, v)) in meta.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "{}:{}", json_str(k), json_str(v));
    }
    out.push_str("}}\n");
    out
}

fn json_num(v: f64) -> String {
    if v.is_finite() {
        format!("{v}")
    } else {
        "null".into()
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// --- JS API ----------------------------------------------------------------------

/// The CPU profiler (see the `trace` module): records the library's spans
/// and your own into a trace for Perfetto / `chrome://tracing`.
#[wasm_bindgen]
pub struct Trace;

#[wasm_bindgen]
impl Trace {
    /// Starts recording (dropping the last recording), of at most
    /// `maxEvents` events (default 1,000,000; it stops when full).
    pub fn start(max_events: Option<u32>) {
        start(max_events.map_or(DEFAULT_MAX_EVENTS, |n| n as usize));
    }

    /// Stops; the recording stays for `report`.
    pub fn stop() {
        stop();
    }

    /// Whether a recording is under way (false after `stop`, or once full).
    #[wasm_bindgen(js_name = isRecording)]
    pub fn is_recording() -> bool {
        is_recording()
    }

    /// Events recorded so far.
    #[wasm_bindgen(js_name = eventCount)]
    pub fn event_count() -> u32 {
        STATE.with(|s| s.borrow().events.len() as u32)
    }

    /// Opens a span of your own on the main track; `end` closes the
    /// innermost open one (spans nest with the library's).
    pub fn begin(name: &str) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if !s.recording {
                return;
            }
            let name = s.intern(name);
            let start = s.now_us();
            s.stack.push(Open {
                id: 0,
                name,
                start,
                args: Vec::new(),
            });
        });
    }

    pub fn end() {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.recording && s.stack.last().is_some_and(|o| o.id == 0) {
                s.close_top();
            }
        });
    }

    /// A counter's value now (a graph in Perfetto).
    pub fn counter(name: &str, value: f64) {
        counter(name, value);
    }

    /// A marker now on the main track.
    pub fn instant(name: &str) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if !s.recording {
                return;
            }
            let name = s.intern(name);
            let ts = s.now_us();
            s.push(Rec {
                name,
                phase: Phase::Instant,
                track: TRACK_MAIN,
                ts,
                args: Vec::new(),
            });
        });
    }

    /// A span with known times (`performance.now()` milliseconds), on
    /// `track` (≥ 2 for rows of your own: name them with `trackName`).
    /// For things measured elsewhere: long animation frames, long tasks.
    pub fn complete(name: &str, start_ms: f64, duration_ms: f64, track: u32) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if !s.recording {
                return;
            }
            let name = s.intern(name);
            let ts = (start_ms - s.origin) * 1000.0;
            s.push(Rec {
                name,
                phase: Phase::Complete {
                    dur: duration_ms.max(0.0) * 1000.0,
                },
                track,
                ts,
                args: Vec::new(),
            });
        });
    }

    /// Probes the GPU's progress once a frame, after the frame's commands
    /// (`Scene.renderFrame` does it itself): spans on the "GPU (fences)"
    /// track and a `gpu frames in flight` counter. See `gpu_fence`.
    #[wasm_bindgen(js_name = gpuFence)]
    pub fn gpu_fence(ctx: &Context) {
        gpu_fence(ctx);
    }

    /// Names a track for `complete`.
    #[wasm_bindgen(js_name = trackName)]
    pub fn track_name(track: u32, name: &str) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.tracks.retain(|(t, _)| *t != track);
            s.tracks.push((track, name.to_string()));
        });
    }

    /// A key / value for the report's metadata (device, settings, model).
    /// Kept across recordings.
    pub fn metadata(key: &str, value: &str) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.metadata.retain(|(k, _)| k != key);
            s.metadata.push((key.to_string(), value.to_string()));
        });
    }

    /// The recording as Chrome trace event JSON (Perfetto opens it).
    pub fn report() -> String {
        report()
    }
}
