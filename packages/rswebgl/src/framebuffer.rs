use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, ResizeObserver, WebGl2RenderingContext, WebGlFramebuffer};

use crate::draw::Viewport;
use crate::renderbuffer::Renderbuffer;
use crate::texture::{CubeMapFace, Texture};

// ---------------------------------------------------------------------------
// ClearMask
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClearMask {
    pub color: bool,
    pub depth: bool,
    pub stencil: bool,
}

#[wasm_bindgen]
impl ClearMask {
    pub fn none() -> Self {
        Self {
            color: false,
            depth: false,
            stencil: false,
        }
    }
    pub fn all() -> Self {
        Self {
            color: true,
            depth: true,
            stencil: true,
        }
    }
    pub fn color() -> Self {
        Self {
            color: true,
            depth: false,
            stencil: false,
        }
    }
    pub fn depth() -> Self {
        Self {
            color: false,
            depth: true,
            stencil: false,
        }
    }
    pub fn stencil() -> Self {
        Self {
            color: false,
            depth: false,
            stencil: true,
        }
    }
    pub fn color_depth() -> Self {
        Self {
            color: true,
            depth: true,
            stencil: false,
        }
    }
    pub fn color_depth_stencil() -> Self {
        Self::all()
    }
}

impl ClearMask {
    pub(crate) fn as_gl(&self) -> u32 {
        let mut m = 0u32;
        if self.color {
            m |= WebGl2RenderingContext::COLOR_BUFFER_BIT;
        }
        if self.depth {
            m |= WebGl2RenderingContext::DEPTH_BUFFER_BIT;
        }
        if self.stencil {
            m |= WebGl2RenderingContext::STENCIL_BUFFER_BIT;
        }
        m
    }
}

// ---------------------------------------------------------------------------
// InvalidateMask
// ---------------------------------------------------------------------------
//
// Selects which attachments to discard via invalidateFramebuffer. Discarding
// data you won't read again (depth/stencil, MSAA color) lets tiled GPUs skip
// writing it back to memory — a real win on mobile. `color` invalidates *all*
// attached color attachments; per-index invalidation can be added later.

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidateMask {
    pub color: bool,
    pub depth: bool,
    pub stencil: bool,
}

#[wasm_bindgen]
impl InvalidateMask {
    pub fn none() -> Self {
        Self {
            color: false,
            depth: false,
            stencil: false,
        }
    }
    pub fn all() -> Self {
        Self {
            color: true,
            depth: true,
            stencil: true,
        }
    }
    pub fn color() -> Self {
        Self {
            color: true,
            depth: false,
            stencil: false,
        }
    }
    pub fn depth() -> Self {
        Self {
            color: false,
            depth: true,
            stencil: false,
        }
    }
    pub fn stencil() -> Self {
        Self {
            color: false,
            depth: false,
            stencil: true,
        }
    }
    pub fn depth_stencil() -> Self {
        Self {
            color: false,
            depth: true,
            stencil: true,
        }
    }
    pub fn color_depth_stencil() -> Self {
        Self::all()
    }
}

// ---------------------------------------------------------------------------
// DefaultFramebuffer
// ---------------------------------------------------------------------------

struct DefaultFramebufferInner {
    viewport: Viewport,
    clear_color: [f32; 4],
    clear_depth: f32,
    clear_stencil: i32,
    // Present only when the context was created from a canvas — needed to size
    // the drawing buffer on auto-resize.
    canvas: Option<HtmlCanvasElement>,
    observer: Option<ResizeObserver>,
    // Kept alive so the ResizeObserver callback stays valid; dropped on disable.
    #[allow(dead_code)]
    resize_closure: Option<Closure<dyn FnMut()>>,
}

