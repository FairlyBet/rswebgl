# TODO

Deferred work / known weak spots. Not committed — scratch backlog.

## Errors and GL limits

Done: the rule is documented at the top of `src/lib.rs` (`Result` / `error` +
no-op / `warn`, per-frame problems reported once), and `src/limits.rs` checks
vertex attribs, texture sizes / layers / mip levels, color attachments + draw
buffers, renderbuffer size and samples. Left:

- `MAX_SAMPLES` is the overall cap; a format can allow fewer
  (`getInternalformatParameter(SAMPLES)`), which still surfaces as a GL error.
- Texture *sub-image* regions aren't checked against the allocated level size;
  GL rejects them and we log its exception.
- Limits come from the first context and are process-wide; fine unless one
  page mixes very different GPUs.

## Uniform buffers (UBO)

- ~~Dirty tracking by 16-byte chunks.~~ Done: `UniformBuffer` uses the
  foundation's `DirtyBits` and `Layout`; scattered writes upload as separate
  runs. Still to check on real hardware: whether many small `bufferSubData`
  runs beat one larger one on mobile drivers (may want a gap threshold that
  merges runs separated by only a few chunks).
- Layout validation is size-only (`UNIFORM_BLOCK_DATA_SIZE` vs our std140 size).
  Catches most drift (field count/types/array counts shift the total) but not
  same-size permutations or per-field offset errors. Per-field offset validation
  needs name-mapping against `getActiveUniform` (block-prefix + array `[0]` +
  array-stride pitfalls) — deferred.
- `bindBufferRange` (sub-allocation: several blocks in one buffer) is wired
  (`set_uniform_block_range`) but unexercised; no example yet.

## Extensions worth first-class integration

The registry (`src/extension.rs`) knows every extension *by name* — but that's
just `getExtension`; the user has to remember what to enable and why. Only
`KHR_parallel_shader_compile` is wired into logic (Program's `parallel` flag →
`COMPLETION_STATUS_KHR` in `is_ready`). These deserve the same first-class
treatment instead of "enable it yourself":

### Float render targets — done (`src/float_targets.rs`)

Enabled at context creation, checked when a float target is created or
attached. Left:

- A 32F texture filtered linearly *through a sampler object* isn't checked
  (only the texture's own filters, at storage time).
- Blending into a 32F target without `EXT_float_blend` is a GL error at draw
  time; not checked against the render state.
- RGB16F / RGB32F are left to GL's completeness check (not renderable in core,
  optional with the half-float extension).

### Useful — maps onto subsystems we already have / have queued

- ~~**`EXT_disjoint_timer_query_webgl2`.**~~ Done: `QueryKind::TimeElapsed` on
  `Pass::set_timer`. Left: a ready-made per-pass profiler (a ring of timer
  queries, results a few frames behind) instead of the app juggling them; a
  timer spanning several passes; `TIMESTAMP_EXT` isn't exposed by WebGL2.
- Occlusion queries wrap one draw (`Pass::set_last_draw_query`). Not done: a
  query around several draws (a whole object of many primitives).
- **`WEBGL_multi_draw`.** Several draws in one JS call (offset/count arrays).
  Real perf win for many-object scenes; fits `draw.rs`/`DrawCommand` as a
  `multi_draw_*` variant.
- **`OES_draw_buffers_indexed`.** Per-attachment blend state (own blend
  func/equation + color mask per MRT target). Pairs with the finished MRT
  (`drawBuffers`) and `RenderState`; without it blending is shared across all
  attachments.

### Cheap & pleasant — a method or two

- **`WEBGL_debug_renderer_info`.** `UNMASKED_VENDOR_WEBGL` /
  `UNMASKED_RENDERER_WEBGL`. One method `ctx.gpu_info() -> String` (vendor +
  renderer). Good for bug reports and "is this Mali/Adreno → enable fallback"
  heuristics. Trivial. *Good second pick after float targets.*
