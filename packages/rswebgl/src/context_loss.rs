// WebGL context loss / restore.
//
// When the GPU process resets (driver crash, too many contexts, mobile tab in the
// background) the browser fires `webglcontextlost` on the canvas. Every GL object
// made before that point is dead for good; after `webglcontextrestored` the same
// `WebGl2RenderingContext` works again, but it's empty.
//
// The library's part: keep the restore possible (preventDefault on the lost
// event), stop the renderer from issuing calls while lost, re-enable the
// extensions the app asked for, and tell the app via `on_lost` / `on_restored`.
// Recreating buffers, textures, programs, … is the app's job, in `on_restored` —
// only it knows where the data came from.
//
// Old handles still get dropped after a restore, and deleting an object from a
// previous generation is an INVALID_OPERATION. So each context has a generation
// counter, bumped on every loss; GL objects record the generation they were
// created in and skip their delete call once it's stale (the GPU side is gone
// with the old generation anyway).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{Event, EventTarget, WebGl2RenderingContext, WebglLoseContext};

use crate::console;
use crate::extension::Extension;

type Callback = RefCell<Option<Box<dyn FnMut()>>>;

thread_local! {
    // Generation counter per GL context that has loss listeners. Contexts are
    // few and long-lived, so a linear scan (and never removing) is fine.
    static GENERATIONS: RefCell<Vec<(WebGl2RenderingContext, Rc<Cell<u32>>)>> =
        const { RefCell::new(Vec::new()) };
}

fn generation_cell(gl: &WebGl2RenderingContext) -> Rc<Cell<u32>> {
    GENERATIONS.with(|g| {
        let mut g = g.borrow_mut();
        if let Some((_, n)) = g.iter().find(|(c, _)| c == gl) {
            return Rc::clone(n);
        }
        let n = Rc::new(Cell::new(0));
        g.push((gl.clone(), Rc::clone(&n)));
        n
    })
}

/// The current generation of `gl`: record it when creating a GL object.
pub(crate) fn generation(gl: &WebGl2RenderingContext) -> u32 {
    GENERATIONS.with(|g| {
        g.borrow()
            .iter()
            .find(|(c, _)| c == gl)
            .map_or(0, |(_, n)| n.get())
    })
}

/// Whether an object created in `generation` still exists on the GPU, i.e.
/// whether its delete call is valid.
pub(crate) fn is_current(gl: &WebGl2RenderingContext, generation: u32) -> bool {
    self::generation(gl) == generation
}

// Shared by the Context and the canvas event listeners, which outlive it.
pub(crate) struct LossState {
    // Extensions are per context generation, so this is also the list to
    // re-enable after a restore.
    pub(crate) extensions: RefCell<Vec<Extension>>,
    pub(crate) on_lost: Callback,
    pub(crate) on_restored: Callback,
    // WEBGL_lose_context from `simulate_context_loss`: restoreContext must be
    // called on the same object, and getExtension returns null while lost.
    pub(crate) lose_ext: RefCell<Option<WebglLoseContext>>,
}

impl LossState {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self {
            extensions: RefCell::new(Vec::new()),
            on_lost: RefCell::new(None),
            on_restored: RefCell::new(None),
            lose_ext: RefCell::new(None),
        })
    }
}

// Runs a stored callback. It's taken out for the call so the callback itself
// may replace it (or create GL objects that touch the Context) without a
// RefCell double borrow; it's put back unless it was replaced meanwhile.
fn invoke(slot: &Callback) {
    let taken = slot.borrow_mut().take();
    if let Some(mut f) = taken {
        f();
        let mut s = slot.borrow_mut();
        if s.is_none() {
            *s = Some(f);
        }
    }
}

// Listens for loss/restore on the context's canvas (an HTMLCanvasElement or an
// OffscreenCanvas). The listeners are handed to JS and live as long as the
// canvas, so the handling keeps working after the Context value itself drops
// (render loops usually keep only cloned handles).
pub(crate) fn install(gl: &WebGl2RenderingContext, state: &Rc<LossState>) {
    let Some(target) = gl.canvas().and_then(|c| c.dyn_into::<EventTarget>().ok()) else {
        console::warn("[rswebgl] no canvas to listen on: context loss won't be handled");
        return;
    };

    let generation = generation_cell(gl);
    let st = Rc::clone(state);
    let lost = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
        // Without this the browser never restores the context.
        e.prevent_default();
        generation.set(generation.get().wrapping_add(1));
        console::warn("[rswebgl] WebGL context lost");
        invoke(&st.on_lost);
    });

    let st = Rc::clone(state);
    let gl = gl.clone();
    let restored = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
        for ext in st.extensions.borrow().iter() {
            let _ = gl.get_extension(ext.name());
        }
        console::log("[rswebgl] WebGL context restored");
        invoke(&st.on_restored);
    });

    let _ =
        target.add_event_listener_with_callback("webglcontextlost", lost.as_ref().unchecked_ref());
    let _ = target.add_event_listener_with_callback(
        "webglcontextrestored",
        restored.as_ref().unchecked_ref(),
    );
    lost.forget();
    restored.forget();
}
