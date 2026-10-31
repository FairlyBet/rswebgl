use rswebgl_foundation::pool::Destroyed;
use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlVertexArrayObject};

use crate::buffer::BufferKind;
use crate::console;
use crate::context::{Context, Resources, stale};
use crate::ids::{BufferId, VertexArrayId};
use crate::limits;

// ---------------------------------------------------------------------------
// AttrType
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttrType {
    Byte = 0x1400,          // BYTE
    UnsignedByte = 0x1401,  // UNSIGNED_BYTE
    Short = 0x1402,         // SHORT
    UnsignedShort = 0x1403, // UNSIGNED_SHORT
    Int = 0x1404,           // INT
    UnsignedInt = 0x1405,   // UNSIGNED_INT
    Float = 0x1406,         // FLOAT
    HalfFloat = 0x140B,     // HALF_FLOAT
}

impl AttrType {
    fn as_gl(&self) -> u32 {
        self.clone() as u32
    }
}

// ---------------------------------------------------------------------------
// VertexAttr
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug)]
pub struct VertexAttr {
    size: u8,
    attr_type: AttrType,
    normalized: bool,
    integer: bool,
    pub stride: i32,
    pub offset: i32,
    pub divisor: u32,
}

impl VertexAttr {
    fn new(size: u8, attr_type: AttrType, normalized: bool, integer: bool) -> Self {
        Self {
            size,
            attr_type,
            normalized,
            integer,
            stride: 0,
            offset: 0,
            divisor: 0,
        }
    }
}

#[wasm_bindgen]
impl VertexAttr {
    // --- float pointer (vertexAttribPointer) ---

    pub fn float(size: u8) -> Self {
        Self::new(size, AttrType::Float, false, false)
    }
    pub fn half_float(size: u8) -> Self {
        Self::new(size, AttrType::HalfFloat, false, false)
    }
    pub fn normalized_byte(size: u8) -> Self {
        Self::new(size, AttrType::Byte, true, false)
    }
    pub fn normalized_ubyte(size: u8) -> Self {
        Self::new(size, AttrType::UnsignedByte, true, false)
    }
    pub fn normalized_short(size: u8) -> Self {
        Self::new(size, AttrType::Short, true, false)
    }
    pub fn normalized_ushort(size: u8) -> Self {
        Self::new(size, AttrType::UnsignedShort, true, false)
    }

    // --- integer pointer (vertexAttribIPointer) ---

    pub fn int(size: u8) -> Self {
        Self::new(size, AttrType::Int, false, true)
    }
    pub fn uint(size: u8) -> Self {
        Self::new(size, AttrType::UnsignedInt, false, true)
    }
    pub fn ibyte(size: u8) -> Self {
        Self::new(size, AttrType::Byte, false, true)
    }
    pub fn iubyte(size: u8) -> Self {
        Self::new(size, AttrType::UnsignedByte, false, true)
    }
    pub fn ishort(size: u8) -> Self {
        Self::new(size, AttrType::Short, false, true)
    }
    pub fn iushort(size: u8) -> Self {
        Self::new(size, AttrType::UnsignedShort, false, true)
    }
}

// ---------------------------------------------------------------------------
// VertexArray
// ---------------------------------------------------------------------------

/// A vertex array in the context's pool. The buffers it reads are retained in
/// the buffer pool (a use count, not ownership): destroying such a buffer is
/// deferred until no vertex array uses it. Buffers in `owned` were created
/// together with the vertex array (by `VaoBuilder`) and are destroyed with it.
#[derive(Debug)]
pub(crate) struct VertexArrayObj {
    pub(crate) raw: WebGlVertexArrayObject,
    attribs: Vec<Option<BufferId>>,
    index_buffer: Option<BufferId>,
    pub(crate) owned: Vec<BufferId>,
}

// Drops one use of a buffer; deletes it if that was the last use of a buffer
// that was already destroyed.
pub(crate) fn release_buffer(res: &mut Resources, gl: &WebGl2RenderingContext, id: BufferId) {
    if let Some(obj) = res.buffers.release(id.0) {
        obj.delete(gl);
    }
}

fn attr_index_in_range(index: u32) -> bool {
    let max = limits::max_vertex_attribs();
    if index >= max {
        console::error(&format!(
            "[rswebgl] vertex attribute index {index} is out of range (MAX_VERTEX_ATTRIBS = \
             {max}); ignored"
        ));
    }
    index < max
}

impl Context {
    pub(crate) fn insert_vertex_array(
        &self,
        raw: WebGlVertexArrayObject,
    ) -> Result<VertexArrayId, String> {
        let obj = VertexArrayObj {
            raw,
            attribs: Vec::new(),
            index_buffer: None,
            owned: Vec::new(),
        };
        self.res()
            .vertex_arrays
            .insert(obj)
            .map(Into::into)
            .map_err(|e| e.to_string())
    }
}

#[wasm_bindgen]
impl Context {
    pub fn create_vertex_array(&self) -> Result<VertexArrayId, String> {
        self.check_alive()?;
        let raw = self
            .gl()
            .create_vertex_array()
            .ok_or("createVertexArray failed")?;
        self.insert_vertex_array(raw)
    }

