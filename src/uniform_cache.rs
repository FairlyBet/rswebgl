use rustc_hash::FxHashMap;
use web_sys::{WebGl2RenderingContext, WebGlProgram, WebGlUniformLocation};

use crate::console;

#[derive(Debug)]
pub struct UniformCache {
    locations: FxHashMap<Box<str>, WebGlUniformLocation>,
}

impl UniformCache {
    pub fn new() -> Self {
        Self {
            locations: FxHashMap::default(),
        }
    }

    pub fn get(
        &mut self,
        gl: &WebGl2RenderingContext,
        program: &WebGlProgram,
        name: &str,
    ) -> Option<&WebGlUniformLocation> {
        if !self.locations.contains_key(name) {
            match gl.get_uniform_location(program, name) {
                Some(loc) => {
                    self.locations.insert(name.into(), loc);
                }
                None => {
                    console::warn(&format!("[rswebgl] uniform not found: \"{name}\""));
                    return None;
                }
            }
        }
        self.locations.get(name)
    }
}
