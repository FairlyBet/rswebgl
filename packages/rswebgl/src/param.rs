//! Parameter layouts and groups — how uniform values reach a draw.
//!
//! A **layout** names the parameters a group carries: plain uniforms (with
//! their type and array length), textures, and uniform blocks. Names are
//! matched against a program's uniforms once (see `ProgramObj::mapping`), so a
//! layout works with any program that declares uniforms of those names.
//!
//! A **group** holds values for a layout: plain values packed into a word array
//! in wasm memory, plus texture and uniform-buffer handles. Batches and draws
//! reference groups by slot, ordered by how often they change:
//!
//! | slot | typical content                   | changes          |
//! |------|-----------------------------------|------------------|
//! | 0    | camera, lights, time              | once per pass    |
//! | 1    | material: textures, colors        | between materials|
//! | 2    | object: model matrix              | every draw       |
//! | 3    | free                              |                  |
//!
//! Fields are addressed by a [`ParamField`] index resolved from the name once
//! (`param_field`); writes go by index, no strings per frame.

use rswebgl_foundation::layout::{FieldType, Scalar};
use rswebgl_foundation::pool::Destroyed;
use wasm_bindgen::prelude::*;

use crate::console;
use crate::context::{Context, Resources, stale};
use crate::ids::{ParamGroupId, ParamLayoutId, TextureId, UniformBufferId};
use crate::limits;
use crate::uniform_buffer::UniformType;

/// Number of group slots on a batch / draw.
pub const GROUP_SLOTS: usize = 4;

/// A field index within a layout, from `Context::param_field`. The top bits
/// say which kind of field it is.
pub type ParamField = u32;

const KIND_SHIFT: u32 = 30;
const KIND_PLAIN: u32 = 0;
const KIND_TEXTURE: u32 = 1;
const KIND_BLOCK: u32 = 2;
const INDEX_MASK: u32 = (1 << KIND_SHIFT) - 1;

fn encode(kind: u32, index: usize) -> ParamField {
    kind << KIND_SHIFT | index as u32
}

fn decode(field: ParamField) -> (u32, usize) {
    (field >> KIND_SHIFT, (field & INDEX_MASK) as usize)
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum FieldDef {
    Plain(FieldType, u32),
    Texture,
    Block,
}

/// Describes a parameter layout. Chain the methods, then pass it to
/// `Context::create_param_layout`.
#[wasm_bindgen]
#[derive(Clone, Default)]
pub struct ParamLayoutBuilder {
    fields: Vec<(String, FieldDef)>,
}

#[wasm_bindgen]
impl ParamLayoutBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// A plain uniform (scalar, vector or matrix).
    pub fn uniform(mut self, name: &str, ty: UniformType) -> Self {
        self.fields
            .push((name.into(), FieldDef::Plain(ty.into(), 1)));
        self
    }

    /// A uniform array of `count` elements (`uniform vec4 name[count]`).
    pub fn uniform_array(mut self, name: &str, ty: UniformType, count: u32) -> Self {
        self.fields
            .push((name.into(), FieldDef::Plain(ty.into(), count.max(1))));
        self
    }

    /// A sampler uniform.
    pub fn texture(mut self, name: &str) -> Self {
        self.fields.push((name.into(), FieldDef::Texture));
        self
    }

    /// A uniform block, by its GLSL block name.
    pub fn block(mut self, name: &str) -> Self {
        self.fields.push((name.into(), FieldDef::Block));
        self
    }
}

/// A plain field's place in a group's word array.
#[derive(Debug, Clone)]
pub(crate) struct PlainField {
    pub(crate) name: Box<str>,
    pub(crate) ty: FieldType,
    pub(crate) count: u32,
    pub(crate) offset: usize,
    pub(crate) words: usize,
}

/// A layout in the context's pool. Immutable once created.
#[derive(Debug)]
pub(crate) struct ParamLayoutObj {
    pub(crate) plain: Vec<PlainField>,
    pub(crate) textures: Vec<Box<str>>,
    pub(crate) blocks: Vec<Box<str>>,
    words: usize,
    // (name, field), sorted by name.
    index: Vec<(Box<str>, ParamField)>,
}

