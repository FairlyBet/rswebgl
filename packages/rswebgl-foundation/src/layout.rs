//! Named fields laid out in a byte buffer.
//!
//! One description serves uniform blocks (std140), vertex/instance data and ECS
//! components (packed). Fields are addressed by path — `"u_view"`,
//! `"lights[2].color"` — which resolves once to a [`FieldId`]; writes go by id.

use std::ops::Range;

/// A primitive a field can hold. Matrices are column-major, matching GLSL and
/// glam's `to_cols_array`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldType {
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

/// How fields are placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutRule {
    /// GLSL `std140`: vec3/vec4/matrix columns aligned to 16, arrays and structs
    /// to 16, block size a multiple of 16. For uniform blocks.
    Std140,
    /// Tightly packed 4-byte components, no padding. For vertex/instance data and
    /// ECS components.
    Packed,
}

// Shape of a type: `columns` columns of `comps` 4-byte components each (a vector
// or scalar is one column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shape {
    columns: u32,
    comps: u32,
}

// Placement of a type under a rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Placement {
    align: u32,
    size: u32,
    column_stride: u32,
}

impl FieldType {
    fn shape(self) -> Shape {
        use FieldType::*;
        let s = |columns, comps| Shape { columns, comps };
        match self {
            F32 | I32 | U32 | Bool => s(1, 1),
            Vec2 | IVec2 | UVec2 => s(1, 2),
            Vec3 | IVec3 | UVec3 => s(1, 3),
            Vec4 | IVec4 | UVec4 => s(1, 4),
            Mat2 => s(2, 2),
            Mat3 => s(3, 3),
            Mat4 => s(4, 4),
            Mat2x3 => s(2, 3),
            Mat2x4 => s(2, 4),
            Mat3x2 => s(3, 2),
            Mat3x4 => s(3, 4),
            Mat4x2 => s(4, 2),
            Mat4x3 => s(4, 3),
        }
    }

    fn placement(self, rule: LayoutRule) -> Placement {
        let Shape { columns, comps } = self.shape();
        match rule {
            LayoutRule::Packed => Placement {
                align: 4,
                size: columns * comps * 4,
                column_stride: comps * 4,
            },
            // A matrix is an array of column vectors, each padded to 16. A lone
            // vector aligns to 8 (vec2) or 16 (vec3/vec4); a vec3 occupies 12.
            LayoutRule::Std140 if columns > 1 => Placement {
                align: 16,
                size: columns * 16,
                column_stride: 16,
            },
            LayoutRule::Std140 => Placement {
                align: match comps {
                    1 => 4,
                    2 => 8,
                    _ => 16,
                },
                size: comps * 4,
                column_stride: 16,
            },
        }
    }

    /// Number of 4-byte components a value of this type is written from
    /// (e.g. 3 for `Vec3`, 9 for `Mat3`).
    pub fn components(self) -> u32 {
        let s = self.shape();
        s.columns * s.comps
    }

    pub fn is_matrix(self) -> bool {
        self.shape().columns > 1
    }
}

#[derive(Clone, Debug)]
enum FieldKind {
    Scalar(FieldType),
    Array(FieldType, u32),
    Struct(Vec<FieldDef>),
    StructArray(Vec<FieldDef>, u32),
}

#[derive(Clone, Debug)]
struct FieldDef {
    name: String,
    kind: FieldKind,
}

/// Field list in declaration order. For std140 the order and types must match
/// the GLSL block exactly. A builder can be nested into another as a struct.
///
/// ```
/// use rswebgl_foundation::layout::{FieldType, LayoutBuilder, LayoutRule};
/// let light = LayoutBuilder::new()
///     .field("pos", FieldType::Vec3)
///     .field("intensity", FieldType::F32);
/// let block = LayoutBuilder::new()
///     .field("u_view", FieldType::Mat4)
///     .struct_array("lights", &light, 4)
///     .build(LayoutRule::Std140);
/// assert!(block.field_id("lights[3].intensity").is_some());
/// ```
#[derive(Clone, Debug, Default)]
pub struct LayoutBuilder {
    fields: Vec<FieldDef>,
}

