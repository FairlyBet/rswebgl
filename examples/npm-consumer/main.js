// The npm package through a bundler: `import … from "rswebgl"`. esbuild
// doesn't follow the module's `new URL("rswebgl_bg.wasm", import.meta.url)`,
// so the .wasm is imported here (its file loader copies it and gives its
// URL) and handed to init. Vite and webpack 5 need neither.
import init, { Context, ContextOptions, Scene, loadModel } from "rswebgl";
import wasmUrl from "rswebgl/rswebgl_bg.wasm";

const status = document.getElementById("status");
await init({ module_or_path: wasmUrl });
const canvas = /** @type {HTMLCanvasElement} */ (document.getElementById("c"));
const ctx = Context.fromCanvas(canvas, new ContextOptions());
ctx.defaultFramebuffer().setClearColor(0.1, 0.1, 0.12, 1);
ctx.defaultFramebuffer().enableAutoResize();
const scene = new Scene(ctx);

const url =
  "https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/main/Models/DamagedHelmet/glTF-Binary/DamagedHelmet.glb";
const root = scene.addModel(await loadModel(ctx, url));
const [cx, cy, cz, r] = scene.modelBounds(root);
const camera = scene.spawn();
scene.setCamera(camera, { type: "perspective", yfov: 0.8, znear: r * 0.01, zfar: r * 100 });
scene.lookAt(camera, [cx + r * 1.5, cy + r * 0.5, cz + r * 2], [cx, cy, cz]);
const light = scene.spawn();
scene.setLight(light, { type: "directional", intensity: 3 });
scene.lookAt(light, [1, 2, 3], [0, 0, 0]);
await scene.whenReady();

requestAnimationFrame(function frame() {
  scene.renderFrame(camera);
  requestAnimationFrame(frame);
});
status.textContent = `rswebgl from npm: ${scene.drawn() >= 0 ? "running" : "?"}, HDR ${scene.isHdr()}`;
