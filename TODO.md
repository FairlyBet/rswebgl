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

- **Dirty tracking by 16-byte chunks.** `UniformBuffer` tracks dirty as one
  contiguous `[lo, hi)` range (`src/uniform_buffer.rs`), so *scattered* writes
  coalesce: touching offset 0 and offset 1000 marks `[0, 1004)` and `flush`
  re-uploads the ~1000 untouched bytes between them. Fine for small/clustered
  blocks (the common case). If large blocks with sparse, spread-out updates show
  up (e.g. a big array-of-structs where only a few elements change per frame),
  switch to a bitmap of 16-byte chunks (std140's granularity): a write sets the
  bits it touches, and `flush` uploads only dirty chunks, coalescing adjacent
  ones into runs. More bookkeeping; only worth it when writes are actually
  scattered.
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

### Top — near-mandatory, complements the finished FBO work

- **`EXT_color_buffer_float` + `EXT_color_buffer_half_float` +
  `OES_texture_float_linear`.** Without these you *cannot render to* float
  targets (RGBA16F/RGBA32F). We have float texture formats and a full FBO
  system, but rendering into them silently yields an incomplete framebuffer.
  Blocks HDR, bloom, deferred, ping-pong GPGPU, accumulation. half-float covers
  mobile; `OES_texture_float_linear` adds linear filtering of float textures
  (else NEAREST only). **Integration:** auto-enable (or validate + clear
  `console::error` instead of "framebuffer incomplete") when a float format is
  attached as a color attachment; possibly auto-enable on context creation since
  desktop support is near-universal. *Do this first — it's a hole in the FBO
  work, not a new feature.*

### Useful — maps onto subsystems we already have / have queued

- **`EXT_disjoint_timer_query_webgl2`.** GPU timing. Pairs with the `Query`
  object (backlog, ❌ in PROGRESS) — gives per-pass "how long did the GPU spend"
  profiling once Query lands.
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

Handled (`src/context_loss.rs`): the renderer idles while lost, extensions are
re-enabled on restore, `on_lost` / `on_restored` callbacks, and stale handles
skip their delete. Recreating resources is the app's job (see the cube
example). Possible follow-ups:

- **Automatic restore** of objects the library itself owns (e.g. the
  `TexturePacker` program and quad), so apps don't have to rebuild them.
- glTF `Model` has no "reload" path; an app has to call `load_model` again.
- No JS example of `set_on_restored` yet.

### Niche — note for completeness, not soon

- **`EXT_clip_control`.** Zero-to-one depth + reverse-Z (like D3D/Vulkan);
  strongly mitigates z-fighting at distance. Powerful but the user must know what
  they're doing.
- **`EXT_depth_clamp`.** Disable near/far clipping (shadow volumes).
- **`EXT_float_blend`.** Blending into 32F targets (16F already blends).

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

- The blit samples sources with whatever filter they currently carry. The glTF
  ORM path works around this by forcing its (throwaway) sources to NEAREST first,
  but the *general* packer can't mutate a caller's textures — a 1×1 sampler object
  would let it override filtering per-blit instead (no sampler objects yet — see
  PROGRESS "Sampler object ❌"). Minor quality point only.
- `pack`/`pack_into` always output `RGBA8`/`SRGB8_ALPHA8`; no `RG8`/`R8` outputs
  (would shave memory when fewer than 3 channels are used). Revisit with the
  RGB/single-channel format question already noted for the glTF loader.

## Minor / nice-to-have (from the audit)

- The unset-uniform check also flags samplers a shader declares but a branch
  never samples. glTF `write_uniforms` leaves `u_base_color_tex` unset for
  untextured materials (it's gated by `u_has_base_color_tex`), so such a model
  will log that warning once. Bind a 1×1 white texture there instead, or allow
  marking uniforms as optional.
- Texture units / block points are assigned per program from 0, so programs
  that share a block name ("Frame") may put it on different points and rebind
  more than needed. A context-wide name→point registry would let them share.

- `Pass`/`Batch` snapshot `RenderState` by value, so mutating a `RenderState`
  after `Batch::new` doesn't reflect (unlike uniforms, which now share via
  `Rc<RefCell>`). Either document this, or make `RenderState` a shared handle for
  a consistent mental model. Low impact (render state is usually static).
- `UniformValues` is now `RefCell`-backed, so misuse (reentrant borrow) is a
  runtime panic rather than a compile error. All call sites are internal today,
  but keep in mind if `sync_from`-like methods are ever exposed.
- MSAA → texture resolve (`blitFramebuffer`) and `invalidateSubFramebuffer` are
  still unimplemented (deferred from the FBO work).
- `copyTex(Sub)Image2D/3D` deferred: they copy from the bound READ framebuffer,
  which the per-pass renderer owns — belongs in a future pass/renderer-level
  copy op rather than loose `Texture` methods.
- Pixel *pack* params + `readPixels` not yet implemented (only *unpack* is, via
  `PixelUnpack`).
