use std::collections::BTreeSet;

use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::buffer::{Buffer, BufferTarget, BufferUsage};
use crate::console;
use crate::context::Context;
use crate::vao::{VertexArray, VertexAttr};

// ---------------------------------------------------------------------------
// AttrKind
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrKind {
    // f32 vectors / matrices
    F32,
    F32x2,
    F32x3,
    F32x4,
    Mat2,
    Mat3,
    Mat4,

    // half-float (passed as u16 bits)
    F16,
    F16x2,
    F16x3,
    F16x4,

    // integer attrs (read as ivec/uvec in shader)
    I8,
    I8x2,
    I8x3,
    I8x4,
    U8,
    U8x2,
    U8x3,
    U8x4,
    I16,
    I16x2,
    I16x3,
    I16x4,
    U16,
    U16x2,
    U16x3,
    U16x4,
    I32,
    I32x2,
    I32x3,
    I32x4,
    U32,
    U32x2,
    U32x3,
    U32x4,

    // normalized -> float in shader
    I8Norm,
    I8x2Norm,
    I8x3Norm,
    I8x4Norm,
    U8Norm,
    U8x2Norm,
    U8x3Norm,
    U8x4Norm,
    I16Norm,
    I16x2Norm,
    I16x3Norm,
    I16x4Norm,
    U16Norm,
    U16x2Norm,
    U16x3Norm,
    U16x4Norm,
}

#[derive(Clone, Copy)]
struct AttrDesc {
    gl_type: u32,
    components: u32,   // per slot (1..=4)
    slots: u32,        // 1 for vectors, 2/3/4 for matrices
    element_size: u32, // bytes per scalar
    normalized: bool,
    integer: bool,
}

