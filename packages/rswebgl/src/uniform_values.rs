use std::cell::RefCell;
use std::rc::Rc;

use smallvec::{SmallVec, smallvec};
use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::console;
use crate::limits;
use crate::program::Program;
use crate::texture::Texture;
use crate::uniform_buffer::UniformBuffer;
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
    // Texture unit / binding point are the program's business (assigned per
    // program by name, see `Program::sampler_unit` / `block_point`).
    Sampler {
        texture: Texture,
    },
    // `range` is None for the whole buffer (bindBufferBase) or
    // Some((offset, size)) for a sub-range (bindBufferRange).
    Block {
        ubo: UniformBuffer,
        range: Option<(i32, i32)>,
    },
}

// Name-sorted, so lookups are a binary search.
type Entries = Vec<(Box<str>, Uniform)>;

// `entries` lives behind a shared `Rc<RefCell>`, so cloning a `UniformValues`
// yields another handle to the *same* data — consistent with the rest of the
// library (Texture/Buffer/VertexArray/Program all share on clone). This is what
// lets a `Draw` hold a clone that stays live: mutating the original handle (e.g.
// a per-frame `set_mat4`) is seen by the already-built pass it was added to.
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct UniformValues {
    entries: Rc<RefCell<Entries>>,
}

impl UniformValues {
    fn put(&self, name: &str, v: Uniform) {
        let mut e = self.entries.borrow_mut();
        match e.binary_search_by(|(k, _)| k.as_ref().cmp(name)) {
            Ok(idx) => e[idx].1 = v,
            Err(idx) => e.insert(idx, (name.into(), v)),
        }
    }

    // Returns a clone: entries sit behind a RefCell, so we can't hand out a borrow
    // that outlives it. Getters aren't a hot path, so the clone is fine.
    fn get(&self, name: &str) -> Option<Uniform> {
        let e = self.entries.borrow();
        match e.binary_search_by(|(k, _)| k.as_ref().cmp(name)) {
            Ok(idx) => Some(e[idx].1.clone()),
            Err(_) => None,
        }
    }

    pub(crate) fn contains(&self, name: &str) -> bool {
        self.entries
            .borrow()
            .binary_search_by(|(k, _)| k.as_ref().cmp(name))
            .is_ok()
    }