// Resizes the drawing buffer to the canvas's CSS size × devicePixelRatio and
// updates the viewport. Only touches the backing store when it actually changed
// (setting width/height clears the canvas).
fn resize_to_display(inner: &Rc<RefCell<DefaultFramebufferInner>>, canvas: &HtmlCanvasElement) {
    let dpr = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0);
    let w = (canvas.client_width().max(0) as f64 * dpr).round() as i32;
    let h = (canvas.client_height().max(0) as f64 * dpr).round() as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    if canvas.width() != w as u32 {
        canvas.set_width(w as u32);
    }
    if canvas.height() != h as u32 {
        canvas.set_height(h as u32);
    }
    inner.borrow_mut().viewport = Viewport::new(0, 0, w, h);
}

#[wasm_bindgen]
pub struct DefaultFramebuffer {
    inner: Rc<RefCell<DefaultFramebufferInner>>,
}

impl DefaultFramebuffer {
    pub(crate) fn new(viewport: Viewport, canvas: Option<HtmlCanvasElement>) -> Self {
        Self {
            inner: Rc::new(RefCell::new(DefaultFramebufferInner {
                viewport,
                clear_color: [0.0, 0.0, 0.0, 1.0],
                clear_depth: 1.0,
                clear_stencil: 0,
                canvas,
                observer: None,
                resize_closure: None,
            })),
        }
    }

    pub(crate) fn handle(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }

    pub(crate) fn viewport(&self) -> Viewport {
        self.inner.borrow().viewport
    }

    pub(crate) fn clear_color_rgba(&self) -> [f32; 4] {
        self.inner.borrow().clear_color
    }

    pub(crate) fn clear_depth_value(&self) -> f32 {
        self.inner.borrow().clear_depth
    }

    pub(crate) fn clear_stencil_value(&self) -> i32 {
        self.inner.borrow().clear_stencil
    }
}

#[wasm_bindgen]
impl DefaultFramebuffer {
    pub fn set_viewport(&self, v: Viewport) {
        self.inner.borrow_mut().viewport = v;
    }

    pub fn set_clear_color(&self, r: f32, g: f32, b: f32, a: f32) {
        self.inner.borrow_mut().clear_color = [r, g, b, a];
    }

    pub fn set_clear_depth(&self, d: f32) {
        self.inner.borrow_mut().clear_depth = d;
    }

    pub fn set_clear_stencil(&self, s: i32) {
        self.inner.borrow_mut().clear_stencil = s;
    }

    /// Starts observing the canvas and keeps the drawing buffer sized to its CSS
    /// size × devicePixelRatio, updating the viewport. Fires once immediately for
    /// the current size. No-op if already enabled; errors if there is no canvas
    /// (context created from a raw gl). Opt-in — call `disable_auto_resize` to stop.
    pub fn enable_auto_resize(&self) -> Result<(), String> {
        let mut s = self.inner.borrow_mut();
        if s.observer.is_some() {
            return Ok(());
        }
        let canvas = s
            .canvas
            .clone()
            .ok_or("auto-resize needs a canvas (context was created from a raw gl)")?;

        // Weak so the closure (owned by the inner) doesn't keep the inner alive.
        let weak = Rc::downgrade(&self.inner);
        let cb_canvas = canvas.clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            if let Some(inner) = weak.upgrade() {
                resize_to_display(&inner, &cb_canvas);
            }
        });
        let observer = ResizeObserver::new(closure.as_ref().unchecked_ref())
            .map_err(|_| "ResizeObserver is not supported")?;
        observer.observe(&canvas);

        s.observer = Some(observer);
        s.resize_closure = Some(closure);
        Ok(())
    }

    pub fn disable_auto_resize(&self) {
        let mut s = self.inner.borrow_mut();
        if let Some(observer) = s.observer.take() {
            observer.disconnect();
        }
        s.resize_closure = None;
    }

    pub fn is_auto_resize_enabled(&self) -> bool {
        self.inner.borrow().observer.is_some()
    }
}

