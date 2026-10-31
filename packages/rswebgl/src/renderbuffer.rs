use rswebgl_foundation::pool::Destroyed;
use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlRenderbuffer};

use crate::context::{Context, stale};
use crate::ids::RenderbufferId;
use crate::limits;

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

/// A renderbuffer in the context's pool.
#[derive(Debug)]
pub(crate) struct RenderbufferObj {
    pub(crate) raw: WebGlRenderbuffer,
    width: i32,
    height: i32,
    samples: i32,
    format: RenderbufferFormat,
}

impl RenderbufferObj {
    pub(crate) fn delete(self, gl: &WebGl2RenderingContext) {
        gl.delete_renderbuffer(Some(&self.raw));
    }
}

impl Context {
    fn new_renderbuffer(
        &self,
        format: RenderbufferFormat,
        samples: i32,
        width: i32,
        height: i32,
    ) -> Result<RenderbufferId, String> {
        self.check_alive()?;
        check_size(width, height, samples)?;
        let gl = self.gl();
        let raw = gl
            .create_renderbuffer()
            .ok_or("createRenderbuffer failed")?;
        gl.bind_renderbuffer(WebGl2RenderingContext::RENDERBUFFER, Some(&raw));
        if samples > 0 {
            gl.renderbuffer_storage_multisample(
                WebGl2RenderingContext::RENDERBUFFER,
                samples,
                format.as_gl(),
                width,
                height,
            );
        } else {
            gl.renderbuffer_storage(
                WebGl2RenderingContext::RENDERBUFFER,
                format.as_gl(),
                width,
                height,
            );
        }
        gl.bind_renderbuffer(WebGl2RenderingContext::RENDERBUFFER, None);
        self.res()
            .renderbuffers
            .insert(RenderbufferObj {
                raw,
                width,
                height,
                samples,
                format,
            })
            .map(Into::into)
            .map_err(|e| e.to_string())
    }

    fn renderbuffer_obj<T>(
        &self,
        rb: RenderbufferId,
        f: impl FnOnce(&RenderbufferObj) -> T,
    ) -> Option<T> {
        self.res().renderbuffers.get(rb.0).map(f)
    }
}

#[wasm_bindgen]
impl Context {
    pub fn create_renderbuffer(
        &self,
        format: RenderbufferFormat,
        width: i32,
        height: i32,
    ) -> Result<RenderbufferId, String> {
        self.new_renderbuffer(format, 0, width, height)
    }

    pub fn create_renderbuffer_multisample(
        &self,
        format: RenderbufferFormat,
        samples: i32,
        width: i32,
        height: i32,
    ) -> Result<RenderbufferId, String> {
        self.new_renderbuffer(format, samples.max(1), width, height)
    }

    /// 0 for a stale handle.
    pub fn renderbuffer_width(&self, rb: RenderbufferId) -> i32 {
        self.renderbuffer_obj(rb, |r| r.width).unwrap_or(0)
    }

    /// 0 for a stale handle.
    pub fn renderbuffer_height(&self, rb: RenderbufferId) -> i32 {
        self.renderbuffer_obj(rb, |r| r.height).unwrap_or(0)
    }

    /// 0 for a non-multisampled renderbuffer (or a stale handle).
    pub fn renderbuffer_samples(&self, rb: RenderbufferId) -> i32 {
        self.renderbuffer_obj(rb, |r| r.samples).unwrap_or(0)
    }

    pub fn renderbuffer_format(&self, rb: RenderbufferId) -> Option<RenderbufferFormat> {
        self.renderbuffer_obj(rb, |r| r.format)
    }

    /// Deletes the renderbuffer. If a framebuffer still has it attached, its
    /// handle is invalid right away and the GL object goes when the
    /// framebuffer lets go of it.
    pub fn destroy_renderbuffer(&self, rb: RenderbufferId) {
        let destroyed = self.res().renderbuffers.destroy(rb.0);
        match destroyed {
            Destroyed::Now(obj) => obj.delete(self.gl()),
            Destroyed::Deferred => {}
            Destroyed::Invalid => stale("renderbuffer", "destroy_renderbuffer", rb),
        }
    }
}

fn check_size(width: i32, height: i32, samples: i32) -> Result<(), String> {
    let max = limits::max_renderbuffer_size();
    if width < 1 || height < 1 || width as u32 > max || height as u32 > max {
        return Err(format!(
            "renderbuffer size {width}x{height} is out of range (1..={max}, \
             MAX_RENDERBUFFER_SIZE)"
        ));
    }
    let max_samples = limits::max_samples();
    if samples < 0 || samples as u32 > max_samples {
        return Err(format!(
            "renderbuffer samples {samples} is out of range (0..={max_samples}, MAX_SAMPLES)"
        ));
    }
    Ok(())
}
