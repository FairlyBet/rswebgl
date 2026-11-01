# WebGL2 Coverage

- ✅ done
- 🔶 partial
- ❌ not implemented
- ➖ not needed

---

## Objects

| Entity                      | Status | File                  |
|-----------------------------|--------|-----------------------|
| Buffer                      | ✅     | `src/buffer.rs`       |
| Shader (vertex / fragment)  | ✅     | `src/program.rs`      |
| Program                     | ✅     | `src/program.rs`      |
| Vertex Array Object (VAO)   | ✅     | `src/vao.rs`          |
| Texture 2D                  | ✅     | `src/texture.rs`      |
| Texture 3D                  | ✅     | `src/texture.rs`      |
| Texture Cube Map            | ✅     | `src/texture.rs`      |
| Texture 2D Array            | ✅     | `src/texture.rs`      |
| Framebuffer                 | ✅     | `src/framebuffer.rs`  |
| Renderbuffer                | ✅     | `src/renderbuffer.rs` |
| Renderbuffer multisample    | ✅     | `src/renderbuffer.rs` |
| Sampler object              | ✅     | `src/sampler.rs`      |
| Transform Feedback          | ✅     | `src/feedback.rs`     |
| Query (occlusion per draw, GPU timer per pass) | ✅ | `src/query.rs` |
| Sync                        | 🔶     | `src/readback.rs` (fences for async reads; not exposed) |

## Rendering

| Entity                          | Status | File                                |
|---------------------------------|--------|-------------------------------------|
| Pass with flat draw list, sort keys (Batch as builder) | ✅ | `src/pass.rs` |
| GL state cache kept across frames (+ `invalidate_state`) | ✅ | `src/renderer.rs` |
| Interned render states (`StateId`) | ✅ | `src/render_state.rs` |
| Frame stats (draws, binds, uploads) | ✅ | `src/renderer.rs` |
| JS API: camelCase, by-reference args, `Error` objects | ✅ | `examples/js-cube` |
| Render-state diff               | ✅     | `src/render_state.rs`               |
| Uniform diff (shadow compare)   | ✅     | `src/program.rs`, `src/renderer.rs` |

## Uniforms

| Entity                      | Status | File                    |
|-----------------------------|--------|-------------------------|
| Program reflection (types, locations, units, block points) | ✅ | `src/program.rs` |
| Parameter layouts + groups, name → field index once | ✅ | `src/param.rs` |
| Scalar / vector / matrix / array params | ✅ | `src/param.rs`       |
| Sampler + uniform-block params | ✅     | `src/param.rs`         |
| Group slots on Batch / Draw  | ✅     | `src/pass.rs`           |
| Uncovered-input check (debug) | ✅    | `src/program.rs`        |
| Uniform Buffer Object (UBO) | ✅     | `src/uniform_buffer.rs` |

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
| readPixels (sync + async via PBO + fence) | ✅ | `src/readback.rs` |
| readBuffer                  | ✅     | `src/readback.rs` (per read) |
| blitFramebuffer (+ pass MSAA resolve) | ✅ | `src/copy.rs`, `src/renderer.rs` |
| invalidateFramebuffer       | ✅     | `src/renderer.rs`     |
| invalidateSubFramebuffer    | ❌     | —                     |
| drawBuffers (MRT)           | ✅     | `src/framebuffer.rs`  |

## Buffer Operations

| Entity                      | Status | File |
|-----------------------------|--------|------|
| copyBufferSubData           | ✅     | `src/copy.rs` (not index buffers: WebGL forbids) |
| getBufferSubData (sync + async) | ✅ | `src/readback.rs` (`buffer_read*`) |

## Texture Operations

| Entity                      | Status | File                       |
|-----------------------------|--------|----------------------------|
| texStorage2D / 3D           | ✅     | `src/texture.rs`           |
| texSubImage2D               | ✅     | `src/texture.rs`           |
| texSubImage3D               | ✅     | `src/texture.rs`           |
| compressedTexImage2D / 3D   | ✅     | `src/texture.rs` (storage+sub) |
| compressedTexSubImage2D/3D  | ✅     | `src/texture.rs`           |
| Compressed format registry  | ✅     | `src/compressed_format.rs` |
| Texture LOD / compare params| ✅     | `src/texture.rs`           |
| Pixel unpack params         | ✅     | `src/pixel_unpack.rs`      |
| DOM-source uploads          | ✅     | `src/texture.rs`           |
| `load_image` (onload upload)| ✅     | `src/texture.rs`           |
| GPU channel packing (ORM)   | ✅     | `src/texture_packer.rs`    |
| copyTexImage2D              | ➖     | storage-first: use copyTexSubImage2D |
| copyTexSubImage2D           | ✅     | `src/copy.rs`              |
| copyTexSubImage3D           | ❌     | —                          |
| Pixel pack params           | ➖     | not needed: reads use 4-byte components |