    /// Reads attribute `index` from `buffer` (a vertex buffer). The vertex array
    /// keeps using the buffer until the attribute is replaced or removed.
    pub fn vertex_array_attr(
        &self,
        vao: VertexArrayId,
        index: u32,
        buffer: BufferId,
        attr: &VertexAttr,
    ) {
        if !attr_index_in_range(index) {
            return;
        }
        let gl = self.gl();
        let res = &mut *self.res();
        let Some(v) = res.vertex_arrays.get(vao.0) else {
            return stale("vertex array", "vertex_array_attr", vao);
        };
        let Some(b) = res.buffers.get(buffer.0) else {
            return stale("buffer", "vertex_array_attr", buffer);
        };
        if b.kind != BufferKind::Generic {
            console::error("[rswebgl] vertex_array_attr: an index buffer can't hold vertex data");
            return;
        }

        self.gl_state().forget_vao();
        gl.bind_vertex_array(Some(&v.raw));
        gl.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(&b.raw));
        if attr.integer {
            gl.vertex_attrib_i_pointer_with_i32(
                index,
                attr.size as i32,
                attr.attr_type.as_gl(),
                attr.stride,
                attr.offset,
            );
        } else {
            gl.vertex_attrib_pointer_with_i32(
                index,
                attr.size as i32,
                attr.attr_type.as_gl(),
                attr.normalized,
                attr.stride,
                attr.offset,
            );
        }
        gl.enable_vertex_attrib_array(index);
        gl.vertex_attrib_divisor(index, attr.divisor);
        gl.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, None);
        gl.bind_vertex_array(None);

        res.buffers.retain(buffer.0);
        let v = res.vertex_arrays.get_mut(vao.0).expect("checked above");
        let idx = index as usize;
        if idx >= v.attribs.len() {
            v.attribs.resize(idx + 1, None);
        }
        if let Some(prev) = v.attribs[idx].replace(buffer) {
            release_buffer(res, gl, prev);
        }
    }

    pub fn vertex_array_remove_attr(&self, vao: VertexArrayId, index: u32) {
        if !attr_index_in_range(index) {
            return;
        }
        let gl = self.gl();
        let res = &mut *self.res();
        let Some(v) = res.vertex_arrays.get_mut(vao.0) else {
            return stale("vertex array", "vertex_array_remove_attr", vao);
        };
        self.gl_state().forget_vao();
        gl.bind_vertex_array(Some(&v.raw));
        gl.disable_vertex_attrib_array(index);
        gl.bind_vertex_array(None);
        if let Some(prev) = v.attribs.get_mut(index as usize).and_then(Option::take) {
            release_buffer(res, gl, prev);
        }
    }

    /// Uses `buffer` (created with `create_index_buffer`) for indexed draws.
    pub fn vertex_array_set_index_buffer(&self, vao: VertexArrayId, buffer: BufferId) {
        let gl = self.gl();
        let res = &mut *self.res();
        let Some(v) = res.vertex_arrays.get(vao.0) else {
            return stale("vertex array", "vertex_array_set_index_buffer", vao);
        };
        let Some(b) = res.buffers.get(buffer.0) else {
            return stale("buffer", "vertex_array_set_index_buffer", buffer);
        };
        if b.kind != BufferKind::Index {
            console::error(
                "[rswebgl] vertex_array_set_index_buffer: buffer wasn't created with \
                 create_index_buffer",
            );
            return;
        }
        self.gl_state().forget_vao();
        gl.bind_vertex_array(Some(&v.raw));
        gl.bind_buffer(WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER, Some(&b.raw));
        gl.bind_vertex_array(None);

        res.buffers.retain(buffer.0);
        let v = res.vertex_arrays.get_mut(vao.0).expect("checked above");
        if let Some(prev) = v.index_buffer.replace(buffer) {
            release_buffer(res, gl, prev);
        }
    }

    pub fn vertex_array_remove_index_buffer(&self, vao: VertexArrayId) {
        let gl = self.gl();
        let res = &mut *self.res();
        let Some(v) = res.vertex_arrays.get_mut(vao.0) else {
            return stale("vertex array", "vertex_array_remove_index_buffer", vao);
        };
        self.gl_state().forget_vao();
        gl.bind_vertex_array(Some(&v.raw));
        gl.bind_buffer(WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER, None);
        gl.bind_vertex_array(None);
        if let Some(prev) = v.index_buffer.take() {
            release_buffer(res, gl, prev);
        }
    }

    /// Deletes the vertex array, stops using its buffers, and destroys the
    /// buffers it owns (the ones `VaoBuilder` created for it).
    pub fn destroy_vertex_array(&self, vao: VertexArrayId) {
        let gl = self.gl();
        let res = &mut *self.res();
        let obj = match res.vertex_arrays.destroy(vao.0) {
            Destroyed::Now(obj) => obj,
            Destroyed::Deferred => return,
            Destroyed::Invalid => return stale("vertex array", "destroy_vertex_array", vao),
        };
        gl.delete_vertex_array(Some(&obj.raw));
        for id in obj.attribs.into_iter().flatten().chain(obj.index_buffer) {
            release_buffer(res, gl, id);
        }
        for id in obj.owned {
            if let Destroyed::Now(b) = res.buffers.destroy(id.0) {
                b.delete(gl);
            }
        }
    }
}
