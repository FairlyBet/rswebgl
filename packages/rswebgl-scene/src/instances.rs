//! Per-frame object matrices for shaders that read their instance
//! (`object_instanced.glsl`): the rows of each affine model matrix in an
//! RGBA32F texture, and the grouping of identical meshes into instanced
//! draws.

use std::collections::HashMap;

use glam::Mat4;
use rswebgl::context::Context;
use rswebgl::error::Error;
use rswebgl::ids::{ParamGroupId, ProgramId, TextureId};
use rswebgl::param::ParamField;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};

use crate::render::MeshRenderer;

/// Texels per row of the instance texture; `INSTANCE_TEXTURE_WIDTH` in the
/// shader.
const WIDTH: i32 = 1024;
/// Floats per instance: three RGBA texels, the model matrix's rows.
const FLOATS: usize = 12;

/// How a program gets its object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgramKind {
    /// Its own uniforms (`u_model`, …) per object: the program doesn't read
    /// the instance texture.
    Uniforms,
    /// Reads its instance, but its fragment stage reads the object's
    /// uniforms too: one draw per object.
    Single,
    /// Reads its instance only: identical meshes draw as one.
    Batched,
}

/// Meshes with the same program, state, vertex array, material and command,
/// gathered over a frame.
pub(crate) struct Batch {
    /// Into the late pass (transmissive).
    pub late: bool,
    pub mesh: MeshRenderer,
    pub matrices: Vec<Mat4>,
}

type BatchKey = (bool, ProgramId, u32, u32, u32);

/// This frame's batches; their allocations are kept across frames.
#[derive(Default)]
pub(crate) struct Batches {
    list: Vec<Batch>,
    used: usize,
    // Batches by everything but the command (compared one by one).
    index: HashMap<BatchKey, Vec<usize>>,
}

impl Batches {
    pub fn add(&mut self, late: bool, mesh: &MeshRenderer, m: &Mat4) {
        let key = (
            late,
            mesh.program,
            mesh.state.to_raw(),
            mesh.vao.to_raw(),
            mesh.material.to_raw(),
        );
        let slots = self.index.entry(key).or_default();
        for &i in slots.iter() {
            if self.list[i].mesh.command == mesh.command {
                self.list[i].matrices.push(*m);
                return;
            }
        }
        let i = self.used;
        if let Some(b) = self.list.get_mut(i) {
            b.late = late;
            b.mesh = *mesh;
            b.matrices.clear();
            b.matrices.push(*m);
        } else {
            self.list.push(Batch {
                late,
                mesh: *mesh,
                matrices: vec![*m],
            });
        }
        self.used += 1;
        slots.push(i);
    }

    pub fn iter(&self) -> impl Iterator<Item = &Batch> {
        self.list[..self.used].iter()
    }

    pub fn clear(&mut self) {
        self.used = 0;
        self.index.clear();
    }
}

/// The instance texture and this frame's data for it.
pub(crate) struct Instances {
    data: Vec<f32>,
    // What the texture holds, to skip an unchanged upload.
    uploaded: Vec<f32>,
    bytes: Vec<u8>,
    texture: TextureId,
    rows: i32,
}

impl Instances {
    pub fn new(ctx: &Context) -> Result<Self, Error> {
        Ok(Self {
            data: Vec::new(),
            uploaded: Vec::new(),
            bytes: Vec::new(),
            texture: Self::make_texture(ctx, 1)?,
            rows: 1,
        })
    }

    fn make_texture(ctx: &Context, rows: i32) -> Result<TextureId, Error> {
        let t = ctx.create_texture(
            TextureTarget::Texture2D,
            TextureMinFilter::Nearest,
            TextureMagFilter::Nearest,
        )?;
        ctx.texture_storage_2d(t, 1, &TextureFormat::rgba32f(), WIDTH, rows);
        Ok(t)
    }

    pub fn texture(&self) -> TextureId {
        self.texture
    }

    pub fn clear(&mut self) {
        self.data.clear();
    }

    /// Instances so far this frame.
    pub fn len(&self) -> i32 {
        (self.data.len() / FLOATS) as i32
    }

    /// Appends `m` (affine: its last row is dropped); returns its index.
    pub fn push(&mut self, m: &Mat4) -> i32 {
        let i = self.len();
        for r in 0..3 {
            self.data.extend_from_slice(&m.row(r).to_array());
        }
        i
    }

    /// Uploads this frame's data (if it changed), growing the texture —
    /// rebound into `group`'s `field` — when it no longer fits.
    pub fn upload(
        &mut self,
        ctx: &Context,
        group: ParamGroupId,
        field: ParamField,
    ) -> Result<(), Error> {
        let texels = (self.data.len() / 4) as i32;
        let rows = ((texels + WIDTH - 1) / WIDTH).max(1);
        if rows > self.rows {
            let rows = (rows as u32).next_power_of_two() as i32;
            let texture = Self::make_texture(ctx, rows)?;
            ctx.destroy_texture(std::mem::replace(&mut self.texture, texture));
            ctx.param_set_texture(group, field, texture);
            self.rows = rows;
            self.uploaded.clear();
        }
        if self.data.is_empty() || self.data == self.uploaded {
            return Ok(());
        }
        // Whole rows: pad the last.
        self.bytes.clear();
        for v in &self.data {
            self.bytes.extend_from_slice(&v.to_le_bytes());
        }
        self.bytes.resize((rows * WIDTH * 4 * 4) as usize, 0);
        let format = TextureFormat::rgba32f();
        ctx.texture_sub_image_2d(self.texture, 0, 0, 0, WIDTH, rows, &format, &self.bytes);
        self.uploaded.clear();
        self.uploaded.extend_from_slice(&self.data);
        Ok(())
    }

    pub fn destroy(&self, ctx: &Context) {
        ctx.destroy_texture(self.texture);
    }
}
