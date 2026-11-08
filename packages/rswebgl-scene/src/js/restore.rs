//! Rebuilding the scene after a context loss.
//!
//! A loss kills every GPU object; parameter groups (materials) and the world
//! are CPU-side and survive. The scene notices the new context generation at
//! its next call (a frame, or anything that creates GPU objects) and rebuilds
//! what it made itself, keeping every handle the app holds that can be kept:
//!
//! - its renderer (frame and lighting blocks, default environment, BRDF
//!   table, background shader), with exposure / intensity / rotation;
//! - the environment, baked again from the `.hdr` bytes it was given;
//! - PBR variants and `createProgram` shaders, recompiled, meshes switched;
//! - `loadTexture` textures, loaded again; materials repointed;
//! - models, fetched again from their URL (`Model::adopt`): until a reload
//!   lands its meshes are left out of frames (`Reloading`); material handles
//!   and edits stay as they were.
//!
//! What the app made itself (programs from `ctx`, meshes from `setMesh`,
//! textures not from `loadTexture`) is the app's to rebuild, in
//! `ctx.setOnRestored`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rswebgl::console;
use rswebgl::error::Error;
use rswebgl::ids::{ProgramId, SamplerId, TextureId};
use rswebgl_ecs::Entity;
use rswebgl_gltf::{Model, load_model};
use wasm_bindgen::prelude::*;

use super::Scene;

use crate::hdr::decode_hdr_rgbe;
use crate::hierarchy::descendants;
use crate::model::{Morph, Primitive};
use crate::pbr::PbrShaders;
use crate::render::{MeshRenderer, Reloading};
use crate::shader::create_scene_program;

/// Models reloaded after a loss, waiting for the scene's next frame: the
/// model root and the fresh load.
pub(crate) type Reloads = Rc<RefCell<Vec<(Entity, Result<Model, Error>)>>>;

/// A scene shader's sources, to compile it again after a loss.
pub(crate) struct SceneProgram {
    pub(crate) id: ProgramId,
    pub(crate) vert: String,
    pub(crate) frag: String,
}

impl Scene {
    /// Rebuilds after a context loss if one happened since the scene last
    /// looked; false while the context is lost (nothing can be made then).
    pub(crate) fn sync_context(&mut self) -> bool {
        if self.ctx.is_lost() {
            return false;
        }
        if self.generation != self.ctx.generation() {
            self.restore();
        }
        true
    }

    fn restore(&mut self) {
        let ctx = self.ctx.clone();
        self.generation = ctx.generation();
        console::log("[rswebgl-scene] rebuilding the scene after a context loss");

        if let Err(e) = self.renderer.rebuild(&ctx) {
            console::error(&format!("[rswebgl-scene] restore: the renderer: {e}"));
        }
        // The bake shaders went with the old context; a bake in progress too.
        if let Some(p) = self.env_pending.take() {
            p.cancel();
        }
        self.env_baker = None;
        if let Some((bytes, size)) = self.env_source.clone() {
            let intensity = self.renderer.environment_intensity();
            let started = decode_hdr_rgbe(&bytes)
                .map_err(Error::from)
                .and_then(|img| self.start_environment(&img, size));
            match started {
                // Baked over the next frames; the intensity survives its swap.
                Ok(()) => self.env_intensity_after_bake = Some(intensity),
                Err(e) => console::error(&format!("[rswebgl-scene] restore: the environment: {e}")),
            }
        }
        self.restore_programs();
        self.restore_textures();
        self.reload_models();
    }

    // Recompiles PBR variants and scene shaders and switches meshes to them.
    fn restore_programs(&mut self) {
        let old_pbr = std::mem::replace(&mut self.pbr, PbrShaders::new());
        self.pbr.set_model(old_pbr.model());
        self.pending_variants.clear();
        let mut remap: HashMap<ProgramId, ProgramId> = HashMap::new();
        let in_use: Vec<ProgramId> = self
            .world
            .storage::<MeshRenderer>()
            .map(|m| m.data().iter().map(|m| m.program).collect())
            .unwrap_or_default();
        self.scene_programs.retain_mut(|p| {
            if !in_use.contains(&p.id) {
                return false; // gone with the old context and unused: forget it
            }
            match create_scene_program(&self.ctx, &p.vert, &p.frag) {
                Ok(new) => {
                    remap.insert(p.id, new);
                    p.id = new;
                    true
                }
                Err(e) => {
                    console::error(&format!("[rswebgl-scene] restore: a scene shader: {e}"));
                    false
                }
            }
        });
        let meshes: Vec<(Entity, ProgramId)> = self
            .world
            .storage::<MeshRenderer>()
            .map(|m| m.iter().map(|(e, m)| (e, m.program)).collect())
            .unwrap_or_default();
        for (e, program) in meshes {
            let new = if old_pbr.is_pbr(program) {
                self.pbr_program(e)
            } else {
                remap.get(&program).copied()
            };
            if let (Some(new), Some(mut m)) = (new, self.world.get_mut::<MeshRenderer>(e)) {
                m.program = new;
            }
        }
    }

