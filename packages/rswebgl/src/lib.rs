//! WebGL2 wrapper: GL objects live in pools owned by a [`context::Context`] and
//! are addressed by numeric handles ([`ids`]) with explicit `destroy_*`; a
//! renderer executes passes of sortable draw lists, diffing against a GL state
//! cache kept across frames.
//!
//! Handles are generational: using one after its object was destroyed (or
//! after a context loss) is detected and reported, never undefined behavior.
//!
//! # Errors
//!
//! One rule, by what the caller can do about it:
//!
//! - **`Result<_, error::Error>`** (a JS `Error` on the JS side) when nothing
//!   usable can be produced: creating an
//!   object (`Context::create_*`, `VaoBuilder::build`, shader compile/link,
//!   out-of-limit renderbuffers), framebuffer completeness, a lost context.
//! - **`console.error` + no-op** when an operation on an existing object is
//!   rejected and nothing changed: an upload GL refused, texture storage or an
//!   attachment/attribute index beyond the implementation limits (`limits`).
//! - **`console.warn` + carry on** for input that is suspicious but has a sane
//!   recovery: an unknown uniform or UBO field, a wrong-sized uniform array,
//!   alignment padding, a uniform no draw sets.
//!
//! Anything reachable once per frame (setters, draws) reports a given problem
//! once, not every frame. Messages are prefixed `[rswebgl]`. The checks that
//! cost per-draw work can be turned off for release builds
//! (`Context::set_validate_uniforms`, `set_validate_uniform_blocks`).

pub mod buffer;
pub mod compressed_format;
pub mod console;
pub mod context;
pub(crate) mod context_loss;
pub mod copy;
pub mod depth_range;
pub mod draw;
pub mod error;
pub mod extension;
pub mod feedback;
pub mod float_targets;
pub mod framebuffer;
pub mod ids;
pub mod limits;
pub mod param;
pub mod pass;
pub mod pixel_unpack;
pub mod program;
pub mod query;
pub mod readback;
pub mod render_state;
pub mod renderbuffer;
pub mod renderer;
pub mod sampler;
pub mod texture;
pub mod texture_packer;
pub mod uniform_buffer;
pub mod vao;
pub mod vao_builder;