impl ParamLayoutObj {
    fn new(builder: &ParamLayoutBuilder) -> Result<Self, String> {
        let mut plain = Vec::new();
        let mut textures = Vec::new();
        let mut blocks: Vec<Box<str>> = Vec::new();
        let mut index = Vec::new();
        let mut words = 0usize;
        for (name, def) in &builder.fields {
            let field = match *def {
                FieldDef::Plain(ty, count) => {
                    let w = (ty.components() * count) as usize;
                    plain.push(PlainField {
                        name: name.as_str().into(),
                        ty,
                        count,
                        offset: words,
                        words: w,
                    });
                    words += w;
                    encode(KIND_PLAIN, plain.len() - 1)
                }
                FieldDef::Texture => {
                    textures.push(name.as_str().into());
                    encode(KIND_TEXTURE, textures.len() - 1)
                }
                FieldDef::Block => {
                    blocks.push(name.as_str().into());
                    encode(KIND_BLOCK, blocks.len() - 1)
                }
            };
            index.push((Box::<str>::from(name.as_str()), field));
        }
        index.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(w) = index.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(format!("param layout: field \"{}\" declared twice", w[0].0));
        }
        Ok(Self {
            plain,
            textures,
            blocks,
            words,
            index,
        })
    }

    fn field(&self, name: &str) -> Option<ParamField> {
        self.index
            .binary_search_by(|(k, _)| k.as_ref().cmp(name))
            .ok()
            .map(|i| self.index[i].1)
    }

    fn field_name(&self, field: ParamField) -> &str {
        let (kind, i) = decode(field);
        match kind {
            KIND_PLAIN => &self.plain[i].name,
            KIND_TEXTURE => &self.textures[i],
            _ => &self.blocks[i],
        }
    }
}

// ---------------------------------------------------------------------------
// Group
// ---------------------------------------------------------------------------

/// A group in the context's pool.
#[derive(Debug)]
pub(crate) struct ParamGroupObj {
    pub(crate) layout: ParamLayoutId,
    // Plain values, packed per the layout (4-byte words: f32/i32/u32 bits).
    pub(crate) words: Vec<u32>,
    pub(crate) textures: Vec<TextureId>,
    // NULL = no buffer set yet.
    pub(crate) blocks: Vec<(UniformBufferId, Option<(i32, i32)>)>,
}

impl ParamGroupObj {
    // A plain field's words (f32/i32/u32 bits), for upload.
    pub(crate) fn uints(&self, f: &PlainField) -> &[u32] {
        &self.words[f.offset..f.offset + f.words]
    }
}

// Releases a group's use of its layout (deleting a layout destroyed meanwhile
// is a no-op beyond dropping it: layouts hold no GL objects).
fn release_layout(res: &mut Resources, layout: ParamLayoutId) {
    let _ = res.param_layouts.release(layout.0);
}

impl Context {
    // Resolves `group` and its layout's field, checks the field kind, and runs
    // `f` on the group with the field index.
    fn with_group_field<T>(
        &self,
        group: ParamGroupId,
        field: ParamField,
        kind: u32,
        op: &str,
        f: impl FnOnce(&mut ParamGroupObj, &ParamLayoutObj, usize) -> T,
    ) -> Option<T> {
        let res = &mut *self.res();
        let Resources {
            param_groups,
            param_layouts,
            ..
        } = res;
        let Some(g) = param_groups.get_mut(group.0) else {
            stale("param group", op, group);
            return None;
        };
        let layout = param_layouts
            .get(g.layout.0)
            .expect("retained by the group");
        let (k, i) = decode(field);
        let count = match k {
            KIND_PLAIN => layout.plain.len(),
            KIND_TEXTURE => layout.textures.len(),
            _ => layout.blocks.len(),
        };
        if k != kind || i >= count {
            console::error_once(&format!(
                "[rswebgl] {op}: field {field:#x} isn't a {} field of this group's layout",
                match kind {
                    KIND_PLAIN => "uniform",
                    KIND_TEXTURE => "texture",
                    _ => "block",
                }
            ));
            return None;
        }
        Some(f(g, layout, i))
    }

