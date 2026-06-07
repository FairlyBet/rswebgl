use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::{
    Blob, BlobPropertyBag, HtmlCanvasElement, HtmlImageElement, HtmlVideoElement, ImageBitmap,
    ImageData, Url, WebGl2RenderingContext, WebGlTexture,
};

use crate::compressed_format::CompressedFormat;
use crate::console;
use crate::pixel_unpack::PixelUnpack;
use crate::ref_count::{RefCount, ref_counted};
use crate::render_state::DepthFunc;

/// Self-referential slot for a one-shot image-load handler (see
/// [`Texture::drive_image_load`]). The handler removes itself from the slot when
/// it fires, breaking the `Rc` cycle that otherwise keeps it alive.
type ImageLoadHandler = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

// ---------------------------------------------------------------------------
// TextureTarget
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureTarget {
    Texture2D = 3553,       // TEXTURE_2D
    Texture3D = 32879,      // TEXTURE_3D
    TextureCubeMap = 34067, // TEXTURE_CUBE_MAP
    Texture2DArray = 35866, // TEXTURE_2D_ARRAY
}

impl TextureTarget {
    pub(crate) fn as_gl(&self) -> u32 {
        self.clone() as u32
    }
}

// ---------------------------------------------------------------------------
// CubeMapFace
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CubeMapFace {
    PositiveX = 34069, // TEXTURE_CUBE_MAP_POSITIVE_X
    NegativeX = 34070,
    PositiveY = 34071,
    NegativeY = 34072,
    PositiveZ = 34073,
    NegativeZ = 34074,
}

impl CubeMapFace {
    pub(crate) fn as_gl(&self) -> u32 {
        self.clone() as u32
    }
}

// ---------------------------------------------------------------------------
// TextureMinFilter / TextureMagFilter
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureMinFilter {
    Nearest = 9728,              // NEAREST
    Linear = 9729,               // LINEAR
    NearestMipmapNearest = 9984, // NEAREST_MIPMAP_NEAREST
    LinearMipmapNearest = 9985,  // LINEAR_MIPMAP_NEAREST
    NearestMipmapLinear = 9986,  // NEAREST_MIPMAP_LINEAR
    LinearMipmapLinear = 9987,   // LINEAR_MIPMAP_LINEAR
}

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureMagFilter {
    Nearest = 9728, // NEAREST
    Linear = 9729,  // LINEAR
}

// ---------------------------------------------------------------------------
// TextureWrap
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureWrap {
    Repeat = 10497,         // REPEAT
    ClampToEdge = 33071,    // CLAMP_TO_EDGE
    MirroredRepeat = 33648, // MIRRORED_REPEAT
}

// ---------------------------------------------------------------------------
// TextureCompareMode (depth / shadow sampling)
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureCompareMode {
    None = 0,                     // NONE
    CompareRefToTexture = 0x884E, // COMPARE_REF_TO_TEXTURE
}

// ---------------------------------------------------------------------------
// TextureFormat
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureFormat {
    pub internal: i32,
    pub format: u32,
    pub data_type: u32,
}

#[wasm_bindgen]
impl TextureFormat {
    pub fn r8() -> TextureFormat {
        TextureFormat::R8
    }
    pub fn rg8() -> TextureFormat {
        TextureFormat::RG8
    }
    pub fn rgb8() -> TextureFormat {
        TextureFormat::RGB8
    }
    pub fn rgba8() -> TextureFormat {
        TextureFormat::RGBA8
    }
    pub fn srgb8() -> TextureFormat {
        TextureFormat::SRGB8
    }
    pub fn srgb8_alpha8() -> TextureFormat {
        TextureFormat::SRGB8_ALPHA8
    }
    pub fn r16f() -> TextureFormat {
        TextureFormat::R16F
    }
    pub fn rg16f() -> TextureFormat {
        TextureFormat::RG16F
    }
    pub fn rgb16f() -> TextureFormat {
        TextureFormat::RGB16F
    }
    pub fn rgba16f() -> TextureFormat {
        TextureFormat::RGBA16F
    }
    pub fn r32f() -> TextureFormat {
        TextureFormat::R32F
    }
    pub fn rg32f() -> TextureFormat {
        TextureFormat::RG32F
    }
    pub fn rgb32f() -> TextureFormat {
        TextureFormat::RGB32F
    }
    pub fn rgba32f() -> TextureFormat {
        TextureFormat::RGBA32F
    }
    pub fn r11f_g11f_b10f() -> TextureFormat {
        TextureFormat::R11F_G11F_B10F
    }
    pub fn depth16() -> TextureFormat {
        TextureFormat::DEPTH16
    }
    pub fn depth24() -> TextureFormat {
        TextureFormat::DEPTH24
    }
    pub fn depth32f() -> TextureFormat {
        TextureFormat::DEPTH32F
    }
    pub fn depth24_stencil8() -> TextureFormat {
        TextureFormat::DEPTH24_STENCIL8
    }
    pub fn depth32f_stencil8() -> TextureFormat {
        TextureFormat::DEPTH32F_STENCIL8
    }
}

