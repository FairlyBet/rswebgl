use crate::error::Error;
use std::cell::{Cell, RefCell, RefMut};
use std::rc::Rc;

use rswebgl_foundation::handle::Handle;
use rswebgl_foundation::pool::Pool;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, WebGl2RenderingContext, WebglLoseContext};

use crate::buffer::BufferObj;
use crate::console;
use crate::context_loss::{self, LossState};
use crate::draw::Viewport;
use crate::extension::Extension;
use crate::feedback::FeedbackObj;
use crate::framebuffer::{DefaultFramebuffer, FramebufferObj};
use crate::limits;
use crate::param::{ParamGroupObj, ParamLayoutObj};
use crate::program::{ProgramObj, Validation};
use crate::query::QueryObj;
use crate::render_state::RenderState;
use crate::renderbuffer::RenderbufferObj;
use crate::renderer::GlState;
use crate::sampler::SamplerObj;
use crate::texture::TextureObj;
use crate::uniform_buffer::UniformBufferObj;
use crate::vao::VertexArrayObj;

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
    #[wasm_bindgen(js_name = premultipliedAlpha)]
    pub premultiplied_alpha: bool,
    #[wasm_bindgen(js_name = preserveDrawingBuffer)]
    pub preserve_drawing_buffer: bool,
    #[wasm_bindgen(js_name = powerPreference)]
    pub power_preference: PowerPreference,
    #[wasm_bindgen(js_name = failIfMajorPerformanceCaveat)]
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

// ---------------------------------------------------------------------------
// Resources — every GL object lives in one of these pools.
// ---------------------------------------------------------------------------

#[derive(Default)]
pub(crate) struct Resources {
    pub(crate) buffers: Pool<BufferObj>,
    pub(crate) textures: Pool<TextureObj>,
    pub(crate) renderbuffers: Pool<RenderbufferObj>,
    pub(crate) framebuffers: Pool<FramebufferObj>,
    pub(crate) programs: Pool<ProgramObj>,
    pub(crate) vertex_arrays: Pool<VertexArrayObj>,
    pub(crate) uniform_buffers: Pool<UniformBufferObj>,
    pub(crate) param_layouts: Pool<ParamLayoutObj>,
    pub(crate) param_groups: Pool<ParamGroupObj>,
    pub(crate) render_states: Pool<RenderState>,
    pub(crate) samplers: Pool<SamplerObj>,
    pub(crate) queries: Pool<QueryObj>,
    pub(crate) feedbacks: Pool<FeedbackObj>,
}

impl Resources {
    // After a context loss: every GPU object is already gone, so the values are
    // dropped without GL delete calls, and every outstanding handle goes stale.
    fn forget_all(&mut self) {
        self.buffers.drain();
        self.textures.drain();
        self.renderbuffers.drain();
        self.framebuffers.drain();
        self.programs.drain();
        self.vertex_arrays.drain();
        self.uniform_buffers.drain();
        self.samplers.drain();
        self.queries.drain();
        self.feedbacks.drain();
        // Parameter layouts and groups are CPU-only and survive; the texture and
        // buffer handles inside groups go stale like every other GPU handle.
    }
}

/// Reports a handle that doesn't resolve (destroyed, from before a context loss,
/// or never issued), once per message.
pub(crate) fn stale(kind: &str, op: &str, h: impl Into<Handle>) {
    // Between a context loss and the app rebuilding in `on_restored`, render
    // loops keep touching old handles: expected, not worth reporting.
    if LOST_CONTEXTS.get() > 0 {
        return;
    }
    console::error_once(&format!(
        "[rswebgl] {op}: {kind} {:?} is destroyed or invalid; ignored",
        h.into()
    ));
}

thread_local! {
    // Contexts currently lost (set by the loss listeners).
    static LOST_CONTEXTS: Cell<u32> = const { Cell::new(0) };
}

/// Tracks loss/restore so `stale` stays quiet while a context is lost.
pub(crate) fn note_lost(lost: bool) {
    LOST_CONTEXTS.set(if lost {
        LOST_CONTEXTS.get() + 1
    } else {
        LOST_CONTEXTS.get().saturating_sub(1)
    });
}

// ---------------------------------------------------------------------------
// Core — the single shared root behind every Context handle.
// ---------------------------------------------------------------------------

