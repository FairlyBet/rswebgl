use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlBuffer};

use crate::buffer::{Buffer, BufferKind, BufferUsage};
use crate::console;
use crate::limits;

// ---------------------------------------------------------------------------
// Std140Type — the primitive a UBO field can hold, with its std140 footprint.
// ---------------------------------------------------------------------------

/// A primitive type usable as a uniform-block field. Drives the std140 layout
/// (alignment, size, and — for matrices — column padding). Matrices are
/// column-major, matching GLSL and glam's `to_cols_array`.
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

// std140 facts for a type: base alignment, the bytes it occupies in the block,
// and — for serialization — its column/component shape. For vectors/scalars
// `columns == 1` and the components are written contiguously. For matrices each
// of `columns` columns holds `comps` components but is padded to a 16-byte stride.
#[derive(Clone, Copy)]
struct Info {
    align: u32,
    size: u32,
    columns: u32,
    comps: u32,
}

impl Std140Type {
    fn info(self) -> Info {
        use Std140Type::*;
        let i = |align, size, columns, comps| Info {
            align,
            size,
            columns,
            comps,
        };
        match self {
            F32 | I32 | U32 | Bool => i(4, 4, 1, 1),
            Vec2 | IVec2 | UVec2 => i(8, 8, 1, 2),
            Vec3 | IVec3 | UVec3 => i(16, 12, 1, 3),
            Vec4 | IVec4 | UVec4 => i(16, 16, 1, 4),
            // Matrices: array of `columns` column-vectors, each padded to 16
            // (so size = columns * 16, independent of the row count `comps`).
            Mat2 => i(16, 32, 2, 2),
            Mat3 => i(16, 48, 3, 3),
            Mat4 => i(16, 64, 4, 4),
            Mat2x3 => i(16, 32, 2, 3),
            Mat2x4 => i(16, 32, 2, 4),
            Mat3x2 => i(16, 48, 3, 2),
            Mat3x4 => i(16, 48, 3, 4),
            Mat4x2 => i(16, 64, 4, 2),
            Mat4x3 => i(16, 64, 4, 3),
        }
    }

    fn is_matrix(self) -> bool {
        self.info().columns > 1
    }
}

// ---------------------------------------------------------------------------
// UboLayout — declarative block description (authoring side).
// ---------------------------------------------------------------------------

// Internal field shape. Not exposed across wasm_bindgen (data-carrying enum);
// the public surface is the builder methods. Nested structs are captured by
// value when added, so a layout can be reused to build several blocks.
#[derive(Clone)]
enum FieldKind {
    Scalar(Std140Type),
    Array(Std140Type, u32),
    Struct(Vec<FieldDef>),
    StructArray(Vec<FieldDef>, u32),
}

#[derive(Clone)]
struct FieldDef {
    name: String,
    kind: FieldKind,
}

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
#[derive(Clone)]
pub struct UboLayout {
    fields: Vec<FieldDef>,
}

impl Default for UboLayout {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl UboLayout {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self { fields: Vec::new() }
    }

    /// A scalar / vector / matrix field.
    pub fn field(mut self, name: &str, ty: Std140Type) -> Self {
        self.fields.push(FieldDef {
            name: name.into(),
            kind: FieldKind::Scalar(ty),
        });
        self
    }

    /// An array of `count` primitives (each element aligned to 16 per std140).
    /// Address elements with an indexed path, e.g. `set_vec4("offsets[3]", …)`.
    pub fn array(mut self, name: &str, ty: Std140Type, count: u32) -> Self {
        self.fields.push(FieldDef {
            name: name.into(),
            kind: FieldKind::Array(ty, count),
        });
        self
    }

    /// A nested struct, laid out per `layout`. Address members with a dotted
    /// path, e.g. `set_vec3("light.pos", …)`.
    pub fn struct_field(mut self, name: &str, layout: &UboLayout) -> Self {
        self.fields.push(FieldDef {
            name: name.into(),
            kind: FieldKind::Struct(layout.fields.clone()),
        });
        self
    }

    /// An array of `count` structs, each laid out per `layout`. Address members
    /// with a path like `set_vec4("lights[2].color", …)`.
    pub fn struct_array(mut self, name: &str, layout: &UboLayout, count: u32) -> Self {
        self.fields.push(FieldDef {
            name: name.into(),
            kind: FieldKind::StructArray(layout.fields.clone(), count),
        });
        self
    }
}

