use wasm_bindgen::prelude::*;

/// A compressed sized internal format for `compressed_storage_*` /
/// `compressed_sub_image_*`. Just wraps the GL enum value.
///
/// Each family requires its extension to be enabled first (via
/// `Context::enable_extension`) or the GL call raises `INVALID_ENUM`:
/// - S3TC / S3TC-sRGB → `WebglCompressedTextureS3tc` / `…S3tcSrgb`
/// - ETC2 / EAC → `WebglCompressedTextureEtc`
/// - ASTC (LDR) → `WebglCompressedTextureAstc`
/// - BPTC → `ExtTextureCompressionBptc`
/// - RGTC → `ExtTextureCompressionRgtc`
///
/// For broad coverage, MDN suggests S3TC (desktop) + ETC (mobile); ASTC is
/// higher quality but newer-hardware only.
#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompressedFormat {
    pub internal: u32,
}

impl CompressedFormat {
    pub(crate) fn as_gl(&self) -> u32 {
        self.internal
    }
}

macro_rules! compressed_formats {
    ($(($cst:ident, $f:ident, $v:expr)),* $(,)?) => {
        impl CompressedFormat {
            $(pub const $cst: Self = Self { internal: $v };)*
        }
        #[wasm_bindgen]
        impl CompressedFormat {
            /// Wrap a raw GL compressed internal-format enum (escape hatch for
            /// formats not covered by the named constants).
            pub fn from_internal(internal: u32) -> CompressedFormat {
                CompressedFormat { internal }
            }
            $(pub fn $f() -> CompressedFormat { CompressedFormat::$cst })*
        }
    };
}

compressed_formats![
    // --- S3TC (DXT) ---
    (RGB_S3TC_DXT1, rgb_s3tc_dxt1, 0x83F0),
    (RGBA_S3TC_DXT1, rgba_s3tc_dxt1, 0x83F1),
    (RGBA_S3TC_DXT3, rgba_s3tc_dxt3, 0x83F2),
    (RGBA_S3TC_DXT5, rgba_s3tc_dxt5, 0x83F3),
    // --- S3TC sRGB ---
    (SRGB_S3TC_DXT1, srgb_s3tc_dxt1, 0x8C4C),
    (SRGB_ALPHA_S3TC_DXT1, srgb_alpha_s3tc_dxt1, 0x8C4D),
    (SRGB_ALPHA_S3TC_DXT3, srgb_alpha_s3tc_dxt3, 0x8C4E),
    (SRGB_ALPHA_S3TC_DXT5, srgb_alpha_s3tc_dxt5, 0x8C4F),
    // --- ETC2 / EAC ---
    (R11_EAC, r11_eac, 0x9270),
    (SIGNED_R11_EAC, signed_r11_eac, 0x9271),
    (RG11_EAC, rg11_eac, 0x9272),
    (SIGNED_RG11_EAC, signed_rg11_eac, 0x9273),
    (RGB8_ETC2, rgb8_etc2, 0x9274),
    (SRGB8_ETC2, srgb8_etc2, 0x9275),
    (
        RGB8_PUNCHTHROUGH_ALPHA1_ETC2,
        rgb8_punchthrough_alpha1_etc2,
        0x9276
    ),
    (
        SRGB8_PUNCHTHROUGH_ALPHA1_ETC2,
        srgb8_punchthrough_alpha1_etc2,
        0x9277
    ),
    (RGBA8_ETC2_EAC, rgba8_etc2_eac, 0x9278),
    (SRGB8_ALPHA8_ETC2_EAC, srgb8_alpha8_etc2_eac, 0x9279),
    // --- ASTC LDR (RGBA) ---
    (RGBA_ASTC_4X4, rgba_astc_4x4, 0x93B0),
    (RGBA_ASTC_5X4, rgba_astc_5x4, 0x93B1),
    (RGBA_ASTC_5X5, rgba_astc_5x5, 0x93B2),
    (RGBA_ASTC_6X5, rgba_astc_6x5, 0x93B3),
    (RGBA_ASTC_6X6, rgba_astc_6x6, 0x93B4),
    (RGBA_ASTC_8X5, rgba_astc_8x5, 0x93B5),
    (RGBA_ASTC_8X6, rgba_astc_8x6, 0x93B6),
    (RGBA_ASTC_8X8, rgba_astc_8x8, 0x93B7),
    (RGBA_ASTC_10X5, rgba_astc_10x5, 0x93B8),
    (RGBA_ASTC_10X6, rgba_astc_10x6, 0x93B9),
    (RGBA_ASTC_10X8, rgba_astc_10x8, 0x93BA),
    (RGBA_ASTC_10X10, rgba_astc_10x10, 0x93BB),
    (RGBA_ASTC_12X10, rgba_astc_12x10, 0x93BC),
    (RGBA_ASTC_12X12, rgba_astc_12x12, 0x93BD),
    // --- ASTC LDR (sRGB8 alpha8) ---
    (SRGB8_ALPHA8_ASTC_4X4, srgb8_alpha8_astc_4x4, 0x93D0),
    (SRGB8_ALPHA8_ASTC_5X4, srgb8_alpha8_astc_5x4, 0x93D1),
    (SRGB8_ALPHA8_ASTC_5X5, srgb8_alpha8_astc_5x5, 0x93D2),
    (SRGB8_ALPHA8_ASTC_6X5, srgb8_alpha8_astc_6x5, 0x93D3),
    (SRGB8_ALPHA8_ASTC_6X6, srgb8_alpha8_astc_6x6, 0x93D4),
    (SRGB8_ALPHA8_ASTC_8X5, srgb8_alpha8_astc_8x5, 0x93D5),
    (SRGB8_ALPHA8_ASTC_8X6, srgb8_alpha8_astc_8x6, 0x93D6),
    (SRGB8_ALPHA8_ASTC_8X8, srgb8_alpha8_astc_8x8, 0x93D7),
    (SRGB8_ALPHA8_ASTC_10X5, srgb8_alpha8_astc_10x5, 0x93D8),
    (SRGB8_ALPHA8_ASTC_10X6, srgb8_alpha8_astc_10x6, 0x93D9),
    (SRGB8_ALPHA8_ASTC_10X8, srgb8_alpha8_astc_10x8, 0x93DA),
    (SRGB8_ALPHA8_ASTC_10X10, srgb8_alpha8_astc_10x10, 0x93DB),
    (SRGB8_ALPHA8_ASTC_12X10, srgb8_alpha8_astc_12x10, 0x93DC),
    (SRGB8_ALPHA8_ASTC_12X12, srgb8_alpha8_astc_12x12, 0x93DD),
    // --- BPTC ---
    (RGBA_BPTC_UNORM, rgba_bptc_unorm, 0x8E8C),
    (SRGB_ALPHA_BPTC_UNORM, srgb_alpha_bptc_unorm, 0x8E8D),
    (RGB_BPTC_SIGNED_FLOAT, rgb_bptc_signed_float, 0x8E8E),
    (RGB_BPTC_UNSIGNED_FLOAT, rgb_bptc_unsigned_float, 0x8E8F),
    // --- RGTC ---
    (RED_RGTC1, red_rgtc1, 0x8DBB),
    (SIGNED_RED_RGTC1, signed_red_rgtc1, 0x8DBC),
    (RED_GREEN_RGTC2, red_green_rgtc2, 0x8DBD),
    (SIGNED_RED_GREEN_RGTC2, signed_red_green_rgtc2, 0x8DBE),
];
