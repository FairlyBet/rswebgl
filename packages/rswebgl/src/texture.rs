use crate::error::Error;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rswebgl_foundation::pool::Destroyed;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use web_sys::{
    Blob, BlobPropertyBag, HtmlCanvasElement, HtmlImageElement, HtmlVideoElement, ImageBitmap,
    ImageData, Url, WebGl2RenderingContext, WebGlTexture,
};

use crate::compressed_format::CompressedFormat;
use crate::console;
use crate::context::{Context, stale};
use crate::float_targets;
use crate::ids::TextureId;
use crate::limits;
use crate::pixel_unpack::PixelUnpack;
use crate::render_state::DepthFunc;

/// Self-referential slot for a one-shot image-load handler (see
/// [`Context::drive_image_load`]). The handler removes itself from the slot when
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        *self as u32
    }
}

// ---------------------------------------------------------------------------
// TextureMinFilter / TextureMagFilter
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureMinFilter {
    Nearest = 9728,              // NEAREST
    Linear = 9729,               // LINEAR
    NearestMipmapNearest = 9984, // NEAREST_MIPMAP_NEAREST
    LinearMipmapNearest = 9985,  // LINEAR_MIPMAP_NEAREST
    NearestMipmapLinear = 9986,  // NEAREST_MIPMAP_LINEAR
    LinearMipmapLinear = 9987,   // LINEAR_MIPMAP_LINEAR
}

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureMagFilter {
    Nearest = 9728, // NEAREST
    Linear = 9729,  // LINEAR
}

// ---------------------------------------------------------------------------
// TextureWrap
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureWrap {
    Repeat = 10497,         // REPEAT
    ClampToEdge = 33071,    // CLAMP_TO_EDGE
    MirroredRepeat = 33648, // MIRRORED_REPEAT
}

// ---------------------------------------------------------------------------
// TextureCompareMode (depth / shadow sampling)
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    #[wasm_bindgen(js_name = dataType)]
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
    #[wasm_bindgen(js_name = srgb8Alpha8)]
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
    #[wasm_bindgen(js_name = r11fG11fB10f)]
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
    #[wasm_bindgen(js_name = depth24Stencil8)]
    pub fn depth24_stencil8() -> TextureFormat {
        TextureFormat::DEPTH24_STENCIL8
    }
    #[wasm_bindgen(js_name = depth32fStencil8)]
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

/// A texture in the context's pool.
#[derive(Debug)]
pub(crate) struct TextureObj {
    pub(crate) raw: WebGlTexture,
    pub(crate) target: TextureTarget,
    // Sized internal format once storage is allocated, 0 before.
    pub(crate) internal: Cell<u32>,
}

impl TextureObj {
    pub(crate) fn delete(self, gl: &WebGl2RenderingContext) {
        gl.delete_texture(Some(&self.raw));
    }

    pub(crate) fn target_gl(&self) -> u32 {
        self.target.as_gl()
    }

    // Checks a texStorage request against the implementation limits for this
    // target (`depth` is 1 for 2D / cube). Reports and returns false if it
    // can't be allocated, so the caller skips the GL call.
    fn storage_fits(&self, levels: i32, width: i32, height: i32, depth: i32) -> bool {
        let (max_wh, max_d, what) = match self.target {
            TextureTarget::Texture2D => (limits::max_texture_size(), 1, "MAX_TEXTURE_SIZE"),
            TextureTarget::TextureCubeMap => (
                limits::max_cube_map_texture_size(),
                1,
                "MAX_CUBE_MAP_TEXTURE_SIZE",
            ),
            TextureTarget::Texture3D => {
                let m = limits::max_3d_texture_size();
                (m, m, "MAX_3D_TEXTURE_SIZE")
            }
            TextureTarget::Texture2DArray => (
                limits::max_texture_size(),
                limits::max_array_texture_layers(),
                "MAX_TEXTURE_SIZE / MAX_ARRAY_TEXTURE_LAYERS",
            ),
        };
        let problem = if self.internal.get() != 0 {
            Some("is already allocated (immutable storage is one-shot)".to_string())
        } else if width < 1 || height < 1 || depth < 1 {
            Some(format!("size {width}x{height}x{depth} must be at least 1"))
        } else if width as u32 > max_wh || height as u32 > max_wh || depth as u32 > max_d {
            Some(format!(
                "size {width}x{height}x{depth} exceeds {what} ({max_wh}x{max_wh}x{max_d})"
            ))
        } else if levels < 1 || levels > mip_levels(width.max(depth), height) {
            Some(format!(
                "{levels} mip levels is out of range for {width}x{height}x{depth} \
                 (1..={})",
                mip_levels(width.max(depth), height)
            ))
        } else {
            None
        };
        if let Some(p) = &problem {
            console::error(&format!(
                "[rswebgl] texture storage {p}; storage not allocated"
            ));
        }
        problem.is_none()
    }
}

/// Full mip-chain length for a base size: `floor(log2(max(w,h))) + 1`.
pub(crate) fn mip_levels(w: i32, h: i32) -> i32 {
    let max = w.max(h).max(1) as u32;
    (32 - max.leading_zeros()) as i32
}

