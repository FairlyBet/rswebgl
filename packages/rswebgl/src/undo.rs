//! Cleanup for constructors that create several GPU objects: each object
//! is registered as it is made, and if the constructor returns early (an
//! error part way — a lost context, a full pool) the guard frees what was
//! made, newest first. `keep` disarms it once everything is in place.
//!
//! ```ignore
//! let mut undo = Undo::new(ctx);
//! let t = ctx.create_texture(..)?;
//! undo.push(move |ctx| ctx.destroy_texture(t));
//! let fb = ctx.create_framebuffer(w, h)?; // an error here frees `t`
//! undo.keep();
//! ```

use crate::context::Context;

type Action = Box<dyn FnOnce(&Context)>;

pub struct Undo {
    ctx: Context,
    actions: Vec<Action>,
}

impl Undo {
    pub fn new(ctx: &Context) -> Self {
        Self {
            ctx: ctx.clone(),
            actions: Vec::new(),
        }
    }

    /// Registers how to free something just made.
    pub fn push(&mut self, f: impl FnOnce(&Context) + 'static) {
        self.actions.push(Box::new(f));
    }

    /// Everything made is kept: nothing is freed.
    pub fn keep(mut self) {
        self.actions.clear();
    }
}

impl Drop for Undo {
    fn drop(&mut self) {
        while let Some(f) = self.actions.pop() {
            f(&self.ctx);
        }
    }
}
