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

mod lights;
mod materials;
mod options;

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
use crate::pick::{Hit, pick};
use crate::render::{MeshRenderer, RenderStats, SceneRenderer, Visible};
use crate::shader::create_scene_program;
use crate::transform::{Parent, Transform, WorldMatrix};

use materials::MaterialRecord;

/// A scene: entities with transforms, hierarchy, cameras, lights and meshes.
/// `renderFrame` draws it to the canvas; `render` into a pass of yours.
/// After a context loss its handles are stale: `destroy` it and build a new
/// one.
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
    // The canvas pass `renderFrame` reuses.
    frame_pass: Pass,
    stats: RenderStats,
    last_hit: Option<Hit>,
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
            frame_pass,
            stats: RenderStats::default(),
            last_hit: None,
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
            program: ProgramId::from_raw(options::u32_or(&mesh, "program", 0)?),
            state: StateId::from_raw(options::u32_or(&mesh, "state", 0)?),
            vao: VertexArrayId::from_raw(options::u32_or(&mesh, "vao", 0)?),
            material: ParamGroupId::from_raw(options::u32_or(&mesh, "material", 0)?),
            command: *draw,
            transparent: options::bool_or(&mesh, "transparent", false)?,
        };
        if mesh.program.is_null() || mesh.state.is_null() {
            return Err("setMesh: \"program\" and \"state\" are required".into());
        }
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
    pub fn create_program(&self, vert: &str, frag: &str) -> Result<ProgramId, Error> {
        create_scene_program(&self.ctx, vert, frag)
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
        self.models.insert(root, model);
        root
    }

    /// `addModel` with your own `program` for every primitive (a scene
    /// shader from `createProgram` sees the camera, lights, environment and
    /// object; the material group is the loader's).
    #[wasm_bindgen(js_name = addModelWithProgram)]
    pub fn add_model_with_program(&mut self, model: Model, program: ProgramId) -> Entity {
        let root = spawn_model(&mut self.world, &model, |_| Some(program));
        self.register_materials(&model);
        self.models.insert(root, model);
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
        let fb = self.ctx.default_framebuffer();
        let aspect = fb.width().max(1) as f32 / fb.height().max(1) as f32;
        if let Some(mut c) = self.world.get_mut::<crate::camera::Camera>(camera) {
            c.set_if_neq(crate::camera::Camera { aspect, ..*c });
        }
        let mut pass = std::mem::replace(&mut self.frame_pass, Pass::to_default());
        pass.clear_draws();
        let drawn = self.render(camera, &mut pass);
        pass.sort();
        self.ctx.render(&pass);
        self.frame_pass = pass;
        drawn
    }

    /// `renderFrame` into a pass of yours (an offscreen target, MSAA, your
    /// own draws next to the scene's): appends the visible draws without
    /// clearing or sorting the pass, and leaves the camera's aspect to you
    /// (`setCameraAspect`). Returns the draws added.
    pub fn render(&mut self, camera: Entity, pass: &mut Pass) -> usize {
        self.transforms.update(&mut self.world);
        self.stats = self.renderer.render(&self.world, &self.ctx, camera, pass);
        self.world.clear_trackers();
        self.stats.drawn
    }

    /// Draws added by the last frame.
    pub fn drawn(&self) -> usize {
        self.stats.drawn
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

    /// The nearest visible mesh entity under a screen point, as `camera`
    /// sees it (as of the last frame), or `0`. `x`, `y` are normalized
    /// device coordinates, −1…1 with +Y up — from a pointer event:
    /// `x = e.offsetX / canvas.clientWidth * 2 - 1`,
    /// `y = 1 - e.offsetY / canvas.clientHeight * 2`. glTF meshes are hit by
    /// their triangles, others by their bounds. The hit is a primitive
    /// entity: `parent` leads to its glTF node. `pickPoint` / `pickDistance`
    /// tell where.
    pub fn pick(&mut self, camera: Entity, x: f32, y: f32) -> Entity {
        self.last_hit = pick(&self.world, camera, Vec2::new(x, y));
        self.last_hit.map_or(Entity::NULL, |h| h.entity)
    }

    /// World point of the last `pick`'s hit (3 numbers), or empty.
    #[wasm_bindgen(js_name = pickPoint)]
    pub fn pick_point(&self) -> Vec<f32> {
        self.last_hit
            .map_or_else(Vec::new, |h| h.point.to_array().to_vec())
    }

    /// World distance from the camera's near plane to the last `pick`'s hit,
    /// or −1.
    #[wasm_bindgen(js_name = pickDistance)]
    pub fn pick_distance(&self) -> f32 {
        self.last_hit.map_or(-1.0, |h| h.distance)
    }

    /// Frees everything the scene owns: its renderer resources, the built-in
    /// shaders, the models added with `addModel` and the cloned materials.
    /// Handles you passed in (`setMesh`) stay yours.
    pub fn destroy(self) {
        for (g, record) in &self.materials {
            if record.cloned {
                self.ctx.destroy_param_group(*g);
            }
        }
        for model in self.models.into_values() {
            model.destroy(&self.ctx);
        }
        self.renderer.destroy(&self.ctx);
        self.pbr.destroy(&self.ctx);
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