impl Context {
    // Resolves `tex`, binds it to its target, runs `f`, unbinds. Reports a
    // stale handle (as `op`) and returns None instead.
    // `texture_storage_2d`, saying whether storage was allocated.
    fn allocate_2d(
        &self,
        tex: TextureId,
        levels: i32,
        format: &TextureFormat,
        width: i32,
        height: i32,
    ) -> bool {
        let filter_float = self.can_filter_float32();
        self.with_texture(tex, "texture_storage_2d", |gl, t, obj| {
            let fits = obj.storage_fits(levels, width, height, 1);
            if fits {
                gl.tex_storage_2d(t, levels, format.internal as u32, width, height);
                storage_allocated(gl, t, obj, format.internal as u32, filter_float);
            }
            fits
        })
        .unwrap_or(false)
    }

    pub(crate) fn with_texture<T>(
        &self,
        tex: TextureId,
        op: &str,
        f: impl FnOnce(&WebGl2RenderingContext, u32, &TextureObj) -> T,
    ) -> Option<T> {
        let res = self.res();
        let Some(obj) = res.textures.get(tex.0) else {
            stale("texture", op, tex);
            return None;
        };
        let gl = self.gl();
        let t = obj.target_gl();
        self.gl_state().forget_texture_unit();
        gl.bind_texture(t, Some(&obj.raw));
        let out = f(gl, t, obj);
        gl.bind_texture(t, None);
        Some(out)
    }

    fn set_param_i(&self, tex: TextureId, op: &str, pname: u32, value: i32) {
        self.with_texture(tex, op, |gl, t, _| gl.tex_parameteri(t, pname, value));
    }

    fn set_param_f(&self, tex: TextureId, op: &str, pname: u32, value: f32) {
        self.with_texture(tex, op, |gl, t, _| gl.tex_parameterf(t, pname, value));
    }
}

// After texStorage: remembers the format, and warns about a 32F texture set
// to filter linearly where that isn't supported (it would sample as black).
fn storage_allocated(
    gl: &WebGl2RenderingContext,
    t: u32,
    obj: &TextureObj,
    internal: u32,
    filter_float: bool,
) {
    obj.internal.set(internal);
    if filter_float || !float_targets::is_float32(internal) {
        return;
    }
    let nearest = |pname| {
        let f = gl.get_tex_parameter(t, pname).as_f64().unwrap_or(0.0) as u32;
        f == WebGl2RenderingContext::NEAREST || f == WebGl2RenderingContext::NEAREST_MIPMAP_NEAREST
    };
    if !nearest(WebGl2RenderingContext::TEXTURE_MIN_FILTER)
        || !nearest(WebGl2RenderingContext::TEXTURE_MAG_FILTER)
    {
        console::warn(
            "[rswebgl] a 32-bit float texture filters linearly, but this device lacks \
             OES_texture_float_linear: it will sample as black. Use Nearest, or a 16F format",
        );
    }
}

// Storage-first model (MDN-recommended for WebGL2): allocate immutable storage
// once with `texture_storage_*`, then fill levels with `texture_sub_image_*`.
// `texImage2D` (the mutable, define-each-level-independently path) is
// deliberately not exposed — it defers validation to draw time and can make
// drivers over-allocate.
#[wasm_bindgen]
impl Context {
    #[wasm_bindgen(js_name = createTexture)]
    pub fn create_texture(
        &self,
        target: TextureTarget,
        min_filter: TextureMinFilter,
        mag_filter: TextureMagFilter,
    ) -> Result<TextureId, Error> {
        self.check_alive()?;
        let gl = self.gl();
        let raw = gl.create_texture().ok_or("createTexture failed")?;
        let t = target.as_gl();
        self.gl_state().forget_texture_unit();
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
        for wrap in [
            WebGl2RenderingContext::TEXTURE_WRAP_S,
            WebGl2RenderingContext::TEXTURE_WRAP_T,
            WebGl2RenderingContext::TEXTURE_WRAP_R,
        ] {
            gl.tex_parameteri(t, wrap, TextureWrap::ClampToEdge as i32);
        }
        gl.bind_texture(t, None);
        self.res()
            .textures
            .insert(TextureObj {
                raw,
                target,
                internal: Cell::new(0),
            })
            .map(Into::into)
            .map_err(|e| Error::from(e.to_string()))
    }

    /// Whether `tex` names a live texture (false once destroyed, or after a
    /// context loss).
    #[wasm_bindgen(js_name = textureIsValid)]
    pub fn texture_is_valid(&self, tex: TextureId) -> bool {
        self.res().textures.get(tex.0).is_some()
    }

    /// Deletes the texture. If a framebuffer still has it attached, its handle
    /// is invalid right away and the GL texture goes when the framebuffer lets
    /// go of it.
    #[wasm_bindgen(js_name = destroyTexture)]
    pub fn destroy_texture(&self, tex: TextureId) {
        let destroyed = self.res().textures.destroy(tex.0);
        match destroyed {
            Destroyed::Now(obj) => obj.delete(self.gl()),
            Destroyed::Deferred => {}
            Destroyed::Invalid => stale("texture", "destroy_texture", tex),
        }
    }

    /// The texture's target, or None for a stale handle.
    #[wasm_bindgen(js_name = textureTarget)]
    pub fn texture_target(&self, tex: TextureId) -> Option<TextureTarget> {
        self.res().textures.get(tex.0).map(|t| t.target.clone())
    }

