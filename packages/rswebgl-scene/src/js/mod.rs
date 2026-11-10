//! The scene for JS: one `Scene` object owning the world, the transform system
//! and the scene renderer. Entities are plain numbers; components are set and
//! read through methods taking the entity — no JS object per entity, nothing
//! to `.free()` but the scene itself (`destroy`).
//!
//! ```js
//! const scene = new Scene(ctx);
//! const ring = scene.addModel(await loadModel(ctx, url)); // built-in PBR
//! scene.setEnvironmentHdr(new Uint8Array(await (await fetch(hdr)).arrayBuffer()), 256);
//!
//! const camera = scene.spawn();
//! scene.setCamera(camera, { type: "perspective", yfov: Math.PI / 4 });
//! scene.lookAt(camera, [0, 0.05, 0.15], [0, 0, 0]);
//!
//! const gold = scene.findMaterial(ring, "Gold");
//! scene.setMaterialProperty(gold, "baseColorFactor", [1, 0.78, 0.34, 1]);
//!
//! function frame() {
//!   scene.renderFrame(camera); // transforms → culling → draws → canvas
//!   requestAnimationFrame(frame);
//! }
//! ```
//!
//! Setters on a dead entity report once in the console and do nothing;
//! malformed option objects and unknown material properties throw.

mod animation;
mod lights;
mod materials;
mod openpbr;
mod options;
mod output;
mod restore;

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec2, Vec3};
use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::draw::DrawCommand;
use rswebgl::error::Error;
use rswebgl::framebuffer::ClearMask;
use rswebgl::ids::{ParamGroupId, ProgramId, StateId, VertexArrayId};
use rswebgl::pass::Pass;
use rswebgl_ecs::{Entity, World};
use rswebgl_gltf::Model;
use wasm_bindgen::prelude::*;

use crate::culling::Bounds;
use crate::hierarchy::{TransformSystem, descendants, despawn_recursive, set_parent};
use crate::model::{Name, find_by_name, spawn_model};
use crate::pbr::PbrShaders;
use crate::render::{MeshRenderer, RenderStats, SceneRenderer, Visible};
use crate::shader::create_scene_program;
use crate::transform::{Parent, Transform, WorldMatrix};

use materials::{LoadedTexture, MaterialRecord};
use restore::{Reloads, SceneProgram};

/// A scene: entities with transforms, hierarchy, cameras, lights and meshes.
/// `renderFrame` draws it to the canvas; `render` into a pass of yours.
/// After a context loss it rebuilds itself at its next call (see
/// `restore.rs`): what it made comes back, models are fetched again from
/// their URL, and material handles the app holds stay valid. What the app
/// made itself through the context is the app's to rebuild.
#[wasm_bindgen]
pub struct Scene {
    ctx: Context,
    world: World,
    transforms: TransformSystem,
    renderer: SceneRenderer,
    // Models added with `addModel`, by their root entity: their GPU
    // resources go when the root is despawned.
    models: HashMap<Entity, Model>,
    // Every material the scene knows (models' and clones), by group.
    materials: HashMap<ParamGroupId, MaterialRecord>,
    pbr: PbrShaders,
    // Textures `loadTexture` made: freed with the scene, reloaded after a
    // context loss.
    textures: Vec<LoadedTexture>,
    // The context generation the scene's GPU objects belong to.
    generation: u32,
    // `createProgram` shaders, recompiled after a loss.
    scene_programs: Vec<SceneProgram>,
    // A `createProgram` handle from before a loss → its recompiled shader
    // (`currentProgram`; setMesh and addModelWithProgram take either).
    program_aliases: HashMap<ProgramId, ProgramId>,
    // Whether models are fetched again after a loss (`setRestoreModels`).
    restore_models: bool,
    // The `.hdr` bytes and cube size of the environment, baked again after
    // a loss.
    env_source: Option<(Vec<u8>, u32)>,
    // The environment bake's shaders (made on first use) and the bake in
    // progress, swapped in at the first frame after it finishes.
    env_baker: Option<std::rc::Rc<crate::environment::EnvBaker>>,
    env_pending: Option<crate::environment::PendingEnvironment>,
    // An intensity set while a bake runs, kept over its swap (which resets
    // to 1).
    env_intensity_after_bake: Option<Vec3>,
    // GPU time per pass of renderFrame, when asked for.
    profiler: Option<rswebgl::profiler::GpuProfiler>,
    // Models reloaded after a loss, applied at the next frame.
    reloads: Reloads,
    // HDR output: wanted (and supported), its target once made, how it is
    // displayed.
    hdr_wanted: bool,
    hdr: Option<crate::output::HdrOutput>,
    samples: i32,
    // Temporal antialiasing: on, the frame count (jitter phase) and the
    // last frame's unjittered view-projection and camera.
    taa: bool,
    taa_frame: u32,
    taa_previous: Option<(Entity, Mat4)>,
    // FXAA after the display pass (`setFxaa`).
    fxaa: bool,
    tone_mapping: crate::output::ToneMapping,
    dither: bool,
    // RCAS strength in the display pass (0 off).
    sharpness: f32,
    // The transmission copy's share of the target (`setTransmissionResolution`).
    transmission_scale: f32,
    // The canvas pass `renderFrame` reuses.
    frame_pass: Pass,
    // Its second pass with HDR output: what draws after the opaque scene.
    late_pass: Pass,
    // Transmissive meshes when depth-peeled (`setTransmissionLayers`).
    glass_pass: Pass,
    transmission_layers: usize,
    stats: RenderStats,
    // PBR variant switches waiting for the new program to link: the mesh
    // keeps drawing with its old one meanwhile.
    pending_variants: Vec<(Entity, ProgramId)>,
}

