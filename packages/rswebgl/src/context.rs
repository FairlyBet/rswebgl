use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext, WebglLoseContext};

use crate::buffer::{Buffer, BufferKind, BufferUsage};
use crate::console;
use crate::context_loss::{self, LossState};
use crate::draw::Viewport;
use crate::extension::Extension;
use crate::framebuffer::{DefaultFramebuffer, Framebuffer};
use crate::limits;
use crate::program::{Program, Validation};
use crate::renderbuffer::{Renderbuffer, RenderbufferFormat};
use crate::renderer::Renderer;
use crate::texture::{Texture, TextureMagFilter, TextureMinFilter, TextureTarget};
use crate::uniform_buffer::{UboLayout, UniformBuffer};
use crate::vao::VertexArray;

// ---------------------------------------------------------------------------
// Context creation options (WebGLContextAttributes)
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerPreference {
    Default,
    HighPerformance,
    LowPower,
}

impl PowerPreference {
    fn as_str(&self) -> &'static str {
        match self {
            PowerPreference::Default => "default",
            PowerPreference::HighPerformance => "high-performance",
            PowerPreference::LowPower => "low-power",
        }
    }
}

/// A predefined color space, for the drawing buffer (`DefaultFramebuffer`) and
/// texture unpacking (`Context`).
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    Srgb,
    DisplayP3,
}

impl ColorSpace {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            ColorSpace::Srgb => "srgb",
            ColorSpace::DisplayP3 => "display-p3",
        }
    }

    pub(crate) fn from_js(v: &str) -> ColorSpace {
        match v {
            "display-p3" => ColorSpace::DisplayP3,
            _ => ColorSpace::Srgb,
        }
    }
}

/// Attributes passed to `getContext("webgl2", ...)`. Fields default to the WebGL
/// spec defaults; construct with `new()` and override what you need.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug)]
pub struct ContextOptions {
    pub alpha: bool,
    pub depth: bool,
    pub stencil: bool,
    pub antialias: bool,
    pub premultiplied_alpha: bool,
    pub preserve_drawing_buffer: bool,
    pub power_preference: PowerPreference,
    pub fail_if_major_performance_caveat: bool,
    pub desynchronized: bool,
}

impl Default for ContextOptions {
    fn default() -> Self {
        Self {
            alpha: true,
            depth: true,
            stencil: false,
            antialias: true,
            premultiplied_alpha: true,
            preserve_drawing_buffer: false,
            power_preference: PowerPreference::Default,
            fail_if_major_performance_caveat: false,
            desynchronized: false,
        }
    }
}

#[wasm_bindgen]
impl ContextOptions {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
}

impl ContextOptions {
    fn to_js(self) -> js_sys::Object {
        let o = js_sys::Object::new();
        let set = |k: &str, v: JsValue| {
            let _ = js_sys::Reflect::set(&o, &JsValue::from_str(k), &v);
        };
        set("alpha", self.alpha.into());
        set("depth", self.depth.into());
        set("stencil", self.stencil.into());
        set("antialias", self.antialias.into());
        set("premultipliedAlpha", self.premultiplied_alpha.into());
        set("preserveDrawingBuffer", self.preserve_drawing_buffer.into());
        set("powerPreference", self.power_preference.as_str().into());
        set(
            "failIfMajorPerformanceCaveat",
            self.fail_if_major_performance_caveat.into(),
        );
        set("desynchronized", self.desynchronized.into());
        o
    }
}

#[wasm_bindgen]
pub struct Context {
    gl: WebGl2RenderingContext,
    // Enabled extensions + loss/restore callbacks, shared with the canvas listeners.
    loss: Rc<LossState>,
    renderer: Renderer,
    default_fb: DefaultFramebuffer,
    // Debug checks for programs created from now on; all on by default.
    validate: Validation,
}

#[wasm_bindgen]
impl Context {
    pub fn from_gl(gl: WebGl2RenderingContext) -> Context {
        let default_fb = DefaultFramebuffer::new(gl.clone(), Viewport::new(0, 0, 0, 0), None);
        Context::new(gl, default_fb)
    }

