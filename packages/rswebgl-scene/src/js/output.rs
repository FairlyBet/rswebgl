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
    /// accumulate into a history reprojected through the camera's motion —
    /// and, for meshes that moved (animation, skinning, morph targets, your
    /// own transforms), their own motion vectors — clipped to each pixel's
    /// neighborhood (ported from Bevy and Godot). Smooths edges, thin
    /// geometry and shading aliasing MSAA leaves; pair it with
    /// `setSharpening`. Returns whether it is on (needs HDR output).
    #[wasm_bindgen(js_name = setTaa)]
    pub fn set_taa(&mut self, on: bool) -> bool {
        if self.taa != on {
            self.taa = on;
            self.taa_previous = None;
            if let Some(out) = self.hdr.take() {
                out.destroy(&self.ctx); // remade at the next frame
            }
        }
        // Its shader compiling from now (whenReady waits for it).
        let on = self.taa && self.hdr_wanted;
        self.renderer.set_motion_vectors(&self.ctx, on);
        on
    }

    #[wasm_bindgen(js_name = isTaa)]
    pub fn is_taa(&self) -> bool {
        self.taa && self.hdr_wanted
    }

    /// FXAA (fast approximate antialiasing, ported from three.js) on the
    /// displayed image, with HDR output: one cheap fullscreen pass that
    /// smooths edges by their contrast — the phone-friendly choice where
    /// MSAA or TAA cost too much (set `setSamples(0)` with it). Softens fine
    /// texture detail a little. Combines with MSAA or TAA too. Returns
    /// whether it is on (needs HDR output).
    #[wasm_bindgen(js_name = setFxaa)]
    pub fn set_fxaa(&mut self, on: bool) -> bool {
        self.fxaa = on;
        // Compiled now when the target exists, else at the next frame.
        if let Some(out) = &mut self.hdr
            && let Err(e) = out.set_fxaa(&self.ctx, on)
        {
            self.give_up_fxaa(&e);
        }
        self.is_fxaa()
    }

    #[wasm_bindgen(js_name = isFxaa)]
    pub fn is_fxaa(&self) -> bool {
        self.fxaa && self.hdr_wanted
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

    /// Sharpening of the displayed image, 0 (off, the default) to 1: AMD's
    /// RCAS (FSR 1), which lifts detail without ringing or clipping. Gives
    /// back what TAA's history filtering softens (~0.3–0.6 with `setTaa`).
    /// With HDR output only.
    #[wasm_bindgen(js_name = setSharpening)]
    pub fn set_sharpening(&mut self, amount: f32) {
        self.sharpness = if amount.is_finite() {
            amount.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    pub fn sharpening(&self) -> f32 {
        self.sharpness
    }

    /// The resolution of the opaque-scene copy transmissive surfaces (glass)
    /// see through, as a share of the canvas: 1 (the default) full size,
    /// down to 0.25. Lower saves memory and the copy's time on phones;
    /// what shows through glass gets blurrier. With HDR output.
    #[wasm_bindgen(js_name = setTransmissionResolution)]
    pub fn set_transmission_resolution(&mut self, scale: f32) {
        self.transmission_scale = if scale.is_finite() {
            scale.clamp(0.25, 1.0)
        } else {
            1.0
        };
    }

    #[wasm_bindgen(js_name = transmissionResolution)]
    pub fn transmission_resolution(&self) -> f32 {
        self.transmission_scale
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
            let mut made = HdrOutput::new(&self.ctx, self.samples, self.taa);
            // A TAA failure (its shader, a target) costs TAA, not HDR.
            if let Err(e) = &made
                && self.taa
            {
                self.give_up_taa(e);
                made = HdrOutput::new(&self.ctx, self.samples, false);
            }
            match made {
                Ok(out) => self.hdr = Some(out),
                Err(e) => self.give_up_hdr(&e),
            }
        }
        if let Some(out) = &mut self.hdr
            && let Err(e) = out.set_fxaa(&self.ctx, self.fxaa)
        {
            self.give_up_fxaa(&e);
        }
        if let Some(out) = &mut self.hdr {
            out.set_transmission_scale(self.transmission_scale);
            match out.prepare_split(&self.ctx, w, h, pass, false) {
                Ok(()) => {
                    out.prepare_late(late);
                    self.renderer.set_output(false, self.tone_mapping);
                    return true;
                }
                Err(e) if self.taa => {
                    // Without TAA's targets at the next frame.
                    self.give_up_taa(&e);
                    if let Some(out) = self.hdr.take() {
                        out.destroy(&self.ctx);
                    }
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

    // TAA off for good (`isTaa` answers false); HDR output stays.
    fn give_up_taa(&mut self, e: &Error) {
        console::warn(&format!(
            "[rswebgl-scene] TAA unavailable ({e}); HDR output without it"
        ));
        self.taa = false;
        self.taa_previous = None;
        self.renderer.set_motion_vectors(&self.ctx, false);
    }

    // FXAA off for good (`isFxaa` answers false).
    fn give_up_fxaa(&mut self, e: &Error) {
        console::warn(&format!(
            "[rswebgl-scene] FXAA unavailable ({e}); output without it"
        ));
        self.fxaa = false;
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
