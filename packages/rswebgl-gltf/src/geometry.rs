//! Translate glTF meshes into vertex arrays + draw commands, sharing GPU
//! buffers wherever the document shares data.
//!
//! Attribute locations follow a fixed convention the shaders agree on:
//!   0 = POSITION (vec3), 1 = NORMAL (vec3), 2 = TEXCOORD_0 (vec2),
//!   3 = TANGENT (vec4), 4 = COLOR_0 (vec4), 5 = TEXCOORD_1 (vec2),
//!   6 = JOINTS_0 (vec4 of joint indices, as floats), 7 = WEIGHTS_0 (vec4).
//! Morph targets go into a texture per primitive (`MorphGpu`): the shader
//! adds their weighted deltas before skinning.
//! Which of them a primitive has is in `PrimitiveGpu::attributes` (`ATTR_*`).
//!
//! Sharing: one GPU buffer per glTF *accessor*, so primitives that reuse an
//! accessor (a multi-material mesh splitting one vertex set by index ranges,
//! say) reuse the buffer; vertex arrays only retain them (rswebgl counts uses,
//! and a destroyed buffer still in use is freed with its last user). Each mesh
//! is built once however many nodes instance it.
//!
//! Attributes are uploaded as `f32` (the `gltf` readers dequantize normalized /
//! integer variants, so one attribute path covers every primitive); indices
//! keep their source width (u8 / u16 / u32) — WebGL2 draws all three.

use std::collections::HashMap;

use rswebgl::buffer::BufferUsage;
use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::ids::{BufferId, VertexArrayId};
use rswebgl::vao::VertexAttr;

use rswebgl::ids::TextureId;
use rswebgl::texture::{TextureFormat, TextureMagFilter, TextureMinFilter, TextureTarget};

use crate::{
    ATTR_COLOR, ATTR_MORPH_NORMAL, ATTR_MORPH_POSITION, ATTR_MORPH_TANGENT, ATTR_NORMAL, ATTR_SKIN,
    ATTR_SKIN8, ATTR_TANGENT, ATTR_UV0, ATTR_UV1,
};

/// Texels per row of a morph texture; `MORPH_TEXTURE_WIDTH` in the shader.
pub const MORPH_TEXTURE_WIDTH: i32 = 2048;

/// A primitive's morph targets on the GPU: an RGBA32F texture of deltas —
/// for vertex `v`, target `t` and kind `k` (position, normal, tangent, those
/// present in that order) the texel `(v * targets + t) * kinds + k`,
/// `MORPH_TEXTURE_WIDTH` texels a row.
#[derive(Clone, Copy, Debug)]
pub struct MorphGpu {
    pub texture: TextureId,
    pub targets: u32,
}

/// One primitive on the GPU, plus its local-space bounds.
#[derive(Clone)]
pub struct PrimitiveGpu {
    pub vao: VertexArrayId,
    pub draw: DrawCommand,
    /// glTF material index, or -1 for the default material.
    pub material: i32,
    /// `ATTR_*` bits: the optional attributes bound.
    pub attributes: u32,
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],
    pub morph: Option<MorphGpu>,
}

/// GPU buffers created so far, by accessor index, and the vertex arrays built
/// from them. Owned by the model and destroyed with it.
#[derive(Default)]
pub struct GeometryCache {
    vertex: HashMap<usize, BufferId>,
    index: HashMap<usize, (BufferId, IndexType)>,
    meshes: HashMap<usize, Vec<PrimitiveGpu>>,
    // The TEXCOORD sets each material reads, in its two UV slots
    // (`material::assign_uv_slots`; [0, 1] for one not listed).
    uv_sets: Vec<[u32; 2]>,
    // Skin weights, renormalized (by WEIGHTS_0 and WEIGHTS_1 accessor).
    weights: HashMap<(usize, Option<usize>), (BufferId, Option<BufferId>)>,
}

