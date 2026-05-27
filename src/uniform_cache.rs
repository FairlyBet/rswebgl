use web_sys::{WebGl2RenderingContext, WebGlProgram, WebGlUniformLocation};

use crate::console;

#[derive(Debug, Clone)]
pub struct UniformCache {
    entries: Vec<(Box<str>, WebGlUniformLocation)>,
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
        match self.entries.binary_search_by(|(k, _)| k.as_ref().cmp(name)) {
            Ok(idx) => Some(&self.entries[idx].1),
            Err(idx) => match gl.get_uniform_location(program, name) {
                Some(loc) => {
                    self.entries.insert(idx, (name.into(), loc));
                    Some(&self.entries[idx].1)
                }
                None => {
                    console::warn(&format!("[rswebgl] uniform not found: \"{name}\""));
                    None
                }
            },
        }
    }
}
