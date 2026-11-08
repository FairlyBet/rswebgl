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
use rswebgl::ids::{ParamGroupId, SamplerId, TextureId};
use rswebgl::texture::{
    TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget, TextureWrap,
};
use rswebgl_ecs::Entity;
use rswebgl_gltf::{EXTERNAL_TEXTURE, MAP_NAMES, MAP_UNIFORMS, Material, Model, default_material};
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
    // The texture of each map (`MAP_NAMES` order), NULL for none.
    pub(crate) maps: [TextureId; 5],
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
        let maps = self.materials[&material].maps;
        let copy = self.ctx.copy_param_group(material)?;
        self.materials.insert(
            copy,
            MaterialRecord {
                material: data,
                cloned: true,
                maps,
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

    /// Loads an image (PNG, JPEG, WebP, … — whatever the browser decodes)
    /// into a texture the scene owns, for `setMaterialTexture`. Resolves with
    /// the texture once it's decoded; rejects if the image can't be loaded
    /// (the texture is freed then). See `TextureOptions` for color space,
    /// wrapping and mipmaps; throws on bad options. After a context loss the
    /// scene loads it again (a new handle, in the same materials).
    #[wasm_bindgen(js_name = loadTexture)]
    pub fn load_texture(
        &mut self,
        url: &str,
        #[wasm_bindgen(unchecked_param_type = "TextureOptions | undefined")] opts: JsValue,
    ) -> Result<js_sys::Promise, Error> {
        self.sync_context();
        let spec = TextureSpec::parse(url, &opts)?;
        let mut result = Ok(TextureId::NULL);
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            let (ctx, url) = (self.ctx.clone(), spec.url.clone());
            let on_load = move |tex: TextureId| -> js_sys::Function {
                Closure::once_into_js(move |w: f64, _h: f64| {
                    if w > 0.0 {
                        let _ = resolve.call1(&JsValue::NULL, &tex.to_raw().into());
                    } else {
                        if ctx.texture_is_valid(tex) {
                            ctx.destroy_texture(tex);
                        }
                        let err = js_sys::Error::new(&format!("loadTexture: can't load {url}"));
                        let _ = reject.call1(&JsValue::NULL, &err);
                    }
                })
                .unchecked_into()
            };
            result = spec.start(&self.ctx, on_load);
        });
        let tex = result?;
        self.textures.push(LoadedTexture { id: tex, spec });
        Ok(promise)
    }

    /// Frees a texture made by `loadTexture`. Materials still using it stop
    /// drawing (reported): set another texture, or none, first.
    #[wasm_bindgen(js_name = destroyTexture)]
    pub fn destroy_texture(&mut self, texture: TextureId) {
        match self.textures.iter().position(|t| t.id == texture) {
            Some(i) => {
                self.textures.swap_remove(i);
                if self.ctx.texture_is_valid(texture) {
                    self.ctx.destroy_texture(texture);
                }
            }
            None => console::error_once(
                "[rswebgl-scene] destroyTexture: not a texture loaded by this scene; ignored",
            ),
        }
    }

    /// Sets one of a material's maps (`baseColorTexture`,
    /// `metallicRoughnessTexture`, `normalTexture`, `occlusionTexture`,
    /// `emissiveTexture` — glTF names) to `texture` (any texture of this
    /// context: `loadTexture`'s, or your own), or removes it with `0`.
    /// Every mesh drawn with the material sees it; adding or removing a map
    /// switches their shader variant. Colors (baseColor, emissive) want an
    /// sRGB texture, the rest linear. Throws for an unknown material or map.
    #[wasm_bindgen(js_name = setMaterialTexture)]
    pub fn set_material_texture(
        &mut self,
        material: ParamGroupId,
        map: &str,
        texture: TextureId,
        #[wasm_bindgen(unchecked_param_type = "MaterialTextureOptions | undefined")] opts: JsValue,
    ) -> Result<(), Error> {
        let slot = MAP_NAMES
            .iter()
            .position(|&n| n == map)
            .ok_or_else(|| Error::from(format!("setMaterialTexture: no map \"{map}\"")))?;
        let uv = options::u32_or(&opts, "texCoord", 0)?;
        if uv > 1 {
            return Err("setMaterialTexture: texCoord must be 0 or 1".into());
        }
        let record = self
            .materials
            .get_mut(&material)
            .ok_or("setMaterialTexture: not a material of this scene")?;
        let index = if texture.is_null() {
            -1
        } else {
            EXTERNAL_TEXTURE
        };
        record.material.set_map(slot, index, uv);
        record.maps[slot] = texture;
        let uniform = MAP_UNIFORMS[slot];
        self.ctx
            .param_set_texture_by_name(material, uniform, texture);
        // The texture's own sampling parameters (`loadTexture`'s wrap / filter).
        self.ctx
            .param_set_sampler_by_name(material, uniform, SamplerId::NULL);
        if slot == 0 {
            self.ctx.param_set_ints_by_name(
                material,
                "u_has_base_color_tex",
                &[!texture.is_null() as i32],
            );
        }
        self.update_variants_of(material);
        Ok(())
    }

    /// The texture of one of a material's maps (names as for
    /// `setMaterialTexture`), or `0` for none.
    #[wasm_bindgen(js_name = materialTexture)]
    pub fn material_texture(&self, material: ParamGroupId, map: &str) -> TextureId {
        let slot = MAP_NAMES.iter().position(|&n| n == map);
        match (slot, self.materials.get(&material)) {
            (Some(slot), Some(r)) => r.maps[slot],
            _ => TextureId::NULL,
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
        self.update_variants_of(material);
        Ok(())
    }

    // Every mesh drawn with `material` follows its variant.
    fn update_variants_of(&mut self, material: ParamGroupId) {
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
    }

    pub(super) fn register_materials(&mut self, model: &Model) {
        let default = default_material();
        let data = model.materials().iter().chain(std::iter::once(&default));
        for (&group, material) in model.material_groups().iter().zip(data) {
            let maps = std::array::from_fn(|slot| {
                material
                    .map(slot)
                    .and_then(|(t, _)| usize::try_from(t).ok())
                    .map_or(TextureId::NULL, |t| model.texture(t))
            });
            self.materials.insert(
                group,
                MaterialRecord {
                    material: material.clone(),
                    cloned: false,
                    maps,
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
        let transmissive = record.material.transmission() > 0.0;
        if mesh.transmissive != transmissive
            && let Some(mut m) = self.world.get_mut::<MeshRenderer>(e)
        {
            m.transmissive = transmissive;
        }
        match self.pbr.program(&self.ctx, key) {
            // A new variant still linking: keep drawing with the current one
            // (if it is ready) and switch when the new one is.
            Ok(program)
                if program != mesh.program
                    && self.ctx.program_link_pending(program)
                    && !self.ctx.program_link_pending(mesh.program) =>
            {
                self.pending_variants.retain(|&(p, _)| p != e);
                self.pending_variants.push((e, program));
            }
            Ok(program) => {
                self.pending_variants.retain(|&(p, _)| p != e);
                if program != mesh.program
                    && let Some(mut m) = self.world.get_mut::<MeshRenderer>(e)
                {
                    m.program = program;
                }
            }
            Err(err) => console::error_once(&format!(
                "[rswebgl-scene] a PBR shader variant failed to build: {err}"
            )),
        }
    }

    // Switches meshes whose new variant has finished linking (or failed:
    // then the draw reports it, as any broken program).
    pub(super) fn apply_pending_variants(&mut self) {
        if self.pending_variants.is_empty() {
            return;
        }
        let ctx = &self.ctx;
        let world = &mut self.world;
        self.pending_variants.retain(|&(e, program)| {
            if ctx.program_link_pending(program) {
                return world.is_alive(e);
            }
            if let Some(mut m) = world.get_mut::<MeshRenderer>(e) {
                m.program = program;
            }
            false
        });
    }
}

/// How a `loadTexture` texture was made, to make it again after a context
/// loss.
pub(crate) struct TextureSpec {
    url: String,
    srgb: bool,
    wrap: TextureWrap,
    mipmaps: bool,
    flip_y: bool,
}

/// A texture `loadTexture` made: freed with the scene, reloaded after a loss.
impl TextureSpec {
    pub(crate) fn url(&self) -> &str {
        &self.url
    }
}

pub(crate) struct LoadedTexture {
    pub(crate) id: TextureId,
    pub(crate) spec: TextureSpec,
}

impl TextureSpec {
    fn parse(url: &str, opts: &JsValue) -> Result<Self, Error> {
        let srgb = match options::string_or(opts, "colorSpace", "srgb")?.as_str() {
            "srgb" => true,
            "linear" => false,
            other => {
                return Err(
                    format!("loadTexture: colorSpace \"{other}\" isn't srgb or linear").into(),
                );
            }
        };
        let wrap = match options::string_or(opts, "wrap", "repeat")?.as_str() {
            "repeat" => TextureWrap::Repeat,
            "clamp" => TextureWrap::ClampToEdge,
            "mirror" => TextureWrap::MirroredRepeat,
            other => {
                return Err(
                    format!("loadTexture: wrap \"{other}\" isn't repeat, clamp or mirror").into(),
                );
            }
        };
        Ok(Self {
            url: url.to_string(),
            srgb,
            wrap,
            mipmaps: options::bool_or(opts, "mipmaps", true)?,
            flip_y: options::bool_or(opts, "flipY", false)?,
        })
    }

    /// Creates the texture and starts loading into it; `on_load(w, h)` as
    /// for `Context::texture_load_image`.
    pub(crate) fn start(
        &self,
        ctx: &rswebgl::context::Context,
        on_load: impl FnOnce(TextureId) -> js_sys::Function,
    ) -> Result<TextureId, Error> {
        let min = if self.mipmaps {
            TextureMinFilter::LinearMipmapLinear
        } else {
            TextureMinFilter::Linear
        };
        let tex = ctx.create_texture(TextureTarget::Texture2D, min, TextureMagFilter::Linear)?;
        ctx.texture_set_wrap_s(tex, self.wrap);
        ctx.texture_set_wrap_t(tex, self.wrap);
        let format = if self.srgb {
            TextureFormat::srgb8_alpha8()
        } else {
            TextureFormat::rgba8()
        };
        ctx.texture_load_image(
            tex,
            &self.url,
            &format,
            self.mipmaps,
            self.flip_y,
            on_load(tex),
        );
        Ok(tex)
    }
}

#[wasm_bindgen]
impl Scene {
    /// The shading model of the built-in PBR: "openpbr" (OpenPBR Surface,
    /// the default) or "gltf" (the glTF reference, as Khronos' sample
    /// renderer shades it) — to compare them on the same materials. Meshes
    /// switch as their new shaders finish compiling. Throws on another name.
    #[wasm_bindgen(js_name = setShadingModel)]
    pub fn set_shading_model(&mut self, name: &str) -> Result<(), Error> {
        let model = match name {
            "openpbr" => crate::pbr::ShadingModel::OpenPbr,
            "gltf" => crate::pbr::ShadingModel::Gltf,
            other => {
                return Err(format!("setShadingModel: \"{other}\" isn't openpbr or gltf").into());
            }
        };
        if model == self.pbr.model() {
            return Ok(());
        }
        self.pbr.set_model(model);
        let meshes: Vec<Entity> = self
            .world
            .storage::<MeshRenderer>()
            .map(|m| m.iter().map(|(e, _)| e).collect())
            .unwrap_or_default();
        for e in meshes {
            self.update_variant(e);
        }
        Ok(())
    }

    #[wasm_bindgen(js_name = shadingModel)]
    pub fn shading_model(&self) -> String {
        match self.pbr.model() {
            crate::pbr::ShadingModel::OpenPbr => "openpbr",
            crate::pbr::ShadingModel::Gltf => "gltf",
        }
        .to_string()
    }
}