impl TextureFormat {
    // --- normalized uint8 ---
    pub const R8: Self = Self {
        internal: 0x8229,
        format: 0x1903,
        data_type: 0x1401,
    }; // R8 / RED / UNSIGNED_BYTE
    pub const RG8: Self = Self {
        internal: 0x822B,
        format: 0x8227,
        data_type: 0x1401,
    }; // RG8 / RG / UNSIGNED_BYTE
    pub const RGB8: Self = Self {
        internal: 0x8051,
        format: 0x1907,
        data_type: 0x1401,
    }; // RGB8 / RGB / UNSIGNED_BYTE
    pub const RGBA8: Self = Self {
        internal: 0x8058,
        format: 0x1908,
        data_type: 0x1401,
    }; // RGBA8 / RGBA / UNSIGNED_BYTE

    // --- sRGB ---
    pub const SRGB8: Self = Self {
        internal: 0x8C41,
        format: 0x1907,
        data_type: 0x1401,
    }; // SRGB8 / RGB / UNSIGNED_BYTE
    pub const SRGB8_ALPHA8: Self = Self {
        internal: 0x8C43,
        format: 0x1908,
        data_type: 0x1401,
    }; // SRGB8_ALPHA8 / RGBA / UNSIGNED_BYTE

    // --- half float ---
    pub const R16F: Self = Self {
        internal: 0x822D,
        format: 0x1903,
        data_type: 0x140B,
    }; // R16F / RED / HALF_FLOAT
    pub const RG16F: Self = Self {
        internal: 0x822F,
        format: 0x8227,
        data_type: 0x140B,
    }; // RG16F / RG / HALF_FLOAT
    pub const RGB16F: Self = Self {
        internal: 0x881B,
        format: 0x1907,
        data_type: 0x140B,
    }; // RGB16F / RGB / HALF_FLOAT
    pub const RGBA16F: Self = Self {
        internal: 0x881A,
        format: 0x1908,
        data_type: 0x140B,
    }; // RGBA16F / RGBA / HALF_FLOAT

    // --- float ---
    pub const R32F: Self = Self {
        internal: 0x822E,
        format: 0x1903,
        data_type: 0x1406,
    }; // R32F / RED / FLOAT
    pub const RG32F: Self = Self {
        internal: 0x8230,
        format: 0x8227,
        data_type: 0x1406,
    }; // RG32F / RG / FLOAT
    pub const RGB32F: Self = Self {
        internal: 0x8815,
        format: 0x1907,
        data_type: 0x1406,
    }; // RGB32F / RGB / FLOAT
    pub const RGBA32F: Self = Self {
        internal: 0x8814,
        format: 0x1908,
        data_type: 0x1406,
    }; // RGBA32F / RGBA / FLOAT

    // --- packed ---
    pub const R11F_G11F_B10F: Self = Self {
        internal: 0x8C3A,
        format: 0x1907,
        data_type: 0x8C3B,
    }; // R11F_G11F_B10F / RGB / UNSIGNED_INT_10F_11F_11F_REV

