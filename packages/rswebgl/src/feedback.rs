// Transform feedback: vertex shader outputs written into buffers, for GPU
// particles, skinning baked once, GPGPU over vertex data.
//
// A program declares what it captures when it is created
// (`create_program_with_feedback`); a transform feedback object says which
// buffers receive it (`transform_feedback_set_buffer`); a pass records into
// that object (`Pass::set_transform_feedback`), usually with rasterization
// off. The renderer begins recording before the pass's first draw and ends it
// after the last, so successive draws append. Next frame the buffer is a
// vertex source like any other — ping-pong between two for a simulation.
//
// WebGL2 is strict here, and each rule is checked per draw with a reason
// instead of a GL error: the draw mode must equal the recording's primitive
// mode (Points / Lines / Triangles — no strips), draws must be non-indexed,
// one program per recording, every buffer the program writes must be bound,
// and nothing may be written past a buffer's end. A draw that breaks one is
// reported once and skipped.

use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext as Gl, WebGlProgram, WebGlTransformFeedback};

use rswebgl_foundation::pool::{Destroyed, Pool};

use crate::buffer::{BufferKind, BufferObj};
use crate::console;
use crate::context::{Context, Resources, stale};
use crate::draw::DrawMode;
use crate::error::Error;
use crate::ids::{BufferId, FeedbackId, ProgramId, QueryId};
use crate::pass::DrawItem;
use crate::program::ProgramObj;
use crate::query::{self, QueryObj, QueryUse};

/// The minimum `MAX_TRANSFORM_FEEDBACK_SEPARATE_ATTRIBS` every WebGL2
/// implementation has.
pub const MAX_FEEDBACK_BUFFERS: u32 = 4;

/// How captured varyings are laid out.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedbackMode {
    /// All varyings into buffer 0, vertex after vertex.
    Interleaved = 0x8C8C,
    /// Varying `i` into buffer `i` (at most `MAX_FEEDBACK_BUFFERS`).
    Separate = 0x8C8D,
}

/// What a linked feedback program writes: bytes per vertex into each buffer.
#[derive(Clone, Debug)]
pub(crate) struct FeedbackLayout {
    pub(crate) strides: Vec<u32>,
}

// Bytes of one captured varying of GL type `ty`.
fn varying_bytes(ty: u32) -> u32 {
    match ty {
        Gl::FLOAT | Gl::INT | Gl::UNSIGNED_INT => 4,
        Gl::FLOAT_VEC2 | Gl::INT_VEC2 | Gl::UNSIGNED_INT_VEC2 => 8,
        Gl::FLOAT_VEC3 | Gl::INT_VEC3 | Gl::UNSIGNED_INT_VEC3 => 12,
        Gl::FLOAT_VEC4 | Gl::INT_VEC4 | Gl::UNSIGNED_INT_VEC4 | Gl::FLOAT_MAT2 => 16,
        Gl::FLOAT_MAT2X3 | Gl::FLOAT_MAT3X2 => 24,
        Gl::FLOAT_MAT2X4 | Gl::FLOAT_MAT4X2 => 32,
        Gl::FLOAT_MAT3 => 36,
        Gl::FLOAT_MAT3X4 | Gl::FLOAT_MAT4X3 => 48,
        Gl::FLOAT_MAT4 => 64,
        _ => 0,
    }
}

/// Reads the captured varyings of a linked program.
pub(crate) fn feedback_layout(gl: &Gl, raw: &WebGlProgram, mode: FeedbackMode) -> FeedbackLayout {
    let count = gl
        .get_program_parameter(raw, Gl::TRANSFORM_FEEDBACK_VARYINGS)
        .as_f64()
        .unwrap_or(0.0) as u32;
    let sizes = (0..count).map(|i| {
        gl.get_transform_feedback_varying(raw, i)
            .map_or(0, |v| varying_bytes(v.type_()) * v.size() as u32)
    });
    let strides = match mode {
        FeedbackMode::Interleaved => vec![sizes.sum()],
        FeedbackMode::Separate => sizes.collect(),
    };
    FeedbackLayout { strides }
}

#[derive(Clone, Copy)]
struct Binding {
    buffer: BufferId,
    offset: u32,
    size: u32,
}

pub(crate) struct FeedbackObj {
    raw: WebGlTransformFeedback,
    bindings: [Option<Binding>; MAX_FEEDBACK_BUFFERS as usize],
}

impl FeedbackObj {
    // Deletes the GL object and lets go of its buffers.
    fn delete(self, gl: &Gl, buffers: &mut Pool<BufferObj>) {
        for b in self.bindings.into_iter().flatten() {
            if let Some(obj) = buffers.release(b.buffer.0) {
                obj.delete(gl);
            }
        }
        gl.delete_transform_feedback(Some(&self.raw));
    }
}

