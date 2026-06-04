use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlProgram, WebGlShader, WebGlUniformLocation};

use crate::console;
use crate::ref_count::{RefCount, ref_counted};
use crate::uniform_cache::UniformCache;

const COMPLETION_STATUS_KHR: u32 = 0x91B1;
// getUniformBlockIndex returns this for a name that isn't an active uniform block.
const INVALID_INDEX: u32 = 0xFFFF_FFFF;

// Cached per-program state for one uniform block: its resolved index and the
// binding point it's currently linked to (via uniformBlockBinding).
#[derive(Debug)]
struct BlockBinding {
    index: u32,
    point: Option<u32>,
}

// Everything that changes after construction. A Program is a clone-able handle
// (like Buffer/Texture/VAO), so all mutable state lives here behind one shared
// `Rc<RefCell>` — otherwise clones would fork it and drift: one clone finalizing
// (or caching a uniform location) would leave the others stale. Sharing it also
// lets the mutating methods take `&self`, so a Program held inside a render
// `Batch` needs no `&mut`.
#[derive(Debug)]
struct ProgramState {
    vert: Option<WebGlShader>,
    frag: Option<WebGlShader>,
    ready: bool,
    valid: bool,
    cache: UniformCache,
    // Resolved/linked uniform blocks, keyed by GLSL block name.
    blocks: HashMap<String, BlockBinding>,
}

#[derive(Debug, Clone)]
struct ProgramInner {
    gl: WebGl2RenderingContext,
    raw: WebGlProgram,
    // Immutable after construction — safe to copy per clone.
    parallel: bool,
    // Whether to check uniform-block layouts against the shader on first bind.
    validate_blocks: bool,
    state: Rc<RefCell<ProgramState>>,
}

ref_counted!(Program wraps ProgramInner; drop(self) {
    {
        let mut st = self.inner.state.borrow_mut();
        self.inner.gl.delete_shader(st.vert.take().as_ref());
        self.inner.gl.delete_shader(st.frag.take().as_ref());
    }
    self.inner.gl.delete_program(Some(&self.inner.raw));
});

impl Program {
    pub(crate) fn new(
        gl: &WebGl2RenderingContext,
        vert_src: &str,
        frag_src: &str,
        parallel: bool,
        validate_blocks: bool,
    ) -> Result<Program, String> {
        let vert = create_shader(gl, WebGl2RenderingContext::VERTEX_SHADER, vert_src)?;
        let frag = create_shader(gl, WebGl2RenderingContext::FRAGMENT_SHADER, frag_src)?;

        let raw = gl.create_program().ok_or("createProgram failed")?;
        gl.attach_shader(&raw, &vert);
        gl.attach_shader(&raw, &frag);
        gl.link_program(&raw);

        Ok(Program {
            inner: ProgramInner {
                gl: gl.clone(),
                raw,
                parallel,
                validate_blocks,
                state: Rc::new(RefCell::new(ProgramState {
                    vert: Some(vert),
                    frag: Some(frag),
                    ready: false,
                    valid: false,
                    cache: UniformCache::new(),
                    blocks: HashMap::new(),
                })),
            },
            rc: RefCount::new(),
        })
    }

    // Links the uniform block named `name` to binding point `point` for this
    // program. The first time a block is seen it's resolved with
    // getUniformBlockIndex and (if validation is on) its shader-reported size is
    // checked against `expected_size` (our computed std140 size). Both the index
    // and the last-linked point are cached, so the steady-state per-draw cost is
    // at most one uniformBlockBinding — and zero once the point stops changing.
    pub(crate) fn bind_block(&self, name: &str, point: u32, expected_size: u32) {
        let gl = &self.inner.gl;
        let raw = &self.inner.raw;
        let mut st = self.inner.state.borrow_mut();

        if !st.blocks.contains_key(name) {
            let index = gl.get_uniform_block_index(raw, name);
            if index == INVALID_INDEX {
                console::warn(&format!("[rswebgl] uniform block not found: \"{name}\""));
            } else if self.inner.validate_blocks {
                validate_block_size(gl, raw, index, name, expected_size);
            }
            st.blocks
                .insert(name.to_string(), BlockBinding { index, point: None });
        }

        let binding = st.blocks.get_mut(name).expect("inserted above");
        if binding.index == INVALID_INDEX {
            return;
        }
        if binding.point != Some(point) {
            gl.uniform_block_binding(raw, binding.index, point);
            binding.point = Some(point);
        }
    }

