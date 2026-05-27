use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

// ---------------------------------------------------------------------------
// DrawMode
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawMode {
    Points        = 0x0000,
    Lines         = 0x0001,
    LineLoop      = 0x0002,
    LineStrip     = 0x0003,
    Triangles     = 0x0004,
    TriangleStrip = 0x0005,
    TriangleFan   = 0x0006,
}

impl DrawMode {
    fn as_gl(&self) -> u32 { *self as u32 }
}

// ---------------------------------------------------------------------------
// IndexType
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexType {
    UnsignedByte  = 0x1401,
    UnsignedShort = 0x1403,
    UnsignedInt   = 0x1405,
}

impl IndexType {
    fn as_gl(&self) -> u32 { *self as u32 }
}

// ---------------------------------------------------------------------------
// Viewport
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[wasm_bindgen]
impl Viewport {
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self { x, y, width, height }
    }
}

// ---------------------------------------------------------------------------
// DrawCommand
// ---------------------------------------------------------------------------

const ARRAYS: u8 = 0;
const ELEMENTS: u8 = 1;
const RANGE_ELEMENTS: u8 = 2;

#[wasm_bindgen]
#[derive(Clone, Copy, Debug)]
pub struct DrawCommand {
    pub mode: DrawMode,
    pub count: i32,
    pub first: i32,
    pub index_type: IndexType,
    pub offset: i32,
    pub instance_count: i32,
    pub range_start: u32,
    pub range_end: u32,
    kind: u8,
}

#[wasm_bindgen]
impl DrawCommand {
    pub fn arrays(mode: DrawMode, first: i32, count: i32) -> Self {
        Self {
            mode, count, first,
            index_type: IndexType::UnsignedShort,
            offset: 0, instance_count: 1,
            range_start: 0, range_end: 0,
            kind: ARRAYS,
        }
    }

    pub fn elements(mode: DrawMode, count: i32, index_type: IndexType, offset: i32) -> Self {
        Self {
            mode, count, first: 0,
            index_type, offset,
            instance_count: 1,
            range_start: 0, range_end: 0,
            kind: ELEMENTS,
        }
    }

    pub fn range_elements(
        mode: DrawMode, start: u32, end: u32,
        count: i32, index_type: IndexType, offset: i32,
    ) -> Self {
        Self {
            mode, count, first: 0,
            index_type, offset,
            instance_count: 1,
            range_start: start, range_end: end,
            kind: RANGE_ELEMENTS,
        }
    }
}

impl DrawCommand {
    pub(crate) fn execute(&self, gl: &WebGl2RenderingContext) {
        let m = self.mode.as_gl();
        match self.kind {
            ARRAYS => {
                gl.draw_arrays_instanced(m, self.first, self.count, self.instance_count);
            }
            ELEMENTS => {
                gl.draw_elements_instanced_with_i32(
                    m, self.count, self.index_type.as_gl(), self.offset, self.instance_count,
                );
            }
            RANGE_ELEMENTS => {
                gl.draw_range_elements_with_i32(
                    m, self.range_start, self.range_end,
                    self.count, self.index_type.as_gl(), self.offset,
                );
            }
            _ => {}
        }
    }
}