    pub fn from_canvas(
        canvas: &HtmlCanvasElement,
        options: &ContextOptions,
    ) -> Result<Context, String> {
        let gl = canvas
            .get_context_with_context_options("webgl2", &options.to_js())
            .map_err(|_| "get_context failed")?
            .ok_or("WebGL2 not supported")?
            .dyn_into::<WebGl2RenderingContext>()
            .map_err(|_| "cast to WebGl2RenderingContext failed")?;
        let viewport = Viewport::new(0, 0, canvas.width() as i32, canvas.height() as i32);
        let default_fb = DefaultFramebuffer::new(gl.clone(), viewport, Some(canvas.clone()));
        Ok(Context::new(gl, default_fb))
    }

    /// Whether the context is currently lost. While it is, `Renderer::render`
    /// does nothing and `create_*` return an error.
    pub fn is_lost(&self) -> bool {
        self.gl.is_context_lost()
    }

    /// JS: called when the context is lost. Everything created so far is dead.
    pub fn set_on_lost(&self, f: js_sys::Function) {
        self.on_lost(move || {
            let _ = f.call0(&JsValue::NULL);
        });
    }

    /// JS: called once the context is back (extensions already re-enabled).
    /// Recreate every buffer, texture, program, … here and rebuild the passes;
    /// handles from before the loss are dead and only safe to drop.
    pub fn set_on_restored(&self, f: js_sys::Function) {
        self.on_restored(move || {
            let _ = f.call0(&JsValue::NULL);
        });
    }

    /// Debug aid: forces a context loss via `WEBGL_lose_context`, to exercise
    /// the restore path. Returns false if the extension isn't available.
    pub fn simulate_context_loss(&self) -> bool {
        let Some(ext) = self
            .gl
            .get_extension(Extension::WebglLoseContext.name())
            .ok()
            .flatten()
        else {
            return false;
        };
        let ext: WebglLoseContext = ext.unchecked_into();
        ext.lose_context();
        *self.loss.lose_ext.borrow_mut() = Some(ext);
        true
    }

    /// Debug aid: restores a context lost via `simulate_context_loss`. The
    /// restore (and `on_restored`) happens asynchronously. Returns false if no
    /// simulated loss is pending.
    pub fn simulate_context_restore(&self) -> bool {
        match self.loss.lose_ext.borrow_mut().take() {
            Some(ext) => {
                ext.restore_context();
                true
            }
            None => false,
        }
    }

    /// Whether programs created from now on validate their uniform-block layouts
    /// against the shader on first bind (a one-time introspection check per block;
    /// the per-frame upload path is never affected). On by default — turn off to
    /// skip the check in release builds.
    pub fn set_validate_uniform_blocks(&mut self, on: bool) {
        self.validate.blocks = on;
    }

    /// Whether programs created from now on report (once per name) active
    /// uniforms that a draw gets from neither its batch nor itself — such a
    /// uniform silently keeps whatever an earlier draw left. Costs a lookup per
    /// active uniform per draw; on by default — turn off in release builds.
    pub fn set_validate_uniforms(&mut self, on: bool) {
        self.validate.uniforms = on;
    }

    pub fn renderer(&self) -> Renderer {
        self.renderer.handle()
    }

    pub fn default_framebuffer(&self) -> DefaultFramebuffer {
        self.default_fb.handle()
    }

    /// Enables an extension; it's re-enabled automatically after a context restore.
    pub fn enable_extension(&self, ext: Extension) -> bool {
        if self.is_extension_enabled(ext.clone()) {
            return true;
        }

        let available = self.gl.get_extension(ext.name()).ok().flatten().is_some();

        if available {
            console::log(&format!("[rswebgl] extension enabled: {}", ext.name()));
            self.loss.extensions.borrow_mut().push(ext);
        } else {
            console::warn(&format!(
                "[rswebgl] extension not available: {}",
                ext.name()
            ));
        }

        available
    }

