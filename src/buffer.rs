use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlBuffer};

use crate::ref_count::{RefCount, ref_counted};

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BufferTarget {
    Array = 34962,             // ARRAY_BUFFER
    ElementArray = 34963,      // ELEMENT_ARRAY_BUFFER
    Uniform = 35345,           // UNIFORM_BUFFER
    TransformFeedback = 35982, // TRANSFORM_FEEDBACK_BUFFER
    CopyRead = 36662,          // COPY_READ_BUFFER
    CopyWrite = 36663,         // COPY_WRITE_BUFFER
    PixelPack = 35051,         // PIXEL_PACK_BUFFER
    PixelUnpack = 35052,       // PIXEL_UNPACK_BUFFER
}

impl BufferTarget {
    pub(crate) fn as_gl(&self) -> u32 {
        self.clone() as u32
    }
}

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BufferUsage {
    StaticDraw = 35044,  // STATIC_DRAW
    DynamicDraw = 35048, // DYNAMIC_DRAW
    StreamDraw = 35040,  // STREAM_DRAW
    StaticRead = 35045,  // STATIC_READ
    DynamicRead = 35049, // DYNAMIC_READ
    StreamRead = 35041,  // STREAM_READ
    StaticCopy = 35046,  // STATIC_COPY
    DynamicCopy = 35050, // DYNAMIC_COPY
    StreamCopy = 35042,  // STREAM_COPY
}

impl BufferUsage {
    pub(crate) fn as_gl(&self) -> u32 {
        self.clone() as u32
    }
}

#[derive(Debug, Clone)]
struct BufferInner {
    gl: WebGl2RenderingContext,
    raw: WebGlBuffer,
    size: u32,
}

ref_counted!(Buffer wraps BufferInner; drop(self) {
    self.inner.gl.delete_buffer(Some(&self.inner.raw));
});

impl Buffer {
    pub(crate) fn raw_gl(&self) -> &WebGlBuffer {
        &self.inner.raw
    }

    pub(crate) fn new(
        gl: &WebGl2RenderingContext,
        target: BufferTarget,
        usage: BufferUsage,
        data: &[u8],
    ) -> Result<Buffer, String> {
        let raw = gl.create_buffer().ok_or("createBuffer failed")?;
        gl.bind_buffer(target.as_gl(), Some(&raw));
        gl.buffer_data_with_u8_array(target.as_gl(), data, usage.as_gl());
        gl.bind_buffer(target.as_gl(), None);
        Ok(Self {
            inner: BufferInner {
                gl: gl.clone(),
                raw,
                size: data.len() as u32,
            },
            rc: RefCount::new(),
        })
    }

    pub(crate) fn new_empty(
        gl: &WebGl2RenderingContext,
        target: BufferTarget,
        usage: BufferUsage,
        size: u32,
    ) -> Result<Buffer, String> {
        let raw = gl.create_buffer().ok_or("createBuffer failed")?;
        gl.bind_buffer(target.as_gl(), Some(&raw));
        gl.buffer_data_with_i32(target.as_gl(), size as i32, usage.as_gl());
        gl.bind_buffer(target.as_gl(), None);
        Ok(Self {
            inner: BufferInner {
                gl: gl.clone(),
                raw,
                size,
            },
            rc: RefCount::new(),
        })
    }
}

#[wasm_bindgen]
impl Buffer {
    pub fn write(&self, target: BufferTarget, offset: i32, data: &[u8]) {
        self.inner.gl.bind_buffer(target.as_gl(), Some(&self.inner.raw));
        self.inner
            .gl
            .buffer_sub_data_with_i32_and_u8_array(target.as_gl(), offset, data);
        self.inner.gl.bind_buffer(target.as_gl(), None);
    }

    pub fn size(&self) -> u32 {
        self.inner.size
    }
}