- ~~**`WEBGL_lose_context`.**~~ Done: `Context::simulate_context_loss` /
  `simulate_context_restore`, used by `tests/context_loss.rs` and the cube
  example (press L).

## Context loss

Handled (`src/context_loss.rs`): a loss clears the resource pools (every handle
goes stale, nothing tries to delete dead objects), the renderer idles while
lost, extensions are re-enabled on restore, and `on_lost` / `on_restored(ctx)`
are called. Stale-handle reports are muted while a context is lost. Recreating
resources is the app's job (see the cube example). Possible follow-ups:

- **Automatic restore** of objects the library itself owns (e.g. a
  `TexturePacker` has to be recreated after a restore).
- glTF `Model` has no "reload" path; an app has to call `load_model` again.
- No JS example of `set_on_restored` yet.

### Niche — note for completeness, not soon

- **`EXT_clip_control`.** Zero-to-one depth + reverse-Z (like D3D/Vulkan);
  strongly mitigates z-fighting at distance. Powerful but the user must know what
  they're doing.
- **`EXT_depth_clamp`.** Disable near/far clipping (shadow volumes).
- **`EXT_float_blend`.** Blending into 32F targets (16F already blends).

## Transform feedback (`src/feedback.rs`)

- At most 4 bindings (the guaranteed `MAX_TRANSFORM_FEEDBACK_SEPARATE_ATTRIBS`);
  the real limit isn't queried.
- One program per recording. Several would need pause / resume around each
  program switch.
- A draw that reads the buffer it records into is a GL error, not checked.
- No JS example yet (GPU particles would be the natural one).

## glTF loader (`packages/rswebgl-gltf`)

v1 translates static meshes + PBR metallic-roughness (factors + base-color
texture) into VAOs/textures/draw-list with spec-correct render state. Deferred:

- **Skinning (JOINTS_n/WEIGHTS_n) & morph targets**, and **animation** (samplers/
  channels). The biggest gap toward "real" glTF.
- **JS entry point.** `load_model` is Rust-only async — an exported async fn
  can't hold the `&Context` borrow across `await`. Add a JS-facing wrapper (e.g.
  context by value / a handle) so JS can call it.
- **Normalized-integer attrs** kept as-is: everything is dequantized to `f32` and
  indices widened to `u32` on upload. Fine for v1; revisit for memory.
- **Multi-UV** (TEXCOORD_1+) — only set 0 is read; `base_color_uv` is exposed but
  the example assumes UV0. Non-PBR/extension textures (clearcoat, sheen, …) and
  the metallic-roughness/normal/occlusion/emissive maps are parsed as indices but
  the example only samples base color.
- **KTX2/Basis, Draco, meshopt** compression — needs decoders; not wired.
- **Sparse accessors** — not handled (rare).
- **Material → shader/scene system.** The translator emits data + a documented
  uniform convention (`write_uniforms`); a real material/scene/lighting system is
  future work. Back-face normal flip currently lives in the example shader.
  A reference PBR shader pair (`shaders/pbr/main.{vert,frag}`) already declares
  the full spec data interface (all attributes, `Frame`/`Object`/`Material`/
  `Skin`/`Lights` UBOs, the five PBR maps, alpha modes, MRT targets) behind
  feature `#define`s — but its BRDF is a placeholder (`// TODO(pbr)`), and the
  translator doesn't yet build these UBOs / select shader permutations / fill in
  the Cook-Torrance + IBL lighting. That permutation+lighting layer is the work.
- **Texture sRGB role** decided by base-color/emissive usage scan; a texture
  shared across linear+sRGB roles would pick one. Acceptable in practice.