    fn check_stride(name: &str, len: usize, stride: usize) -> bool {
        if len == 0 || !len.is_multiple_of(stride) {
            console::warn_once(&format!(
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

fn apply_value(program: &Program, name: &str, value: &Uniform) {
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
        Uniform::Sampler { .. } | Uniform::Block { .. } => {
            // Samplers and blocks are handled in upload/sync_from (they bind GL
            // objects / share activeTexture tracking rather than uploading a value).
        }
    }
}

// Binds a sampler's texture / a block's buffer to the slot the program assigned
// that name. Returns false for plain values (nothing to bind).
fn apply_binding(
    program: &Program,
    name: &str,
    value: &Uniform,
    bindings: &mut GlBindings,
) -> bool {
    match value {
        Uniform::Sampler { texture } => {
            if let Some(unit) = program.sampler_unit(name) {
                bindings.bind_texture(program.gl(), unit, texture);
            }
            true
        }
        Uniform::Block { ubo, range } => {
            // The UBO's bytes can be dirty while the entry is unchanged, so
            // flush unconditionally.
            ubo.flush();
            if let Some(point) = program.block_point(name, ubo.byte_size()) {
                bindings.bind_block(program.gl(), point, ubo, *range);
            }
            true
        }
        _ => false,
    }
}

// What's actually bound to each texture unit / uniform-buffer binding point, for
// the lifetime of one render call. These are *global* GL state, not program
// state: different programs (and draws) put different textures on the same unit,
// so this is keyed by unit/point. It survives program switches, so an unchanged
// binding isn't re-issued after `useProgram`.
#[derive(Default)]
pub(crate) struct GlBindings {
    active_unit: Option<u32>,
    units: Vec<Option<Texture>>,
    blocks: Vec<Option<BoundBlock>>,
}

// A buffer bound to a uniform-buffer point, with its sub-range (None = whole).
type BoundBlock = (UniformBuffer, Option<(i32, i32)>);

impl GlBindings {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn bind_texture(&mut self, gl: &WebGl2RenderingContext, unit: u32, texture: &Texture) {
        let idx = unit as usize;
        if self.units.get(idx).and_then(|t| t.as_ref()) == Some(texture) {
            return;
        }
        if self.active_unit != Some(unit) {
            gl.active_texture(WebGl2RenderingContext::TEXTURE0 + unit);
            self.active_unit = Some(unit);
        }
        gl.bind_texture(texture.target_gl(), Some(texture.raw_gl()));
        if idx >= self.units.len() {
            self.units.resize_with(idx + 1, || None);
        }
        self.units[idx] = Some(texture.clone());
    }

    fn bind_block(
        &mut self,
        gl: &WebGl2RenderingContext,
        point: u32,
        ubo: &UniformBuffer,
        range: Option<(i32, i32)>,
    ) {
        let idx = point as usize;
        if let Some(Some((cur, cur_range))) = self.blocks.get(idx)
            && cur == ubo
            && *cur_range == range
        {
            return;
        }
        bind_block_point(gl, point, ubo, range);
        if idx >= self.blocks.len() {
            self.blocks.resize_with(idx + 1, || None);
        }
        self.blocks[idx] = Some((ubo.clone(), range));
    }
}

fn bind_block_point(
    gl: &WebGl2RenderingContext,
    point: u32,
    ubo: &UniformBuffer,
    range: Option<(i32, i32)>,
) {
    match range {
        Some((offset, size)) => gl.bind_buffer_range_with_i32_and_i32(
            WebGl2RenderingContext::UNIFORM_BUFFER,
            point,
            Some(ubo.buffer_raw()),
            offset,
            size,
        ),
        None => gl.bind_buffer_base(
            WebGl2RenderingContext::UNIFORM_BUFFER,
            point,
            Some(ubo.buffer_raw()),
        ),
    }
}

#[wasm_bindgen]
impl UniformValues {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            entries: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn clear(&self) {
        self.entries.borrow_mut().clear();
    }

    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    pub fn has(&self, name: &str) -> bool {
        self.contains(name)
    }

    pub fn remove(&self, name: &str) -> bool {
        let mut e = self.entries.borrow_mut();
        match e.binary_search_by(|(k, _)| k.as_ref().cmp(name)) {
            Ok(idx) => {
                e.remove(idx);
                true
            }
            Err(_) => false,
        }
    }

    /// Uploads every uniform to `program` and binds its textures and blocks,
    /// without diffing. The program must be in use (`useProgram`).
    pub fn upload(&self, program: &Program) {
        let mut bindings = GlBindings::new();
        for (name, value) in self.entries.borrow().iter() {
            if !apply_binding(program, name, value, &mut bindings) {
                apply_value(program, name, value);
            }
        }
    }

    /// Diffs `incoming` against `self` — the renderer's per-program cache of what's
    /// already in the program — uploading only plain values that are new or
    /// changed and recording them here in place. No whole-map clone: only
    /// individual changed values are cloned. The caller must `clear` this cache on
    /// program switch, since uniform locations belong to the active program.
    ///
    /// Samplers and blocks aren't cached here: their slot is fixed per program,
    /// and what's bound to the slot (global state) diffs against `bindings`.
    pub(crate) fn sync_from(
        &self,
        incoming: &UniformValues,
        program: &Program,
        bindings: &mut GlBindings,
    ) {
        let incoming = incoming.entries.borrow();
        let mut applied = self.entries.borrow_mut();
        for (name, value) in incoming.iter() {
            if apply_binding(program, name, value, bindings) {
                continue;
            }
            match applied.binary_search_by(|(k, _)| k.as_ref().cmp(name.as_ref())) {
                Ok(idx) if &applied[idx].1 == value => {}
                Ok(idx) => {
                    apply_value(program, name, value);
                    applied[idx].1 = value.clone();
                }
                Err(idx) => {
                    apply_value(program, name, value);
                    applied.insert(idx, (name.clone(), value.clone()));
                }
            }
        }
    }

    // --- scalar setters (single) ---

    pub fn set_float(&self, name: &str, x: f32) {
        self.put(name, Uniform::Float(smallvec![x]));
    }
    pub fn set_int(&self, name: &str, x: i32) {
        self.put(name, Uniform::Int(smallvec![x]));
    }
    pub fn set_uint(&self, name: &str, x: u32) {
        self.put(name, Uniform::UInt(smallvec![x]));
    }

    // --- vector setters (single) ---

    pub fn set_vec2(&self, name: &str, x: f32, y: f32) {
        self.put(name, Uniform::Vec2(smallvec![x, y]));
    }
    pub fn set_vec3(&self, name: &str, x: f32, y: f32, z: f32) {
        self.put(name, Uniform::Vec3(smallvec![x, y, z]));
    }
    pub fn set_vec4(&self, name: &str, x: f32, y: f32, z: f32, w: f32) {
        self.put(name, Uniform::Vec4(smallvec![x, y, z, w]));
    }
    pub fn set_ivec2(&self, name: &str, x: i32, y: i32) {
        self.put(name, Uniform::IVec2(smallvec![x, y]));
    }
    pub fn set_ivec3(&self, name: &str, x: i32, y: i32, z: i32) {
        self.put(name, Uniform::IVec3(smallvec![x, y, z]));
    }
    pub fn set_ivec4(&self, name: &str, x: i32, y: i32, z: i32, w: i32) {
        self.put(name, Uniform::IVec4(smallvec![x, y, z, w]));
    }
    pub fn set_uvec2(&self, name: &str, x: u32, y: u32) {
        self.put(name, Uniform::UVec2(smallvec![x, y]));
    }
    pub fn set_uvec3(&self, name: &str, x: u32, y: u32, z: u32) {
        self.put(name, Uniform::UVec3(smallvec![x, y, z]));
    }
    pub fn set_uvec4(&self, name: &str, x: u32, y: u32, z: u32, w: u32) {
        self.put(name, Uniform::UVec4(smallvec![x, y, z, w]));
    }

    // --- bool ergonomics (uploaded as int) ---

    pub fn set_bool(&self, name: &str, x: bool) {
        self.put(name, Uniform::Int(smallvec![x as i32]));
    }
    pub fn set_bvec2(&self, name: &str, x: bool, y: bool) {
        self.put(name, Uniform::IVec2(smallvec![x as i32, y as i32]));
    }
    pub fn set_bvec3(&self, name: &str, x: bool, y: bool, z: bool) {
        self.put(
            name,
            Uniform::IVec3(smallvec![x as i32, y as i32, z as i32]),
        );
    }
    pub fn set_bvec4(&self, name: &str, x: bool, y: bool, z: bool, w: bool) {
        self.put(
            name,
            Uniform::IVec4(smallvec![x as i32, y as i32, z as i32, w as i32]),
        );
    }

    pub fn get_bool(&self, name: &str) -> Option<bool> {
        self.get_int(name).map(|x| x != 0)
    }

    // --- sampler ---

    /// Bind `texture` to the sampler uniform `name`. The texture unit is assigned
    /// by the program (per sampler name), so samplers set on a Batch and on its
    /// draws never collide.
    pub fn set_sampler(&self, name: &str, texture: &Texture) {
        self.put(
            name,
            Uniform::Sampler {
                texture: texture.clone(),
            },
        );
    }

    pub fn get_sampler_texture(&self, name: &str) -> Option<Texture> {
        match self.get(name) {
            Some(Uniform::Sampler { texture }) => Some(texture),
            _ => None,
        }
    }

    // --- uniform block (UBO) ---

    /// Bind a uniform block (whole buffer) by its GLSL block name. The binding
    /// point is assigned by the program (per block name), like sampler units.
    pub fn set_uniform_block(&self, name: &str, ubo: &UniformBuffer) {
        self.put(
            name,
            Uniform::Block {
                ubo: ubo.clone(),
                range: None,
            },
        );
    }

    /// Bind a sub-range of a uniform block (`bindBufferRange`). `offset` must be a
    /// multiple of `UNIFORM_BUFFER_OFFSET_ALIGNMENT` (else GL rejects the bind).
    pub fn set_uniform_block_range(&self, name: &str, ubo: &UniformBuffer, offset: i32, size: i32) {
        let align = limits::uniform_buffer_offset_alignment() as i32;
        if align > 0 && offset % align != 0 {
            console::warn_once(&format!(
                "[rswebgl] uniform block \"{name}\": offset {offset} is not a multiple of \
                 UNIFORM_BUFFER_OFFSET_ALIGNMENT ({align})"
            ));
        }
        self.put(
            name,
            Uniform::Block {
                ubo: ubo.clone(),
                range: Some((offset, size)),
            },
        );
    }

    pub fn get_uniform_block(&self, name: &str) -> Option<UniformBuffer> {
        match self.get(name) {
            Some(Uniform::Block { ubo, .. }) => Some(ubo),
            _ => None,
        }
    }

    // --- array setters (length must be non-zero multiple of stride) ---

    pub fn set_float_array(&self, name: &str, data: &[f32]) {
        if Self::check_stride(name, data.len(), 1) {
            self.put(name, Uniform::Float(SmallVec::from_slice(data)));
        }
    }
    pub fn set_int_array(&self, name: &str, data: &[i32]) {
        if Self::check_stride(name, data.len(), 1) {
            self.put(name, Uniform::Int(SmallVec::from_slice(data)));
        }
    }
    pub fn set_uint_array(&self, name: &str, data: &[u32]) {
        if Self::check_stride(name, data.len(), 1) {
            self.put(name, Uniform::UInt(SmallVec::from_slice(data)));
        }
    }
    pub fn set_vec2_array(&self, name: &str, data: &[f32]) {
        if Self::check_stride(name, data.len(), 2) {
            self.put(name, Uniform::Vec2(SmallVec::from_slice(data)));
        }
    }
    pub fn set_vec3_array(&self, name: &str, data: &[f32]) {
        if Self::check_stride(name, data.len(), 3) {
            self.put(name, Uniform::Vec3(SmallVec::from_slice(data)));
        }
    }
    pub fn set_vec4_array(&self, name: &str, data: &[f32]) {
        if Self::check_stride(name, data.len(), 4) {
            self.put(name, Uniform::Vec4(SmallVec::from_slice(data)));
        }
    }
    pub fn set_ivec2_array(&self, name: &str, data: &[i32]) {
        if Self::check_stride(name, data.len(), 2) {
            self.put(name, Uniform::IVec2(SmallVec::from_slice(data)));
        }
    }
    pub fn set_ivec3_array(&self, name: &str, data: &[i32]) {
        if Self::check_stride(name, data.len(), 3) {
            self.put(name, Uniform::IVec3(SmallVec::from_slice(data)));
        }
    }
    pub fn set_ivec4_array(&self, name: &str, data: &[i32]) {
        if Self::check_stride(name, data.len(), 4) {
            self.put(name, Uniform::IVec4(SmallVec::from_slice(data)));
        }
    }
    pub fn set_uvec2_array(&self, name: &str, data: &[u32]) {
        if Self::check_stride(name, data.len(), 2) {
            self.put(name, Uniform::UVec2(SmallVec::from_slice(data)));
        }
    }
    pub fn set_uvec3_array(&self, name: &str, data: &[u32]) {
        if Self::check_stride(name, data.len(), 3) {
            self.put(name, Uniform::UVec3(SmallVec::from_slice(data)));
        }
    }
    pub fn set_uvec4_array(&self, name: &str, data: &[u32]) {
        if Self::check_stride(name, data.len(), 4) {
            self.put(name, Uniform::UVec4(SmallVec::from_slice(data)));
        }
    }

    // --- matrix setters (length must be non-zero multiple of NxM) ---

    pub fn set_mat2(&self, name: &str, transpose: bool, data: &[f32]) {
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
    pub fn set_mat3(&self, name: &str, transpose: bool, data: &[f32]) {
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
    pub fn set_mat4(&self, name: &str, transpose: bool, data: &[f32]) {
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
    pub fn set_mat2x3(&self, name: &str, transpose: bool, data: &[f32]) {
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
    pub fn set_mat2x4(&self, name: &str, transpose: bool, data: &[f32]) {
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
    pub fn set_mat3x2(&self, name: &str, transpose: bool, data: &[f32]) {
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
    pub fn set_mat3x4(&self, name: &str, transpose: bool, data: &[f32]) {
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
    pub fn set_mat4x2(&self, name: &str, transpose: bool, data: &[f32]) {
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
    pub fn set_mat4x3(&self, name: &str, transpose: bool, data: &[f32]) {
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
        match self.get(name) {
            Some(Uniform::Float(v)) if v.len() == 1 => Some(v[0]),
            _ => None,
        }
    }
    pub fn get_int(&self, name: &str) -> Option<i32> {
        match self.get(name) {
            Some(Uniform::Int(v)) if v.len() == 1 => Some(v[0]),
            _ => None,
        }
    }
    pub fn get_uint(&self, name: &str) -> Option<u32> {
        match self.get(name) {
            Some(Uniform::UInt(v)) if v.len() == 1 => Some(v[0]),
            _ => None,
        }
    }

    // --- vector / array getters (return full stored data) ---

    pub fn get_float_array(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Float(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_int_array(&self, name: &str) -> Option<Vec<i32>> {
        match self.get(name) {
            Some(Uniform::Int(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_uint_array(&self, name: &str) -> Option<Vec<u32>> {
        match self.get(name) {
            Some(Uniform::UInt(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_vec2(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Vec2(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_vec3(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Vec3(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_vec4(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Vec4(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_ivec2(&self, name: &str) -> Option<Vec<i32>> {
        match self.get(name) {
            Some(Uniform::IVec2(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_ivec3(&self, name: &str) -> Option<Vec<i32>> {
        match self.get(name) {
            Some(Uniform::IVec3(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_ivec4(&self, name: &str) -> Option<Vec<i32>> {
        match self.get(name) {
            Some(Uniform::IVec4(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_uvec2(&self, name: &str) -> Option<Vec<u32>> {
        match self.get(name) {
            Some(Uniform::UVec2(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_uvec3(&self, name: &str) -> Option<Vec<u32>> {
        match self.get(name) {
            Some(Uniform::UVec3(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    pub fn get_uvec4(&self, name: &str) -> Option<Vec<u32>> {
        match self.get(name) {
            Some(Uniform::UVec4(v)) => Some(v.to_vec()),
            _ => None,
        }
    }

    // --- matrix getters (return full stored data, single or array) ---

    pub fn get_mat2(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Mat2 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat3(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Mat3 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat4(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Mat4 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat2x3(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Mat2x3 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat2x4(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Mat2x4 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat3x2(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Mat3x2 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat3x4(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Mat3x4 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat4x2(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Mat4x2 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }
    pub fn get_mat4x3(&self, name: &str) -> Option<Vec<f32>> {
        match self.get(name) {
            Some(Uniform::Mat4x3 { data, .. }) => Some(data.to_vec()),
            _ => None,
        }
    }

    pub fn get_transpose(&self, name: &str) -> Option<bool> {
        match self.get(name) {
            Some(Uniform::Mat2 { transpose, .. })
            | Some(Uniform::Mat3 { transpose, .. })
            | Some(Uniform::Mat4 { transpose, .. })
            | Some(Uniform::Mat2x3 { transpose, .. })
            | Some(Uniform::Mat2x4 { transpose, .. })
            | Some(Uniform::Mat3x2 { transpose, .. })
            | Some(Uniform::Mat3x4 { transpose, .. })
            | Some(Uniform::Mat4x2 { transpose, .. })
            | Some(Uniform::Mat4x3 { transpose, .. }) => Some(transpose),
            _ => None,
        }
    }
}
