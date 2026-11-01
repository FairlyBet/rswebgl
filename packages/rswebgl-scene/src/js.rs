//! The scene for JS: one `Scene` object owning the world, the transform system
//! and the scene renderer. Entities are plain numbers; components are set and
//! read through methods taking the entity — no JS object per entity, nothing
//! to `.free()` but the scene itself (`destroy`).
//!
//! ```js
//! const scene = new Scene(ctx);
//! const camera = scene.spawn();
//! scene.setPerspectiveCamera(camera, Math.PI / 4, 0.1, 100);
//! scene.lookAt(camera, 0, 2, 6, 0, 0, 0);
//! const ring = scene.addModel(await loadModel(ctx, url), program);
//!
//! function frame() {
//!   scene.setCameraAspect(camera, canvas.width / canvas.height);
//!   pass.clearDraws();
//!   scene.render(camera, pass);   // transforms → culling → draws
//!   pass.sort();
//!   ctx.render(pass);
//! }
//! ```

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};
use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::draw::DrawCommand;
use rswebgl::error::Error;
use rswebgl::ids::{ParamGroupId, ProgramId, StateId, VertexArrayId};
use rswebgl::pass::Pass;
use rswebgl_ecs::{Entity, World};
use rswebgl_gltf::Model;
use wasm_bindgen::prelude::*;

use crate::camera::Camera;
use crate::culling::Bounds;
use crate::hierarchy::{TransformSystem, despawn_recursive, set_parent};
use crate::model::{Name, find_by_name, spawn_model};
use crate::render::{MeshRenderer, RenderStats, SceneRenderer, Visible};
use crate::transform::{Transform, WorldMatrix};

/// A scene: entities with transforms, hierarchy, cameras and meshes, drawn
/// into a `Pass` by `render`. After a context loss its handles are stale:
/// `destroy` it and build a new one.
#[wasm_bindgen]
pub struct Scene {
    ctx: Context,
    world: World,
    transforms: TransformSystem,
    renderer: SceneRenderer,
    // Models added with `addModel`, by their root entity: their GPU
    // resources go when the root is despawned.
    models: HashMap<Entity, Model>,
    stats: RenderStats,
}

// A rejected call on a dead entity: reported once per call site, no-op.
fn dead(op: &str) {
    console::error_once(&format!(
        "[rswebgl-scene] {op}: the entity is not alive; ignored"
    ));
}

