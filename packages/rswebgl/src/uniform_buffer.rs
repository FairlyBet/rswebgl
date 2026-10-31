use rswebgl_foundation::dirty::DirtyBits;
use rswebgl_foundation::layout::{FieldType, Layout, LayoutBuilder, LayoutRule, Word};
use rswebgl_foundation::pool::Destroyed;
use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlBuffer};

use crate::buffer::{BufferKind, BufferUsage, Init, create_raw, write_raw};
use crate::console;
use crate::context::{Context, stale};
use crate::ids::UniformBufferId;
use crate::limits;

// ---------------------------------------------------------------------------
// Std140Type — the primitive a UBO field can hold.
// ---------------------------------------------------------------------------

/// A primitive type usable as a uniform-block field. Matrices are column-major,
/// matching GLSL and glam's `to_cols_array`. (JS-facing mirror of the
/// foundation's `FieldType`, which carries the std140 layout rules.)
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Std140Type {
    F32,
    I32,
    U32,
    Bool,
    Vec2,
    Vec3,
    Vec4,
    IVec2,
    IVec3,
    IVec4,
    UVec2,
    UVec3,
    UVec4,
    Mat2,
    Mat3,
    Mat4,
    Mat2x3,
    Mat2x4,
    Mat3x2,
    Mat3x4,
    Mat4x2,
    Mat4x3,
}

impl From<Std140Type> for FieldType {
    fn from(t: Std140Type) -> FieldType {
        use Std140Type as S;
        match t {
            S::F32 => FieldType::F32,
            S::I32 => FieldType::I32,
            S::U32 => FieldType::U32,
            S::Bool => FieldType::Bool,
            S::Vec2 => FieldType::Vec2,
            S::Vec3 => FieldType::Vec3,
            S::Vec4 => FieldType::Vec4,
            S::IVec2 => FieldType::IVec2,
            S::IVec3 => FieldType::IVec3,
            S::IVec4 => FieldType::IVec4,
            S::UVec2 => FieldType::UVec2,
            S::UVec3 => FieldType::UVec3,
            S::UVec4 => FieldType::UVec4,
            S::Mat2 => FieldType::Mat2,
            S::Mat3 => FieldType::Mat3,
            S::Mat4 => FieldType::Mat4,
            S::Mat2x3 => FieldType::Mat2x3,
            S::Mat2x4 => FieldType::Mat2x4,
            S::Mat3x2 => FieldType::Mat3x2,
            S::Mat3x4 => FieldType::Mat3x4,
            S::Mat4x2 => FieldType::Mat4x2,
            S::Mat4x3 => FieldType::Mat4x3,
        }
    }
}

// ---------------------------------------------------------------------------
// UboLayout — declarative block description (authoring side).
// ---------------------------------------------------------------------------

/// Describes a uniform block's fields in declaration order. The order and types
/// must match the GLSL `uniform Block { … }` exactly. Build it once, then create
/// one or more `UniformBuffer`s from it. The same layout also serves as a nested
/// struct via `struct_field` / `struct_array`.
///
/// The builder methods consume and return `self`, so they chain:
/// ```ignore
/// let light = UboLayout::new()
///     .field("pos", Std140Type::Vec3)
///     .field("intensity", Std140Type::F32)
///     .field("color", Std140Type::Vec4);
/// let block = UboLayout::new()
///     .field("u_view", Std140Type::Mat4)
///     .struct_array("lights", &light, 4)
///     .field("u_count", Std140Type::I32);
/// ```
#[wasm_bindgen]
#[derive(Clone, Default)]
pub struct UboLayout {
    builder: LayoutBuilder,
}

#[wasm_bindgen]
impl UboLayout {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// A scalar / vector / matrix field.
    pub fn field(self, name: &str, ty: Std140Type) -> Self {
        Self {
            builder: self.builder.field(name, ty.into()),
        }
    }

    /// An array of `count` primitives (each element aligned to 16 per std140).
    /// Address elements with an indexed path, e.g. `set_vec4("offsets[3]", …)`.
    pub fn array(self, name: &str, ty: Std140Type, count: u32) -> Self {
        Self {
            builder: self.builder.array(name, ty.into(), count),
        }
    }

    /// A nested struct, laid out per `layout`. Address members with a dotted
    /// path, e.g. `set_vec3("light.pos", …)`.
    pub fn struct_field(self, name: &str, layout: &UboLayout) -> Self {
        Self {
            builder: self.builder.struct_field(name, &layout.builder),
        }
    }

    /// An array of `count` structs, each laid out per `layout`. Address members
    /// with a path like `set_vec4("lights[2].color", …)`.
    pub fn struct_array(self, name: &str, layout: &UboLayout, count: u32) -> Self {
        Self {
            builder: self.builder.struct_array(name, &layout.builder, count),
        }
    }
}

// A block can't exceed MAX_UNIFORM_BLOCK_SIZE or the GL bind would fail later;
// catch it at creation with a clear message instead.
fn validate_block_size(size: u32, max: u32) -> Result<(), String> {
    if size > max {
        Err(format!(
            "UBO block is {size} bytes, over MAX_UNIFORM_BLOCK_SIZE ({max})"
        ))
    } else {
        Ok(())
    }
}