impl GeometryCache {
    /// Every buffer created (to destroy with the model).
    pub fn buffers(&self) -> impl Iterator<Item = BufferId> + '_ {
        self.vertex
            .values()
            .copied()
            .chain(self.index.values().map(|(b, _)| *b))
            .chain(
                self.weights
                    .values()
                    .flat_map(|(a, b)| std::iter::once(*a).chain(*b)),
            )
    }

    /// Every vertex array created, one per primitive of each built mesh.
    pub fn vertex_arrays(&self) -> impl Iterator<Item = VertexArrayId> + '_ {
        self.meshes.values().flatten().map(|p| p.vao)
    }

    /// Every morph texture created (to destroy with the model).
    pub fn morph_textures(&self) -> impl Iterator<Item = TextureId> + '_ {
        self.meshes
            .values()
            .flatten()
            .filter_map(|p| p.morph.map(|m| m.texture))
    }

    /// Number of distinct meshes built.
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// The TEXCOORD sets material `m` reads go into the UV slots as
    /// `uv_sets[m]` says (before building meshes).
    pub fn set_uv_sets(&mut self, uv_sets: Vec<[u32; 2]>) {
        self.uv_sets = uv_sets;
    }

    /// The primitives of `mesh`, built on first request.
    pub fn mesh(
        &mut self,
        ctx: &Context,
        mesh: &gltf::Mesh,
        buffers: &[Vec<u8>],
    ) -> &[PrimitiveGpu] {
        if !self.meshes.contains_key(&mesh.index()) {
            let uv_sets = self.uv_sets.clone();
            let prims = mesh
                .primitives()
                .filter_map(|p| {
                    let uv = p
                        .material()
                        .index()
                        .and_then(|m| uv_sets.get(m))
                        .copied()
                        .unwrap_or([0, 1]);
                    self.build_primitive(ctx, &p, buffers, uv)
                })
                .collect();
            self.meshes.insert(mesh.index(), prims);
        }
        &self.meshes[&mesh.index()]
    }

    /// Build one primitive's vertex array. `None` (with a logged reason) if it
    /// has no POSITION or a GPU object can't be created.
    fn build_primitive(
        &mut self,
        ctx: &Context,
        primitive: &gltf::Primitive,
        buffers: &[Vec<u8>],
        uv: [u32; 2],
    ) -> Option<PrimitiveGpu> {
        let reader = primitive.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));
        let Some(pos_accessor) = primitive.get(&gltf::Semantic::Positions) else {
            console::warn("[rswebgl-gltf] primitive has no POSITION; skipped");
            return None;
        };
        let vertex_count = pos_accessor.count() as i32;
        // Normalized integer positions (KHR_mesh_quantization) have their min
        // and max in integer units: computed from the data then.
        let quantized =
            pos_accessor.normalized() && pos_accessor.data_type() != gltf::accessor::DataType::F32;
        let bounds = if quantized {
            None
        } else {
            accessor_bounds(&pos_accessor)
        };
        let (aabb_min, aabb_max) = bounds.unwrap_or_else(|| {
            // Bounds are required by the spec for POSITION; compute if absent.
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for p in crate::accessor::read_vecs::<3>(&pos_accessor, buffers)
                .into_iter()
                .flatten()
            {
                for i in 0..3 {
                    min[i] = min[i].min(p[i]);
                    max[i] = max[i].max(p[i]);
                }
            }
            (min, max)
        });

        let vao = match ctx.create_vertex_array() {
            Ok(v) => v,
            Err(e) => {
                console::error(&format!("[rswebgl-gltf] vertex array: {e}"));
                return None;
            }
        };

        // (location, semantic, components, ATTR_* bit)
        let attrs = [
            (0, gltf::Semantic::Positions, 3u8, 0),
            (1, gltf::Semantic::Normals, 3, ATTR_NORMAL),
            (2, gltf::Semantic::TexCoords(uv[0]), 2, ATTR_UV0),
            (3, gltf::Semantic::Tangents, 4, ATTR_TANGENT),
            (4, gltf::Semantic::Colors(0), 4, ATTR_COLOR),
            (5, gltf::Semantic::TexCoords(uv[1]), 2, ATTR_UV1),
        ];
        let mut attributes = 0;
        for (loc, semantic, size, bit) in attrs {
            let Some(accessor) = primitive.get(&semantic) else {
                continue;
            };
            let Some(buf) = self.vertex_buffer(ctx, buffers, &reader, &accessor, &semantic) else {
                continue;
            };
            ctx.vertex_array_attr(vao, loc, buf, &VertexAttr::float(size));
            attributes |= bit;
        }
        // Skinning needs both: four joint indices and their weights, and
        // four more with JOINTS_1 / WEIGHTS_1.
        let joints = primitive.get(&gltf::Semantic::Joints(0));
        let weights = primitive.get(&gltf::Semantic::Weights(0));
        if let (Some(j), Some(w)) = (joints, weights) {
            let more = primitive
                .get(&gltf::Semantic::Joints(1))
                .zip(primitive.get(&gltf::Semantic::Weights(1)));
            let j = self.vertex_buffer(ctx, buffers, &reader, &j, &gltf::Semantic::Joints(0));
            let j1 = more.as_ref().and_then(|(j1, _)| {
                self.vertex_buffer(ctx, buffers, &reader, j1, &gltf::Semantic::Joints(1))
            });
            let w = self.skin_weights(ctx, buffers, &w, more.as_ref().map(|(_, w1)| w1));
            if let (Some(j), Some((w, w1))) = (j, w) {
                ctx.vertex_array_attr(vao, 6, j, &VertexAttr::float(4));
                ctx.vertex_array_attr(vao, 7, w, &VertexAttr::float(4));
                attributes |= ATTR_SKIN;
                if let (Some(j1), Some(w1)) = (j1, w1) {
                    ctx.vertex_array_attr(vao, 8, j1, &VertexAttr::float(4));
                    ctx.vertex_array_attr(vao, 9, w1, &VertexAttr::float(4));
                    attributes |= ATTR_SKIN8;
                }
            }
        }
        let morph = morph_texture(ctx, primitive, buffers, vertex_count as usize, attributes);
        if let Some((_, kinds)) = &morph {
            attributes |= kinds;
        }

        let mode = draw_mode(primitive.mode());
        let draw = match primitive.indices() {
            Some(accessor) => match self.index_buffer(ctx, &reader, &accessor) {
                Some((buf, ty)) => {
                    ctx.vertex_array_set_index_buffer(vao, buf);
                    DrawCommand::elements(mode, accessor.count() as i32, ty, 0)
                }
                None => {
                    ctx.destroy_vertex_array(vao);
                    return None;
                }
            },
            None => DrawCommand::arrays(mode, 0, vertex_count),
        };

        let material = primitive.material().index().map_or(-1, |i| i as i32);
        Some(PrimitiveGpu {
            vao,
            draw,
            material,
            attributes,
            aabb_min,
            aabb_max,
            morph: morph.map(|(m, _)| m),
        })
    }

    // The skin weights (WEIGHTS_0, and WEIGHTS_1 if given) as floats,
    // renormalized so each vertex's sum to 1 (glTF 2.0 §3.7.3.3: exporters
    // quantize; a sum off 1 scales the mesh), created on first use.
    fn skin_weights(
        &mut self,
        ctx: &Context,
        buffers: &[Vec<u8>],
        w0: &gltf::Accessor,
        w1: Option<&gltf::Accessor>,
    ) -> Option<(BufferId, Option<BufferId>)> {
        let key = (w0.index(), w1.map(|a| a.index()));
        if let Some(&b) = self.weights.get(&key) {
            return Some(b);
        }
        let mut a = crate::accessor::read_vecs::<4>(w0, buffers)?;
        let mut b = match w1 {
            Some(w1) => Some(crate::accessor::read_vecs::<4>(w1, buffers)?),
            None => None,
        };
        for (i, w0) in a.iter_mut().enumerate() {
            let extra = b
                .as_ref()
                .map_or(0.0, |b| b.get(i).map_or(0.0, |w| w.iter().sum()));
            let sum: f32 = w0.iter().sum::<f32>() + extra;
            if sum > 0.0 && (sum - 1.0).abs() > 1e-6 {
                *w0 = w0.map(|w| w / sum);
                if let Some(w) = b.as_mut().and_then(|b| b.get_mut(i)) {
                    *w = w.map(|w| w / sum);
                }
            }
        }
        let upload = |v: &[[f32; 4]]| {
            ctx.create_buffer(BufferUsage::StaticDraw, bytes_of(v.as_flattened()))
                .inspect_err(|e| console::error(&format!("[rswebgl-gltf] skin weights: {e}")))
                .ok()
        };
        let first = upload(&a)?;
        let second = b.as_deref().and_then(upload);
        self.weights.insert(key, (first, second));
        Some((first, second))
    }

    // The buffer for a vertex accessor, created on first use.
    fn vertex_buffer<'a, 's, F>(
        &mut self,
        ctx: &Context,
        buffers: &[Vec<u8>],
        reader: &gltf::mesh::Reader<'a, 's, F>,
        accessor: &gltf::Accessor,
        semantic: &gltf::Semantic,
    ) -> Option<BufferId>
    where
        F: Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>,
    {
        if let Some(&b) = self.vertex.get(&accessor.index()) {
            return Some(b);
        }
        let data: Vec<f32> = match semantic {
            // Any component type (KHR_mesh_quantization), dequantized.
            gltf::Semantic::Positions
            | gltf::Semantic::Normals
            | gltf::Semantic::TexCoords(_)
            | gltf::Semantic::Tangents => crate::accessor::read_floats(accessor, buffers)?,
            gltf::Semantic::Colors(n) => flatten(reader.read_colors(*n)?.into_rgba_f32()),
            // Indices as floats: exact up to 2^24, cast back in the shader.
            gltf::Semantic::Joints(n) => {
                flatten(reader.read_joints(*n)?.into_u16().map(|j| j.map(f32::from)))
            }
            gltf::Semantic::Weights(n) => flatten(reader.read_weights(*n)?.into_f32()),
            _ => return None,
        };
        match ctx.create_buffer(BufferUsage::StaticDraw, bytes_of(&data)) {
            Ok(b) => {
                self.vertex.insert(accessor.index(), b);
                Some(b)
            }
            Err(e) => {
                console::error(&format!("[rswebgl-gltf] vertex buffer: {e}"));
                None
            }
        }
    }

    // The index buffer for an accessor, in its source width, created on first use.
    fn index_buffer<'a, 's, F>(
        &mut self,
        ctx: &Context,
        reader: &gltf::mesh::Reader<'a, 's, F>,
        accessor: &gltf::Accessor,
    ) -> Option<(BufferId, IndexType)>
    where
        F: Clone + Fn(gltf::Buffer<'a>) -> Option<&'s [u8]>,
    {
        if let Some(&b) = self.index.get(&accessor.index()) {
            return Some(b);
        }
        use gltf::mesh::util::ReadIndices;
        let (bytes, ty): (Vec<u8>, IndexType) = match reader.read_indices()? {
            ReadIndices::U8(it) => (it.collect(), IndexType::UnsignedByte),
            ReadIndices::U16(it) => (
                it.flat_map(u16::to_le_bytes).collect(),
                IndexType::UnsignedShort,
            ),
            ReadIndices::U32(it) => (
                it.flat_map(u32::to_le_bytes).collect(),
                IndexType::UnsignedInt,
            ),
        };
        match ctx.create_index_buffer(BufferUsage::StaticDraw, &bytes) {
            Ok(b) => {
                self.index.insert(accessor.index(), (b, ty));
                Some((b, ty))
            }
            Err(e) => {
                console::error(&format!("[rswebgl-gltf] index buffer: {e}"));
                None
            }
        }
    }
}

