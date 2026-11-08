//! Cameras, lights and the environment for JS.

use glam::Vec3;
use rswebgl::console;
use rswebgl::error::Error;
use rswebgl_ecs::Entity;
use wasm_bindgen::prelude::*;

use super::{Scene, options};
use crate::camera::Camera;
use crate::environment::{EnvBaker, PendingEnvironment};
use crate::hdr::{RgbeImage, decode_hdr_rgbe};
use crate::light::Light;

#[wasm_bindgen]
impl Scene {
    // --- cameras --------------------------------------------------------------

    /// Makes `e` a camera (see `CameraOptions`; glTF camera names). It looks
    /// down its local −Z: place it with `lookAt` or the transform setters.
    /// `renderFrame` keeps its aspect in step with the canvas. Throws on bad
    /// options.
    #[wasm_bindgen(js_name = setCamera)]
    pub fn set_camera(
        &mut self,
        e: Entity,
        #[wasm_bindgen(unchecked_param_type = "CameraOptions")] camera: JsValue,
    ) -> Result<(), Error> {
        let aspect = self.world.get::<Camera>(e).map_or(1.0, |c| c.aspect);
        let near = options::f32_or(&camera, "znear", 0.1)?;
        let far = options::f32_or(&camera, "zfar", 1000.0)?;
        if !(near > 0.0 && far > near) {
            return Err("setCamera: needs 0 < znear < zfar".into());
        }
        let cam = match options::string(&camera, "type")?.as_str() {
            "perspective" => {
                let fov = options::f32_or(&camera, "yfov", std::f32::consts::FRAC_PI_4)?;
                Camera::perspective(fov, aspect, near, far)
            }
            "orthographic" => {
                let ymag = options::f32_or(&camera, "ymag", 1.0)?;
                Camera::orthographic(ymag, aspect, near, far)
            }
            other => {
                return Err(format!(
                    "setCamera: type \"{other}\" isn't \"perspective\" or \"orthographic\""
                )
                .into());
            }
        };
        self.insert_camera(e, cam);
        Ok(())
    }

    /// Width / height of the target, for cameras used with `render` into a
    /// pass of yours (`renderFrame` sets it from the canvas).
    #[wasm_bindgen(js_name = setCameraAspect)]
    pub fn set_camera_aspect(&mut self, e: Entity, aspect: f32) {
        match self.world.get_mut::<Camera>(e) {
            Some(mut c) => c.aspect = aspect,
            None => console::error_once(
                "[rswebgl-scene] setCameraAspect: the entity is not a camera; ignored",
            ),
        }
    }

    // --- lights ---------------------------------------------------------------

    /// Makes `e` a punctual light (see `LightOptions`; KHR_lights_punctual
    /// names and units). Directional and spot lights shine down the entity's
    /// −Z: aim them with `lookAt`. Up to 8 lights reach the shaders. Throws on
    /// bad options.
    #[wasm_bindgen(js_name = setLight)]
    pub fn set_light(
        &mut self,
        e: Entity,
        #[wasm_bindgen(unchecked_param_type = "LightOptions")] light: JsValue,
    ) -> Result<(), Error> {
        let color = options::vec3_or(&light, "color", Vec3::ONE)?;
        let intensity = options::f32_or(&light, "intensity", 1.0)?;
        let range = options::f32_or(&light, "range", 0.0)?;
        let l = match options::string(&light, "type")?.as_str() {
            "directional" => Light::directional(color, intensity),
            "point" => Light::point(color, intensity, range),
            "spot" => {
                let inner = options::f32_or(&light, "innerConeAngle", 0.0)?;
                let outer = options::f32_or(&light, "outerConeAngle", std::f32::consts::FRAC_PI_4)?;
                if !(0.0 <= inner && inner < outer && outer <= std::f32::consts::FRAC_PI_2) {
                    return Err("setLight: needs 0 ≤ innerConeAngle < outerConeAngle ≤ π/2".into());
                }
                Light::spot(color, intensity, range, inner, outer)
            }
            other => {
                return Err(format!(
                    "setLight: type \"{other}\" isn't \"directional\", \"point\" or \"spot\""
                )
                .into());
            }
        };
        self.insert_light(e, l);
        Ok(())
    }

    #[wasm_bindgen(js_name = removeLight)]
    pub fn remove_light(&mut self, e: Entity) {
        self.world.remove::<Light>(e);
    }

    /// Scales radiance before tone mapping. Default 1.
    #[wasm_bindgen(js_name = setExposure)]
    pub fn set_exposure(&mut self, exposure: f32) {
        self.renderer.set_exposure(exposure);
    }

    /// Geometric specular antialiasing: where the normal turns within a
    /// pixel, roughness grows (by up to `threshold` in α², `variance`
    /// scaling how fast), so sharp highlights on curved smooth surfaces
    /// don't sparkle. Higher is calmer and softer; variance 0 turns it off.
    /// Default (0.15, 0.2), as Filament.
    #[wasm_bindgen(js_name = setSpecularAntialiasing)]
    pub fn set_specular_antialiasing(&mut self, variance: f32, threshold: f32) {
        self.renderer.set_specular_antialiasing(variance, threshold);
    }

