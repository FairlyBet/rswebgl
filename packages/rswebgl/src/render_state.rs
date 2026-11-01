use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::context::Context;
use crate::ids::StateId;

// ---------------------------------------------------------------------------
// DepthFunc
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthFunc {
    Never = 0x0200,
    Less = 0x0201,
    Equal = 0x0202,
    LessEqual = 0x0203,
    Greater = 0x0204,
    NotEqual = 0x0205,
    GreaterEqual = 0x0206,
    Always = 0x0207,
}

impl DepthFunc {
    pub(crate) fn as_gl(&self) -> u32 {
        *self as u32
    }
}

// ---------------------------------------------------------------------------
// BlendFactor
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendFactor {
    Zero = 0,
    One = 1,
    SrcColor = 0x0300,
    OneMinusSrcColor = 0x0301,
    SrcAlpha = 0x0302,
    OneMinusSrcAlpha = 0x0303,
    DstAlpha = 0x0304,
    OneMinusDstAlpha = 0x0305,
    DstColor = 0x0306,
    OneMinusDstColor = 0x0307,
    SrcAlphaSaturate = 0x0308,
    ConstantColor = 0x8001,
    OneMinusConstantColor = 0x8002,
    ConstantAlpha = 0x8003,
    OneMinusConstantAlpha = 0x8004,
}

impl BlendFactor {
    fn as_gl(&self) -> u32 {
        *self as u32
    }
}

// ---------------------------------------------------------------------------
// BlendEquation
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendEquation {
    Add = 0x8006,
    Subtract = 0x800A,
    ReverseSubtract = 0x800B,
    Min = 0x8007,
    Max = 0x8008,
}

impl BlendEquation {
    fn as_gl(&self) -> u32 {
        *self as u32
    }
}

// ---------------------------------------------------------------------------
// CullFace
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CullFace {
    Front = 0x0404,
    Back = 0x0405,
    FrontAndBack = 0x0408,
}

impl CullFace {
    fn as_gl(&self) -> u32 {
        *self as u32
    }
}

// ---------------------------------------------------------------------------
// FrontFace
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontFace {
    Cw = 0x0900,
    Ccw = 0x0901,
}

impl FrontFace {
    fn as_gl(&self) -> u32 {
        *self as u32
    }
}

// ---------------------------------------------------------------------------
// StencilFunc / StencilOp
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StencilFunc {
    Never = 0x0200,
    Less = 0x0201,
    Equal = 0x0202,
    LessEqual = 0x0203,
    Greater = 0x0204,
    NotEqual = 0x0205,
    GreaterEqual = 0x0206,
    Always = 0x0207,
}

impl StencilFunc {
    fn as_gl(&self) -> u32 {
        *self as u32
    }
}

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StencilOp {
    Keep = 0x1E00,
    Zero = 0,
    Replace = 0x1E01,
    Incr = 0x1E02,
    IncrWrap = 0x8507,
    Decr = 0x1E03,
    DecrWrap = 0x8508,
    Invert = 0x150A,
}

impl StencilOp {
    fn as_gl(&self) -> u32 {
        *self as u32
    }
}

// ---------------------------------------------------------------------------
// RenderState — fixed-function GL pipeline state (no shaders).
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq)]
pub struct RenderState {
    #[wasm_bindgen(js_name = depthTest)]
    pub depth_test: bool,
    #[wasm_bindgen(js_name = depthFunc)]
    pub depth_func: DepthFunc,
    #[wasm_bindgen(js_name = depthMask)]
    pub depth_mask: bool,

    pub blend: bool,
    #[wasm_bindgen(js_name = blendSrcRgb)]
    pub blend_src_rgb: BlendFactor,
    #[wasm_bindgen(js_name = blendDstRgb)]
    pub blend_dst_rgb: BlendFactor,
    #[wasm_bindgen(js_name = blendSrcAlpha)]
    pub blend_src_alpha: BlendFactor,
    #[wasm_bindgen(js_name = blendDstAlpha)]
    pub blend_dst_alpha: BlendFactor,
    #[wasm_bindgen(js_name = blendEqRgb)]
    pub blend_eq_rgb: BlendEquation,
    #[wasm_bindgen(js_name = blendEqAlpha)]
    pub blend_eq_alpha: BlendEquation,

    #[wasm_bindgen(js_name = cullFace)]
    pub cull_face: bool,
    #[wasm_bindgen(js_name = cullMode)]
    pub cull_mode: CullFace,
    #[wasm_bindgen(js_name = frontFace)]
    pub front_face: FrontFace,