- **ORM channel packing** (`src/pack.rs` + `texture_packer`) combines a *separate*
  occlusion + metallic-roughness pair into one `R=AO,G=rough,B=metal` texture
  (sources loaded transiently, freed after the GPU pack — only the packed texture
  is kept). Conservative: only when each source is used exclusively in its role
  and the occ↔MR pairing is 1:1. The transient sources are forced to NEAREST
  before packing (safe — they're dropped right after), so a same-size pack is an
  exact texel copy. Not yet exercised end-to-end — the example shader samples only
  base color, so there's no visible consumer until the reference PBR shader is
  wired in. Possible refinement: a metric/log of bytes saved vs. textures kept.

## Texture packer (`packages/rswebgl/src/texture_packer.rs`)

General GPU channel packer (up to 4 single-channel sources → RGBA via a fullscreen
FBO blit). Done. Deferred polish:
- `pack`/`pack_into` always output `RGBA8`/`SRGB8_ALPHA8`; no `RG8`/`R8` outputs
  (would shave memory when fewer than 3 channels are used). Revisit with the
  RGB/single-channel format question already noted for the glTF loader.

## Minor / nice-to-have (from the audit)

- Parameter groups: a group unchanged since it was last applied in a slot could be
  skipped wholesale instead of compared field by field; needs care when two
  slots' layouts overlap. Also: zero-copy JS views over a group's words
  (foundation `Column`-style), sampler arrays, and retaining textures / UBOs
  referenced by groups (today a destroyed one is reported at render).
- Texture units / block points are assigned per program from 0, so programs
  that share a block name ("Frame") may put it on different points and rebind
  more than needed. A context-wide name→point registry would let them share.

- Draw list: the default sort key truncates handle indices (program 15 bits,
  state 12, material 20, VAO 16), so very large scenes group slightly worse —
  never wrong. No depth-sort helper for transparent draws yet (`KEY_LAST` +
  your own depth bits). `ctx.render_state` interns by linear search: fine for
  tens of states, not thousands.
- The GL state cache trusts that only the library binds. Raw GL on the same
  context needs `ctx.invalidate_state()` afterwards.
- Pass resolve copies color attachment 0 only; MRT or depth resolves go
  through `blit_framebuffer` by hand. `invalidateSubFramebuffer` and
  `copyTexSubImage3D` (into a layer of an array / 3D texture) aren't done.
- Async reads poll their fence on a 1 ms timer. Polling once per frame from
  `render` would save timer wake-ups if many reads are in flight.

## JS API

- Handles are bare `number`s in the `.d.ts`; branded types
  (`type TextureId = number & { __brand: "TextureId" }`) would catch mixing
  them up, via a `typescript_custom_section` or a post-processing step.
- Parameter names in the `.d.ts` stay snake_case (`index_type`); cosmetic.
- Builders consume themselves (`VaoBuilder.build`, `ParamLayoutBuilder`
  chaining): in JS the object is freed by the call. Documented, but a
  `&mut self` builder would be less surprising from JS.
- No npm package / bundler setup yet; `examples/js-cube` builds with
  `wasm-pack --target web`.
- Description objects via serde (`ctx.renderState({ depthTest: true })`) as
  planned in the design doc — not needed yet.

## glTF

- `EXT_mesh_gpu_instancing` instances come out as separate draw items; one
  `drawElementsInstanced` with a per-instance matrix attribute needs renderer
  support (with the ECS work). Quantized instance attributes (normalized
  rotation) aren't read — such nodes are drawn once, with a warning.
- Vertex attributes are dequantized to f32 on upload; keeping normalized
  u8/u16 (`KHR_mesh_quantization`) would cut memory on mobile.
- Not handled yet: skins, morph targets, animations, cameras, lights — scene
  concerns, for the ECS.

## Scene (`packages/rswebgl-scene`)

- World-space AABBs are recomputed for culling every frame; cache them per
  `WorldMatrix` change if culling shows up in profiles (thousands of objects).
- Transparency: a `MeshRenderer` flag → `KEY_LAST` + back-to-front depth in
  the sort key (needed for the jewelry viewer's gems, with E5 materials).
- `Visible(false)` isn't inherited; a hidden subtree needs every node hidden.
- Many copies of one mesh → one instanced draw instead of a draw per entity.
- The frame block must be declared with all four members (std140 size has to
  match); consider reflecting the block and binding a range instead.
