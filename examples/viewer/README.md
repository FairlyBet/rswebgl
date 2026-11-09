# rswebgl viewer

A glTF viewer built on `rswebgl-scene`:

- **Models.** Pick a model from Khronos' sample assets, enter a URL, open a `.glb` file, or drop one on the page. `?model=<url>` loads a model at start.
- **Shading.** Switch between OpenPBR and the glTF reference model. You can also set tone mapping (PBR Neutral, ACES, AgX, none), exposure, HDR output, MSAA, render resolution, dither and reverse-Z.
- **Environment and light.** Choose the HDRI and set its intensity, rotation, background and blur, plus the key light.
- **Stats.** FPS, frame time with a graph, CPU time of `renderFrame`, and drawn, culled and pending counts, shader variants and entities.
- **Material inspector.** Tap a part to see its material and edit it with OpenPBR parameters (colors and scalars). "Own copy" detaches the part from the material it shares with other parts.
- **Compressed models.** Draco, meshopt and KTX2 samples from the list load with the decoders the viewer points at (Google's Draco, three.js' copy of the Basis transcoder).
- **Antialiasing.** MSAA or TAA, with a Sharpen slider (RCAS).
- **Variants.** Models with `KHR_materials_variants` (the shoe) list their variants to switch between.
- **Context loss.** A button simulates losing the WebGL context; the scene restores itself.
- **Profiling.** Records a trace to download or send to the computer (see below). "Measure what this view costs" times the current view with each feature (thin film, transmission, coat, fuzz, the glass copy's size, the other shading model, TAA) turned off in turn.

## Run

```bash
wasm-pack build packages/rswebgl-scene --target web --out-dir ../../examples/viewer/pkg
node examples/viewer/serve.mjs          # http://127.0.0.1:8091/
node examples/viewer/serve.mjs --lan    # also on this machine's LAN addresses, for a phone
```

The models and HDRIs are fetched from GitHub (KhronosGroup/glTF-Sample-Assets, mrdoob/three.js), so the device needs internet access.

## Profiling a device

The Profiling panel records a trace for a few seconds: the library's spans
(`renderFrame` and its stages, GL passes, uploads, shader links), frame
intervals, GPU progress by fences (works without timer queries, as on most
phones), wasm memory, and the browser's long animation frames. The panel
then shows frame-time percentiles; **Download the report** saves the JSON,
**Send it to the computer** posts it to `serve.mjs`, which saves it under
`traces/` at the repository root (the phone opens the viewer through
`--lan`).

Open a report in <https://ui.perfetto.dev>, or summarize its slowest frames:

```bash
node scripts/trace-summary.mjs traces/<file>.json
```

## Uniform upload benchmark

`ubo-bench.html` (linked from the Profiling panel) draws N quads with 80
bytes of uniform data each and changes it every frame in different ways:
plain uniforms, one small UBO rewritten per draw, one big UBO uploaded at
once or written per draw, a ring of three, orphaning, and with only a tenth
of the draws changing. Raw WebGL2, recorded with the same profiler: the
page shows a table (CPU submit time, frame interval, fence latency) and
sends or downloads the trace like the viewer.
