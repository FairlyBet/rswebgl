// Sampler objects: how a texture is read (filtering, wrapping, LOD range,
// depth compare, anisotropy), separate from the texture itself.
//
// A texture keeps its own sampling parameters as the fallback; a sampler bound
// to the same unit overrides all of them. So one texture can be read several
// ways (a material's REPEAT and a post-process's CLAMP over the same image)
// without a second copy, and sampling state is set once per sampler rather
// than once per texture. Samplers reach draws through parameter groups, next
// to the texture they apply to (`Context::param_set_sampler`).

use wasm_bindgen::prelude::*;
use web_sys::{WebGl2RenderingContext as Gl, WebGlSampler};

use rswebgl_foundation::pool::Destroyed;

use crate::console;
use crate::context::{Context, stale};
use crate::error::Error;
use crate::extension::Extension;
use crate::ids::SamplerId;
use crate::render_state::DepthFunc;
use crate::texture::{TextureCompareMode, TextureMagFilter, TextureMinFilter, TextureWrap};

const TEXTURE_MAX_ANISOTROPY_EXT: u32 = 0x84FE;

/// Sampling parameters. `new()` gives the WebGL defaults; set the fields you
/// need (`linear_clamp` / `nearest_clamp` / `trilinear_repeat` cover the usual
/// cases).
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SamplerDesc {
    #[wasm_bindgen(js_name = minFilter)]
    pub min_filter: TextureMinFilter,
    #[wasm_bindgen(js_name = magFilter)]
    pub mag_filter: TextureMagFilter,
    #[wasm_bindgen(js_name = wrapS)]
    pub wrap_s: TextureWrap,
    #[wasm_bindgen(js_name = wrapT)]
    pub wrap_t: TextureWrap,
    #[wasm_bindgen(js_name = wrapR)]
    pub wrap_r: TextureWrap,
    #[wasm_bindgen(js_name = minLod)]
    pub min_lod: f32,
    #[wasm_bindgen(js_name = maxLod)]
    pub max_lod: f32,
    /// `CompareRefToTexture` for shadow samplers (`sampler2DShadow`).
    #[wasm_bindgen(js_name = compareMode)]
    pub compare_mode: TextureCompareMode,
    #[wasm_bindgen(js_name = compareFunc)]
    pub compare_func: DepthFunc,
    /// Above 1 needs `Extension::ExtTextureFilterAnisotropic` enabled;
    /// otherwise it is ignored (with a warning).
    #[wasm_bindgen(js_name = maxAnisotropy)]
    pub max_anisotropy: f32,
}

impl Default for SamplerDesc {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl SamplerDesc {
    /// The WebGL defaults: NEAREST_MIPMAP_LINEAR / LINEAR, REPEAT, LOD
    /// -1000..1000, no compare.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            min_filter: TextureMinFilter::NearestMipmapLinear,
            mag_filter: TextureMagFilter::Linear,
            wrap_s: TextureWrap::Repeat,
            wrap_t: TextureWrap::Repeat,
            wrap_r: TextureWrap::Repeat,
            min_lod: -1000.0,
            max_lod: 1000.0,
            compare_mode: TextureCompareMode::None,
            compare_func: DepthFunc::LessEqual,
            max_anisotropy: 1.0,
        }
    }

    /// Bilinear, no mipmaps, clamped: render targets, post-processing.
    #[wasm_bindgen(js_name = linearClamp)]
    pub fn linear_clamp() -> Self {
        Self::with(
            TextureMinFilter::Linear,
            TextureMagFilter::Linear,
            TextureWrap::ClampToEdge,
        )
    }

    /// Exact texel reads, clamped: lookup tables, data textures, copies.
    #[wasm_bindgen(js_name = nearestClamp)]
    pub fn nearest_clamp() -> Self {
        Self::with(
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
            TextureWrap::ClampToEdge,
        )
    }

    /// Trilinear, repeating: mipmapped material textures.
    #[wasm_bindgen(js_name = trilinearRepeat)]
    pub fn trilinear_repeat() -> Self {
        Self::with(
            TextureMinFilter::LinearMipmapLinear,
            TextureMagFilter::Linear,
            TextureWrap::Repeat,
        )
    }
}