// The accessor's declared min/max, if it has them as 3 floats.
fn accessor_bounds(a: &gltf::Accessor) -> Option<([f32; 3], [f32; 3])> {
    let read = |v: Option<gltf::json::Value>| -> Option<[f32; 3]> {
        let arr = v?.as_array()?.clone();
        if arr.len() != 3 {
            return None;
        }
        let mut out = [0.0; 3];
        for (o, x) in out.iter_mut().zip(arr) {
            *o = x.as_f64()? as f32;
        }
        Some(out)
    };
    Some((read(a.min())?, read(a.max())?))
}

/// glTF primitive topology → our `DrawMode` (the GL enum values are identical).
fn draw_mode(mode: gltf::mesh::Mode) -> DrawMode {
    use gltf::mesh::Mode;
    match mode {
        Mode::Points => DrawMode::Points,
        Mode::Lines => DrawMode::Lines,
        Mode::LineLoop => DrawMode::LineLoop,
        Mode::LineStrip => DrawMode::LineStrip,
        Mode::Triangles => DrawMode::Triangles,
        Mode::TriangleStrip => DrawMode::TriangleStrip,
        Mode::TriangleFan => DrawMode::TriangleFan,
    }
}

/// Iterator of `[f32; N]` → flat `Vec<f32>`.
fn flatten<const N: usize>(v: impl Iterator<Item = [f32; N]>) -> Vec<f32> {
    let mut out = Vec::with_capacity(v.size_hint().0 * N);
    for a in v {
        out.extend_from_slice(&a);
    }
    out
}

