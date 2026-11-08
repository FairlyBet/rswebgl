// WebGL context loss / restore.
//
// When the GPU process resets (driver crash, too many contexts, mobile tab in the
// background) the browser fires `webglcontextlost` on the canvas. Every GL object
// made before that point is dead for good; after `webglcontextrestored` the same
// `WebGl2RenderingContext` works again, but it's empty.
//
// The library's part: keep the restore possible (preventDefault on the lost
// event), clear the resource pools (every handle goes stale, and nothing tries
// to delete the dead objects), stop the renderer from issuing calls while lost,
// re-enable the extensions the app asked for, and tell the app via `on_lost` /
// `on_restored`. Recreating resources is the app's job, in `on_restored` — only
// it knows where the data came from.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{Event, EventTarget, WebglLoseContext};

use crate::console;
use crate::context::{Context, Core, note_lost};
use crate::extension::Extension;

type Callback = RefCell<Option<Box<dyn FnMut(&Context)>>>;

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
    pub(crate) fn new() -> Self {
        Self {
            extensions: RefCell::new(Vec::new()),
            on_lost: RefCell::new(None),
            on_restored: RefCell::new(None),
            lose_ext: RefCell::new(None),
        }
    }
}

// Runs a stored callback. It's taken out for the call so the callback itself
// may replace it (or create resources on the Context) without a RefCell double
// borrow; it's put back unless it was replaced meanwhile.
fn invoke(slot: &Callback, ctx: &Context) {
    let taken = slot.borrow_mut().take();
    if let Some(mut f) = taken {
        f(ctx);
        let mut s = slot.borrow_mut();
        if s.is_none() {
            *s = Some(f);
        }
    }
}

// Listens for loss/restore on the context's canvas (an HTMLCanvasElement or an
// OffscreenCanvas). The listeners are handed to JS and live as long as the
// canvas; they hold the context weakly, so they don't keep it alive.
pub(crate) fn install(core: &Rc<Core>) {
    let Some(target) = core
        .gl
        .canvas()
        .and_then(|c| c.dyn_into::<EventTarget>().ok())
    else {
        console::warn("[rswebgl] no canvas to listen on: context loss won't be handled");
        return;
    };

    let weak: Weak<Core> = Rc::downgrade(core);
    let lost = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
        // Without this the browser never restores the context.
        e.prevent_default();
        console::warn("[rswebgl] WebGL context lost");
        if let Some(core) = weak.upgrade() {
            if !core.counted_lost.replace(true) {
                note_lost(true);
            }
            core.epoch.set(core.epoch.get().wrapping_add(1));
            core.forget_all();
            let ctx = Context { core };
            invoke(&ctx.core.loss.on_lost, &ctx);
        }
    });

    let weak: Weak<Core> = Rc::downgrade(core);
    let restored = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
        console::log("[rswebgl] WebGL context restored");
        if let Some(core) = weak.upgrade() {
            core.invalidate_state();
            for ext in core.loss.extensions.borrow().iter() {
                let _ = core.gl.get_extension(ext.name());
            }
            if core.reverse_z.get() {
                crate::depth_range::apply(&core);
            }
            let ctx = Context { core };
            invoke(&ctx.core.loss.on_restored, &ctx);
            // Only now: rebuilding in `on_restored` (dropping old handles) stays
            // quiet.
            if ctx.core.counted_lost.replace(false) {
                note_lost(false);
            }
        }
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
