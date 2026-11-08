# js-scene

A glTF model in a scene, driven from plain JS through the `Scene` facade of
`rswebgl-scene`: entities are plain numbers, one `scene.render(camera, pass)`
per frame does transforms, frustum culling and the draw list. The model's
glTF nodes become entities you can find by name, move and hide.

Default model: Khronos' DamagedHelmet (fetched from GitHub; all five PBR maps),
shaded by the built-in glTF PBR, lit by an HDR environment (three.js' Venice
sunset, baked into cube maps on load and shown blurred behind it) plus a key
light. Other assets: `?model=<url>` and `?env=<url.hdr>`.
**Click** a part to pick it (its name shows), **C** paints it a random color
(through a copy of its material). Keys: **V** hides / shows the model (through its root, so the
whole subtree), **R** pauses the turntable, **+ / −** change the exposure, **B**
toggles the background, **E** turns the environment, **L** simulates a
context loss (restored a second later: the scene rebuilds itself), **M** switches
the shading model (OpenPBR ↔ the glTF reference), **T** the tone mapper.

```bash
wasm-pack build packages/rswebgl-scene --target web --out-dir ../../examples/js-scene/pkg
node examples/js-scene/serve.mjs
```

(Run both from the repository root.) The bundle holds the renderer, the glTF
loader and the scene in one module. `main.js` is type-checked against the
generated `.d.ts` (`// @ts-check`):

```bash
npx -p typescript tsc --noEmit --allowJs --checkJs --target es2022 --module es2022 --moduleResolution bundler --lib esnext,dom --noImplicitAny false examples/js-scene/main.js
```