// ---------------------------------------------------------------------------
// std140 offset computation
// ---------------------------------------------------------------------------

fn align_up(value: u32, align: u32) -> u32 {
    value.div_ceil(align) * align
}

fn round16(value: u32) -> u32 {
    align_up(value, 16)
}

// Base alignment a field contributes to its enclosing struct's alignment.
fn member_align(kind: &FieldKind) -> u32 {
    match kind {
        FieldKind::Scalar(ty) => ty.info().align,
        // Arrays and structs are always 16-aligned in std140.
        FieldKind::Array(_, _) => 16,
        FieldKind::Struct(f) | FieldKind::StructArray(f, _) => measure(f).0,
    }
}

// Struct (align, size): alignment is the largest member alignment rounded up to
// 16; size is the laid-out end rounded up to that alignment.
fn measure(fields: &[FieldDef]) -> (u32, u32) {
    let align = round16(
        fields
            .iter()
            .map(|f| member_align(&f.kind))
            .max()
            .unwrap_or(16),
    );
    let mut sink = Vec::new();
    let end = emit(fields, 0, "", &mut sink);
    (align, align_up(end, align))
}

// Walks `fields` from `base`, recording each leaf's (path, offset, type) into
// `out`, and returns the offset just past the last field.
fn emit(
    fields: &[FieldDef],
    base: u32,
    prefix: &str,
    out: &mut Vec<(String, (u32, Std140Type))>,
) -> u32 {
    let mut off = base;
    for f in fields {
        let name = if prefix.is_empty() {
            f.name.clone()
        } else {
            format!("{prefix}.{}", f.name)
        };
        match &f.kind {
            FieldKind::Scalar(ty) => {
                let info = ty.info();
                off = align_up(off, info.align);
                out.push((name, (off, *ty)));
                off += info.size;
            }
            FieldKind::Array(ty, count) => {
                let stride = round16(ty.info().size);
                off = align_up(off, 16);
                for i in 0..*count {
                    out.push((format!("{name}[{i}]"), (off + i * stride, *ty)));
                }
                off += stride * count;
            }
            FieldKind::Struct(sf) => {
                let (salign, ssize) = measure(sf);
                off = align_up(off, salign);
                emit(sf, off, &name, out);
                off += ssize;
            }
            FieldKind::StructArray(sf, count) => {
                let (salign, ssize) = measure(sf);
                off = align_up(off, salign);
                for i in 0..*count {
                    emit(sf, off + i * ssize, &format!("{name}[{i}]"), out);
                }
                off += ssize * count;
            }
        }
    }
    off
}

fn compute_layout(fields: &[FieldDef]) -> (u32, HashMap<String, (u32, Std140Type)>) {
    let mut out = Vec::new();
    let end = emit(fields, 0, "", &mut out);
    (round16(end), out.into_iter().collect())
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

// ---------------------------------------------------------------------------
// UniformBuffer — CPU-staged, std140-packed, GPU-backed uniform block.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct UboState {
    data: Vec<u8>,
    fields: HashMap<String, (u32, Std140Type)>,
    // Smallest contiguous [start, end) byte range touched since the last flush.
    dirty: Option<(u32, u32)>,
}

/// A uniform block: writes land in a CPU-side byte buffer laid out per std140,
/// and are flushed to the GL buffer as one `bufferSubData` of the changed range.
///
/// You normally don't flush manually — the renderer uploads a dirty block when
/// it's bound in a pass. Call `upload()` only to make the GPU copy current
/// outside of rendering (e.g. before copying or reading the buffer).
///
/// Cloning yields another handle to the same block (shared CPU staging + GL
/// buffer), like the other resource types.
#[wasm_bindgen]
#[derive(Debug)]
pub struct UniformBuffer {
    buffer: Buffer,
    state: Rc<RefCell<UboState>>,
}

impl Clone for UniformBuffer {
    fn clone(&self) -> Self {
        Self {
            buffer: self.buffer.clone(),
            state: Rc::clone(&self.state),
        }
    }
}

// Two handles are equal iff they share the same staging/GL buffer — lets the
// renderer's uniform diffing skip re-binding an unchanged block.
impl PartialEq for UniformBuffer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.state, &other.state)
    }
}

