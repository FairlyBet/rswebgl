use crate::error::Error;
use std::cell::RefCell;
use std::rc::Rc;

use rswebgl_foundation::pool::{Destroyed, Pool};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, ResizeObserver, WebGl2RenderingContext, WebGlFramebuffer};

use crate::console;
use crate::context::{ColorSpace, Context, Resources, stale};
use crate::draw::Viewport;
use crate::ids::{FramebufferId, RenderbufferId, TextureId};
use crate::limits;
use crate::renderbuffer::RenderbufferObj;
use crate::texture::{CubeMapFace, TextureObj};

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
    #[wasm_bindgen(js_name = colorDepth)]
    pub fn color_depth() -> Self {
        Self {
            color: true,
            depth: true,
            stencil: false,
        }
    }
    #[wasm_bindgen(js_name = colorDepthStencil)]
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
    #[wasm_bindgen(js_name = depthStencil)]
    pub fn depth_stencil() -> Self {
        Self {
            color: false,
            depth: true,
            stencil: true,
        }
    }
    #[wasm_bindgen(js_name = colorDepthStencil)]
    pub fn color_depth_stencil() -> Self {
        Self::all()
    }
}

// ---------------------------------------------------------------------------
// DefaultFramebuffer
// ---------------------------------------------------------------------------

