use std::sync::OnceLock;

use web_sys::WebGl2RenderingContext;

// WebGL2 spec minimums (used as fallback if query fails or runs before init).
const MIN_COMBINED_TEXTURE_UNITS: u32 = 32;
const MIN_UNIFORM_BLOCK_SIZE: u32 = 16384;
const MIN_UNIFORM_BUFFER_BINDINGS: u32 = 24;
// No spec minimum; 256 is the common hardware value and a safe (over-)alignment.
const DEFAULT_UNIFORM_BUFFER_OFFSET_ALIGNMENT: u32 = 256;

static MAX_COMBINED_TEXTURE_UNITS: OnceLock<u32> = OnceLock::new();
static MAX_UNIFORM_BLOCK_SIZE: OnceLock<u32> = OnceLock::new();
static MAX_UNIFORM_BUFFER_BINDINGS: OnceLock<u32> = OnceLock::new();
static UNIFORM_BUFFER_OFFSET_ALIGNMENT: OnceLock<u32> = OnceLock::new();

pub(crate) fn init(gl: &WebGl2RenderingContext) {
    let query = |pname: u32, fallback: u32| {
        gl.get_parameter(pname)
            .ok()
            .and_then(|v| v.as_f64())
            .map(|n| n as u32)
            .unwrap_or(fallback)
    };
    MAX_COMBINED_TEXTURE_UNITS.get_or_init(|| {
        query(
            WebGl2RenderingContext::MAX_COMBINED_TEXTURE_IMAGE_UNITS,
            MIN_COMBINED_TEXTURE_UNITS,
        )
    });
    MAX_UNIFORM_BLOCK_SIZE.get_or_init(|| {
        query(
            WebGl2RenderingContext::MAX_UNIFORM_BLOCK_SIZE,
            MIN_UNIFORM_BLOCK_SIZE,
        )
    });
    MAX_UNIFORM_BUFFER_BINDINGS.get_or_init(|| {
        query(
            WebGl2RenderingContext::MAX_UNIFORM_BUFFER_BINDINGS,
            MIN_UNIFORM_BUFFER_BINDINGS,
        )
    });
    UNIFORM_BUFFER_OFFSET_ALIGNMENT.get_or_init(|| {
        query(
            WebGl2RenderingContext::UNIFORM_BUFFER_OFFSET_ALIGNMENT,
            DEFAULT_UNIFORM_BUFFER_OFFSET_ALIGNMENT,
        )
    });
}

pub fn max_combined_texture_units() -> u32 {
    *MAX_COMBINED_TEXTURE_UNITS
        .get()
        .unwrap_or(&MIN_COMBINED_TEXTURE_UNITS)
}

/// Maximum size in bytes of a uniform block (`MAX_UNIFORM_BLOCK_SIZE`); spec
/// minimum is 16 KiB.
pub fn max_uniform_block_size() -> u32 {
    *MAX_UNIFORM_BLOCK_SIZE
        .get()
        .unwrap_or(&MIN_UNIFORM_BLOCK_SIZE)
}

/// Number of uniform-buffer binding points (`MAX_UNIFORM_BUFFER_BINDINGS`); spec
/// minimum is 24.
pub fn max_uniform_buffer_bindings() -> u32 {
    *MAX_UNIFORM_BUFFER_BINDINGS
        .get()
        .unwrap_or(&MIN_UNIFORM_BUFFER_BINDINGS)
}

/// Required byte alignment for `bindBufferRange` offsets into a uniform buffer
/// (`UNIFORM_BUFFER_OFFSET_ALIGNMENT`); hardware-defined, commonly 256.
pub fn uniform_buffer_offset_alignment() -> u32 {
    *UNIFORM_BUFFER_OFFSET_ALIGNMENT
        .get()
        .unwrap_or(&DEFAULT_UNIFORM_BUFFER_OFFSET_ALIGNMENT)
}