impl UniformBuffer {
    pub(crate) fn new(
        gl: &WebGl2RenderingContext,
        layout: &UboLayout,
        usage: BufferUsage,
    ) -> Result<Self, String> {
        let (size, fields) = compute_layout(&layout.fields);
        validate_block_size(size, limits::max_uniform_block_size())?;
        let buffer = Buffer::new_empty(gl, BufferKind::Generic, usage, size)?;
        Ok(Self {
            buffer,
            state: Rc::new(RefCell::new(UboState {
                data: vec![0u8; size as usize],
                fields,
                dirty: None,
            })),
        })
    }

    // Flush the dirty range to the GL buffer (no-op if clean). Shared by the
    // public `upload()` and the renderer's bind-time auto-upload.
    pub(crate) fn flush(&self) {
        let mut st = self.state.borrow_mut();
        if let Some((lo, hi)) = st.dirty.take() {
            self.buffer
                .write(lo as i32, &st.data[lo as usize..hi as usize]);
        }
    }

    // The GL buffer, for binding to a uniform-buffer binding point.
    pub(crate) fn buffer_raw(&self) -> &WebGlBuffer {
        self.buffer.raw_gl()
    }
}

// A 4-byte component (the only scalar widths std140 uses here), written little-
// endian — which is also wasm's and web typed-arrays' native order, so on our
// targets `to_le_bytes` compiles to a plain store, not a byte swap.
trait Word: Copy {
    fn le(self) -> [u8; 4];
}
impl Word for f32 {
    fn le(self) -> [u8; 4] {
        self.to_le_bytes()
    }
}
impl Word for i32 {
    fn le(self) -> [u8; 4] {
        self.to_le_bytes()
    }
}
impl Word for u32 {
    fn le(self) -> [u8; 4] {
        self.to_le_bytes()
    }
}

// Writes `vals` (components, column-major for matrices) straight into `dst` at
// `offset` — no intermediate buffer. A vector/scalar is just a one-column matrix:
// column `c` starts at `offset + c*16` (the std140 column stride) and holds
// `comps` components, so the single formula covers both (for `columns == 1`,
// `c*16` is 0 and the components pack contiguously from `offset`).
fn write_std140<T: Word>(dst: &mut [u8], offset: usize, info: &Info, vals: &[T]) {
    let comps = info.comps as usize;
    for c in 0..info.columns as usize {
        let col = offset + c * 16;
        for r in 0..comps {
            let pos = col + r * 4;
            dst[pos..pos + 4].copy_from_slice(&vals[c * comps + r].le());
        }
    }
}

impl UboState {
    fn mark(&mut self, offset: u32, size: u32) {
        let end = offset + size;
        self.dirty = Some(match self.dirty {
            Some((lo, hi)) => (lo.min(offset), hi.max(end)),
            None => (offset, end),
        });
    }
}

#[wasm_bindgen]
impl UniformBuffer {
    /// Flush pending writes to the GL buffer. Normally unnecessary — see the
    /// type docs; the renderer does this for blocks used in a pass.
    pub fn upload(&self) {
        self.flush();
    }

    /// Total std140 size of the block in bytes.
    pub fn byte_size(&self) -> u32 {
        self.state.borrow().data.len() as u32
    }

    /// A copy of the CPU-side staging bytes (std140-packed). Reflects writes
    /// immediately, regardless of whether they've been uploaded.
    pub fn data(&self) -> Vec<u8> {
        self.state.borrow().data.clone()
    }

    // --- scalar / vector setters --------------------------------------------

    pub fn set_f32(&self, path: &str, x: f32) {
        self.put(path, Std140Type::F32, &[x]);
    }
    pub fn set_vec2(&self, path: &str, x: f32, y: f32) {
        self.put(path, Std140Type::Vec2, &[x, y]);
    }
    pub fn set_vec3(&self, path: &str, x: f32, y: f32, z: f32) {
        self.put(path, Std140Type::Vec3, &[x, y, z]);
    }
    pub fn set_vec4(&self, path: &str, x: f32, y: f32, z: f32, w: f32) {
        self.put(path, Std140Type::Vec4, &[x, y, z, w]);
    }