// ---------------------------------------------------------------------------
// Recording — one pass's transform feedback, driven by the renderer.
// ---------------------------------------------------------------------------

/// The recording a pass asked for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PassFeedback {
    pub(crate) feedback: FeedbackId,
    pub(crate) mode: DrawMode,
    pub(crate) discard: bool,
    // NULL = none; else a TransformFeedbackPrimitivesWritten query.
    pub(crate) query: QueryId,
}

pub(crate) struct Recording<'a> {
    obj: &'a FeedbackObj,
    want: PassFeedback,
    program: Option<ProgramId>,
    // Bytes written into each binding so far.
    written: [u64; MAX_FEEDBACK_BUFFERS as usize],
    active: bool,
    query_target: Option<u32>,
}

impl<'a> Recording<'a> {
    /// None (reported) if the pass's feedback object is stale.
    pub(crate) fn new(feedbacks: &'a Pool<FeedbackObj>, want: PassFeedback) -> Option<Self> {
        let Some(obj) = feedbacks.get(want.feedback.0) else {
            stale("transform feedback", "render (pass)", want.feedback);
            return None;
        };
        Some(Self {
            obj,
            want,
            program: None,
            written: [0; MAX_FEEDBACK_BUFFERS as usize],
            active: false,
            query_target: None,
        })
    }

    /// Whether draw `d` with `program` can be recorded; if not, says why
    /// (once) and the caller skips it. Accounts for what it will write.
    pub(crate) fn admit(&mut self, d: &DrawItem, program: &ProgramObj) -> bool {
        match self.check(d, program) {
            Ok(()) => true,
            Err(why) => {
                console::error_once(&format!(
                    "[rswebgl] render (transform feedback): {why}; draw skipped"
                ));
                false
            }
        }
    }

    fn check(&mut self, d: &DrawItem, program: &ProgramObj) -> Result<(), String> {
        let mode = self.want.mode;
        if d.command.mode != mode {
            return Err(format!(
                "draw mode {:?} isn't the recording's {mode:?}",
                d.command.mode
            ));
        }
        if !d.command.is_arrays() {
            return Err("indexed draws can't be recorded in WebGL2".into());
        }
        let Some(layout) = &program.feedback else {
            return Err("the program captures no varyings \
                        (create it with create_program_with_feedback)"
                .into());
        };
        if self.program.is_some_and(|p| p != d.program) {
            return Err("one program per recording".into());
        }
        let per_prim = match mode {
            DrawMode::Points => 1,
            DrawMode::Lines => 2,
            _ => 3,
        };
        let vertices = (d.command.count.max(0) as u64 / per_prim * per_prim)
            * d.command.instance_count.max(1) as u64;
        let mut after = self.written;
        for (i, &stride) in layout.strides.iter().enumerate() {
            let Some(b) = self.obj.bindings.get(i).copied().flatten() else {
                return Err(format!(
                    "the program writes buffer {i}, but none is bound there"
                ));
            };
            after[i] += vertices * stride as u64;
            if after[i] > b.size as u64 {
                return Err(format!(
                    "buffer {i} would get {} bytes but holds {}",
                    after[i], b.size
                ));
            }
        }
        self.written = after;
        self.program = Some(d.program);
        Ok(())
    }

    /// Starts recording if not yet started; call after the admitted draw's
    /// program is in use.
    pub(crate) fn start(&mut self, gl: &Gl, queries: &mut Pool<QueryObj>) {
        if self.active {
            return;
        }
        self.active = true;
        gl.bind_transform_feedback(Gl::TRANSFORM_FEEDBACK, Some(&self.obj.raw));
        if self.want.discard {
            gl.enable(Gl::RASTERIZER_DISCARD);
        }
        if !self.want.query.is_null() {
            self.query_target = query::begin(gl, queries, self.want.query, QueryUse::Feedback);
        }
        gl.begin_transform_feedback(self.want.mode as u32);
    }

    /// Ends recording and puts the global state back.
    pub(crate) fn finish(self, gl: &Gl) {
        if !self.active {
            return;
        }
        gl.end_transform_feedback();
        if let Some(target) = self.query_target {
            gl.end_query(target);
        }
        if self.want.discard {
            gl.disable(Gl::RASTERIZER_DISCARD);
        }
        // Bound, the object keeps its buffers off every other binding point.
        gl.bind_transform_feedback(Gl::TRANSFORM_FEEDBACK, None);
    }
}

// ---------------------------------------------------------------------------
// API
// ---------------------------------------------------------------------------

