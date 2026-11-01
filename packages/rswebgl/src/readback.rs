// Reading pixels back from the GPU.
//
// `read_pixels` is the plain synchronous read: it waits for every queued draw
// to finish before returning — a full pipeline stall, fine for tests and tools.
//
// `read_pixels_async` doesn't stall. It copies the rectangle into a pixel-pack
// buffer on the GPU (`readPixels` with a PIXEL_PACK_BUFFER bound returns at
// once), drops a fence after it, and polls that fence without waiting
// (`clientWaitSync` with a zero timeout) until the GPU has passed it; only then
// does it fetch the bytes (`getBufferSubData`), which by that point costs no
// wait. The result reflects the rectangle as it was when the read was issued;
// rendering can go on meanwhile.
//
// Formats are the four every WebGL2 implementation accepts for its buffer
// kinds, all with 4-byte components — so pack alignment never comes into it.

use std::future::Future;
use std::time::Duration;

use js_sys::{ArrayBuffer, Float32Array, Int32Array, Uint8Array, Uint32Array};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{WebGl2RenderingContext as Gl, WebGlBuffer, WebGlSync};

use crate::context::Context;
use crate::error::{Error, err};
use crate::ids::FramebufferId;

/// What to read and as what.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadFormat {
    /// RGBA, one byte per channel — normalized (RGBA8, sRGB8_ALPHA8) targets
    /// and the canvas.
    Rgba8,
    /// RGBA as f32 — float targets (needs `ExtColorBufferFloat` to render to
    /// them in the first place).
    RgbaF32,
    /// RGBA as i32 — signed integer targets.
    RgbaI32,
    /// RGBA as u32 — unsigned integer targets.
    RgbaU32,
}

impl ReadFormat {
    fn gl(self) -> (u32, u32) {
        match self {
            Self::Rgba8 => (Gl::RGBA, Gl::UNSIGNED_BYTE),
            Self::RgbaF32 => (Gl::RGBA, Gl::FLOAT),
            Self::RgbaI32 => (Gl::RGBA_INTEGER, Gl::INT),
            Self::RgbaU32 => (Gl::RGBA_INTEGER, Gl::UNSIGNED_INT),
        }
    }

    /// Bytes per pixel.
    pub fn pixel_size(self) -> usize {
        match self {
            Self::Rgba8 => 4,
            _ => 16,
        }
    }

    // A view over `buf` of the element type WebGL requires for this format.
    fn view(self, buf: &ArrayBuffer) -> js_sys::Object {
        match self {
            Self::Rgba8 => Uint8Array::new(buf).into(),
            Self::RgbaF32 => Float32Array::new(buf).into(),
            Self::RgbaI32 => Int32Array::new(buf).into(),
            Self::RgbaU32 => Uint32Array::new(buf).into(),
        }
    }
}

/// A rectangle of a framebuffer's color attachment.
#[derive(Clone, Copy)]
struct Region {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[wasm_bindgen]
impl Context {
    /// Reads a rectangle of color attachment `attachment` of `fb`
    /// (`FramebufferId::NULL` = the canvas, attachment 0) as tightly packed
    /// rows, bottom row first. Waits for all queued GPU work — prefer
    /// `read_pixels_async` in a running app.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = readPixels)]
    pub fn read_pixels(
        &self,
        fb: FramebufferId,
        attachment: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: ReadFormat,
    ) -> Result<Vec<u8>, Error> {
        let r = Region {
            x,
            y,
            width,
            height,
        };
        let len = self.bind_read_source(fb, attachment, r, format)?;
        let buf = ArrayBuffer::new(len as u32);
        let (f, t) = format.gl();
        self.gl()
            .read_pixels_with_opt_array_buffer_view(
                x,
                y,
                width,
                height,
                f,
                t,
                Some(&format.view(&buf)),
            )
            .map_err(|e| err!("readPixels failed: {e:?}"))?;
        Ok(Uint8Array::new(&buf).to_vec())
    }

    /// `read_pixels` without the stall, for JS: a promise of the bytes
    /// (`Uint8Array`). Rejects if the read can't start or the context is lost
    /// before the GPU gets to it.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = readPixelsAsync)]
    pub fn read_pixels_promise(
        &self,
        fb: FramebufferId,
        attachment: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: ReadFormat,
    ) -> js_sys::Promise {
        let read = self.read_pixels_async(fb, attachment, x, y, width, height, format);
        wasm_bindgen_futures::future_to_promise(async move {
            let bytes = read.await?;
            Ok(Uint8Array::from(&bytes[..]).into())
        })
    }
}

impl Context {
    /// `read_pixels` without the stall: copies the rectangle into a GPU-side
    /// buffer *now* (so the bytes are the rectangle as of this call, whatever
    /// is drawn before the future is awaited) and resolves once the GPU has
    /// written them, polling a fence in between.
    #[allow(clippy::too_many_arguments)]
    pub fn read_pixels_async(
        &self,
        fb: FramebufferId,
        attachment: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: ReadFormat,
    ) -> impl Future<Output = Result<Vec<u8>, Error>> + 'static {
        let r = Region {
            x,
            y,
            width,
            height,
        };
        // Issued here, not on first poll: an `async fn` would start late.
        let started = self.start_readback(fb, attachment, r, format);
        let ctx = self.clone();
        async move {
            let pending = started?;
            let result = pending.finish(ctx.gl()).await;
            pending.delete(ctx.gl());
            result
        }
    }

