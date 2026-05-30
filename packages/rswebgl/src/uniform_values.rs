use smallvec::{SmallVec, smallvec};
use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::console;
use crate::limits;
use crate::program::Program;
use crate::texture::Texture;
use crate::uniform_value::{self as uv, UniformValue};

#[derive(Debug, Clone, PartialEq)]
pub enum Uniform {
    Float(SmallVec<[f32; 1]>),
    Vec2(SmallVec<[f32; 2]>),
    Vec3(SmallVec<[f32; 3]>),
    Vec4(SmallVec<[f32; 4]>),
    Int(SmallVec<[i32; 1]>),
    IVec2(SmallVec<[i32; 2]>),
    IVec3(SmallVec<[i32; 3]>),
    IVec4(SmallVec<[i32; 4]>),
    UInt(SmallVec<[u32; 1]>),
    UVec2(SmallVec<[u32; 2]>),
    UVec3(SmallVec<[u32; 3]>),
    UVec4(SmallVec<[u32; 4]>),
    Mat2 {
        transpose: bool,
        data: SmallVec<[f32; 4]>,
    },
    Mat3 {
        transpose: bool,
        data: SmallVec<[f32; 9]>,
    },
    Mat4 {
        transpose: bool,
        data: SmallVec<[f32; 16]>,
    },
    Mat2x3 {
        transpose: bool,
        data: SmallVec<[f32; 6]>,
    },
    Mat2x4 {
        transpose: bool,
        data: SmallVec<[f32; 8]>,
    },
    Mat3x2 {
        transpose: bool,
        data: SmallVec<[f32; 6]>,
    },
    Mat3x4 {
        transpose: bool,
        data: SmallVec<[f32; 12]>,
    },
    Mat4x2 {
        transpose: bool,
        data: SmallVec<[f32; 8]>,
    },
    Mat4x3 {
        transpose: bool,
        data: SmallVec<[f32; 12]>,
    },
    Sampler {
        unit: u32,
        texture: Texture,
    },
}

#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct UniformValues {
    entries: Vec<(Box<str>, Uniform)>,
}

impl UniformValues {
    fn put(&mut self, name: &str, v: Uniform) {
        match self.entries.binary_search_by(|(k, _)| k.as_ref().cmp(name)) {
            Ok(idx) => self.entries[idx].1 = v,
            Err(idx) => self.entries.insert(idx, (name.into(), v)),
        }
    }

    fn find(&self, name: &str) -> Option<&Uniform> {
        match self.entries.binary_search_by(|(k, _)| k.as_ref().cmp(name)) {
            Ok(idx) => Some(&self.entries[idx].1),
            Err(_) => None,
        }
    }

    fn assign_unit(&self, name: &str) -> u32 {
        let max = limits::max_combined_texture_units() as usize;
        let mut used: SmallVec<[bool; 32]> = SmallVec::from_elem(false, max);
        for (k, v) in &self.entries {
            if let Uniform::Sampler { unit, .. } = v {
                if k.as_ref() == name {
                    return *unit;
                }
                let idx = *unit as usize;
                if idx < max {
                    used[idx] = true;
                }
            }
        }
        match used.iter().position(|b| !b) {
            Some(i) => i as u32,
            None => {
                console::warn(&format!(
                    "[rswebgl] sampler \"{name}\": all {max} texture units occupied"
                ));
                0
            }
        }
    }

    fn check_stride(name: &str, len: usize, stride: usize) -> bool {
        if len == 0 || !len.is_multiple_of(stride) {
            console::warn(&format!(
                "[rswebgl] uniform \"{name}\": length must be a non-zero multiple of {stride}, got {len}"
            ));
            false
        } else {
            true
        }
    }
}

