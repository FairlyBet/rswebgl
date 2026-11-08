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
- **UV sets** — TEXCOORD_0 and _1 are read (locations 2 and 5); _2+ aren't.
  `KHR_texture_transform` isn't applied. Extension textures (clearcoat, sheen,
  transmission, …) aren't read yet (E5c).
- **KTX2/Basis, Draco, meshopt** compression — needs decoders; not wired.
- **Sparse accessors** — not handled (rare).
- **Materials → shaders** live in `rswebgl-scene` (`PbrShaders`, E5a): the
  loader fills material groups (factors + all five maps with their samplers);
  the scene picks a shader variant per primitive and material. Missing:
  MikkTSpace tangents for normal-mapped primitives without TANGENT (a
  derivative-based frame stands in), skinning / morph targets.
- **Texture sRGB role** decided by base-color/emissive usage scan; a texture
  shared across linear+sRGB roles would pick one. Acceptable in practice.
- **ORM channel packing** (`src/pack.rs` + `texture_packer`) combines a *separate*
  occlusion + metallic-roughness pair into one `R=AO,G=rough,B=metal` texture
  (sources loaded transiently, freed after the GPU pack — only the packed texture
  is kept). Conservative: only when each source is used exclusively in its role
  and the occ↔MR pairing is 1:1. The transient sources are forced to NEAREST
  before packing (safe — they're dropped right after), so a same-size pack is an
  exact texel copy. The PBR shader samples both roles from it (DamagedHelmet in
  `examples/js-scene`). Possible refinement: a metric/log of bytes saved vs.
  textures kept.

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
- Many copies of one mesh → one instanced draw instead of a draw per entity.
- Transparent draws sort per object (bounds center), not per triangle:
  intersecting or concave blended meshes can still show order errors.
- Front-face winding of a glTF part is fixed at load from its world matrix;
  a part mirrored later by a negative scale draws inside out.
- A model added twice needs two loads (`addModel` takes the `Model`); sharing
  one model's GPU data between several roots would need reference counts.
- JS reads: `copyWorldMatrix` fills a buffer of the caller's; the rest
  (`translation`, `pickPoint`, …) return a new array. Zero-copy views over
  component storage when a viewer needs bulk reads.
- PBR (E5a): lights are capped at 8 per frame (all lights reach every draw;
  no per-object light lists); spot lights untested by pixels; no shadows.
  `KHR_lights_punctual` lights in a glTF file aren't spawned yet.
- IBL (E5b): the bake is synchronous (a few dozen draws at load: a stall of
  a frame or two on mobile); no progressive / async bake. `.hdr` only — no
  EXR, no KTX2 prebaked cubes. One environment per scene. The specular cube
  is also the background, so a sharp background is only as sharp as its
  `size`. A device without half-float render targets gets an error, no
  fallback (e.g. RGBM in RGBA8).
- Material extensions (E5c): factors only — the extensions' textures
  (transmission, thickness, clearcoat + its roughness and normal, specular
  and specular color) aren't read. Transmission refracts the environment
  only: objects behind glass or a gem don't show through it (needs an
  opaque pass into a mipmapped texture, sampled by the transmissive pass —
  Khronos' approach). One refraction (entry) for volumes; no internal
  reflections, which is what makes cut diamonds sparkle — a dedicated gem
  shader (ray-marching the stone's own geometry) is the jewelry-grade
  follow-up. Punctual lights don't transmit. Not read: sheen, iridescence,
  anisotropy, diffuse transmission, KHR_materials_variants.
- Picking keeps a CPU copy of every glTF triangle primitive's positions and
  indices (12 B per vertex + 4 B per index): a loader option to skip it for
  models that are never picked. Picking tests every triangle of the
  meshes whose box the ray hits (no BVH); fine for jewelry-sized models.
  Skinned or morphed meshes would need their deformed positions.
- Materials: `setMaterialTexture` sets the five core maps; the extension
  maps (clearcoat, transmission, …) aren't settable (nor read from glTF).
  The color space of a texture set on a map isn't checked against the role
  (sRGB for colors). `cloneMaterial` shares the source model's textures, so
  a clone outlives its model only as long as nothing samples them.
- Options objects are read with `Reflect` per call (setup-time API; not for
  per-frame use).

## From the async / lifetime review (deferred)

Fixed in the review commits; these were found and left for later:

- `ProgramObj::mappings` gets an entry per parameter layout ever drawn with
  and is searched linearly per draw; never pruned. Shared PBR programs grow
  with every `addModel` / despawn cycle (each model has its own layout).
  Prune when a layout is freed, or key by layout with lazy eviction.
- Error-path leaks reachable only on a lost context or a full pool:
  `Baker::new` / `run`, `Environment::constant`, `SceneRenderer::new` after
  their first allocation; a partially built `Model` when `load_model` fails
  late. Parameter layouts / groups are CPU-only and survive a loss.
- Every pending async read / query polls on its own 1 ms timer (clamped to
  ~4 ms); many in flight → many timers. One poller per context.
- `SceneRenderer::release_removed` misses a removal when the app clears the
  world's trackers before rendering and adds one mesh as it removes another
  (the `Scene` order is right; affects Rust users driving the world).
- `texture_generate_mipmaps` doesn't check the format can have mipmaps
  generated (compressed, or float without the extension → GL error, black
  with a mipmap filter).
- `decode_data_uri` doesn't percent-decode non-base64 data URIs.
- Images decode through `HtmlImageElement`; `createImageBitmap` with
  `premultiplyAlpha: "none"` / `colorSpaceConversion: "none"` would decode
  off the main thread and avoid a premultiply round trip on low alpha.
- Timer queries: disjoint handling is now context-wide but untested by
  pixels (headless Chrome never reports a disjoint).
