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
| Vertex Array Object (VAO)   | ❌     | —                     |
| Texture 2D                  | ✅     | `src/texture.rs`      |
| Texture 3D                  | 🔶     | `src/texture.rs`      |
| Texture Cube Map            | ✅     | `src/texture.rs`      |
| Texture 2D Array            | 🔶     | `src/texture.rs`      |
| Sampler                     | ❌     | —                     |
| Framebuffer                 | ❌     | —                     |
| Renderbuffer                | ❌     | —                     |
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

| Entity                           | Status |
|----------------------------------|--------|
| vertexAttribPointer              | ❌     |
| vertexAttribIPointer (integer)   | ❌     |
| vertexAttribDivisor (instancing) | ❌     |

## Pipeline State

| Entity          | Status |
|-----------------|--------|
| Viewport        | ❌     |
| Blending        | ❌     |
| Depth test      | ❌     |
| Stencil test    | ❌     |
| Face culling    | ❌     |
| Scissor test    | ❌     |
| Color mask      | ❌     |
| Polygon offset  | ❌     |

## Draw Calls

| Entity                   | Status |
|--------------------------|--------|
| drawArrays               | ❌     |
| drawElements             | ❌     |
| drawArraysInstanced      | ❌     |
| drawElementsInstanced    | ❌     |
| drawRangeElements        | ❌     |

## Misc

| Entity                 | Status | File                  |
|------------------------|--------|-----------------------|
| Context                | ✅     | `src/context.rs`      |
| Extension registry     | ✅     | `src/extension.rs`    |
| KHR_parallel_compile   | ✅     | `src/program.rs`      |
| Pixel pack / unpack    | ❌     | —                     |
| Blit framebuffer       | ❌     | —                     |
| Invalidate framebuffer | ❌     | —                     |