    pub fn set_i32(&self, path: &str, x: i32) {
        self.put(path, Std140Type::I32, &[x]);
    }
    pub fn set_ivec2(&self, path: &str, x: i32, y: i32) {
        self.put(path, Std140Type::IVec2, &[x, y]);
    }
    pub fn set_ivec3(&self, path: &str, x: i32, y: i32, z: i32) {
        self.put(path, Std140Type::IVec3, &[x, y, z]);
    }
    pub fn set_ivec4(&self, path: &str, x: i32, y: i32, z: i32, w: i32) {
        self.put(path, Std140Type::IVec4, &[x, y, z, w]);
    }

    pub fn set_u32(&self, path: &str, x: u32) {
        self.put(path, Std140Type::U32, &[x]);
    }
    pub fn set_uvec2(&self, path: &str, x: u32, y: u32) {
        self.put(path, Std140Type::UVec2, &[x, y]);
    }
    pub fn set_uvec3(&self, path: &str, x: u32, y: u32, z: u32) {
        self.put(path, Std140Type::UVec3, &[x, y, z]);
    }
    pub fn set_uvec4(&self, path: &str, x: u32, y: u32, z: u32, w: u32) {
        self.put(path, Std140Type::UVec4, &[x, y, z, w]);
    }

    pub fn set_bool(&self, path: &str, value: bool) {
        self.put(path, Std140Type::Bool, &[value as u32]);
    }

    /// Set a matrix field of any shape. `data` is column-major with exactly
    /// `columns * rows` floats for the field's declared type (e.g. 9 for `Mat3`,
    /// 16 for `Mat4`). std140 column padding is applied on write.
    pub fn set_mat(&self, path: &str, data: &[f32]) {
        let mut st = self.state.borrow_mut();
        let Some(&(offset, ty)) = st.fields.get(path) else {
            console::warn(&format!("[rswebgl] UBO field not found: \"{path}\""));
            return;
        };
        if !ty.is_matrix() {
            console::warn(&format!(
                "[rswebgl] UBO field \"{path}\" is {ty:?}, not a matrix"
            ));
            return;
        }
        let info = ty.info();
        let need = (info.columns * info.comps) as usize;
        if data.len() != need {
            console::warn(&format!(
                "[rswebgl] UBO field \"{path}\" ({ty:?}) expects {need} floats, got {}",
                data.len()
            ));
            return;
        }
        write_std140(&mut st.data, offset as usize, &info, data);
        st.mark(offset, info.size);
    }
}