// Displacements of one morph target: positions, normals, tangents.
type TargetDeltas = (
    Option<Vec<[f32; 3]>>,
    Option<Vec<[f32; 3]>>,
    Option<Vec<[f32; 3]>>,
);

// The primitive's morph targets as a texture of deltas (see `MorphGpu`), and
// the `ATTR_MORPH_*` kinds it holds. Normal and tangent deltas only where the
// primitive has normals and tangents (glTF 2.0 §3.7.2.2). None without
// targets, or when they don't fit a texture.
fn morph_texture(
    ctx: &Context,
    primitive: &gltf::Primitive,
    buffers: &[Vec<u8>],
    vertices: usize,
    attributes: u32,
) -> Option<(MorphGpu, u32)> {
    // Deltas of any component type (KHR_mesh_quantization), dequantized.
    let read =
        |a: Option<gltf::Accessor>| a.and_then(|a| crate::accessor::read_vecs::<3>(&a, buffers));
    let targets: Vec<TargetDeltas> = primitive
        .morph_targets()
        .map(|t| (read(t.positions()), read(t.normals()), read(t.tangents())))
        .collect();
    if targets.is_empty() {
        return None;
    }
    let mut kinds = 0;
    if targets.iter().any(|t| t.0.is_some()) {
        kinds |= ATTR_MORPH_POSITION;
    }
    if attributes & ATTR_NORMAL != 0 && targets.iter().any(|t| t.1.is_some()) {
        kinds |= ATTR_MORPH_NORMAL;
    }
    if attributes & ATTR_TANGENT != 0 && targets.iter().any(|t| t.2.is_some()) {
        kinds |= ATTR_MORPH_TANGENT;
    }
    if kinds == 0 {
        return None;
    }
    let per = kinds.count_ones() as usize;
    let width = MORPH_TEXTURE_WIDTH as usize;
    let rows = (vertices * targets.len() * per).div_ceil(width);
    if rows > rswebgl::limits::max_texture_size() as usize {
        console::warn(&format!(
            "[rswebgl-gltf] {} morph targets of {vertices} vertices don't fit a texture; \
             left out",
            targets.len()
        ));
        return None;
    }
    let mut data = vec![0.0f32; rows * width * 4];
    for v in 0..vertices {
        for (t, (p, n, tg)) in targets.iter().enumerate() {
            let base = (v * targets.len() + t) * per;
            let mut k = 0;
            for (kind, deltas) in [
                (ATTR_MORPH_POSITION, p),
                (ATTR_MORPH_NORMAL, n),
                (ATTR_MORPH_TANGENT, tg),
            ] {
                if kinds & kind == 0 {
                    continue;
                }
                if let Some(d) = deltas.as_ref().and_then(|d| d.get(v)) {
                    data[(base + k) * 4..][..3].copy_from_slice(d);
                }
                k += 1;
            }
        }
    }
    let texture = match ctx.create_texture(
        TextureTarget::Texture2D,
        TextureMinFilter::Nearest,
        TextureMagFilter::Nearest,
    ) {
        Ok(t) => t,
        Err(e) => {
            console::error(&format!("[rswebgl-gltf] morph texture: {e}"));
            return None;
        }
    };
    let format = TextureFormat::rgba32f();
    let (w, h) = (MORPH_TEXTURE_WIDTH, rows as i32);
    ctx.texture_storage_2d(texture, 1, &format, w, h);
    ctx.texture_sub_image_2d(texture, 0, 0, 0, w, h, &format, bytes_of(&data));
    let morph = MorphGpu {
        texture,
        targets: targets.len() as u32,
    };
    Some((morph, kinds))
}

fn bytes_of(v: &[f32]) -> &[u8] {
    // SAFETY: f32 has no padding or invalid bit patterns as bytes; the slice
    // covers exactly the same memory.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast(), std::mem::size_of_val(v)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gltf::mesh::Mode;

    #[test]
    fn primitive_mode_maps_to_draw_mode() {
        assert_eq!(draw_mode(Mode::Points), DrawMode::Points);
        assert_eq!(draw_mode(Mode::Lines), DrawMode::Lines);
        assert_eq!(draw_mode(Mode::LineLoop), DrawMode::LineLoop);
        assert_eq!(draw_mode(Mode::LineStrip), DrawMode::LineStrip);
        assert_eq!(draw_mode(Mode::Triangles), DrawMode::Triangles);
        assert_eq!(draw_mode(Mode::TriangleStrip), DrawMode::TriangleStrip);
        assert_eq!(draw_mode(Mode::TriangleFan), DrawMode::TriangleFan);
    }

    #[test]
    fn flatten_concatenates() {
        let v = [[1.0f32, 2.0, 3.0], [4.0, 5.0, 6.0]];
        assert_eq!(flatten(v.into_iter()), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }
}