#[wasm_bindgen]
impl Context {
    /// Creates a transform feedback object: the set of buffers a recording
    /// writes into.
    #[wasm_bindgen(js_name = createTransformFeedback)]
    pub fn create_transform_feedback(&self) -> Result<FeedbackId, Error> {
        self.check_alive()?;
        let raw = self
            .gl()
            .create_transform_feedback()
            .ok_or("createTransformFeedback failed")?;
        self.res()
            .feedbacks
            .insert(FeedbackObj {
                raw,
                bindings: [None; MAX_FEEDBACK_BUFFERS as usize],
            })
            .map(Into::into)
            .map_err(|e| Error::from(e.to_string()))
    }

    /// Deletes the object; its buffers are let go of (and deleted if they
    /// were destroyed meanwhile).
    #[wasm_bindgen(js_name = destroyTransformFeedback)]
    pub fn destroy_transform_feedback(&self, tf: FeedbackId) {
        let res = &mut *self.res();
        match res.feedbacks.destroy(tf.0) {
            Destroyed::Now(obj) => obj.delete(self.gl(), &mut res.buffers),
            Destroyed::Deferred => {}
            Destroyed::Invalid => stale("transform feedback", "destroy_transform_feedback", tf),
        }
    }

    #[wasm_bindgen(js_name = transformFeedbackIsValid)]
    pub fn transform_feedback_is_valid(&self, tf: FeedbackId) -> bool {
        self.res().feedbacks.get(tf.0).is_some()
    }

    /// Captured output `index` goes into all of `buffer` (NULL unbinds). The
    /// object keeps the buffer alive until it is unbound or the object is
    /// destroyed.
    #[wasm_bindgen(js_name = transformFeedbackSetBuffer)]
    pub fn transform_feedback_set_buffer(&self, tf: FeedbackId, index: u32, buffer: BufferId) {
        self.set_feedback_binding(tf, index, buffer, None);
    }

    /// Captured output `index` goes into `size` bytes of `buffer` from
    /// `offset` (a multiple of 4).
    #[wasm_bindgen(js_name = transformFeedbackSetBufferRange)]
    pub fn transform_feedback_set_buffer_range(
        &self,
        tf: FeedbackId,
        index: u32,
        buffer: BufferId,
        offset: u32,
        size: u32,
    ) {
        self.set_feedback_binding(tf, index, buffer, Some((offset, size)));
    }
}

impl Context {
    fn set_feedback_binding(
        &self,
        tf: FeedbackId,
        index: u32,
        buffer: BufferId,
        range: Option<(u32, u32)>,
    ) {
        const OP: &str = "transform_feedback_set_buffer";
        let gl = self.gl();
        let res = &mut *self.res();
        let Resources {
            feedbacks, buffers, ..
        } = res;
        let Some(obj) = feedbacks.get_mut(tf.0) else {
            return stale("transform feedback", OP, tf);
        };
        if index >= MAX_FEEDBACK_BUFFERS {
            return console::error(&format!(
                "[rswebgl] {OP}: index {index} is out of range (0..{MAX_FEEDBACK_BUFFERS}); ignored"
            ));
        }
        let binding = if buffer.is_null() {
            None
        } else {
            let Some(b) = buffers.get(buffer.0) else {
                return stale("buffer", OP, buffer);
            };
            let (offset, size) = range.unwrap_or((0, b.size));
            let problem = if b.kind == BufferKind::Index {
                Some("index buffers can't receive transform feedback".to_string())
            } else if offset % 4 != 0 {
                Some(format!("offset {offset} isn't a multiple of 4"))
            } else if size == 0 || offset as u64 + size as u64 > b.size as u64 {
                Some(format!(
                    "range {offset}+{size} doesn't fit the buffer ({} bytes)",
                    b.size
                ))
            } else {
                None
            };
            if let Some(p) = problem {
                return console::error(&format!("[rswebgl] {OP}: {p}; ignored"));
            }
            Some((
                b.raw.clone(),
                Binding {
                    buffer,
                    offset,
                    size,
                },
            ))
        };

        // GL first: the object stores the binding itself.
        gl.bind_transform_feedback(Gl::TRANSFORM_FEEDBACK, Some(&obj.raw));
        match &binding {
            Some((raw, b)) => gl.bind_buffer_range_with_i32_and_i32(
                Gl::TRANSFORM_FEEDBACK_BUFFER,
                index,
                Some(raw),
                b.offset as i32,
                b.size as i32,
            ),
            None => gl.bind_buffer_base(Gl::TRANSFORM_FEEDBACK_BUFFER, index, None),
        }
        gl.bind_buffer(Gl::TRANSFORM_FEEDBACK_BUFFER, None);
        gl.bind_transform_feedback(Gl::TRANSFORM_FEEDBACK, None);

        let new = binding.map(|(_, b)| b);
        if let Some(b) = new {
            buffers.retain(b.buffer.0);
        }
        if let Some(old) = std::mem::replace(&mut obj.bindings[index as usize], new)
            && let Some(dead) = buffers.release(old.buffer.0)
        {
            dead.delete(gl);
        }
    }
}