// ---------------------------------------------------------------------------
// Attachments
// ---------------------------------------------------------------------------
//
// Internal enums (wasm_bindgen can't carry data through variants, so the public
// surface is builder methods on Framebuffer instead). Each variant *owns* a
// clone of the texture/renderbuffer — the implicit "FBO references this object"
// GL binding becomes explicit Rust ownership, and the resource lives at least as
// long as the FBO via its RefCount. The three texture variants encode the only
// three ways WebGL2 attaches a texture: a 2D image, a cube-map face, or a layer
// of a 3D / 2D-array texture.

enum ColorAttachment {
    Texture2D {
        tex: Texture,
        level: i32,
    },
    CubeFace {
        tex: Texture,
        face: CubeMapFace,
        level: i32,
    },
    Layer {
        tex: Texture,
        level: i32,
        layer: i32,
    },
    Renderbuffer(Renderbuffer),
}

impl ColorAttachment {
    fn texture(&self) -> Option<Texture> {
        match self {
            ColorAttachment::Texture2D { tex, .. }
            | ColorAttachment::CubeFace { tex, .. }
            | ColorAttachment::Layer { tex, .. } => Some(tex.clone()),
            ColorAttachment::Renderbuffer(_) => None,
        }
    }

    fn renderbuffer(&self) -> Option<Renderbuffer> {
        match self {
            ColorAttachment::Renderbuffer(rb) => Some(rb.clone()),
            _ => None,
        }
    }
}

// `stencil` flags a combined depth-stencil attachment (DEPTH_STENCIL_ATTACHMENT)
// vs depth-only (DEPTH_ATTACHMENT).
enum DepthAttachment {
    Texture {
        tex: Texture,
        level: i32,
        stencil: bool,
    },
    Renderbuffer {
        rb: Renderbuffer,
        stencil: bool,
    },
}

impl DepthAttachment {
    fn is_stencil(&self) -> bool {
        match self {
            DepthAttachment::Texture { stencil, .. }
            | DepthAttachment::Renderbuffer { stencil, .. } => *stencil,
        }
    }

    fn attachment_point(&self) -> u32 {
        if self.is_stencil() {
            WebGl2RenderingContext::DEPTH_STENCIL_ATTACHMENT
        } else {
            WebGl2RenderingContext::DEPTH_ATTACHMENT
        }
    }

    fn texture(&self) -> Option<Texture> {
        match self {
            DepthAttachment::Texture { tex, .. } => Some(tex.clone()),
            _ => None,
        }
    }