pub(crate) struct Core {
    pub(crate) gl: WebGl2RenderingContext,
    res: RefCell<Resources>,
    // Set by a context-loss event that arrived while `res` was borrowed; the
    // pools are cleared on the next access instead.
    forget_pending: Cell<bool>,
    pub(crate) default_fb: DefaultFramebuffer,
    pub(crate) validate: Cell<Validation>,
    pub(crate) loss: LossState,
    // What the renderer last left bound, kept across frames so an unchanged
    // binding isn't re-issued. Anything else that binds resets the part it
    // touched (`GlState::forget_*`).
    state: RefCell<GlState>,
    // Set when GL state may differ from `state` in unknown ways (context loss
    // or restore, `invalidate_state`); `state` is reset on its next access.
    state_stale: Cell<bool>,
}

impl Core {
    /// The resource pools. Not reentrant: never call user code while holding it.
    pub(crate) fn res(&self) -> RefMut<'_, Resources> {
        let mut res = self.res.borrow_mut();
        if self.forget_pending.replace(false) {
            res.forget_all();
        }
        res
    }

    /// The GL state cache. Not reentrant, like `res`.
    pub(crate) fn gl_state(&self) -> RefMut<'_, GlState> {
        let mut s = self.state.borrow_mut();
        if self.state_stale.replace(false) {
            s.reset();
        }
        s
    }

    pub(crate) fn invalidate_state(&self) {
        self.state_stale.set(true);
    }

    pub(crate) fn forget_all(&self) {
        self.invalidate_state();
        match self.res.try_borrow_mut() {
            Ok(mut res) => res.forget_all(),
            Err(_) => self.forget_pending.set(true),
        }
    }
}

/// A WebGL2 context and every GPU object created on it.
///
/// Resources are created and used through the context and identified by
/// numeric handles (`TextureId`, `BufferId`, …). A handle is just a number: copy
/// it, store it, pass it to JS. The object lives until its `destroy_*` call;
/// after that (or after a context loss) the handle is detected as stale.
///
/// `Context` itself is a cheap handle to shared state — clone it into render
/// loops and callbacks.
#[wasm_bindgen]
#[derive(Clone)]
pub struct Context {
    pub(crate) core: Rc<Core>,
}

#[wasm_bindgen]
impl Context {
    #[wasm_bindgen(js_name = fromGl)]
    pub fn from_gl(gl: WebGl2RenderingContext) -> Context {
        let default_fb = DefaultFramebuffer::new(gl.clone(), Viewport::new(0, 0, 0, 0), None);
        Context::new(gl, default_fb)
    }

