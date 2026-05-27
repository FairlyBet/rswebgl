use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext, WebGlTexture};

use crate::console;
use crate::ref_count::{RefCount, ref_counted};

// ---------------------------------------------------------------------------
// TextureTarget
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureTarget {
    Texture2D      = 3553,  // TEXTURE_2D
    Texture3D      = 32879, // TEXTURE_3D
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
    Nearest              = 9728,  // NEAREST
    Linear               = 9729,  // LINEAR
    NearestMipmapNearest = 9984,  // NEAREST_MIPMAP_NEAREST
    LinearMipmapNearest  = 9985,  // LINEAR_MIPMAP_NEAREST
    NearestMipmapLinear  = 9986,  // NEAREST_MIPMAP_LINEAR
    LinearMipmapLinear   = 9987,  // LINEAR_MIPMAP_LINEAR
}

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureMagFilter {
    Nearest = 9728, // NEAREST
    Linear  = 9729, // LINEAR
}

// ---------------------------------------------------------------------------
// TextureWrap
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureWrap {
    Repeat         = 10497, // REPEAT
    ClampToEdge    = 33071, // CLAMP_TO_EDGE
    MirroredRepeat = 33648, // MIRRORED_REPEAT
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
    pub fn r8()             -> TextureFormat { TextureFormat::R8 }
    pub fn rg8()            -> TextureFormat { TextureFormat::RG8 }
    pub fn rgb8()           -> TextureFormat { TextureFormat::RGB8 }
    pub fn rgba8()          -> TextureFormat { TextureFormat::RGBA8 }
    pub fn srgb8()          -> TextureFormat { TextureFormat::SRGB8 }
    pub fn srgb8_alpha8()   -> TextureFormat { TextureFormat::SRGB8_ALPHA8 }
    pub fn r16f()           -> TextureFormat { TextureFormat::R16F }
    pub fn rg16f()          -> TextureFormat { TextureFormat::RG16F }
    pub fn rgb16f()         -> TextureFormat { TextureFormat::RGB16F }
    pub fn rgba16f()        -> TextureFormat { TextureFormat::RGBA16F }
    pub fn r32f()           -> TextureFormat { TextureFormat::R32F }
    pub fn rg32f()          -> TextureFormat { TextureFormat::RG32F }
    pub fn rgb32f()         -> TextureFormat { TextureFormat::RGB32F }
    pub fn rgba32f()        -> TextureFormat { TextureFormat::RGBA32F }
    pub fn r11f_g11f_b10f() -> TextureFormat { TextureFormat::R11F_G11F_B10F }
    pub fn depth16()        -> TextureFormat { TextureFormat::DEPTH16 }
    pub fn depth24()        -> TextureFormat { TextureFormat::DEPTH24 }
    pub fn depth32f()       -> TextureFormat { TextureFormat::DEPTH32F }
    pub fn depth24_stencil8()  -> TextureFormat { TextureFormat::DEPTH24_STENCIL8 }
    pub fn depth32f_stencil8() -> TextureFormat { TextureFormat::DEPTH32F_STENCIL8 }
}

impl TextureFormat {
    // --- normalized uint8 ---
    pub const R8:          Self = Self { internal: 0x8229, format: 0x1903, data_type: 0x1401 }; // R8 / RED / UNSIGNED_BYTE
    pub const RG8:         Self = Self { internal: 0x822B, format: 0x8227, data_type: 0x1401 }; // RG8 / RG / UNSIGNED_BYTE
    pub const RGB8:        Self = Self { internal: 0x8051, format: 0x1907, data_type: 0x1401 }; // RGB8 / RGB / UNSIGNED_BYTE
    pub const RGBA8:       Self = Self { internal: 0x8058, format: 0x1908, data_type: 0x1401 }; // RGBA8 / RGBA / UNSIGNED_BYTE

    // --- sRGB ---
    pub const SRGB8:       Self = Self { internal: 0x8C41, format: 0x1907, data_type: 0x1401 }; // SRGB8 / RGB / UNSIGNED_BYTE
    pub const SRGB8_ALPHA8:Self = Self { internal: 0x8C43, format: 0x1908, data_type: 0x1401 }; // SRGB8_ALPHA8 / RGBA / UNSIGNED_BYTE

    // --- half float ---
    pub const R16F:        Self = Self { internal: 0x822D, format: 0x1903, data_type: 0x140B }; // R16F / RED / HALF_FLOAT
    pub const RG16F:       Self = Self { internal: 0x822F, format: 0x8227, data_type: 0x140B }; // RG16F / RG / HALF_FLOAT
    pub const RGB16F:      Self = Self { internal: 0x881B, format: 0x1907, data_type: 0x140B }; // RGB16F / RGB / HALF_FLOAT
    pub const RGBA16F:     Self = Self { internal: 0x881A, format: 0x1908, data_type: 0x140B }; // RGBA16F / RGBA / HALF_FLOAT

    // --- float ---
    pub const R32F:        Self = Self { internal: 0x822E, format: 0x1903, data_type: 0x1406 }; // R32F / RED / FLOAT
    pub const RG32F:       Self = Self { internal: 0x8230, format: 0x8227, data_type: 0x1406 }; // RG32F / RG / FLOAT
    pub const RGB32F:      Self = Self { internal: 0x8815, format: 0x1907, data_type: 0x1406 }; // RGB32F / RGB / FLOAT
    pub const RGBA32F:     Self = Self { internal: 0x8814, format: 0x1908, data_type: 0x1406 }; // RGBA32F / RGBA / FLOAT

