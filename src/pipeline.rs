use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

// ---------------------------------------------------------------------------
// DepthFunc
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthFunc {
    Never        = 0x0200,
    Less         = 0x0201,
    Equal        = 0x0202,
    LessEqual    = 0x0203,
    Greater      = 0x0204,
    NotEqual     = 0x0205,
    GreaterEqual = 0x0206,
    Always       = 0x0207,
}

impl DepthFunc {
    fn as_gl(&self) -> u32 { self.clone() as u32 }
}

// ---------------------------------------------------------------------------
// BlendFactor
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendFactor {
    Zero                  = 0,
    One                   = 1,
    SrcColor              = 0x0300,
    OneMinusSrcColor      = 0x0301,
    SrcAlpha              = 0x0302,
    OneMinusSrcAlpha      = 0x0303,
    DstAlpha              = 0x0304,
    OneMinusDstAlpha      = 0x0305,
    DstColor              = 0x0306,
    OneMinusDstColor      = 0x0307,
    SrcAlphaSaturate      = 0x0308,
    ConstantColor         = 0x8001,
    OneMinusConstantColor = 0x8002,
    ConstantAlpha         = 0x8003,
    OneMinusConstantAlpha = 0x8004,
}

impl BlendFactor {
    fn as_gl(&self) -> u32 { self.clone() as u32 }
}

// ---------------------------------------------------------------------------
// BlendEquation
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendEquation {
    Add             = 0x8006,
    Subtract        = 0x800A,
    ReverseSubtract = 0x800B,
    Min             = 0x8007,
    Max             = 0x8008,
}

impl BlendEquation {
    fn as_gl(&self) -> u32 { self.clone() as u32 }
}

// ---------------------------------------------------------------------------
// CullFace
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullFace {
    Front        = 0x0404,
    Back         = 0x0405,
    FrontAndBack = 0x0408,
}

impl CullFace {
    fn as_gl(&self) -> u32 { self.clone() as u32 }
}

// ---------------------------------------------------------------------------
// FrontFace
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontFace {
    Cw  = 0x0900,
    Ccw = 0x0901,
}

impl FrontFace {
    fn as_gl(&self) -> u32 { self.clone() as u32 }
}

// ---------------------------------------------------------------------------
// StencilFunc / StencilOp
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StencilFunc {
    Never        = 0x0200,
    Less         = 0x0201,
    Equal        = 0x0202,
    LessEqual    = 0x0203,
    Greater      = 0x0204,
    NotEqual     = 0x0205,
    GreaterEqual = 0x0206,
    Always       = 0x0207,
}

impl StencilFunc {
    fn as_gl(&self) -> u32 { self.clone() as u32 }
}

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StencilOp {
    Keep     = 0x1E00,
    Zero     = 0,
    Replace  = 0x1E01,
    Incr     = 0x1E02,
    IncrWrap = 0x8507,
    Decr     = 0x1E03,
    DecrWrap = 0x8508,
    Invert   = 0x150A,
}

impl StencilOp {
    fn as_gl(&self) -> u32 { self.clone() as u32 }
}

// ---------------------------------------------------------------------------
// PipelineState
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug)]
pub struct PipelineState {
    pub depth_test: bool,
    pub depth_func: DepthFunc,
    pub depth_mask: bool,

    pub blend: bool,
    pub blend_src_rgb: BlendFactor,
    pub blend_dst_rgb: BlendFactor,
    pub blend_src_alpha: BlendFactor,
    pub blend_dst_alpha: BlendFactor,
    pub blend_eq_rgb: BlendEquation,
    pub blend_eq_alpha: BlendEquation,

    pub cull_face: bool,
    pub cull_mode: CullFace,
    pub front_face: FrontFace,

    pub stencil_test: bool,
    pub stencil_func: StencilFunc,
    pub stencil_ref: i32,
    pub stencil_mask: u32,
    pub stencil_fail: StencilOp,
    pub stencil_depth_fail: StencilOp,
    pub stencil_pass: StencilOp,
    pub stencil_write_mask: u32,

    pub scissor_test: bool,
    pub scissor_x: i32,
    pub scissor_y: i32,
    pub scissor_width: i32,
    pub scissor_height: i32,

