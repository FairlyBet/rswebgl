use rswebgl_foundation::pool::Destroyed;
use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlBuffer};

use crate::context::{Context, stale};
use crate::ids::BufferId;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        *self as u32
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

/// A buffer in the context's pool.
#[derive(Debug)]
pub(crate) struct BufferObj {
    pub(crate) raw: WebGlBuffer,
    pub(crate) size: u32,
    pub(crate) kind: BufferKind,
}

impl BufferObj {
    pub(crate) fn delete(self, gl: &WebGl2RenderingContext) {
        gl.delete_buffer(Some(&self.raw));
    }
}

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

/// Initial contents of a new buffer.
pub(crate) enum Init<'a> {
    Data(&'a [u8]),
    Size(u32),
}

/// Creates a GL buffer of `kind` with `init` contents (not pooled).
pub(crate) fn create_raw(
    gl: &WebGl2RenderingContext,
    kind: BufferKind,
    usage: BufferUsage,
    init: Init<'_>,
) -> Result<BufferObj, String> {
    let raw = gl.create_buffer().ok_or("createBuffer failed")?;
    let t = bind_for_upload(gl, kind, &raw);
    let size = match init {
        Init::Data(data) => {
            gl.buffer_data_with_u8_array(t, data, usage.as_gl());
            data.len() as u32
        }
        Init::Size(size) => {
            gl.buffer_data_with_i32(t, size as i32, usage.as_gl());
            size
        }
    };
    gl.bind_buffer(t, None);
    Ok(BufferObj { raw, size, kind })
}

/// `bufferSubData` through the buffer's scratch binding, so it never disturbs
/// vertex or other generic buffer bindings.
pub(crate) fn write_raw(
    gl: &WebGl2RenderingContext,
    kind: BufferKind,
    raw: &WebGlBuffer,
    offset: i32,
    data: &[u8],
) {
    let t = bind_for_upload(gl, kind, raw);
    gl.buffer_sub_data_with_i32_and_u8_array(t, offset, data);
    gl.bind_buffer(t, None);
}

impl Context {
    pub(crate) fn insert_buffer(&self, obj: BufferObj) -> Result<BufferId, String> {
        let mut res = self.res();
        match res.buffers.insert(obj) {
            Ok(h) => Ok(h.into()),
            Err(e) => {
                drop(res);
                Err(e.to_string())
            }
        }
    }
}

#[wasm_bindgen]
impl Context {
    /// A vertex / uniform / generic buffer filled with `data`.
    pub fn create_buffer(&self, usage: BufferUsage, data: &[u8]) -> Result<BufferId, String> {
        self.check_alive()?;
        let obj = create_raw(self.gl(), BufferKind::Generic, usage, Init::Data(data))?;
        self.insert_buffer(obj)
    }

    /// A vertex / uniform / generic buffer of `size` zeroed bytes.
    pub fn create_empty_buffer(&self, usage: BufferUsage, size: u32) -> Result<BufferId, String> {
        self.check_alive()?;
        let obj = create_raw(self.gl(), BufferKind::Generic, usage, Init::Size(size))?;
        self.insert_buffer(obj)
    }

    /// An index buffer filled with `data` (raw bytes of u8/u16/u32 indices).
    /// WebGL never lets an index buffer be bound as anything else, so it's a
    /// separate kind.
    pub fn create_index_buffer(&self, usage: BufferUsage, data: &[u8]) -> Result<BufferId, String> {
        self.check_alive()?;
        // The upload unbinds any VAO (see `bind_for_upload`).
        self.gl_state().forget_vao();
        let obj = create_raw(self.gl(), BufferKind::Index, usage, Init::Data(data))?;
        self.insert_buffer(obj)
    }

    /// Overwrite a region of the buffer's data store (`bufferSubData`).
    pub fn buffer_write(&self, buffer: BufferId, offset: i32, data: &[u8]) {
        let res = self.res();
        let Some(b) = res.buffers.get(buffer.0) else {
            return stale("buffer", "buffer_write", buffer);
        };
        if b.kind == BufferKind::Index {
            self.gl_state().forget_vao();
        }
        write_raw(self.gl(), b.kind, &b.raw, offset, data);
    }

    /// Size in bytes; 0 for a stale handle.
    pub fn buffer_size(&self, buffer: BufferId) -> u32 {
        self.res().buffers.get(buffer.0).map_or(0, |b| b.size)
    }

    /// Deletes the buffer. If vertex arrays still use it, its handle is invalid
    /// right away and the GL buffer goes when the last of them is destroyed.
    pub fn destroy_buffer(&self, buffer: BufferId) {
        let destroyed = self.res().buffers.destroy(buffer.0);
        match destroyed {
            Destroyed::Now(obj) => obj.delete(self.gl()),
            Destroyed::Deferred => {}
            Destroyed::Invalid => stale("buffer", "destroy_buffer", buffer),
        }
    }
}