// std140 aligns everything to at most 16 bytes, so 16-byte chunks never split
// a vec4 or a matrix column across two uploads.
const DIRTY_CHUNK: u32 = 16;

// ---------------------------------------------------------------------------
// UniformBuffer — CPU-staged, std140-packed, GPU-backed uniform block.
// ---------------------------------------------------------------------------

/// A uniform block in the context's pool: writes land in a CPU-side byte buffer
/// laid out per std140, and are flushed to its own GL buffer as one
/// `bufferSubData` per changed run when the block is bound in a pass.
#[derive(Debug)]
pub(crate) struct UniformBufferObj {
    pub(crate) raw: WebGlBuffer,
    data: Vec<u8>,
    layout: Layout,
    // Touched 16-byte chunks since the last flush; scattered writes upload as
    // separate runs instead of one span from the first to the last.
    dirty: DirtyBits,
}

impl UniformBufferObj {
    pub(crate) fn delete(self, gl: &WebGl2RenderingContext) {
        gl.delete_buffer(Some(&self.raw));
    }

    pub(crate) fn byte_size(&self) -> u32 {
        self.data.len() as u32
    }

    // Flush the dirty runs to the GL buffer (no-op if clean).
    pub(crate) fn flush(&mut self, gl: &WebGl2RenderingContext) {
        if !self.dirty.is_dirty() {
            return;
        }
        for run in self.dirty.take_runs() {
            write_raw(
                gl,
                BufferKind::Generic,
                &self.raw,
                run.start as i32,
                &self.data[run.start as usize..run.end as usize],
            );
        }
    }

    // Shared body for the setters: look up the field, type-check (`expect`;
    // None = any matrix), write the components into staging, mark the chunks.
    fn put<T: Word>(&mut self, path: &str, expect: Option<FieldType>, vals: &[T]) {
        let Some(id) = self.layout.field_id(path) else {
            console::warn_once(&format!("[rswebgl] UBO field not found: \"{path}\""));
            return;
        };
        let ty = self.layout.field(id).ty;
        let type_ok = match expect {
            Some(e) => e == ty,
            None => ty.is_matrix(),
        };
        if !type_ok {
            let wanted = expect.map_or("a matrix".to_string(), |e| format!("{e:?}"));
            console::warn_once(&format!(
                "[rswebgl] UBO field \"{path}\" is {ty:?}, not {wanted}"
            ));
            return;
        }
        match self.layout.write(&mut self.data, id, vals) {
            Ok(range) => self.dirty.mark(range),
            Err(e) => {
                console::warn_once(&format!("[rswebgl] UBO field \"{path}\" ({ty:?}): {e:?}"))
            }
        }
    }
}

impl Context {
    fn ubo_put<T: Word>(
        &self,
        ubo: UniformBufferId,
        op: &str,
        path: &str,
        expect: Option<FieldType>,
        vals: &[T],
    ) {
        match self.res().uniform_buffers.get_mut(ubo.0) {
            Some(u) => u.put(path, expect, vals),
            None => stale("uniform buffer", op, ubo),
        }
    }
}