## Clear

| Entity                      | Status | File                  |
|-----------------------------|--------|-----------------------|
| clear / clearColor          | ✅     | `src/renderer.rs`     |
| clearDepth / clearStencil   | ✅     | `src/framebuffer.rs`  |
| clearBuffer (fv/iv/uiv/fi)  | ✅     | `src/framebuffer.rs`  |

## Program Introspection

| Entity                      | Status | File |
|-----------------------------|--------|------|
| getActiveAttrib             | ❌     | —                |
| getActiveUniform            | ✅     | `src/program.rs` (reflection) |
| getAttribLocation           | ❌     | —                |
| getUniformBlockIndex        | ✅     | `src/program.rs` |
| uniformBlockBinding         | ✅     | `src/program.rs` |

## Misc

| Entity                                  | Status | File                  |
|-----------------------------------------|--------|-----------------------|
| Context                                 | ✅     | `src/context.rs`      |
| Context creation options                | ✅     | `src/context.rs`      |
| Color space (drawingBufferColorSpace)   | ✅     | `src/framebuffer.rs`  |
| Default FB auto-resize (ResizeObserver) | ✅     | `src/framebuffer.rs`  |
| Extension registry                      | ✅     | `src/extension.rs`    |
| Float render targets (auto-enabled extensions, up-front checks) | ✅ | `src/float_targets.rs` |
| Context loss / restore                  | ✅     | `src/context_loss.rs` |
| KHR_parallel_compile                    | ✅     | `src/program.rs`      |
| Resource pools + numeric handles, explicit destroy, deferred destroy while in use | ✅ | `src/context.rs`, `src/ids.rs` |

## Asset loading

| Entity                      | Status | File                       |
|-----------------------------|--------|----------------------------|
| glTF 2.0 / GLB → primitives | ✅     | `packages/rswebgl-gltf`    |

Separate crate translating glTF/GLB (geometry → VAOs, materials → uniforms +
render state, textures, node transforms → draw list) using only rswebgl's public
API. Async load (fetches external buffers/images; textures fill in via the DOM
image decoder). v1 covers static meshes + PBR metallic-roughness factors/base
color; deferred items (skinning, animation, KTX2/Draco, …) are in `TODO.md`.
Separate occlusion + metallic-roughness maps are GPU-packed into one ORM texture
(`src/pack.rs` + `texture_packer`); a reference PBR shader interface lives in
`packages/rswebgl-gltf/shaders/pbr/`.

## Foundation (`packages/rswebgl-foundation`)

Stage 1 of `docs/design/architecture.md`: the data layer shared by the renderer
and the future ECS. Nothing in it touches WebGL.

| Entity                                   | Status | File               |
|------------------------------------------|--------|--------------------|
| Generational `u32` handles, typed ids    | ✅     | `src/handle.rs`    |
| Pool: FIFO reuse, slot retirement, deferred destroy (use counts) | ✅ | `src/pool.rs` |
| Layout: std140 + packed, path → FieldId  | ✅     | `src/layout.rs`    |
| Dirty tracking by chunk, coalesced runs  | ✅     | `src/dirty.rs`     |
| Columns with zero-copy JS views + version counters | ✅ | `src/column.rs` |

The renderer's `UniformBuffer` already runs on `Layout` + `DirtyBits`; pools and
handles get adopted in stage 2.

## ECS and scene (`packages/rswebgl-ecs`, `packages/rswebgl-scene`)

Design: `docs/design/ecs.md`. ECS and hierarchy: host tests; `SceneRenderer`: browser tests. Example: `examples/scene`.

| Entity                                   | Status | File                         |
|------------------------------------------|--------|------------------------------|
| World, generational entities             | ✅     | `rswebgl-ecs/src/world.rs`   |
| Sparse-set storage, change ticks, removal log | ✅ | `rswebgl-ecs/src/storage.rs` |
| Transform, Parent, WorldMatrix           | ✅     | `rswebgl-scene/src/transform.rs` |
| Hierarchy: cached order, dirty subtrees, `set_parent`, `despawn_recursive` | ✅ | `rswebgl-scene/src/hierarchy.rs` |
| Camera, Bounds, frustum culling          | ✅     | `rswebgl-scene/src/{camera,culling}.rs` |
| SceneRenderer: frame UBO, object groups, Pass assembly (E3) | ✅ | `rswebgl-scene/src/render.rs` |
| glTF into the scene, JS facade (E4)      | ⬜     |                              |
