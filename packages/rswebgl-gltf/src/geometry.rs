//! Translate a glTF mesh primitive into a `VertexArray` + `DrawCommand`.
//!
//! Attribute locations follow a fixed convention the shaders agree on:
//!   0 = POSITION (vec3), 1 = NORMAL (vec3), 2 = TEXCOORD_0 (vec2),
//!   3 = TANGENT (vec4), 4 = COLOR_0 (vec4).
//! All attributes are uploaded as `f32` and indices as `u32`: the `gltf` util
//! readers dequantize the normalized/integer source variants for us, so a single
//! uniform `VaoBuilder` path covers every primitive (no normalized-int handling
//! in v1). WebGL2 supports `u32` indices natively.

use rswebgl::console;
use rswebgl::context::Context;
use rswebgl::draw::{DrawCommand, DrawMode, IndexType};
use rswebgl::vao::VertexArray;
use rswebgl::vao_builder::{AttrKind, VaoBuilder};

/// GPU geometry for one primitive plus its local-space axis-aligned bounds
/// (used to frame the camera).
pub struct PrimitiveGpu {
    pub vao: VertexArray,
    pub draw: DrawCommand,
    pub aabb_min: [f32; 3],
    pub aabb_max: [f32; 3],
}

/// Build the GPU geometry for one primitive. Returns `None` (with a logged
/// reason) if it has no POSITION or the VAO fails to build.
pub fn build_primitive(
    ctx: &Context,
    primitive: &gltf::Primitive,
    buffers: &[Vec<u8>],
) -> Option<PrimitiveGpu> {
    let reader = primitive.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));

    let positions: Vec<[f32; 3]> = match reader.read_positions() {
        Some(p) => p.collect(),
        None => {
            console::warn("[rswebgl-gltf] primitive has no POSITION; skipped");
            return None;
        }
    };
    let vertex_count = positions.len() as i32;

    let mut aabb_min = [f32::INFINITY; 3];
    let mut aabb_max = [f32::NEG_INFINITY; 3];
    for p in &positions {
        for i in 0..3 {
            aabb_min[i] = aabb_min[i].min(p[i]);
            aabb_max[i] = aabb_max[i].max(p[i]);
        }
    }

    let mut builder = VaoBuilder::new(ctx);
    builder.add_f32(0, AttrKind::F32x3, &flatten(&positions));

    if let Some(normals) = reader.read_normals() {
        let n: Vec<[f32; 3]> = normals.collect();
        builder.add_f32(1, AttrKind::F32x3, &flatten(&n));
    }
    if let Some(uv) = reader.read_tex_coords(0) {
        let uv: Vec<[f32; 2]> = uv.into_f32().collect();
        builder.add_f32(2, AttrKind::F32x2, &flatten(&uv));
    }
    if let Some(tangents) = reader.read_tangents() {
        let t: Vec<[f32; 4]> = tangents.collect();
        builder.add_f32(3, AttrKind::F32x4, &flatten(&t));
    }
    if let Some(colors) = reader.read_colors(0) {
        let c: Vec<[f32; 4]> = colors.into_rgba_f32().collect();
        builder.add_f32(4, AttrKind::F32x4, &flatten(&c));
    }

    let mode = draw_mode(primitive.mode());
    let draw = match reader.read_indices() {
        Some(indices) => {
            let indices: Vec<u32> = indices.into_u32().collect();
            let count = indices.len() as i32;
            builder.add_indices_u32(&indices);
            DrawCommand::elements(mode, count, IndexType::UnsignedInt, 0)
        }
        None => DrawCommand::arrays(mode, 0, vertex_count),
    };

    match builder.build() {
        Ok(vao) => Some(PrimitiveGpu {
            vao,
            draw,
            aabb_min,
            aabb_max,
        }),
        Err(e) => {
            console::error(&format!("[rswebgl-gltf] VAO build failed: {e}"));
            None
        }
    }
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

/// `[[f32; N]]` → flat `Vec<f32>` for `VaoBuilder::add_f32`.
fn flatten<const N: usize>(v: &[[f32; N]]) -> Vec<f32> {
    let mut out = Vec::with_capacity(v.len() * N);
    for a in v {
        out.extend_from_slice(a);
    }
    out
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
        assert_eq!(flatten(&v), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    }
}