    pub fn is_extension_enabled(&self, ext: Extension) -> bool {
        self.loss.extensions.borrow().iter().any(|e| e == &ext)
    }

    pub fn create_buffer(&self, usage: BufferUsage, data: &[u8]) -> Result<Buffer, String> {
        self.check_alive()?;
        Buffer::new(&self.gl, BufferKind::Generic, usage, data)
    }

    pub fn create_empty_buffer(&self, usage: BufferUsage, size: u32) -> Result<Buffer, String> {
        self.check_alive()?;
        Buffer::new_empty(&self.gl, BufferKind::Generic, usage, size)
    }

    pub fn create_texture(
        &self,
        target: TextureTarget,
        min_filter: TextureMinFilter,
        mag_filter: TextureMagFilter,
    ) -> Result<Texture, String> {
        self.check_alive()?;
        Texture::new(&self.gl, target, min_filter, mag_filter)
    }

    pub fn create_vertex_array(&self) -> Result<VertexArray, String> {
        self.check_alive()?;
        VertexArray::new(&self.gl)
    }

    /// Allocate a uniform block backed by a GL buffer, laid out per std140 from
    /// `layout`. Fields are addressed by path (e.g. `"lights[2].color"`).
    pub fn create_uniform_buffer(
        &self,
        layout: &UboLayout,
        usage: BufferUsage,
    ) -> Result<UniformBuffer, String> {
        self.check_alive()?;
        UniformBuffer::new(&self.gl, layout, usage)
    }

    pub fn create_framebuffer(&self, width: i32, height: i32) -> Result<Framebuffer, String> {
        self.check_alive()?;
        Framebuffer::new(&self.gl, width, height)
    }

    pub fn create_renderbuffer(
        &self,
        format: RenderbufferFormat,
        width: i32,
        height: i32,
    ) -> Result<Renderbuffer, String> {
        self.check_alive()?;
        Renderbuffer::new(&self.gl, format, width, height)
    }

    pub fn create_renderbuffer_multisample(
        &self,
        format: RenderbufferFormat,
        samples: i32,
        width: i32,
        height: i32,
    ) -> Result<Renderbuffer, String> {
        self.check_alive()?;
        Renderbuffer::new_multisample(&self.gl, format, samples, width, height)
    }

    pub fn create_program(&self, vert_src: &str, frag_src: &str) -> Result<Program, String> {
        self.check_alive()?;
        let parallel = self.is_extension_enabled(Extension::KhrParallelShaderCompile);
        Program::new(&self.gl, vert_src, frag_src, parallel, self.validate)
    }
}

impl Context {
    fn new(gl: WebGl2RenderingContext, default_fb: DefaultFramebuffer) -> Context {
        limits::init(&gl);
        let loss = LossState::new();
        context_loss::install(&gl, &loss);
        let renderer = Renderer::new(gl.clone(), default_fb.handle());
        Context {
            gl,
            loss,
            renderer,
            default_fb,
            validate: Validation {
                blocks: true,
                uniforms: true,
            },
        }
    }

    /// Called when the context is lost. Everything created so far is dead.
    pub fn on_lost(&self, f: impl FnMut() + 'static) {
        *self.loss.on_lost.borrow_mut() = Some(Box::new(f));
    }

    /// Called once the context is back (extensions already re-enabled). Recreate
    /// every buffer, texture, program, … here and rebuild the passes; handles
    /// from before the loss are dead and only safe to drop.
    pub fn on_restored(&self, f: impl FnMut() + 'static) {
        *self.loss.on_restored.borrow_mut() = Some(Box::new(f));
    }

    fn check_alive(&self) -> Result<(), String> {
        if self.gl.is_context_lost() {
            Err("WebGL context is lost".into())
        } else {
            Ok(())
        }
    }

    // Internal raw-context accessor for builders within this crate. Deliberately
    // not public: handing the live WebGl2RenderingContext to users would let them
    // mutate global GL state behind the renderer's back — the desync hazard the
    // pass-based renderer exists to remove.
    pub(crate) fn gl(&self) -> WebGl2RenderingContext {
        self.gl.clone()
    }
}