    /// `[variance, threshold]` of the specular antialiasing.
    #[wasm_bindgen(js_name = specularAntialiasing)]
    pub fn specular_antialiasing(&self) -> Vec<f32> {
        let (v, t) = self.renderer.specular_antialiasing();
        vec![v, t]
    }

    // --- environment ----------------------------------------------------------

    /// Scales the environment's light (linear RGB). Without an environment
    /// (the default white one) it's a flat ambient light: default 0.2.
    /// `setEnvironmentHdr` resets it to 1 (an intensity set while that bakes
    /// is kept).
    #[wasm_bindgen(js_name = setEnvironmentIntensity)]
    pub fn set_environment_intensity(&mut self, r: f32, g: f32, b: f32) {
        let rgb = Vec3::new(r, g, b);
        self.renderer.set_environment_intensity(rgb);
        if self.env_pending.is_some() {
            self.env_intensity_after_bake = Some(rgb);
        }
    }

    /// Lights the scene with an HDR environment: the bytes of a Radiance
    /// `.hdr` equirectangular image (fetch it yourself), baked on the GPU into
    /// cube maps `size` texels per face (256 is a good default). The file is
    /// read and uploaded now (throws if it can't be read); the bake runs in
    /// the background over the next few frames — the previous environment
    /// shows until it's done, and `whenReady` waits for it. A bake that fails
    /// (no half-float render targets) is reported on the console.
    /// The bytes are kept (a copy) to bake it again after a context loss.
    #[wasm_bindgen(js_name = setEnvironmentHdr)]
    pub fn set_environment_hdr(&mut self, bytes: &[u8], size: u32) -> Result<(), Error> {
        self.sync_context();
        let image = decode_hdr_rgbe(bytes)?;
        self.start_environment(&image, size)?;
        // Kept to bake again after a context loss.
        self.env_source = Some((bytes.to_vec(), size));
        Ok(())
    }

    /// Back to the default: a flat white ambient light at intensity 0.2.
    #[wasm_bindgen(js_name = clearEnvironment)]
    pub fn clear_environment(&mut self) {
        self.sync_context();
        self.env_source = None;
        if let Some(p) = self.env_pending.take() {
            p.cancel();
        }
        self.renderer.clear_environment(&self.ctx);
    }

    /// Whether an HDR environment is set (or baking).
    #[wasm_bindgen(js_name = hasEnvironment)]
    pub fn has_environment(&self) -> bool {
        self.renderer.has_environment() || self.env_pending.is_some()
    }

    /// Turns the environment (light and background) about +Y, in radians.
    #[wasm_bindgen(js_name = setEnvironmentRotation)]
    pub fn set_environment_rotation(&mut self, radians: f32) {
        self.renderer.set_environment_rotation(radians);
    }

    /// Shows the environment behind the scene, `blur` 0 (sharp) … 1.
    #[wasm_bindgen(js_name = setBackground)]
    pub fn set_background(&mut self, blur: f32) {
        self.renderer.set_background(Some(blur));
        // Start compiling now; `whenReady` waits for it.
        self.renderer.background_program(&self.ctx);
    }

    /// No background: the clear color shows.
    #[wasm_bindgen(js_name = clearBackground)]
    pub fn clear_background(&mut self) {
        self.renderer.set_background(None);
    }
}

impl Scene {
    /// `setCamera` from Rust.
    pub fn insert_camera(&mut self, e: Entity, camera: Camera) {
        self.insert(e, "setCamera", camera);
    }

    /// `setLight` from Rust.
    pub fn insert_light(&mut self, e: Entity, light: Light) {
        self.insert(e, "setLight", light);
    }
}

impl Scene {
    // Starts baking `image` in the background (the bake shaders made on
    // first use), cancelling a bake still running.
    pub(crate) fn start_environment(&mut self, image: &RgbeImage, size: u32) -> Result<(), Error> {
        if let Some(p) = self.env_pending.take() {
            p.cancel();
        }
        self.env_intensity_after_bake = None;
        let baker = match &self.env_baker {
            Some(b) => b.clone(),
            None => {
                let b = std::rc::Rc::new(EnvBaker::new(&self.ctx)?);
                self.env_baker = Some(b.clone());
                b
            }
        };
        self.env_pending = Some(PendingEnvironment::start(&self.ctx, baker, image, size)?);
        Ok(())
    }

    // Swaps in a bake that has finished (at most once a frame).
    pub(crate) fn apply_environment(&mut self) {
        let Some(result) = self.env_pending.as_ref().and_then(|p| p.take()) else {
            return;
        };
        self.env_pending = None;
        match result {
            Ok(env) => {
                self.renderer.set_environment(&self.ctx, env);
                if let Some(rgb) = self.env_intensity_after_bake.take() {
                    self.renderer.set_environment_intensity(rgb);
                }
            }
            Err(e) => console::error(&format!("[rswebgl-scene] environment: {e}")),
        }
    }
}