impl LayoutBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// A scalar / vector / matrix field.
    pub fn field(mut self, name: &str, ty: FieldType) -> Self {
        self.push(name, FieldKind::Scalar(ty));
        self
    }

    /// An array of `count` primitives, addressed as `name[i]`.
    pub fn array(mut self, name: &str, ty: FieldType, count: u32) -> Self {
        self.push(name, FieldKind::Array(ty, count));
        self
    }

    /// A nested struct, members addressed as `name.member`.
    pub fn struct_field(mut self, name: &str, members: &LayoutBuilder) -> Self {
        self.push(name, FieldKind::Struct(members.fields.clone()));
        self
    }

    /// An array of `count` structs, members addressed as `name[i].member`.
    pub fn struct_array(mut self, name: &str, members: &LayoutBuilder, count: u32) -> Self {
        self.push(name, FieldKind::StructArray(members.fields.clone(), count));
        self
    }

    fn push(&mut self, name: &str, kind: FieldKind) {
        self.fields.push(FieldDef {
            name: name.into(),
            kind,
        });
    }

    pub fn build(&self, rule: LayoutRule) -> Layout {
        let mut out = Vec::new();
        let end = emit(&self.fields, 0, "", rule, &mut out);
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Layout {
            rule,
            size: align_up(end, block_align(rule)),
            fields: out,
        }
    }
}

/// Index of a field in its [`Layout`], resolved once from a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FieldId(u32);

impl FieldId {
    pub fn index(self) -> u32 {
        self.0
    }
}

/// A leaf field: full path, byte offset and type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub path: Box<str>,
    pub offset: u32,
    pub ty: FieldType,
}

/// A computed layout: total size and every leaf field.
#[derive(Clone, Debug)]
pub struct Layout {
    rule: LayoutRule,
    size: u32,
    // Sorted by path, so lookups are a binary search.
    fields: Vec<Field>,
}

/// Why a write was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriteError {
    /// `got` components were passed for a field that takes `expected`.
    WrongLength { expected: u32, got: u32 },
}

impl Layout {
    pub fn rule(&self) -> LayoutRule {
        self.rule
    }

    /// Total size in bytes (std140: a multiple of 16).
    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn field_id(&self, path: &str) -> Option<FieldId> {
        self.fields
            .binary_search_by(|f| f.path.as_ref().cmp(path))
            .ok()
            .map(|i| FieldId(i as u32))
    }

    pub fn field(&self, id: FieldId) -> &Field {
        &self.fields[id.0 as usize]
    }

    /// Every leaf field, sorted by path.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// Writes a field's components (column-major for matrices) into `dst`,
    /// applying the rule's column padding. Returns the byte range touched, for
    /// change tracking. Doesn't check float vs int: that's the caller's type.
    pub fn write<T: Word>(
        &self,
        dst: &mut [u8],
        id: FieldId,
        vals: &[T],
    ) -> Result<Range<u32>, WriteError> {
        let f = &self.fields[id.0 as usize];
        let shape = f.ty.shape();
        let expected = shape.columns * shape.comps;
        if vals.len() as u32 != expected {
            return Err(WriteError::WrongLength {
                expected,
                got: vals.len() as u32,
            });
        }
        let p = f.ty.placement(self.rule);
        let base = f.offset as usize;
        let comps = shape.comps as usize;
        for c in 0..shape.columns as usize {
            let col = base + c * p.column_stride as usize;
            for r in 0..comps {
                let at = col + r * 4;
                dst[at..at + 4].copy_from_slice(&vals[c * comps + r].le());
            }
        }
        Ok(f.offset..f.offset + p.size)
    }
}