impl AttrKind {
    fn desc(self) -> AttrDesc {
        use AttrKind::*;
        use WebGl2RenderingContext as Gl;

        macro_rules! d {
            ($t:expr, $c:expr, $s:expr, $sz:expr, $n:expr, $i:expr) => {
                AttrDesc {
                    gl_type: $t,
                    components: $c,
                    slots: $s,
                    element_size: $sz,
                    normalized: $n,
                    integer: $i,
                }
            };
        }

        match self {
            F32 => d!(Gl::FLOAT, 1, 1, 4, false, false),
            F32x2 => d!(Gl::FLOAT, 2, 1, 4, false, false),
            F32x3 => d!(Gl::FLOAT, 3, 1, 4, false, false),
            F32x4 => d!(Gl::FLOAT, 4, 1, 4, false, false),
            Mat2 => d!(Gl::FLOAT, 2, 2, 4, false, false),
            Mat3 => d!(Gl::FLOAT, 3, 3, 4, false, false),
            Mat4 => d!(Gl::FLOAT, 4, 4, 4, false, false),

            F16 => d!(Gl::HALF_FLOAT, 1, 1, 2, false, false),
            F16x2 => d!(Gl::HALF_FLOAT, 2, 1, 2, false, false),
            F16x3 => d!(Gl::HALF_FLOAT, 3, 1, 2, false, false),
            F16x4 => d!(Gl::HALF_FLOAT, 4, 1, 2, false, false),

            I8 => d!(Gl::BYTE, 1, 1, 1, false, true),
            I8x2 => d!(Gl::BYTE, 2, 1, 1, false, true),
            I8x3 => d!(Gl::BYTE, 3, 1, 1, false, true),
            I8x4 => d!(Gl::BYTE, 4, 1, 1, false, true),
            U8 => d!(Gl::UNSIGNED_BYTE, 1, 1, 1, false, true),
            U8x2 => d!(Gl::UNSIGNED_BYTE, 2, 1, 1, false, true),
            U8x3 => d!(Gl::UNSIGNED_BYTE, 3, 1, 1, false, true),
            U8x4 => d!(Gl::UNSIGNED_BYTE, 4, 1, 1, false, true),
            I16 => d!(Gl::SHORT, 1, 1, 2, false, true),
            I16x2 => d!(Gl::SHORT, 2, 1, 2, false, true),
            I16x3 => d!(Gl::SHORT, 3, 1, 2, false, true),
            I16x4 => d!(Gl::SHORT, 4, 1, 2, false, true),
            U16 => d!(Gl::UNSIGNED_SHORT, 1, 1, 2, false, true),
            U16x2 => d!(Gl::UNSIGNED_SHORT, 2, 1, 2, false, true),
            U16x3 => d!(Gl::UNSIGNED_SHORT, 3, 1, 2, false, true),
            U16x4 => d!(Gl::UNSIGNED_SHORT, 4, 1, 2, false, true),
            I32 => d!(Gl::INT, 1, 1, 4, false, true),
            I32x2 => d!(Gl::INT, 2, 1, 4, false, true),
            I32x3 => d!(Gl::INT, 3, 1, 4, false, true),
            I32x4 => d!(Gl::INT, 4, 1, 4, false, true),
            U32 => d!(Gl::UNSIGNED_INT, 1, 1, 4, false, true),
            U32x2 => d!(Gl::UNSIGNED_INT, 2, 1, 4, false, true),
            U32x3 => d!(Gl::UNSIGNED_INT, 3, 1, 4, false, true),
            U32x4 => d!(Gl::UNSIGNED_INT, 4, 1, 4, false, true),

            I8Norm => d!(Gl::BYTE, 1, 1, 1, true, false),
            I8x2Norm => d!(Gl::BYTE, 2, 1, 1, true, false),
            I8x3Norm => d!(Gl::BYTE, 3, 1, 1, true, false),
            I8x4Norm => d!(Gl::BYTE, 4, 1, 1, true, false),
            U8Norm => d!(Gl::UNSIGNED_BYTE, 1, 1, 1, true, false),
            U8x2Norm => d!(Gl::UNSIGNED_BYTE, 2, 1, 1, true, false),
            U8x3Norm => d!(Gl::UNSIGNED_BYTE, 3, 1, 1, true, false),
            U8x4Norm => d!(Gl::UNSIGNED_BYTE, 4, 1, 1, true, false),
            I16Norm => d!(Gl::SHORT, 1, 1, 2, true, false),
            I16x2Norm => d!(Gl::SHORT, 2, 1, 2, true, false),
            I16x3Norm => d!(Gl::SHORT, 3, 1, 2, true, false),
            I16x4Norm => d!(Gl::SHORT, 4, 1, 2, true, false),
            U16Norm => d!(Gl::UNSIGNED_SHORT, 1, 1, 2, true, false),
            U16x2Norm => d!(Gl::UNSIGNED_SHORT, 2, 1, 2, true, false),
            U16x3Norm => d!(Gl::UNSIGNED_SHORT, 3, 1, 2, true, false),
            U16x4Norm => d!(Gl::UNSIGNED_SHORT, 4, 1, 2, true, false),
        }
    }

    fn bytes_per_vertex(self) -> u32 {
        let d = self.desc();
        d.components * d.slots * d.element_size
    }