#[wasm_bindgen]
impl Scene {
    #[wasm_bindgen(constructor)]
    pub fn new(ctx: &Context) -> Result<Scene, Error> {
        Ok(Self {
            ctx: ctx.clone(),
            world: World::new(),
            transforms: TransformSystem::new(),
            renderer: SceneRenderer::new(ctx)?,
            models: HashMap::new(),
            stats: RenderStats::default(),
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
    /// despawned by its root, frees its GPU resources too). Returns how many
    /// entities went.
    pub fn despawn(&mut self, e: Entity) -> usize {
        if let Some(model) = self.models.remove(&e) {
            model.destroy(&self.ctx);
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
    pub fn set_local_matrix(&mut self, e: Entity, m: &[f32]) {
        let Ok(cols) = <[f32; 16]>::try_from(m) else {
            console::error_once("[rswebgl-scene] setLocalMatrix needs 16 floats; ignored");
            return;
        };
        let t = Transform::from_matrix(Mat4::from_cols_array(&cols));
        self.edit_transform(e, "setLocalMatrix", |old| *old = t);
    }

    /// Places `e` at the eye looking at the target, +Y up — for cameras (and
    /// lights). In its parent's space if it has one.
    #[wasm_bindgen(js_name = lookAt)]
    #[allow(clippy::too_many_arguments)]
    pub fn look_at(&mut self, e: Entity, ex: f32, ey: f32, ez: f32, tx: f32, ty: f32, tz: f32) {
        let eye = Vec3::new(ex, ey, ez);
        let m = Mat4::look_at_rh(eye, Vec3::new(tx, ty, tz), Vec3::Y).inverse();
        let t = Transform::from_matrix(m);
        self.edit_transform(e, "lookAt", |old| *old = t);
    }

    /// Local translation (3 floats), or empty if `e` has no transform.
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

    /// Local scale (3 floats), or empty.
    pub fn scale(&self, e: Entity) -> Vec<f32> {
        self.world
            .get::<Transform>(e)
            .map_or_else(Vec::new, |t| t.scale.to_array().to_vec())
    }

    /// World matrix (16 floats, column-major) as of the last `render` /
    /// `updateTransforms`, or empty if it hasn't got one yet.
    #[wasm_bindgen(js_name = worldMatrix)]
    pub fn world_matrix(&self, e: Entity) -> Vec<f32> {
        self.world
            .get::<WorldMatrix>(e)
            .map_or_else(Vec::new, |m| m.0.to_cols_array().to_vec())
    }

    /// Brings world matrices up to date now (`render` does it too) — for
    /// reading `worldMatrix` right after moving things.
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

    /// The parent of `e`, or `0` (null) for a root.
    pub fn parent(&self, e: Entity) -> Entity {
        self.world
            .get::<crate::transform::Parent>(e)
            .map_or(Entity::NULL, |p| p.0)
    }

    #[wasm_bindgen(js_name = setName)]
    pub fn set_name(&mut self, e: Entity, name: String) {
        if self.world.insert(e, Name(name)).is_none() && !self.world.is_alive(e) {
            dead("setName");
        }
    }

    pub fn name(&self, e: Entity) -> Option<String> {
        self.world.get::<Name>(e).map(|n| n.0.clone())
    }

    /// The first entity named `name` at or under `root`, or `0` (null).
    /// Scans every name: look parts up once and keep the number.
    #[wasm_bindgen(js_name = findByName)]
    pub fn find_by_name(&self, root: Entity, name: &str) -> Entity {
        find_by_name(&self.world, root, name).unwrap_or(Entity::NULL)
    }

    // --- cameras --------------------------------------------------------------

    /// Makes `e` a perspective camera (`fovY` in radians). It looks down its
    /// local −Z; place it with the transform setters or `lookAt`.
    #[wasm_bindgen(js_name = setPerspectiveCamera)]
    pub fn set_perspective_camera(&mut self, e: Entity, fov_y: f32, near: f32, far: f32) {
        let aspect = self.world.get::<Camera>(e).map_or(1.0, |c| c.aspect);
        self.insert(
            e,
            "setPerspectiveCamera",
            Camera::perspective(fov_y, aspect, near, far),
        );
    }

    #[wasm_bindgen(js_name = setOrthographicCamera)]
    pub fn set_orthographic_camera(&mut self, e: Entity, half_height: f32, near: f32, far: f32) {
        let aspect = self.world.get::<Camera>(e).map_or(1.0, |c| c.aspect);
        self.insert(
            e,
            "setOrthographicCamera",
            Camera::orthographic(half_height, aspect, near, far),
        );
    }

    /// Width / height of the target; keep it in sync on resize.
    #[wasm_bindgen(js_name = setCameraAspect)]
    pub fn set_camera_aspect(&mut self, e: Entity, aspect: f32) {
        match self.world.get_mut::<Camera>(e) {
            Some(mut c) => c.aspect = aspect,
            None => console::error_once(
                "[rswebgl-scene] setCameraAspect: the entity is not a camera; ignored",
            ),
        }
    }

    // --- drawing --------------------------------------------------------------

    /// Makes `e` draw: `material` goes in group slot 1 (0 = none). The handles
    /// stay the caller's to destroy.
    #[wasm_bindgen(js_name = setMesh)]
    pub fn set_mesh(
        &mut self,
        e: Entity,
        program: ProgramId,
        state: StateId,
        vao: VertexArrayId,
        material: ParamGroupId,
        command: &DrawCommand,
    ) {
        let mesh = MeshRenderer {
            program,
            state,
            vao,
            material,
            command: *command,
        };
        self.insert(e, "setMesh", mesh);
    }

    /// Changes only the material of a drawing entity — e.g. a gem variant.
    #[wasm_bindgen(js_name = setMaterial)]
    pub fn set_material(&mut self, e: Entity, material: ParamGroupId) {
        match self.world.get_mut::<MeshRenderer>(e) {
            Some(mut m) => m.material = material,
            None => {
                console::error_once("[rswebgl-scene] setMaterial: the entity has no mesh; ignored")
            }
        }
    }

    /// The material group of a drawing entity, or `0`.
    pub fn material(&self, e: Entity) -> ParamGroupId {
        self.world
            .get::<MeshRenderer>(e)
            .map_or(ParamGroupId::NULL, |m| m.material)
    }

    #[wasm_bindgen(js_name = removeMesh)]
    pub fn remove_mesh(&mut self, e: Entity) {
        self.world.remove::<MeshRenderer>(e);
    }

    /// Local bounding box, in the space of `e`'s transform: enables frustum
    /// culling for it.
    #[wasm_bindgen(js_name = setBounds)]
    #[allow(clippy::too_many_arguments)]
    pub fn set_bounds(
        &mut self,
        e: Entity,
        min_x: f32,
        min_y: f32,
        min_z: f32,
        max_x: f32,
        max_y: f32,
        max_z: f32,
    ) {
        let b = Bounds::new(
            Vec3::new(min_x, min_y, min_z),
            Vec3::new(max_x, max_y, max_z),
        );
        self.insert(e, "setBounds", b);
    }

    /// Hides or shows `e` and everything under it.
    #[wasm_bindgen(js_name = setVisible)]
    pub fn set_visible(&mut self, e: Entity, visible: bool) {
        self.insert(e, "setVisible", Visible(visible));
    }

    /// Spawns a loaded model under a new root entity and returns the root;
    /// every primitive draws with `program`. The scene takes the model over:
    /// its GPU resources are freed when the root is despawned (or the scene
    /// destroyed). Parts keep their glTF node names (`findByName`).
    #[wasm_bindgen(js_name = addModel)]
    pub fn add_model(&mut self, model: Model, program: ProgramId) -> Entity {
        let root = spawn_model(&mut self.world, &model, program);
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

    /// One frame: updates world matrices, culls against `camera` and appends
    /// the visible draws to `pass` (doesn't clear or sort it). Returns how
    /// many draws it added.
    pub fn render(&mut self, camera: Entity, pass: &mut Pass) -> usize {
        self.transforms.update(&mut self.world);
        self.stats = self.renderer.render(&self.world, &self.ctx, camera, pass);
        self.world.clear_trackers();
        self.stats.drawn
    }

    /// Draws added by the last `render`.
    pub fn drawn(&self) -> usize {
        self.stats.drawn
    }

    /// Meshes the last `render` left out by frustum culling.
    pub fn culled(&self) -> usize {
        self.stats.culled
    }

    /// Object matrices the last `render` rewrote (moved or new meshes).
    #[wasm_bindgen(js_name = objectsWritten)]
    pub fn objects_written(&self) -> usize {
        self.stats.objects_written
    }

    /// Frees everything the scene owns: its renderer resources and the models
    /// added with `addModel`. Handles you passed in (`setMesh`) stay yours.
    pub fn destroy(self) {
        for model in self.models.into_values() {
            model.destroy(&self.ctx);
        }
        self.renderer.destroy(&self.ctx);
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
