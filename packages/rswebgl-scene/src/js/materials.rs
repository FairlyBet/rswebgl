//! Materials for JS: find them in a model, read and change their properties
//! by glTF name, give one part a variant of its own (`cloneMaterial`). A
//! material is a number — its parameter group.
//!
//! ```js
//! const gold = scene.findMaterial(ring, "Gold");
//! scene.setMaterialProperty(gold, "baseColorFactor", [1, 0.78, 0.34, 1]);
//! scene.setMaterialProperty(gold, "roughnessFactor", 0.2);
//!
//! // One stone in ruby, the others unchanged:
//! const stone = scene.findByName(ring, "Stone_3");
//! const ruby = scene.cloneMaterial(scene.meshMaterial(stone));
//! scene.setMaterialProperty(ruby, "attenuationColor", [0.9, 0.05, 0.1]);
//! scene.setMaterial(stone, ruby);
//! ```

use rswebgl::console;
use rswebgl::error::Error;
use rswebgl::ids::ParamGroupId;
use rswebgl_ecs::Entity;
use rswebgl_gltf::{Material, Model, default_material};
use wasm_bindgen::prelude::*;

use super::{Scene, options};
use crate::hierarchy::descendants;
use crate::model::Primitive;
use crate::pbr::PbrKey;
use crate::render::MeshRenderer;

/// What the scene knows about one material: its data (mirrored in the
/// group's uniforms) and whether the scene made it (`cloneMaterial`) and so
/// destroys it.
pub(crate) struct MaterialRecord {
    pub(crate) material: Material,
    pub(crate) cloned: bool,
}

