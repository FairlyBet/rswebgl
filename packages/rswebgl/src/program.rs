use std::collections::{HashMap, HashSet};

use rswebgl_foundation::pool::Destroyed;
use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlProgram, WebGlShader, WebGlUniformLocation};

use crate::console;
use crate::context::{Context, stale};
use crate::extension::Extension;
use crate::ids::ProgramId;
use crate::limits;
use crate::uniform_cache::UniformCache;
use crate::uniform_values::UniformValues;

const COMPLETION_STATUS_KHR: u32 = 0x91B1;
// getUniformBlockIndex returns this for a name that isn't an active uniform block.
const INVALID_INDEX: u32 = 0xFFFF_FFFF;

/// Debug checks a program runs while drawing (each reported once).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Validation {
    /// Uniform-block size vs our std140 layout, on first bind.
    pub blocks: bool,
    /// Active uniforms that neither the batch nor the draw sets.
    pub uniforms: bool,
}

/// A program in the context's pool.
///
/// Texture units and uniform-buffer binding points are assigned here, per program
/// and by name, in order of first use: the sampler's uniform1i / the block's
/// uniformBlockBinding is then written once for the program's lifetime, and
/// samplers/blocks coming from different UniformValues (batch + draw) can never
/// collide on a slot. `None` = no such active sampler/block in the shader.
#[derive(Debug)]
pub(crate) struct ProgramObj {
    pub(crate) raw: WebGlProgram,
    validate: Validation,
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

impl ProgramObj {
    fn new(
        gl: &WebGl2RenderingContext,
        vert_src: &str,
        frag_src: &str,
        parallel: bool,
        validate: Validation,
    ) -> Result<ProgramObj, String> {
        let vert = create_shader(gl, WebGl2RenderingContext::VERTEX_SHADER, vert_src)?;
        let frag = create_shader(gl, WebGl2RenderingContext::FRAGMENT_SHADER, frag_src)?;

        let raw = gl.create_program().ok_or("createProgram failed")?;
        gl.attach_shader(&raw, &vert);
        gl.attach_shader(&raw, &frag);
        gl.link_program(&raw);

        let mut program = ProgramObj {
            raw,
            validate,
            vert: Some(vert),
            frag: Some(frag),
            ready: false,
            valid: false,
            cache: UniformCache::new(),
            sampler_units: HashMap::new(),
            block_points: HashMap::new(),
            active: None,
            reported_unset: HashSet::new(),
        };

        // Without parallel compile the first status query blocks on the link
        // anyway, so do it now and hand compile/link errors back to the caller.
        // With it, the link finishes in the background; `is_ready` polls it
        // without blocking, and the renderer waits on it at first draw.
        if !parallel && let Err(e) = program.finalize(gl) {
            program.delete(gl);
            return Err(e);
        }
        Ok(program)
    }

    pub(crate) fn delete(self, gl: &WebGl2RenderingContext) {
        gl.delete_shader(self.vert.as_ref());
        gl.delete_shader(self.frag.as_ref());
        gl.delete_program(Some(&self.raw));
    }

    // The texture unit sampler `name` reads from. Assigned and written to the
    // sampler uniform (uniform1i) the first time it's asked for, so the program
    // must be in use then.
    pub(crate) fn sampler_unit(&mut self, gl: &WebGl2RenderingContext, name: &str) -> Option<u32> {
        if let Some(unit) = self.sampler_units.get(name) {
            return *unit;
        }
        let loc = self.loc(gl, name);
        let unit = loc.and_then(|loc| {
            let next = self.sampler_units.values().flatten().count() as u32;
            let max = limits::max_combined_texture_units();
            if next >= max {
                console::warn(&format!(
                    "[rswebgl] sampler \"{name}\": all {max} texture units are in use"
                ));
                return None;
            }
            gl.uniform1i(Some(&loc), next as i32);
            Some(next)
        });
        self.sampler_units.insert(name.to_string(), unit);
        unit
    }

    // The uniform-buffer binding point block `name` reads from. On first use the
    // block is resolved, linked to its point (uniformBlockBinding) and, if
    // validation is on, its shader-reported size checked against `expected_size`
    // (our computed std140 size).
    pub(crate) fn block_point(
        &mut self,
        gl: &WebGl2RenderingContext,
        name: &str,
        expected_size: u32,
    ) -> Option<u32> {
        if let Some(point) = self.block_points.get(name) {
            return *point;
        }
        let index = gl.get_uniform_block_index(&self.raw, name);
        let point = if index == INVALID_INDEX {
            console::warn(&format!("[rswebgl] uniform block not found: \"{name}\""));
            None
        } else {
            if self.validate.blocks {
                validate_block_size(gl, &self.raw, index, name, expected_size);
            }
            let next = self.block_points.values().flatten().count() as u32;
            let max = limits::max_uniform_buffer_bindings();
            if next >= max {
                console::warn(&format!(
                    "[rswebgl] uniform block \"{name}\": all {max} binding points are in use"
                ));
                None
            } else {
                gl.uniform_block_binding(&self.raw, index, next);
                Some(next)
            }
        };
        self.block_points.insert(name.to_string(), point);
        point
    }

