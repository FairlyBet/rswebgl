# js-cube

A grid of cubes driven from plain JS through the generated bindings: no Rust
of its own. Press **S** to toggle draw-list sorting and watch the program /
state switch counts in the corner.

```bash
wasm-pack build packages/rswebgl --target web --out-dir ../../examples/js-cube/pkg
node examples/js-cube/serve.mjs
```

(Run both from the repository root.) `main.js` is type-checked against the
generated `.d.ts` (`// @ts-check`):

```bash
npx -p typescript tsc --noEmit --allowJs --checkJs --target es2022 --module es2022 --moduleResolution bundler --lib esnext,dom --noImplicitAny false examples/js-cube/main.js
```
