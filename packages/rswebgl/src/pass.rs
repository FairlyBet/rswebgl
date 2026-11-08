// A render pass — the unit the context executes: a render target with its
// load/store ops, and a flat list of draws.
//
// Each draw carries everything it needs (program, render state, vertex array,
// parameter groups, command), so there is no ordering to get wrong: the
// renderer walks the list and diffs state between consecutive draws itself.
// Sorting the list by key (`Pass::sort`) groups draws that share state so that
// fewer switches reach GL. Everything is referenced by handle, so a built pass
// picks up later writes to its parameter groups; a resource destroyed
// meanwhile is reported and skipped.
//
// `Batch` is a convenience for building: a program, render state and default
// groups shared by several draws, flattened into the pass by `Pass::add`.

use wasm_bindgen::prelude::*;

use crate::console;
use crate::draw::{DrawCommand, DrawMode, Viewport};
use crate::feedback::PassFeedback;
use crate::framebuffer::{ClearMask, InvalidateMask};
use crate::ids::{
    FeedbackId, FramebufferId, ParamGroupId, ProgramId, QueryId, StateId, VertexArrayId,
};
use crate::param::GROUP_SLOTS;

fn slot_index(slot: u32) -> Option<usize> {
    let s = slot as usize;
    if s >= GROUP_SLOTS {
        console::error(&format!(
            "[rswebgl] group slot {slot} is out of range (0..{GROUP_SLOTS}); ignored"
        ));
        return None;
    }
    Some(s)
}

// ---------------------------------------------------------------------------
// DrawItem — one entry of a pass's list.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawItem {
    pub(crate) key: u64,
    // Insertion order: ties in `key` keep it, without a stable (allocating) sort.
    seq: u32,
    pub(crate) program: ProgramId,
    pub(crate) state: StateId,
    // NULL = no VAO bound (vertex-pulling shaders that use gl_VertexID etc.).
    pub(crate) vao: VertexArrayId,
    pub(crate) groups: [ParamGroupId; GROUP_SLOTS],
    pub(crate) command: DrawCommand,
    // NULL = none; else an occlusion query around this draw.
    pub(crate) query: QueryId,
}

/// Set on a sort key to draw after every draw with a default key — e.g.
/// transparent draws. `default_key` never sets it.
pub const KEY_LAST: u64 = 1 << 63;

/// The sort key `Pass::push` assigns: most expensive switch first — program,
/// then render state, then the slot-1 group (by convention the material), then
/// the vertex array. Equal keys keep their insertion order. Bit 63 is left
/// clear (see `KEY_LAST`).
#[wasm_bindgen(js_name = defaultKey)]
pub fn default_key(
    program: ProgramId,
    state: StateId,
    material: ParamGroupId,
    vao: VertexArrayId,
) -> u64 {
    let ix =
        |h: rswebgl_foundation::handle::Handle, bits: u32| h.index() as u64 & ((1 << bits) - 1);
    ix(program.0, 15) << 48 | ix(state.0, 12) << 36 | ix(material.0, 20) << 16 | ix(vao.0, 16)
}

// ---------------------------------------------------------------------------
// Batch — a builder for draws sharing a program, state and default groups.
// ---------------------------------------------------------------------------

struct BatchDraw {
    vao: VertexArrayId,
    // NULL = use the batch's group for that slot.
    groups: [ParamGroupId; GROUP_SLOTS],
    command: DrawCommand,
}

/// A program, render state and parameter groups shared by several draws.
/// `Pass::add` flattens it into the pass's draw list.
///
/// Parameters: each draw sees the batch's group in every slot, except where it
/// sets its own. A program input that none of a draw's groups provides keeps
/// whatever an earlier draw left — reported once (see
/// `Context::set_validate_uniforms`).
#[wasm_bindgen]
pub struct Batch {
    program: ProgramId,
    state: StateId,
    groups: [ParamGroupId; GROUP_SLOTS],
    draws: Vec<BatchDraw>,
}