    #[wasm_bindgen(js_name = fromCanvas)]
    pub fn from_canvas(
        canvas: &HtmlCanvasElement,
        options: &ContextOptions,
    ) -> Result<Context, Error> {
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

    /// Whether the context is currently lost. While it is, `render` does
    /// nothing and `create_*` return an error.
    #[wasm_bindgen(js_name = isLost)]
    pub fn is_lost(&self) -> bool {
        self.core.gl.is_context_lost()
    }

    /// JS: called when the context is lost. Every handle is dead from here on.
    #[wasm_bindgen(js_name = setOnLost)]
    pub fn set_on_lost(&self, f: js_sys::Function) {
        self.on_lost(move |_| {
            let _ = f.call0(&JsValue::NULL);
        });
    }

    /// JS: called once the context is back (extensions already re-enabled).
    /// Recreate every resource here; handles from before the loss are stale.
    #[wasm_bindgen(js_name = setOnRestored)]
    pub fn set_on_restored(&self, f: js_sys::Function) {
        self.on_restored(move |_| {
            let _ = f.call0(&JsValue::NULL);
        });
    }

    /// Debug aid: forces a context loss via `WEBGL_lose_context`, to exercise
    /// the restore path. Returns false if the extension isn't available.
    #[wasm_bindgen(js_name = simulateContextLoss)]
    pub fn simulate_context_loss(&self) -> bool {
        let Some(ext) = self
            .core
            .gl
            .get_extension(Extension::WebglLoseContext.name())
            .ok()
            .flatten()
        else {
            return false;
        };
        let ext: WebglLoseContext = ext.unchecked_into();
        ext.lose_context();
        *self.core.loss.lose_ext.borrow_mut() = Some(ext);
        true
    }

    /// Debug aid: restores a context lost via `simulate_context_loss`. The
    /// restore (and `on_restored`) happens asynchronously. Returns false if no
    /// simulated loss is pending.
    #[wasm_bindgen(js_name = simulateContextRestore)]
    pub fn simulate_context_restore(&self) -> bool {
        let ext = self.core.loss.lose_ext.borrow_mut().take();
        match ext {
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
    #[wasm_bindgen(js_name = setValidateUniformBlocks)]
    pub fn set_validate_uniform_blocks(&self, on: bool) {
        let mut v = self.core.validate.get();
        v.blocks = on;
        self.core.validate.set(v);
    }

    /// Whether programs created from now on report (once per name) active
    /// uniforms that a draw gets from neither its batch nor itself — such a
    /// uniform silently keeps whatever an earlier draw left. Costs a lookup per
    /// active uniform per draw; on by default — turn off in release builds.
    #[wasm_bindgen(js_name = setValidateUniforms)]
    pub fn set_validate_uniforms(&self, on: bool) {
        let mut v = self.core.validate.get();
        v.uniforms = on;
        self.core.validate.set(v);
    }

    #[wasm_bindgen(js_name = defaultFramebuffer)]
    pub fn default_framebuffer(&self) -> DefaultFramebuffer {
        self.core.default_fb.handle()
    }

    /// Enables an extension; it's re-enabled automatically after a context restore.
    #[wasm_bindgen(js_name = enableExtension)]
    pub fn enable_extension(&self, ext: Extension) -> bool {
        if self.is_extension_enabled(ext.clone()) {
            return true;
        }
        let name = ext.name();
        let available = self.enable_extension_quietly(ext);
        if available {
            console::log(&format!("[rswebgl] extension enabled: {name}"));
        } else {
            console::warn(&format!("[rswebgl] extension not available: {name}"));
        }

        available
    }

    #[wasm_bindgen(js_name = isExtensionEnabled)]
    pub fn is_extension_enabled(&self, ext: Extension) -> bool {
        self.core.loss.extensions.borrow().iter().any(|e| e == &ext)
    }
}

impl Context {
    // `enable_extension` without the console line; true if available.
    pub(crate) fn enable_extension_quietly(&self, ext: Extension) -> bool {
        if self.is_extension_enabled(ext.clone()) {
            return true;
        }
        let available = self
            .core
            .gl
            .get_extension(ext.name())
            .ok()
            .flatten()
            .is_some();
        if available {
            self.core.loss.extensions.borrow_mut().push(ext);
        }
        available
    }

    fn new(gl: WebGl2RenderingContext, default_fb: DefaultFramebuffer) -> Context {
        limits::init(&gl);
        let core = Rc::new(Core {
            gl,
            res: RefCell::new(Resources::default()),
            forget_pending: Cell::new(false),
            default_fb,
            validate: Cell::new(Validation {
                blocks: true,
                uniforms: true,
            }),
            loss: LossState::new(),
            state: RefCell::new(GlState::default()),
            state_stale: Cell::new(false),
        });
        context_loss::install(&core);
        let ctx = Context { core };
        ctx.enable_float_extensions();
        ctx
    }

    /// Called when the context is lost. Every handle is dead from here on.
    pub fn on_lost(&self, f: impl FnMut(&Context) + 'static) {
        *self.core.loss.on_lost.borrow_mut() = Some(Box::new(f));
    }

    /// Called once the context is back (extensions already re-enabled), with the
    /// context to recreate every resource on; handles from before the loss are
    /// stale. Taking the context as an argument (rather than capturing a clone)
    /// avoids a reference cycle through the context's own callback slot.
    pub fn on_restored(&self, f: impl FnMut(&Context) + 'static) {
        *self.core.loss.on_restored.borrow_mut() = Some(Box::new(f));
    }

    pub(crate) fn check_alive(&self) -> Result<(), Error> {
        if self.core.gl.is_context_lost() {
            Err("WebGL context is lost".into())
        } else {
            Ok(())
        }
    }

    pub(crate) fn gl(&self) -> &WebGl2RenderingContext {
        &self.core.gl
    }

    pub(crate) fn res(&self) -> RefMut<'_, Resources> {
        self.core.res()
    }

    pub(crate) fn gl_state(&self) -> RefMut<'_, GlState> {
        self.core.gl_state()
    }
}