    // --- depth / stencil ---
    pub const DEPTH16: Self = Self {
        internal: 0x81A5,
        format: 0x1902,
        data_type: 0x1403,
    }; // DEPTH_COMPONENT16 / DEPTH_COMPONENT / UNSIGNED_SHORT
    pub const DEPTH24: Self = Self {
        internal: 0x81A6,
        format: 0x1902,
        data_type: 0x1405,
    }; // DEPTH_COMPONENT24 / DEPTH_COMPONENT / UNSIGNED_INT
    pub const DEPTH32F: Self = Self {
        internal: 0x8CAC,
        format: 0x1902,
        data_type: 0x1406,
    }; // DEPTH_COMPONENT32F / DEPTH_COMPONENT / FLOAT
    pub const DEPTH24_STENCIL8: Self = Self {
        internal: 0x88F0,
        format: 0x84F9,
        data_type: 0x84FA,
    }; // DEPTH24_STENCIL8 / DEPTH_STENCIL / UNSIGNED_INT_24_8
    pub const DEPTH32F_STENCIL8: Self = Self {
        internal: 0x8CAD,
        format: 0x84F9,
        data_type: 0x8DAD,
    }; // DEPTH32F_STENCIL8 / DEPTH_STENCIL / FLOAT_32_UNSIGNED_INT_24_8_REV
}

// ---------------------------------------------------------------------------
// Texture
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct TextureInner {
    gl: WebGl2RenderingContext,
    raw: WebGlTexture,
    target: TextureTarget,
}

ref_counted!(Texture wraps TextureInner; drop(self) {
    self.inner.gl.delete_texture(Some(&self.inner.raw));
});

impl Texture {
    pub(crate) fn new(
        gl: &WebGl2RenderingContext,
        target: TextureTarget,
        min_filter: TextureMinFilter,
        mag_filter: TextureMagFilter,
    ) -> Result<Self, String> {
        let raw = gl.create_texture().ok_or("createTexture failed")?;
        let t = target.as_gl();

        gl.bind_texture(t, Some(&raw));
        gl.tex_parameteri(
            t,
            WebGl2RenderingContext::TEXTURE_MIN_FILTER,
            min_filter as i32,
        );
        gl.tex_parameteri(
            t,
            WebGl2RenderingContext::TEXTURE_MAG_FILTER,
            mag_filter as i32,
        );
        gl.tex_parameteri(
            t,
            WebGl2RenderingContext::TEXTURE_WRAP_S,
            TextureWrap::ClampToEdge as i32,
        );
        gl.tex_parameteri(
            t,
            WebGl2RenderingContext::TEXTURE_WRAP_T,
            TextureWrap::ClampToEdge as i32,
        );
        gl.tex_parameteri(
            t,
            WebGl2RenderingContext::TEXTURE_WRAP_R,
            TextureWrap::ClampToEdge as i32,
        );
        gl.bind_texture(t, None);

        Ok(Self {
            inner: TextureInner {
                gl: gl.clone(),
                raw,
                target,
            },
            rc: RefCount::new(),
        })
    }

    pub(crate) fn raw_gl(&self) -> &WebGlTexture {
        &self.inner.raw
    }

    pub(crate) fn target_gl(&self) -> u32 {
        self.inner.target.as_gl()
    }

    fn set_param_i(&self, pname: u32, value: i32) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner.gl.tex_parameteri(t, pname, value);
        self.inner.gl.bind_texture(t, None);
    }

    fn set_param_f(&self, pname: u32, value: f32) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner.gl.tex_parameterf(t, pname, value);
        self.inner.gl.bind_texture(t, None);
    }
}

/// Full mip-chain length for a base size: `floor(log2(max(w,h))) + 1`.
pub(crate) fn mip_levels(w: i32, h: i32) -> i32 {
    let max = w.max(h).max(1) as u32;
    (32 - max.leading_zeros()) as i32
}

impl PartialEq for Texture {
    fn eq(&self, other: &Self) -> bool {
        self.inner.raw == other.inner.raw
    }
}

impl Eq for Texture {}

// Storage-first model (MDN-recommended for WebGL2): allocate immutable storage
// once with `storage_*`, then fill levels with `sub_image_*`. `texImage2D` (the
// mutable, define-each-level-independently path) is deliberately not exposed —
// it defers validation to draw time and can make drivers over-allocate.
#[wasm_bindgen]
impl Texture {
    // --- immutable storage allocation ---------------------------------------

