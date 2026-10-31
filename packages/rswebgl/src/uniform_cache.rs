use web_sys::{WebGl2RenderingContext, WebGlProgram, WebGlUniformLocation};

use crate::console;

// Uniform locations by name. Misses are cached too (as None): a name the shader
// doesn't have (or the compiler optimized out) is looked up and reported once,
// not on every draw.
#[derive(Debug, Clone)]
pub struct UniformCache {
    entries: Vec<(Box<str>, Option<WebGlUniformLocation>)>,
}

impl UniformCache {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn get(
        &mut self,
        gl: &WebGl2RenderingContext,
        program: &WebGlProgram,
        name: &str,
    ) -> Option<&WebGlUniformLocation> {
        let idx = match self.entries.binary_search_by(|(k, _)| k.as_ref().cmp(name)) {
            Ok(idx) => idx,
            Err(idx) => {
                let loc = gl.get_uniform_location(program, name);
                if loc.is_none() {
                    console::warn(&format!(
                        "[rswebgl] uniform not found: \"{name}\" (not declared, or unused \
                         and optimized out)"
                    ));
                }
                self.entries.insert(idx, (name.into(), loc));
                idx
            }
        };
        self.entries[idx].1.as_ref()
    }
}
