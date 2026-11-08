// Implementation limits, queried once from the first context and checked
// wherever exceeding them would otherwise surface as an opaque GL error.
// Before `init` (or if a query fails) the WebGL2 spec minimums are used, so a
// check can only be stricter than the hardware, never looser.

use std::sync::OnceLock;

use web_sys::WebGl2RenderingContext as Gl;

#[derive(Debug, Clone, Copy)]
struct Limits {
    max_combined_texture_units: u32,
    max_texture_image_units: u32,
    max_uniform_block_size: u32,
    max_uniform_buffer_bindings: u32,
    uniform_buffer_offset_alignment: u32,
    max_vertex_attribs: u32,
    max_texture_size: u32,
    max_3d_texture_size: u32,
    max_array_texture_layers: u32,
    max_cube_map_texture_size: u32,
    max_color_attachments: u32,
    max_draw_buffers: u32,
    max_samples: u32,
    max_renderbuffer_size: u32,
}

// WebGL2 spec minimums.
const MINIMUMS: Limits = Limits {
    max_combined_texture_units: 32,
    max_texture_image_units: 16,
    max_uniform_block_size: 16384,
    max_uniform_buffer_bindings: 24,
    // No spec minimum; 256 is the common hardware value and a safe (over-)alignment.
    uniform_buffer_offset_alignment: 256,
    max_vertex_attribs: 16,
    max_texture_size: 2048,
    max_3d_texture_size: 256,
    max_array_texture_layers: 256,
    max_cube_map_texture_size: 2048,
    max_color_attachments: 4,
    max_draw_buffers: 4,
    max_samples: 4,
    max_renderbuffer_size: 2048,
};

static LIMITS: OnceLock<Limits> = OnceLock::new();

pub(crate) fn init(gl: &Gl) {
    LIMITS.get_or_init(|| {
        let q = |pname: u32, fallback: u32| {
            gl.get_parameter(pname)
                .ok()
                .and_then(|v| v.as_f64())
                .map_or(fallback, |n| n as u32)
        };
        let m = MINIMUMS;
        Limits {
            max_combined_texture_units: q(
                Gl::MAX_COMBINED_TEXTURE_IMAGE_UNITS,
                m.max_combined_texture_units,
            ),
            max_texture_image_units: q(Gl::MAX_TEXTURE_IMAGE_UNITS, m.max_texture_image_units),
            max_uniform_block_size: q(Gl::MAX_UNIFORM_BLOCK_SIZE, m.max_uniform_block_size),
            max_uniform_buffer_bindings: q(
                Gl::MAX_UNIFORM_BUFFER_BINDINGS,
                m.max_uniform_buffer_bindings,
            ),
            uniform_buffer_offset_alignment: q(
                Gl::UNIFORM_BUFFER_OFFSET_ALIGNMENT,
                m.uniform_buffer_offset_alignment,
            ),
            max_vertex_attribs: q(Gl::MAX_VERTEX_ATTRIBS, m.max_vertex_attribs),
            max_texture_size: q(Gl::MAX_TEXTURE_SIZE, m.max_texture_size),
            max_3d_texture_size: q(Gl::MAX_3D_TEXTURE_SIZE, m.max_3d_texture_size),
            max_array_texture_layers: q(Gl::MAX_ARRAY_TEXTURE_LAYERS, m.max_array_texture_layers),
            max_cube_map_texture_size: q(
                Gl::MAX_CUBE_MAP_TEXTURE_SIZE,
                m.max_cube_map_texture_size,
            ),
            max_color_attachments: q(Gl::MAX_COLOR_ATTACHMENTS, m.max_color_attachments),
            max_draw_buffers: q(Gl::MAX_DRAW_BUFFERS, m.max_draw_buffers),
            max_samples: q(Gl::MAX_SAMPLES, m.max_samples),
            max_renderbuffer_size: q(Gl::MAX_RENDERBUFFER_SIZE, m.max_renderbuffer_size),
        }
    });
}

fn get() -> &'static Limits {
    LIMITS.get().unwrap_or(&MINIMUMS)
}

/// `MAX_TEXTURE_IMAGE_UNITS`: textures a fragment shader can sample (16 at
/// least, often exactly 16 on mobile).
pub fn max_texture_image_units() -> u32 {
    get().max_texture_image_units
}

/// `MAX_COMBINED_TEXTURE_IMAGE_UNITS`: texture units one program can sample.
pub fn max_combined_texture_units() -> u32 {
    get().max_combined_texture_units
}

/// `MAX_UNIFORM_BLOCK_SIZE`: bytes in one uniform block; spec minimum 16 KiB.
pub fn max_uniform_block_size() -> u32 {
    get().max_uniform_block_size
}

/// `MAX_UNIFORM_BUFFER_BINDINGS`: uniform-buffer binding points; spec minimum 24.
pub fn max_uniform_buffer_bindings() -> u32 {
    get().max_uniform_buffer_bindings
}

/// Required byte alignment for `bindBufferRange` offsets into a uniform buffer
/// (`UNIFORM_BUFFER_OFFSET_ALIGNMENT`); hardware-defined, commonly 256.
pub fn uniform_buffer_offset_alignment() -> u32 {
    get().uniform_buffer_offset_alignment
}

/// `MAX_VERTEX_ATTRIBS`: vertex attribute slots; spec minimum 16.
pub fn max_vertex_attribs() -> u32 {
    get().max_vertex_attribs
}

/// `MAX_TEXTURE_SIZE`: width/height of a 2D texture or 2D-array layer.
pub fn max_texture_size() -> u32 {
    get().max_texture_size
}

/// `MAX_3D_TEXTURE_SIZE`: any dimension of a 3D texture.
pub fn max_3d_texture_size() -> u32 {
    get().max_3d_texture_size
}

/// `MAX_ARRAY_TEXTURE_LAYERS`: layers of a 2D-array texture.
pub fn max_array_texture_layers() -> u32 {
    get().max_array_texture_layers
}

/// `MAX_CUBE_MAP_TEXTURE_SIZE`: face size of a cube map.
pub fn max_cube_map_texture_size() -> u32 {
    get().max_cube_map_texture_size
}

/// `MAX_COLOR_ATTACHMENTS`: color attachment slots per framebuffer.
pub fn max_color_attachments() -> u32 {
    get().max_color_attachments
}

/// `MAX_DRAW_BUFFERS`: color outputs one draw can write (MRT).
pub fn max_draw_buffers() -> u32 {
    get().max_draw_buffers
}

/// `MAX_SAMPLES`: sample count for multisample renderbuffers.
pub fn max_samples() -> u32 {
    get().max_samples
}

/// `MAX_RENDERBUFFER_SIZE`: width/height of a renderbuffer.
pub fn max_renderbuffer_size() -> u32 {
    get().max_renderbuffer_size
}