    pub color_mask_r: bool,
    pub color_mask_g: bool,
    pub color_mask_b: bool,
    pub color_mask_a: bool,

    pub polygon_offset_fill: bool,
    pub polygon_offset_factor: f32,
    pub polygon_offset_units: f32,
}

#[wasm_bindgen]
impl PipelineState {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for PipelineState {
    fn default() -> Self {
        Self {
            depth_test: false,
            depth_func: DepthFunc::Less,
            depth_mask: true,

            blend: false,
            blend_src_rgb: BlendFactor::One,
            blend_dst_rgb: BlendFactor::Zero,
            blend_src_alpha: BlendFactor::One,
            blend_dst_alpha: BlendFactor::Zero,
            blend_eq_rgb: BlendEquation::Add,
            blend_eq_alpha: BlendEquation::Add,

            cull_face: false,
            cull_mode: CullFace::Back,
            front_face: FrontFace::Ccw,

            stencil_test: false,
            stencil_func: StencilFunc::Always,
            stencil_ref: 0,
            stencil_mask: 0xFFFFFFFF,
            stencil_fail: StencilOp::Keep,
            stencil_depth_fail: StencilOp::Keep,
            stencil_pass: StencilOp::Keep,
            stencil_write_mask: 0xFFFFFFFF,

            scissor_test: false,
            scissor_x: 0,
            scissor_y: 0,
            scissor_width: 0,
            scissor_height: 0,

            color_mask_r: true,
            color_mask_g: true,
            color_mask_b: true,
            color_mask_a: true,

            polygon_offset_fill: false,
            polygon_offset_factor: 0.0,
            polygon_offset_units: 0.0,
        }
    }
}

impl PipelineState {
    pub(crate) fn apply(&self, gl: &WebGl2RenderingContext) {
        // Depth
        if self.depth_test {
            gl.enable(WebGl2RenderingContext::DEPTH_TEST);
            gl.depth_func(self.depth_func.as_gl());
        } else {
            gl.disable(WebGl2RenderingContext::DEPTH_TEST);
        }
        gl.depth_mask(self.depth_mask);

        // Blend
        if self.blend {
            gl.enable(WebGl2RenderingContext::BLEND);
            gl.blend_func_separate(
                self.blend_src_rgb.as_gl(),
                self.blend_dst_rgb.as_gl(),
                self.blend_src_alpha.as_gl(),
                self.blend_dst_alpha.as_gl(),
            );
            gl.blend_equation_separate(
                self.blend_eq_rgb.as_gl(),
                self.blend_eq_alpha.as_gl(),
            );
        } else {
            gl.disable(WebGl2RenderingContext::BLEND);
        }

        // Cull face
        if self.cull_face {
            gl.enable(WebGl2RenderingContext::CULL_FACE);
            gl.cull_face(self.cull_mode.as_gl());
        } else {
            gl.disable(WebGl2RenderingContext::CULL_FACE);
        }
        gl.front_face(self.front_face.as_gl());

        // Stencil
        if self.stencil_test {
            gl.enable(WebGl2RenderingContext::STENCIL_TEST);
            gl.stencil_func(self.stencil_func.as_gl(), self.stencil_ref, self.stencil_mask);
            gl.stencil_op(
                self.stencil_fail.as_gl(),
                self.stencil_depth_fail.as_gl(),
                self.stencil_pass.as_gl(),
            );
            gl.stencil_mask(self.stencil_write_mask);
        } else {
            gl.disable(WebGl2RenderingContext::STENCIL_TEST);
        }

        // Scissor
        if self.scissor_test {
            gl.enable(WebGl2RenderingContext::SCISSOR_TEST);
            gl.scissor(self.scissor_x, self.scissor_y, self.scissor_width, self.scissor_height);
        } else {
            gl.disable(WebGl2RenderingContext::SCISSOR_TEST);
        }

        // Color mask
        gl.color_mask(self.color_mask_r, self.color_mask_g, self.color_mask_b, self.color_mask_a);

        // Polygon offset
        if self.polygon_offset_fill {
            gl.enable(WebGl2RenderingContext::POLYGON_OFFSET_FILL);
            gl.polygon_offset(self.polygon_offset_factor, self.polygon_offset_units);
        } else {
            gl.disable(WebGl2RenderingContext::POLYGON_OFFSET_FILL);
        }
    }
}