impl Default for UniformValues {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_value(program: &mut Program, name: &str, value: &Uniform) {
    let Some(loc) = program.loc(name) else { return };
    let gl = program.gl();
    let loc = &loc;
    match value {
        Uniform::Float(v) => uv::Floats(v).upload(gl, loc),
        Uniform::Vec2(v) => uv::Vec2s(v).upload(gl, loc),
        Uniform::Vec3(v) => uv::Vec3s(v).upload(gl, loc),
        Uniform::Vec4(v) => uv::Vec4s(v).upload(gl, loc),
        Uniform::Int(v) => uv::Ints(v).upload(gl, loc),
        Uniform::IVec2(v) => uv::IVec2s(v).upload(gl, loc),
        Uniform::IVec3(v) => uv::IVec3s(v).upload(gl, loc),
        Uniform::IVec4(v) => uv::IVec4s(v).upload(gl, loc),
        Uniform::UInt(v) => uv::UInts(v).upload(gl, loc),
        Uniform::UVec2(v) => uv::UVec2s(v).upload(gl, loc),
        Uniform::UVec3(v) => uv::UVec3s(v).upload(gl, loc),
        Uniform::UVec4(v) => uv::UVec4s(v).upload(gl, loc),
        Uniform::Mat2 { transpose, data } => uv::Mat2 {
            transpose: *transpose,
            data,
        }
        .upload(gl, loc),
        Uniform::Mat3 { transpose, data } => uv::Mat3 {
            transpose: *transpose,
            data,
        }
        .upload(gl, loc),
        Uniform::Mat4 { transpose, data } => uv::Mat4 {
            transpose: *transpose,
            data,
        }
        .upload(gl, loc),
        Uniform::Mat2x3 { transpose, data } => uv::Mat2x3 {
            transpose: *transpose,
            data,
        }
        .upload(gl, loc),
        Uniform::Mat2x4 { transpose, data } => uv::Mat2x4 {
            transpose: *transpose,
            data,
        }
        .upload(gl, loc),
        Uniform::Mat3x2 { transpose, data } => uv::Mat3x2 {
            transpose: *transpose,
            data,
        }
        .upload(gl, loc),
        Uniform::Mat3x4 { transpose, data } => uv::Mat3x4 {
            transpose: *transpose,
            data,
        }
        .upload(gl, loc),
        Uniform::Mat4x2 { transpose, data } => uv::Mat4x2 {
            transpose: *transpose,
            data,
        }
        .upload(gl, loc),
        Uniform::Mat4x3 { transpose, data } => uv::Mat4x3 {
            transpose: *transpose,
            data,
        }
        .upload(gl, loc),
        Uniform::Sampler { .. } => {
            // Samplers are handled in upload/upload_diff to share activeTexture tracking
        }
    }
}

fn apply_sampler(
    program: &mut Program,
    name: &str,
    unit: u32,
    texture: &Texture,
    write_uniform: bool,
    current_active: &mut Option<u32>,
) {
    let Some(loc) = program.loc(name) else { return };
    let gl = program.gl();
    if *current_active != Some(unit) {
        gl.active_texture(WebGl2RenderingContext::TEXTURE0 + unit);
        *current_active = Some(unit);
    }
    gl.bind_texture(texture.target_gl(), Some(texture.raw_gl()));
    if write_uniform {
        gl.uniform1i(Some(&loc), unit as i32);
    }
}

#[wasm_bindgen]
impl UniformValues {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    pub fn remove(&mut self, name: &str) -> bool {
        match self.entries.binary_search_by(|(k, _)| k.as_ref().cmp(name)) {
            Ok(idx) => {
                self.entries.remove(idx);
                true
            }
            Err(_) => false,
        }
    }

    pub fn upload(&self, program: &mut Program) {
        let mut current_active: Option<u32> = None;
        for (name, value) in &self.entries {
            match value {
                Uniform::Sampler { unit, texture } => {
                    apply_sampler(program, name, *unit, texture, true, &mut current_active);
                }
                _ => apply_value(program, name, value),
            }
        }
    }

    pub fn upload_diff(&self, prev: &UniformValues, program: &mut Program) {
        let mut current_active: Option<u32> = None;
        for (name, value) in &self.entries {
            let prev_v = prev.find(name);
            match value {
                Uniform::Sampler { unit, texture } => {
                    let (tex_changed, unit_changed) = match prev_v {
                        Some(Uniform::Sampler {
                            unit: pu,
                            texture: pt,
                        }) => (pt != texture, pu != unit),
                        _ => (true, true),
                    };
                    if !tex_changed && !unit_changed {
                        continue;
                    }
                    apply_sampler(
                        program,
                        name,
                        *unit,
                        texture,
                        unit_changed,
                        &mut current_active,
                    );
                }
                _ => {
                    let changed = match prev_v {
                        Some(p) => p != value,
                        None => true,
                    };
                    if changed {
                        apply_value(program, name, value);
                    }
                }
            }
        }
    }

