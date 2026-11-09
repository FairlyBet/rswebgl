# rswebgl

A WebGL2 renderer written in Rust, compiled to WebAssembly. It has two levels:

- **`Scene`**: a scene graph with glTF 2.0 models (compressed ones too),
  OpenPBR or glTF-reference shading, image-based light from `.hdr`
  environments, HDR output with tone mapping, MSAA or temporal
  antialiasing, picking, instancing, skinning, morph targets, animation and
  material variants.
- **The low-level API** under it (`Context`, `Pass`, programs, buffers,
  textures, render states, queries) for drawing your own way.

```js
import init, { Context, ContextOptions, Scene, loadModel } from "rswebgl";

await init(); // fetches rswebgl_bg.wasm next to the module

const canvas = document.querySelector("canvas");
const ctx = Context.fromCanvas(canvas, new ContextOptions());
ctx.defaultFramebuffer().enableAutoResize();
const scene = new Scene(ctx);

const root = scene.addModel(await loadModel(ctx, "model.glb"));
const [cx, cy, cz, r] = scene.modelBounds(root);
const camera = scene.spawn();
scene.setCamera(camera, { type: "perspective", yfov: 0.8, znear: r * 0.01, zfar: r * 100 });
scene.lookAt(camera, [cx, cy, cz + r * 2.5], [cx, cy, cz]);

const hdr = new Uint8Array(await (await fetch("studio.hdr")).arrayBuffer());
scene.setEnvironmentHdr(hdr, 256);
await scene.whenReady();

let last = performance.now();
requestAnimationFrame(function frame(now) {
  scene.advanceAnimations((now - last) / 1000);
  last = now;
  scene.renderFrame(camera);
  requestAnimationFrame(frame);
});
```

## Compressed glTF

meshopt-compressed files load as they are. Draco meshes and KTX2 (Basis
Universal) textures are decoded in workers by decoders you point the loader
at, before loading such a model (three.js' conventions: a directory with
`draco_wasm_wrapper.js` + `draco_decoder.wasm`, and one with
`basis_transcoder.js` + `basis_transcoder.wasm`):

```js
import { setDracoDecoderPath, setKtx2TranscoderPath } from "rswebgl";

setDracoDecoderPath("https://www.gstatic.com/draco/versioned/decoders/1.5.7/");
setKtx2TranscoderPath("https://cdn.jsdelivr.net/npm/three.170.0/examples/jsm/libs/basis/");
```

## Antialiasing

The scene's HDR target is multisampled by default (`scene.setSamples(4)`).
`scene.setTaa(true)` switches to temporal antialiasing, which also smooths
shading and thin geometry; pair it with `scene.setSharpening(0.5)`.

## Bundlers

The module finds its `.wasm` with `new URL("rswebgl_bg.wasm", import.meta.url)`,
which Vite, webpack 5 and Parcel follow. esbuild doesn't: import the file
and pass its URL (`--loader:.wasm=file`, with `--public-path` set to where
the bundle is served from):

```js
import wasmUrl from "rswebgl/rswebgl_bg.wasm";
await init({ module_or_path: wasmUrl });
```

Without a bundler, serve the package's files and import `rswebgl.js`; serve
`.wasm` as `application/wasm`.

Types are in `rswebgl.d.ts`.

## License

MIT. Third-party code it ports (MaterialX, Khronos, three.js, Filament,
others) is listed in `THIRD_PARTY.md`.