    /// Allocate immutable storage (`texStorage2D`). For TEXTURE_2D and
    /// TEXTURE_CUBE_MAP (the latter allocates all six faces). `levels` is the
    /// mip count (≥1). Can only be called once per texture.
    pub fn storage_2d(&self, levels: i32, format: &TextureFormat, width: i32, height: i32) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner
            .gl
            .tex_storage_2d(t, levels, format.internal as u32, width, height);
        self.inner.gl.bind_texture(t, None);
    }

    /// Allocate immutable storage (`texStorage3D`). For TEXTURE_3D and
    /// TEXTURE_2D_ARRAY (where `depth` is the layer count).
    pub fn storage_3d(
        &self,
        levels: i32,
        format: &TextureFormat,
        width: i32,
        height: i32,
        depth: i32,
    ) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner
            .gl
            .tex_storage_3d(t, levels, format.internal as u32, width, height, depth);
        self.inner.gl.bind_texture(t, None);
    }

    /// Allocate immutable storage with a compressed sized internal format.
    /// Requires the matching extension enabled (see `CompressedFormat`).
    pub fn compressed_storage_2d(
        &self,
        levels: i32,
        format: &CompressedFormat,
        width: i32,
        height: i32,
    ) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner
            .gl
            .tex_storage_2d(t, levels, format.as_gl(), width, height);
        self.inner.gl.bind_texture(t, None);
    }

    /// Compressed immutable storage for TEXTURE_3D / TEXTURE_2D_ARRAY.
    pub fn compressed_storage_3d(
        &self,
        levels: i32,
        format: &CompressedFormat,
        width: i32,
        height: i32,
        depth: i32,
    ) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner
            .gl
            .tex_storage_3d(t, levels, format.as_gl(), width, height, depth);
        self.inner.gl.bind_texture(t, None);
    }

    // --- fill from raw bytes (texSubImage) ----------------------------------

    /// Fill a region of an allocated 2D level from raw bytes. Default unpack.
    #[allow(clippy::too_many_arguments)]
    pub fn sub_image_2d(
        &self,
        level: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: &TextureFormat,
        data: &[u8],
    ) {
        self.sub_image_2d_with(
            level,
            x,
            y,
            width,
            height,
            format,
            data,
            &PixelUnpack::default(),
        );
    }

    /// `sub_image_2d` with explicit pixel-unpack settings (flip_y, alignment…).
    #[allow(clippy::too_many_arguments)]
    pub fn sub_image_2d_with(
        &self,
        level: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: &TextureFormat,
        data: &[u8],
        unpack: &PixelUnpack,
    ) {
        let t = self.inner.target.as_gl();
        let gl = &self.inner.gl;
        gl.bind_texture(t, Some(&self.inner.raw));
        unpack.apply(gl);
        if let Err(e) = gl.tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_u8_array(
            t,
            level,
            x,
            y,
            width,
            height,
            format.format,
            format.data_type,
            Some(data),
        ) {
            console::error(&format!("[rswebgl] texSubImage2D failed: {e:?}"));
        }
        PixelUnpack::reset(gl);
        gl.bind_texture(t, None);
    }

    /// Fill a region of an allocated 3D / 2D-array level from raw bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn sub_image_3d(
        &self,
        level: i32,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        height: i32,
        depth: i32,
        format: &TextureFormat,
        data: &[u8],
    ) {
        self.sub_image_3d_with(
            level,
            x,
            y,
            z,
            width,
            height,
            depth,
            format,
            data,
            &PixelUnpack::default(),
        );
    }

    /// `sub_image_3d` with explicit pixel-unpack settings.
    #[allow(clippy::too_many_arguments)]
    pub fn sub_image_3d_with(
        &self,
        level: i32,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        height: i32,
        depth: i32,
        format: &TextureFormat,
        data: &[u8],
        unpack: &PixelUnpack,
    ) {
        let t = self.inner.target.as_gl();
        let gl = &self.inner.gl;
        gl.bind_texture(t, Some(&self.inner.raw));
        unpack.apply(gl);
        if let Err(e) = gl.tex_sub_image_3d_with_opt_u8_array(
            t,
            level,
            x,
            y,
            z,
            width,
            height,
            depth,
            format.format,
            format.data_type,
            Some(data),
        ) {
            console::error(&format!("[rswebgl] texSubImage3D failed: {e:?}"));
        }
        PixelUnpack::reset(gl);
        gl.bind_texture(t, None);
    }

    /// Fill a region of one cube-map face from raw bytes.
    #[allow(clippy::too_many_arguments)]
    pub fn sub_image_cube_face(
        &self,
        face: CubeMapFace,
        level: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: &TextureFormat,
        data: &[u8],
    ) {
        let t = self.inner.target.as_gl();
        let gl = &self.inner.gl;
        gl.bind_texture(t, Some(&self.inner.raw));
        if let Err(e) = gl.tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_u8_array(
            face.as_gl(),
            level,
            x,
            y,
            width,
            height,
            format.format,
            format.data_type,
            Some(data),
        ) {
            console::error(&format!("[rswebgl] texSubImage2D cube face failed: {e:?}"));
        }
        gl.bind_texture(t, None);
    }

    // --- fill from compressed bytes (compressedTexSubImage) -----------------

    /// Fill a region of an allocated 2D level with compressed block data.
    #[allow(clippy::too_many_arguments)]
    pub fn compressed_sub_image_2d(
        &self,
        level: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: &CompressedFormat,
        data: &[u8],
    ) {
        let t = self.inner.target.as_gl();
        let gl = &self.inner.gl;
        gl.bind_texture(t, Some(&self.inner.raw));
        // SAFETY: the view borrows wasm memory only for this call; the GL copy
        // happens synchronously and we don't allocate while it's alive.
        let view = unsafe { js_sys::Uint8Array::view(data) };
        gl.compressed_tex_sub_image_2d_with_array_buffer_view(
            t,
            level,
            x,
            y,
            width,
            height,
            format.as_gl(),
            &view,
        );
        gl.bind_texture(t, None);
    }

    /// Fill a region of an allocated 3D / 2D-array level with compressed data.
    #[allow(clippy::too_many_arguments)]
    pub fn compressed_sub_image_3d(
        &self,
        level: i32,
        x: i32,
        y: i32,
        z: i32,
        width: i32,
        height: i32,
        depth: i32,
        format: &CompressedFormat,
        data: &[u8],
    ) {
        let t = self.inner.target.as_gl();
        let gl = &self.inner.gl;
        gl.bind_texture(t, Some(&self.inner.raw));
        // SAFETY: see compressed_sub_image_2d.
        let view = unsafe { js_sys::Uint8Array::view(data) };
        gl.compressed_tex_sub_image_3d_with_array_buffer_view(
            t,
            level,
            x,
            y,
            z,
            width,
            height,
            depth,
            format.as_gl(),
            &view,
        );
        gl.bind_texture(t, None);
    }

    // --- fill from DOM sources (texSubImage) --------------------------------

    /// Fill a 2D level from an `HtmlImageElement` (must be loaded & CORS-clean).
    pub fn sub_image_2d_from_image(
        &self,
        level: i32,
        x: i32,
        y: i32,
        image: &HtmlImageElement,
        format: &TextureFormat,
        unpack: &PixelUnpack,
    ) {
        let t = self.inner.target.as_gl();
        let gl = &self.inner.gl;
        gl.bind_texture(t, Some(&self.inner.raw));
        unpack.apply(gl);
        if let Err(e) = gl.tex_sub_image_2d_with_u32_and_u32_and_html_image_element(
            t,
            level,
            x,
            y,
            format.format,
            format.data_type,
            image,
        ) {
            console::error(&format!("[rswebgl] texSubImage2D(image) failed: {e:?}"));
        }
        PixelUnpack::reset(gl);
        gl.bind_texture(t, None);
    }

    /// Fill a 2D level from an `ImageBitmap`.
    pub fn sub_image_2d_from_bitmap(
        &self,
        level: i32,
        x: i32,
        y: i32,
        bitmap: &ImageBitmap,
        format: &TextureFormat,
        unpack: &PixelUnpack,
    ) {
        let t = self.inner.target.as_gl();
        let gl = &self.inner.gl;
        gl.bind_texture(t, Some(&self.inner.raw));
        unpack.apply(gl);
        if let Err(e) = gl.tex_sub_image_2d_with_u32_and_u32_and_image_bitmap(
            t,
            level,
            x,
            y,
            format.format,
            format.data_type,
            bitmap,
        ) {
            console::error(&format!("[rswebgl] texSubImage2D(bitmap) failed: {e:?}"));
        }
        PixelUnpack::reset(gl);
        gl.bind_texture(t, None);
    }

    /// Fill a 2D level from the current frame of an `HtmlVideoElement`.
    pub fn sub_image_2d_from_video(
        &self,
        level: i32,
        x: i32,
        y: i32,
        video: &HtmlVideoElement,
        format: &TextureFormat,
        unpack: &PixelUnpack,
    ) {
        let t = self.inner.target.as_gl();
        let gl = &self.inner.gl;
        gl.bind_texture(t, Some(&self.inner.raw));
        unpack.apply(gl);
        if let Err(e) = gl.tex_sub_image_2d_with_u32_and_u32_and_html_video_element(
            t,
            level,
            x,
            y,
            format.format,
            format.data_type,
            video,
        ) {
            console::error(&format!("[rswebgl] texSubImage2D(video) failed: {e:?}"));
        }
        PixelUnpack::reset(gl);
        gl.bind_texture(t, None);
    }

    /// Fill a 2D level from an `HtmlCanvasElement`.
    pub fn sub_image_2d_from_canvas(
        &self,
        level: i32,
        x: i32,
        y: i32,
        canvas: &HtmlCanvasElement,
        format: &TextureFormat,
        unpack: &PixelUnpack,
    ) {
        let t = self.inner.target.as_gl();
        let gl = &self.inner.gl;
        gl.bind_texture(t, Some(&self.inner.raw));
        unpack.apply(gl);
        if let Err(e) = gl.tex_sub_image_2d_with_u32_and_u32_and_html_canvas_element(
            t,
            level,
            x,
            y,
            format.format,
            format.data_type,
            canvas,
        ) {
            console::error(&format!("[rswebgl] texSubImage2D(canvas) failed: {e:?}"));
        }
        PixelUnpack::reset(gl);
        gl.bind_texture(t, None);
    }

    /// Fill a 2D level from `ImageData`.
    pub fn sub_image_2d_from_image_data(
        &self,
        level: i32,
        x: i32,
        y: i32,
        image_data: &ImageData,
        format: &TextureFormat,
        unpack: &PixelUnpack,
    ) {
        let t = self.inner.target.as_gl();
        let gl = &self.inner.gl;
        gl.bind_texture(t, Some(&self.inner.raw));
        unpack.apply(gl);
        if let Err(e) = gl.tex_sub_image_2d_with_u32_and_u32_and_image_data(
            t,
            level,
            x,
            y,
            format.format,
            format.data_type,
            image_data,
        ) {
            console::error(&format!("[rswebgl] texSubImage2D(imageData) failed: {e:?}"));
        }
        PixelUnpack::reset(gl);
        gl.bind_texture(t, None);
    }

    // --- convenience: load an image URL and upload it on `onload` -----------

    /// Load an image from `url` and upload it into this texture once it loads.
    ///
    /// On load it allocates immutable storage sized to the image, fills level 0,
    /// optionally builds mipmaps, then invokes `on_load` (a JS callback) to
    /// signal the texture is ready to sample. Until then the texture is empty.
    /// `on_load` is called as `on_load(width, height)` — the decoded size, or
    /// `(0, 0)` if the image failed to load; callbacks that don't need the size
    /// can ignore the arguments.
    ///
    /// The texture must be freshly created (storage is immutable / one-shot).
    /// The image is requested with `crossOrigin = "anonymous"` so the result is
    /// CORS-clean and usable as a texture.
    pub fn load_image(
        &self,
        url: &str,
        format: &TextureFormat,
        generate_mipmaps: bool,
        flip_y: bool,
        on_load: js_sys::Function,
    ) {
        let img = match HtmlImageElement::new() {
            Ok(img) => img,
            Err(_) => {
                console::error("[rswebgl] failed to create HtmlImageElement");
                return;
            }
        };
        img.set_cross_origin(Some("anonymous"));
        self.drive_image_load(
            img,
            url,
            format.clone(),
            generate_mipmaps,
            flip_y,
            on_load,
            None,
        );
    }

    /// Load an encoded image (PNG/JPEG/…) from raw `bytes` and upload it into
    /// this texture once it decodes — the in-memory counterpart of `load_image`.
    ///
    /// The bytes are wrapped in a `Blob` (tagged with `mime`, e.g. `"image/png"`)
    /// and handed to the browser's image decoder via an object URL, which is
    /// revoked once decoding finishes. Same storage/mipmap/`on_load` semantics as
    /// `load_image`. Useful for images embedded in a container (e.g. a glTF
    /// `bufferView`) where no URL exists.
    pub fn load_bytes(
        &self,
        bytes: &[u8],
        mime: &str,
        format: &TextureFormat,
        generate_mipmaps: bool,
        flip_y: bool,
        on_load: js_sys::Function,
    ) {
        let parts = js_sys::Array::new();
        parts.push(&js_sys::Uint8Array::from(bytes));
        let opts = BlobPropertyBag::new();
        opts.set_type(mime);
        let blob = match Blob::new_with_u8_array_sequence_and_options(&parts, &opts) {
            Ok(b) => b,
            Err(e) => {
                console::error(&format!(
                    "[rswebgl] load_bytes: Blob creation failed: {e:?}"
                ));
                return;
            }
        };
        let url = match Url::create_object_url_with_blob(&blob) {
            Ok(u) => u,
            Err(e) => {
                console::error(&format!(
                    "[rswebgl] load_bytes: createObjectURL failed: {e:?}"
                ));
                return;
            }
        };
        // Blob URLs are same-origin and CORS-clean — no crossOrigin needed.
        let img = match HtmlImageElement::new() {
            Ok(img) => img,
            Err(_) => {
                console::error("[rswebgl] failed to create HtmlImageElement");
                let _ = Url::revoke_object_url(&url);
                return;
            }
        };
        let revoke = url.clone();
        self.drive_image_load(
            img,
            &url,
            format.clone(),
            generate_mipmaps,
            flip_y,
            on_load,
            Some(revoke),
        );
    }

    /// Shared decode machinery for `load_image`/`load_bytes`. One closure handles
    /// both `onload` and `onerror`: on success it sizes immutable storage to the
    /// image, fills level 0, optionally builds mipmaps; on failure it logs. Either
    /// way it revokes a transient object URL, fires `on_load`, and tears itself
    /// down — so a failed/aborted load leaks neither the closure (and its captured
    /// texture handle) nor the object URL.
    #[allow(clippy::too_many_arguments)]
    fn drive_image_load(
        &self,
        img: HtmlImageElement,
        src: &str,
        format: TextureFormat,
        generate_mipmaps: bool,
        flip_y: bool,
        on_load: js_sys::Function,
        revoke_url: Option<String>,
    ) {
        let tex = self.clone();
        let img_cb = img.clone();
        let src_owned = src.to_string(); // moved into the closure for error logs
        // The closure is stored in this slot and captures a clone of the same
        // `Rc`, so it keeps itself alive until an event fires (the only strong
        // refs form a cycle). The handler breaks that cycle by taking itself out
        // of the slot, which is also what frees its captures — so it must run at
        // most once and own the cleanup for both the success and error paths.
        let slot: ImageLoadHandler = Rc::new(RefCell::new(None));
        let me = slot.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            // Detach both handlers so a stray second event can't re-enter, and
            // move ourselves out of the shared slot. `_self` now solely owns this
            // closure and frees it (and every capture below) when this call ends,
            // breaking the Rc cycle — keep it the last thing dropped.
            img_cb.set_onload(None);
            img_cb.set_onerror(None);
            let _self = me.borrow_mut().take();

            // onload vs onerror without inspecting the event: a decoded image
            // reports a non-zero natural size, a failed one does not.
            let (w, h) = if img_cb.complete() && img_cb.natural_width() > 0 {
                let w = img_cb.natural_width() as i32;
                let h = img_cb.natural_height() as i32;
                let levels = if generate_mipmaps {
                    mip_levels(w, h)
                } else {
                    1
                };
                tex.storage_2d(levels, &format, w, h);
                let unpack = PixelUnpack {
                    flip_y,
                    ..PixelUnpack::default()
                };
                tex.sub_image_2d_from_image(0, 0, 0, &img_cb, &format, &unpack);
                if generate_mipmaps {
                    tex.generate_mipmaps();
                }
                (w, h)
            } else {
                console::error(&format!("[rswebgl] image failed to load: {src_owned}"));
                (0, 0) // signal failure to the callback
            };

            if let Some(url) = &revoke_url {
                let _ = Url::revoke_object_url(url);
            }
            // Report the decoded size (0,0 on failure). Callbacks that don't need
            // it (the common case) are nullary JS functions and ignore the args.
            let _ = on_load.call2(&JsValue::NULL, &(w as f64).into(), &(h as f64).into());
            // `_self` drops here, after every capture has been used for the last
            // time, freeing the closure box.
        });
        img.set_onload(Some(cb.as_ref().unchecked_ref()));
        img.set_onerror(Some(cb.as_ref().unchecked_ref()));
        *slot.borrow_mut() = Some(cb);
        img.set_src(src);
    }

    // --- LOD / sampler parameters -------------------------------------------

    /// `TEXTURE_BASE_LEVEL` — lowest mip level used when sampling.
    pub fn set_base_level(&self, level: i32) {
        self.set_param_i(WebGl2RenderingContext::TEXTURE_BASE_LEVEL, level);
    }

    /// `TEXTURE_MAX_LEVEL` — highest mip level used when sampling.
    pub fn set_max_level(&self, level: i32) {
        self.set_param_i(WebGl2RenderingContext::TEXTURE_MAX_LEVEL, level);
    }

    /// `TEXTURE_MIN_LOD` — clamp the minimum level-of-detail.
    pub fn set_min_lod(&self, lod: f32) {
        self.set_param_f(WebGl2RenderingContext::TEXTURE_MIN_LOD, lod);
    }

    /// `TEXTURE_MAX_LOD` — clamp the maximum level-of-detail.
    pub fn set_max_lod(&self, lod: f32) {
        self.set_param_f(WebGl2RenderingContext::TEXTURE_MAX_LOD, lod);
    }

    /// `TEXTURE_COMPARE_MODE` — enable depth comparison (shadow sampling).
    pub fn set_compare_mode(&self, mode: TextureCompareMode) {
        self.set_param_i(WebGl2RenderingContext::TEXTURE_COMPARE_MODE, mode as i32);
    }

    /// `TEXTURE_COMPARE_FUNC` — comparison used when compare mode is enabled.
    pub fn set_compare_func(&self, func: DepthFunc) {
        self.set_param_i(
            WebGl2RenderingContext::TEXTURE_COMPARE_FUNC,
            func.as_gl() as i32,
        );
    }

    /// `TEXTURE_MAX_ANISOTROPY_EXT` — anisotropic filtering level (≥1.0).
    /// Requires `ExtTextureFilterAnisotropic` to be enabled.
    pub fn set_max_anisotropy(&self, value: f32) {
        const TEXTURE_MAX_ANISOTROPY_EXT: u32 = 0x84FE;
        self.set_param_f(TEXTURE_MAX_ANISOTROPY_EXT, value);
    }

    pub fn set_min_filter(&self, filter: TextureMinFilter) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner
            .gl
            .tex_parameteri(t, WebGl2RenderingContext::TEXTURE_MIN_FILTER, filter as i32);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn set_mag_filter(&self, filter: TextureMagFilter) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner
            .gl
            .tex_parameteri(t, WebGl2RenderingContext::TEXTURE_MAG_FILTER, filter as i32);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn set_wrap_s(&self, wrap: TextureWrap) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner
            .gl
            .tex_parameteri(t, WebGl2RenderingContext::TEXTURE_WRAP_S, wrap as i32);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn set_wrap_t(&self, wrap: TextureWrap) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner
            .gl
            .tex_parameteri(t, WebGl2RenderingContext::TEXTURE_WRAP_T, wrap as i32);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn set_wrap_r(&self, wrap: TextureWrap) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner
            .gl
            .tex_parameteri(t, WebGl2RenderingContext::TEXTURE_WRAP_R, wrap as i32);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn generate_mipmaps(&self) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner.gl.generate_mipmap(t);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn target(&self) -> TextureTarget {
        self.inner.target.clone()
    }
}