    // --- scalar setters (single) ---

    pub fn set_float(&mut self, name: &str, x: f32) {
        self.put(name, Uniform::Float(smallvec![x]));
    }
    pub fn set_int(&mut self, name: &str, x: i32) {
        self.put(name, Uniform::Int(smallvec![x]));
    }
    pub fn set_uint(&mut self, name: &str, x: u32) {
        self.put(name, Uniform::UInt(smallvec![x]));
    }

    // --- vector setters (single) ---

    pub fn set_vec2(&mut self, name: &str, x: f32, y: f32) {
        self.put(name, Uniform::Vec2(smallvec![x, y]));
    }
    pub fn set_vec3(&mut self, name: &str, x: f32, y: f32, z: f32) {
        self.put(name, Uniform::Vec3(smallvec![x, y, z]));
    }
    pub fn set_vec4(&mut self, name: &str, x: f32, y: f32, z: f32, w: f32) {
        self.put(name, Uniform::Vec4(smallvec![x, y, z, w]));
    }
    pub fn set_ivec2(&mut self, name: &str, x: i32, y: i32) {
        self.put(name, Uniform::IVec2(smallvec![x, y]));
    }
    pub fn set_ivec3(&mut self, name: &str, x: i32, y: i32, z: i32) {
        self.put(name, Uniform::IVec3(smallvec![x, y, z]));
    }
    pub fn set_ivec4(&mut self, name: &str, x: i32, y: i32, z: i32, w: i32) {
        self.put(name, Uniform::IVec4(smallvec![x, y, z, w]));
    }
    pub fn set_uvec2(&mut self, name: &str, x: u32, y: u32) {
        self.put(name, Uniform::UVec2(smallvec![x, y]));
    }
    pub fn set_uvec3(&mut self, name: &str, x: u32, y: u32, z: u32) {
        self.put(name, Uniform::UVec3(smallvec![x, y, z]));
    }
    pub fn set_uvec4(&mut self, name: &str, x: u32, y: u32, z: u32, w: u32) {
        self.put(name, Uniform::UVec4(smallvec![x, y, z, w]));
    }

    // --- bool ergonomics (uploaded as int) ---

    pub fn set_bool(&mut self, name: &str, x: bool) {
        self.put(name, Uniform::Int(smallvec![x as i32]));
    }
    pub fn set_bvec2(&mut self, name: &str, x: bool, y: bool) {
        self.put(name, Uniform::IVec2(smallvec![x as i32, y as i32]));
    }
    pub fn set_bvec3(&mut self, name: &str, x: bool, y: bool, z: bool) {
        self.put(
            name,
            Uniform::IVec3(smallvec![x as i32, y as i32, z as i32]),
        );
    }
    pub fn set_bvec4(&mut self, name: &str, x: bool, y: bool, z: bool, w: bool) {
        self.put(
            name,
            Uniform::IVec4(smallvec![x as i32, y as i32, z as i32, w as i32]),
        );
    }

    pub fn get_bool(&self, name: &str) -> Option<bool> {
        self.get_int(name).map(|x| x != 0)
    }

    // --- sampler ---

    pub fn set_sampler(&mut self, name: &str, texture: &Texture) {
        let unit = self.assign_unit(name);
        self.put(
            name,
            Uniform::Sampler {
                unit,
                texture: texture.clone(),
            },
        );
    }

    pub fn get_sampler_unit(&self, name: &str) -> Option<u32> {
        match self.find(name) {
            Some(Uniform::Sampler { unit, .. }) => Some(*unit),
            _ => None,
        }
    }

    pub fn get_sampler_texture(&self, name: &str) -> Option<Texture> {
        match self.find(name) {
            Some(Uniform::Sampler { texture, .. }) => Some(texture.clone()),
            _ => None,
        }
    }

    // --- array setters (length must be non-zero multiple of stride) ---

