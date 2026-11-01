// Float render targets (HDR, bloom, deferred, GPGPU ping-pong).
//
// Core WebGL2 can sample float textures but can't render into them: that takes
// `EXT_color_buffer_float` (16F, 32F and R11F_G11F_B10F) or, on devices that
// only have half floats (common on mobile), `EXT_color_buffer_half_float`
// (16F). Without them the GL answer is an INVALID_ENUM from
// `renderbufferStorage` or a framebuffer that is silently incomplete.
//
// So the context enables these when it is created (they cost nothing until
// used, and restore re-enables them), together with `OES_texture_float_linear`
// (linear filtering of 32F textures, else they sample as black) and
// `EXT_float_blend` (blending into 32F targets). Creating or attaching a float
// target the device can't render to is then reported up front, with the
// extension it lacks; `can_render_to` lets an app pick a format beforehand
// (e.g. RGBA16F, falling back to RGBA8).

use wasm_bindgen::prelude::*;

use crate::context::Context;
use crate::extension::Extension;
use crate::renderbuffer::RenderbufferFormat;
use crate::texture::TextureFormat;

/// What rendering into a float color format takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FloatNeed {
    /// 16-bit float: `EXT_color_buffer_float` or `EXT_color_buffer_half_float`.
    Half,
    /// 32-bit float or R11F_G11F_B10F: `EXT_color_buffer_float`.
    Full,
}

/// For a sized internal format: what rendering into it needs, None if it is
/// not a float color format (or never renderable, like RGB16F / RGB32F —
/// those are left to GL's own completeness check).
pub(crate) fn float_need(internal: u32) -> Option<FloatNeed> {
    match internal {
        0x822D | 0x822F | 0x881A => Some(FloatNeed::Half), // R16F, RG16F, RGBA16F
        0x822E | 0x8230 | 0x8814 | 0x8C3A => Some(FloatNeed::Full), // R32F, RG32F, RGBA32F, R11F_G11F_B10F
        _ => None,
    }
}

/// 32-bit float formats, which filter linearly only with
/// `OES_texture_float_linear`.
pub(crate) fn is_float32(internal: u32) -> bool {
    matches!(internal, 0x822E | 0x8230 | 0x8815 | 0x8814) // R32F, RG32F, RGB32F, RGBA32F
}

/// Enabled at context creation; see the module comment.
pub(crate) const AUTO_EXTENSIONS: [Extension; 4] = [
    Extension::ExtColorBufferFloat,
    Extension::ExtColorBufferHalfFloat,
    Extension::OesTextureFloatLinear,
    Extension::ExtFloatBlend,
];

impl Context {
    pub(crate) fn enable_float_extensions(&self) {
        for ext in AUTO_EXTENSIONS {
            self.enable_extension_quietly(ext);
        }
    }

    /// Why color format `internal` can't be rendered to here, if it can't.
    pub(crate) fn float_target_problem(&self, internal: u32) -> Option<String> {
        let name = format_name(internal);
        match float_need(internal)? {
            FloatNeed::Half
                if !self.is_extension_enabled(Extension::ExtColorBufferFloat)
                    && !self.is_extension_enabled(Extension::ExtColorBufferHalfFloat) =>
            {
                Some(format!(
                    "rendering into {name} needs EXT_color_buffer_float or \
                     EXT_color_buffer_half_float, which this device doesn't have"
                ))
            }
            FloatNeed::Full if !self.is_extension_enabled(Extension::ExtColorBufferFloat) => {
                Some(format!(
                    "rendering into {name} needs EXT_color_buffer_float, which this device \
                     doesn't have; RGBA16F may still work (see can_render_to)"
                ))
            }
            _ => None,
        }
    }
}

fn format_name(internal: u32) -> &'static str {
    match internal {
        0x822D => "R16F",
        0x822F => "RG16F",
        0x881A => "RGBA16F",
        0x822E => "R32F",
        0x8230 => "RG32F",
        0x8814 => "RGBA32F",
        0x8C3A => "R11F_G11F_B10F",
        _ => "this format",
    }
}

#[wasm_bindgen]
impl Context {
    /// Whether a texture of `format` can be a color attachment here. Only
    /// float formats depend on the device; the rest answer true (depth formats
    /// go to the depth slot).
    #[wasm_bindgen(js_name = canRenderTo)]
    pub fn can_render_to(&self, format: &TextureFormat) -> bool {
        self.float_target_problem(format.internal as u32).is_none()
    }

    /// `can_render_to` for a renderbuffer format.
    #[wasm_bindgen(js_name = canRenderToRenderbuffer)]
    pub fn can_render_to_renderbuffer(&self, format: RenderbufferFormat) -> bool {
        self.float_target_problem(format as u32).is_none()
    }

    /// Whether 32-bit float textures can use linear filtering
    /// (`OES_texture_float_linear`). Without it they need `Nearest`, or they
    /// sample as black.
    #[wasm_bindgen(js_name = canFilterFloat32)]
    pub fn can_filter_float32(&self) -> bool {
        self.is_extension_enabled(Extension::OesTextureFloatLinear)
    }
}