struct DefaultFramebufferInner {
    gl: WebGl2RenderingContext,
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
    pub(crate) fn new(
        gl: WebGl2RenderingContext,
        viewport: Viewport,
        canvas: Option<HtmlCanvasElement>,
    ) -> Self {
        Self {
            inner: Rc::new(RefCell::new(DefaultFramebufferInner {
                gl,
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
    /// Current drawing-buffer width in device pixels (tracks auto-resize).
    pub fn width(&self) -> i32 {
        self.inner.borrow().viewport.width
    }

    /// Current drawing-buffer height in device pixels (tracks auto-resize).
    pub fn height(&self) -> i32 {
        self.inner.borrow().viewport.height
    }

    #[wasm_bindgen(js_name = setViewport)]
    pub fn set_viewport(&self, v: Viewport) {
        self.inner.borrow_mut().viewport = v;
    }

    #[wasm_bindgen(js_name = setClearColor)]
    pub fn set_clear_color(&self, r: f32, g: f32, b: f32, a: f32) {
        self.inner.borrow_mut().clear_color = [r, g, b, a];
    }

    #[wasm_bindgen(js_name = setClearDepth)]
    pub fn set_clear_depth(&self, d: f32) {
        self.inner.borrow_mut().clear_depth = d;
    }

    #[wasm_bindgen(js_name = setClearStencil)]
    pub fn set_clear_stencil(&self, s: i32) {
        self.inner.borrow_mut().clear_stencil = s;
    }

    /// Starts observing the canvas and keeps the drawing buffer sized to its CSS
    /// size × devicePixelRatio, updating the viewport. Fires once immediately for
    /// the current size. No-op if already enabled; errors if there is no canvas
    /// (context created from a raw gl). Opt-in — call `disable_auto_resize` to stop.
    #[wasm_bindgen(js_name = enableAutoResize)]
    pub fn enable_auto_resize(&self) -> Result<(), Error> {
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

    #[wasm_bindgen(js_name = disableAutoResize)]
    pub fn disable_auto_resize(&self) {
        let mut s = self.inner.borrow_mut();
        if let Some(observer) = s.observer.take() {
            observer.disconnect();
        }
        s.resize_closure = None;
    }

    #[wasm_bindgen(js_name = isAutoResizeEnabled)]
    pub fn is_auto_resize_enabled(&self) -> bool {
        self.inner.borrow().observer.is_some()
    }

    /// Color space of the drawing buffer (`drawingBufferColorSpace`). `Srgb`
    /// (default) or `DisplayP3` for wide-gamut output. Invalid values are ignored
    /// by the browser, leaving it unchanged.
    #[wasm_bindgen(js_name = setDrawingBufferColorSpace)]
    pub fn set_drawing_buffer_color_space(&self, space: ColorSpace) {
        let s = self.inner.borrow();
        let _ = js_sys::Reflect::set(
            s.gl.as_ref(),
            &JsValue::from_str("drawingBufferColorSpace"),
            &JsValue::from_str(space.as_str()),
        );
    }

    #[wasm_bindgen(js_name = drawingBufferColorSpace)]
    pub fn drawing_buffer_color_space(&self) -> ColorSpace {
        let s = self.inner.borrow();
        let v = js_sys::Reflect::get(s.gl.as_ref(), &JsValue::from_str("drawingBufferColorSpace"))
            .ok()
            .and_then(|v| v.as_string())
            .unwrap_or_default();
        ColorSpace::from_js(&v)
    }
}

// ---------------------------------------------------------------------------
// Attachments
// ---------------------------------------------------------------------------
//
// Internal enums (wasm_bindgen can't carry data through variants, so the public
// surface is `framebuffer_attach_*` methods instead). An attached texture or
// renderbuffer is *in use* by the framebuffer (a use count in its pool):
// destroying it while attached is deferred until the framebuffer lets go. The
// three texture variants encode the only three ways WebGL2 attaches a texture:
// a 2D image, a cube-map face, or a layer of a 3D / 2D-array texture.

#[derive(Clone, Copy)]
enum ColorAttachment {
    Texture2D {
        tex: TextureId,
        level: i32,
    },
    CubeFace {
        tex: TextureId,
        face: CubeMapFace,
        level: i32,
    },
    Layer {
        tex: TextureId,
        level: i32,
        layer: i32,
    },
    Renderbuffer(RenderbufferId),
}

// What an attachment keeps in use.
#[derive(Clone, Copy)]
enum Source {
    Texture(TextureId),
    Renderbuffer(RenderbufferId),
}

impl ColorAttachment {
    fn source(&self) -> Source {
        match *self {
            ColorAttachment::Texture2D { tex, .. }
            | ColorAttachment::CubeFace { tex, .. }
            | ColorAttachment::Layer { tex, .. } => Source::Texture(tex),
            ColorAttachment::Renderbuffer(rb) => Source::Renderbuffer(rb),
        }
    }
}

// `stencil` flags a combined depth-stencil attachment (DEPTH_STENCIL_ATTACHMENT)
// vs depth-only (DEPTH_ATTACHMENT).
#[derive(Clone, Copy)]
enum DepthAttachment {
    Texture {
        tex: TextureId,
        level: i32,
        stencil: bool,
    },
    Renderbuffer {
        rb: RenderbufferId,
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

    fn source(&self) -> Source {
        match *self {
            DepthAttachment::Texture { tex, .. } => Source::Texture(tex),
            DepthAttachment::Renderbuffer { rb, .. } => Source::Renderbuffer(rb),
        }
    }
}

// Starts using an attachment source; false if its handle is stale.
fn retain(
    src: Source,
    textures: &mut Pool<TextureObj>,
    renderbuffers: &mut Pool<RenderbufferObj>,
    op: &str,
) -> bool {
    let ok = match src {
        Source::Texture(t) => textures.retain(t.0),
        Source::Renderbuffer(r) => renderbuffers.retain(r.0),
    };
    if !ok {
        match src {
            Source::Texture(t) => stale("texture", op, t),
            Source::Renderbuffer(r) => stale("renderbuffer", op, r),
        }
    }
    ok
}

// Stops using an attachment source; deletes it if it was destroyed meanwhile.
fn release(
    src: Source,
    textures: &mut Pool<TextureObj>,
    renderbuffers: &mut Pool<RenderbufferObj>,
    gl: &WebGl2RenderingContext,
) {
    match src {
        Source::Texture(t) => {
            if let Some(obj) = textures.release(t.0) {
                obj.delete(gl);
            }
        }
        Source::Renderbuffer(r) => {
            if let Some(obj) = renderbuffers.release(r.0) {
                obj.delete(gl);
            }
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

/// A framebuffer in the context's pool.
pub(crate) struct FramebufferObj {
    pub(crate) raw: WebGlFramebuffer,
    width: i32,
    height: i32,
    pub(crate) viewport: Viewport,
    color: Vec<Option<ColorAttachment>>,
    depth: Option<DepthAttachment>,
    // Indexed in lockstep with `color`; missing entries default to opaque black.
    color_clear: Vec<ColorClearValue>,
    clear_depth: f32,
    clear_stencil: i32,
}

// Attach/detach are eager: they bind this FBO and issue the GL call immediately,
// leaving it bound. In WebGL *any* object mutates global state, so there is no
// point pretending otherwise — instead each public method that binds tells the
// renderer's state cache (`forget_target`), so the next pass re-binds its
// target. The helpers below assume the FBO is already bound; the public
// methods bind first.

fn apply_color(
    gl: &WebGl2RenderingContext,
    point: u32,
    att: &ColorAttachment,
    textures: &Pool<TextureObj>,
    renderbuffers: &Pool<RenderbufferObj>,
) {
    let tex = |t: TextureId| textures.get(t.0).map(|o| &o.raw);
    match *att {
        ColorAttachment::Texture2D { tex: t, level } => gl.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            WebGl2RenderingContext::TEXTURE_2D,
            tex(t),
            level,
        ),
        ColorAttachment::CubeFace {
            tex: t,
            face,
            level,
        } => gl.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            face.as_gl(),
            tex(t),
            level,
        ),
        ColorAttachment::Layer {
            tex: t,
            level,
            layer,
        } => gl.framebuffer_texture_layer(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            tex(t),
            level,
            layer,
        ),
        ColorAttachment::Renderbuffer(rb) => gl.framebuffer_renderbuffer(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            WebGl2RenderingContext::RENDERBUFFER,
            renderbuffers.get(rb.0).map(|o| &o.raw),
        ),
    }
}

fn apply_depth(
    gl: &WebGl2RenderingContext,
    depth: &DepthAttachment,
    textures: &Pool<TextureObj>,
    renderbuffers: &Pool<RenderbufferObj>,
) {
    let point = depth.attachment_point();
    match *depth {
        DepthAttachment::Texture { tex, level, .. } => gl.framebuffer_texture_2d(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            WebGl2RenderingContext::TEXTURE_2D,
            textures.get(tex.0).map(|o| &o.raw),
            level,
        ),
        DepthAttachment::Renderbuffer { rb, .. } => gl.framebuffer_renderbuffer(
            WebGl2RenderingContext::FRAMEBUFFER,
            point,
            WebGl2RenderingContext::RENDERBUFFER,
            renderbuffers.get(rb.0).map(|o| &o.raw),
        ),
    }
}

// `draw_buffers` is per-FBO state, so it's part of realization too.
fn sync_draw_buffers(fb: &FramebufferObj, gl: &WebGl2RenderingContext) {
    let arr = js_sys::Array::new();
    if fb.color.iter().all(|s| s.is_none()) {
        // Depth/stencil-only (or empty) FBO: a single NONE tells GL there's no
        // color output, otherwise the default [COLOR_ATTACHMENT0] would dangle.
        arr.push(&JsValue::from(WebGl2RenderingContext::NONE));
    } else {
        for (i, slot) in fb.color.iter().enumerate() {
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

// Detaches whatever sits at both depth points, so switching depth kind (or
// detaching) never leaves a stale attachment behind.
fn detach_depth_points(gl: &WebGl2RenderingContext) {
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
}

impl FramebufferObj {
    // Size of the attachments (what reads are bounded by).
    pub(crate) fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    pub(crate) fn has_color(&self, index: u32) -> bool {
        self.color.get(index as usize).is_some_and(Option::is_some)
    }

    /// The sized internal format of color attachment `index` and its sample
    /// count (0 = single-sampled), or None without one.
    pub(crate) fn color_format(&self, res: &Resources, index: u32) -> Option<(u32, i32)> {
        let a = self.color.get(index as usize)?.as_ref()?;
        match a.source() {
            Source::Texture(t) => Some((res.textures.get_in_use(t.0)?.internal.get(), 0)),
            Source::Renderbuffer(r) => {
                let rb = res.renderbuffers.get_in_use(r.0)?;
                Some((rb.internal(), rb.samples()))
            }
        }
    }

    // Clears this FBO's attachments per the mask, using each attachment's typed
    // clear value. The FBO must already be bound (the renderer ensures this).
    pub(crate) fn clear(&self, gl: &WebGl2RenderingContext, mask: ClearMask) {
        if mask.color {
            for (i, slot) in self.color.iter().enumerate() {
                if slot.is_none() {
                    continue;
                }
                let drawbuffer = i as i32;
                match self.color_clear.get(i).copied().unwrap_or_default() {
                    ColorClearValue::Float(v) => gl.clear_bufferfv_with_f32_array(
                        WebGl2RenderingContext::COLOR,
                        drawbuffer,
                        &v,
                    ),
                    ColorClearValue::Int(v) => gl.clear_bufferiv_with_i32_array(
                        WebGl2RenderingContext::COLOR,
                        drawbuffer,
                        &v,
                    ),
                    ColorClearValue::Uint(v) => gl.clear_bufferuiv_with_u32_array(
                        WebGl2RenderingContext::COLOR,
                        drawbuffer,
                        &v,
                    ),
                }
            }
        }
        // One fused call when both are wanted (also correct for separate
        // depth/stencil attachments); otherwise the individual typed clears.
        if mask.depth && mask.stencil {
            gl.clear_bufferfi(
                WebGl2RenderingContext::DEPTH_STENCIL,
                0,
                self.clear_depth,
                self.clear_stencil,
            );
        } else if mask.depth {
            gl.clear_bufferfv_with_f32_array(WebGl2RenderingContext::DEPTH, 0, &[self.clear_depth]);
        } else if mask.stencil {
            gl.clear_bufferiv_with_i32_array(
                WebGl2RenderingContext::STENCIL,
                0,
                &[self.clear_stencil],
            );
        }
    }

    // Pushes the GL attachment-point enums selected by `mask` (only those that
    // are actually attached) into `arr`, for invalidateFramebuffer.
    pub(crate) fn collect_invalidate_attachments(&self, arr: &js_sys::Array, mask: InvalidateMask) {
        if mask.color {
            for (i, slot) in self.color.iter().enumerate() {
                if slot.is_some() {
                    arr.push(&JsValue::from(
                        WebGl2RenderingContext::COLOR_ATTACHMENT0 + i as u32,
                    ));
                }
            }
        }
        if let Some(depth) = &self.depth {
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

    fn set_color_clear(&mut self, index: u32, val: ColorClearValue) {
        let idx = index as usize;
        if idx >= self.color_clear.len() {
            self.color_clear.resize(idx + 1, ColorClearValue::default());
        }
        self.color_clear[idx] = val;
    }
}

impl Context {
    // Puts `att` in color slot `index` (None detaches): retains the new source,
    // pushes the slot to GL, then releases the previous source.
    fn set_color_slot(
        &self,
        fb: FramebufferId,
        index: u32,
        att: Option<ColorAttachment>,
        op: &str,
    ) {
        if !color_index_in_range(index) {
            return;
        }
        let gl = self.gl();
        let res = &mut *self.res();
        let Resources {
            framebuffers,
            textures,
            renderbuffers,
            ..
        } = res;
        let Some(f) = framebuffers.get_mut(fb.0) else {
            return stale("framebuffer", op, fb);
        };
        // A float texture the device can't render to would leave the
        // framebuffer silently incomplete: say why instead.
        if let Some(Source::Texture(t)) = att.as_ref().map(ColorAttachment::source)
            && let Some(p) = textures
                .get(t.0)
                .and_then(|o| self.float_target_problem(o.internal.get()))
        {
            return console::error(&format!("[rswebgl] {op}: {p}; ignored"));
        }
        if let Some(a) = &att
            && !retain(a.source(), textures, renderbuffers, op)
        {
            return;
        }

        let idx = index as usize;
        if idx >= f.color.len() {
            f.color.resize(idx + 1, None);
        }
        let prev = std::mem::replace(&mut f.color[idx], att);

        self.gl_state().forget_target();
        gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&f.raw));
        let point = WebGl2RenderingContext::COLOR_ATTACHMENT0 + index;
        match &f.color[idx] {
            Some(a) => apply_color(gl, point, a, textures, renderbuffers),
            None => gl.framebuffer_texture_2d(
                WebGl2RenderingContext::FRAMEBUFFER,
                point,
                WebGl2RenderingContext::TEXTURE_2D,
                None,
                0,
            ),
        }
        sync_draw_buffers(f, gl);

        if let Some(p) = prev {
            release(p.source(), textures, renderbuffers, gl);
        }
    }

    // Same for the depth / depth-stencil slot.
    fn set_depth_slot(&self, fb: FramebufferId, att: Option<DepthAttachment>, op: &str) {
        let gl = self.gl();
        let res = &mut *self.res();
        let Resources {
            framebuffers,
            textures,
            renderbuffers,
            ..
        } = res;
        let Some(f) = framebuffers.get_mut(fb.0) else {
            return stale("framebuffer", op, fb);
        };
        if let Some(a) = &att
            && !retain(a.source(), textures, renderbuffers, op)
        {
            return;
        }
        let prev = std::mem::replace(&mut f.depth, att);

        self.gl_state().forget_target();
        gl.bind_framebuffer(WebGl2RenderingContext::FRAMEBUFFER, Some(&f.raw));
        detach_depth_points(gl);
        if let Some(d) = &f.depth {
            apply_depth(gl, d, textures, renderbuffers);
        }

        if let Some(p) = prev {
            release(p.source(), textures, renderbuffers, gl);
        }
    }

    fn with_framebuffer<T>(
        &self,
        fb: FramebufferId,
        op: &str,
        f: impl FnOnce(&mut FramebufferObj) -> T,
    ) -> Option<T> {
        let mut res = self.res();
        match res.framebuffers.get_mut(fb.0) {
            Some(obj) => Some(f(obj)),
            None => {
                stale("framebuffer", op, fb);
                None
            }
        }
    }
}

#[wasm_bindgen]
impl Context {
    #[wasm_bindgen(js_name = createFramebuffer)]
    pub fn create_framebuffer(&self, width: i32, height: i32) -> Result<FramebufferId, Error> {
        self.check_alive()?;
        let raw = self
            .gl()
            .create_framebuffer()
            .ok_or("createFramebuffer failed")?;
        self.res()
            .framebuffers
            .insert(FramebufferObj {
                raw,
                width,
                height,
                viewport: Viewport::new(0, 0, width, height),
                color: Vec::new(),
                depth: None,
                color_clear: Vec::new(),
                clear_depth: 1.0,
                clear_stencil: 0,
            })
            .map(Into::into)
            .map_err(|e| Error::from(e.to_string()))
    }

    /// Deletes the framebuffer and stops using its attachments (textures and
    /// renderbuffers destroyed while attached are deleted now).
    #[wasm_bindgen(js_name = destroyFramebuffer)]
    pub fn destroy_framebuffer(&self, fb: FramebufferId) {
        let gl = self.gl();
        let res = &mut *self.res();
        let obj = match res.framebuffers.destroy(fb.0) {
            Destroyed::Now(obj) => obj,
            Destroyed::Deferred => return,
            Destroyed::Invalid => return stale("framebuffer", "destroy_framebuffer", fb),
        };
        gl.delete_framebuffer(Some(&obj.raw));
        let sources = obj
            .color
            .iter()
            .flatten()
            .map(|a| a.source())
            .chain(obj.depth.map(|d| d.source()));
        for src in sources {
            release(src, &mut res.textures, &mut res.renderbuffers, gl);
        }
    }

    /// 0 for a stale handle.
    #[wasm_bindgen(js_name = framebufferWidth)]
    pub fn framebuffer_width(&self, fb: FramebufferId) -> i32 {
        self.with_framebuffer(fb, "framebuffer_width", |f| f.width)
            .unwrap_or(0)
    }

    /// 0 for a stale handle.
    #[wasm_bindgen(js_name = framebufferHeight)]
    pub fn framebuffer_height(&self, fb: FramebufferId) -> i32 {
        self.with_framebuffer(fb, "framebuffer_height", |f| f.height)
            .unwrap_or(0)
    }

    /// Viewport applied when this FBO is the render target (defaults to its full
    /// size). Set it to render into a sub-region.
    #[wasm_bindgen(js_name = framebufferSetViewport)]
    pub fn framebuffer_set_viewport(&self, fb: FramebufferId, v: &Viewport) {
        self.with_framebuffer(fb, "framebuffer_set_viewport", |f| f.viewport = *v);
    }

    // --- color attachments (applied to GL immediately; FBO left bound) ---

    #[wasm_bindgen(js_name = framebufferAttachColorTexture2D)]
    pub fn framebuffer_attach_color_texture_2d(
        &self,
        fb: FramebufferId,
        index: u32,
        tex: TextureId,
        level: i32,
    ) {
        self.set_color_slot(
            fb,
            index,
            Some(ColorAttachment::Texture2D { tex, level }),
            "framebuffer_attach_color_texture_2d",
        );
    }

    #[wasm_bindgen(js_name = framebufferAttachColorCubeFace)]
    pub fn framebuffer_attach_color_cube_face(
        &self,
        fb: FramebufferId,
        index: u32,
        tex: TextureId,
        face: CubeMapFace,
        level: i32,
    ) {
        self.set_color_slot(
            fb,
            index,
            Some(ColorAttachment::CubeFace { tex, face, level }),
            "framebuffer_attach_color_cube_face",
        );
    }

    #[wasm_bindgen(js_name = framebufferAttachColorLayer)]
    pub fn framebuffer_attach_color_layer(
        &self,
        fb: FramebufferId,
        index: u32,
        tex: TextureId,
        level: i32,
        layer: i32,
    ) {
        self.set_color_slot(
            fb,
            index,
            Some(ColorAttachment::Layer { tex, level, layer }),
            "framebuffer_attach_color_layer",
        );
    }

    #[wasm_bindgen(js_name = framebufferAttachColorRenderbuffer)]
    pub fn framebuffer_attach_color_renderbuffer(
        &self,
        fb: FramebufferId,
        index: u32,
        rb: RenderbufferId,
    ) {
        self.set_color_slot(
            fb,
            index,
            Some(ColorAttachment::Renderbuffer(rb)),
            "framebuffer_attach_color_renderbuffer",
        );
    }

    #[wasm_bindgen(js_name = framebufferDetachColor)]
    pub fn framebuffer_detach_color(&self, fb: FramebufferId, index: u32) {
        self.set_color_slot(fb, index, None, "framebuffer_detach_color");
    }

    // --- depth / stencil attachments ---

    #[wasm_bindgen(js_name = framebufferAttachDepthTexture)]
    pub fn framebuffer_attach_depth_texture(&self, fb: FramebufferId, tex: TextureId, level: i32) {
        self.set_depth_slot(
            fb,
            Some(DepthAttachment::Texture {
                tex,
                level,
                stencil: false,
            }),
            "framebuffer_attach_depth_texture",
        );
    }

    #[wasm_bindgen(js_name = framebufferAttachDepthStencilTexture)]
    pub fn framebuffer_attach_depth_stencil_texture(
        &self,
        fb: FramebufferId,
        tex: TextureId,
        level: i32,
    ) {
        self.set_depth_slot(
            fb,
            Some(DepthAttachment::Texture {
                tex,
                level,
                stencil: true,
            }),
            "framebuffer_attach_depth_stencil_texture",
        );
    }

    #[wasm_bindgen(js_name = framebufferAttachDepthRenderbuffer)]
    pub fn framebuffer_attach_depth_renderbuffer(&self, fb: FramebufferId, rb: RenderbufferId) {
        self.set_depth_slot(
            fb,
            Some(DepthAttachment::Renderbuffer { rb, stencil: false }),
            "framebuffer_attach_depth_renderbuffer",
        );
    }

    #[wasm_bindgen(js_name = framebufferAttachDepthStencilRenderbuffer)]
    pub fn framebuffer_attach_depth_stencil_renderbuffer(
        &self,
        fb: FramebufferId,
        rb: RenderbufferId,
    ) {
        self.set_depth_slot(
            fb,
            Some(DepthAttachment::Renderbuffer { rb, stencil: true }),
            "framebuffer_attach_depth_stencil_renderbuffer",
        );
    }

    #[wasm_bindgen(js_name = framebufferDetachDepth)]
    pub fn framebuffer_detach_depth(&self, fb: FramebufferId) {
        self.set_depth_slot(fb, None, "framebuffer_detach_depth");
    }

    // --- getters (NULL if the slot is empty or holds the other kind) ---

    #[wasm_bindgen(js_name = framebufferColorTexture)]
    pub fn framebuffer_color_texture(&self, fb: FramebufferId, index: u32) -> TextureId {
        self.with_framebuffer(fb, "framebuffer_color_texture", |f| {
            match f
                .color
                .get(index as usize)
                .copied()
                .flatten()
                .map(|a| a.source())
            {
                Some(Source::Texture(t)) => t,
                _ => TextureId::NULL,
            }
        })
        .unwrap_or(TextureId::NULL)
    }

    #[wasm_bindgen(js_name = framebufferColorRenderbuffer)]
    pub fn framebuffer_color_renderbuffer(&self, fb: FramebufferId, index: u32) -> RenderbufferId {
        self.with_framebuffer(fb, "framebuffer_color_renderbuffer", |f| {
            match f
                .color
                .get(index as usize)
                .copied()
                .flatten()
                .map(|a| a.source())
            {
                Some(Source::Renderbuffer(r)) => r,
                _ => RenderbufferId::NULL,
            }
        })
        .unwrap_or(RenderbufferId::NULL)
    }

    #[wasm_bindgen(js_name = framebufferDepthTexture)]
    pub fn framebuffer_depth_texture(&self, fb: FramebufferId) -> TextureId {
        self.with_framebuffer(fb, "framebuffer_depth_texture", |f| {
            match f.depth.map(|d| d.source()) {
                Some(Source::Texture(t)) => t,
                _ => TextureId::NULL,
            }
        })
        .unwrap_or(TextureId::NULL)
    }

    #[wasm_bindgen(js_name = framebufferDepthRenderbuffer)]
    pub fn framebuffer_depth_renderbuffer(&self, fb: FramebufferId) -> RenderbufferId {
        self.with_framebuffer(fb, "framebuffer_depth_renderbuffer", |f| {
            match f.depth.map(|d| d.source()) {
                Some(Source::Renderbuffer(r)) => r,
                _ => RenderbufferId::NULL,
            }
        })
        .unwrap_or(RenderbufferId::NULL)
    }

    // --- clear values (applied per-attachment by the pass's clear) ---

    /// Float/normalized color clear for attachment `index` (the common case).
    #[wasm_bindgen(js_name = framebufferSetColorClearF32)]
    pub fn framebuffer_set_color_clear_f32(
        &self,
        fb: FramebufferId,
        index: u32,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
    ) {
        self.with_framebuffer(fb, "framebuffer_set_color_clear_f32", |f| {
            f.set_color_clear(index, ColorClearValue::Float([r, g, b, a]))
        });
    }

    /// Signed-integer color clear, for attachments with an integer internal format.
    #[wasm_bindgen(js_name = framebufferSetColorClearI32)]
    pub fn framebuffer_set_color_clear_i32(
        &self,
        fb: FramebufferId,
        index: u32,
        r: i32,
        g: i32,
        b: i32,
        a: i32,
    ) {
        self.with_framebuffer(fb, "framebuffer_set_color_clear_i32", |f| {
            f.set_color_clear(index, ColorClearValue::Int([r, g, b, a]))
        });
    }

    /// Unsigned-integer color clear, for attachments with an unsigned integer format.
    #[wasm_bindgen(js_name = framebufferSetColorClearU32)]
    pub fn framebuffer_set_color_clear_u32(
        &self,
        fb: FramebufferId,
        index: u32,
        r: u32,
        g: u32,
        b: u32,
        a: u32,
    ) {
        self.with_framebuffer(fb, "framebuffer_set_color_clear_u32", |f| {
            f.set_color_clear(index, ColorClearValue::Uint([r, g, b, a]))
        });
    }

    /// Shorthand for `framebuffer_set_color_clear_f32(fb, 0, ..)`.
    #[wasm_bindgen(js_name = framebufferSetClearColor)]
    pub fn framebuffer_set_clear_color(&self, fb: FramebufferId, r: f32, g: f32, b: f32, a: f32) {
        self.framebuffer_set_color_clear_f32(fb, 0, r, g, b, a);
    }

    #[wasm_bindgen(js_name = framebufferSetClearDepth)]
    pub fn framebuffer_set_clear_depth(&self, fb: FramebufferId, d: f32) {
        self.with_framebuffer(fb, "framebuffer_set_clear_depth", |f| f.clear_depth = d);
    }

    #[wasm_bindgen(js_name = framebufferSetClearStencil)]
    pub fn framebuffer_set_clear_stencil(&self, fb: FramebufferId, s: i32) {
        self.with_framebuffer(fb, "framebuffer_set_clear_stencil", |f| f.clear_stencil = s);
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

// Color slots must exist (MAX_COLOR_ATTACHMENTS), and since every attached slot
// is listed in drawBuffers, also fit MAX_DRAW_BUFFERS.
fn color_index_in_range(index: u32) -> bool {
    let max = limits::max_color_attachments().min(limits::max_draw_buffers());
    if index >= max {
        console::error(&format!(
            "[rswebgl] color attachment index {index} is out of range (MAX_COLOR_ATTACHMENTS = \
             {}, MAX_DRAW_BUFFERS = {}); ignored",
            limits::max_color_attachments(),
            limits::max_draw_buffers()
        ));
    }
    index < max
}
