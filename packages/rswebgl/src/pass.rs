// A render pass — the declarative unit the Renderer executes.
//
// The shape mirrors the cost of switching GL state, coarsest first:
//
//   Pass    → one render target (framebuffer) + clear/invalidate + viewport
//     Batch → one program + render state
//       Draw → one VAO + uniforms + draw command
//
// The user builds this tree and hands it to `Renderer::render`. Because every
// draw carries everything it needs, there is no per-call ordering to get wrong:
// the Renderer walks the tree and diffs state between consecutive items itself.
// Each level *owns* clones of its GL objects (cheap ref-count bumps), so the tree
// is self-contained and can outlive the locals it was built from.

use wasm_bindgen::prelude::*;

use crate::draw::{DrawCommand, Viewport};
use crate::framebuffer::{ClearMask, Framebuffer, InvalidateMask};
use crate::program::Program;
use crate::render_state::RenderState;
use crate::uniform_values::UniformValues;
use crate::vao::VertexArray;

// ---------------------------------------------------------------------------
// Draw — innermost: geometry + bindings + the draw command.
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct Draw {
    // None = no VAO bound (vertex-pulling shaders that use gl_VertexID etc.).
    pub(crate) vao: Option<VertexArray>,
    pub(crate) uniforms: UniformValues,
    pub(crate) command: DrawCommand,
}

// ---------------------------------------------------------------------------
// Batch — a program + render state and the draws sharing them.
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct Batch {
    pub(crate) program: Program,
    pub(crate) render_state: RenderState,
    pub(crate) draws: Vec<Draw>,
}

#[wasm_bindgen]
impl Batch {
    #[wasm_bindgen(constructor)]
    pub fn new(program: &Program, render_state: &RenderState) -> Self {
        Self {
            program: program.clone(),
            render_state: render_state.clone(),
            draws: Vec::new(),
        }
    }

    /// Adds a draw that binds `vao` for its geometry.
    pub fn draw(&mut self, vao: &VertexArray, uniforms: &UniformValues, command: DrawCommand) {
        self.draws.push(Draw {
            vao: Some(vao.clone()),
            uniforms: uniforms.clone(),
            command,
        });
    }

    /// Adds a draw with no VAO — for vertex-pulling shaders that source vertices
    /// from `gl_VertexID` (or bound buffers) rather than vertex attributes.
    pub fn draw_vertexless(&mut self, uniforms: &UniformValues, command: DrawCommand) {
        self.draws.push(Draw {
            vao: None,
            uniforms: uniforms.clone(),
            command,
        });
    }

    pub fn draw_count(&self) -> usize {
        self.draws.len()
    }
}

// ---------------------------------------------------------------------------
// Pass — outermost: a render target with its load/store ops and batches.
// ---------------------------------------------------------------------------

#[wasm_bindgen]
pub struct Pass {
    // None = the default (canvas) framebuffer.
    pub(crate) target: Option<Framebuffer>,
    // None = the target's full size.
    pub(crate) viewport: Option<Viewport>,
    pub(crate) clear: ClearMask,
    pub(crate) invalidate: InvalidateMask,
    pub(crate) batches: Vec<Batch>,
}

impl Pass {
    fn with_target(target: Option<Framebuffer>) -> Self {
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
    pub fn new(target: &Framebuffer) -> Self {
        Self::with_target(Some(target.clone()))
    }

    /// A pass that renders into the default (canvas) framebuffer.
    pub fn to_default() -> Self {
        Self::with_target(None)
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
