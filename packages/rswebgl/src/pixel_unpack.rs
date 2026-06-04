use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

// pixelStorei pnames not exposed as WebGl2RenderingContext associated constants
// (the WEBGL_* ones live on the context object itself, not the GL enum).
const UNPACK_FLIP_Y_WEBGL: u32 = 0x9240;
const UNPACK_PREMULTIPLY_ALPHA_WEBGL: u32 = 0x9241;
const UNPACK_COLORSPACE_CONVERSION_WEBGL: u32 = 0x9243;
const BROWSER_DEFAULT_WEBGL: i32 = 0x9244;
const NONE: i32 = 0;

/// Per-upload pixel-store (unpack) settings. WebGL's `pixelStorei` state is
/// global and sticky, which is a footgun; this bundles the relevant params so
/// upload methods can apply them right before the transfer and restore the
/// defaults right after — leaving global state untouched between calls.
///
/// Construct with `new()` (WebGL defaults) and override the fields you need.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug)]
pub struct PixelUnpack {
    /// Flip the source vertically — the usual fix for images that load upside
    /// down (WebGL's texture origin is bottom-left). `UNPACK_FLIP_Y_WEBGL`.
    pub flip_y: bool,
    /// Premultiply RGB by alpha during upload. `UNPACK_PREMULTIPLY_ALPHA_WEBGL`.
    pub premultiply_alpha: bool,
    /// Apply the browser's default color-space conversion to DOM sources (vs
    /// none). `UNPACK_COLORSPACE_CONVERSION_WEBGL`.
    pub colorspace_conversion: bool,
    /// Row byte alignment: 1, 2, 4 (default), or 8. `UNPACK_ALIGNMENT`.
    pub alignment: i32,
    /// Full source row length in pixels, 0 = tightly packed. `UNPACK_ROW_LENGTH`.
    pub row_length: i32,
    /// Pixels skipped at the start of each row. `UNPACK_SKIP_PIXELS`.
    pub skip_pixels: i32,
    /// Rows skipped before the first. `UNPACK_SKIP_ROWS`.
    pub skip_rows: i32,
    /// Full source image height in rows (3D), 0 = tightly packed. `UNPACK_IMAGE_HEIGHT`.
    pub image_height: i32,
    /// Images skipped before the first (3D). `UNPACK_SKIP_IMAGES`.
    pub skip_images: i32,
}

impl Default for PixelUnpack {
    fn default() -> Self {
        Self {
            flip_y: false,
            premultiply_alpha: false,
            colorspace_conversion: true,
            alignment: 4,
            row_length: 0,
            skip_pixels: 0,
            skip_rows: 0,
            image_height: 0,
            skip_images: 0,
        }
    }
}

#[wasm_bindgen]
impl PixelUnpack {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Convenience: defaults with `flip_y` set — the common DOM-image case.
    pub fn flipped() -> Self {
        Self {
            flip_y: true,
            ..Self::default()
        }
    }
}

impl PixelUnpack {
    pub(crate) fn apply(&self, gl: &WebGl2RenderingContext) {
        gl.pixel_storei(UNPACK_FLIP_Y_WEBGL, self.flip_y as i32);
        gl.pixel_storei(
            UNPACK_PREMULTIPLY_ALPHA_WEBGL,
            self.premultiply_alpha as i32,
        );
        gl.pixel_storei(
            UNPACK_COLORSPACE_CONVERSION_WEBGL,
            if self.colorspace_conversion {
                BROWSER_DEFAULT_WEBGL
            } else {
                NONE
            },
        );
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_ALIGNMENT, self.alignment);
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_ROW_LENGTH, self.row_length);
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_SKIP_PIXELS, self.skip_pixels);
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_SKIP_ROWS, self.skip_rows);
        gl.pixel_storei(
            WebGl2RenderingContext::UNPACK_IMAGE_HEIGHT,
            self.image_height,
        );
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_SKIP_IMAGES, self.skip_images);
    }

    /// Restore all of the above to their WebGL defaults so upload state never
    /// leaks across calls.
    pub(crate) fn reset(gl: &WebGl2RenderingContext) {
        gl.pixel_storei(UNPACK_FLIP_Y_WEBGL, 0);
        gl.pixel_storei(UNPACK_PREMULTIPLY_ALPHA_WEBGL, 0);
        gl.pixel_storei(UNPACK_COLORSPACE_CONVERSION_WEBGL, BROWSER_DEFAULT_WEBGL);
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_ALIGNMENT, 4);
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_ROW_LENGTH, 0);
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_SKIP_PIXELS, 0);
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_SKIP_ROWS, 0);
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_IMAGE_HEIGHT, 0);
        gl.pixel_storei(WebGl2RenderingContext::UNPACK_SKIP_IMAGES, 0);
    }
}
