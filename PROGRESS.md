# WebGL2 Coverage

- ✅ done
- 🔶 partial
- ❌ not implemented

---

## Objects

| Entity                      | Status | File                  |
|-----------------------------|--------|-----------------------|
| Buffer                      | ✅     | `src/buffer.rs`       |
| Shader (vertex / fragment)  | ✅     | `src/program.rs`      |
| Program                     | ✅     | `src/program.rs`      |
| Vertex Array Object (VAO)   | ✅     | `src/vao.rs`          |
| Texture 2D                  | ✅     | `src/texture.rs`      |
| Texture 3D                  | 🔶     | `src/texture.rs`      |
| Texture Cube Map            | ✅     | `src/texture.rs`      |
| Texture 2D Array            | 🔶     | `src/texture.rs`      |
| Framebuffer                 | ✅     | `src/framebuffer.rs`  |
| Renderbuffer                | ✅     | `src/renderbuffer.rs` |
| Renderbuffer multisample    | ✅     | `src/renderbuffer.rs` |
| Sampler object              | ❌     | —                     |
| Transform Feedback          | ❌     | —                     |
| Query                       | ❌     | —                     |
| Sync                        | ❌     | —                     |

## Rendering

| Entity                          | Status | File                                |
|---------------------------------|--------|-------------------------------------|
| Pass / Batch / Draw API         | ✅     | `src/pass.rs`, `src/renderer.rs`    |
| Per-pass state tracking (diff)  | ✅     | `src/renderer.rs`                   |
| Render-state diff               | ✅     | `src/render_state.rs`               |
| Uniform diff (in-place)         | ✅     | `src/uniform_values.rs`             |

## Uniforms

| Entity                      | Status | File                    |
|-----------------------------|--------|-------------------------|
| Scalar (float, int, uint)   | ✅     | `src/uniforms.rs`       |
| Vector (vec2–4, ivec, uvec) | ✅     | `src/uniforms.rs`       |
| Matrix (mat2–4, non-square) | ✅     | `src/uniforms.rs`       |
| Sampler binding             | ✅     | `src/uniform_values.rs` |
| Rust trait `UniformValue`   | ✅     | `src/uniform_value.rs`  |
| Uniform location cache      | ✅     | `src/uniform_cache.rs`  |
| Uniform Buffer Object (UBO) | ❌     | —                       |

## Vertex Attributes

| Entity                           | Status | File                  |
|----------------------------------|--------|-----------------------|
| vertexAttribPointer              | ✅     | `src/vao.rs`          |
| vertexAttribIPointer (integer)   | ✅     | `src/vao.rs`          |
| vertexAttribDivisor (instancing) | ✅     | `src/vao.rs`          |

## Render State

| Entity          | Status | File                  |
|-----------------|--------|-----------------------|
| Viewport        | ✅     | `src/draw.rs`         |
| Blending        | ✅     | `src/render_state.rs` |
| Depth test      | ✅     | `src/render_state.rs` |
| Stencil test    | ✅     | `src/render_state.rs` |
| Face culling    | ✅     | `src/render_state.rs` |
| Scissor test    | ✅     | `src/render_state.rs` |
| Color mask      | ✅     | `src/render_state.rs` |
| Polygon offset  | ✅     | `src/render_state.rs` |

## Draw Calls

| Entity                   | Status | File                  |
|--------------------------|--------|-----------------------|
| drawArrays               | ✅     | `src/draw.rs`         |
| drawElements             | ✅     | `src/draw.rs`         |
| drawArraysInstanced      | ✅     | `src/draw.rs`         |
| drawElementsInstanced    | ✅     | `src/draw.rs`         |
| drawRangeElements        | ✅     | `src/draw.rs`         |

## Framebuffer Operations

| Entity                      | Status | File                  |
|-----------------------------|--------|-----------------------|
| readPixels                  | ❌     | —                     |
| readBuffer                  | ❌     | —                     |
| blitFramebuffer             | ❌     | —                     |
| invalidateFramebuffer       | ✅     | `src/renderer.rs`     |
| invalidateSubFramebuffer    | ❌     | —                     |
| drawBuffers (MRT)           | ✅     | `src/framebuffer.rs`  |

## Buffer Operations

| Entity                      | Status | File |
|-----------------------------|--------|------|
| copyBufferSubData           | ❌     | —    |
| getBufferSubData            | ❌     | —    |

## Texture Operations

| Entity                      | Status | File |
|-----------------------------|--------|------|
| texSubImage2D               | ❌     | —    |
| texSubImage3D               | ❌     | —    |
| copyTexImage2D              | ❌     | —    |
| copyTexSubImage2D           | ❌     | —    |
| copyTexSubImage3D           | ❌     | —    |
| texStorage2D                | ❌     | —    |
| texStorage3D                | ❌     | —    |
| compressedTexImage2D        | ❌     | —    |
| compressedTexImage3D        | ❌     | —    |
| Texture LOD params          | ❌     | —    |
| Pixel pack / unpack params  | ❌     | —    |
| DOM-source uploads          | ❌     | —    |

## Clear

| Entity                      | Status | File                  |
|-----------------------------|--------|-----------------------|
| clear / clearColor          | ✅     | `src/renderer.rs`     |
| clearDepth / clearStencil   | ✅     | `src/framebuffer.rs`  |
| clearBuffer (fv/iv/uiv/fi)  | ✅     | `src/framebuffer.rs`  |

## Program Introspection

| Entity                      | Status | File |
|-----------------------------|--------|------|
| getActiveAttrib             | ❌     | —    |
| getActiveUniform            | ❌     | —    |
| getAttribLocation           | ❌     | —    |
| getUniformBlockIndex        | ❌     | —    |
| uniformBlockBinding         | ❌     | —    |

## Misc

| Entity                                  | Status | File                  |
|-----------------------------------------|--------|-----------------------|
| Context                                 | ✅     | `src/context.rs`      |
| Context creation options                | ✅     | `src/context.rs`      |
| Color space (drawingBufferColorSpace)   | ✅     | `src/framebuffer.rs`  |
| Default FB auto-resize (ResizeObserver) | ✅     | `src/framebuffer.rs`  |
| Extension registry                      | ✅     | `src/extension.rs`    |
| KHR_parallel_compile                    | ✅     | `src/program.rs`      |
| RefCount (GL lifecycle)                 | ✅     | `src/ref_count.rs`    |