impl UniformBuffer {
    // Shared body for the scalar/vector setters: look up the field, type-check,
    // then write the components straight into staging and grow the dirty range.
    fn put<T: Word>(&self, path: &str, expect: Std140Type, vals: &[T]) {
        let mut st = self.state.borrow_mut();
        let Some(&(offset, ty)) = st.fields.get(path) else {
            console::warn(&format!("[rswebgl] UBO field not found: \"{path}\""));
            return;
        };
        if ty != expect {
            console::warn(&format!(
                "[rswebgl] UBO field \"{path}\" is {ty:?}, not {expect:?}"
            ));
            return;
        }
        let info = expect.info();
        write_std140(&mut st.data, offset as usize, &info, vals);
        st.mark(offset, info.size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar(name: &str, ty: Std140Type) -> FieldDef {
        FieldDef {
            name: name.into(),
            kind: FieldKind::Scalar(ty),
        }
    }

    fn le_bytes(vals: &[f32]) -> Vec<u8> {
        vals.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    #[test]
    fn std140_block_with_struct_array() {
        // struct Light { vec3 pos; float intensity; vec4 color; }  → align 16, size 32
        let light = vec![
            scalar("pos", Std140Type::Vec3),
            scalar("intensity", Std140Type::F32),
            scalar("color", Std140Type::Vec4),
        ];
        assert_eq!(measure(&light), (16, 32));

        let block = vec![
            scalar("u_time", Std140Type::F32),
            scalar("u_view", Std140Type::Mat4),
            FieldDef {
                name: "lights".into(),
                kind: FieldKind::StructArray(light, 2),
            },
            scalar("u_count", Std140Type::I32),
        ];
        let (size, map) = compute_layout(&block);

        assert_eq!(map["u_time"].0, 0);
        assert_eq!(map["u_view"].0, 16); // mat4 aligned past the float
        assert_eq!(map["lights[0].pos"].0, 80); // after 16 + 64
        assert_eq!(map["lights[0].intensity"].0, 92); // packed into pos's 4th lane
        assert_eq!(map["lights[0].color"].0, 96); // vec4 re-aligned to 16
        assert_eq!(map["lights[1].pos"].0, 112); // +32 struct stride
        assert_eq!(map["lights[1].color"].0, 128);
        assert_eq!(map["u_count"].0, 144);
        assert_eq!(size, 160); // 148 rounded up to 16
    }

    #[test]
    fn std140_scalar_array_has_16_byte_stride() {
        let block = vec![FieldDef {
            name: "vals".into(),
            kind: FieldKind::Array(Std140Type::F32, 3),
        }];
        let (size, map) = compute_layout(&block);
        assert_eq!(map["vals[0]"].0, 0);
        assert_eq!(map["vals[1]"].0, 16);
        assert_eq!(map["vals[2]"].0, 32);
        assert_eq!(size, 48);
    }

    #[test]
    fn vec3_array_inside_struct() {
        // struct S { float header; vec3 data[3]; }
        let s = vec![
            scalar("header", Std140Type::F32),
            FieldDef {
                name: "data".into(),
                kind: FieldKind::Array(Std140Type::Vec3, 3),
            },
        ];
        let block = vec![FieldDef {
            name: "s".into(),
            kind: FieldKind::Struct(s),
        }];
        let (size, map) = compute_layout(&block);
        assert_eq!(map["s.header"].0, 0);
        assert_eq!(map["s.data[0]"].0, 16); // array re-aligns to 16 past the float
        assert_eq!(map["s.data[1]"].0, 32); // vec3 array stride = 16
        assert_eq!(map["s.data[2]"].0, 48);
        assert_eq!(size, 64);
    }

    #[test]
    fn struct_of_only_vec2() {
        // struct V { vec2 a; vec2 b; vec2 c; } — align rounds up to 16, size 32.
        let v = vec![
            scalar("a", Std140Type::Vec2),
            scalar("b", Std140Type::Vec2),
            scalar("c", Std140Type::Vec2),
        ];
        assert_eq!(measure(&v), (16, 32));
        let block = vec![FieldDef {
            name: "v".into(),
            kind: FieldKind::Struct(v),
        }];
        let (size, map) = compute_layout(&block);
        assert_eq!(map["v.a"].0, 0);
        assert_eq!(map["v.b"].0, 8); // vec2 packs tightly at align 8
        assert_eq!(map["v.c"].0, 16);
        assert_eq!(size, 32);
    }

    #[test]
    fn mat3_inside_struct() {
        // struct M { float tag; mat3 m; }
        let m = vec![
            scalar("tag", Std140Type::F32),
            scalar("m", Std140Type::Mat3),
        ];
        let block = vec![FieldDef {
            name: "mm".into(),
            kind: FieldKind::Struct(m),
        }];
        let (size, map) = compute_layout(&block);
        assert_eq!(map["mm.tag"].0, 0);
        assert_eq!(map["mm.m"].0, 16); // mat3 aligned to 16 past the float
        assert_eq!(size, 64); // 16 + 48
    }

    #[test]
    fn block_size_limit() {
        assert!(validate_block_size(16384, 16384).is_ok());
        assert!(validate_block_size(16400, 16384).is_err());
    }

    #[test]
    fn mat3_columns_padded_to_16() {
        // 9 floats in, written as three 12-byte columns at 0/16/32 of a 48-byte slot.
        let info = Std140Type::Mat3.info();
        assert_eq!(
            (info.align, info.size, info.columns, info.comps),
            (16, 48, 3, 3)
        );
        let data = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let mut dst = vec![0u8; 48];
        write_std140(&mut dst, 0, &info, &data);
        assert_eq!(&dst[0..12], le_bytes(&data[0..3]).as_slice()); // column 0
        assert_eq!(&dst[12..16], &[0, 0, 0, 0]); // pad
        assert_eq!(&dst[16..28], le_bytes(&data[3..6]).as_slice()); // column 1
        assert_eq!(&dst[32..44], le_bytes(&data[6..9]).as_slice()); // column 2
    }
}