    fn renderbuffer(&self) -> Option<Renderbuffer> {
        match self {
            DepthAttachment::Renderbuffer { rb, .. } => Some(rb.clone()),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Framebuffer
// ---------------------------------------------------------------------------

// Per-color-attachment clear value. The variant must match the attachment's
// internal format kind — clearing a float target with `Int` is a GL error — so
// the user picks the type via the corresponding setter.
#[derive(Clone, Copy)]
enum ColorClearValue {
    Float([f32; 4]),
    Int([i32; 4]),
    Uint([u32; 4]),
}

impl Default for ColorClearValue {
    fn default() -> Self {
        ColorClearValue::Float([0.0, 0.0, 0.0, 1.0])
    }
}

struct FramebufferInner {
    gl: WebGl2RenderingContext,
    raw: WebGlFramebuffer,
    width: i32,
    height: i32,
    viewport: Viewport,
    color: Vec<Option<ColorAttachment>>,
    depth: Option<DepthAttachment>,
    // Indexed in lockstep with `color`; missing entries default to opaque black.
    color_clear: Vec<ColorClearValue>,
    clear_depth: f32,
    clear_stencil: i32,
    // Attachments changed since the last realize? The Renderer applies them to GL
    // (while this FBO is bound) before using it — see `realize_if_dirty`.
    dirty: bool,
}

impl Drop for FramebufferInner {
    fn drop(&mut self) {
        self.gl.delete_framebuffer(Some(&self.raw));
    }
}

// All of the following assume the FBO is *already bound* — they only issue
// attach/detach calls, never bind. Binding is the Renderer's job exclusively, so
// these can never desync its tracked state.

fn apply_color(gl: &WebGl2RenderingContext, point: u32, att: &ColorAttachment) {
    match att {
        ColorAttachment::Texture2D { tex, level } => gl.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            WebGl2RenderingContext::TEXTURE_2D,
            Some(tex.raw_gl()),
            *level,
        ),
        ColorAttachment::CubeFace { tex, face, level } => gl.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            face.as_gl(),
            Some(tex.raw_gl()),
            *level,
        ),
        ColorAttachment::Layer { tex, level, layer } => gl.framebuffer_texture_layer(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            Some(tex.raw_gl()),
            *level,
            *layer,
        ),
        ColorAttachment::Renderbuffer(rb) => gl.framebuffer_renderbuffer(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            WebGl2RenderingContext::RENDERBUFFER,
            Some(rb.raw_gl()),
        ),
    }
}

fn apply_depth(gl: &WebGl2RenderingContext, depth: &DepthAttachment) {
    let point = depth.attachment_point();
    match depth {
        DepthAttachment::Texture { tex, level, .. } => gl.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            WebGl2RenderingContext::TEXTURE_2D,
            Some(tex.raw_gl()),
            *level,
        ),
        DepthAttachment::Renderbuffer { rb, .. } => gl.framebuffer_renderbuffer(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            WebGl2RenderingContext::RENDERBUFFER,
            Some(rb.raw_gl()),
        ),
    }
}

// `draw_buffers` is per-FBO state, so it's part of realization too.
fn sync_draw_buffers(inner: &FramebufferInner, gl: &WebGl2RenderingContext) {
    let arr = js_sys::Array::new();
    if inner.color.iter().all(|s| s.is_none()) {
        // Depth/stencil-only (or empty) FBO: a single NONE tells GL there's no
        // color output, otherwise the default [COLOR_ATTACHMENT0] would dangle.
        arr.push(&JsValue::from(WebGl2RenderingContext::NONE));
    } else {
        for (i, slot) in inner.color.iter().enumerate() {
            let v = if slot.is_some() {
                WebGl2RenderingContext::COLOR_ATTACHMENT0 + i as u32
            } else {
                WebGl2RenderingContext::NONE
            };
            arr.push(&JsValue::from(v));
        }
    }
    gl.draw_buffers(arr.as_ref());
}

fn store_color(inner: &mut FramebufferInner, index: u32, att: ColorAttachment) {
    let idx = index as usize;
    if idx >= inner.color.len() {
        inner.color.resize_with(idx + 1, || None);
    }
    inner.color[idx] = Some(att);
}

fn set_color_clear(inner: &mut FramebufferInner, index: u32, val: ColorClearValue) {
    let idx = index as usize;
    if idx >= inner.color_clear.len() {
        inner
            .color_clear
            .resize(idx + 1, ColorClearValue::default());
    }
    inner.color_clear[idx] = val;
}

