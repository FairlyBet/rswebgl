// Reverse-Z: depth 1 at the near plane, 0 at the far one, in a [0, 1] clip
// range (`EXT_clip_control`). Floats are densest near 0, which is now far
// away, so depth precision spreads evenly over distance instead of piling up
// at the near plane — no z-fighting at a distance, and an infinite far plane
// for free. Pays off with a float depth buffer (`Depth32f`); a 24-bit one
// gains less.
//
// The renderer makes it transparent to render states: under reverse-Z every
// depth comparison flips (`Less` → `Greater`, …) and depth clears become
// `1 − d`, so states and clear values keep meaning "nearer" and "far". The
// projection is the app's to switch (a reverse, zero-to-one one; the scene's
// cameras do it themselves).

use wasm_bindgen::prelude::*;

use crate::context::{Context, Core};
use crate::extension::Extension;

const LOWER_LEFT: u32 = 0x8CA1; // LOWER_LEFT_EXT
const NEGATIVE_ONE_TO_ONE: u32 = 0x935E; // NEGATIVE_ONE_TO_ONE_EXT
const ZERO_TO_ONE: u32 = 0x935F; // ZERO_TO_ONE_EXT

#[wasm_bindgen]
impl Context {
    /// Turns reverse-Z on or off (see the module docs). False (and nothing
    /// changes) where `EXT_clip_control` isn't available. Survives a context
    /// loss: it's re-applied on restore.
    #[wasm_bindgen(js_name = setReverseZ)]
    pub fn set_reverse_z(&self, on: bool) -> bool {
        if !self.enable_extension_quietly(Extension::ExtClipControl) {
            return !on;
        }
        self.core.reverse_z.set(on);
        apply(&self.core);
        // Depth funcs are applied flipped (or not) from now on: re-apply the
        // next render state in full.
        self.core.invalidate_state();
        true
    }

    /// Whether reverse-Z is on.
    #[wasm_bindgen(js_name = reverseZ)]
    pub fn reverse_z(&self) -> bool {
        self.core.reverse_z.get()
    }

    /// Whether reverse-Z is available (`EXT_clip_control`, enabled on first
    /// ask).
    #[wasm_bindgen(js_name = canReverseZ)]
    pub fn can_reverse_z(&self) -> bool {
        self.enable_extension_quietly(Extension::ExtClipControl)
    }
}

// Sets the clip-control state for the context's flag (GL loses it with the
// context).
pub(crate) fn apply(core: &Core) {
    let Some(ext) = core.gl.get_extension("EXT_clip_control").ok().flatten() else {
        return;
    };
    let depth = if core.reverse_z.get() {
        ZERO_TO_ONE
    } else {
        NEGATIVE_ONE_TO_ONE
    };
    if let Ok(f) = js_sys::Reflect::get(&ext, &"clipControlEXT".into())
        .and_then(|f| f.dyn_into::<js_sys::Function>())
    {
        let _ = f.call2(&ext, &LOWER_LEFT.into(), &depth.into());
    }
}