    #[wasm_bindgen(js_name = stencilTest)]
    pub stencil_test: bool,
    #[wasm_bindgen(js_name = stencilFunc)]
    pub stencil_func: StencilFunc,
    #[wasm_bindgen(js_name = stencilRef)]
    pub stencil_ref: i32,
    #[wasm_bindgen(js_name = stencilMask)]
    pub stencil_mask: u32,
    #[wasm_bindgen(js_name = stencilFail)]
    pub stencil_fail: StencilOp,
    #[wasm_bindgen(js_name = stencilDepthFail)]
    pub stencil_depth_fail: StencilOp,
    #[wasm_bindgen(js_name = stencilPass)]
    pub stencil_pass: StencilOp,
    #[wasm_bindgen(js_name = stencilWriteMask)]
    pub stencil_write_mask: u32,

    #[wasm_bindgen(js_name = scissorTest)]
    pub scissor_test: bool,
    #[wasm_bindgen(js_name = scissorX)]
    pub scissor_x: i32,
    #[wasm_bindgen(js_name = scissorY)]
    pub scissor_y: i32,
    #[wasm_bindgen(js_name = scissorWidth)]
    pub scissor_width: i32,
    #[wasm_bindgen(js_name = scissorHeight)]
    pub scissor_height: i32,

    #[wasm_bindgen(js_name = colorMaskR)]
    pub color_mask_r: bool,
    #[wasm_bindgen(js_name = colorMaskG)]
    pub color_mask_g: bool,
    #[wasm_bindgen(js_name = colorMaskB)]
    pub color_mask_b: bool,
    #[wasm_bindgen(js_name = colorMaskA)]
    pub color_mask_a: bool,

    #[wasm_bindgen(js_name = polygonOffsetFill)]
    pub polygon_offset_fill: bool,
    #[wasm_bindgen(js_name = polygonOffsetFactor)]
    pub polygon_offset_factor: f32,
    #[wasm_bindgen(js_name = polygonOffsetUnits)]
    pub polygon_offset_units: f32,
}

#[wasm_bindgen]
impl RenderState {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for RenderState {
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

impl RenderState {
    // Every parameter is set, enabled or not: `apply_diff` compares them all,
    // so a parameter skipped here would be assumed set by the next diff.
    pub(crate) fn apply(&self, gl: &WebGl2RenderingContext) {
        type Gl = WebGl2RenderingContext;
        let toggle = |cap: u32, on: bool| {
            if on { gl.enable(cap) } else { gl.disable(cap) }
        };

        toggle(Gl::DEPTH_TEST, self.depth_test);
        gl.depth_func(self.depth_func.as_gl());
        gl.depth_mask(self.depth_mask);

        toggle(Gl::BLEND, self.blend);
        gl.blend_func_separate(
            self.blend_src_rgb.as_gl(),
            self.blend_dst_rgb.as_gl(),
            self.blend_src_alpha.as_gl(),
            self.blend_dst_alpha.as_gl(),
        );
        gl.blend_equation_separate(self.blend_eq_rgb.as_gl(), self.blend_eq_alpha.as_gl());

        toggle(Gl::CULL_FACE, self.cull_face);
        gl.cull_face(self.cull_mode.as_gl());
        gl.front_face(self.front_face.as_gl());

        toggle(Gl::STENCIL_TEST, self.stencil_test);
        gl.stencil_func(
            self.stencil_func.as_gl(),
            self.stencil_ref,
            self.stencil_mask,
        );
        gl.stencil_op(
            self.stencil_fail.as_gl(),
            self.stencil_depth_fail.as_gl(),
            self.stencil_pass.as_gl(),
        );
        gl.stencil_mask(self.stencil_write_mask);

        toggle(Gl::SCISSOR_TEST, self.scissor_test);
        gl.scissor(
            self.scissor_x,
            self.scissor_y,
            self.scissor_width,
            self.scissor_height,
        );

        gl.color_mask(
            self.color_mask_r,
            self.color_mask_g,
            self.color_mask_b,
            self.color_mask_a,
        );

        toggle(Gl::POLYGON_OFFSET_FILL, self.polygon_offset_fill);
        gl.polygon_offset(self.polygon_offset_factor, self.polygon_offset_units);
    }