    // --- immutable storage allocation ---------------------------------------

    /// Allocate immutable storage (`texStorage2D`). For TEXTURE_2D and
    /// TEXTURE_CUBE_MAP (the latter allocates all six faces). `levels` is the
    /// mip count (≥1). Can only be called once per texture.
    #[wasm_bindgen(js_name = textureStorage2D)]
    pub fn texture_storage_2d(
        &self,
        tex: TextureId,
        levels: i32,
        format: &TextureFormat,
        width: i32,
        height: i32,
    ) {
        self.allocate_2d(tex, levels, format, width, height);
    }

    /// Allocate immutable storage (`texStorage3D`). For TEXTURE_3D and
    /// TEXTURE_2D_ARRAY (where `depth` is the layer count).
    #[wasm_bindgen(js_name = textureStorage3D)]
    pub fn texture_storage_3d(
        &self,
        tex: TextureId,
        levels: i32,
        format: &TextureFormat,
        width: i32,
        height: i32,
        depth: i32,
    ) {
        let filter_float = self.can_filter_float32();
        self.with_texture(tex, "texture_storage_3d", |gl, t, obj| {
            if obj.storage_fits(levels, width, height, depth) {
                gl.tex_storage_3d(t, levels, format.internal as u32, width, height, depth);
                storage_allocated(gl, t, obj, format.internal as u32, filter_float);
            }
        });
    }

    /// Allocate immutable storage with a compressed sized internal format.
    /// Requires the matching extension enabled (see `CompressedFormat`).
    #[wasm_bindgen(js_name = textureCompressedStorage2D)]
    pub fn texture_compressed_storage_2d(
        &self,
        tex: TextureId,
        levels: i32,
        format: &CompressedFormat,
        width: i32,
        height: i32,
    ) {
        self.with_texture(tex, "texture_compressed_storage_2d", |gl, t, obj| {
            if obj.storage_fits(levels, width, height, 1) {
                gl.tex_storage_2d(t, levels, format.as_gl(), width, height);
                obj.internal.set(format.as_gl());
            }
        });
    }

    /// Compressed immutable storage for TEXTURE_3D / TEXTURE_2D_ARRAY.
    #[wasm_bindgen(js_name = textureCompressedStorage3D)]
    pub fn texture_compressed_storage_3d(
        &self,
        tex: TextureId,
        levels: i32,
        format: &CompressedFormat,
        width: i32,
        height: i32,
        depth: i32,
    ) {
        self.with_texture(tex, "texture_compressed_storage_3d", |gl, t, obj| {
            if obj.storage_fits(levels, width, height, depth) {
                gl.tex_storage_3d(t, levels, format.as_gl(), width, height, depth);
                obj.internal.set(format.as_gl());
            }
        });
    }

    // --- fill from raw bytes (texSubImage) ----------------------------------

    /// Fill a region of an allocated 2D level from raw bytes. Default unpack.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureSubImage2D)]
    pub fn texture_sub_image_2d(
        &self,
        tex: TextureId,
        level: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: &TextureFormat,
        data: &[u8],
    ) {
        self.texture_sub_image_2d_with(
            tex,
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

    /// `texture_sub_image_2d` with explicit pixel-unpack settings (flip_y,
    /// alignment…).
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureSubImage2DWith)]
    pub fn texture_sub_image_2d_with(
        &self,
        tex: TextureId,
        level: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: &TextureFormat,
        data: &[u8],
        unpack: &PixelUnpack,
    ) {
        self.with_texture(tex, "texture_sub_image_2d", |gl, t, _| {
            unpack.apply(gl);
            if let Err(e) = sub_image_2d(gl, t, level, x, y, width, height, format, data) {
                console::error(&format!("[rswebgl] texSubImage2D failed: {e}"));
            }
            PixelUnpack::reset(gl);
        });
    }

