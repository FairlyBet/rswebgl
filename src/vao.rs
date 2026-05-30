use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlVertexArrayObject};

use crate::buffer::Buffer;
use crate::ref_count::{RefCount, ref_counted};

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

#[derive(Debug, Clone)]
struct VertexArrayInner {
    gl: WebGl2RenderingContext,
    raw: WebGlVertexArrayObject,
    attribs: Vec<Option<(Buffer, VertexAttr)>>,
    index_buffer: Option<Buffer>,
}

ref_counted!(VertexArray wraps VertexArrayInner; drop(self) {
    self.inner.gl.delete_vertex_array(Some(&self.inner.raw));
});

impl VertexArray {
    pub(crate) fn new(gl: &WebGl2RenderingContext) -> Result<Self, String> {
        let raw = gl.create_vertex_array().ok_or("createVertexArray failed")?;
        Ok(Self {
            inner: VertexArrayInner {
                gl: gl.clone(),
                raw,
                attribs: Vec::new(),
                index_buffer: None,
            },
            rc: RefCount::new(),
        })
    }
}

#[wasm_bindgen]
impl VertexArray {
    pub fn attr(&mut self, index: u32, buffer: &Buffer, attr: &VertexAttr) {
        let gl = &self.inner.gl;
        gl.bind_vertex_array(Some(&self.inner.raw));
        gl.bind_buffer(WebGl2RenderingContext::ARRAY_BUFFER, Some(buffer.raw_gl()));

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

        let idx = index as usize;
        if idx >= self.inner.attribs.len() {
            self.inner.attribs.resize_with(idx + 1, || None);
        }
        self.inner.attribs[idx] = Some((buffer.clone(), attr.clone()));
    }

    pub fn remove_attr(&mut self, index: u32) {
        let gl = &self.inner.gl;
        gl.bind_vertex_array(Some(&self.inner.raw));
        gl.disable_vertex_attrib_array(index);
        gl.bind_vertex_array(None);

        let idx = index as usize;
        if idx < self.inner.attribs.len() {
            self.inner.attribs[idx] = None;
        }
    }

    pub fn set_index_buffer(&mut self, buffer: &Buffer) {
        let gl = &self.inner.gl;
        gl.bind_vertex_array(Some(&self.inner.raw));
        gl.bind_buffer(
            WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER,
            Some(buffer.raw_gl()),
        );
        gl.bind_vertex_array(None);

        self.inner.index_buffer = Some(buffer.clone());
    }

    pub fn remove_index_buffer(&mut self) {
        let gl = &self.inner.gl;
        gl.bind_vertex_array(Some(&self.inner.raw));
        gl.bind_buffer(WebGl2RenderingContext::ELEMENT_ARRAY_BUFFER, None);
        gl.bind_vertex_array(None);

        self.inner.index_buffer = None;
    }

    pub fn bind(&self) {
        self.inner.gl.bind_vertex_array(Some(&self.inner.raw));
    }

    pub fn unbind(&self) {
        self.inner.gl.bind_vertex_array(None);
    }
}
