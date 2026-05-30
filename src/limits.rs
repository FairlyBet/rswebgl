use std::sync::OnceLock;

use web_sys::WebGl2RenderingContext;

// WebGL2 spec minimums (used as fallback if query fails or runs before init).
const MIN_COMBINED_TEXTURE_UNITS: u32 = 32;

static MAX_COMBINED_TEXTURE_UNITS: OnceLock<u32> = OnceLock::new();

pub(crate) fn init(gl: &WebGl2RenderingContext) {
    MAX_COMBINED_TEXTURE_UNITS.get_or_init(|| {
        gl.get_parameter(WebGl2RenderingContext::MAX_COMBINED_TEXTURE_IMAGE_UNITS)
            .ok()
            .and_then(|v| v.as_f64())
            .map(|n| n as u32)
            .unwrap_or(MIN_COMBINED_TEXTURE_UNITS)
    });
}

pub fn max_combined_texture_units() -> u32 {
    *MAX_COMBINED_TEXTURE_UNITS
        .get()
        .unwrap_or(&MIN_COMBINED_TEXTURE_UNITS)
}