// Clears the bound FBO per-attachment with the correctly typed clearBuffer* call.
// (Float/Int/Uint color, float depth, int stencil, fused depth-stencil.)
fn clear_framebuffer(inner: &FramebufferInner, gl: &WebGl2RenderingContext, mask: ClearMask) {
    if mask.color {
        for (i, slot) in inner.color.iter().enumerate() {
            if slot.is_none() {
                continue;
            }
            let drawbuffer = i as i32;
            match inner.color_clear.get(i).copied().unwrap_or_default() {
                ColorClearValue::Float(v) => {
                    gl.clear_bufferfv_with_f32_array(WebGl2RenderingContext::COLOR, drawbuffer, &v)
                }
                ColorClearValue::Int(v) => {
                    gl.clear_bufferiv_with_i32_array(WebGl2RenderingContext::COLOR, drawbuffer, &v)
                }
                ColorClearValue::Uint(v) => {
                    gl.clear_bufferuiv_with_u32_array(WebGl2RenderingContext::COLOR, drawbuffer, &v)
                }
            }
        }
    }
    // One fused call when both are wanted (also correct for separate depth/stencil
    // attachments); otherwise the individual typed clears.
    if mask.depth && mask.stencil {
        gl.clear_bufferfi(
            WebGl2RenderingContext::DEPTH_STENCIL,
            0,
            inner.clear_depth,
            inner.clear_stencil,
        );
    } else if mask.depth {
        gl.clear_bufferfv_with_f32_array(WebGl2RenderingContext::DEPTH, 0, &[inner.clear_depth]);
    } else if mask.stencil {
        gl.clear_bufferiv_with_i32_array(
            WebGl2RenderingContext::STENCIL,
            0,
            &[inner.clear_stencil],
        );
    }
}

#[wasm_bindgen]
pub struct Framebuffer {
    inner: Rc<RefCell<FramebufferInner>>,
}

impl Clone for Framebuffer {
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }
}

impl PartialEq for Framebuffer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner) || self.inner.borrow().raw == other.inner.borrow().raw
    }
}

impl Eq for Framebuffer {}

impl Framebuffer {
    pub(crate) fn new(
        gl: &WebGl2RenderingContext,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let raw = gl.create_framebuffer().ok_or("createFramebuffer failed")?;
        Ok(Self {
            inner: Rc::new(RefCell::new(FramebufferInner {
                gl: gl.clone(),
                raw,
                width,
                height,
                viewport: Viewport::new(0, 0, width, height),
                color: Vec::new(),
                depth: None,
                color_clear: Vec::new(),
                clear_depth: 1.0,
                clear_stencil: 0,
                dirty: false,
            })),
        })
    }

    pub(crate) fn raw_gl(&self) -> WebGlFramebuffer {
        self.inner.borrow().raw.clone()
    }

    pub(crate) fn viewport(&self) -> Viewport {
        self.inner.borrow().viewport
    }

    // Applies pending attachment changes to GL. The FBO must already be bound by
    // the caller (the Renderer). Cheap no-op when nothing changed.
    pub(crate) fn realize_if_dirty(&self, gl: &WebGl2RenderingContext) {
        let mut s = self.inner.borrow_mut();
        if !s.dirty {
            return;
        }
        // Color: apply each slot, explicitly detaching empty ones so a removed
        // attachment doesn't linger in GL state.
        for i in 0..s.color.len() {
            let point = WebGl2RenderingContext::COLOR_ATTACHMENT0 + i as u32;
            match &s.color[i] {
                Some(att) => apply_color(gl, point, att),
                None => gl.framebuffer_texture_2d(
                    WebGl2RenderingContext::FRAMEBUFFER,
                    point,
                    WebGl2RenderingContext::TEXTURE_2D,
                    None,
                    0,
                ),
            }
        }
        // Depth/stencil: clear both points first, then attach the current one.
        gl.framebuffer_renderbuffer(
            WebGl2RenderingContext::FRAMEBUFFER,
            WebGl2RenderingContext::DEPTH_ATTACHMENT,
            WebGl2RenderingContext::RENDERBUFFER,
            None,
        );
        gl.framebuffer_renderbuffer(
            WebGl2RenderingContext::FRAMEBUFFER,
            WebGl2RenderingContext::DEPTH_STENCIL_ATTACHMENT,
            WebGl2RenderingContext::RENDERBUFFER,
            None,
        );
        if let Some(depth) = &s.depth {
            apply_depth(gl, depth);
        }
        sync_draw_buffers(&s, gl);
        s.dirty = false;
    }

