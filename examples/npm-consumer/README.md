# npm-consumer

The `rswebgl` npm package used the way an app would: installed as a
dependency and bundled (esbuild here — the strictest about `.wasm`; Vite and
webpack 5 need no extra setup).

```bash
node scripts/build-npm.mjs
cd examples/npm-consumer
npm install
npm run build
npm run serve     # http://127.0.0.1:8091/
```

(The first command from the repository root.) The model comes from
KhronosGroup/glTF-Sample-Assets on GitHub.
