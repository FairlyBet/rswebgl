// The CPU profiler: `tracing` spans and JS-style spans land in the Chrome
// trace JSON with their fields, nothing is kept outside a recording, and a
// recording stops when full. Run with:
// wasm-pack test --headless --chrome packages/rswebgl
#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use rswebgl::trace::{self, Trace};

wasm_bindgen_test_configure!(run_in_browser);

fn names(report: &str) -> Vec<String> {
    let v = js_sys::JSON::parse(report).expect("valid JSON");
    let events = js_sys::Reflect::get(&v, &"traceEvents".into()).unwrap();
    js_sys::Array::from(&events)
        .iter()
        .filter(|e| {
            js_sys::Reflect::get(e, &"ph".into())
                .unwrap()
                .as_string()
                .as_deref()
                != Some("M")
        })
        .map(|e| {
            js_sys::Reflect::get(&e, &"name".into())
                .unwrap()
                .as_string()
                .unwrap()
        })
        .collect()
}

#[wasm_bindgen_test]
fn records_spans_counters_and_stops() {
    // Not recording: nothing kept.
    {
        let _s = tracing::info_span!("before").entered();
    }
    trace::start(1000);
    {
        let _outer = tracing::info_span!("outer", n = 3u32).entered();
        Trace::begin("js span");
        Trace::end();
        trace::counter("value", 1.5);
    }
    tracing::info!("marker");
    trace::stop();
    {
        let _s = tracing::info_span!("after").entered();
    }
    let report = trace::report();
    let n = names(&report);
    assert!(n.contains(&"outer".to_string()), "{n:?}");
    assert!(n.contains(&"js span".to_string()));
    assert!(n.contains(&"value".to_string()));
    assert!(n.contains(&"marker".to_string()));
    assert!(!n.contains(&"before".to_string()));
    assert!(!n.contains(&"after".to_string()));
    assert!(report.contains(r#""args":{"n":3}"#), "{report}");

    // Full: recording stops by itself.
    trace::start(3);
    for _ in 0..10 {
        let _s = tracing::info_span!("many").entered();
    }
    assert!(!trace::is_recording());
    assert_eq!(Trace::event_count(), 3);
    assert!(trace::report().contains(r#""truncated":"true""#));
}
