// A render pass — the declarative unit the context executes.
//
// The shape mirrors the cost of switching GL state, coarsest first:
//
//   Pass    → one render target (framebuffer) + clear/invalidate + viewport
//     Batch → one program + render state + default parameter groups
//       Draw → one VAO + draw command (+ groups overriding the batch's)
//
// The user builds this tree and hands it to `Context::render`. Because every
// draw carries everything it needs, there is no per-call ordering to get wrong:
// the renderer walks the tree and diffs state between consecutive items itself.
// Everything is referenced by handle, so a built pass picks up later writes to
// its parameter groups; a resource destroyed meanwhile is reported and skipped.

use wasm_bindgen::prelude::*;

use crate::console;
use crate::draw::{DrawCommand, Viewport};
use crate::framebuffer::{ClearMask, InvalidateMask};
use crate::ids::{FramebufferId, ParamGroupId, ProgramId, VertexArrayId};
use crate::param::GROUP_SLOTS;
use crate::render_state::RenderState;

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
// Draw — innermost: geometry + the draw command.
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct Draw {
    // NULL = no VAO bound (vertex-pulling shaders that use gl_VertexID etc.).
    pub(crate) vao: VertexArrayId,
    // NULL = use the batch's group for that slot.
    pub(crate) groups: [ParamGroupId; GROUP_SLOTS],
    pub(crate) command: DrawCommand,
}

// ---------------------------------------------------------------------------
// Batch — a program + render state and the draws sharing them.
// ---------------------------------------------------------------------------

/// A program, render state and parameter groups shared by a list of draws.
///
/// Parameters: each draw sees the batch's group in every slot, except where it
/// sets its own. A program input that none of a draw's groups provides keeps
/// whatever an earlier draw left — reported once (see
/// `Context::set_validate_uniforms`).
#[wasm_bindgen]
pub struct Batch {
    pub(crate) program: ProgramId,
    pub(crate) render_state: RenderState,
    pub(crate) groups: [ParamGroupId; GROUP_SLOTS],
    pub(crate) draws: Vec<Draw>,
}

#[wasm_bindgen]
impl Batch {
    #[wasm_bindgen(constructor)]
    pub fn new(program: ProgramId, render_state: &RenderState) -> Self {
        Self {
            program,
            render_state: render_state.clone(),
            groups: [ParamGroupId::NULL; GROUP_SLOTS],
            draws: Vec::new(),
        }
    }

    /// The group every draw sees in `slot` unless it sets its own.
    pub fn set_group(&mut self, slot: u32, group: ParamGroupId) {
        if let Some(s) = slot_index(slot) {
            self.groups[s] = group;
        }
    }

    /// Adds a draw that binds `vao` and uses the batch's groups.
    pub fn draw(&mut self, vao: VertexArrayId, command: DrawCommand) {
        self.draws.push(Draw {
            vao,
            groups: [ParamGroupId::NULL; GROUP_SLOTS],
            command,
        });
    }

    /// Adds a draw that uses `group` in `slot` (typically the object's own
    /// parameters) and the batch's groups elsewhere.
    pub fn draw_with_group(
        &mut self,
        vao: VertexArrayId,
        slot: u32,
        group: ParamGroupId,
        command: DrawCommand,
    ) {
        let mut groups = [ParamGroupId::NULL; GROUP_SLOTS];
        if let Some(s) = slot_index(slot) {
            groups[s] = group;
        }
        self.draws.push(Draw {
            vao,
            groups,
            command,
        });
    }

    /// Adds a draw with no VAO — for vertex-pulling shaders that source vertices
    /// from `gl_VertexID` (or bound buffers) rather than vertex attributes.
    pub fn draw_vertexless(&mut self, command: DrawCommand) {
        self.draw(VertexArrayId::NULL, command);
    }

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
        command: DrawCommand,
    ) {
        self.draws.push(Draw {
            vao,
            groups,
            command,
        });
    }
}

// ---------------------------------------------------------------------------
// Pass — outermost: a render target with its load/store ops and batches.
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct Pass {
    // NULL = the default (canvas) framebuffer.
    pub(crate) target: FramebufferId,
    // None = the target's full size.
    pub(crate) viewport: Option<Viewport>,
    pub(crate) clear: ClearMask,
    pub(crate) invalidate: InvalidateMask,
    pub(crate) batches: Vec<Batch>,
}

impl Pass {
    fn with_target(target: FramebufferId) -> Self {
        Self {
            target,
            viewport: None,
            clear: ClearMask::none(),
            invalidate: InvalidateMask::none(),
            batches: Vec::new(),
        }
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
    pub fn to_default() -> Self {
        Self::with_target(FramebufferId::NULL)
    }

    /// Restricts rendering to a sub-region; defaults to the target's full size.
    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.viewport = Some(viewport);
    }

    /// Buffers to clear before drawing (the load op), using the target's clear
    /// values. Defaults to clearing nothing.
    pub fn set_clear(&mut self, mask: ClearMask) {
        self.clear = mask;
    }

    /// Attachments to discard after drawing (the store op) via
    /// invalidateFramebuffer — lets tiled GPUs skip writing back data you won't
    /// read again (depth, MSAA color). Defaults to discarding nothing.
    pub fn set_invalidate(&mut self, mask: InvalidateMask) {
        self.invalidate = mask;
    }

    pub fn add(&mut self, batch: Batch) {
        self.batches.push(batch);
    }

    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }
}
