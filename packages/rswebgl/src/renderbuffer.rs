use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlRenderbuffer};

use crate::ref_count::{RefCount, ref_counted};

// ---------------------------------------------------------------------------
// RenderbufferFormat
// ---------------------------------------------------------------------------
//
// Sized internal formats valid for renderbufferStorage in WebGL2. Renderbuffers
// are write-only render targets (you can't sample them) — used for color when
// you only need MSAA, and for depth/stencil you won't read back.

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderbufferFormat {
    // --- color ---
    Rgba8 = 0x8058,        // RGBA8
    Srgb8Alpha8 = 0x8C43,  // SRGB8_ALPHA8
    Rgba16f = 0x881A,      // RGBA16F
    Rgba32f = 0x8814,      // RGBA32F
    R11fG11fB10f = 0x8C3A, // R11F_G11F_B10F
    // --- depth / stencil ---
    Depth16 = 0x81A5,          // DEPTH_COMPONENT16
    Depth24 = 0x81A6,          // DEPTH_COMPONENT24
    Depth32f = 0x8CAC,         // DEPTH_COMPONENT32F
    Depth24Stencil8 = 0x88F0,  // DEPTH24_STENCIL8
    Depth32fStencil8 = 0x8CAD, // DEPTH32F_STENCIL8
    Stencil8 = 0x8D48,         // STENCIL_INDEX8
}

impl RenderbufferFormat {
    fn as_gl(&self) -> u32 {
        *self as u32
    }
}

// ---------------------------------------------------------------------------
// Renderbuffer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct RenderbufferInner {
    gl: WebGl2RenderingContext,
    raw: WebGlRenderbuffer,
    width: i32,
    height: i32,
    samples: i32,
    format: RenderbufferFormat,
}

ref_counted!(Renderbuffer wraps RenderbufferInner; drop(self) {
    self.inner.gl.delete_renderbuffer(Some(&self.inner.raw));
});

impl Renderbuffer {
    pub(crate) fn new(
        gl: &WebGl2RenderingContext,
        format: RenderbufferFormat,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let raw = gl
            .create_renderbuffer()
            .ok_or("createRenderbuffer failed")?;
        gl.bind_renderbuffer(WebGl2RenderingContext::RENDERBUFFER, Some(&raw));
        gl.renderbuffer_storage(
            WebGl2RenderingContext::RENDERBUFFER,
            format.as_gl(),
            width,
            height,
        );
        gl.bind_renderbuffer(WebGl2RenderingContext::RENDERBUFFER, None);
        Ok(Self {
            inner: RenderbufferInner {
                gl: gl.clone(),
                raw,
                width,
                height,
                samples: 0,
                format,
            },
            rc: RefCount::new(),
        })
    }

    pub(crate) fn new_multisample(
        gl: &WebGl2RenderingContext,
        format: RenderbufferFormat,
        samples: i32,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let raw = gl
            .create_renderbuffer()
            .ok_or("createRenderbuffer failed")?;
        gl.bind_renderbuffer(WebGl2RenderingContext::RENDERBUFFER, Some(&raw));
        gl.renderbuffer_storage_multisample(
            WebGl2RenderingContext::RENDERBUFFER,
            samples,
            format.as_gl(),
            width,
            height,
        );
        gl.bind_renderbuffer(WebGl2RenderingContext::RENDERBUFFER, None);
        Ok(Self {
            inner: RenderbufferInner {
                gl: gl.clone(),
                raw,
                width,
                height,
                samples,
                format,
            },
            rc: RefCount::new(),
        })
    }

    pub(crate) fn raw_gl(&self) -> &WebGlRenderbuffer {
        &self.inner.raw
    }
}

#[wasm_bindgen]
impl Renderbuffer {
    pub fn width(&self) -> i32 {
        self.inner.width
    }

    pub fn height(&self) -> i32 {
        self.inner.height
    }

    /// 0 for a non-multisampled renderbuffer.
    pub fn samples(&self) -> i32 {
        self.inner.samples
    }

    pub fn format(&self) -> RenderbufferFormat {
        self.inner.format
    }
}