/// A 4-byte component, written little-endian (wasm's and typed arrays' native
/// order, so this is a plain store).
pub trait Word: Copy {
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

fn align_up(value: u32, align: u32) -> u32 {
    value.div_ceil(align) * align
}

fn block_align(rule: LayoutRule) -> u32 {
    match rule {
        LayoutRule::Std140 => 16,
        LayoutRule::Packed => 4,
    }
}

// Arrays and structs are 16-aligned in std140; packed has no such rule.
fn aggregate_align(rule: LayoutRule) -> u32 {
    block_align(rule)
}

fn array_stride(ty: FieldType, rule: LayoutRule) -> u32 {
    let size = ty.placement(rule).size;
    align_up(size, aggregate_align(rule))
}

fn member_align(kind: &FieldKind, rule: LayoutRule) -> u32 {
    match kind {
        FieldKind::Scalar(ty) => ty.placement(rule).align,
        FieldKind::Array(..) => aggregate_align(rule),
        FieldKind::Struct(f) | FieldKind::StructArray(f, _) => measure(f, rule).0,
    }
}

// Struct (align, size): the largest member alignment rounded up to the
// aggregate alignment; size is the laid-out end rounded up to that.
fn measure(fields: &[FieldDef], rule: LayoutRule) -> (u32, u32) {
    let align = align_up(
        fields
            .iter()
            .map(|f| member_align(&f.kind, rule))
            .max()
            .unwrap_or(1),
        aggregate_align(rule),
    );
    let end = emit(fields, 0, "", rule, &mut Vec::new());
    (align, align_up(end, align))
}

// Walks `fields` from `base`, recording each leaf into `out`; returns the
// offset just past the last field.
fn emit(
    fields: &[FieldDef],
    base: u32,
    prefix: &str,
    rule: LayoutRule,
    out: &mut Vec<Field>,
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
                let p = ty.placement(rule);
                off = align_up(off, p.align);
                out.push(Field {
                    path: name.into(),
                    offset: off,
                    ty: *ty,
                });
                off += p.size;
            }
            FieldKind::Array(ty, count) => {
                let stride = array_stride(*ty, rule);
                off = align_up(off, aggregate_align(rule));
                for i in 0..*count {
                    out.push(Field {
                        path: format!("{name}[{i}]").into(),
                        offset: off + i * stride,
                        ty: *ty,
                    });
                }
                off += stride * count;
            }
            FieldKind::Struct(sf) => {
                let (salign, ssize) = measure(sf, rule);
                off = align_up(off, salign);
                emit(sf, off, &name, rule, out);
                off += ssize;
            }
            FieldKind::StructArray(sf, count) => {
                let (salign, ssize) = measure(sf, rule);
                off = align_up(off, salign);
                for i in 0..*count {
                    emit(sf, off + i * ssize, &format!("{name}[{i}]"), rule, out);
                }
                off += ssize * count;
            }
        }
    }
    off
}

#[cfg(test)]
mod tests {
    use super::*;
    use FieldType::*;
    use LayoutRule::*;

    fn offset(l: &Layout, path: &str) -> u32 {
        l.field(
            l.field_id(path)
                .unwrap_or_else(|| panic!("no field {path}")),
        )
        .offset
    }

