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

// WebGL2 pins exactly one bit of a buffer's identity at first bind: a buffer ever
// bound to ELEMENT_ARRAY_BUFFER can never be bound to any other target, and vice
// versa. Every *non*-element target (ARRAY, UNIFORM, COPY_*, …) is interchangeable,
// so this index-vs-generic bit is the only role we must pin at creation — the rest
// stays decided at the use site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BufferKind {
    /// Vertex / uniform / copy / … — anything that is not an index buffer.
    Generic,
    /// Index (ELEMENT_ARRAY) buffer.
    Index,
}

#[derive(Debug, Clone)]
struct BufferInner {
    gl: WebGl2RenderingContext,
    raw: WebGlBuffer,
    size: u32,
    kind: BufferKind,
}

ref_counted!(Buffer wraps BufferInner; drop(self) {
    self.inner.gl.delete_buffer(Some(&self.inner.raw));
});

// The scratch slot a data transfer binds to. Generic buffers use COPY_WRITE_BUFFER:
// no pipeline side effects, and it isn't VAO state, so an upload can't disturb the
// vertex-array bindings. Index buffers must use ELEMENT_ARRAY_BUFFER (per the rule
// above) — and that target *is* VAO state, so `bind_for_upload` detaches any bound
// VAO first so the transfer can't clobber a user VAO's index binding.
fn scratch_target(kind: BufferKind) -> u32 {
    match kind {
        BufferKind::Generic => BufferTarget::CopyWrite,
        BufferKind::Index => BufferTarget::ElementArray,
    }
    .as_gl()
}

// Binds `raw` to its scratch target for a transfer and returns that target.
fn bind_for_upload(gl: &WebGl2RenderingContext, kind: BufferKind, raw: &WebGlBuffer) -> u32 {
    if kind == BufferKind::Index {
        gl.bind_vertex_array(None);
    }
    let t = scratch_target(kind);
    gl.bind_buffer(t, Some(raw));
    t
}

impl Buffer {
    pub(crate) fn raw_gl(&self) -> &WebGlBuffer {
        &self.inner.raw
    }

    pub(crate) fn new(
        gl: &WebGl2RenderingContext,
        kind: BufferKind,
        usage: BufferUsage,
        data: &[u8],
    ) -> Result<Buffer, String> {
        let raw = gl.create_buffer().ok_or("createBuffer failed")?;
        let t = bind_for_upload(gl, kind, &raw);
        gl.buffer_data_with_u8_array(t, data, usage.as_gl());
        gl.bind_buffer(t, None);
        Ok(Self {
            inner: BufferInner {
                gl: gl.clone(),
                raw,
                size: data.len() as u32,
                kind,
            },
            rc: RefCount::new(),
        })
    }

    pub(crate) fn new_empty(
        gl: &WebGl2RenderingContext,
        kind: BufferKind,
        usage: BufferUsage,
        size: u32,
    ) -> Result<Buffer, String> {
        let raw = gl.create_buffer().ok_or("createBuffer failed")?;
        let t = bind_for_upload(gl, kind, &raw);
        gl.buffer_data_with_i32(t, size as i32, usage.as_gl());
        gl.bind_buffer(t, None);
        Ok(Self {
            inner: BufferInner {
                gl: gl.clone(),
                raw,
                size,
                kind,
            },
            rc: RefCount::new(),
        })
    }
}

#[wasm_bindgen]
impl Buffer {
    /// Overwrite a region of the buffer's data store (`bufferSubData`). Goes
    /// through the buffer's scratch binding, so it never disturbs vertex or other
    /// generic buffer bindings.
    pub fn write(&self, offset: i32, data: &[u8]) {
        let gl = &self.inner.gl;
        let t = bind_for_upload(gl, self.inner.kind, &self.inner.raw);
        gl.buffer_sub_data_with_i32_and_u8_array(t, offset, data);
        gl.bind_buffer(t, None);
    }

    pub fn size(&self) -> u32 {
        self.inner.size
    }
}