    // Reports (once per name) active uniforms that none of `sources` sets: such a
    // uniform silently keeps whatever an earlier draw of this program left in it.
    pub(crate) fn check_uniforms_set(
        &mut self,
        gl: &WebGl2RenderingContext,
        sources: &[&UniformValues],
    ) {
        if !self.validate.uniforms {
            return;
        }
        self.ensure_active(gl);
        for name in self.active.iter().flatten() {
            if self.reported_unset.contains(name) || sources.iter().any(|u| u.contains(name)) {
                continue;
            }
            console::warn(&format!(
                "[rswebgl] uniform \"{name}\" is set by neither the batch nor the draw, so it \
                 keeps the value an earlier draw of this program left. Set it on the Batch to \
                 share it across draws."
            ));
            self.reported_unset.insert(name.clone());
        }
    }

    fn ensure_active(&mut self, gl: &WebGl2RenderingContext) {
        if self.active.is_none() {
            self.active = Some(active_uniforms(gl, &self.raw));
        }
    }

    pub(crate) fn loc(
        &mut self,
        gl: &WebGl2RenderingContext,
        name: &str,
    ) -> Option<WebGlUniformLocation> {
        self.cache.get(gl, &self.raw, name).cloned()
    }

    // Waits for the link if it's still running (blocking), reporting a failure
    // once. Returns whether the program is usable.
    pub(crate) fn ensure_linked(&mut self, gl: &WebGl2RenderingContext) -> bool {
        if !self.ready
            && let Err(msg) = self.finalize(gl)
        {
            console::error(&format!("[rswebgl] {msg}"));
        }
        self.valid
    }

    // Reads the link status once the link has completed, frees the shader
    // objects, and returns the compile/link logs on failure.
    fn finalize(&mut self, gl: &WebGl2RenderingContext) -> Result<(), String> {
        self.ready = true;
        self.valid = gl
            .get_program_parameter(&self.raw, WebGl2RenderingContext::LINK_STATUS)
            .as_bool()
            .unwrap_or(false);

        let result = if self.valid {
            Ok(())
        } else {
            let prog_log = gl
                .get_program_info_log(&self.raw)
                .unwrap_or_else(|| "unknown error".into());
            let mut msg = format!("program link failed: {}", prog_log.trim());
            for (stage, shader) in [("vertex", &self.vert), ("fragment", &self.frag)] {
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

        gl.delete_shader(self.vert.take().as_ref());
        gl.delete_shader(self.frag.take().as_ref());
        result
    }
}

#[wasm_bindgen]
impl Context {
    /// Compiles and links a program. Without `KHR_parallel_shader_compile` a
    /// compile or link error comes back here, with the shader logs; with it,
    /// creation doesn't block and errors are reported when the link completes.
    pub fn create_program(&self, vert_src: &str, frag_src: &str) -> Result<ProgramId, String> {
        self.check_alive()?;
        let parallel = self.is_extension_enabled(Extension::KhrParallelShaderCompile);
        let obj = ProgramObj::new(
            self.gl(),
            vert_src,
            frag_src,
            parallel,
            self.core.validate.get(),
        )?;
        let mut res = self.res();
        match res.programs.insert(obj) {
            Ok(h) => Ok(h.into()),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn destroy_program(&self, program: ProgramId) {
        let destroyed = self.res().programs.destroy(program.0);
        match destroyed {
            Destroyed::Now(obj) => obj.delete(self.gl()),
            Destroyed::Deferred => {}
            Destroyed::Invalid => stale("program", "destroy_program", program),
        }
    }

    /// Whether the link has finished. Always true unless the program was created
    /// with `KHR_parallel_shader_compile` enabled; then it polls without blocking
    /// (the renderer would otherwise wait on the link at first draw), and a failed
    /// link is logged to the console once, when it completes. False for a stale
    /// handle.
    pub fn program_is_ready(&self, program: ProgramId) -> bool {
        let gl = self.gl();
        let mut res = self.res();
        let Some(p) = res.programs.get_mut(program.0) else {
            return false;
        };
        if p.ready {
            return true;
        }
        // A lost context reports every link as complete and failed.
        if gl.is_context_lost() {
            return false;
        }
        let complete = gl
            .get_program_parameter(&p.raw, COMPLETION_STATUS_KHR)
            .as_bool()
            .unwrap_or(false);
        if complete {
            p.ensure_linked(gl);
        }
        p.ready
    }

    /// Whether the program linked successfully (false while still linking, or
    /// for a stale handle).
    pub fn program_is_valid(&self, program: ProgramId) -> bool {
        self.res().programs.get(program.0).is_some_and(|p| p.valid)
    }

    /// Names of the program's active uniforms outside uniform blocks (samplers
    /// included; arrays as their base name, without `[0]`), sorted.
    pub fn program_active_uniform_names(&self, program: ProgramId) -> Vec<String> {
        let gl = self.gl();
        let mut res = self.res();
        let Some(p) = res.programs.get_mut(program.0) else {
            stale("program", "program_active_uniform_names", program);
            return Vec::new();
        };
        p.ensure_active(gl);
        p.active.clone().unwrap_or_default()
    }
}

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