    fn le_bytes(vals: &[f32]) -> Vec<u8> {
        vals.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    #[test]
    fn std140_block_with_struct_array() {
        // struct Light { vec3 pos; float intensity; vec4 color; }  → align 16, size 32
        let light = LayoutBuilder::new()
            .field("pos", Vec3)
            .field("intensity", F32)
            .field("color", Vec4);
        assert_eq!(measure(&light.fields, Std140), (16, 32));

        let l = LayoutBuilder::new()
            .field("u_time", F32)
            .field("u_view", Mat4)
            .struct_array("lights", &light, 2)
            .field("u_count", I32)
            .build(Std140);
        assert_eq!(offset(&l, "u_time"), 0);
        assert_eq!(offset(&l, "u_view"), 16); // mat4 aligned past the float
        assert_eq!(offset(&l, "lights[0].pos"), 80); // after 16 + 64
        assert_eq!(offset(&l, "lights[0].intensity"), 92); // packed into pos's 4th lane
        assert_eq!(offset(&l, "lights[0].color"), 96); // vec4 re-aligned to 16
        assert_eq!(offset(&l, "lights[1].pos"), 112); // +32 struct stride
        assert_eq!(offset(&l, "lights[1].color"), 128);
        assert_eq!(offset(&l, "u_count"), 144);
        assert_eq!(l.size(), 160); // 148 rounded up to 16
    }

    #[test]
    fn std140_scalar_array_has_16_byte_stride() {
        let l = LayoutBuilder::new().array("vals", F32, 3).build(Std140);
        assert_eq!(offset(&l, "vals[0]"), 0);
        assert_eq!(offset(&l, "vals[1]"), 16);
        assert_eq!(offset(&l, "vals[2]"), 32);
        assert_eq!(l.size(), 48);
    }

    #[test]
    fn std140_vec3_array_inside_struct() {
        // struct S { float header; vec3 data[3]; }
        let s = LayoutBuilder::new()
            .field("header", F32)
            .array("data", Vec3, 3);
        let l = LayoutBuilder::new().struct_field("s", &s).build(Std140);
        assert_eq!(offset(&l, "s.header"), 0);
        assert_eq!(offset(&l, "s.data[0]"), 16); // array re-aligns to 16 past the float
        assert_eq!(offset(&l, "s.data[1]"), 32); // vec3 array stride = 16
        assert_eq!(offset(&l, "s.data[2]"), 48);
        assert_eq!(l.size(), 64);
    }

    #[test]
    fn std140_struct_of_only_vec2() {
        // struct V { vec2 a; vec2 b; vec2 c; } — align rounds up to 16, size 32.
        let v = LayoutBuilder::new()
            .field("a", Vec2)
            .field("b", Vec2)
            .field("c", Vec2);
        assert_eq!(measure(&v.fields, Std140), (16, 32));
        let l = LayoutBuilder::new().struct_field("v", &v).build(Std140);
        assert_eq!(offset(&l, "v.a"), 0);
        assert_eq!(offset(&l, "v.b"), 8); // vec2 packs tightly at align 8
        assert_eq!(offset(&l, "v.c"), 16);
        assert_eq!(l.size(), 32);
    }

    #[test]
    fn std140_mat3_inside_struct() {
        // struct M { float tag; mat3 m; }
        let m = LayoutBuilder::new().field("tag", F32).field("m", Mat3);
        let l = LayoutBuilder::new().struct_field("mm", &m).build(Std140);
        assert_eq!(offset(&l, "mm.tag"), 0);
        assert_eq!(offset(&l, "mm.m"), 16); // mat3 aligned to 16 past the float
        assert_eq!(l.size(), 64); // 16 + 48
    }

    #[test]
    fn std140_mat3_columns_padded_to_16() {
        let l = LayoutBuilder::new().field("m", Mat3).build(Std140);
        let id = l.field_id("m").unwrap();
        let data = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let mut dst = vec![0u8; l.size() as usize];
        assert_eq!(l.write(&mut dst, id, &data), Ok(0..48));
        assert_eq!(&dst[0..12], le_bytes(&data[0..3]).as_slice()); // column 0
        assert_eq!(&dst[12..16], &[0, 0, 0, 0]); // pad
        assert_eq!(&dst[16..28], le_bytes(&data[3..6]).as_slice()); // column 1
        assert_eq!(&dst[32..44], le_bytes(&data[6..9]).as_slice()); // column 2
    }

    #[test]
    fn packed_has_no_padding() {
        // Instance data: mat4 model; vec3 tint; float alpha; vec2 uv[2]
        let l = LayoutBuilder::new()
            .field("model", Mat4)
            .field("tint", Vec3)
            .field("alpha", F32)
            .array("uv", Vec2, 2)
            .build(Packed);
        assert_eq!(offset(&l, "model"), 0);
        assert_eq!(offset(&l, "tint"), 64);
        assert_eq!(offset(&l, "alpha"), 76);
        assert_eq!(offset(&l, "uv[0]"), 80);
        assert_eq!(offset(&l, "uv[1]"), 88);
        assert_eq!(l.size(), 96);
    }

    #[test]
    fn packed_mat3_columns_are_contiguous() {
        let l = LayoutBuilder::new().field("m", Mat3).build(Packed);
        let data = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let mut dst = vec![0u8; l.size() as usize];
        assert_eq!(
            l.write(&mut dst, l.field_id("m").unwrap(), &data),
            Ok(0..36)
        );
        assert_eq!(dst, le_bytes(&data));
    }

    #[test]
    fn packed_struct_array() {
        let p = LayoutBuilder::new().field("pos", Vec3).field("size", F32);
        let l = LayoutBuilder::new().struct_array("p", &p, 3).build(Packed);
        assert_eq!(offset(&l, "p[1].pos"), 16);
        assert_eq!(offset(&l, "p[2].size"), 44);
        assert_eq!(l.size(), 48);
    }

    #[test]
    fn write_checks_length() {
        let l = LayoutBuilder::new().field("v", Vec4).build(Std140);
        let mut dst = vec![0u8; 16];
        assert_eq!(
            l.write(&mut dst, l.field_id("v").unwrap(), &[1.0f32, 2.0]),
            Err(WriteError::WrongLength {
                expected: 4,
                got: 2
            })
        );
    }

    #[test]
    fn unknown_path_has_no_id() {
        let l = LayoutBuilder::new().field("a", F32).build(Std140);
        assert!(l.field_id("b").is_none());
        assert!(l.field_id("a[0]").is_none());
    }
}