// A rejected call on a dead entity: reported once per call site, no-op.
fn dead(op: &str) {
    console::error_once(&format!(
        "[rswebgl-scene] {op}: the entity is not alive; ignored"
    ));
}

// Three floats from a slice, or None (reported once) for another length.
fn vec3(op: &str, what: &str, v: &[f32]) -> Option<Vec3> {
    match v {
        [x, y, z] => Some(Vec3::new(*x, *y, *z)),
        _ => {
            console::error_once(&format!(
                "[rswebgl-scene] {op}: {what} needs 3 numbers; ignored"
            ));
            None
        }
    }
}

#[wasm_bindgen]
impl Scene {
    #[wasm_bindgen(constructor)]
    pub fn new(ctx: &Context) -> Result<Scene, Error> {
        // Shaders link in the background: a mesh whose shader isn't ready
        // is left out of the frame instead of stalling it (`whenReady`).
        ctx.enable_extension_quietly(rswebgl::extension::Extension::KhrParallelShaderCompile);
        let mut frame_pass = Pass::to_default();
        frame_pass.set_clear(&ClearMask::color_depth());
        Ok(Self {
            ctx: ctx.clone(),
            world: World::new(),
            transforms: TransformSystem::new(),
            renderer: SceneRenderer::new(ctx)?,
            models: HashMap::new(),
            materials: HashMap::new(),
            pbr: PbrShaders::new(),
            textures: Vec::new(),
            generation: ctx.generation(),
            scene_programs: Vec::new(),
            program_aliases: HashMap::new(),
            restore_models: true,
            env_source: None,
            env_baker: None,
            env_pending: None,
            env_intensity_after_bake: None,
            profiler: None,
            reloads: Reloads::default(),
            hdr_wanted: crate::output::HdrOutput::supported(ctx),
            hdr: None,
            samples: 4,
            taa: false,
            taa_frame: 0,
            taa_previous: None,
            fxaa: false,
            tone_mapping: crate::output::ToneMapping::Neutral,
            dither: true,
            sharpness: 0.0,
            transmission_scale: 1.0,
            frame_pass,
            late_pass: Pass::to_default(),
            glass_pass: Pass::to_default(),
            transmission_layers: 0,
            stats: RenderStats::default(),
            pending_variants: Vec::new(),
        })
    }

    // --- entities -------------------------------------------------------------

    /// A new entity with an identity transform.
    pub fn spawn(&mut self) -> Entity {
        let e = self.world.spawn();
        self.world.insert(e, Transform::IDENTITY);
        e
    }

    /// Despawns `e` and everything under it (a model added with `addModel`,
    /// despawned by its root, frees its GPU resources and materials too).
    /// Returns how many entities went.
    pub fn despawn(&mut self, e: Entity) -> usize {
        // Every model root in the subtree, not just `e`: a model hung under a
        // group goes with the group.
        if !self.models.is_empty() {
            for d in descendants(&self.world, e) {
                if let Some(model) = self.models.remove(&d) {
                    for g in model.material_groups() {
                        self.materials.remove(g);
                    }
                    model.destroy(&self.ctx);
                }
            }
        }
        despawn_recursive(&mut self.world, e)
    }

    #[wasm_bindgen(js_name = isAlive)]
    pub fn is_alive(&self, e: Entity) -> bool {
        self.world.is_alive(e)
    }

    #[wasm_bindgen(js_name = entityCount)]
    pub fn entity_count(&self) -> usize {
        self.world.len()
    }

    // --- transforms -----------------------------------------------------------

    #[wasm_bindgen(js_name = setTranslation)]
    pub fn set_translation(&mut self, e: Entity, x: f32, y: f32, z: f32) {
        self.edit_transform(e, "setTranslation", |t| t.translation = Vec3::new(x, y, z));
    }

    /// A unit quaternion (x, y, z, w); normalized on the way in.
    #[wasm_bindgen(js_name = setRotation)]
    pub fn set_rotation(&mut self, e: Entity, x: f32, y: f32, z: f32, w: f32) {
        let q = Quat::from_xyzw(x, y, z, w).normalize();
        self.edit_transform(e, "setRotation", |t| t.rotation = q);
    }

    /// Rotation from Euler angles in radians, applied Y, then X, then Z.
    #[wasm_bindgen(js_name = setRotationEuler)]
    pub fn set_rotation_euler(&mut self, e: Entity, x: f32, y: f32, z: f32) {
        let q = Quat::from_euler(glam::EulerRot::YXZ, y, x, z);
        self.edit_transform(e, "setRotationEuler", |t| t.rotation = q);
    }

    #[wasm_bindgen(js_name = setScale)]
    pub fn set_scale(&mut self, e: Entity, x: f32, y: f32, z: f32) {
        self.edit_transform(e, "setScale", |t| t.scale = Vec3::new(x, y, z));
    }