    // The PBR variant for primitive entity `e` with its current material.
    fn pbr_program(&mut self, e: Entity) -> Option<ProgramId> {
        let mesh = self.world.get::<MeshRenderer>(e)?;
        let attributes = self.world.get::<Primitive>(e).map_or(0, |p| p.attributes);
        let material = self
            .materials
            .get(&mesh.material)
            .map(|r| r.material.clone());
        let material = material.unwrap_or_else(rswebgl_gltf::default_material);
        let key = crate::pbr::PbrKey::new(attributes, &material);
        self.pbr
            .program(&self.ctx, key)
            .map_err(|e| console::error(&format!("[rswebgl-scene] restore: a PBR variant: {e}")))
            .ok()
    }

    // Loads `loadTexture`'s textures again and points materials at them.
    fn restore_textures(&mut self) {
        let mut remap = HashMap::new();
        for t in &mut self.textures {
            let url = t.spec.url().to_string();
            let on_load = move |_| -> js_sys::Function {
                Closure::once_into_js(move |w: f64, _h: f64| {
                    if w == 0.0 {
                        console::error(&format!("[rswebgl-scene] restore: can't load {url}"));
                    }
                })
                .unchecked_into()
            };
            match t.spec.start(&self.ctx, on_load) {
                Ok(new) => {
                    remap.insert(t.id, new);
                    t.id = new;
                }
                Err(e) => console::error(&format!("[rswebgl-scene] restore: a texture: {e}")),
            }
        }
        self.remap_textures(&remap, &HashMap::new());
    }

    // Points every material (groups and the scene's records) at new textures.
    fn remap_textures(
        &mut self,
        textures: &HashMap<TextureId, TextureId>,
        samplers: &HashMap<SamplerId, SamplerId>,
    ) {
        self.ctx.remap_group_handles(textures, samplers);
        for record in self.materials.values_mut() {
            for t in &mut record.maps {
                if let Some(&new) = textures.get(t) {
                    *t = new;
                }
            }
        }
    }

    // Starts fetching every model again; its meshes wait meanwhile.
    fn reload_models(&mut self) {
        for (&root, model) in &self.models {
            if !model.is_lost(&self.ctx) {
                continue;
            }
            for e in descendants(&self.world, root) {
                if self.world.get::<MeshRenderer>(e).is_some() {
                    self.world.insert(e, Reloading);
                }
            }
            let (ctx, url, inbox) = (self.ctx.clone(), model.source(), self.reloads.clone());
            wasm_bindgen_futures::spawn_local(async move {
                let result = load_model(&ctx, &url).await;
                inbox.borrow_mut().push((root, result));
            });
        }
    }

    /// Puts reloaded models in place (called each frame).
    pub(crate) fn apply_reloads(&mut self) {
        if self.reloads.borrow().is_empty() {
            return;
        }
        let done = std::mem::take(&mut *self.reloads.borrow_mut());
        for (root, result) in done {
            let fresh = match result {
                Ok(fresh) => fresh,
                Err(e) => {
                    // A second loss mid-reload starts another reload; anything
                    // else leaves the model out, reported.
                    if !self.ctx.is_lost() && self.models.contains_key(&root) {
                        console::error(&format!("[rswebgl-scene] restore: a model: {e}"));
                    }
                    continue;
                }
            };
            let Some(model) = self.models.get_mut(&root) else {
                fresh.destroy(&self.ctx); // despawned meanwhile
                continue;
            };
            if fresh.is_lost(&self.ctx) || !model.is_lost(&self.ctx) {
                fresh.destroy(&self.ctx); // from a context lost since, or stale
                continue;
            }
            let old: Vec<TextureId> = (0..model.texture_count())
                .map(|i| model.texture(i))
                .collect();
            if let Err(e) = model.adopt(&self.ctx, fresh) {
                console::error(&format!("[rswebgl-scene] restore: {e}"));
                continue;
            }
            let textures: HashMap<TextureId, TextureId> = old
                .into_iter()
                .zip((0..model.texture_count()).map(|i| model.texture(i)))
                .collect();
            // `adopt` repointed the groups; the scene's records follow.
            for record in self.materials.values_mut() {
                for t in &mut record.maps {
                    if let Some(&new) = textures.get(t) {
                        *t = new;
                    }
                }
            }
            let items: Vec<_> = model
                .items()
                .iter()
                .map(|i| (i.vao(), i.state(), i.morph()))
                .collect();
            for e in descendants(&self.world, root) {
                let Some(item) = self.world.get::<Primitive>(e).map(|p| p.item as usize) else {
                    continue;
                };
                let Some(&(vao, state, morph)) = items.get(item) else {
                    continue;
                };
                if let Some(mut m) = self.world.get_mut::<MeshRenderer>(e) {
                    m.vao = vao;
                    m.state = state;
                }
                if let (Some((texture, _)), Some(mut m)) = (morph, self.world.get_mut::<Morph>(e)) {
                    m.texture = texture;
                }
                self.world.remove::<Reloading>(e);
            }
        }
    }
}
