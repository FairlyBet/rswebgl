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
| Sampler                     | ❌     | —                     |
| Framebuffer                 | ❌     | —                     |
| Renderbuffer                | ❌     | —                     |
| Renderbuffer multisample    | ❌     | —                     |
| Transform Feedback          | ❌     | —                     |
| Query                       | ❌     | —                     |
| Sync                        | ❌     | —                     |

## Uniforms

| Entity                      | Status | File                    |
|-----------------------------|--------|-------------------------|
| Scalar (float, int, uint)   | ✅     | `src/uniforms.rs`       |
| Vector (vec2–4, ivec, uvec) | ✅     | `src/uniforms.rs`       |
| Matrix (mat2–4, non-square) | ✅     | `src/uniforms.rs`       |
| Rust trait `UniformValue`   | ✅     | `src/uniform_value.rs`  |
| Uniform location cache      | ✅     | `src/uniform_cache.rs`  |
| Uniform Buffer Object (UBO) | ❌     | —                       |

## Vertex Attributes

| Entity                           | Status | File                  |
|----------------------------------|--------|-----------------------|
| vertexAttribPointer              | ✅     | `src/vao.rs`          |
| vertexAttribIPointer (integer)   | ✅     | `src/vao.rs`          |
| vertexAttribDivisor (instancing) | ✅     | `src/vao.rs`          |

## Pipeline State

| Entity          | Status | File                  |
|-----------------|--------|-----------------------|
| Viewport        | ✅     | `src/draw.rs`         |
| Blending        | ✅     | `src/pipeline.rs`     |
| Depth test      | ✅     | `src/pipeline.rs`     |
| Stencil test    | ✅     | `src/pipeline.rs`     |
| Face culling    | ✅     | `src/pipeline.rs`     |
| Scissor test    | ✅     | `src/pipeline.rs`     |
| Color mask      | ✅     | `src/pipeline.rs`     |
| Polygon offset  | ✅     | `src/pipeline.rs`     |

## Draw Calls

| Entity                   | Status | File                  |
|--------------------------|--------|-----------------------|
| drawArrays               | ✅     | `src/draw.rs`         |
| drawElements             | ✅     | `src/draw.rs`         |
| drawArraysInstanced      | ✅     | `src/draw.rs`         |
| drawElementsInstanced    | ✅     | `src/draw.rs`         |
| drawRangeElements        | ✅     | `src/draw.rs`         |

## Framebuffer Operations

| Entity                      | Status | File |
|-----------------------------|--------|------|
| readPixels                  | ❌     | —    |
| readBuffer                  | ❌     | —    |
| blitFramebuffer             | ❌     | —    |
| invalidateFramebuffer       | ❌     | —    |
| invalidateSubFramebuffer    | ❌     | —    |
| drawBuffers (MRT)           | ❌     | —    |

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

## Clear

| Entity                      | Status | File |
|-----------------------------|--------|------|
| clear / clearColor          | ❌     | —    |
| clearDepth / clearStencil   | ❌     | —    |
| clearBuffer (fv/iv/uiv/fi)  | ❌     | —    |

## Program Introspection

| Entity                      | Status | File |
|-----------------------------|--------|------|
| getActiveAttrib             | ❌     | —    |
| getActiveUniform            | ❌     | —    |
| getAttribLocation           | ❌     | —    |
| getUniformBlockIndex        | ❌     | —    |
| uniformBlockBinding         | ❌     | —    |

## Misc

| Entity                 | Status | File                  |
|------------------------|--------|-----------------------|
| Context                | ✅     | `src/context.rs`      |
| Extension registry     | ✅     | `src/extension.rs`    |
| KHR_parallel_compile   | ✅     | `src/program.rs`      |
| RefCount (GL lifecycle) | ✅     | `src/ref_count.rs`    |