    /// Sets the local transform from 16 floats, column-major (no shear).
    #[wasm_bindgen(js_name = setLocalMatrix)]
    pub fn set_local_matrix(
        &mut self,
        e: Entity,
        #[wasm_bindgen(unchecked_param_type = "ArrayLike<number>")] m: &[f32],
    ) {
        let Ok(cols) = <[f32; 16]>::try_from(m) else {
            console::error_once("[rswebgl-scene] setLocalMatrix needs 16 numbers; ignored");
            return;
        };
        let t = Transform::from_matrix(Mat4::from_cols_array(&cols));
        self.edit_transform(e, "setLocalMatrix", |old| *old = t);
    }

    /// Places `e` at `eye` looking at `target`, +Y up — for cameras and
    /// lights. In its parent's space if it has one.
    #[wasm_bindgen(js_name = lookAt)]
    pub fn look_at(
        &mut self,
        e: Entity,
        #[wasm_bindgen(unchecked_param_type = "ArrayLike<number>")] eye: &[f32],
        #[wasm_bindgen(unchecked_param_type = "ArrayLike<number>")] target: &[f32],
    ) {
        let (Some(eye), Some(target)) =
            (vec3("lookAt", "eye", eye), vec3("lookAt", "target", target))
        else {
            return;
        };
        // Looking straight up or down: any other up axis will do.
        let up = if (target - eye).normalize_or_zero().y.abs() > 0.999 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let t = Transform::from_matrix(Mat4::look_at_rh(eye, target, up).inverse());
        self.edit_transform(e, "lookAt", |old| *old = t);
    }

    /// Local translation (3 numbers), or empty if `e` has no transform.
    pub fn translation(&self, e: Entity) -> Vec<f32> {
        self.world
            .get::<Transform>(e)
            .map_or_else(Vec::new, |t| t.translation.to_array().to_vec())
    }

    /// Local rotation quaternion (x, y, z, w), or empty.
    pub fn rotation(&self, e: Entity) -> Vec<f32> {
        self.world
            .get::<Transform>(e)
            .map_or_else(Vec::new, |t| t.rotation.to_array().to_vec())
    }

    /// Local scale (3 numbers), or empty.
    pub fn scale(&self, e: Entity) -> Vec<f32> {
        self.world
            .get::<Transform>(e)
            .map_or_else(Vec::new, |t| t.scale.to_array().to_vec())
    }

    /// World matrix (16 floats, column-major) as of the last frame /
    /// `updateTransforms`, or empty if it hasn't got one yet. A new array
    /// each call; `copyWorldMatrix` fills one of yours instead.
    #[wasm_bindgen(js_name = worldMatrix)]
    pub fn world_matrix(&self, e: Entity) -> Vec<f32> {
        self.world
            .get::<WorldMatrix>(e)
            .map_or_else(Vec::new, |m| m.0.to_cols_array().to_vec())
    }

    /// Writes the world matrix into `out` (16 floats); false (and `out`
    /// untouched) if `e` has none yet.
    #[wasm_bindgen(js_name = copyWorldMatrix)]
    pub fn copy_world_matrix(&self, e: Entity, out: &mut [f32]) -> bool {
        match (self.world.get::<WorldMatrix>(e), out.len() >= 16) {
            (Some(m), true) => {
                out[..16].copy_from_slice(&m.0.to_cols_array());
                true
            }
            (Some(_), false) => {
                console::error_once("[rswebgl-scene] copyWorldMatrix needs room for 16 floats");
                false
            }
            (None, _) => false,
        }
    }

    /// World position (3 numbers), or empty.
    #[wasm_bindgen(js_name = worldPosition)]
    pub fn world_position(&self, e: Entity) -> Vec<f32> {
        self.world
            .get::<WorldMatrix>(e)
            .map_or_else(Vec::new, |m| m.0.w_axis.truncate().to_array().to_vec())
    }

    /// Brings world matrices up to date now (a frame does it too) — for
    /// reading them right after moving things.
    #[wasm_bindgen(js_name = updateTransforms)]
    pub fn update_transforms(&mut self) {
        self.transforms.update(&mut self.world);
    }

    // --- hierarchy ------------------------------------------------------------

    /// Makes `child`'s transform relative to `parent`'s. False (and an error
    /// in the console) for a cycle or a dead entity.
    #[wasm_bindgen(js_name = setParent)]
    pub fn set_parent(&mut self, child: Entity, parent: Entity) -> bool {
        match set_parent(&mut self.world, child, Some(parent)) {
            Ok(()) => true,
            Err(err) => {
                console::error(&format!("[rswebgl-scene] setParent: {err}; ignored"));
                false
            }
        }
    }

    #[wasm_bindgen(js_name = clearParent)]
    pub fn clear_parent(&mut self, child: Entity) {
        let _ = set_parent(&mut self.world, child, None);
    }

    /// The parent of `e`, or `0` for a root.
    pub fn parent(&self, e: Entity) -> Entity {
        self.world.get::<Parent>(e).map_or(Entity::NULL, |p| p.0)
    }

    #[wasm_bindgen(js_name = setName)]
    pub fn set_name(&mut self, e: Entity, name: String) {
        self.insert(e, "setName", Name(name));
    }

