use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlProgram, WebGlShader, WebGlUniformLocation};

use crate::console;
use crate::ref_count::{RefCount, ref_counted};
use crate::uniform_cache::UniformCache;

const COMPLETION_STATUS_KHR: u32 = 0x91B1;

#[derive(Debug, Clone)]
struct ProgramInner {
    gl: WebGl2RenderingContext,
    raw: WebGlProgram,
    vert: Option<WebGlShader>,
    frag: Option<WebGlShader>,
    parallel: bool,
    ready: bool,
    valid: bool,
    cache: UniformCache,
}

ref_counted!(Program wraps ProgramInner; drop(self) {
    self.inner.gl.delete_shader(self.inner.vert.take().as_ref());
    self.inner.gl.delete_shader(self.inner.frag.take().as_ref());
    self.inner.gl.delete_program(Some(&self.inner.raw));
});

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
            inner: ProgramInner {
                gl: gl.clone(),
                raw,
                vert: Some(vert),
                frag: Some(frag),
                parallel,
                ready: false,
                valid: false,
                cache: UniformCache::new(),
            },
            rc: RefCount::new(),
        })
    }

    pub(crate) fn gl(&self) -> &WebGl2RenderingContext {
        &self.inner.gl
    }

    pub(crate) fn loc(&mut self, name: &str) -> Option<WebGlUniformLocation> {
        self.inner
            .cache
            .get(&self.inner.gl, &self.inner.raw, name)
            .cloned()
    }

    fn finalize(&mut self) {
        self.inner.ready = true;
        self.inner.valid = self
            .inner
            .gl
            .get_program_parameter(&self.inner.raw, WebGl2RenderingContext::LINK_STATUS)
            .as_bool()
            .unwrap_or(false);

        if !self.inner.valid {
            let prog_log = self
                .inner
                .gl
                .get_program_info_log(&self.inner.raw)
                .unwrap_or_else(|| "unknown error".into());
            let vert_log = self
                .inner
                .vert
                .as_ref()
                .and_then(|s| self.inner.gl.get_shader_info_log(s))
                .unwrap_or_default();
            let frag_log = self
                .inner
                .frag
                .as_ref()
                .and_then(|s| self.inner.gl.get_shader_info_log(s))
                .unwrap_or_default();

            console::error(&format!("[rswebgl] program link failed: {prog_log}"));
            if !vert_log.is_empty() {
                console::error(&format!("[rswebgl] vertex shader: {vert_log}"));
            }
            if !frag_log.is_empty() {
                console::error(&format!("[rswebgl] fragment shader: {frag_log}"));
            }
        }

        self.inner.gl.delete_shader(self.inner.vert.take().as_ref());
        self.inner.gl.delete_shader(self.inner.frag.take().as_ref());
    }
}

#[wasm_bindgen]
impl Program {
    pub fn is_ready(&mut self) -> bool {
        if self.inner.ready {
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

        self.inner.ready
    }

    pub fn is_valid(&self) -> bool {
        self.inner.valid
    }

    pub fn raw(&self) -> WebGlProgram {
        self.inner.raw.clone()
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
