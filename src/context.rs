use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext};

use crate::buffer::{Buffer, BufferTarget, BufferUsage};
use crate::console;
use crate::extension::Extension;
use crate::limits;
use crate::program::Program;
use crate::texture::{Texture, TextureMagFilter, TextureMinFilter, TextureTarget};
use crate::vao::VertexArray;

#[wasm_bindgen]
#[derive(Debug)]
pub struct Context {
    gl: WebGl2RenderingContext,
    extensions: Vec<Extension>,
}

#[wasm_bindgen]
impl Context {
    pub fn from_gl(gl: WebGl2RenderingContext) -> Context {
        limits::init(&gl);
        Context {
            gl,
            extensions: Vec::new(),
        }
    }

    pub fn from_canvas(canvas: &HtmlCanvasElement) -> Result<Context, String> {
        let gl = canvas
            .get_context("webgl2")
            .map_err(|_| "get_context failed")?
            .ok_or("WebGL2 not supported")?
            .dyn_into::<WebGl2RenderingContext>()
            .map_err(|_| "cast to WebGl2RenderingContext failed")?;
        limits::init(&gl);
        Ok(Context {
            gl,
            extensions: Vec::new(),
        })
    }

    pub fn enable_extension(&mut self, ext: Extension) -> bool {
        if self.is_extension_enabled(ext.clone()) {
            return true;
        }

        let available = self.gl.get_extension(ext.name()).ok().flatten().is_some();

        if available {
            console::log(&format!("[rswebgl] extension enabled: {}", ext.name()));
            self.extensions.push(ext);
        } else {
            console::warn(&format!(
                "[rswebgl] extension not available: {}",
                ext.name()
            ));
        }

        available
    }

    pub fn is_extension_enabled(&self, ext: Extension) -> bool {
        self.extensions.iter().any(|e| e == &ext)
    }

    pub fn create_buffer(
        &self,
        target: BufferTarget,
        usage: BufferUsage,
        data: &[u8],
    ) -> Result<Buffer, String> {
        Buffer::new(&self.gl, target, usage, data)
    }

    pub fn create_empty_buffer(
        &self,
        target: BufferTarget,
        usage: BufferUsage,
        size: u32,
    ) -> Result<Buffer, String> {
        Buffer::new_empty(&self.gl, target, usage, size)
    }

    pub fn create_texture(
        &self,
        target: TextureTarget,
        min_filter: TextureMinFilter,
        mag_filter: TextureMagFilter,
    ) -> Result<Texture, String> {
        Texture::new(&self.gl, target, min_filter, mag_filter)
    }

    pub fn create_vertex_array(&self) -> Result<VertexArray, String> {
        VertexArray::new(&self.gl)
    }

    pub fn create_program(&self, vert_src: &str, frag_src: &str) -> Result<Program, String> {
        let parallel = self.is_extension_enabled(Extension::KhrParallelShaderCompile);
        Program::new(&self.gl, vert_src, frag_src, parallel)
    }

    pub fn gl(&self) -> WebGl2RenderingContext {
        self.gl.clone()
    }
}

