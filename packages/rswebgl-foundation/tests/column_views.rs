// JS views over columns, in a real browser.
// Run with: wasm-pack test --headless --chrome packages/rswebgl-foundation
#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

use rswebgl_foundation::column::{Column, memory_version};

wasm_bindgen_test_configure!(run_in_browser);

// JS writes land in the column without a copy, and Rust writes show in the view.
#[wasm_bindgen_test]
fn view_shares_memory_both_ways() {
    let mut c = Column::<f32>::new(3, 16);
    c.resize(4);
    let view = c.js_view();
    assert_eq!(view.length(), 12);

    view.set_index(4, 42.0); // element 1, component 1
    assert_eq!(c.get(1), &[0.0, 42.0, 0.0]);

    c.set(3, &[7.0, 8.0, 9.0]);
    let view = c.js_view();
    assert_eq!(view.get_index(9), 7.0);
}

// Growing the wasm memory detaches every view; memory_version reports it.
#[wasm_bindgen_test]
fn memory_growth_detaches_views() {
    let mut c = Column::<u32>::new(1, 16);
    c.resize(8);
    let view = c.js_view();
    let before = memory_version();

    // Allocate well past the current memory size to force memory.grow.
    let pages = before as usize;
    let big = vec![1u8; (pages + 64) * 65536];
    assert!(big[big.len() - 1] == 1);

    assert_ne!(memory_version(), before);
    assert_eq!(view.length(), 0, "old view should be detached");
    assert_eq!(c.js_view().length(), 8, "a fresh view works");
}

// Reallocating the column doesn't detach the old view — it silently points at
// the old address. layout_version is what catches that.
#[wasm_bindgen_test]
fn reallocation_changes_layout_version() {
    let mut c = Column::<f32>::new(1, 16);
    c.resize(2);
    let v = c.layout_version();
    c.resize(100_000);
    assert_ne!(c.layout_version(), v);
}