#[wasm_bindgen]
impl Batch {
    /// `state` comes from `Context::render_state`.
    #[wasm_bindgen(constructor)]
    pub fn new(program: ProgramId, state: StateId) -> Self {
        Self {
            program,
            state,
            groups: [ParamGroupId::NULL; GROUP_SLOTS],
            draws: Vec::new(),
        }
    }

    /// The group every draw sees in `slot` unless it sets its own.
    #[wasm_bindgen(js_name = setGroup)]
    pub fn set_group(&mut self, slot: u32, group: ParamGroupId) {
        if let Some(s) = slot_index(slot) {
            self.groups[s] = group;
        }
    }

    /// Adds a draw that binds `vao` and uses the batch's groups.
    pub fn draw(&mut self, vao: VertexArrayId, command: &DrawCommand) {
        self.draw_with_groups(vao, [ParamGroupId::NULL; GROUP_SLOTS], command);
    }

    /// Adds a draw that uses `group` in `slot` (typically the object's own
    /// parameters) and the batch's groups elsewhere.
    #[wasm_bindgen(js_name = drawWithGroup)]
    pub fn draw_with_group(
        &mut self,
        vao: VertexArrayId,
        slot: u32,
        group: ParamGroupId,
        command: &DrawCommand,
    ) {
        let mut groups = [ParamGroupId::NULL; GROUP_SLOTS];
        if let Some(s) = slot_index(slot) {
            groups[s] = group;
        }
        self.draw_with_groups(vao, groups, command);
    }

    /// Adds a draw with no VAO — for vertex-pulling shaders that source vertices
    /// from `gl_VertexID` (or bound buffers) rather than vertex attributes.
    #[wasm_bindgen(js_name = drawVertexless)]
    pub fn draw_vertexless(&mut self, command: &DrawCommand) {
        self.draw(VertexArrayId::NULL, command);
    }

    #[wasm_bindgen(js_name = drawCount)]
    pub fn draw_count(&self) -> usize {
        self.draws.len()
    }
}

impl Batch {
    /// Adds a draw with its own group per slot (NULL = the batch's).
    pub fn draw_with_groups(
        &mut self,
        vao: VertexArrayId,
        groups: [ParamGroupId; GROUP_SLOTS],
        command: &DrawCommand,
    ) {
        self.draws.push(BatchDraw {
            vao,
            groups,
            command: *command,
        });
    }
}

// ---------------------------------------------------------------------------
// Pass
// ---------------------------------------------------------------------------

/// A render target with its load/store ops and a list of draws. Reusable:
/// `clear_draws` empties the list but keeps its memory, so a pass rebuilt every
/// frame doesn't allocate.
#[wasm_bindgen]
pub struct Pass {
    // NULL = the default (canvas) framebuffer.
    pub(crate) target: FramebufferId,
    // None = the target's full size.
    pub(crate) viewport: Option<Viewport>,
    pub(crate) clear: ClearMask,
    pub(crate) invalidate: InvalidateMask,
    // Some(NULL) = resolve into the canvas.
    pub(crate) resolve: Option<FramebufferId>,
    // NULL = none; else a TimeElapsed query around the whole pass.
    pub(crate) timer: QueryId,
    // Some = the draws record into a transform feedback object.
    pub(crate) feedback: Option<PassFeedback>,
    pub(crate) items: Vec<DrawItem>,
}

impl Pass {
    fn with_target(target: FramebufferId) -> Self {
        Self {
            target,
            viewport: None,
            clear: ClearMask::none(),
            invalidate: InvalidateMask::none(),
            resolve: None,
            timer: QueryId::NULL,
            feedback: None,
            items: Vec::new(),
        }
    }

    /// Appends a draw with its groups as an array (NULL = none in that slot)
    /// and the default sort key.
    pub fn push_groups(
        &mut self,
        program: ProgramId,
        state: StateId,
        vao: VertexArrayId,
        groups: [ParamGroupId; GROUP_SLOTS],
        command: &DrawCommand,
    ) {
        let key = default_key(program, state, groups[1], vao);
        self.push_groups_with_key(key, program, state, vao, groups, command);
    }