    pub(crate) fn gl(&self) -> &WebGl2RenderingContext {
        &self.inner.gl
    }

    pub(crate) fn raw_gl(&self) -> &WebGlProgram {
        &self.inner.raw
    }

    pub(crate) fn loc(&self, name: &str) -> Option<WebGlUniformLocation> {
        self.inner
            .state
            .borrow_mut()
            .cache
            .get(&self.inner.gl, &self.inner.raw, name)
            .cloned()
    }

    fn finalize(&self) {
        let gl = &self.inner.gl;
        let mut st = self.inner.state.borrow_mut();
        st.ready = true;
        st.valid = gl
            .get_program_parameter(&self.inner.raw, WebGl2RenderingContext::LINK_STATUS)
            .as_bool()
            .unwrap_or(false);

        if !st.valid {
            let prog_log = gl
                .get_program_info_log(&self.inner.raw)
                .unwrap_or_else(|| "unknown error".into());
            let vert_log = st
                .vert
                .as_ref()
                .and_then(|s| gl.get_shader_info_log(s))
                .unwrap_or_default();
            let frag_log = st
                .frag
                .as_ref()
                .and_then(|s| gl.get_shader_info_log(s))
                .unwrap_or_default();

            console::error(&format!("[rswebgl] program link failed: {prog_log}"));
            if !vert_log.is_empty() {
                console::error(&format!("[rswebgl] vertex shader: {vert_log}"));
            }
            if !frag_log.is_empty() {
                console::error(&format!("[rswebgl] fragment shader: {frag_log}"));
            }
        }

        gl.delete_shader(st.vert.take().as_ref());
        gl.delete_shader(st.frag.take().as_ref());
    }
}

#[wasm_bindgen]
impl Program {
    pub fn is_ready(&self) -> bool {
        if self.inner.state.borrow().ready {
            return true;
        }

        let complete = if self.inner.parallel {
            self.inner
                .gl
                .get_program_parameter(&self.inner.raw, COMPLETION_STATUS_KHR)
                .as_bool()
                .unwrap_or(false)
        } else {
            true
        };

        if complete {
            self.finalize();
        }

        self.inner.state.borrow().ready
    }

    pub fn is_valid(&self) -> bool {
        self.inner.state.borrow().valid
    }
}

impl PartialEq for Program {
    fn eq(&self, other: &Self) -> bool {
        self.inner.raw == other.inner.raw
    }
}

impl Eq for Program {}

// Compares the shader's reported block size (UNIFORM_BLOCK_DATA_SIZE) against our
// computed std140 size. A mismatch means the layout drifted from the GLSL block
// (wrong field order/types/counts) — the kind of silent corruption that's near-
// impossible to debug at draw time, so we surface it loudly here, once.
fn validate_block_size(
    gl: &WebGl2RenderingContext,
    program: &WebGlProgram,
    index: u32,
    name: &str,
    expected: u32,
) {
    let driver = gl
        .get_active_uniform_block_parameter(
            program,
            index,
            WebGl2RenderingContext::UNIFORM_BLOCK_DATA_SIZE,
        )
        .ok()
        .and_then(|v| v.as_f64())
        .map(|n| n as u32);
    if let Some(size) = driver
        && size != expected
    {
        console::error(&format!(
            "[rswebgl] uniform block \"{name}\" layout mismatch: shader needs {size} bytes, \
             layout computes {expected}. Check field order/types/counts against the GLSL block."
        ));
    }
}

fn create_shader(
    gl: &WebGl2RenderingContext,
    shader_type: u32,
    src: &str,
) -> Result<WebGlShader, String> {
    let shader = gl.create_shader(shader_type).ok_or("createShader failed")?;
    gl.shader_source(&shader, src);
    gl.compile_shader(&shader);
    Ok(shader)
}
