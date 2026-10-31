use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlProgram, WebGlShader, WebGlUniformLocation};

use crate::console;
use crate::context_loss;
use crate::limits;
use crate::uniform_cache::UniformCache;
use crate::uniform_values::UniformValues;

const COMPLETION_STATUS_KHR: u32 = 0x91B1;
// getUniformBlockIndex returns this for a name that isn't an active uniform block.
const INVALID_INDEX: u32 = 0xFFFF_FFFF;

// Everything that changes after construction. It lives in the shared inner, so
// all clones of a Program see one finalize / one uniform-location cache, and the
// mutating methods take `&self` (a Program held inside a render `Batch` needs no
// `&mut`).
//
// Texture units and uniform-buffer binding points are assigned here, per program
// and by name, in order of first use: the sampler's uniform1i / the block's
// uniformBlockBinding is then written once for the program's lifetime, and
// samplers/blocks coming from different UniformValues (batch + draw) can never
// collide on a slot. `None` = no such active sampler/block in the shader.
#[derive(Debug)]
struct ProgramState {
    vert: Option<WebGlShader>,
    frag: Option<WebGlShader>,
    ready: bool,
    valid: bool,
    cache: UniformCache,
    sampler_units: HashMap<String, Option<u32>>,
    block_points: HashMap<String, Option<u32>>,
    // Active non-block uniforms (array names without "[0]"), read lazily.
    active: Option<Vec<String>>,
    // Uniforms already reported as unset, so each is reported once.
    reported_unset: HashSet<String>,
}

#[derive(Debug)]
struct ProgramInner {
    gl: WebGl2RenderingContext,
    raw: WebGlProgram,
    // Context generation it was created in (see context_loss).
    generation: u32,
    validate: Validation,
    state: RefCell<ProgramState>,
}

/// Debug checks a program runs while drawing (each reported once).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Validation {
    /// Uniform-block size vs our std140 layout, on first bind.
    pub blocks: bool,
    /// Active uniforms that neither the batch nor the draw sets.
    pub uniforms: bool,
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
        validate: Validation,
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
                validate,
                state: RefCell::new(ProgramState {
                    vert: Some(vert),
                    frag: Some(frag),
                    ready: false,
                    valid: false,
                    cache: UniformCache::new(),
                    sampler_units: HashMap::new(),
                    block_points: HashMap::new(),
                    active: None,
                    reported_unset: HashSet::new(),
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

    // The texture unit sampler `name` reads from. Assigned and written to the
    // sampler uniform (uniform1i) the first time it's asked for, so the program
    // must be in use then.
    pub(crate) fn sampler_unit(&self, name: &str) -> Option<u32> {
        if let Some(unit) = self.inner.state.borrow().sampler_units.get(name) {
            return *unit;
        }
        let loc = self.loc(name);
        let mut st = self.inner.state.borrow_mut();
        let unit = loc.and_then(|loc| {
            let next = st.sampler_units.values().flatten().count() as u32;
            let max = limits::max_combined_texture_units();
            if next >= max {
                console::warn(&format!(
                    "[rswebgl] sampler \"{name}\": all {max} texture units are in use"
                ));
                return None;
            }
            self.inner.gl.uniform1i(Some(&loc), next as i32);
            Some(next)
        });
        st.sampler_units.insert(name.to_string(), unit);
        unit
    }

    // The uniform-buffer binding point block `name` reads from. On first use the
    // block is resolved, linked to its point (uniformBlockBinding) and, if
    // validation is on, its shader-reported size checked against `expected_size`
    // (our computed std140 size).
    pub(crate) fn block_point(&self, name: &str, expected_size: u32) -> Option<u32> {
        if let Some(point) = self.inner.state.borrow().block_points.get(name) {
            return *point;
        }
        let gl = &self.inner.gl;
        let raw = &self.inner.raw;
        let mut st = self.inner.state.borrow_mut();
        let index = gl.get_uniform_block_index(raw, name);
        let point = if index == INVALID_INDEX {
            console::warn(&format!("[rswebgl] uniform block not found: \"{name}\""));
            None
        } else {
            if self.inner.validate.blocks {
                validate_block_size(gl, raw, index, name, expected_size);
            }
            let next = st.block_points.values().flatten().count() as u32;
            let max = limits::max_uniform_buffer_bindings();
            if next >= max {
                console::warn(&format!(
                    "[rswebgl] uniform block \"{name}\": all {max} binding points are in use"
                ));
                None
            } else {
                gl.uniform_block_binding(raw, index, next);
                Some(next)
            }
        };
        st.block_points.insert(name.to_string(), point);
        point
    }

    // Reports (once per name) active uniforms that none of `sources` sets: such a
    // uniform silently keeps whatever an earlier draw of this program left in it.
    pub(crate) fn check_uniforms_set(&self, sources: &[&UniformValues]) {
        if !self.inner.validate.uniforms {
            return;
        }
        self.ensure_active();
        let mut st = self.inner.state.borrow_mut();
        let st = &mut *st;
        for name in st.active.iter().flatten() {
            if st.reported_unset.contains(name) || sources.iter().any(|u| u.contains(name)) {
                continue;
            }
            console::warn(&format!(
                "[rswebgl] uniform \"{name}\" is set by neither the batch nor the draw, so it \
                 keeps the value an earlier draw of this program left. Set it on the Batch to \
                 share it across draws."
            ));
            st.reported_unset.insert(name.clone());
        }
    }

    fn ensure_active(&self) {
        if self.inner.state.borrow().active.is_some() {
            return;
        }
        let names = active_uniforms(&self.inner.gl, &self.inner.raw);
        self.inner.state.borrow_mut().active = Some(names);
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

    /// Names of the program's active uniforms outside uniform blocks (samplers
    /// included; arrays as their base name, without `[0]`), sorted.
    pub fn active_uniform_names(&self) -> Vec<String> {
        self.ensure_active();
        self.inner.state.borrow().active.clone().unwrap_or_default()
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

// Active uniforms that live outside uniform blocks (block members are set
// through the block's buffer, not one by one). Empty for an unlinked program.
fn active_uniforms(gl: &WebGl2RenderingContext, program: &WebGlProgram) -> Vec<String> {
    let count = gl
        .get_program_parameter(program, WebGl2RenderingContext::ACTIVE_UNIFORMS)
        .as_f64()
        .unwrap_or(0.0) as u32;
    if count == 0 {
        return Vec::new();
    }
    let indices: js_sys::Array = (0..count).map(JsValue::from).collect();
    let block_of = js_sys::Array::from(&gl.get_active_uniforms(
        program,
        &indices,
        WebGl2RenderingContext::UNIFORM_BLOCK_INDEX,
    ));
    let mut names: Vec<String> = (0..count)
        .filter(|&i| block_of.get(i).as_f64() == Some(-1.0))
        .filter_map(|i| gl.get_active_uniform(program, i))
        .map(|info| {
            let name = info.name();
            match name.strip_suffix("[0]") {
                Some(base) => base.to_string(),
                None => name,
            }
        })
        .filter(|n| !n.starts_with("gl_"))
        .collect();
    names.sort();
    names
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