    pub fn set_float_array(&mut self, name: &str, data: &[f32]) {
        if Self::check_stride(name, data.len(), 1) {
            self.put(name, Uniform::Float(SmallVec::from_slice(data)));
        }
    }
    pub fn set_int_array(&mut self, name: &str, data: &[i32]) {
        if Self::check_stride(name, data.len(), 1) {
            self.put(name, Uniform::Int(SmallVec::from_slice(data)));
        }
    }
    pub fn set_uint_array(&mut self, name: &str, data: &[u32]) {
        if Self::check_stride(name, data.len(), 1) {
            self.put(name, Uniform::UInt(SmallVec::from_slice(data)));
        }
    }
    pub fn set_vec2_array(&mut self, name: &str, data: &[f32]) {
        if Self::check_stride(name, data.len(), 2) {
            self.put(name, Uniform::Vec2(SmallVec::from_slice(data)));
        }
    }
    pub fn set_vec3_array(&mut self, name: &str, data: &[f32]) {
        if Self::check_stride(name, data.len(), 3) {
            self.put(name, Uniform::Vec3(SmallVec::from_slice(data)));
        }
    }
    pub fn set_vec4_array(&mut self, name: &str, data: &[f32]) {
        if Self::check_stride(name, data.len(), 4) {
            self.put(name, Uniform::Vec4(SmallVec::from_slice(data)));
        }
    }
    pub fn set_ivec2_array(&mut self, name: &str, data: &[i32]) {
        if Self::check_stride(name, data.len(), 2) {
            self.put(name, Uniform::IVec2(SmallVec::from_slice(data)));
        }
    }
    pub fn set_ivec3_array(&mut self, name: &str, data: &[i32]) {
        if Self::check_stride(name, data.len(), 3) {
            self.put(name, Uniform::IVec3(SmallVec::from_slice(data)));
        }
    }
    pub fn set_ivec4_array(&mut self, name: &str, data: &[i32]) {
        if Self::check_stride(name, data.len(), 4) {
            self.put(name, Uniform::IVec4(SmallVec::from_slice(data)));
        }
    }
    pub fn set_uvec2_array(&mut self, name: &str, data: &[u32]) {
        if Self::check_stride(name, data.len(), 2) {
            self.put(name, Uniform::UVec2(SmallVec::from_slice(data)));
        }
    }
    pub fn set_uvec3_array(&mut self, name: &str, data: &[u32]) {
        if Self::check_stride(name, data.len(), 3) {
            self.put(name, Uniform::UVec3(SmallVec::from_slice(data)));
        }
    }
    pub fn set_uvec4_array(&mut self, name: &str, data: &[u32]) {
        if Self::check_stride(name, data.len(), 4) {
            self.put(name, Uniform::UVec4(SmallVec::from_slice(data)));
        }
    }

    // --- matrix setters (length must be non-zero multiple of NxM) ---

    pub fn set_mat2(&mut self, name: &str, transpose: bool, data: &[f32]) {
        if Self::check_stride(name, data.len(), 4) {
            self.put(
                name,
                Uniform::Mat2 {
                    transpose,
                    data: SmallVec::from_slice(data),
                },
            );
        }
    }
    pub fn set_mat3(&mut self, name: &str, transpose: bool, data: &[f32]) {
        if Self::check_stride(name, data.len(), 9) {
            self.put(
                name,
                Uniform::Mat3 {
                    transpose,
                    data: SmallVec::from_slice(data),
                },
            );
        }
    }
    pub fn set_mat4(&mut self, name: &str, transpose: bool, data: &[f32]) {
        if Self::check_stride(name, data.len(), 16) {
            self.put(
                name,
                Uniform::Mat4 {
                    transpose,
                    data: SmallVec::from_slice(data),
                },
            );
        }
    }
    pub fn set_mat2x3(&mut self, name: &str, transpose: bool, data: &[f32]) {
        if Self::check_stride(name, data.len(), 6) {
            self.put(
                name,
                Uniform::Mat2x3 {
                    transpose,
                    data: SmallVec::from_slice(data),
                },
            );
        }
    }
    pub fn set_mat2x4(&mut self, name: &str, transpose: bool, data: &[f32]) {
        if Self::check_stride(name, data.len(), 8) {
            self.put(
                name,
                Uniform::Mat2x4 {
                    transpose,
                    data: SmallVec::from_slice(data),
                },
            );
        }
    }
    pub fn set_mat3x2(&mut self, name: &str, transpose: bool, data: &[f32]) {
        if Self::check_stride(name, data.len(), 6) {
            self.put(
                name,
                Uniform::Mat3x2 {
                    transpose,
                    data: SmallVec::from_slice(data),
                },
            );
        }
    }
    pub fn set_mat3x4(&mut self, name: &str, transpose: bool, data: &[f32]) {
        if Self::check_stride(name, data.len(), 12) {
            self.put(
                name,
                Uniform::Mat3x4 {
                    transpose,
                    data: SmallVec::from_slice(data),
                },
            );
        }
    }
    pub fn set_mat4x2(&mut self, name: &str, transpose: bool, data: &[f32]) {
        if Self::check_stride(name, data.len(), 8) {
            self.put(
                name,
                Uniform::Mat4x2 {
                    transpose,
                    data: SmallVec::from_slice(data),
                },
            );
        }
    }
    pub fn set_mat4x3(&mut self, name: &str, transpose: bool, data: &[f32]) {
        if Self::check_stride(name, data.len(), 12) {
            self.put(
                name,
                Uniform::Mat4x3 {
                    transpose,
                    data: SmallVec::from_slice(data),
                },
            );
        }
    }