    fn to_vertex_attr(self) -> VertexAttr {
        let d = self.desc();
        let c = d.components as u8;
        use WebGl2RenderingContext as Gl;

        if d.integer {
            match d.gl_type {
                x if x == Gl::INT => VertexAttr::int(c),
                x if x == Gl::UNSIGNED_INT => VertexAttr::uint(c),
                x if x == Gl::BYTE => VertexAttr::ibyte(c),
                x if x == Gl::UNSIGNED_BYTE => VertexAttr::iubyte(c),
                x if x == Gl::SHORT => VertexAttr::ishort(c),
                x if x == Gl::UNSIGNED_SHORT => VertexAttr::iushort(c),
                _ => unreachable!("invalid integer AttrKind descriptor"),
            }
        } else if d.normalized {
            match d.gl_type {
                x if x == Gl::BYTE => VertexAttr::normalized_byte(c),
                x if x == Gl::UNSIGNED_BYTE => VertexAttr::normalized_ubyte(c),
                x if x == Gl::SHORT => VertexAttr::normalized_short(c),
                x if x == Gl::UNSIGNED_SHORT => VertexAttr::normalized_ushort(c),
                _ => unreachable!("invalid normalized AttrKind descriptor"),
            }
        } else {
            match d.gl_type {
                x if x == Gl::FLOAT => VertexAttr::float(c),
                x if x == Gl::HALF_FLOAT => VertexAttr::half_float(c),
                _ => unreachable!("invalid float AttrKind descriptor"),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// VaoBuilder
// ---------------------------------------------------------------------------

struct AttrEntry {
    index: u32,
    kind: AttrKind,
    data: Vec<u8>,
    source_gl_type: u32,
}

struct IndexEntry {
    gl_type: u32,
    data: Vec<u8>,
}

#[wasm_bindgen]
pub struct VaoBuilder {
    gl: WebGl2RenderingContext,
    attrs: Vec<AttrEntry>,
    indices: Option<IndexEntry>,
}

fn bytes_of<T>(slice: &[T]) -> Vec<u8> {
    let len = std::mem::size_of_val(slice);
    let ptr = slice.as_ptr() as *const u8;
    unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec()
}

#[wasm_bindgen]
impl VaoBuilder {
    #[wasm_bindgen(constructor)]
    pub fn new(ctx: &Context) -> Self {
        Self {
            gl: ctx.gl(),
            attrs: Vec::new(),
            indices: None,
        }
    }

    pub fn add_f32(&mut self, index: u32, kind: AttrKind, data: &[f32]) {
        self.attrs.push(AttrEntry {
            index,
            kind,
            data: bytes_of(data),
            source_gl_type: WebGl2RenderingContext::FLOAT,
        });
    }

    pub fn add_f16(&mut self, index: u32, kind: AttrKind, data: &[u16]) {
        self.attrs.push(AttrEntry {
            index,
            kind,
            data: bytes_of(data),
            source_gl_type: WebGl2RenderingContext::HALF_FLOAT,
        });
    }

    pub fn add_i8(&mut self, index: u32, kind: AttrKind, data: &[i8]) {
        self.attrs.push(AttrEntry {
            index,
            kind,
            data: bytes_of(data),
            source_gl_type: WebGl2RenderingContext::BYTE,
        });
    }

    pub fn add_u8(&mut self, index: u32, kind: AttrKind, data: &[u8]) {
        self.attrs.push(AttrEntry {
            index,
            kind,
            data: data.to_vec(),
            source_gl_type: WebGl2RenderingContext::UNSIGNED_BYTE,
        });
    }

    pub fn add_i16(&mut self, index: u32, kind: AttrKind, data: &[i16]) {
        self.attrs.push(AttrEntry {
            index,
            kind,
            data: bytes_of(data),
            source_gl_type: WebGl2RenderingContext::SHORT,
        });
    }

    pub fn add_u16(&mut self, index: u32, kind: AttrKind, data: &[u16]) {
        self.attrs.push(AttrEntry {
            index,
            kind,
            data: bytes_of(data),
            source_gl_type: WebGl2RenderingContext::UNSIGNED_SHORT,
        });
    }

    pub fn add_i32(&mut self, index: u32, kind: AttrKind, data: &[i32]) {
        self.attrs.push(AttrEntry {
            index,
            kind,
            data: bytes_of(data),
            source_gl_type: WebGl2RenderingContext::INT,
        });
    }

    pub fn add_u32(&mut self, index: u32, kind: AttrKind, data: &[u32]) {
        self.attrs.push(AttrEntry {
            index,
            kind,
            data: bytes_of(data),
            source_gl_type: WebGl2RenderingContext::UNSIGNED_INT,
        });
    }

    pub fn add_indices_u8(&mut self, data: &[u8]) {
        self.indices = Some(IndexEntry {
            gl_type: WebGl2RenderingContext::UNSIGNED_BYTE,
            data: data.to_vec(),
        });
    }

    pub fn add_indices_u16(&mut self, data: &[u16]) {
        self.indices = Some(IndexEntry {
            gl_type: WebGl2RenderingContext::UNSIGNED_SHORT,
            data: bytes_of(data),
        });
    }

    pub fn add_indices_u32(&mut self, data: &[u32]) {
        self.indices = Some(IndexEntry {
            gl_type: WebGl2RenderingContext::UNSIGNED_INT,
            data: bytes_of(data),
        });
    }

    pub fn build(self) -> Result<VertexArray, String> {
        let mut vao = VertexArray::new(&self.gl)?;
        self.build_into(&mut vao)?;
        Ok(vao)
    }

    pub fn build_into(self, vao: &mut VertexArray) -> Result<(), String> {
        // 1. Source type matches declared kind
        for e in &self.attrs {
            let expected = e.kind.desc().gl_type;
            if expected != e.source_gl_type {
                return Err(format!(
                    "VaoBuilder: attr index {} has kind {:?} (expects GL type 0x{:X}) but was added via a typed method for GL type 0x{:X}",
                    e.index, e.kind, expected, e.source_gl_type
                ));
            }
        }

        // 2. Vertex counts consistent across all attrs
        let mut vertex_count: Option<u32> = None;
        for e in &self.attrs {
            let bpv = e.kind.bytes_per_vertex();
            if !e.data.len().is_multiple_of(bpv as usize) {
                return Err(format!(
                    "VaoBuilder: attr index {} ({:?}) data length {} is not a multiple of bytes-per-vertex {}",
                    e.index,
                    e.kind,
                    e.data.len(),
                    bpv
                ));
            }
            let n = (e.data.len() as u32) / bpv;
            match vertex_count {
                None => vertex_count = Some(n),
                Some(prev) if prev != n => {
                    return Err(format!(
                        "VaoBuilder: attr index {} has {} vertices, expected {} (from earlier attr)",
                        e.index, n, prev
                    ));
                }
                _ => {}
            }
        }
        let vertex_count = vertex_count.unwrap_or(0);

        // 3. No attribute slot collisions (matrices span multiple slots)
        let mut used: BTreeSet<u32> = BTreeSet::new();
        for e in &self.attrs {
            let d = e.kind.desc();
            for s in 0..d.slots {
                let slot = e.index + s;
                if !used.insert(slot) {
                    return Err(format!(
                        "VaoBuilder: attribute slot {} used by multiple entries (collision from {:?} at index {})",
                        slot, e.kind, e.index
                    ));
                }
            }
        }

        // 4. Compute interleaved offsets with alignment padding
        let mut running = 0u32;
        let mut max_align = 1u32;
        let mut offsets: Vec<u32> = Vec::with_capacity(self.attrs.len());
        for e in &self.attrs {
            let d = e.kind.desc();
            let align = d.element_size;
            max_align = max_align.max(align);
            let aligned = running.next_multiple_of(align);
            if aligned != running {
                console::warn(&format!(
                    "[rswebgl] VaoBuilder: padded attr at index {} from offset {} to {} for {}-byte alignment",
                    e.index, running, aligned, align
                ));
            }
            offsets.push(aligned);
            running = aligned + e.kind.bytes_per_vertex();
        }
        let stride = running.next_multiple_of(max_align);
        if stride != running {
            console::warn(&format!(
                "[rswebgl] VaoBuilder: padded total stride from {} to {} for {}-byte alignment",
                running, stride, max_align
            ));
        }

        // 5. Interleave into one byte buffer
        let total = stride as usize * vertex_count as usize;
        let mut interleaved = vec![0u8; total];
        for (e, &off) in self.attrs.iter().zip(offsets.iter()) {
            let pv = e.kind.bytes_per_vertex() as usize;
            for v in 0..vertex_count as usize {
                let dst = v * stride as usize + off as usize;
                let src = v * pv;
                interleaved[dst..dst + pv].copy_from_slice(&e.data[src..src + pv]);
            }
        }

        // 6. Upload
        let vbo = Buffer::new(
            &self.gl,
            BufferTarget::Array,
            BufferUsage::StaticDraw,
            &interleaved,
        )?;

        // 7. Configure attribs — one vertex_attrib_pointer per slot for matrices
        for (e, &off) in self.attrs.iter().zip(offsets.iter()) {
            let d = e.kind.desc();
            let slot_bytes = d.components * d.element_size;
            let mut attr = e.kind.to_vertex_attr();
            attr.stride = stride as i32;
            for s in 0..d.slots {
                attr.offset = (off + s * slot_bytes) as i32;
                vao.attr(e.index + s, &vbo, &attr);
            }
        }

        // 8. Index buffer (optional)
        if let Some(idx) = &self.indices {
            let ibo = Buffer::new(
                &self.gl,
                BufferTarget::ElementArray,
                BufferUsage::StaticDraw,
                &idx.data,
            )?;
            vao.set_index_buffer(&ibo);
            let _ = idx.gl_type; // tracked for future API (e.g. expose IndexType to caller)
        }

        Ok(())
    }
}