    pub fn name(&self, e: Entity) -> Option<String> {
        self.world.get::<Name>(e).map(|n| n.0.clone())
    }

    /// The first entity named `name` at or under `root`, or `0`. Scans every
    /// name: look parts up once and keep the number.
    #[wasm_bindgen(js_name = findByName)]
    pub fn find_by_name(&self, root: Entity, name: &str) -> Entity {
        find_by_name(&self.world, root, name).unwrap_or(Entity::NULL)
    }

    /// The camera entities at or under `root` — a model's glTF cameras,
    /// parents before children. Render through one with `renderFrame(camera)`.
    pub fn cameras(&self, root: Entity) -> Vec<u32> {
        self.entities_with::<crate::camera::Camera>(root)
    }

    /// The light entities at or under `root` — a model's
    /// `KHR_lights_punctual` lights, which light the scene once it is added.
    /// Turn one off with `setVisible(e, false)` or `removeLight`.
    pub fn lights(&self, root: Entity) -> Vec<u32> {
        self.entities_with::<crate::light::Light>(root)
    }

    fn entities_with<C: 'static>(&self, root: Entity) -> Vec<u32> {
        descendants(&self.world, root)
            .into_iter()
            .filter(|&e| self.world.get::<C>(e).is_some())
            .map(Entity::to_raw)
            .collect()
    }

    // --- meshes ---------------------------------------------------------------

    /// Makes `e` draw `draw` with your own renderer handles (see
    /// `MeshOptions`); they stay yours to destroy. Throws on bad options.
    #[wasm_bindgen(js_name = setMesh)]
    pub fn set_mesh(
        &mut self,
        e: Entity,
        #[wasm_bindgen(unchecked_param_type = "MeshOptions")] mesh: JsValue,
        draw: &DrawCommand,
    ) -> Result<(), Error> {
        let mesh = MeshRenderer {
            program: self
                .current_program(ProgramId::from_raw(options::u32_or(&mesh, "program", 0)?)),
            state: StateId::from_raw(options::u32_or(&mesh, "state", 0)?),
            vao: VertexArrayId::from_raw(options::u32_or(&mesh, "vao", 0)?),
            material: ParamGroupId::from_raw(options::u32_or(&mesh, "material", 0)?),
            command: *draw,
            transparent: options::bool_or(&mesh, "transparent", false)?,
            transmissive: options::bool_or(&mesh, "transmissive", false)?,
        };
        if mesh.program.is_null() || mesh.state.is_null() {
            return Err("setMesh: \"program\" and \"state\" are required".into());
        }
        self.pending_variants.retain(|&(p, _)| p != e);
        self.insert(e, "setMesh", mesh);
        Ok(())
    }

    #[wasm_bindgen(js_name = removeMesh)]
    pub fn remove_mesh(&mut self, e: Entity) {
        self.world.remove::<MeshRenderer>(e);
    }

    /// Local bounding box, in the space of `e`'s transform: enables frustum
    /// culling and box picking for it.
    #[wasm_bindgen(js_name = setBounds)]
    pub fn set_bounds(
        &mut self,
        e: Entity,
        #[wasm_bindgen(unchecked_param_type = "ArrayLike<number>")] min: &[f32],
        #[wasm_bindgen(unchecked_param_type = "ArrayLike<number>")] max: &[f32],
    ) {
        if let (Some(min), Some(max)) =
            (vec3("setBounds", "min", min), vec3("setBounds", "max", max))
        {
            self.insert(e, "setBounds", Bounds::new(min, max));
        }
    }

    /// Hides or shows `e` and everything under it.
    #[wasm_bindgen(js_name = setVisible)]
    pub fn set_visible(&mut self, e: Entity, visible: bool) {
        self.insert(e, "setVisible", Visible(visible));
    }

    /// Compiles a scene shader: your code on top of the scene's preludes
    /// (camera, lights, environment, object; `scene_irradiance`,
    /// `scene_reflection`, `scene_output`), without `#version` or those
    /// declarations. The program is yours to destroy (`ctx.destroyProgram`).
    #[wasm_bindgen(js_name = createProgram)]
    pub fn create_program(&mut self, vert: &str, frag: &str) -> Result<ProgramId, Error> {
        self.sync_context();
        let id = create_scene_program(&self.ctx, vert, frag)?;
        self.scene_programs.push(SceneProgram {
            id,
            vert: vert.to_string(),
            frag: frag.to_string(),
        });
        Ok(id)
    }

    // --- models ---------------------------------------------------------------

    /// Spawns a loaded model under a new root entity and returns the root,
    /// shaded with the built-in glTF PBR (a variant per primitive and
    /// material, compiled on first use). The scene takes the model over: its
    /// GPU resources are freed when the root is despawned (or the scene
    /// destroyed). Parts keep their glTF node names (`findByName`), materials
    /// theirs (`findMaterial`).
    #[wasm_bindgen(js_name = addModel)]
    pub fn add_model(&mut self, model: Model) -> Entity {
        self.sync_context();
        let (ctx, pbr) = (&self.ctx, &mut self.pbr);
        let root = spawn_model(&mut self.world, &model, |item| {
            pbr.program_for(ctx, &model, item)
                .map_err(|e| {
                    console::error_once(&format!(
                        "[rswebgl-scene] a PBR shader variant failed to build: {e}"
                    ))
                })
                .ok()
        });
        self.register_materials(&model);
        self.keep_model(root, model);
        root
    }

    /// `addModel` with your own `program` for every primitive (a scene
    /// shader from `createProgram` sees the camera, lights, environment and
    /// object; the material group is the loader's).
    #[wasm_bindgen(js_name = addModelWithProgram)]
    pub fn add_model_with_program(&mut self, model: Model, program: ProgramId) -> Entity {
        self.sync_context();
        let program = self.current_program(program);
        let root = spawn_model(&mut self.world, &model, |_| Some(program));
        self.register_materials(&model);
        self.keep_model(root, model);
        root
    }

    /// Bounding sphere of a model added with `addModel`, as loaded (before
    /// any transform of yours): `[cx, cy, cz, radius]`, or empty for an
    /// entity that isn't a model root. For framing a camera.
    #[wasm_bindgen(js_name = modelBounds)]
    pub fn model_bounds(&self, root: Entity) -> Vec<f32> {
        self.models.get(&root).map_or_else(Vec::new, |m| {
            let mut v = m.center();
            v.push(m.radius());
            v
        })
    }

    // --- frames ---------------------------------------------------------------

    /// One frame to the canvas, seen from `camera`: its aspect follows the
    /// canvas, world matrices update, invisible meshes are culled, the rest
    /// sorted and drawn over a cleared target (the canvas's clear color:
    /// `ctx.defaultFramebuffer().setClearColor`). Returns the draws made.
    #[wasm_bindgen(js_name = renderFrame)]
    pub fn render_frame(&mut self, camera: Entity) -> usize {
        let _span = tracing::info_span!("renderFrame").entered();
        let drawn = self.render_frame_inner(camera);
        rswebgl::trace::gpu_fence(&self.ctx);
        drawn
    }

    fn render_frame_inner(&mut self, camera: Entity) -> usize {
        let fb = self.ctx.default_framebuffer();
        let (w, h) = (fb.width(), fb.height());
        let aspect = w.max(1) as f32 / h.max(1) as f32;
        if let Some(mut c) = self.world.get_mut::<crate::camera::Camera>(camera) {
            c.set_if_neq(crate::camera::Camera { aspect, ..*c });
        }
        if !self.sync_context() {
            return 0; // lost: nothing to draw with
        }
        let mut pass = std::mem::replace(&mut self.frame_pass, Pass::to_default());
        let mut late = std::mem::replace(&mut self.late_pass, Pass::to_default());
        pass.clear_draws();
        late.clear_draws();
        let hdr = {
            let _s = tracing::info_span!("scene.prepareOutput").entered();
            self.prepare_output(&mut pass, &mut late, w, h)
        };
        // TAA: this frame's subpixel offset (Bevy's Halton (2, 3) − 0.5
        // sequence, in pixels) and a sharper LOD for the maps.
        let taa = hdr && self.hdr.as_ref().is_some_and(|o| o.taa());
        self.renderer.set_motion_vectors(&self.ctx, taa);
        if taa {
            const HALTON: [(f32, f32); 8] = [
                (0.0, 0.0),
                (0.0, -0.16666666),
                (-0.25, 0.16666669),
                (0.25, -0.3888889),
                (-0.375, -0.055555552),
                (0.125, 0.2777778),
                (-0.125, -0.2777778),
                (0.375, 0.055555582),
            ];
            let (jx, jy) = HALTON[self.taa_frame as usize % HALTON.len()];
            self.taa_frame = self.taa_frame.wrapping_add(1);
            let ndc = glam::Vec2::new(2.0 * jx / w.max(1) as f32, 2.0 * jy / h.max(1) as f32);
            self.renderer.set_jitter(ndc, -1.0);
        } else {
            self.renderer.set_jitter(glam::Vec2::ZERO, 0.0);
            self.taa_previous = None;
        }
        if let Some(p) = &mut self.profiler {
            p.update();
        }
        let drawn = if hdr {
            // Opaque first; then, if anything transmits, a copy of it for
            // them to see through; then transmissive and transparent meshes.
            self.renderer
                .set_scene_color(&self.ctx, self.hdr.as_ref().and_then(|o| o.scene_color()));
            let mut glass = std::mem::replace(&mut self.glass_pass, Pass::to_default());
            glass.clear_draws();
            let peeling = self.transmission_layers > 0;
            let drawn = self.render_passes(
                camera,
                &mut pass,
                Some(&mut late),
                peeling.then_some(&mut glass),
            );
            {
                let _s = tracing::info_span!("scene.opaque").entered();
                pass.sort();
                self.time(&mut pass, "opaque");
                self.ctx.render(&pass);
            }
            if !peeling && let Some(out) = &mut self.hdr {
                out.release_peel(&self.ctx);
            }
            // TAA: the opaque scene alone, for the reactive mask.
            if taa
                && (glass.draw_count() > 0 || late.draw_count() > 0)
                && let Some(out) = &mut self.hdr
            {
                let _s = tracing::info_span!("scene.copyOpaque").entered();
                out.keep_opaque_color(&self.ctx);
            }
            if glass.draw_count() > 0 {
                let _s = tracing::info_span!("scene.peelGlass").entered();
                self.peel_glass(&mut glass, &mut late);
            } else if self.stats.transmissive > 0
                && let Some(out) = &self.hdr
            {
                let _s = tracing::info_span!("scene.grab").entered();
                out.grab(&self.ctx);
            }
            self.glass_pass = glass;
            {
                let _s = tracing::info_span!("scene.late").entered();
                late.sort();
                self.time(&mut late, "late");
                self.ctx.render(&late);
            }
            if taa {
                let _s = tracing::info_span!("scene.taa").entered();
                self.resolve_taa(camera);
            }
            if let Some(out) = &mut self.hdr {
                let _s = tracing::info_span!("scene.present").entered();
                out.present(
                    &self.ctx,
                    self.renderer.exposure(),
                    self.tone_mapping,
                    self.dither,
                    self.sharpness,
                    self.profiler.as_mut(),
                );
            }
            drawn
        } else {
            self.renderer.set_scene_color(&self.ctx, None);
            let drawn = self.render(camera, &mut pass);
            let _s = tracing::info_span!("scene.opaque").entered();
            pass.sort();
            self.time(&mut pass, "opaque");
            self.ctx.render(&pass);
            drawn
        };
        self.frame_pass = pass;
        self.late_pass = late;
        drawn
    }

    /// `renderFrame` into a pass of yours (an offscreen target, MSAA, your
    /// own draws next to the scene's): appends the visible draws without
    /// clearing or sorting the pass, and leaves the camera's aspect to you
    /// (`setCameraAspect`). Returns the meshes added. Render the pass before
    /// the next `render` call: each call rewrites the scene's instance data
    /// (the matrices its batched draws read).
    pub fn render(&mut self, camera: Entity, pass: &mut Pass) -> usize {
        // Straight to displayable colors (renderFrame's HDR target set
        // linear output), and nothing transmissive sees a scene copy.
        self.renderer.set_output(true, self.tone_mapping);
        self.renderer.set_scene_color(&self.ctx, None);
        self.render_passes(camera, pass, None, None)
    }

    /// Reverse-Z on or off (`EXT_clip_control`): depth precision spread
    /// evenly over distance and an infinite far plane for perspective
    /// cameras — no z-fighting on distant surfaces. Render states and clear
    /// values keep their meaning; the scene's cameras switch projection
    /// themselves. Pays off fully with a float depth buffer (an offscreen
    /// `Depth32f` target; the canvas's is 24-bit). Returns whether reverse-Z
    /// is on (like `setHdr`): false where unavailable.
    #[wasm_bindgen(js_name = setReverseZ)]
    pub fn set_reverse_z(&mut self, on: bool) -> bool {
        self.ctx.set_reverse_z(on);
        self.ctx.reverse_z()
    }

    /// Resolves once every shader the scene's meshes use (and any variant
    /// switch under way) has finished compiling. Shaders compile in the
    /// background (`KHR_parallel_shader_compile`); until then frames leave
    /// those meshes out rather than stall. Await it after `addModel` to show
    /// a model whole; resolves at once where compiles are synchronous. Also
    /// waits for an environment bake under way (`setEnvironmentHdr`).
    #[wasm_bindgen(js_name = whenReady)]
    pub fn when_ready(&self) -> js_sys::Promise {
        let mut programs: Vec<ProgramId> = self
            .world
            .storage::<MeshRenderer>()
            .map(|m| m.data().iter().map(|m| m.program).collect())
            .unwrap_or_default();
        programs.extend(self.pending_variants.iter().map(|&(_, p)| p));
        programs.extend(self.renderer.built_background_program());
        programs.extend(self.renderer.motion_programs());
        programs.sort_unstable_by_key(|p| p.to_raw());
        programs.dedup();
        let ready = self.ctx.programs_ready(programs);
        let environment = self.env_pending.as_ref().map(|p| p.finished());
        wasm_bindgen_futures::future_to_promise(async move {
            ready.await;
            if let Some(environment) = environment {
                environment.await;
            }
            Ok(JsValue::UNDEFINED)
        })
    }

    /// Meshes the last frame left out because their shader was still
    /// compiling.
    pub fn pending(&self) -> usize {
        self.stats.pending
    }

    /// Meshes the last frame drew.
    pub fn drawn(&self) -> usize {
        self.stats.drawn
    }

    /// Draw calls the last frame made: fewer than `drawn` when identical
    /// meshes (same shader, geometry and material) drew as one instanced
    /// draw.
    /// Meshes the last frame drew motion vectors for (they moved; with TAA).
    #[wasm_bindgen(js_name = movingMeshes)]
    pub fn moving_meshes(&self) -> usize {
        self.stats.moving
    }

    #[wasm_bindgen(js_name = drawCalls)]
    pub fn draw_calls(&self) -> usize {
        self.stats.draws
    }

    /// Times `renderFrame`'s passes on the GPU (`gpuTimings`), or stops.
    /// Returns whether timing is on: false where the browser doesn't expose
    /// GPU timers (`EXT_disjoint_timer_query_webgl2`, often withheld).
    #[wasm_bindgen(js_name = setGpuProfiling)]
    pub fn set_gpu_profiling(&mut self, on: bool) -> bool {
        if let Some(mut p) = self.profiler.take() {
            p.destroy();
        }
        if on {
            let p = rswebgl::profiler::GpuProfiler::new(&self.ctx);
            if p.available() {
                self.profiler = Some(p);
            }
        }
        self.profiler.is_some()
    }

    /// GPU milliseconds per pass of recent frames, averaged: `opaque` (with
    /// the MSAA resolve), `late` (transmissive and transparent) and
    /// `present` (tone mapping) with HDR output; `opaque` alone without.
    /// Results come a frame or two late; empty until then, or without
    /// `setGpuProfiling(true)`.
    #[wasm_bindgen(js_name = gpuTimings)]
    pub fn gpu_timings(&self) -> js_sys::Object {
        let out = js_sys::Object::new();
        if let Some(p) = &self.profiler {
            for label in p.labels() {
                if let Some(ms) = p.milliseconds(&label) {
                    let _ = js_sys::Reflect::set(&out, &label.into(), &ms.into());
                }
            }
        }
        out
    }

    /// Transmissive layers seen through each other (depth peeling, with HDR
    /// output): 0 (the default) shows glass behind glass as the opaque scene
    /// behind it, cheaply; 2–4 peels that many layers, each refracting the
    /// ones behind it, for a geometry pass and a scene copy per layer. Deeper
    /// layers than that aren't drawn; peeled glass isn't multisampled.
    #[wasm_bindgen(js_name = setTransmissionLayers)]
    pub fn set_transmission_layers(&mut self, layers: u32) {
        self.transmission_layers = layers.min(4) as usize;
    }

    #[wasm_bindgen(js_name = transmissionLayers)]
    pub fn transmission_layers(&self) -> u32 {
        self.transmission_layers as u32
    }

    // Depth peeling (Everitt 2001), after the opaque pass: peel the
    // transmissive surfaces front to back into layer depths, shade the
    // layers back to front into the resolved scene — each refracting a copy
    // of everything behind it — and point `late` (transparent meshes) at
    // the result with the nearest layer's depth.
    fn peel_glass(&mut self, glass: &mut Pass, late: &mut Pass) {
        let layers = self.transmission_layers;
        let Some(out) = self.hdr.as_mut() else {
            return;
        };
        if let Err(e) = out.ensure_peel(&self.ctx, layers) {
            console::error_once(&format!("[rswebgl-scene] depth peeling: {e}"));
            return;
        }
        out.keep_opaque_depth(&self.ctx);
        glass.sort();
        glass.set_clear(&rswebgl::framebuffer::ClearMask::none());
        glass.clear_resolve();
        glass.set_invalidate(&rswebgl::framebuffer::InvalidateMask::none());
        // The depth peels aren't timed (only the shading passes are).
        glass.clear_timer();
        let out = self.hdr.as_ref().expect("checked");
        // Front to back: layer k is the nearest surface beyond layer k − 1.
        for k in 0..layers {
            let Some((fb, previous)) = out.peel_layer(&self.ctx, k) else {
                return;
            };
            self.renderer
                .set_peel(&self.ctx, if k == 0 { 0 } else { 1 }, previous);
            glass.set_target(fb);
            self.ctx.render(glass);
        }
        // Back to front, each over a copy of what is behind it.
        let Some((glass_fb, transparent_fb)) = out.peel_color_targets() else {
            return;
        };
        for k in (0..layers).rev() {
            out.grab(&self.ctx);
            self.renderer.set_peel(&self.ctx, 2, out.layer_depth(k));
            glass.set_target(glass_fb);
            if let Some(p) = &mut self.profiler {
                p.time(glass, "glass");
            }
            self.ctx.render(glass);
            glass.clear_timer();
        }
        self.renderer
            .set_peel(&self.ctx, 0, rswebgl::ids::TextureId::NULL);
        late.set_target(transparent_fb);
        late.clear_resolve();
        late.set_invalidate(&rswebgl::framebuffer::InvalidateMask::none());
    }

    // TAA after the scene's passes: reprojects the history through the
    // camera's motion since the last frame (starting over for a new camera
    // or a first frame).
    fn resolve_taa(&mut self, camera: Entity) {
        let now = self.renderer.last_view_proj();
        let Some(out) = self.hdr.as_mut() else {
            return;
        };
        // Meshes that moved: their own motion into the velocity target.
        let mut velocity = false;
        if let Some((mut pass, depth)) = out.motion_pass(&self.ctx)
            && self.renderer.push_motion(&self.ctx, &mut pass, depth) > 0
        {
            if let Some(p) = &mut self.profiler {
                p.time(&mut pass, "motion");
            }
            self.ctx.render(&pass);
            velocity = true;
        }
        let reproject = match self.taa_previous {
            Some((cam, previous)) if cam == camera => previous * now.inverse(),
            _ => {
                out.reset_taa();
                Mat4::IDENTITY
            }
        };
        out.taa_resolve(
            &self.ctx,
            &reproject,
            self.renderer.jitter(),
            self.renderer.exposure(),
            velocity,
            self.renderer.late_moved(),
        );
        self.taa_previous = Some((camera, now));
    }

    // Times `pass` under `label` when profiling.
    fn time(&mut self, pass: &mut Pass, label: &str) {
        match &mut self.profiler {
            Some(p) => p.time(pass, label),
            // Passes live across frames: drop a query an earlier frame set.
            None => pass.clear_timer(),
        }
    }

    /// Meshes the last frame left out by frustum culling.
    pub fn culled(&self) -> usize {
        self.stats.culled
    }

    /// Object matrices the last frame rewrote (moved or new meshes).
    #[wasm_bindgen(js_name = objectsWritten)]
    pub fn objects_written(&self) -> usize {
        self.stats.objects_written
    }

    // --- picking --------------------------------------------------------------

    /// What's under a screen point, as `camera` sees it: a promise of
    /// `{ entity, point, distance }` (`PickHit`), or null when nothing is.
    /// Found on the GPU (exact for any geometry; the answer comes a frame or
    /// so later), with the world as of now. `x`, `y` are normalized device
    /// coordinates, −1…1 with +Y up — from a pointer event:
    /// `x = e.offsetX / canvas.clientWidth * 2 - 1`,
    /// `y = 1 - e.offsetY / canvas.clientHeight * 2`. The entity is the mesh
    /// entity (`parent` leads to its glTF node), `point` the world position
    /// hit, `distance` its distance from the camera. Meshes are drawn with
    /// their location-0 attribute as the position.
    #[wasm_bindgen(unchecked_return_type = "Promise<PickHit | null>")]
    pub fn pick(&mut self, camera: Entity, x: f32, y: f32) -> Result<js_sys::Promise, Error> {
        if !self.sync_context() {
            return Err("pick: the WebGL context is lost".into());
        }
        self.transforms.update(&mut self.world);
        let fb = self.ctx.default_framebuffer();
        let found = self.renderer.pick(
            &self.world,
            &self.ctx,
            camera,
            Vec2::new(x, y),
            fb.width().max(0) as u32,
            fb.height().max(0) as u32,
        )?;
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let Some(hit) = found.await? else {
                return Ok(JsValue::NULL);
            };
            let out = js_sys::Object::new();
            let set = |k: &str, v: JsValue| js_sys::Reflect::set(&out, &k.into(), &v);
            set("entity", hit.entity.to_raw().into())?;
            set(
                "point",
                js_sys::Float32Array::from(&hit.point.to_array()[..]).into(),
            )?;
            set("distance", hit.distance.into())?;
            Ok(out.into())
        }))
    }

    /// Frees everything the scene owns: its renderer resources, the built-in
    /// shaders, the models added with `addModel`, the cloned materials and
    /// the textures `loadTexture` made.
    /// Handles you passed in (`setMesh`) stay yours.
    pub fn destroy(mut self) {
        if let Some(mut p) = self.profiler.take() {
            p.destroy();
        }
        if let Some(p) = &self.env_pending {
            p.cancel();
        }
        if let Some(baker) = &self.env_baker {
            baker.destroy();
        }
        for (g, record) in &self.materials {
            if record.cloned {
                self.ctx.destroy_param_group(*g);
            }
        }
        for model in self.models.into_values() {
            model.destroy(&self.ctx);
        }
        if let Some(out) = self.hdr {
            out.destroy(&self.ctx);
        }
        // GPU objects of a context lost since are gone already.
        let current = self.generation == self.ctx.generation();
        for t in self.textures {
            if current && self.ctx.texture_is_valid(t.id) {
                self.ctx.destroy_texture(t.id);
            }
        }
        for m in std::mem::take(&mut *self.reloads.borrow_mut()) {
            if let (_, Ok(model)) = m {
                model.destroy(&self.ctx);
            }
        }
        self.renderer.destroy(&self.ctx);
        if current {
            self.pbr.destroy(&self.ctx);
        }
    }
}

