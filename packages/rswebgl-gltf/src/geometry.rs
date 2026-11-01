//! Translate glTF meshes into vertex arrays + draw commands, sharing GPU
//! buffers wherever the document shares data.
//!
//! Attribute locations follow a fixed convention the shaders agree on:
//!   0 = POSITION (vec3), 1 = NORMAL (vec3), 2 = TEXCOORD_0 (vec2),
//!   3 = TANGENT (vec4), 4 = COLOR_0 (vec4).
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

/// One primitive on the GPU, plus its local-space bounds.
#[derive(Clone)]
pub struct PrimitiveGpu {
    pub vao: VertexArrayId,
    pub draw: DrawCommand,
    /// glTF material index, or -1 for the default material.
    pub material: i32,
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],
}

/// GPU buffers created so far, by accessor index, and the vertex arrays built
/// from them. Owned by the model and destroyed with it.
#[derive(Default)]
pub struct GeometryCache {
    vertex: HashMap<usize, BufferId>,
    index: HashMap<usize, (BufferId, IndexType)>,
    meshes: HashMap<usize, Vec<PrimitiveGpu>>,
}

impl GeometryCache {
    /// Every buffer created (to destroy with the model).
    pub fn buffers(&self) -> impl Iterator<Item = BufferId> + '_ {
        self.vertex
            .values()
            .copied()
            .chain(self.index.values().map(|(b, _)| *b))
    }

    /// Every vertex array created, one per primitive of each built mesh.
    pub fn vertex_arrays(&self) -> impl Iterator<Item = VertexArrayId> + '_ {
        self.meshes.values().flatten().map(|p| p.vao)
    }

    /// Number of distinct meshes built.
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// The primitives of `mesh`, built on first request.
    pub fn mesh(
        &mut self,
        ctx: &Context,
        mesh: &gltf::Mesh,
        buffers: &[Vec<u8>],
    ) -> &[PrimitiveGpu] {
        if !self.meshes.contains_key(&mesh.index()) {
            let prims = mesh
                .primitives()
                .filter_map(|p| self.build_primitive(ctx, &p, buffers))
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
    ) -> Option<PrimitiveGpu> {
        let reader = primitive.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));
        let Some(pos_accessor) = primitive.get(&gltf::Semantic::Positions) else {
            console::warn("[rswebgl-gltf] primitive has no POSITION; skipped");
            return None;
        };
        let vertex_count = pos_accessor.count() as i32;
        let (aabb_min, aabb_max) = accessor_bounds(&pos_accessor).unwrap_or_else(|| {
            // Bounds are required by the spec for POSITION; compute if absent.
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for p in reader.read_positions().into_iter().flatten() {
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

        // (location, semantic, components)
        let attrs = [
            (0, gltf::Semantic::Positions, 3u8),
            (1, gltf::Semantic::Normals, 3),
            (2, gltf::Semantic::TexCoords(0), 2),
            (3, gltf::Semantic::Tangents, 4),
            (4, gltf::Semantic::Colors(0), 4),
        ];
        for (loc, semantic, size) in attrs {
            let Some(accessor) = primitive.get(&semantic) else {
                continue;
            };
            let Some(buf) = self.vertex_buffer(ctx, &reader, &accessor, &semantic) else {
                continue;
            };
            ctx.vertex_array_attr(vao, loc, buf, &VertexAttr::float(size));
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
            aabb_min,
            aabb_max,
        })
    }

    // The buffer for a vertex accessor, created on first use.
    fn vertex_buffer<'a, 's, F>(
        &mut self,
        ctx: &Context,
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
            gltf::Semantic::Positions => flatten(reader.read_positions()?),
            gltf::Semantic::Normals => flatten(reader.read_normals()?),
            gltf::Semantic::TexCoords(n) => flatten(reader.read_tex_coords(*n)?.into_f32()),
            gltf::Semantic::Tangents => flatten(reader.read_tangents()?),
            gltf::Semantic::Colors(n) => flatten(reader.read_colors(*n)?.into_rgba_f32()),
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
