//! How `renderFrame` gets to the canvas: through the HDR target and display
//! pass (`crate::output`) where the device can render half floats, else
//! straight, with each shader tone mapping its own pixels.

use rswebgl::console;
use rswebgl::error::Error;
use rswebgl::pass::Pass;
use wasm_bindgen::prelude::*;

use super::Scene;
use crate::output::{HdrOutput, ToneMapping};

#[wasm_bindgen]
impl Scene {
    /// HDR rendering on or off (on by default where the device renders
    /// half-float targets). On, the scene shades linear radiance into an
    /// offscreen target and tone maps it once; off, it draws straight to the
    /// canvas, each shader tone mapping its pixels (cheaper on bandwidth,
    /// but blending happens after tone mapping). Returns whether HDR is on.
    #[wasm_bindgen(js_name = setHdr)]
    pub fn set_hdr(&mut self, on: bool) -> bool {
        self.hdr_wanted = on && HdrOutput::supported(&self.ctx);
        if !self.hdr_wanted
            && let Some(out) = self.hdr.take()
        {
            out.destroy(&self.ctx);
        }
        self.hdr_wanted
    }

    #[wasm_bindgen(js_name = isHdr)]
    pub fn is_hdr(&self) -> bool {
        self.hdr_wanted
    }

    /// Tone mapping: "neutral" (Khronos PBR Neutral, the default: true
    /// colors, soft highlights), "aces", "agx", or "none" (clip). Throws on
    /// another name.
    #[wasm_bindgen(js_name = setToneMapping)]
    pub fn set_tone_mapping(&mut self, name: &str) -> Result<(), Error> {
        self.tone_mapping = ToneMapping::from_name(name).ok_or_else(|| {
            Error::from(format!(
                "setToneMapping: \"{name}\" isn't none, neutral, aces or agx"
            ))
        })?;
        Ok(())
    }

    /// Temporal antialiasing instead of multisampling (with HDR output):
    /// the camera jitters by a subpixel each frame (Halton 2, 3) and frames
    /// accumulate into a history reprojected through the camera's motion and
    /// clipped to each pixel's neighborhood (ported from Bevy). Smooths
    /// edges, thin geometry and shading aliasing MSAA leaves; moving objects
    /// lean on the clip (no per-object motion vectors yet). Returns whether
    /// it is on (needs HDR output).
    #[wasm_bindgen(js_name = setTaa)]
    pub fn set_taa(&mut self, on: bool) -> bool {
        if self.taa != on {
            self.taa = on;
            self.taa_previous = None;
            if let Some(out) = self.hdr.take() {
                out.destroy(&self.ctx); // remade at the next frame
            }
        }
        self.taa && self.hdr_wanted
    }

    #[wasm_bindgen(js_name = isTaa)]
    pub fn is_taa(&self) -> bool {
        self.taa && self.hdr_wanted
    }

    /// Antialiasing samples per pixel of the HDR target (default 4; 0 or 1
    /// for none; lowered to what the device allows). Set the canvas's own
    /// `antialias: false` with HDR on: it isn't used.
    #[wasm_bindgen(js_name = setSamples)]
    pub fn set_samples(&mut self, samples: i32) {
        self.samples = samples.max(0);
        if let Some(out) = self.hdr.take() {
            out.destroy(&self.ctx); // remade at the next frame
        }
    }

    /// Dither the displayed colors (default on): hides banding in dark,
    /// smooth gradients at the cost of faint noise.
    #[wasm_bindgen(js_name = setDither)]
    pub fn set_dither(&mut self, on: bool) {
        self.dither = on;
    }
}

impl Scene {
    // Points `pass` at the HDR target (made or remade as needed) and the
    // renderer at linear output; or, without HDR, at the canvas with display
    // output. Returns whether HDR is in use this frame.
    pub(super) fn prepare_output(
        &mut self,
        pass: &mut Pass,
        late: &mut Pass,
        w: i32,
        h: i32,
    ) -> bool {
        if self.hdr.as_ref().is_some_and(|o| o.is_lost(&self.ctx))
            && let Some(out) = self.hdr.take()
        {
            out.destroy(&self.ctx);
        }
        if self.hdr_wanted && self.hdr.is_none() {
            match HdrOutput::new(&self.ctx, self.samples, self.taa) {
                Ok(out) => self.hdr = Some(out),
                Err(e) => self.give_up_hdr(&e),
            }
        }
        if let Some(out) = &mut self.hdr {
            match out.prepare_split(&self.ctx, w, h, pass, false) {
                Ok(()) => {
                    out.prepare_late(late);
                    self.renderer.set_output(false, self.tone_mapping);
                    return true;
                }
                Err(e) => self.give_up_hdr(&e),
            }
        }
        self.renderer.set_output(true, self.tone_mapping);
        pass.set_target(rswebgl::ids::FramebufferId::NULL);
        pass.set_clear(&rswebgl::framebuffer::ClearMask::color_depth());
        pass.clear_resolve();
        pass.set_invalidate(&rswebgl::framebuffer::InvalidateMask::none());
        false
    }

    fn give_up_hdr(&mut self, e: &Error) {
        console::warn(&format!(
            "[rswebgl-scene] HDR output unavailable ({e}); drawing straight to the canvas"
        ));
        self.hdr_wanted = false;
        if let Some(out) = self.hdr.take() {
            out.destroy(&self.ctx);
        }
    }
}
