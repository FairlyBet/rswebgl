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
`COMPLETION_STATUS_KHR` in `is_ready`, `program_link_pending`,
`programs_ready`; the scene enables it and leaves meshes whose shader is
still linking out of the frame instead of stalling — `Scene.whenReady`). These deserve the same first-class
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
  `Pass::set_timer`; `GpuProfiler` (a ring of timer queries per label,
  smoothed, a few frames behind; the scene's `setGpuProfiling` /
  `gpuTimings` time renderFrame's passes). Left: a timer spanning several
  passes or a blit (the transmission grab isn't timed); `TIMESTAMP_EXT`
  isn't exposed by WebGL2.
- Occlusion queries wrap one draw (`Pass::set_last_draw_query`). Not done: a
  query around several draws (a whole object of many primitives).
- **`WEBGL_multi_draw`.** Several draws in one JS call (offset/count arrays)
  sharing program, state, vertex array and uniforms. Looked at with the
  scene's instancing: little left for it there — glTF primitives each have
  their own vertex array, and draws of one array with different index
  ranges carry different materials; the mergeable case (same mesh, same
  material) is what instancing covers. Without base instance (only in the
  draft `WEBGL_draw_instanced_base_vertex_base_instance`) the draws of one
  multi-draw can't each find their instances either. Worth it for apps
  with their own big shared vertex buffers, not soon.
- ~~**`OES_draw_buffers_indexed`.**~~ Done: `RenderState::set_target_blend`
  (blend func / equation / color mask per MRT attachment), `can_blend_per_target`.
  A state with overrides resets and re-applies blending on every switch to or
  from it (cheap: a few calls per attachment); without the extension the
  overrides are dropped when the state is interned, reported.

### Cheap & pleasant — a method or two

- **`WEBGL_debug_renderer_info`.** `UNMASKED_VENDOR_WEBGL` /
  `UNMASKED_RENDERER_WEBGL`. One method `ctx.gpu_info() -> String` (vendor +
  renderer). Good for bug reports and "is this Mali/Adreno → enable fallback"
  heuristics. Trivial. *Good second pick after float targets.*
- ~~**`WEBGL_lose_context`.**~~ Done: `Context::simulate_context_loss` /
  `simulate_context_restore`, used by `tests/context_loss.rs` and the cube
  example (press L).

## TAA

- `setTaa(true)` (HDR output, instead of MSAA): Bevy's resolve ported
  (`shaders/taa.frag`) with Godot's 9-tap Catmull-Rom history, Halton
  jitter, map LOD bias −1. Static geometry: the depth reprojected through
  the camera's motion. Meshes that moved (transform, skin, morph weights):
  their own motion vectors, drawn after the scene by `src/motion.rs` into a
  velocity target (Unreal's split; nothing for a still scene). Sharpening:
  `setSharpening` (FSR1 RCAS in the display pass).
- Motion-vector caveats: custom instanced draws (`instance_count` > 1) get
  none; a mesh's first frame has no previous state. Alpha-masked cutouts
  are fine: the motion pass keeps only fragments at the scene's depth.
- Transmissive and transparent meshes: FSR2's autogenerated reactive mask
  (the opaque scene copied before they draw, compared with the final
  frame) cuts the history where they show while the pixel moves or one of
  them moved. While still they accumulate like the rest. The mask is
  binary (FSR2's sample settings); per-material reactivity would be finer.
- Next: TAAU (render below the output size and upsample in the resolve),
  Godot's velocity disocclusion (needs last frame's velocity for every
  pixel, not only moving meshes).

## From the review (deferred)

- `remap_group_handles` repoints textures and samplers after a reload, not
  uniform buffers: a group holding an app's own UBO needs it set again after a
  restore (the scene recreates its own).
- Auto-resize sizes from the CSS box × devicePixelRatio when the observer
  fires: a DPR change alone (zoom, moving the window to another screen)
  doesn't fire it, and a browser-clamped drawing buffer isn't noticed.
- `bind_read_source` (readback) rebinds READ_FRAMEBUFFER behind the cache.

## Context loss

Handled (`src/context_loss.rs`): a loss clears the resource pools (every handle
goes stale, nothing tries to delete dead objects), the renderer idles while
lost, extensions are re-enabled on restore, and `on_lost` / `on_restored(ctx)`
are called. Stale-handle reports are muted while a context is lost. Recreating
resources is the app's job (see the cube example). Possible follow-ups:

- The scene rebuilds itself (`rswebgl-scene/src/js/restore.rs`): renderer,
  environment (from the kept `.hdr` bytes — a copy of the file stays in
  memory), shaders, `loadTexture` textures, models (fetched again from their
  URL, `Model::adopt`; material groups and their edits survive). Left:
  - a model given as a data URI keeps its whole URI string for the reload;
    no "keep nothing, the app reloads" switch for memory-tight pages;
  - `createProgram` shaders come back under a new handle (meshes switched;
    the app's copy of the old handle is stale);
  - a `TexturePacker` (Rust) has to be recreated by its owner.
- glTF `Model` reload: `load_model` again + `Model::adopt`.
- No JS example of `set_on_restored` yet.

### Niche — note for completeness, not soon

- ~~**`EXT_clip_control`.**~~ Done: `Context::set_reverse_z` (`depth_range.rs`).
  Depth funcs and depth clears flip in the renderer, so states keep meaning
  "nearer"; the scene's cameras switch to reverse (infinite) projections and
  culling / picking follow. Gains most with a float depth buffer: the
  canvas's is 24-bit (no way to ask for a float one), so a scene that needs
  it renders offscreen into `Depth32f` and resolves to the canvas.
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

- **Skinning, morph targets, animation** — done (JOINTS_0/WEIGHTS_0; morph
  deltas in a texture; animations as data, sampled by the scene). Left:
  JOINTS_1/WEIGHTS_1 (8 influences), weights not renormalized. Was:
  **Skinning (JOINTS_n/WEIGHTS_n) & morph targets**, and **animation** (samplers/
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
- **Sparse accessors** — read through the `gltf` crate (with or without a
  base view), tested by reading the uploaded buffer back.
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
- Async reads, queries and parallel shader links poll on one shared timer
  per context (`poller.rs`), not one each.

## JS API

- Handles are bare `number`s in the `.d.ts`; branded types
  (`type TextureId = number & { __brand: "TextureId" }`) would catch mixing
  them up, via a `typescript_custom_section` or a post-processing step.
- Parameter names in the `.d.ts` stay snake_case (`index_type`); cosmetic.
- Builders consume themselves (`VaoBuilder.build`, `ParamLayoutBuilder`
  chaining): in JS the object is freed by the call. Documented, but a
  `&mut self` builder would be less surprising from JS.
- npm: `node scripts/build-npm.mjs` builds the `rswebgl` package (the
  scene crate: renderer, loader, scene in one wasm) into dist/npm, not
  published; `examples/npm-consumer` bundles it with esbuild. Left: the
  name on npm, a bundler-target build (`--target bundler`) if wanted, a
  smaller core-only package.
- Description objects via serde (`ctx.renderState({ depthTest: true })`) as
  planned in the design doc — not needed yet.

## glTF

- `EXT_mesh_gpu_instancing` instances come out as separate draw items
  (entities); the scene batches them back into one instanced draw.
  Quantized instance attributes (normalized rotation) aren't read — such nodes are drawn once, with a warning.
- Vertex attributes are dequantized to f32 on upload; keeping normalized
  u8/u16 (`KHR_mesh_quantization`) would cut memory on mobile.
- Skins, morph targets and animations: done (a skinned mesh isn't culled —
  its rest bounds don't hold; GPU picking sees the rest pose). Cameras and
  `KHR_lights_punctual` lights become `Camera` / `Light` entities
  (`Scene.cameras` / `lights`); a glTF camera's aspect yields to the canvas.

## Scene (`packages/rswebgl-scene`)

- World-space AABBs are recomputed for culling every frame. Measured (native,
  release): 100k entities cost 1.5 ms for box transform + frustum test, 0.8 ms
  of it the transform a cache would save — ~0.15 ms per 10k in wasm, less
  than the cache's own tick checks would leave. Not worth it; past ~10k
  objects the costs are draw calls (instancing) and a flat cull loop (a BVH).
- Instancing: opaque meshes of a scene-prelude shader with the same
  program, state, vertex array, material and command draw as one (matrices
  in an RGBA32F instance texture, `object_instanced.glsl`). Transparent ones
  still draw one by one (they sort per object). The texture is rewritten per
  `render` call: two passes built before either is rendered would share it.
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
  A glTF file's lights count toward the 8.
- IBL (E5b): `setEnvironmentHdr` only undoes the RLE (RGBE bytes, uploaded
  as RGBA8 and decoded on the GPU) and bakes in the background, a stage per
  frame (~130 ms; 2.4 ms on the main thread for a 1k HDRI, 11 for 2k, from
  30 / 80). The RLE decode itself is still on the main thread. `.hdr` only — no
  EXR, no KTX2 prebaked cubes. One environment per scene. The specular cube
  is also the background, so a sharp background is only as sharp as its
  `size`. A device without half-float render targets gets an error, no
  fallback (e.g. RGBM in RGBA8).
- Material model (OpenPBR + glTF reference, `shaders/lib/surface_*.glsl`):
  all extension maps and `KHR_texture_transform` are read (17 maps; a
  variant needing more samplers than the device has drops the subtlest
  maps, reported). The coat's normal map goes through the base's tangent
  frame (fine unless its UV set is oriented differently). Transmission sees a half-size copy of the
  opaque scene (HDR output) — one layer: glass behind glass shows the
  environment's / the opaque scene, not the other glass — unless depth
  peeling is on (`setTransmissionLayers(2..4)`: layers peeled front to
  back, shaded back to front over a copy of what is behind each; peeled
  glass isn't multisampled, deeper layers aren't drawn; dual peeling would
  halve the geometry passes but needs EXT_float_blend).
  One refraction (entry) for volumes; no internal reflections — a dedicated
  gem shader (ray-marching the stone's own geometry) is the jewelry-grade
  follow-up. glTF sheen's image-based light uses the GGX-prefiltered map at
  the sheen roughness (no Charlie-prefiltered cube). OpenPBR:
  subsurface is MaterialX's real-time approximation (curvature-sized Burley
  profile for direct light, irradiance × color for indirect; no scatter
  anisotropy, not thin-walled, OpenPBR names only — glTF has no extension);
  coat anisotropy along the surface tangent; emission is radiance
  (not nits), thin-walled translucency only through glTF's thickness = 0.
  Not read: diffuse transmission, KHR_materials_variants.
- Picking is on the GPU (`gpu_pick.rs`): a 1×1 draw through a pick matrix,
  read back async. It draws meshes by their location-0 attribute, not their
  own shaders (a displacing or skinning vertex shader is picked at rest).
  One pick in flight at a time is the natural use; many at once each draw
  and read on their own. No region (box-select) picking yet.
- Materials: `setMaterialTexture` sets any of the 17 maps (with a texture
  transform: offset, scale, rotation).
  The color space of a texture set on a map isn't checked against the role
  (sRGB for colors). `cloneMaterial` shares the source model's textures, so
  a clone outlives its model only as long as nothing samples them.
- Options objects are read with `Reflect` per call (setup-time API; not for
  per-frame use).

## From the async / lifetime review (deferred)

Fixed in the review commits; these were found and left for later:

- Error-path leaks reachable only on a lost context or a full pool:
  `Baker::new` / `run`, `Environment::constant`, `SceneRenderer::new` after
  their first allocation; a partially built `Model` when `load_model` fails
  late. Parameter layouts / groups are CPU-only and survive a loss.
- `SceneRenderer::release_removed` misses a removal when the app clears the
  world's trackers before rendering and adds one mesh as it removes another
  (the `Scene` order is right; affects Rust users driving the world).
- `decode_data_uri` doesn't percent-decode non-base64 data URIs.
- Image loads decode with `createImageBitmap` (off the main thread, no
  premultiply / color conversion, flip as a decode option); browsers whose
  `createImageBitmap` ignores those options (old Safari / Firefox) would
  premultiply or convert. No detection beyond "is there a
  `createImageBitmap`" (else an `<img>`).
- Timer queries: disjoint handling is now context-wide but untested by
  pixels (headless Chrome never reports a disjoint).
