use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlProgram, WebGlShader};

use crate::console;
use crate::uniform_cache::UniformCache;

// KHR_parallel_shader_compile — COMPLETION_STATUS_KHR
const COMPLETION_STATUS_KHR: u32 = 0x91B1;

#[wasm_bindgen]
#[derive(Debug)]
pub struct Program {
    pub(crate) gl: WebGl2RenderingContext,
    pub(crate) raw: WebGlProgram,
    vert: Option<WebGlShader>,
    frag: Option<WebGlShader>,
    parallel: bool,
    ready: bool,
    valid: bool,
    pub(crate) cache: UniformCache,
}

impl Program {
    pub(crate) fn new(
        gl: &WebGl2RenderingContext,
        vert_src: &str,
        frag_src: &str,
        parallel: bool,
    ) -> Result<Program, String> {
        let vert = create_shader(gl, WebGl2RenderingContext::VERTEX_SHADER, vert_src)?;
        let frag = create_shader(gl, WebGl2RenderingContext::FRAGMENT_SHADER, frag_src)?;

        let raw = gl.create_program().ok_or("createProgram failed")?;
        gl.attach_shader(&raw, &vert);
        gl.attach_shader(&raw, &frag);
        gl.link_program(&raw);

        Ok(Program {
            gl: gl.clone(),
            raw,
            vert: Some(vert),
            frag: Some(frag),
            parallel,
            ready: false,
            valid: false,
            cache: UniformCache::new(),
        })
    }

    fn finalize(&mut self) {
        self.ready = true;
        self.valid = self
            .gl
            .get_program_parameter(&self.raw, WebGl2RenderingContext::LINK_STATUS)
            .as_bool()
            .unwrap_or(false);

        if !self.valid {
            let prog_log = self
                .gl
                .get_program_info_log(&self.raw)
                .unwrap_or_else(|| "unknown error".into());
            let vert_log = self
                .vert
                .as_ref()
                .and_then(|s| self.gl.get_shader_info_log(s))
                .unwrap_or_default();
            let frag_log = self
                .frag
                .as_ref()
                .and_then(|s| self.gl.get_shader_info_log(s))
                .unwrap_or_default();

            console::error(&format!("[rswebgl] program link failed: {prog_log}"));
            if !vert_log.is_empty() {
                console::error(&format!("[rswebgl] vertex shader: {vert_log}"));
            }
            if !frag_log.is_empty() {
                console::error(&format!("[rswebgl] fragment shader: {frag_log}"));
            }
        }

        self.gl.delete_shader(self.vert.take().as_ref());
        self.gl.delete_shader(self.frag.take().as_ref());
    }
}

#[wasm_bindgen]
impl Program {
    /// Returns true once compilation and linking are complete.
    /// With KHR_parallel_shader_compile this is non-blocking — call
    /// each frame until true, then check `is_valid()`.
    pub fn is_ready(&mut self) -> bool {
        if self.ready {
            return true;
        }

        let complete = if self.parallel {
            self.gl
                .get_program_parameter(&self.raw, COMPLETION_STATUS_KHR)
                .as_bool()
                .unwrap_or(false)
        } else {
            true
        };

        if complete {
            self.finalize();
        }

        self.ready
    }

    /// Returns true if the program linked successfully.
    /// Only meaningful after `is_ready()` returns true.
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    pub fn raw(&self) -> WebGlProgram {
        self.raw.clone()
    }
}

impl Drop for Program {
    fn drop(&mut self) {
        self.gl.delete_shader(self.vert.take().as_ref());
        self.gl.delete_shader(self.frag.take().as_ref());
        self.gl.delete_program(Some(&self.raw));
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