    // Clears this FBO's attachments per the mask, using each attachment's typed
    // clear value. The FBO must already be bound (the Renderer ensures this).
    pub(crate) fn clear(&self, gl: &WebGl2RenderingContext, mask: ClearMask) {
        clear_framebuffer(&self.inner.borrow(), gl, mask);
    }

    // Pushes the GL attachment-point enums selected by `mask` (only those that
    // are actually attached) into `arr`, for invalidateFramebuffer.
    pub(crate) fn collect_invalidate_attachments(&self, arr: &js_sys::Array, mask: InvalidateMask) {
        let s = self.inner.borrow();
        if mask.color {
            for (i, slot) in s.color.iter().enumerate() {
                if slot.is_some() {
                    arr.push(&JsValue::from(
                        WebGl2RenderingContext::COLOR_ATTACHMENT0 + i as u32,
                    ));
                }
            }
        }
        if let Some(depth) = &s.depth {
            if depth.is_stencil() {
                if mask.depth || mask.stencil {
                    arr.push(&JsValue::from(
                        WebGl2RenderingContext::DEPTH_STENCIL_ATTACHMENT,
                    ));
                }
            } else if mask.depth {
                arr.push(&JsValue::from(WebGl2RenderingContext::DEPTH_ATTACHMENT));
            }
        }
    }
}

#[wasm_bindgen]
impl Framebuffer {
    pub fn width(&self) -> i32 {
        self.inner.borrow().width
    }

    pub fn height(&self) -> i32 {
        self.inner.borrow().height
    }

    /// Viewport applied when this FBO is the render target (defaults to its full
    /// size). Set it to render into a sub-region.
    pub fn set_viewport(&self, v: Viewport) {
        self.inner.borrow_mut().viewport = v;
    }

    // --- color attachments (recorded only; applied by the Renderer on bind) ---

    pub fn attach_color_texture_2d(&self, index: u32, tex: &Texture, level: i32) {
        let mut s = self.inner.borrow_mut();
        store_color(
            &mut s,
            index,
            ColorAttachment::Texture2D {
                tex: tex.clone(),
                level,
            },
        );
        s.dirty = true;
    }

    pub fn attach_color_cube_face(&self, index: u32, tex: &Texture, face: CubeMapFace, level: i32) {
        let mut s = self.inner.borrow_mut();
        store_color(
            &mut s,
            index,
            ColorAttachment::CubeFace {
                tex: tex.clone(),
                face,
                level,
            },
        );
        s.dirty = true;
    }

    pub fn attach_color_layer(&self, index: u32, tex: &Texture, level: i32, layer: i32) {
        let mut s = self.inner.borrow_mut();
        store_color(
            &mut s,
            index,
            ColorAttachment::Layer {
                tex: tex.clone(),
                level,
                layer,
            },
        );
        s.dirty = true;
    }

    pub fn attach_color_renderbuffer(&self, index: u32, rb: &Renderbuffer) {
        let mut s = self.inner.borrow_mut();
        store_color(&mut s, index, ColorAttachment::Renderbuffer(rb.clone()));
        s.dirty = true;
    }

    pub fn detach_color(&self, index: u32) {
        let mut s = self.inner.borrow_mut();
        let idx = index as usize;
        if idx < s.color.len() {
            s.color[idx] = None;
        }
        s.dirty = true;
    }

    // --- depth / stencil attachments ---

    pub fn attach_depth_texture(&self, tex: &Texture, level: i32) {
        self.attach_depth_texture_impl(tex, level, false);
    }

    pub fn attach_depth_stencil_texture(&self, tex: &Texture, level: i32) {
        self.attach_depth_texture_impl(tex, level, true);
    }

    pub fn attach_depth_renderbuffer(&self, rb: &Renderbuffer) {
        self.attach_depth_renderbuffer_impl(rb, false);
    }

    pub fn attach_depth_stencil_renderbuffer(&self, rb: &Renderbuffer) {
        self.attach_depth_renderbuffer_impl(rb, true);
    }