#[wasm_bindgen]
impl Scene {
    /// The distinct materials drawn at or under `root`, in mesh order.
    pub fn materials(&self, root: Entity) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for e in descendants(&self.world, root) {
            if let Some(m) = self.world.get::<MeshRenderer>(e) {
                let id = m.material.to_raw();
                if !m.material.is_null() && !out.contains(&id) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// The glTF name of a material, if it has one.
    #[wasm_bindgen(js_name = materialName)]
    pub fn material_name(&self, material: ParamGroupId) -> Option<String> {
        self.materials.get(&material)?.material.name()
    }

    /// The first material named `name` drawn at or under `root`, or `0`.
    #[wasm_bindgen(js_name = findMaterial)]
    pub fn find_material(&self, root: Entity, name: &str) -> ParamGroupId {
        self.materials(root)
            .into_iter()
            .map(ParamGroupId::from_raw)
            .find(|g| self.material_name(*g).as_deref() == Some(name))
            .unwrap_or(ParamGroupId::NULL)
    }

    /// The material `e` draws with — or, for an entity that doesn't draw (a
    /// glTF node), the first one under it. `0` if none.
    #[wasm_bindgen(js_name = meshMaterial)]
    pub fn mesh_material(&self, e: Entity) -> ParamGroupId {
        descendants(&self.world, e)
            .into_iter()
            .find_map(|d| self.world.get::<MeshRenderer>(d).map(|m| m.material))
            .unwrap_or(ParamGroupId::NULL)
    }

    /// Draws `e` and every mesh under it with `material` (a material of the
    /// scene's, or a group of your own for meshes with your own shader).
    #[wasm_bindgen(js_name = setMaterial)]
    pub fn set_material(&mut self, e: Entity, material: ParamGroupId) {
        if !self.world.is_alive(e) {
            super::dead("setMaterial");
            return;
        }
        for d in descendants(&self.world, e) {
            if let Some(mut m) = self.world.get_mut::<MeshRenderer>(d) {
                m.material = material;
            }
            self.update_variant(d);
        }
    }

    /// A material property by its glTF name (`baseColorFactor`,
    /// `metallicFactor`, `roughnessFactor`, `emissiveFactor`,
    /// `emissiveStrength`, `normalScale`, `occlusionStrength`, `alphaCutoff`,
    /// `ior`, `specularFactor`, `specularColorFactor`, `transmissionFactor`,
    /// `thicknessFactor`, `attenuationColor`, `attenuationDistance`,
    /// `clearcoatFactor`, `clearcoatRoughnessFactor`, `dispersion`), or
    /// undefined for an unknown material or name.
    #[wasm_bindgen(js_name = materialProperty)]
    pub fn material_property(&self, material: ParamGroupId, name: &str) -> Option<Vec<f32>> {
        self.materials.get(&material)?.material.property(name)
    }

    /// Sets a material property (names as for `materialProperty`): a number,
    /// or 3 / 4 numbers for colors. Every mesh drawn with the material sees
    /// it; turning transmission, volume, clearcoat or dispersion on or off
    /// switches their shader variant. Throws for an unknown material,
    /// property, or wrong number of values.
    #[wasm_bindgen(js_name = setMaterialProperty)]
    pub fn set_material_property(
        &mut self,
        material: ParamGroupId,
        name: &str,
        #[wasm_bindgen(unchecked_param_type = "number | ArrayLike<number>")] value: JsValue,
    ) -> Result<(), Error> {
        let values = options::numbers(&value)
            .ok_or_else(|| Error::from(format!("\"{name}\" takes a number or numbers")))?;
        self.set_material_values(material, name, &values)
    }

    /// A copy of `material` the scene owns: change it without touching the
    /// meshes that share the original. Freed with `destroyMaterial` or the
    /// scene. Its textures are the original model's: keep that model.
    #[wasm_bindgen(js_name = cloneMaterial)]
    pub fn clone_material(&mut self, material: ParamGroupId) -> Result<ParamGroupId, Error> {
        let data = self
            .materials
            .get(&material)
            .ok_or("cloneMaterial: not a material of this scene")?
            .material
            .clone();
        let copy = self.ctx.copy_param_group(material)?;
        self.materials.insert(
            copy,
            MaterialRecord {
                material: data,
                cloned: true,
            },
        );
        Ok(copy)
    }

    /// Frees a material made by `cloneMaterial`; meshes still drawn with it
    /// stop drawing (stale material), so `setMaterial` them first.
    #[wasm_bindgen(js_name = destroyMaterial)]
    pub fn destroy_material(&mut self, material: ParamGroupId) {
        match self.materials.get(&material) {
            Some(r) if r.cloned => {
                self.materials.remove(&material);
                self.ctx.destroy_param_group(material);
            }
            Some(_) => console::error_once(
                "[rswebgl-scene] destroyMaterial: a model's own material goes with the model; \
                 ignored",
            ),
            None => console::error_once(
                "[rswebgl-scene] destroyMaterial: not a material of this scene; ignored",
            ),
        }
    }

    /// Built-in PBR shader variants compiled so far.
    #[wasm_bindgen(js_name = shaderVariantCount)]
    pub fn shader_variant_count(&self) -> usize {
        self.pbr.len()
    }
}

impl Scene {
    /// `setMaterialProperty` from Rust.
    pub fn set_material_values(
        &mut self,
        material: ParamGroupId,
        name: &str,
        values: &[f32],
    ) -> Result<(), Error> {
        let record = self
            .materials
            .get_mut(&material)
            .ok_or("setMaterialProperty: not a material of this scene")?;
        record.material.set_property(name, values)?;
        record.material.write_property(&self.ctx, material, name);
        // A factor crossing zero can change the shader variant.
        let users: Vec<Entity> = self
            .world
            .storage::<MeshRenderer>()
            .map(|meshes| {
                meshes
                    .iter()
                    .filter(|(_, m)| m.material == material)
                    .map(|(e, _)| e)
                    .collect()
            })
            .unwrap_or_default();
        for e in users {
            self.update_variant(e);
        }
        Ok(())
    }

    pub(super) fn register_materials(&mut self, model: &Model) {
        let default = default_material();
        let data = model.materials().iter().chain(std::iter::once(&default));
        for (&group, material) in model.material_groups().iter().zip(data) {
            self.materials.insert(
                group,
                MaterialRecord {
                    material: material.clone(),
                    cloned: false,
                },
            );
        }
    }

    // A glTF mesh on the built-in PBR: its program follows its material's
    // variant.
    fn update_variant(&mut self, e: Entity) {
        let Some(mesh) = self.world.get::<MeshRenderer>(e).copied() else {
            return;
        };
        let (Some(prim), Some(record)) = (
            self.world.get::<Primitive>(e),
            self.materials.get(&mesh.material),
        ) else {
            return;
        };
        if !self.pbr.is_pbr(mesh.program) {
            return;
        }
        let key = PbrKey::new(prim.attributes, &record.material);
        match self.pbr.program(&self.ctx, key) {
            Ok(program) if program != mesh.program => {
                if let Some(mut m) = self.world.get_mut::<MeshRenderer>(e) {
                    m.program = program;
                }
            }
            Ok(_) => {}
            Err(err) => console::error_once(&format!(
                "[rswebgl-scene] a PBR shader variant failed to build: {err}"
            )),
        }
    }
}