#[wasm_bindgen]
impl Context {
    /// Allocate a uniform block backed by a GL buffer, laid out per std140 from
    /// `layout`. Fields are addressed by path (e.g. `"lights[2].color"`).
    pub fn create_uniform_buffer(
        &self,
        layout: &UboLayout,
        usage: BufferUsage,
    ) -> Result<UniformBufferId, String> {
        self.check_alive()?;
        let layout = layout.builder.build(LayoutRule::Std140);
        let size = layout.size();
        validate_block_size(size, limits::max_uniform_block_size())?;
        let buf = create_raw(self.gl(), BufferKind::Generic, usage, Init::Size(size))?;
        let obj = UniformBufferObj {
            raw: buf.raw,
            data: vec![0u8; size as usize],
            layout,
            dirty: DirtyBits::new(size, DIRTY_CHUNK),
        };
        let mut res = self.res();
        match res.uniform_buffers.insert(obj) {
            Ok(h) => Ok(h.into()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn destroy_uniform_buffer(&self, ubo: UniformBufferId) {
        let destroyed = self.res().uniform_buffers.destroy(ubo.0);
        match destroyed {
            Destroyed::Now(obj) => obj.delete(self.gl()),
            Destroyed::Deferred => {}
            Destroyed::Invalid => stale("uniform buffer", "destroy_uniform_buffer", ubo),
        }
    }

    /// Flush pending writes to the GL buffer. Normally unnecessary: the renderer
    /// does this for blocks bound in a pass. Use it to make the GPU copy current
    /// outside of rendering.
    pub fn ubo_upload(&self, ubo: UniformBufferId) {
        let gl = self.gl();
        match self.res().uniform_buffers.get_mut(ubo.0) {
            Some(u) => u.flush(gl),
            None => stale("uniform buffer", "ubo_upload", ubo),
        }
    }

    /// Total std140 size of the block in bytes (0 for a stale handle).
    pub fn ubo_byte_size(&self, ubo: UniformBufferId) -> u32 {
        self.res()
            .uniform_buffers
            .get(ubo.0)
            .map_or(0, |u| u.byte_size())
    }

    /// A copy of the CPU-side staging bytes (std140-packed). Reflects writes
    /// immediately, regardless of whether they've been uploaded.
    pub fn ubo_data(&self, ubo: UniformBufferId) -> Vec<u8> {
        self.res()
            .uniform_buffers
            .get(ubo.0)
            .map_or_else(Vec::new, |u| u.data.clone())
    }

    // --- scalar / vector setters --------------------------------------------

    pub fn ubo_set_f32(&self, ubo: UniformBufferId, path: &str, x: f32) {
        self.ubo_put(ubo, "ubo_set_f32", path, Some(FieldType::F32), &[x]);
    }
    pub fn ubo_set_vec2(&self, ubo: UniformBufferId, path: &str, x: f32, y: f32) {
        self.ubo_put(ubo, "ubo_set_vec2", path, Some(FieldType::Vec2), &[x, y]);
    }
    pub fn ubo_set_vec3(&self, ubo: UniformBufferId, path: &str, x: f32, y: f32, z: f32) {
        self.ubo_put(ubo, "ubo_set_vec3", path, Some(FieldType::Vec3), &[x, y, z]);
    }
    pub fn ubo_set_vec4(&self, ubo: UniformBufferId, path: &str, x: f32, y: f32, z: f32, w: f32) {
        self.ubo_put(
            ubo,
            "ubo_set_vec4",
            path,
            Some(FieldType::Vec4),
            &[x, y, z, w],
        );
    }

    pub fn ubo_set_i32(&self, ubo: UniformBufferId, path: &str, x: i32) {
        self.ubo_put(ubo, "ubo_set_i32", path, Some(FieldType::I32), &[x]);
    }
    pub fn ubo_set_ivec2(&self, ubo: UniformBufferId, path: &str, x: i32, y: i32) {
        self.ubo_put(ubo, "ubo_set_ivec2", path, Some(FieldType::IVec2), &[x, y]);
    }
    pub fn ubo_set_ivec3(&self, ubo: UniformBufferId, path: &str, x: i32, y: i32, z: i32) {
        self.ubo_put(
            ubo,
            "ubo_set_ivec3",
            path,
            Some(FieldType::IVec3),
            &[x, y, z],
        );
    }
    pub fn ubo_set_ivec4(&self, ubo: UniformBufferId, path: &str, x: i32, y: i32, z: i32, w: i32) {
        self.ubo_put(
            ubo,
            "ubo_set_ivec4",
            path,
            Some(FieldType::IVec4),
            &[x, y, z, w],
        );
    }

    pub fn ubo_set_u32(&self, ubo: UniformBufferId, path: &str, x: u32) {
        self.ubo_put(ubo, "ubo_set_u32", path, Some(FieldType::U32), &[x]);
    }
    pub fn ubo_set_uvec2(&self, ubo: UniformBufferId, path: &str, x: u32, y: u32) {
        self.ubo_put(ubo, "ubo_set_uvec2", path, Some(FieldType::UVec2), &[x, y]);
    }
    pub fn ubo_set_uvec3(&self, ubo: UniformBufferId, path: &str, x: u32, y: u32, z: u32) {
        self.ubo_put(
            ubo,
            "ubo_set_uvec3",
            path,
            Some(FieldType::UVec3),
            &[x, y, z],
        );
    }
    pub fn ubo_set_uvec4(&self, ubo: UniformBufferId, path: &str, x: u32, y: u32, z: u32, w: u32) {
        self.ubo_put(
            ubo,
            "ubo_set_uvec4",
            path,
            Some(FieldType::UVec4),
            &[x, y, z, w],
        );
    }

    pub fn ubo_set_bool(&self, ubo: UniformBufferId, path: &str, value: bool) {
        self.ubo_put(
            ubo,
            "ubo_set_bool",
            path,
            Some(FieldType::Bool),
            &[value as u32],
        );
    }

    /// Set a matrix field of any shape. `data` is column-major with exactly
    /// `columns * rows` floats for the field's declared type (e.g. 9 for `Mat3`,
    /// 16 for `Mat4`). std140 column padding is applied on write.
    pub fn ubo_set_mat(&self, ubo: UniformBufferId, path: &str, data: &[f32]) {
        self.ubo_put(ubo, "ubo_set_mat", path, None, data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_size_limit() {
        assert!(validate_block_size(16384, 16384).is_ok());
        assert!(validate_block_size(16400, 16384).is_err());
    }

    #[test]
    fn std140_type_maps_onto_field_type() {
        // Layout math lives in the foundation; spot-check the bridge.
        let l = UboLayout::new()
            .field("t", Std140Type::F32)
            .field("m", Std140Type::Mat3)
            .builder
            .build(LayoutRule::Std140);
        assert_eq!(l.field(l.field_id("m").unwrap()).offset, 16);
        assert_eq!(l.size(), 64);
    }
}