    /// Fill a region of an allocated 3D / 2D-array level from raw bytes.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureSubImage3D)]
    pub fn texture_sub_image_3d(
        &self,
        tex: TextureId,
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
        self.texture_sub_image_3d_with(
            tex,
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

    /// `texture_sub_image_3d` with explicit pixel-unpack settings.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureSubImage3DWith)]
    pub fn texture_sub_image_3d_with(
        &self,
        tex: TextureId,
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
        self.with_texture(tex, "texture_sub_image_3d", |gl, t, _| {
            unpack.apply(gl);
            let result = if format.data_type == UNSIGNED_BYTE {
                gl.tex_sub_image_3d_with_opt_u8_array(
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
                )
                .map_err(|e| format!("{e:?}"))
            } else {
                pixel_view(data, format.data_type).and_then(|view| {
                    gl.tex_sub_image_3d_with_opt_array_buffer_view(
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
                        Some(&view),
                    )
                    .map_err(|e| format!("{e:?}"))
                })
            };
            if let Err(e) = result {
                console::error(&format!("[rswebgl] texSubImage3D failed: {e}"));
            }
            PixelUnpack::reset(gl);
        });
    }

    /// Fill a region of one cube-map face from raw bytes.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureSubImageCubeFace)]
    pub fn texture_sub_image_cube_face(
        &self,
        tex: TextureId,
        face: CubeMapFace,
        level: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: &TextureFormat,
        data: &[u8],
    ) {
        self.with_texture(tex, "texture_sub_image_cube_face", |gl, _, _| {
            let target = face.as_gl();
            if let Err(e) = sub_image_2d(gl, target, level, x, y, width, height, format, data) {
                console::error(&format!("[rswebgl] texSubImage2D cube face failed: {e}"));
            }
        });
    }

    // --- fill from compressed bytes (compressedTexSubImage) -----------------

    /// Fill a region of an allocated 2D level with compressed block data.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureCompressedSubImage2D)]
    pub fn texture_compressed_sub_image_2d(
        &self,
        tex: TextureId,
        level: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        format: &CompressedFormat,
        data: &[u8],
    ) {
        self.with_texture(tex, "texture_compressed_sub_image_2d", |gl, t, _| {
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
        });
    }

    /// Fill a region of an allocated 3D / 2D-array level with compressed data.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureCompressedSubImage3D)]
    pub fn texture_compressed_sub_image_3d(
        &self,
        tex: TextureId,
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
        self.with_texture(tex, "texture_compressed_sub_image_3d", |gl, t, _| {
            // SAFETY: see texture_compressed_sub_image_2d.
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
        });
    }

    // --- fill from DOM sources (texSubImage) --------------------------------

    /// Fill a 2D level from an `HtmlImageElement` (must be loaded & CORS-clean).
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureSubImage2DFromImage)]
    pub fn texture_sub_image_2d_from_image(
        &self,
        tex: TextureId,
        level: i32,
        x: i32,
        y: i32,
        image: &HtmlImageElement,
        format: &TextureFormat,
        unpack: &PixelUnpack,
    ) {
        self.with_texture(tex, "texture_sub_image_2d_from_image", |gl, t, _| {
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
        });
    }

    /// Fill a 2D level from an `ImageBitmap`.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureSubImage2DFromBitmap)]
    pub fn texture_sub_image_2d_from_bitmap(
        &self,
        tex: TextureId,
        level: i32,
        x: i32,
        y: i32,
        bitmap: &ImageBitmap,
        format: &TextureFormat,
        unpack: &PixelUnpack,
    ) {
        self.with_texture(tex, "texture_sub_image_2d_from_bitmap", |gl, t, _| {
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
        });
    }

    /// Fill a 2D level from the current frame of an `HtmlVideoElement`.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureSubImage2DFromVideo)]
    pub fn texture_sub_image_2d_from_video(
        &self,
        tex: TextureId,
        level: i32,
        x: i32,
        y: i32,
        video: &HtmlVideoElement,
        format: &TextureFormat,
        unpack: &PixelUnpack,
    ) {
        self.with_texture(tex, "texture_sub_image_2d_from_video", |gl, t, _| {
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
        });
    }

    /// Fill a 2D level from an `HtmlCanvasElement`.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureSubImage2DFromCanvas)]
    pub fn texture_sub_image_2d_from_canvas(
        &self,
        tex: TextureId,
        level: i32,
        x: i32,
        y: i32,
        canvas: &HtmlCanvasElement,
        format: &TextureFormat,
        unpack: &PixelUnpack,
    ) {
        self.with_texture(tex, "texture_sub_image_2d_from_canvas", |gl, t, _| {
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
        });
    }

    /// Fill a 2D level from `ImageData`.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureSubImage2DFromImageData)]
    pub fn texture_sub_image_2d_from_image_data(
        &self,
        tex: TextureId,
        level: i32,
        x: i32,
        y: i32,
        image_data: &ImageData,
        format: &TextureFormat,
        unpack: &PixelUnpack,
    ) {
        self.with_texture(tex, "texture_sub_image_2d_from_image_data", |gl, t, _| {
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
        });
    }

    // --- convenience: load an image URL and upload it on `onload` -----------

