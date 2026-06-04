use wasm_bindgen::prelude::*;

use crate::program::Program;

macro_rules! with_uniform {
    ($self:ident, $name:expr, $loc:ident => $body:expr) => {
        if let Some($loc) = $self.loc($name) {
            $body;
        }
    };
}

#[wasm_bindgen]
impl Program {
    // --- float ---

    pub fn uniform_float(&self, name: &str, x: f32) {
        with_uniform!(self, name, loc => self.gl().uniform1f(Some(&loc), x));
    }

    pub fn uniform_vec2(&self, name: &str, x: f32, y: f32) {
        with_uniform!(self, name, loc => self.gl().uniform2f(Some(&loc), x, y));
    }

    pub fn uniform_vec3(&self, name: &str, x: f32, y: f32, z: f32) {
        with_uniform!(self, name, loc => self.gl().uniform3f(Some(&loc), x, y, z));
    }

    pub fn uniform_vec4(&self, name: &str, x: f32, y: f32, z: f32, w: f32) {
        with_uniform!(self, name, loc => self.gl().uniform4f(Some(&loc), x, y, z, w));
    }

    // --- int ---

    pub fn uniform_int(&self, name: &str, x: i32) {
        with_uniform!(self, name, loc => self.gl().uniform1i(Some(&loc), x));
    }

    pub fn uniform_ivec2(&self, name: &str, x: i32, y: i32) {
        with_uniform!(self, name, loc => self.gl().uniform2i(Some(&loc), x, y));
    }

    pub fn uniform_ivec3(&self, name: &str, x: i32, y: i32, z: i32) {
        with_uniform!(self, name, loc => self.gl().uniform3i(Some(&loc), x, y, z));
    }

    pub fn uniform_ivec4(&self, name: &str, x: i32, y: i32, z: i32, w: i32) {
        with_uniform!(self, name, loc => self.gl().uniform4i(Some(&loc), x, y, z, w));
    }

    // --- uint ---

    pub fn uniform_uint(&self, name: &str, x: u32) {
        with_uniform!(self, name, loc => self.gl().uniform1ui(Some(&loc), x));
    }

    pub fn uniform_uvec2(&self, name: &str, x: u32, y: u32) {
        with_uniform!(self, name, loc => self.gl().uniform2ui(Some(&loc), x, y));
    }

    pub fn uniform_uvec3(&self, name: &str, x: u32, y: u32, z: u32) {
        with_uniform!(self, name, loc => self.gl().uniform3ui(Some(&loc), x, y, z));
    }

    pub fn uniform_uvec4(&self, name: &str, x: u32, y: u32, z: u32, w: u32) {
        with_uniform!(self, name, loc => self.gl().uniform4ui(Some(&loc), x, y, z, w));
    }

    // --- matrices ---

    pub fn uniform_mat2(&self, name: &str, transpose: bool, data: &[f32]) {
        with_uniform!(self, name, loc => self.gl().uniform_matrix2fv_with_f32_array(Some(&loc), transpose, data));
    }

    pub fn uniform_mat3(&self, name: &str, transpose: bool, data: &[f32]) {
        with_uniform!(self, name, loc => self.gl().uniform_matrix3fv_with_f32_array(Some(&loc), transpose, data));
    }

    pub fn uniform_mat4(&self, name: &str, transpose: bool, data: &[f32]) {
        with_uniform!(self, name, loc => self.gl().uniform_matrix4fv_with_f32_array(Some(&loc), transpose, data));
    }

    pub fn uniform_mat2x3(&self, name: &str, transpose: bool, data: &[f32]) {
        with_uniform!(self, name, loc => self.gl().uniform_matrix2x3fv_with_f32_array(Some(&loc), transpose, data));
    }

    pub fn uniform_mat2x4(&self, name: &str, transpose: bool, data: &[f32]) {
        with_uniform!(self, name, loc => self.gl().uniform_matrix2x4fv_with_f32_array(Some(&loc), transpose, data));
    }

    pub fn uniform_mat3x2(&self, name: &str, transpose: bool, data: &[f32]) {
        with_uniform!(self, name, loc => self.gl().uniform_matrix3x2fv_with_f32_array(Some(&loc), transpose, data));
    }

    pub fn uniform_mat3x4(&self, name: &str, transpose: bool, data: &[f32]) {
        with_uniform!(self, name, loc => self.gl().uniform_matrix3x4fv_with_f32_array(Some(&loc), transpose, data));
    }

    pub fn uniform_mat4x2(&self, name: &str, transpose: bool, data: &[f32]) {
        with_uniform!(self, name, loc => self.gl().uniform_matrix4x2fv_with_f32_array(Some(&loc), transpose, data));
    }

    pub fn uniform_mat4x3(&self, name: &str, transpose: bool, data: &[f32]) {
        with_uniform!(self, name, loc => self.gl().uniform_matrix4x3fv_with_f32_array(Some(&loc), transpose, data));
    }
}
