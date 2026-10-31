use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlProgram, WebGlShader, WebGlUniformLocation};

use crate::console;
use crate::context_loss;
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

// Everything that changes after construction. It lives in the shared inner, so
// all clones of a Program see one finalize / one uniform-location cache, and the
// mutating methods take `&self` (a Program held inside a render `Batch` needs no
// `&mut`).
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

#[derive(Debug)]
struct ProgramInner {
    gl: WebGl2RenderingContext,
    raw: WebGlProgram,
    // Context generation it was created in (see context_loss).
    generation: u32,
    // Whether to check uniform-block layouts against the shader on first bind.
    validate_blocks: bool,
    state: RefCell<ProgramState>,
}

impl Drop for ProgramInner {
    fn drop(&mut self) {
        if !context_loss::is_current(&self.gl, self.generation) {
            return;
        }
        let st = self.state.get_mut();
        self.gl.delete_shader(st.vert.take().as_ref());
        self.gl.delete_shader(st.frag.take().as_ref());
        self.gl.delete_program(Some(&self.raw));
    }
}

/// A shared handle to a linked GL program: clones point at the same program,
/// which is deleted when the last handle drops.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct Program {
    inner: Rc<ProgramInner>,
}

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

        let program = Program {
            inner: Rc::new(ProgramInner {
                gl: gl.clone(),
                raw,
                generation: context_loss::generation(gl),
                validate_blocks,
                state: RefCell::new(ProgramState {
                    vert: Some(vert),
                    frag: Some(frag),
                    ready: false,
                    valid: false,
                    cache: UniformCache::new(),
                    blocks: HashMap::new(),
                }),
            }),
        };

        // Without parallel compile the first status query blocks on the link
        // anyway, so do it now and hand compile/link errors back to the caller.
        // With it, the link finishes in the background; `is_ready` polls it
        // without blocking, and the renderer waits on it at first draw.
        if !parallel {
            program.finalize()?;
        }
        Ok(program)
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

    // Waits for the link if it's still running (blocking), reporting a failure
    // once. Returns whether the program is usable.
    pub(crate) fn ensure_linked(&self) -> bool {
        if !self.inner.state.borrow().ready
            && let Err(msg) = self.finalize()
        {
            console::error(&format!("[rswebgl] {msg}"));
        }
        self.inner.state.borrow().valid
    }

    // Reads the link status once the link has completed, frees the shader
    // objects, and returns the compile/link logs on failure.
    fn finalize(&self) -> Result<(), String> {
        let gl = &self.inner.gl;
        let mut st = self.inner.state.borrow_mut();
        st.ready = true;
        st.valid = gl
            .get_program_parameter(&self.inner.raw, WebGl2RenderingContext::LINK_STATUS)
            .as_bool()
            .unwrap_or(false);

        let result = if st.valid {
            Ok(())
        } else {
            let prog_log = gl
                .get_program_info_log(&self.inner.raw)
                .unwrap_or_else(|| "unknown error".into());
            let mut msg = format!("program link failed: {}", prog_log.trim());
            for (stage, shader) in [("vertex", &st.vert), ("fragment", &st.frag)] {
                let log = shader
                    .as_ref()
                    .and_then(|s| gl.get_shader_info_log(s))
                    .unwrap_or_default();
                if !log.trim().is_empty() {
                    msg.push_str(&format!("\n{stage} shader: {}", log.trim()));
                }
            }
            Err(msg)
        };

        gl.delete_shader(st.vert.take().as_ref());
        gl.delete_shader(st.frag.take().as_ref());
        result
    }
}

#[wasm_bindgen]
impl Program {
    /// Whether the link has finished. Always true unless the program was created
    /// with `KHR_parallel_shader_compile` enabled; then it polls without blocking
    /// (the renderer would otherwise wait on the link at first draw), and a failed
    /// link is logged to the console once, when it completes.
    pub fn is_ready(&self) -> bool {
        if self.inner.state.borrow().ready {
            return true;
        }
        // A lost context reports every link as complete and failed.
        if self.inner.gl.is_context_lost() {
            return false;
        }

        let complete = self
            .inner
            .gl
            .get_program_parameter(&self.inner.raw, COMPLETION_STATUS_KHR)
            .as_bool()
            .unwrap_or(false);

        if complete {
            self.ensure_linked();
        }

        self.inner.state.borrow().ready
    }

    pub fn is_valid(&self) -> bool {
        self.inner.state.borrow().valid
    }
}

impl PartialEq for Program {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
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
