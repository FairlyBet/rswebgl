# TODO

Known limits and deferred work.

## Planned

In the order we mean to take them (TAAU, npm publishing and a gem shader are
set aside for now).

1. **Frame-time spikes on phones**: profile them (`rswebgl::trace`, the
   viewer's Profiling panel) and fix what the traces show.
2. **`EXT_texture_webp` / `EXT_texture_avif`**: not handled; gltf-transform
   writes WebP often. The browser decodes both (`createImageBitmap`); the
   texture's source moves to the extension's, as for `KHR_texture_basisu`.
3. **Quantized vertex attributes on the GPU**: everything is dequantized to
   f32 today. Upload `KHR_mesh_quantization` attributes as they are
   (positions i16 / i8, normals and tangents i8 normalized, UVs u16
   normalized; the dequantization is in the node matrix), and quantize
   plain files ourselves (normals / tangents to i8, UVs to u16 or half,
   morph deltas to half): about half the vertex memory. CPU-side users
   (bounds, MikkTSpace) keep a temporary f32 copy.
4. **`R11F_G11F_B10F`**: for the IBL cubes (half of RGBA16F; Filament does
   it) and, if no pass needs its alpha, the HDR scene target (RGBA16F with
   4× MSAA at 2560×1440 is ~118 MB).
5. **Shadows**: directional and spot shadow maps (depth texture,
   `sampler2DShadow` hardware PCF, Castaño's optimized PCF as in Filament /
   Bevy, normal-offset bias, the shadow camera fitted to the scene bounds;
   cascades for large scenes; point lights later). Then screen-space contact
   shadows (a short march toward the light through the depth buffer; MSAA
   needs a resolved depth). For environment-only scenes: GTAO (XeGTAO, as
   Godot / Bevy) and a ground shadow (three.js' contact-shadows example).
6. **Runtime texture compression**: glTF images that came uncompressed,
   encoded on the GPU in fragment shaders (Godot's Betsy, MIT: BC1/BC4/BC5/
   BC6H, ETC2), read back through a PBO and uploaded compressed — BC on
   desktop, ETC2 / EAC on mobile, two-channel formats for normal maps.
   2048² RGBA8 with mips is ~21 MB, BC1 / ETC2 ~2.8 MB. BC6H for the IBL
   cubes on desktop.
7. **Uniform uploads on mobile**: measured (`examples/viewer/ubo-bench.html`,
   Mali-G57 MC2, Chrome 157, 1000 draws × 80 bytes, CPU submit / fence ms):
   one upload of a big UBO 1.31 / 13.0; a `bufferSubData` per draw into it
   2.97 / 16.5; a small UBO rewritten per draw 2.31 / 21.0; plain uniforms
   1.77 / 15.4; a ring of three or orphaning no better than one upload.
   A tenth changing (100 scattered writes) ≈ uploading all 80 KB (1.13 vs
   1.05): a write costs about as much as ~1 KB of upload. So: merge dirty
   runs less than ~1 KB apart into one write (`uniform_buffer.rs`); a
   shared material UBO would gain ~25% over plain uniforms — low priority,
   per-object data already goes through the instance texture.
8. Later: bloom, LOD by meshoptimizer's simplifier, `KHR_animation_pointer`,
   `KHR_node_visibility`, `KHR_materials_volume_scatter`, `ctx.gpuInfo()`.

Texture arrays to cut texture switches: not planned in general. Layers
share one size and format (glTF textures don't: resampling loses quality),
and texture binds are cheap next to program switches and draw calls. They
pay off for shadow cascades, or to merge draws of different materials into
one instanced draw (a layer index per instance); look at `frameStats`'
texture binds on real scenes first.

## Core (`packages/rswebgl`)

### Errors and GL limits

The rule is documented at the top of `src/lib.rs` (`Result` / `error` + no-op /
`warn`, per-frame problems reported once). `src/limits.rs` checks vertex
attribs, texture sizes / layers / mip levels, color attachments + draw
buffers, renderbuffer size and samples (per format too: `maxSamplesFor`);
sub-image regions are checked against their level. Left:

- Limits come from the first context and are process-wide; fine unless one
  page mixes very different GPUs.
- RGB16F / RGB32F render targets are left to GL's completeness check (not
  renderable in core, optional with the half-float extension).

### Uniform buffers (UBO)

- Scattered writes upload as separate runs; whether many small
  `bufferSubData` runs beat one larger one on mobile drivers is unmeasured
  (a gap threshold could merge runs a few chunks apart). See "Planned" 7:
  the scene itself has two blocks written once a frame, so it matters
  only once materials live in UBOs.
- Layout validation is size-only (`UNIFORM_BLOCK_DATA_SIZE` vs our std140
  size): same-size permutations and per-field offset errors slip through.
- `bindBufferRange` (several blocks in one buffer) is wired
  (`set_uniform_block_range`) but has no example.

### Extensions

- `WEBGL_debug_renderer_info`: a `ctx.gpu_info()` (vendor + renderer) for bug
  reports and per-GPU fallbacks.
- `WEBGL_multi_draw`: little left for it after the scene's instancing (glTF
  primitives each have their own vertex array; without base instance the
  draws of one multi-draw can't find their instances). Worth it for apps
  with big shared vertex buffers.
- `EXT_depth_clamp` (shadow volumes) isn't wired.
- Timer queries can't span several passes or a blit (the transmission grab
  isn't timed); `TIMESTAMP_EXT` isn't exposed by WebGL2. Occlusion queries
  wrap one draw, not a group of draws. Disjoint handling is untested by
  pixels (headless Chrome never reports one).

### Transform feedback (`src/feedback.rs`)

- At most 4 bindings (the guaranteed minimum); the real limit isn't queried.
- One program per recording (several would need pause / resume).
- A draw that reads the buffer it records into isn't checked.
- No JS example (GPU particles would be the natural one).

### Context loss

Handled (`src/context_loss.rs`): a loss clears the resource pools, the
renderer idles while lost, extensions come back on restore, `on_lost` /
`on_restored(ctx)` are called; parameter groups and render states survive.
`remap_group_handles` / `remap_group_blocks` repoint groups at recreated
textures, samplers and uniform buffers. Recreating GPU objects is the app's
job (`examples/js-cube`, press L). A `TexturePacker` (Rust) is recreated by
its owner.

### Minor

- Parameter groups: an unchanged group could skip its per-field compare;
  zero-copy JS views over a group's words; sampler arrays; groups don't keep
  the textures / UBOs they reference alive (a destroyed one is reported at
  render).
- Texture units / block points are assigned per program from 0, so programs
  sharing a block name may rebind more than needed (a context-wide registry
  would let them share).
- The default sort key truncates handle indices (program 15 bits, state 12,
  material 20, VAO 16): very large scenes group slightly worse, never wrong.
  `ctx.render_state` interns by linear search: fine for tens of states.
- The GL state cache trusts that only the library binds: raw GL on the same
  context needs `ctx.invalidate_state()` afterwards.
- Pass resolve copies color attachment 0 only (MRT or depth resolves go
  through `blit_framebuffer`). `invalidateSubFramebuffer` and
  `copyTexSubImage3D` aren't done.
- Async reads, queries and parallel links poll on one shared timer per
  context (`poller.rs`).
- Texture packer: outputs are always `RGBA8` / `SRGB8_ALPHA8`, no `RG8` /
  `R8`.

## glTF loader (`packages/rswebgl-gltf`)

- Compression: `EXT_meshopt_compression` / `KHR_meshopt_compression`
  decoded in Rust (`meshopt.rs`) on the main thread (a memcpy-class codec);
  `KHR_texture_basisu` (ETC1S / UASTC, not HDR) and
  `KHR_draco_mesh_compression` decoded in worker pools (`decoders.js`) by
  the decoders the app points at. KTX2 picks ASTC → BC7 → ETC2 → BC1/3 →
  RGBA8 as three.js does. Draco and KTX2 have no automated tests (the
  decoders come from CDNs); checked with the Khronos samples in the viewer.
- Attributes (quantized ones too) are dequantized to f32 on upload, indices
  keep their width: keeping normalized u8 / u16 would cut memory on mobile.
- The texture sRGB role is decided by usage; a texture shared by a linear
  and an sRGB role picks one.
- A material reading more than two TEXCOORD sets keeps the first two (the
  shader has two UV slots), reported.
- MikkTSpace tangents are per vertex: a vertex shared by faces whose
  tangents differ keeps one (no welding split, as Bevy and three.js).
- `EXT_mesh_gpu_instancing` with quantized instance attributes isn't read
  (drawn once, with a warning).

## Scene (`packages/rswebgl-scene`)

### TAA

- `setTaa(true)` (HDR output, instead of MSAA): Bevy's resolve
  (`shaders/taa.frag`) with Godot's 9-tap Catmull-Rom history, Halton
  jitter, map LOD bias −1. Static geometry is reprojected through the depth;
  meshes that moved get motion vectors (`src/motion.rs`, Unreal's split).
  FSR2's autogenerated reactive mask for transmissive and transparent
  meshes, Godot's velocity disocclusion, FSR1 RCAS sharpening.
- Motion vectors: custom instanced draws (`instance_count` > 1) get none; a
  mesh's first frame has no previous state.
- The reactive mask is binary (FSR2's threshold, a value of 0.5 measured
  on refracting glass); per-material reactivity would be finer. Through
  glass, while still, the clip box is three times wider: the refraction
  jumps with the jitter and a tight box flickered (DragonDispersion, still
  camera: frame-to-frame change ~10x lower).
- TAAU (render below the output size) was tried and reverted (3543d86): a
  Karis / Unreal-style reconstruction converged on a still scene but ghosted
  badly while turning. Next attempt: port FSR2's accumulate pass properly
  (locks, depth clip, luma instability, Lanczos reconstruction).

### Rendering

- Lights are capped at 8 per frame (all reach every draw, a file's lights
  included); spot lights untested by pixels; no shadows.
- Transparent draws sort per object (bounds center): intersecting or concave
  blended meshes can show order errors. They draw one by one (no
  instancing).
- Front-face winding is fixed at load from the world matrix: a part mirrored
  later by a negative scale draws inside out.
- World-space AABBs are recomputed every frame (~0.15 ms per 10k objects in
  wasm); past ~10k objects a BVH and instancing matter more.
- The instance texture is rewritten per `render` call: two passes built
  before either is rendered would share it.
- IBL: `.hdr` only (no EXR, no prebaked KTX2 cubes), one environment per
  scene, the RLE decode on the main thread. The specular cube is also the
  background, as sharp as its `size`. No fallback without half-float render
  targets (e.g. RGBM in RGBA8).
- Materials: 19 maps; a variant needing more samplers than the device has
  drops the subtlest, reported. The coat's normal map uses the base's
  tangent frame. Transmission sees a copy of the opaque scene
  (`setTransmissionResolution`, full size by default);
  glass behind glass needs depth peeling (`setTransmissionLayers(2..4)`;
  peeled glass isn't multisampled, deeper layers aren't drawn; dual peeling
  would halve the passes but needs `EXT_float_blend`). Volumes refract once
  (entry), no internal reflections. glTF sheen's IBL uses the GGX map at the
  sheen roughness (no Charlie-prefiltered cube). OpenPBR subsurface is
  MaterialX's real-time approximation; emission is radiance, not nits.
- `setMaterialTexture` doesn't check a texture's color space against the
  map's role; `cloneMaterial` shares the source model's textures.
- `setMaterialVariant` sets the variant's materials; edits made with
  `setMaterial` on those meshes are replaced.

### Picking, models, API

- Picking draws meshes by their location-0 attribute, not their own
  shaders: a displacing or skinning shader is picked at rest. No box
  selection.
- A model added twice needs two loads (no shared GPU data between roots).
- Context loss: the environment's `.hdr` bytes stay in memory for the
  rebake; models are fetched again from their URL unless
  `setRestoreModels(false)`.
- JS reads return new arrays except `copyWorldMatrix`; zero-copy views over
  component storage would help bulk reads. Options objects are read with
  `Reflect` per call (setup-time API).
- Image loads use `createImageBitmap` (off the main thread); browsers that
  ignore its options (old Safari / Firefox) would premultiply or convert.

## JS API and packaging

- Handles are bare `number`s in the `.d.ts`; branded types would catch
  mixing them up. Parameter names there stay snake_case.
- Builders consume themselves (`VaoBuilder.build`, `ParamLayoutBuilder`
  chaining): in JS the object is freed by the call.
- npm: `node scripts/build-npm.mjs` builds the package into `dist/npm`, not
  published. Left: the name on npm, a `--target bundler` build if wanted, a
  smaller core-only package.
- Description objects via serde (`ctx.renderState({ depthTest: true })`)
  as planned in the design doc — not needed yet.