    // --- scalar single getters (return Some only if stored as 1-element) ---

    pub fn get_float(&self, name: &str) -> Option<f32> {
        match self.find(name) {
            Some(Uniform::Float(v)) if v.len() == 1 => Some(v[0]),
            _ => None,
        }
    }
    pub fn get_int(&self, name: &str) -> Option<i32> {
        match self.find(name) {
            Some(Uniform::Int(v)) if v.len() == 1 => Some(v[0]),
            _ => None,
        }
    }
    pub fn get_uint(&self, name: &str) -> Option<u32> {
        match self.find(name) {
            Some(Uniform::UInt(v)) if v.len() == 1 => Some(v[0]),
            _ => None,
        }
    }

    // --- vector / array getters (return full stored data) ---

    pub fn get_float_array(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Float(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_int_array(&self, name: &str) -> Option<Vec<i32>> {
        match self.find(name) {
            Some(Uniform::Int(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_uint_array(&self, name: &str) -> Option<Vec<u32>> {
        match self.find(name) {
            Some(Uniform::UInt(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_vec2(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Vec2(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_vec3(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Vec3(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_vec4(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Vec4(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_ivec2(&self, name: &str) -> Option<Vec<i32>> {
        match self.find(name) {
            Some(Uniform::IVec2(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_ivec3(&self, name: &str) -> Option<Vec<i32>> {
        match self.find(name) {
            Some(Uniform::IVec3(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_ivec4(&self, name: &str) -> Option<Vec<i32>> {
        match self.find(name) {
            Some(Uniform::IVec4(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_uvec2(&self, name: &str) -> Option<Vec<u32>> {
        match self.find(name) {
            Some(Uniform::UVec2(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_uvec3(&self, name: &str) -> Option<Vec<u32>> {
        match self.find(name) {
            Some(Uniform::UVec3(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_uvec4(&self, name: &str) -> Option<Vec<u32>> {
        match self.find(name) {
            Some(Uniform::UVec4(v)) => Some(v.to_vec()),
            _ => None,
        }
    }

    // --- matrix getters (return full stored data, single or array) ---

    pub fn get_mat2(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Mat2 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat3(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Mat3 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat4(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Mat4 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat2x3(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Mat2x3 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat2x4(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Mat2x4 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat3x2(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Mat3x2 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat3x4(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Mat3x4 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat4x2(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Mat4x2 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat4x3(&self, name: &str) -> Option<Vec<f32>> {
        match self.find(name) {
            Some(Uniform::Mat4x3 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }

    pub fn get_transpose(&self, name: &str) -> Option<bool> {
        match self.find(name) {
            Some(Uniform::Mat2 { transpose, .. })
            | Some(Uniform::Mat3 { transpose, .. })
            | Some(Uniform::Mat4 { transpose, .. })
            | Some(Uniform::Mat2x3 { transpose, .. })
            | Some(Uniform::Mat2x4 { transpose, .. })
            | Some(Uniform::Mat3x2 { transpose, .. })
            | Some(Uniform::Mat3x4 { transpose, .. })
            | Some(Uniform::Mat4x2 { transpose, .. })
            | Some(Uniform::Mat4x3 { transpose, .. }) => Some(*transpose),
            _ => None,
        }
    }
}
