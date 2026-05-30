use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Extension {
    // --- Ratified ---
    ExtColorBufferFloat,
    ExtColorBufferHalfFloat,
    ExtDisjointTimerQueryWebgl2,
    ExtFloatBlend,
    ExtTextureCompressionBptc,
    ExtTextureCompressionRgtc,
    ExtTextureFilterAnisotropic,
    ExtTextureNorm16,
    KhrParallelShaderCompile,
    OesDrawBuffersIndexed,
    OesTextureFloatLinear,
    OvrMultiview2,
    WebglBlendEquationAdvancedCoherent,
    WebglClipCullDistance,
    WebglCompressedTextureAstc,
    WebglCompressedTextureEtc,
    WebglCompressedTextureEtc1,
    WebglCompressedTexturePvrtc,
    WebglCompressedTextureS3tc,
    WebglCompressedTextureS3tcSrgb,
    WebglDebugRendererInfo,
    WebglDebugShaders,
    WebglDrawInstancedBaseVertexBaseInstance,
    WebglLoseContext,
    WebglMultiDraw,
    WebglMultiDrawInstancedBaseVertexBaseInstance,
    WebglProvokingVertex,
    WebglStencilTexturing,
    // --- Draft ---
    ExtClipControl,
    ExtDepthClamp,
    ExtPolygonOffsetClamp,
    ExtRenderSnorm,
    ExtTextureMirrorClampToEdge,
    NvShaderNoperspectiveInterpolation,
    WebglShaderPixelLocalStorage,
}

impl Extension {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ExtColorBufferFloat => "EXT_color_buffer_float",
            Self::ExtColorBufferHalfFloat => "EXT_color_buffer_half_float",
            Self::ExtDisjointTimerQueryWebgl2 => "EXT_disjoint_timer_query_webgl2",
            Self::ExtFloatBlend => "EXT_float_blend",
            Self::ExtTextureCompressionBptc => "EXT_texture_compression_bptc",
            Self::ExtTextureCompressionRgtc => "EXT_texture_compression_rgtc",
            Self::ExtTextureFilterAnisotropic => "EXT_texture_filter_anisotropic",
            Self::ExtTextureNorm16 => "EXT_texture_norm16",
            Self::KhrParallelShaderCompile => "KHR_parallel_shader_compile",
            Self::OesDrawBuffersIndexed => "OES_draw_buffers_indexed",
            Self::OesTextureFloatLinear => "OES_texture_float_linear",
            Self::OvrMultiview2 => "OVR_multiview2",
            Self::WebglBlendEquationAdvancedCoherent => "WEBGL_blend_equation_advanced_coherent",
            Self::WebglClipCullDistance => "WEBGL_clip_cull_distance",
            Self::WebglCompressedTextureAstc => "WEBGL_compressed_texture_astc",
            Self::WebglCompressedTextureEtc => "WEBGL_compressed_texture_etc",
            Self::WebglCompressedTextureEtc1 => "WEBGL_compressed_texture_etc1",
            Self::WebglCompressedTexturePvrtc => "WEBGL_compressed_texture_pvrtc",
            Self::WebglCompressedTextureS3tc => "WEBGL_compressed_texture_s3tc",
            Self::WebglCompressedTextureS3tcSrgb => "WEBGL_compressed_texture_s3tc_srgb",
            Self::WebglDebugRendererInfo => "WEBGL_debug_renderer_info",
            Self::WebglDebugShaders => "WEBGL_debug_shaders",
            Self::WebglDrawInstancedBaseVertexBaseInstance => {
                "WEBGL_draw_instanced_base_vertex_base_instance"
            }
            Self::WebglLoseContext => "WEBGL_lose_context",
            Self::WebglMultiDraw => "WEBGL_multi_draw",
            Self::WebglMultiDrawInstancedBaseVertexBaseInstance => {
                "WEBGL_multi_draw_instanced_base_vertex_base_instance"
            }
            Self::WebglProvokingVertex => "WEBGL_provoking_vertex",
            Self::WebglStencilTexturing => "WEBGL_stencil_texturing",
            Self::ExtClipControl => "EXT_clip_control",
            Self::ExtDepthClamp => "EXT_depth_clamp",
            Self::ExtPolygonOffsetClamp => "EXT_polygon_offset_clamp",
            Self::ExtRenderSnorm => "EXT_render_snorm",
            Self::ExtTextureMirrorClampToEdge => "EXT_texture_mirror_clamp_to_edge",
            Self::NvShaderNoperspectiveInterpolation => "NV_shader_noperspective_interpolation",
            Self::WebglShaderPixelLocalStorage => "WEBGL_shader_pixel_local_storage",
        }
    }
}