    pub fn detach_depth(&self) {
        let mut s = self.inner.borrow_mut();
        s.depth = None;
        s.dirty = true;
    }

    // --- getters (return owned clones; None if that slot holds the other kind) ---

    pub fn color_texture(&self, index: u32) -> Option<Texture> {
        self.inner
            .borrow()
            .color
            .get(index as usize)
            .and_then(|o| o.as_ref())
            .and_then(|a| a.texture())
    }

    pub fn color_renderbuffer(&self, index: u32) -> Option<Renderbuffer> {
        self.inner
            .borrow()
            .color
            .get(index as usize)
            .and_then(|o| o.as_ref())
            .and_then(|a| a.renderbuffer())
    }

    pub fn depth_texture(&self) -> Option<Texture> {
        self.inner.borrow().depth.as_ref().and_then(|d| d.texture())
    }

    pub fn depth_renderbuffer(&self) -> Option<Renderbuffer> {
        self.inner
            .borrow()
            .depth
            .as_ref()
            .and_then(|d| d.renderbuffer())
    }

    // --- clear values (applied per-attachment by Renderer::clear) ---

    /// Float/normalized color clear for attachment `index` (the common case).
    pub fn set_color_clear_f32(&self, index: u32, r: f32, g: f32, b: f32, a: f32) {
        set_color_clear(
            &mut self.inner.borrow_mut(),
            index,
            ColorClearValue::Float([r, g, b, a]),
        );
    }

    /// Signed-integer color clear, for attachments with an integer internal format.
    pub fn set_color_clear_i32(&self, index: u32, r: i32, g: i32, b: i32, a: i32) {
        set_color_clear(
            &mut self.inner.borrow_mut(),
            index,
            ColorClearValue::Int([r, g, b, a]),
        );
    }

    /// Unsigned-integer color clear, for attachments with an unsigned integer format.
    pub fn set_color_clear_u32(&self, index: u32, r: u32, g: u32, b: u32, a: u32) {
        set_color_clear(
            &mut self.inner.borrow_mut(),
            index,
            ColorClearValue::Uint([r, g, b, a]),
        );
    }

    /// Shorthand for `set_color_clear_f32(0, ..)`.
    pub fn set_clear_color(&self, r: f32, g: f32, b: f32, a: f32) {
        set_color_clear(
            &mut self.inner.borrow_mut(),
            0,
            ColorClearValue::Float([r, g, b, a]),
        );
    }

    pub fn set_clear_depth(&self, d: f32) {
        self.inner.borrow_mut().clear_depth = d;
    }

    pub fn set_clear_stencil(&self, s: i32) {
        self.inner.borrow_mut().clear_stencil = s;
    }
}

impl Framebuffer {
    fn attach_depth_texture_impl(&self, tex: &Texture, level: i32, stencil: bool) {
        let mut s = self.inner.borrow_mut();
        s.depth = Some(DepthAttachment::Texture {
            tex: tex.clone(),
            level,
            stencil,
        });
        s.dirty = true;
    }

    fn attach_depth_renderbuffer_impl(&self, rb: &Renderbuffer, stencil: bool) {
        let mut s = self.inner.borrow_mut();
        s.depth = Some(DepthAttachment::Renderbuffer {
            rb: rb.clone(),
            stencil,
        });
        s.dirty = true;
    }
}

pub(crate) fn framebuffer_status_str(status: u32) -> &'static str {
    match status {
        0x8CD6 => "FRAMEBUFFER_INCOMPLETE_ATTACHMENT",
        0x8CD7 => "FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT",
        0x8CD9 => "FRAMEBUFFER_INCOMPLETE_DIMENSIONS",
        0x8CDD => "FRAMEBUFFER_UNSUPPORTED",
        0x8D56 => "FRAMEBUFFER_INCOMPLETE_MULTISAMPLE",
        _ => "FRAMEBUFFER_INCOMPLETE (unknown status)",
    }
}
