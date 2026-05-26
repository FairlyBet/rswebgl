use crate::program::Program;

pub trait UniformValue {
    fn upload(&self, program: &mut Program, name: &str);
}

// --- scalars ---

impl UniformValue for f32 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_float(name, *self);
    }
}

impl UniformValue for i32 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_int(name, *self);
    }
}

impl UniformValue for u32 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_uint(name, *self);
    }
}

impl UniformValue for bool {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_int(name, *self as i32);
    }
}

// --- float vectors ---

impl UniformValue for [f32; 2] {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_vec2(name, self[0], self[1]);
    }
}

impl UniformValue for [f32; 3] {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_vec3(name, self[0], self[1], self[2]);
    }
}

impl UniformValue for [f32; 4] {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_vec4(name, self[0], self[1], self[2], self[3]);
    }
}

// --- int vectors ---

impl UniformValue for [i32; 2] {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_ivec2(name, self[0], self[1]);
    }
}

impl UniformValue for [i32; 3] {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_ivec3(name, self[0], self[1], self[2]);
    }
}

impl UniformValue for [i32; 4] {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_ivec4(name, self[0], self[1], self[2], self[3]);
    }
}

// --- uint vectors ---

impl UniformValue for [u32; 2] {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_uvec2(name, self[0], self[1]);
    }
}

impl UniformValue for [u32; 3] {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_uvec3(name, self[0], self[1], self[2]);
    }
}

impl UniformValue for [u32; 4] {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_uvec4(name, self[0], self[1], self[2], self[3]);
    }
}

// --- matrix newtypes ---

#[derive(Debug, Clone)] pub struct Mat2(pub [f32; 4]);
#[derive(Debug, Clone)] pub struct Mat3(pub [f32; 9]);
#[derive(Debug, Clone)] pub struct Mat4(pub [f32; 16]);
#[derive(Debug, Clone)] pub struct Mat2x3(pub [f32; 6]);
#[derive(Debug, Clone)] pub struct Mat2x4(pub [f32; 8]);
#[derive(Debug, Clone)] pub struct Mat3x2(pub [f32; 6]);
#[derive(Debug, Clone)] pub struct Mat3x4(pub [f32; 12]);
#[derive(Debug, Clone)] pub struct Mat4x2(pub [f32; 8]);
#[derive(Debug, Clone)] pub struct Mat4x3(pub [f32; 12]);

impl UniformValue for Mat2 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_mat2(name, false, &self.0);
    }
}

impl UniformValue for Mat3 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_mat3(name, false, &self.0);
    }
}

impl UniformValue for Mat4 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_mat4(name, false, &self.0);
    }
}

impl UniformValue for Mat2x3 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_mat2x3(name, false, &self.0);
    }
}

impl UniformValue for Mat2x4 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_mat2x4(name, false, &self.0);
    }
}

impl UniformValue for Mat3x2 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_mat3x2(name, false, &self.0);
    }
}

impl UniformValue for Mat3x4 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_mat3x4(name, false, &self.0);
    }
}

impl UniformValue for Mat4x2 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_mat4x2(name, false, &self.0);
    }
}

impl UniformValue for Mat4x3 {
    fn upload(&self, program: &mut Program, name: &str) {
        program.uniform_mat4x3(name, false, &self.0);
    }
}
