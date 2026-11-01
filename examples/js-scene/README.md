# js-scene

A glTF model in a scene, driven from plain JS through the `Scene` facade of
`rswebgl-scene`: entities are plain numbers, one `scene.render(camera, pass)`
per frame does transforms, frustum culling and the draw list. The model's
glTF nodes become entities you can find by name, move and hide.

Default model: Khronos' Avocado (fetched from GitHub); another one with
`?model=<url>`. Keys: **V** hides / shows the model (through its root, so the
whole subtree), **R** pauses the turntable.

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
