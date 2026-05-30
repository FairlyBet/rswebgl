use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext;

use crate::draw::Viewport;

// ---------------------------------------------------------------------------
// ClearMask
// ---------------------------------------------------------------------------

#[wasm_bindgen]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClearMask {
    pub color: bool,
    pub depth: bool,
    pub stencil: bool,
}

#[wasm_bindgen]
impl ClearMask {
    pub fn none() -> Self {
        Self {
            color: false,
            depth: false,
            stencil: false,
        }
    }
    pub fn all() -> Self {
        Self {
            color: true,
            depth: true,
            stencil: true,
        }
    }
    pub fn color() -> Self {
        Self {
            color: true,
            depth: false,
            stencil: false,
        }
    }
    pub fn depth() -> Self {
        Self {
            color: false,
            depth: true,
            stencil: false,
        }
    }
    pub fn stencil() -> Self {
        Self {
            color: false,
            depth: false,
            stencil: true,
        }
    }
    pub fn color_depth() -> Self {
        Self {
            color: true,
            depth: true,
            stencil: false,
        }
    }
    pub fn color_depth_stencil() -> Self {
        Self::all()
    }
}

impl ClearMask {
    pub(crate) fn as_gl(&self) -> u32 {
        let mut m = 0u32;
        if self.color {
            m |= WebGl2RenderingContext::COLOR_BUFFER_BIT;
        }
        if self.depth {
            m |= WebGl2RenderingContext::DEPTH_BUFFER_BIT;
        }
        if self.stencil {
            m |= WebGl2RenderingContext::STENCIL_BUFFER_BIT;
        }
        m
    }
}

// ---------------------------------------------------------------------------
// DefaultFramebuffer
// ---------------------------------------------------------------------------

struct DefaultFramebufferInner {
    viewport: Viewport,
    clear_color: [f32; 4],
    clear_depth: f32,
    clear_stencil: i32,
}

#[wasm_bindgen]
pub struct DefaultFramebuffer {
    inner: Rc<RefCell<DefaultFramebufferInner>>,
}

impl DefaultFramebuffer {
    pub(crate) fn new(viewport: Viewport) -> Self {
        Self {
            inner: Rc::new(RefCell::new(DefaultFramebufferInner {
                viewport,
                clear_color: [0.0, 0.0, 0.0, 1.0],
                clear_depth: 1.0,
                clear_stencil: 0,
            })),
        }
    }

    pub(crate) fn handle(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
        }
    }

    pub(crate) fn viewport(&self) -> Viewport {
        self.inner.borrow().viewport
    }

    pub(crate) fn clear_color_rgba(&self) -> [f32; 4] {
        self.inner.borrow().clear_color
    }

    pub(crate) fn clear_depth_value(&self) -> f32 {
        self.inner.borrow().clear_depth
    }

    pub(crate) fn clear_stencil_value(&self) -> i32 {
        self.inner.borrow().clear_stencil
    }
}

#[wasm_bindgen]
impl DefaultFramebuffer {
    pub fn set_viewport(&self, v: Viewport) {
        self.inner.borrow_mut().viewport = v;
    }

    pub fn set_clear_color(&self, r: f32, g: f32, b: f32, a: f32) {
        self.inner.borrow_mut().clear_color = [r, g, b, a];
    }

    pub fn set_clear_depth(&self, d: f32) {
        self.inner.borrow_mut().clear_depth = d;
    }

    pub fn set_clear_stencil(&self, s: i32) {
        self.inner.borrow_mut().clear_stencil = s;
    }
}