    fn param_set_words(
        &self,
        group: ParamGroupId,
        field: ParamField,
        op: &str,
        accepts: &[Scalar],
        words: impl ExactSizeIterator<Item = u32>,
    ) {
        self.with_group_field(group, field, KIND_PLAIN, op, |g, layout, i| {
            let f = &layout.plain[i];
            if !accepts.contains(&f.ty.scalar()) {
                console::warn_once(&format!(
                    "[rswebgl] {op}: \"{}\" is {:?}; use the setter for its type",
                    f.name, f.ty
                ));
                return;
            }
            if words.len() > f.words {
                console::warn_once(&format!(
                    "[rswebgl] {op}: \"{}\" ({:?} x{}) takes {} values, got {}",
                    f.name,
                    f.ty,
                    f.count,
                    f.words,
                    words.len()
                ));
                return;
            }
            for (dst, w) in g.words[f.offset..].iter_mut().zip(words) {
                *dst = w;
            }
        });
    }
}

#[wasm_bindgen]
impl Context {
    pub fn create_param_layout(
        &self,
        builder: &ParamLayoutBuilder,
    ) -> Result<ParamLayoutId, String> {
        let obj = ParamLayoutObj::new(builder)?;
        let mut res = self.res();
        match res.param_layouts.insert(obj) {
            Ok(h) => Ok(h.into()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Destroys the layout. Groups still using it keep it alive until they're
    /// destroyed; no new groups can be created from it.
    pub fn destroy_param_layout(&self, layout: ParamLayoutId) {
        let destroyed = self.res().param_layouts.destroy(layout.0);
        if let Destroyed::Invalid = destroyed {
            stale("param layout", "destroy_param_layout", layout);
        }
    }

    /// The field named `name` in `layout`, for the `param_set_*` calls. Resolve
    /// once and keep the number.
    pub fn param_field(&self, layout: ParamLayoutId, name: &str) -> Result<ParamField, String> {
        let res = self.res();
        let Some(l) = res.param_layouts.get(layout.0) else {
            return Err(format!(
                "param layout {:?} is destroyed or invalid",
                layout.0
            ));
        };
        l.field(name)
            .ok_or_else(|| format!("param layout has no field \"{name}\""))
    }

    /// The name of a field (for diagnostics).
    pub fn param_field_name(&self, layout: ParamLayoutId, field: ParamField) -> String {
        self.res()
            .param_layouts
            .get(layout.0)
            .map(|l| l.field_name(field).to_string())
            .unwrap_or_default()
    }

    /// A group of values for `layout`, all zero / unset to start with.
    pub fn create_param_group(&self, layout: ParamLayoutId) -> Result<ParamGroupId, String> {
        let res = &mut *self.res();
        let Some(l) = res.param_layouts.get(layout.0) else {
            return Err(format!(
                "param layout {:?} is destroyed or invalid",
                layout.0
            ));
        };
        let obj = ParamGroupObj {
            layout,
            words: vec![0; l.words],
            textures: vec![TextureId::NULL; l.textures.len()],
            blocks: vec![(UniformBufferId::NULL, None); l.blocks.len()],
        };
        res.param_layouts.retain(layout.0);
        match res.param_groups.insert(obj) {
            Ok(h) => Ok(h.into()),
            Err(e) => {
                release_layout(res, layout);
                Err(e.to_string())
            }
        }
    }

    pub fn destroy_param_group(&self, group: ParamGroupId) {
        let res = &mut *self.res();
        match res.param_groups.destroy(group.0) {
            Destroyed::Now(g) => release_layout(res, g.layout),
            Destroyed::Deferred => {}
            Destroyed::Invalid => stale("param group", "destroy_param_group", group),
        }
    }

    /// Writes a float field (float scalars, vectors, matrices — column-major —
    /// and their arrays). Fewer values than the field holds write a prefix.
    pub fn param_set_floats(&self, group: ParamGroupId, field: ParamField, values: &[f32]) {
        self.param_set_words(
            group,
            field,
            "param_set_floats",
            &[Scalar::F32],
            values.iter().map(|v| v.to_bits()),
        );
    }

    /// Writes an int (or bool) field.
    pub fn param_set_ints(&self, group: ParamGroupId, field: ParamField, values: &[i32]) {
        self.param_set_words(
            group,
            field,
            "param_set_ints",
            &[Scalar::I32, Scalar::Bool],
            values.iter().map(|v| *v as u32),
        );
    }

    /// Writes an unsigned int (or bool) field.
    pub fn param_set_uints(&self, group: ParamGroupId, field: ParamField, values: &[u32]) {
        self.param_set_words(
            group,
            field,
            "param_set_uints",
            &[Scalar::U32, Scalar::Bool],
            values.iter().copied(),
        );
    }

    pub fn param_set_f32(&self, group: ParamGroupId, field: ParamField, x: f32) {
        self.param_set_floats(group, field, &[x]);
    }

    pub fn param_set_vec4(
        &self,
        group: ParamGroupId,
        field: ParamField,
        x: f32,
        y: f32,
        z: f32,
        w: f32,
    ) {
        self.param_set_floats(group, field, &[x, y, z, w]);
    }

    pub fn param_set_i32(&self, group: ParamGroupId, field: ParamField, x: i32) {
        self.param_set_ints(group, field, &[x]);
    }

    pub fn param_set_bool(&self, group: ParamGroupId, field: ParamField, x: bool) {
        self.param_set_ints(group, field, &[x as i32]);
    }

    /// Sets the texture a sampler field reads (NULL unsets it).
    pub fn param_set_texture(&self, group: ParamGroupId, field: ParamField, texture: TextureId) {
        self.with_group_field(
            group,
            field,
            KIND_TEXTURE,
            "param_set_texture",
            |g, _, i| {
                g.textures[i] = texture;
            },
        );
    }

    /// Sets the uniform buffer a block field reads (the whole buffer).
    pub fn param_set_block(&self, group: ParamGroupId, field: ParamField, ubo: UniformBufferId) {
        self.with_group_field(group, field, KIND_BLOCK, "param_set_block", |g, _, i| {
            g.blocks[i] = (ubo, None);
        });
    }

    /// Sets a sub-range of a uniform buffer for a block field
    /// (`bindBufferRange`). `offset` must be a multiple of
    /// `UNIFORM_BUFFER_OFFSET_ALIGNMENT`.
    pub fn param_set_block_range(
        &self,
        group: ParamGroupId,
        field: ParamField,
        ubo: UniformBufferId,
        offset: i32,
        size: i32,
    ) {
        let align = limits::uniform_buffer_offset_alignment() as i32;
        if align > 0 && offset % align != 0 {
            console::warn_once(&format!(
                "[rswebgl] param_set_block_range: offset {offset} is not a multiple of \
                 UNIFORM_BUFFER_OFFSET_ALIGNMENT ({align})"
            ));
        }
        self.with_group_field(
            group,
            field,
            KIND_BLOCK,
            "param_set_block_range",
            |g, _, i| {
                g.blocks[i] = (ubo, Some((offset, size)));
            },
        );
    }
}

impl Context {
    /// Convenience for setup code: resolves `name` and writes floats. Prefer
    /// resolving the field once with `param_field` for per-frame writes.
    pub fn param_set_floats_by_name(&self, group: ParamGroupId, name: &str, values: &[f32]) {
        if let Some(field) = self.field_of(group, name, "param_set_floats_by_name") {
            self.param_set_floats(group, field, values);
        }
    }

    /// Convenience for setup code: resolves `name` and sets a texture.
    pub fn param_set_texture_by_name(&self, group: ParamGroupId, name: &str, texture: TextureId) {
        if let Some(field) = self.field_of(group, name, "param_set_texture_by_name") {
            self.param_set_texture(group, field, texture);
        }
    }

    /// Convenience for setup code: resolves `name` and sets a block's buffer.
    pub fn param_set_block_by_name(&self, group: ParamGroupId, name: &str, ubo: UniformBufferId) {
        if let Some(field) = self.field_of(group, name, "param_set_block_by_name") {
            self.param_set_block(group, field, ubo);
        }
    }

    /// Convenience for setup code: resolves `name` and writes ints.
    pub fn param_set_ints_by_name(&self, group: ParamGroupId, name: &str, values: &[i32]) {
        if let Some(field) = self.field_of(group, name, "param_set_ints_by_name") {
            self.param_set_ints(group, field, values);
        }
    }

    fn field_of(&self, group: ParamGroupId, name: &str, op: &str) -> Option<ParamField> {
        let res = self.res();
        let Some(g) = res.param_groups.get(group.0) else {
            stale("param group", op, group);
            return None;
        };
        let l = res.param_layouts.get(g.layout.0)?;
        let f = l.field(name);
        if f.is_none() {
            console::warn_once(&format!(
                "[rswebgl] {op}: no field \"{name}\" in the layout"
            ));
        }
        f
    }
}