    pub(crate) fn apply_diff(&self, prev: &Self, gl: &WebGl2RenderingContext) {
        // Depth
        if self.depth_test != prev.depth_test {
            if self.depth_test {
                gl.enable(WebGl2RenderingContext::DEPTH_TEST);
            } else {
                gl.disable(WebGl2RenderingContext::DEPTH_TEST);
            }
        }
        if self.depth_func != prev.depth_func {
            gl.depth_func(self.depth_func.as_gl());
        }
        if self.depth_mask != prev.depth_mask {
            gl.depth_mask(self.depth_mask);
        }

        // Blend
        if self.blend != prev.blend {
            if self.blend {
                gl.enable(WebGl2RenderingContext::BLEND);
            } else {
                gl.disable(WebGl2RenderingContext::BLEND);
            }
        }
        if self.blend_src_rgb != prev.blend_src_rgb
            || self.blend_dst_rgb != prev.blend_dst_rgb
            || self.blend_src_alpha != prev.blend_src_alpha
            || self.blend_dst_alpha != prev.blend_dst_alpha
        {
            gl.blend_func_separate(
                self.blend_src_rgb.as_gl(),
                self.blend_dst_rgb.as_gl(),
                self.blend_src_alpha.as_gl(),
                self.blend_dst_alpha.as_gl(),
            );
        }
        if self.blend_eq_rgb != prev.blend_eq_rgb || self.blend_eq_alpha != prev.blend_eq_alpha {
            gl.blend_equation_separate(self.blend_eq_rgb.as_gl(), self.blend_eq_alpha.as_gl());
        }

        // Cull face
        if self.cull_face != prev.cull_face {
            if self.cull_face {
                gl.enable(WebGl2RenderingContext::CULL_FACE);
            } else {
                gl.disable(WebGl2RenderingContext::CULL_FACE);
            }
        }
        if self.cull_mode != prev.cull_mode {
            gl.cull_face(self.cull_mode.as_gl());
        }
        if self.front_face != prev.front_face {
            gl.front_face(self.front_face.as_gl());
        }

        // Stencil
        if self.stencil_test != prev.stencil_test {
            if self.stencil_test {
                gl.enable(WebGl2RenderingContext::STENCIL_TEST);
            } else {
                gl.disable(WebGl2RenderingContext::STENCIL_TEST);
            }
        }
        if self.stencil_func != prev.stencil_func
            || self.stencil_ref != prev.stencil_ref
            || self.stencil_mask != prev.stencil_mask
        {
            gl.stencil_func(
                self.stencil_func.as_gl(),
                self.stencil_ref,
                self.stencil_mask,
            );
        }
        if self.stencil_fail != prev.stencil_fail
            || self.stencil_depth_fail != prev.stencil_depth_fail
            || self.stencil_pass != prev.stencil_pass
        {
            gl.stencil_op(
                self.stencil_fail.as_gl(),
                self.stencil_depth_fail.as_gl(),
                self.stencil_pass.as_gl(),
            );
        }
        if self.stencil_write_mask != prev.stencil_write_mask {
            gl.stencil_mask(self.stencil_write_mask);
        }

        // Scissor
        if self.scissor_test != prev.scissor_test {
            if self.scissor_test {
                gl.enable(WebGl2RenderingContext::SCISSOR_TEST);
            } else {
                gl.disable(WebGl2RenderingContext::SCISSOR_TEST);
            }
        }
        if self.scissor_x != prev.scissor_x
            || self.scissor_y != prev.scissor_y
            || self.scissor_width != prev.scissor_width
            || self.scissor_height != prev.scissor_height
        {
            gl.scissor(
                self.scissor_x,
                self.scissor_y,
                self.scissor_width,
                self.scissor_height,
            );
        }

        // Color mask
        if self.color_mask_r != prev.color_mask_r
            || self.color_mask_g != prev.color_mask_g
            || self.color_mask_b != prev.color_mask_b
            || self.color_mask_a != prev.color_mask_a
        {
            gl.color_mask(
                self.color_mask_r,
                self.color_mask_g,
                self.color_mask_b,
                self.color_mask_a,
            );
        }

        // Polygon offset
        if self.polygon_offset_fill != prev.polygon_offset_fill {
            if self.polygon_offset_fill {
                gl.enable(WebGl2RenderingContext::POLYGON_OFFSET_FILL);
            } else {
                gl.disable(WebGl2RenderingContext::POLYGON_OFFSET_FILL);
            }
        }
        if self.polygon_offset_factor != prev.polygon_offset_factor
            || self.polygon_offset_units != prev.polygon_offset_units
        {
            gl.polygon_offset(self.polygon_offset_factor, self.polygon_offset_units);
        }
    }
}

#[wasm_bindgen]
impl Context {
    /// The id of `state`, interned: equal states get the same id, so draws
    /// compare render states as numbers and the full diff runs only on an
    /// actual change. States are small and never freed (an app has a handful).
    #[wasm_bindgen(js_name = renderState)]
    pub fn render_state(&self, state: &RenderState) -> StateId {
        let mut res = self.res();
        if let Some((h, _)) = res.render_states.iter().find(|(_, s)| *s == state) {
            return h.into();
        }
        res.render_states
            .insert(state.clone())
            .map(Into::into)
            .unwrap_or(StateId::NULL)
    }
}