    // Binds `fb` for reading attachment `attachment`, after checking the
    // region lies inside it. Returns the byte length of the result.
    fn bind_read_source(
        &self,
        fb: FramebufferId,
        attachment: u32,
        r: Region,
        format: ReadFormat,
    ) -> Result<usize, Error> {
        self.check_alive()?;
        let gl = self.gl();
        let (w, h) = if fb.is_null() {
            if attachment != 0 {
                return Err(err!("read: the canvas has only color attachment 0"));
            }
            let d = &self.core.default_fb;
            gl.bind_framebuffer(Gl::READ_FRAMEBUFFER, None);
            gl.read_buffer(Gl::BACK);
            (d.width(), d.height())
        } else {
            let res = self.res();
            let Some(f) = res.framebuffers.get(fb.0) else {
                return Err(err!("read: framebuffer {:?} is destroyed or invalid", fb.0));
            };
            if !f.has_color(attachment) {
                return Err(err!(
                    "read: framebuffer has no color attachment {attachment}"
                ));
            }
            // Only the read binding changes; the renderer's draw target (and
            // its cache of it) is untouched.
            gl.bind_framebuffer(Gl::READ_FRAMEBUFFER, Some(&f.raw));
            gl.read_buffer(Gl::COLOR_ATTACHMENT0 + attachment);
            f.size()
        };
        if r.width < 1
            || r.height < 1
            || r.x < 0
            || r.y < 0
            || r.x + r.width > w
            || r.y + r.height > h
        {
            return Err(err!(
                "read: region {}x{} at ({}, {}) is outside the {w}x{h} target",
                r.width,
                r.height,
                r.x,
                r.y
            ));
        }
        Ok(r.width as usize * r.height as usize * format.pixel_size())
    }

    fn start_readback(
        &self,
        fb: FramebufferId,
        attachment: u32,
        r: Region,
        format: ReadFormat,
    ) -> Result<PendingRead, Error> {
        let len = self.bind_read_source(fb, attachment, r, format)?;
        let gl = self.gl();
        let buffer = gl.create_buffer().ok_or("read: createBuffer failed")?;
        gl.bind_buffer(Gl::PIXEL_PACK_BUFFER, Some(&buffer));
        gl.buffer_data_with_i32(Gl::PIXEL_PACK_BUFFER, len as i32, Gl::STREAM_READ);
        let (f, t) = format.gl();
        let issued = gl.read_pixels_with_i32(r.x, r.y, r.width, r.height, f, t, 0);
        gl.bind_buffer(Gl::PIXEL_PACK_BUFFER, None);
        if let Err(e) = issued {
            gl.delete_buffer(Some(&buffer));
            return Err(err!("readPixels failed: {e:?}"));
        }
        let Some(sync) = gl.fence_sync(Gl::SYNC_GPU_COMMANDS_COMPLETE, 0) else {
            gl.delete_buffer(Some(&buffer));
            return Err("read: fenceSync failed".into());
        };
        // Submit the fence now; otherwise it may sit in the command queue and
        // never signal.
        gl.flush();
        Ok(PendingRead { buffer, sync, len })
    }
}

// A read in flight: the GPU-side copy and the fence behind it.
struct PendingRead {
    buffer: WebGlBuffer,
    sync: WebGlSync,
    len: usize,
}

impl PendingRead {
    async fn finish(&self, gl: &Gl) -> Result<Vec<u8>, Error> {
        loop {
            if gl.is_context_lost() {
                return Err("read: the WebGL context was lost".into());
            }
            match gl.client_wait_sync_with_u32(&self.sync, 0, 0) {
                Gl::ALREADY_SIGNALED | Gl::CONDITION_SATISFIED => break,
                Gl::WAIT_FAILED => return Err("read: waiting on the fence failed".into()),
                _ => sleep(Duration::from_millis(1)).await,
            }
        }
        let mut out = vec![0u8; self.len];
        gl.bind_buffer(Gl::COPY_READ_BUFFER, Some(&self.buffer));
        gl.get_buffer_sub_data_with_i32_and_u8_array(Gl::COPY_READ_BUFFER, 0, &mut out);
        gl.bind_buffer(Gl::COPY_READ_BUFFER, None);
        Ok(out)
    }

    fn delete(self, gl: &Gl) {
        gl.delete_sync(Some(&self.sync));
        gl.delete_buffer(Some(&self.buffer));
    }
}

// Yields to the event loop for about `d`. A timer rather than
// requestAnimationFrame: it keeps polling in background tabs and workers,
// and the wait is usually a frame or two.
async fn sleep(d: Duration) {
    let p = js_sys::Promise::new(&mut |resolve, _| {
        let global = js_sys::global();
        let set_timeout = js_sys::Reflect::get(&global, &"setTimeout".into())
            .ok()
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
        match set_timeout {
            Some(f) => {
                let _ = f.call2(&global, &resolve, &(d.as_millis() as f64).into());
            }
            None => {
                let _ = resolve.call0(&JsValue::NULL);
            }
        }
    });
    let _ = JsFuture::from(p).await;
}
