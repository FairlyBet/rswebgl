//! Resource handles. Each is a generational `u32` (a plain `number` in JS) into
//! a pool owned by the [`Context`](crate::context::Context). Copying one copies a
//! number; the object lives until `destroy_*` is called on the context, after
//! which the handle is detected as stale.

use rswebgl_foundation::typed_handle;

typed_handle!(
    /// A GL buffer (vertex/index/generic data).
    pub struct BufferId
);
typed_handle!(
    /// A texture (2D, 3D, cube map or 2D array).
    pub struct TextureId
);
typed_handle!(
    /// A renderbuffer (optionally multisampled).
    pub struct RenderbufferId
);
typed_handle!(
    /// A framebuffer object. `FramebufferId::NULL` targets the canvas.
    pub struct FramebufferId
);
typed_handle!(
    /// A linked shader program.
    pub struct ProgramId
);
typed_handle!(
    /// A vertex array (attribute bindings + optional index buffer).
    pub struct VertexArrayId
);
typed_handle!(
    /// A std140 uniform block with CPU staging.
    pub struct UniformBufferId
);
typed_handle!(
    /// The fields of a parameter group: uniforms, textures, blocks by name.
    pub struct ParamLayoutId
);
typed_handle!(
    /// Values for a parameter layout, referenced by batches and draws.
    pub struct ParamGroupId
);
typed_handle!(
    /// An interned `RenderState`: equal states share one id.
    pub struct StateId
);
