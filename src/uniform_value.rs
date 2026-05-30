use web_sys::{WebGl2RenderingContext, WebGlUniformLocation};

pub trait UniformValue {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation);
}

// --- scalar / vector slice wrappers ---

pub struct Floats<'a>(pub &'a [f32]);
pub struct Vec2s<'a>(pub &'a [f32]);
pub struct Vec3s<'a>(pub &'a [f32]);
pub struct Vec4s<'a>(pub &'a [f32]);

pub struct Ints<'a>(pub &'a [i32]);
pub struct IVec2s<'a>(pub &'a [i32]);
pub struct IVec3s<'a>(pub &'a [i32]);
pub struct IVec4s<'a>(pub &'a [i32]);

pub struct UInts<'a>(pub &'a [u32]);
pub struct UVec2s<'a>(pub &'a [u32]);
pub struct UVec3s<'a>(pub &'a [u32]);
pub struct UVec4s<'a>(pub &'a [u32]);

// --- matrix slice wrappers ---

pub struct Mat2<'a> {
    pub transpose: bool,
    pub data: &'a [f32],
}
pub struct Mat3<'a> {
    pub transpose: bool,
    pub data: &'a [f32],
}
pub struct Mat4<'a> {
    pub transpose: bool,
    pub data: &'a [f32],
}
pub struct Mat2x3<'a> {
    pub transpose: bool,
    pub data: &'a [f32],
}
pub struct Mat2x4<'a> {
    pub transpose: bool,
    pub data: &'a [f32],
}
pub struct Mat3x2<'a> {
    pub transpose: bool,
    pub data: &'a [f32],
}
pub struct Mat3x4<'a> {
    pub transpose: bool,
    pub data: &'a [f32],
}
pub struct Mat4x2<'a> {
    pub transpose: bool,
    pub data: &'a [f32],
}
pub struct Mat4x3<'a> {
    pub transpose: bool,
    pub data: &'a [f32],
}

// --- impls ---

impl UniformValue for Floats<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform1fv_with_f32_array(Some(loc), self.0);
    }
}
impl UniformValue for Vec2s<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform2fv_with_f32_array(Some(loc), self.0);
    }
}
impl UniformValue for Vec3s<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform3fv_with_f32_array(Some(loc), self.0);
    }
}
impl UniformValue for Vec4s<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform4fv_with_f32_array(Some(loc), self.0);
    }
}

impl UniformValue for Ints<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform1iv_with_i32_array(Some(loc), self.0);
    }
}
impl UniformValue for IVec2s<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform2iv_with_i32_array(Some(loc), self.0);
    }
}
impl UniformValue for IVec3s<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform3iv_with_i32_array(Some(loc), self.0);
    }
}
impl UniformValue for IVec4s<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform4iv_with_i32_array(Some(loc), self.0);
    }
}

impl UniformValue for UInts<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform1uiv_with_u32_array(Some(loc), self.0);
    }
}
impl UniformValue for UVec2s<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform2uiv_with_u32_array(Some(loc), self.0);
    }
}
impl UniformValue for UVec3s<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform3uiv_with_u32_array(Some(loc), self.0);
    }
}
impl UniformValue for UVec4s<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform4uiv_with_u32_array(Some(loc), self.0);
    }
}

impl UniformValue for Mat2<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform_matrix2fv_with_f32_array(Some(loc), self.transpose, self.data);
    }
}
impl UniformValue for Mat3<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform_matrix3fv_with_f32_array(Some(loc), self.transpose, self.data);
    }
}
impl UniformValue for Mat4<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform_matrix4fv_with_f32_array(Some(loc), self.transpose, self.data);
    }
}
impl UniformValue for Mat2x3<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform_matrix2x3fv_with_f32_array(Some(loc), self.transpose, self.data);
    }
}
impl UniformValue for Mat2x4<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform_matrix2x4fv_with_f32_array(Some(loc), self.transpose, self.data);
    }
}
impl UniformValue for Mat3x2<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform_matrix3x2fv_with_f32_array(Some(loc), self.transpose, self.data);
    }
}
impl UniformValue for Mat3x4<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform_matrix3x4fv_with_f32_array(Some(loc), self.transpose, self.data);
    }
}
impl UniformValue for Mat4x2<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform_matrix4x2fv_with_f32_array(Some(loc), self.transpose, self.data);
    }
}
impl UniformValue for Mat4x3<'_> {
    fn upload(&self, gl: &WebGl2RenderingContext, loc: &WebGlUniformLocation) {
        gl.uniform_matrix4x3fv_with_f32_array(Some(loc), self.transpose, self.data);
    }
}