    /// Load an image from `url` and upload it into `tex` once it loads.
    ///
    /// On load it allocates immutable storage sized to the image, fills level 0,
    /// optionally builds mipmaps, then invokes `on_load` (a JS callback) to
    /// signal the texture is ready to sample. Until then the texture is empty.
    /// `on_load` is called as `on_load(width, height)` — the decoded size, or
    /// `(0, 0)` if the image failed to load; callbacks that don't need the size
    /// can ignore the arguments. If the texture is destroyed before the image
    /// arrives, the upload is skipped.
    ///
    /// The texture must be freshly created (storage is immutable / one-shot);
    /// an image larger than the GPU allows isn't uploaded (reported, `(0, 0)`).
    /// Pixels upload as stored — no flip unless `flip_y`, no premultiplying,
    /// no browser color-space conversion (glTF's convention). `on_load` always
    /// comes exactly once, never before this returns. The image is fetched in
    /// CORS mode (like `crossOrigin = "anonymous"`) and decoded off the main
    /// thread with `createImageBitmap` where there is one (an `<img>`
    /// otherwise), so the result is CORS-clean and usable as a texture.
    #[wasm_bindgen(js_name = textureLoadImage)]
    pub fn texture_load_image(
        &self,
        tex: TextureId,
        url: &str,
        format: &TextureFormat,
        generate_mipmaps: bool,
        flip_y: bool,
        on_load: js_sys::Function,
    ) {
        if let Some(decode) = bitmap_decoder() {
            let source = BitmapSource::Url(url.to_string());
            self.load_bitmap(
                tex,
                decode,
                source,
                format.clone(),
                generate_mipmaps,
                flip_y,
                on_load,
            );
            return;
        }
        let img = match HtmlImageElement::new() {
            Ok(img) => img,
            Err(_) => {
                console::error("[rswebgl] failed to create HtmlImageElement");
                failed_later(on_load);
                return;
            }
        };
        img.set_cross_origin(Some("anonymous"));
        self.drive_image_load(
            tex,
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
    /// `tex` once it decodes — the in-memory counterpart of `texture_load_image`.
    ///
    /// The bytes are wrapped in a `Blob` (tagged with `mime`, e.g. `"image/png"`)
    /// and handed to the browser's image decoder via an object URL, which is
    /// revoked once decoding finishes. Same storage/mipmap/`on_load` semantics as
    /// `texture_load_image`. Useful for images embedded in a container (e.g. a
    /// glTF `bufferView`) where no URL exists.
    #[allow(clippy::too_many_arguments)]
    #[wasm_bindgen(js_name = textureLoadBytes)]
    pub fn texture_load_bytes(
        &self,
        tex: TextureId,
        bytes: &[u8],
        mime: &str,
        format: &TextureFormat,
        generate_mipmaps: bool,
        flip_y: bool,
        on_load: js_sys::Function,
    ) {
        let parts = js_sys::Array::new();
        // A view, not a copy: the Blob constructor copies the bytes right away,
        // before anything could grow (and move) wasm memory.
        parts.push(&unsafe { js_sys::Uint8Array::view(bytes) }.into());
        let opts = BlobPropertyBag::new();
        opts.set_type(mime);
        let blob = match Blob::new_with_u8_array_sequence_and_options(&parts, &opts) {
            Ok(b) => b,
            Err(e) => {
                console::error(&format!(
                    "[rswebgl] load_bytes: Blob creation failed: {e:?}"
                ));
                failed_later(on_load);
                return;
            }
        };
        if let Some(decode) = bitmap_decoder() {
            let source = BitmapSource::Blob(blob);
            self.load_bitmap(
                tex,
                decode,
                source,
                format.clone(),
                generate_mipmaps,
                flip_y,
                on_load,
            );
            return;
        }
        let url = match Url::create_object_url_with_blob(&blob) {
            Ok(u) => u,
            Err(e) => {
                console::error(&format!(
                    "[rswebgl] load_bytes: createObjectURL failed: {e:?}"
                ));
                failed_later(on_load);
                return;
            }
        };
        // Blob URLs are same-origin and CORS-clean — no crossOrigin needed.
        let img = match HtmlImageElement::new() {
            Ok(img) => img,
            Err(_) => {
                console::error("[rswebgl] failed to create HtmlImageElement");
                let _ = Url::revoke_object_url(&url);
                failed_later(on_load);
                return;
            }
        };
        let revoke = url.clone();
        self.drive_image_load(
            tex,
            img,
            &url,
            format.clone(),
            generate_mipmaps,
            flip_y,
            on_load,
            Some(revoke),
        );
    }

    // --- LOD / sampler parameters -------------------------------------------

    /// `TEXTURE_BASE_LEVEL` — lowest mip level used when sampling.
    #[wasm_bindgen(js_name = textureSetBaseLevel)]
    pub fn texture_set_base_level(&self, tex: TextureId, level: i32) {
        self.set_param_i(
            tex,
            "texture_set_base_level",
            WebGl2RenderingContext::TEXTURE_BASE_LEVEL,
            level,
        );
    }

    /// `TEXTURE_MAX_LEVEL` — highest mip level used when sampling.
    #[wasm_bindgen(js_name = textureSetMaxLevel)]
    pub fn texture_set_max_level(&self, tex: TextureId, level: i32) {
        self.set_param_i(
            tex,
            "texture_set_max_level",
            WebGl2RenderingContext::TEXTURE_MAX_LEVEL,
            level,
        );
    }

    /// `TEXTURE_MIN_LOD` — clamp the minimum level-of-detail.
    #[wasm_bindgen(js_name = textureSetMinLod)]
    pub fn texture_set_min_lod(&self, tex: TextureId, lod: f32) {
        self.set_param_f(
            tex,
            "texture_set_min_lod",
            WebGl2RenderingContext::TEXTURE_MIN_LOD,
            lod,
        );
    }

    /// `TEXTURE_MAX_LOD` — clamp the maximum level-of-detail.
    #[wasm_bindgen(js_name = textureSetMaxLod)]
    pub fn texture_set_max_lod(&self, tex: TextureId, lod: f32) {
        self.set_param_f(
            tex,
            "texture_set_max_lod",
            WebGl2RenderingContext::TEXTURE_MAX_LOD,
            lod,
        );
    }

    /// `TEXTURE_COMPARE_MODE` — enable depth comparison (shadow sampling).
    #[wasm_bindgen(js_name = textureSetCompareMode)]
    pub fn texture_set_compare_mode(&self, tex: TextureId, mode: TextureCompareMode) {
        self.set_param_i(
            tex,
            "texture_set_compare_mode",
            WebGl2RenderingContext::TEXTURE_COMPARE_MODE,
            mode as i32,
        );
    }

    /// `TEXTURE_COMPARE_FUNC` — comparison used when compare mode is enabled.
    #[wasm_bindgen(js_name = textureSetCompareFunc)]
    pub fn texture_set_compare_func(&self, tex: TextureId, func: DepthFunc) {
        self.set_param_i(
            tex,
            "texture_set_compare_func",
            WebGl2RenderingContext::TEXTURE_COMPARE_FUNC,
            func.as_gl() as i32,
        );
    }

    /// `TEXTURE_MAX_ANISOTROPY_EXT` — anisotropic filtering level (≥1.0).
    /// Requires `ExtTextureFilterAnisotropic` to be enabled.
    #[wasm_bindgen(js_name = textureSetMaxAnisotropy)]
    pub fn texture_set_max_anisotropy(&self, tex: TextureId, value: f32) {
        const TEXTURE_MAX_ANISOTROPY_EXT: u32 = 0x84FE;
        self.set_param_f(
            tex,
            "texture_set_max_anisotropy",
            TEXTURE_MAX_ANISOTROPY_EXT,
            value,
        );
    }

    #[wasm_bindgen(js_name = textureSetMinFilter)]
    pub fn texture_set_min_filter(&self, tex: TextureId, filter: TextureMinFilter) {
        self.set_param_i(
            tex,
            "texture_set_min_filter",
            WebGl2RenderingContext::TEXTURE_MIN_FILTER,
            filter as i32,
        );
    }

    #[wasm_bindgen(js_name = textureSetMagFilter)]
    pub fn texture_set_mag_filter(&self, tex: TextureId, filter: TextureMagFilter) {
        self.set_param_i(
            tex,
            "texture_set_mag_filter",
            WebGl2RenderingContext::TEXTURE_MAG_FILTER,
            filter as i32,
        );
    }

    #[wasm_bindgen(js_name = textureSetWrapS)]
    pub fn texture_set_wrap_s(&self, tex: TextureId, wrap: TextureWrap) {
        self.set_param_i(
            tex,
            "texture_set_wrap_s",
            WebGl2RenderingContext::TEXTURE_WRAP_S,
            wrap as i32,
        );
    }

    #[wasm_bindgen(js_name = textureSetWrapT)]
    pub fn texture_set_wrap_t(&self, tex: TextureId, wrap: TextureWrap) {
        self.set_param_i(
            tex,
            "texture_set_wrap_t",
            WebGl2RenderingContext::TEXTURE_WRAP_T,
            wrap as i32,
        );
    }

    #[wasm_bindgen(js_name = textureSetWrapR)]
    pub fn texture_set_wrap_r(&self, tex: TextureId, wrap: TextureWrap) {
        self.set_param_i(
            tex,
            "texture_set_wrap_r",
            WebGl2RenderingContext::TEXTURE_WRAP_R,
            wrap as i32,
        );
    }

    #[wasm_bindgen(js_name = textureGenerateMipmaps)]
    pub fn texture_generate_mipmaps(&self, tex: TextureId) {
        let internal = self.res().textures.get(tex.0).map(|t| t.internal.get());
        if let Some(problem) = internal.and_then(|i| self.mipmap_problem(i)) {
            console::error(&format!("[rswebgl] texture_generate_mipmaps: {problem}"));
            return;
        }
        self.with_texture(tex, "texture_generate_mipmaps", |gl, t, _| {
            gl.generate_mipmap(t)
        });
    }
}

impl Context {
    /// Why GL can't generate mipmaps for a texture of sized format
    /// `internal`, if it can't: the format must be color-renderable and
    /// filterable — not compressed, integer, depth, or a float format the
    /// device can't render to (or, for 32F, filter).
    pub(crate) fn mipmap_problem(&self, internal: u32) -> Option<String> {
        match internal {
            0 => Some("the texture has no storage yet".into()),
            // R8 RG8 RGB8 RGBA8 SRGB8_ALPHA8 RGB565 RGBA4 RGB5_A1 RGB10_A2
            0x8229 | 0x822B | 0x8051 | 0x8058 | 0x8C43 | 0x8D62 | 0x8056 | 0x8057 | 0x8059 => None,
            _ if float_targets::float_need(internal).is_some() => {
                if let Some(p) = self.float_target_problem(internal) {
                    Some(p)
                } else if float_targets::is_float32(internal) && !self.can_filter_float32() {
                    Some(
                        "32-bit float textures can't be filtered on this device \
                         (OES_texture_float_linear); use a 16F format"
                            .into(),
                    )
                } else {
                    None
                }
            }
            _ => Some(format!(
                "format 0x{internal:04X} can't have mipmaps generated (compressed, integer, \
                 depth, or not color-renderable); upload each level instead"
            )),
        }
    }
}

// What a bitmap load decodes: fetched from a URL, or bytes already in hand.
enum BitmapSource {
    Url(String),
    Blob(Blob),
}

// `createImageBitmap` on this global (window or worker), if there is one.
fn bitmap_decoder() -> Option<js_sys::Function> {
    js_sys::Reflect::get(&js_sys::global(), &"createImageBitmap".into())
        .ok()
        .and_then(|f| f.dyn_into().ok())
}

// Awaits `promise`-returning `f.call(this, args…)`.
async fn call_async(
    this: &JsValue,
    f: &js_sys::Function,
    args: &js_sys::Array,
) -> Result<JsValue, JsValue> {
    let promise: js_sys::Promise = f.apply(this, args)?.dyn_into()?;
    wasm_bindgen_futures::JsFuture::from(promise).await
}

// Fetches `url` (CORS, like an `<img crossOrigin="anonymous">`) as a Blob.
async fn fetch_blob(url: &str) -> Result<JsValue, JsValue> {
    let global = js_sys::global();
    let fetch: js_sys::Function = js_sys::Reflect::get(&global, &"fetch".into())?.dyn_into()?;
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(&opts, &"mode".into(), &"cors".into())?;
    let response = call_async(&global, &fetch, &js_sys::Array::of2(&url.into(), &opts)).await?;
    let ok = js_sys::Reflect::get(&response, &"ok".into())?.is_truthy();
    if !ok {
        let status = js_sys::Reflect::get(&response, &"status".into())?;
        return Err(format!("HTTP {}", status.as_f64().unwrap_or(0.0)).into());
    }
    let blob: js_sys::Function = js_sys::Reflect::get(&response, &"blob".into())?.dyn_into()?;
    call_async(&response, &blob, &js_sys::Array::new()).await
}

// Decodes `blob` off the main thread, as stored: no premultiplying, no
// color-space conversion (WebGL ignores its unpack flags for bitmaps, so
// these and the flip are decode options).
async fn decode_bitmap(
    decode: &js_sys::Function,
    blob: &JsValue,
    flip_y: bool,
) -> Result<ImageBitmap, JsValue> {
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(&opts, &"premultiplyAlpha".into(), &"none".into())?;
    js_sys::Reflect::set(&opts, &"colorSpaceConversion".into(), &"none".into())?;
    let orientation = if flip_y { "flipY" } else { "from-image" };
    js_sys::Reflect::set(&opts, &"imageOrientation".into(), &orientation.into())?;
    let args = js_sys::Array::of2(blob, &opts);
    call_async(&js_sys::global(), decode, &args)
        .await?
        .dyn_into()
}

impl Context {
    /// `texture_load_image` / `texture_load_bytes` through `createImageBitmap`:
    /// the decode runs off the main thread (an `<img>` decodes on it, at first
    /// upload), and works in workers too. Same contract as the image path.
    #[allow(clippy::too_many_arguments)]
    fn load_bitmap(
        &self,
        tex: TextureId,
        decode: js_sys::Function,
        source: BitmapSource,
        format: TextureFormat,
        generate_mipmaps: bool,
        flip_y: bool,
        on_load: js_sys::Function,
    ) {
        let ctx = self.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let (what, blob) = match source {
                BitmapSource::Url(url) => {
                    let blob = fetch_blob(&url).await;
                    (url, blob)
                }
                BitmapSource::Blob(blob) => ("in-memory image".to_string(), Ok(blob.into())),
            };
            let bitmap = match blob {
                Ok(blob) => decode_bitmap(&decode, &blob, flip_y).await,
                Err(e) => Err(e),
            };
            let (w, h) = match bitmap {
                // Destroyed (or lost) meanwhile: nothing to fill or report.
                Ok(bitmap) if !ctx.texture_is_valid(tex) => {
                    bitmap.close();
                    (0, 0)
                }
                Ok(bitmap) => {
                    let size = ctx.fill_from_bitmap(tex, &bitmap, &format, generate_mipmaps);
                    bitmap.close();
                    size
                }
                Err(_) if !ctx.texture_is_valid(tex) => (0, 0),
                Err(e) => {
                    let why = e.as_string().unwrap_or_else(|| format!("{e:?}"));
                    console::error(&format!("[rswebgl] image failed to load: {what} ({why})"));
                    (0, 0)
                }
            };
            let _ = on_load.call2(&JsValue::NULL, &(w as f64).into(), &(h as f64).into());
        });
    }

