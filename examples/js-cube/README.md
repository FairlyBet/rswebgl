# js-cube

A grid of cubes driven from plain JS through the generated bindings: no Rust
of its own, rendered through a 4× MSAA framebuffer resolved into the canvas.
Press **S** to toggle draw-list sorting and watch the program / state switch
counts in the corner; **M** toggles MSAA. **L** loses the WebGL context and
restores it half a second later: `ctx.setOnRestored` makes the GPU objects
again (programs, the cube's vertex array, the MSAA target); parameter groups
and render states survive a loss.



```bash
wasm-pack build packages/rswebgl --target web --out-dir ../../examples/js-cube/pkg
node examples/js-cube/serve.mjs
```

(Run both from the repository root.) `main.js` is type-checked against the
generated `.d.ts` (`// @ts-check`):

```bash
npx -p typescript tsc --noEmit --allowJs --checkJs --target es2022 --module es2022 --moduleResolution bundler --lib esnext,dom --noImplicitAny false examples/js-cube/main.js
```