    /// `push_groups` with an explicit sort key.
    pub fn push_groups_with_key(
        &mut self,
        key: u64,
        program: ProgramId,
        state: StateId,
        vao: VertexArrayId,
        groups: [ParamGroupId; GROUP_SLOTS],
        command: &DrawCommand,
    ) {
        let seq = self.items.len() as u32;
        self.items.push(DrawItem {
            key,
            seq,
            program,
            state,
            vao,
            groups,
            command: *command,
            query: QueryId::NULL,
        });
    }
}

#[wasm_bindgen]
impl Pass {
    /// A pass that renders into `target`.
    #[wasm_bindgen(constructor)]
    pub fn new(target: FramebufferId) -> Self {
        Self::with_target(target)
    }

    /// A pass that renders into the default (canvas) framebuffer.
    #[wasm_bindgen(js_name = toDefault)]
    pub fn to_default() -> Self {
        Self::with_target(FramebufferId::NULL)
    }

    /// Points the pass at another target (keeping its draws and settings) —
    /// to reuse one pass, and its allocations, across frames whose target
    /// changes.
    #[wasm_bindgen(js_name = setTarget)]
    pub fn set_target(&mut self, target: FramebufferId) {
        self.target = target;
    }

    /// Restricts rendering to a sub-region; defaults to the target's full size.
    #[wasm_bindgen(js_name = setViewport)]
    pub fn set_viewport(&mut self, viewport: &Viewport) {
        self.viewport = Some(*viewport);
    }

    /// Buffers to clear before drawing (the load op), using the target's clear
    /// values. Defaults to clearing nothing.
    #[wasm_bindgen(js_name = setClear)]
    pub fn set_clear(&mut self, mask: &ClearMask) {
        self.clear = *mask;
    }

    /// Attachments to discard after drawing (the store op) via
    /// invalidateFramebuffer — lets tiled GPUs skip writing back data you won't
    /// read again (depth, MSAA color). Defaults to discarding nothing.
    #[wasm_bindgen(js_name = setInvalidate)]
    pub fn set_invalidate(&mut self, mask: &InvalidateMask) {
        self.invalidate = *mask;
    }

    /// After drawing, copy the target's color attachment 0 into `dst`
    /// (`FramebufferId::NULL` = the canvas) — the MSAA resolve: render into a
    /// multisampled renderbuffer, resolve into a texture or the canvas. Same
    /// size required; the copy happens before `set_invalidate` discards the
    /// multisampled data, so both together are the cheap path on tiled GPUs.
    #[wasm_bindgen(js_name = setResolve)]
    pub fn set_resolve(&mut self, dst: FramebufferId) {
        self.resolve = Some(dst);
    }

    #[wasm_bindgen(js_name = clearResolve)]
    pub fn clear_resolve(&mut self) {
        self.resolve = None;
    }

    /// Measures the GPU time of the whole pass (clear, draws, resolve) with
    /// `query`, a `TimeElapsed` query; read it with `Context::query_result`
    /// a frame or so later.
    #[wasm_bindgen(js_name = setTimer)]
    pub fn set_timer(&mut self, query: QueryId) {
        self.timer = query;
    }

    #[wasm_bindgen(js_name = clearTimer)]
    pub fn clear_timer(&mut self) {
        self.timer = QueryId::NULL;
    }

    /// Records the pass's draws into transform feedback object `tf`: their
    /// vertex shader outputs are appended to its buffers. `mode` is the
    /// primitive type (Points, Lines or Triangles) every draw must use;
    /// `discard` turns rasterization off, for a pass that only computes (the
    /// clear still happens). All draws must use one program created with
    /// `create_program_with_feedback`, and be non-indexed.
    #[wasm_bindgen(js_name = setTransformFeedback)]
    pub fn set_transform_feedback(&mut self, tf: FeedbackId, mode: DrawMode, discard: bool) {
        if !matches!(
            mode,
            DrawMode::Points | DrawMode::Lines | DrawMode::Triangles
        ) {
            return console::error(&format!(
                "[rswebgl] set_transform_feedback: {mode:?} can't be recorded; use Points, \n                 Lines or Triangles; ignored"
            ));
        }
        let query = self.feedback.map_or(QueryId::NULL, |f| f.query);
        self.feedback = Some(PassFeedback {
            feedback: tf,
            mode,
            discard,
            query,
        });
    }