    // --- packed ---
    pub const R11F_G11F_B10F: Self = Self { internal: 0x8C3A, format: 0x1907, data_type: 0x8C3B }; // R11F_G11F_B10F / RGB / UNSIGNED_INT_10F_11F_11F_REV

    // --- depth / stencil ---
    pub const DEPTH16:         Self = Self { internal: 0x81A5, format: 0x1902, data_type: 0x1403 }; // DEPTH_COMPONENT16 / DEPTH_COMPONENT / UNSIGNED_SHORT
    pub const DEPTH24:         Self = Self { internal: 0x81A6, format: 0x1902, data_type: 0x1405 }; // DEPTH_COMPONENT24 / DEPTH_COMPONENT / UNSIGNED_INT
    pub const DEPTH32F:        Self = Self { internal: 0x8CAC, format: 0x1902, data_type: 0x1406 }; // DEPTH_COMPONENT32F / DEPTH_COMPONENT / FLOAT
    pub const DEPTH24_STENCIL8:Self = Self { internal: 0x88F0, format: 0x84F9, data_type: 0x84FA }; // DEPTH24_STENCIL8 / DEPTH_STENCIL / UNSIGNED_INT_24_8
    pub const DEPTH32F_STENCIL8:Self= Self { internal: 0x8CAD, format: 0x84F9, data_type: 0x8DAD }; // DEPTH32F_STENCIL8 / DEPTH_STENCIL / FLOAT_32_UNSIGNED_INT_24_8_REV
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
        gl.tex_parameteri(t, WebGl2RenderingContext::TEXTURE_MIN_FILTER, min_filter as i32);
        gl.tex_parameteri(t, WebGl2RenderingContext::TEXTURE_MAG_FILTER, mag_filter as i32);
        gl.tex_parameteri(t, WebGl2RenderingContext::TEXTURE_WRAP_S, TextureWrap::ClampToEdge as i32);
        gl.tex_parameteri(t, WebGl2RenderingContext::TEXTURE_WRAP_T, TextureWrap::ClampToEdge as i32);
        gl.tex_parameteri(t, WebGl2RenderingContext::TEXTURE_WRAP_R, TextureWrap::ClampToEdge as i32);
        gl.bind_texture(t, None);

        Ok(Self {
            inner: TextureInner { gl: gl.clone(), raw, target },
            rc: RefCount::new(),
        })
    }
}

#[wasm_bindgen]
impl Texture {
    pub fn upload_2d(
        &self,
        level: i32,
        format: &TextureFormat,
        width: i32,
        height: i32,
        data: &[u8],
    ) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        if let Err(e) = self.inner.gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
            t, level, format.internal, width, height, 0,
            format.format, format.data_type, Some(data),
        ) {
            console::error(&format!("[rswebgl] texImage2D failed: {:?}", e));
        }
        self.inner.gl.bind_texture(t, None);
    }

    pub fn alloc_2d(&self, level: i32, format: &TextureFormat, width: i32, height: i32) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        if let Err(e) = self.inner.gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
            t, level, format.internal, width, height, 0,
            format.format, format.data_type, None,
        ) {
            console::error(&format!("[rswebgl] texImage2D alloc failed: {:?}", e));
        }
        self.inner.gl.bind_texture(t, None);
    }

    pub fn upload_cube_face(
        &self,
        face: CubeMapFace,
        level: i32,
        format: &TextureFormat,
        width: i32,
        height: i32,
        data: &[u8],
    ) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        if let Err(e) = self.inner.gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
            face.as_gl(), level, format.internal, width, height, 0,
            format.format, format.data_type, Some(data),
        ) {
            console::error(&format!("[rswebgl] texImage2D cube face failed: {:?}", e));
        }
        self.inner.gl.bind_texture(t, None);
    }

    pub fn set_min_filter(&self, filter: TextureMinFilter) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner.gl.tex_parameteri(t, WebGl2RenderingContext::TEXTURE_MIN_FILTER, filter as i32);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn set_mag_filter(&self, filter: TextureMagFilter) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner.gl.tex_parameteri(t, WebGl2RenderingContext::TEXTURE_MAG_FILTER, filter as i32);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn set_wrap_s(&self, wrap: TextureWrap) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner.gl.tex_parameteri(t, WebGl2RenderingContext::TEXTURE_WRAP_S, wrap as i32);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn set_wrap_t(&self, wrap: TextureWrap) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner.gl.tex_parameteri(t, WebGl2RenderingContext::TEXTURE_WRAP_T, wrap as i32);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn set_wrap_r(&self, wrap: TextureWrap) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner.gl.tex_parameteri(t, WebGl2RenderingContext::TEXTURE_WRAP_R, wrap as i32);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn generate_mipmaps(&self) {
        let t = self.inner.target.as_gl();
        self.inner.gl.bind_texture(t, Some(&self.inner.raw));
        self.inner.gl.generate_mipmap(t);
        self.inner.gl.bind_texture(t, None);
    }

    pub fn raw(&self) -> WebGlTexture {
        self.inner.raw.clone()
    }

    pub fn target(&self) -> TextureTarget {
        self.inner.target.clone()
    }
}