impl SamplerDesc {
    fn with(min: TextureMinFilter, mag: TextureMagFilter, wrap: TextureWrap) -> Self {
        Self {
            min_filter: min,
            mag_filter: mag,
            wrap_s: wrap,
            wrap_t: wrap,
            wrap_r: wrap,
            ..Self::new()
        }
    }
}

pub(crate) struct SamplerObj {
    pub(crate) raw: WebGlSampler,
    pub(crate) desc: SamplerDesc,
}

#[wasm_bindgen]
impl Context {
    /// Creates a sampler with `desc`'s parameters.
    #[wasm_bindgen(js_name = createSampler)]
    pub fn create_sampler(&self, desc: &SamplerDesc) -> Result<SamplerId, Error> {
        self.check_alive()?;
        let gl = self.gl();
        let raw = gl.create_sampler().ok_or("createSampler failed")?;
        let i = |pname: u32, v: u32| gl.sampler_parameteri(&raw, pname, v as i32);
        i(Gl::TEXTURE_MIN_FILTER, desc.min_filter as u32);
        i(Gl::TEXTURE_MAG_FILTER, desc.mag_filter as u32);
        i(Gl::TEXTURE_WRAP_S, desc.wrap_s as u32);
        i(Gl::TEXTURE_WRAP_T, desc.wrap_t as u32);
        i(Gl::TEXTURE_WRAP_R, desc.wrap_r as u32);
        i(Gl::TEXTURE_COMPARE_MODE, desc.compare_mode as u32);
        i(Gl::TEXTURE_COMPARE_FUNC, desc.compare_func.as_gl());
        gl.sampler_parameterf(&raw, Gl::TEXTURE_MIN_LOD, desc.min_lod);
        gl.sampler_parameterf(&raw, Gl::TEXTURE_MAX_LOD, desc.max_lod);
        if desc.max_anisotropy > 1.0 {
            if self.is_extension_enabled(Extension::ExtTextureFilterAnisotropic) {
                gl.sampler_parameterf(&raw, TEXTURE_MAX_ANISOTROPY_EXT, desc.max_anisotropy);
            } else {
                console::warn(
                    "[rswebgl] create_sampler: max_anisotropy needs \
                     ExtTextureFilterAnisotropic enabled; ignored",
                );
            }
        }
        self.res()
            .samplers
            .insert(SamplerObj { raw, desc: *desc })
            .map(Into::into)
            .map_err(|e| Error::from(e.to_string()))
    }

    /// Deletes the sampler. Groups that still name it report it at render and
    /// fall back to the texture's own parameters.
    #[wasm_bindgen(js_name = destroySampler)]
    pub fn destroy_sampler(&self, sampler: SamplerId) {
        let destroyed = self.res().samplers.destroy(sampler.0);
        match destroyed {
            Destroyed::Now(obj) => self.gl().delete_sampler(Some(&obj.raw)),
            Destroyed::Deferred => {}
            Destroyed::Invalid => stale("sampler", "destroy_sampler", sampler),
        }
    }

    /// Whether `sampler` names a live sampler.
    #[wasm_bindgen(js_name = samplerIsValid)]
    pub fn sampler_is_valid(&self, sampler: SamplerId) -> bool {
        self.res().samplers.get(sampler.0).is_some()
    }

    /// The parameters `sampler` was created with (WebGL defaults for a stale
    /// handle).
    #[wasm_bindgen(js_name = samplerDesc)]
    pub fn sampler_desc(&self, sampler: SamplerId) -> SamplerDesc {
        self.res()
            .samplers
            .get(sampler.0)
            .map_or_else(SamplerDesc::new, |s| s.desc)
    }
}