    // Sizes storage to `bitmap`, fills level 0 and the mipmaps; the size, or
    // (0, 0) when it can't be allocated (already reported).
    fn fill_from_bitmap(
        &self,
        tex: TextureId,
        bitmap: &ImageBitmap,
        format: &TextureFormat,
        generate_mipmaps: bool,
    ) -> (i32, i32) {
        let (w, h) = (bitmap.width() as i32, bitmap.height() as i32);
        let levels = if generate_mipmaps {
            mip_levels(w, h)
        } else {
            1
        };
        if w == 0 || h == 0 || !self.allocate_2d(tex, levels, format, w, h) {
            return (0, 0);
        }
        self.texture_sub_image_2d_from_bitmap(
            tex,
            0,
            0,
            0,
            bitmap,
            format,
            &PixelUnpack::default(),
        );
        if generate_mipmaps {
            self.texture_generate_mipmaps(tex);
        }
        (w, h)
    }

    /// Shared decode machinery for `texture_load_image`/`texture_load_bytes`.
    /// One closure handles both `onload` and `onerror`: on success it sizes
    /// immutable storage to the image, fills level 0, optionally builds mipmaps;
    /// on failure it logs. Either way it revokes a transient object URL, fires
    /// `on_load`, and tears itself down — so a failed/aborted load leaks neither
    /// the closure nor the object URL.
    #[allow(clippy::too_many_arguments)]
    fn drive_image_load(
        &self,
        tex: TextureId,
        img: HtmlImageElement,
        src: &str,
        format: TextureFormat,
        generate_mipmaps: bool,
        flip_y: bool,
        on_load: js_sys::Function,
        revoke_url: Option<String>,
    ) {
        let ctx = self.clone();
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
            let (w, h) = if !ctx.texture_is_valid(tex) {
                // Destroyed (or lost) while the image loaded: nothing to fill,
                // and nothing to report — dropping an asset mid-load is normal.
                (0, 0)
            } else if img_cb.complete() && img_cb.natural_width() > 0 {
                let w = img_cb.natural_width() as i32;
                let h = img_cb.natural_height() as i32;
                let levels = if generate_mipmaps {
                    mip_levels(w, h)
                } else {
                    1
                };
                if ctx.allocate_2d(tex, levels, &format, w, h) {
                    // No browser color-space conversion: the image's stored values,
                    // as glTF (and data maps generally) expect.
                    let unpack = PixelUnpack {
                        flip_y,
                        colorspace_conversion: false,
                        ..PixelUnpack::default()
                    };
                    ctx.texture_sub_image_2d_from_image(tex, 0, 0, 0, &img_cb, &format, &unpack);
                    if generate_mipmaps {
                        ctx.texture_generate_mipmaps(tex);
                    }
                    (w, h)
                } else {
                    // Already reported (too large for this GPU, or loaded twice).
                    (0, 0)
                }
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
}

const UNSIGNED_BYTE: u32 = 0x1401;

// texSubImage2D from raw bytes. WebGL wants the typed array that matches the
// pixel type (a Uint8Array only for UNSIGNED_BYTE: HALF_FLOAT needs a
// Uint16Array, FLOAT a Float32Array, …), so other types go through a typed
// copy of the bytes.
#[allow(clippy::too_many_arguments)]
fn sub_image_2d(
    gl: &WebGl2RenderingContext,
    target: u32,
    level: i32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    format: &TextureFormat,
    data: &[u8],
) -> Result<(), String> {
    if format.data_type == UNSIGNED_BYTE {
        return gl
            .tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_u8_array(
                target,
                level,
                x,
                y,
                width,
                height,
                format.format,
                format.data_type,
                Some(data),
            )
            .map_err(|e| format!("{e:?}"));
    }
    let view = pixel_view(data, format.data_type)?;
    gl.tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_array_buffer_view(
        target,
        level,
        x,
        y,
        width,
        height,
        format.format,
        format.data_type,
        Some(&view),
    )
    .map_err(|e| format!("{e:?}"))
}

// The bytes as the typed array a pixel `data_type` calls for (a copy in JS
// memory; the bytes are little-endian, as wasm and every WebGL platform are).
fn pixel_view(data: &[u8], data_type: u32) -> Result<js_sys::Object, String> {
    let element = match data_type {
        0x1400 | 0x1401 => 1,                                     // (UNSIGNED_)BYTE
        0x1402 | 0x1403 | 0x140B | 0x8363 | 0x8033 | 0x8034 => 2, // shorts, HALF_FLOAT, 565/4444/5551
        0x1404 | 0x1405 | 0x1406 | 0x84FA | 0x8368 | 0x8C3B | 0x8C3E => 4, // ints, FLOAT, packed 32-bit
        other => {
            return Err(format!(
                "pixel type {other:#x} can't be uploaded from bytes"
            ));
        }
    };
    if !data.len().is_multiple_of(element) {
        return Err(format!(
            "{} bytes isn't a whole number of {element}-byte texel components",
            data.len()
        ));
    }
    let buffer = js_sys::Uint8Array::from(data).buffer();
    let len = (data.len() / element) as u32;
    Ok(match data_type {
        0x1400 => js_sys::Int8Array::new(&buffer).into(),
        0x1401 => js_sys::Uint8Array::new(&buffer).into(),
        0x1402 => js_sys::Int16Array::new_with_byte_offset_and_length(&buffer, 0, len).into(),
        0x1404 => js_sys::Int32Array::new_with_byte_offset_and_length(&buffer, 0, len).into(),
        0x1406 => js_sys::Float32Array::new_with_byte_offset_and_length(&buffer, 0, len).into(),
        _ if element == 2 => {
            js_sys::Uint16Array::new_with_byte_offset_and_length(&buffer, 0, len).into()
        }
        _ => js_sys::Uint32Array::new_with_byte_offset_and_length(&buffer, 0, len).into(),
    })
}

/// `on_load(0, 0)` for a load that couldn't even start, called on a microtask:
/// the callback always comes once and never synchronously, so a caller can
/// set up what it needs after starting the load (as the glTF loader's
/// texture-packing join does).
pub fn failed_later(on_load: js_sys::Function) {
    wasm_bindgen_futures::spawn_local(async move {
        let _ = on_load.call2(&JsValue::NULL, &0.into(), &0.into());
    });
}