    /// Counts the primitives the recording writes with `query`, a
    /// `TransformFeedbackPrimitivesWritten` query. Needs
    /// `set_transform_feedback` first.
    #[wasm_bindgen(js_name = setTransformFeedbackQuery)]
    pub fn set_transform_feedback_query(&mut self, query: QueryId) {
        match &mut self.feedback {
            Some(f) => f.query = query,
            None => console::error(
                "[rswebgl] set_transform_feedback_query: the pass doesn't record; ignored",
            ),
        }
    }

    #[wasm_bindgen(js_name = clearTransformFeedback)]
    pub fn clear_transform_feedback(&mut self) {
        self.feedback = None;
    }

    /// Wraps the most recently added draw in occlusion query `query`
    /// (`AnySamplesPassed*`): did any of its samples pass the depth test? The
    /// usual use is a cheap proxy draw (a bounding box, color writes off)
    /// that decides whether to draw the real object next frame. One draw per
    /// query: a second one would overwrite the first's result.
    #[wasm_bindgen(js_name = setLastDrawQuery)]
    pub fn set_last_draw_query(&mut self, query: QueryId) {
        match self.items.last_mut() {
            Some(d) => d.query = query,
            None => console::error("[rswebgl] set_last_draw_query: the pass has no draws; ignored"),
        }
    }

    /// Appends a batch's draws. The batch is left as is, so it can be added
    /// again (to this pass next frame, or to another).
    pub fn add(&mut self, batch: &Batch) {
        for d in &batch.draws {
            let mut groups = batch.groups;
            for (slot, g) in d.groups.iter().enumerate() {
                if !g.is_null() {
                    groups[slot] = *g;
                }
            }
            self.push_groups(batch.program, batch.state, d.vao, groups, &d.command);
        }
    }

    /// Appends one draw. Groups `g0..g3` go in slots 0..3 (NULL / 0 = none);
    /// by convention 0 = frame, 1 = material, 2 = object. Gets the default
    /// sort key (see `sort`).
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        program: ProgramId,
        state: StateId,
        vao: VertexArrayId,
        g0: ParamGroupId,
        g1: ParamGroupId,
        g2: ParamGroupId,
        g3: ParamGroupId,
        command: &DrawCommand,
    ) {
        self.push_groups(program, state, vao, [g0, g1, g2, g3], command);
    }

    /// `push` with an explicit sort key, given as its high and low 32 bits
    /// (JS numbers, no BigInt per draw): `key = key_hi << 32 | key_lo`. E.g. a
    /// layer in `key_hi` (`0x8000_0000` = `KEY_LAST`) and quantized depth in
    /// `key_lo` for back-to-front transparent draws.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = pushWithKey)]
    pub fn push_with_key(
        &mut self,
        key_hi: u32,
        key_lo: u32,
        program: ProgramId,
        state: StateId,
        vao: VertexArrayId,
        g0: ParamGroupId,
        g1: ParamGroupId,
        g2: ParamGroupId,
        g3: ParamGroupId,
        command: &DrawCommand,
    ) {
        let key = (key_hi as u64) << 32 | key_lo as u64;
        self.push_groups_with_key(key, program, state, vao, [g0, g1, g2, g3], command);
    }

    /// Orders the draws by sort key (equal keys keep insertion order). With
    /// the default keys this groups draws by program, then render state, then
    /// material, then vertex array — the fewest switches. Doesn't allocate.
    pub fn sort(&mut self) {
        self.items.sort_unstable_by_key(|d| (d.key, d.seq));
    }

    /// Empties the draw list, keeping its memory for reuse.
    #[wasm_bindgen(js_name = clearDraws)]
    pub fn clear_draws(&mut self) {
        self.items.clear();
    }

    #[wasm_bindgen(js_name = drawCount)]
    pub fn draw_count(&self) -> usize {
        self.items.len()
    }
}
