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

## Run

```bash
wasm-pack build packages/rswebgl-scene --target web --out-dir ../../examples/viewer/pkg
node examples/viewer/serve.mjs          # http://127.0.0.1:8091/
node examples/viewer/serve.mjs --lan    # also on this machine's LAN addresses, for a phone
```

The models and HDRIs are fetched from GitHub (KhronosGroup/glTF-Sample-Assets, mrdoob/three.js), so the device needs internet access.
