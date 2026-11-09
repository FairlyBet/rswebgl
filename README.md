# rswebgl

A WebGL2 renderer in Rust, compiled to WebAssembly, usable from Rust and from
JavaScript alike. It is a general-purpose engine in the spirit of three.js:
a scene graph with glTF models and physically based shading on top of a
low-level WebGL2 layer that owns all GL state.

## Packages

| Package | What it is |
|---|---|
| [`rswebgl`](packages/rswebgl) | The core: `Context`, passes with sorted draw lists, programs, buffers, textures, samplers, framebuffers, render states, parameter groups, uniform buffers, queries, readback, transform feedback. The renderer keeps the GL state cache; GL objects are plain handles with explicit lifetimes. |
| [`rswebgl-gltf`](packages/rswebgl-gltf) | glTF 2.0 / GLB loader into GPU resources and a flat draw list. |
| [`rswebgl-ecs`](packages/rswebgl-ecs) | A small sparse-set ECS with change ticks. |
| [`rswebgl-scene`](packages/rswebgl-scene) | The scene: `Scene` for JS, ECS systems and a PBR renderer for Rust. |
| [`rswebgl-foundation`](packages/rswebgl-foundation) | Handle pools and other shared pieces. |

## Scene features

- **glTF 2.0**: meshes, PBR materials, node hierarchy, cameras, lights
  (`KHR_lights_punctual`), skins (up to eight joint influences), morph
  targets, animation, `EXT_mesh_gpu_instancing`, sparse accessors, any
  `TEXCOORD` set, MikkTSpace tangents where the file has none.
- **Material extensions**: `KHR_materials_*` emissive_strength, ior,
  specular, transmission, volume, dispersion, clearcoat, sheen, iridescence,
  anisotropy, diffuse_transmission, unlit, variants; `KHR_texture_transform`.
- **Compressed glTF**: `EXT_meshopt_compression` / `KHR_meshopt_compression`
  (decoded in Rust), `KHR_draco_mesh_compression` and `KHR_texture_basisu`
  (KTX2, transcoded to ASTC / BC7 / ETC2 / S3TC) in worker pools,
  `KHR_mesh_quantization`.
- **Shading**: OpenPBR (MaterialX's real-time model) or the glTF reference
  model; image-based light from `.hdr` environments (prefiltered on the
  GPU); punctual lights; transmission with depth-peeled glass layers;
  specular antialiasing.
- **Output**: HDR target, tone mapping (PBR Neutral, ACES, AgX), exposure,
  dither, MSAA or temporal antialiasing (motion vectors for moving meshes,
  a reactive mask for glass, velocity disocclusion) with RCAS sharpening.
- **The rest**: GPU picking, frustum culling, instanced batching, GPU
  timings, reverse-Z, and recovery from WebGL context loss (the scene
  rebuilds itself and reloads its models).

## Using it from JavaScript

```js
import init, { Context, ContextOptions, Scene, loadModel } from "rswebgl";

await init();
const ctx = Context.fromCanvas(canvas, new ContextOptions());
ctx.defaultFramebuffer().enableAutoResize();
const scene = new Scene(ctx);
const root = scene.addModel(await loadModel(ctx, "model.glb"));
// ... a camera, an environment, then scene.renderFrame(camera) each frame.
```

See [`npm/README.md`](npm/README.md) for the full example, bundler notes and
the decoders compressed glTF needs.

## Building and testing

The crates target `wasm32-unknown-unknown`; the JS packages are built with
[wasm-pack](https://rustwasm.github.io/wasm-pack/).

```bash
cargo clippy --workspace --target wasm32-unknown-unknown --all-targets -- -D warnings
```

```bash
wasm-pack test --headless --chrome packages/rswebgl
```

```bash
wasm-pack test --headless --chrome packages/rswebgl-scene
```

Browser tests run in headless Chrome; pass `--chromedriver <path>` when the
installed Chrome and the ChromeDriver wasm-pack downloads disagree. Host
tests (`cargo test -p rswebgl-ecs`, `-p rswebgl-gltf --lib`) run natively.

The npm package (`dist/npm`) is built by `node scripts/build-npm.mjs`.

## Examples

| Example | What it shows |
|---|---|
| [`examples/viewer`](examples/viewer) | A glTF viewer: Khronos sample models, shading and output settings, a material inspector, compressed models, context loss. |
| [`examples/js-scene`](examples/js-scene) | The `Scene` API from plain JS. |
| [`examples/js-cube`](examples/js-cube) | The core API from plain JS: draw lists, MSAA target, context-loss recovery. |
| [`examples/npm-consumer`](examples/npm-consumer) | The npm package bundled with esbuild. |
| `examples/cube`, `cubemap`, `texture`, `gltf`, `scene` | The Rust API (built with Trunk). |

## Documents

- [`PROGRESS.md`](PROGRESS.md): WebGL2 coverage of the core.
- [`TODO.md`](TODO.md): known limits and deferred work.
- [`THIRD_PARTY.md`](THIRD_PARTY.md): code ported from other projects, and
  their licenses.
- [`docs/design`](docs/design): architecture notes (in Russian).

## License

MIT (see [`LICENSE`](LICENSE)). Ported parts keep their original licenses,
listed in `THIRD_PARTY.md`.