impl Scene {
    /// The world, for Rust code sharing the scene with JS.
    pub fn world(&self) -> &World {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    fn insert<T: 'static>(&mut self, e: Entity, op: &str, value: T) {
        if !self.world.is_alive(e) {
            dead(op);
            return;
        }
        self.world.insert(e, value);
    }

    // Edits `e`'s transform, adding an identity one first if it has none.
    fn edit_transform(&mut self, e: Entity, op: &str, f: impl FnOnce(&mut Transform)) {
        if !self.world.is_alive(e) {
            dead(op);
            return;
        }
        if !self.world.has::<Transform>(e) {
            self.world.insert(e, Transform::IDENTITY);
        }
        let mut t = self.world.get_mut::<Transform>(e).expect("just ensured");
        let mut next = *t;
        f(&mut next);
        t.set_if_neq(next);
    }
}

impl Scene {
    // A frame's draws: into `pass`, or with transmissive and transparent
    // meshes into `late`.
    fn render_passes(
        &mut self,
        camera: Entity,
        pass: &mut Pass,
        late: Option<&mut Pass>,
        glass: Option<&mut Pass>,
    ) -> usize {
        if !self.sync_context() {
            return 0; // lost: nothing to draw with
        }
        {
            let _s = tracing::info_span!("scene.apply").entered();
            self.apply_reloads();
            self.apply_environment();
            self.apply_pending_variants();
        }
        {
            let _s = tracing::info_span!("scene.transforms").entered();
            self.transforms.update(&mut self.world);
        }
        self.stats = self
            .renderer
            .render_split(&self.world, &self.ctx, camera, pass, late, glass);
        self.world.clear_trackers();
        self.stats.drawn
    }
}
