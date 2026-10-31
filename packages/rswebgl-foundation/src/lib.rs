//! The data layer shared by the rswebgl renderer and (later) its ECS.
//!
//! - [`handle`] / [`pool`]: generational `u32` handles and the slot pools behind
//!   them. A stale handle never resolves to a newer object.
//! - [`layout`]: named fields laid out per std140 (uniform blocks) or packed
//!   (vertex/instance data, ECS components); names resolve to [`layout::FieldId`]s
//!   once.
//! - [`dirty`]: which chunks of a buffer changed, coalesced into upload runs.
//! - [`column`]: dense typed arrays in wasm memory that JS can view directly.
//!
//! Nothing here touches WebGL.

pub mod column;
pub mod dirty;
pub mod handle;
pub mod layout;
pub mod pool;

// For `typed_handle!` expansions in other crates.
#[doc(hidden)]
pub use wasm_bindgen as __wasm_bindgen;
