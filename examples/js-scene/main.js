// @ts-check
// A glTF model in a scene, driven from plain JS. The scene owns the entities
// (plain numbers), their transforms and the draw list; JS only says what goes
// where. Shading is the built-in glTF PBR (a shader variant per primitive and
// material); lights are entities too. One `scene.render(camera, pass)` per
// frame updates world matrices, culls, and appends the draws.
//
// Build the bundle first (see README.md), then serve this folder.

import init, { ClearMask, Context, ContextOptions, Pass, Scene, loadModel } from "./pkg/rswebgl_scene.js";

const MODEL_URL =
  "https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Models/main/2.0/DamagedHelmet/glTF-Binary/DamagedHelmet.glb";
// An HDR environment (Radiance .hdr, equirectangular) from the three.js
// examples.
const ENV_URL =
  "https://raw.githubusercontent.com/mrdoob/three.js/dev/examples/textures/equirectangular/venice_sunset_1k.hdr";

await init();

const canvas = /** @type {HTMLCanvasElement} */ (document.getElementById("c"));
const statsEl = /** @type {HTMLElement} */ (document.getElementById("stats"));
const ctx = Context.fromCanvas(canvas, new ContextOptions());
const fb = ctx.defaultFramebuffer();
fb.setClearColor(0.08, 0.08, 0.1, 1);
fb.enableAutoResize();

const scene = new Scene(ctx);

const params = new URLSearchParams(location.search);
const url = params.get("model") ?? MODEL_URL;
const envUrl = params.get("env") ?? ENV_URL;
statsEl.textContent = `loading ${url}…`;
// The scene takes the model over: its GPU data goes with the root entity.
const [model, hdr] = await Promise.all([
  loadModel(ctx, url),
  fetch(envUrl).then((r) => r.arrayBuffer()),
]);
const root = scene.addModel(model);
// Image-based light: baked on the GPU into cube maps (256 per face), and
// shown blurred behind the model.
scene.setEnvironmentHdr(new Uint8Array(hdr), 256);
scene.setBackground(0.3);

// Frame the model: a camera looking at its bounding sphere.
const [cx, cy, cz, radius] = scene.modelBounds(root);
const camera = scene.spawn();
scene.setPerspectiveCamera(camera, Math.PI / 4, radius * 0.05, radius * 20);
const distance = (radius / Math.sin(Math.PI / 8)) * 1.1;

// A pivot at the model's center carries the turntable rotation; the model
// hangs off it, moved so its center sits on the pivot.
const pivot = scene.spawn();
scene.setTranslation(pivot, cx, cy, cz);
scene.setParent(root, pivot);
scene.setTranslation(root, -cx, -cy, -cz);

// A key light on top of the environment: lights are entities aimed with lookAt.
const light = (x, y, z, r, g, b, lux) => {
  const e = scene.spawn();
  scene.setDirectionalLight(e, r, g, b, lux);
  scene.lookAt(e, cx + x, cy + y, cz + z, cx, cy, cz);
  return e;
};
light(1, 1.5, 1.2, 1, 0.96, 0.9, 2); // key

const pass = Pass.toDefault();
pass.setClear(ClearMask.colorDepth());

let visible = true;
let background = true;
let envAngle = 0;
let spinning = true;
let exposure = 1;
let angle = 0;
window.addEventListener("keydown", (e) => {
  if (e.key === "v" || e.key === "V") scene.setVisible(root, (visible = !visible));
  if (e.key === "r" || e.key === "R") spinning = !spinning;
  if (e.key === "+" || e.key === "=") scene.setExposure((exposure *= 1.25));
  if (e.key === "-") scene.setExposure((exposure /= 1.25));
  if (e.key === "b" || e.key === "B") {
    background = !background;
    if (background) scene.setBackground(0.3);
    else scene.clearBackground();
  }
  if (e.key === "e" || e.key === "E") scene.setEnvironmentRotation((envAngle += Math.PI / 4));
});

let last = performance.now();
function frame(now) {
  if (spinning) angle += ((now - last) / 1000) * 0.6;
  last = now;
  scene.setRotationEuler(pivot, 0, angle, 0);
  scene.lookAt(camera, cx, cy + radius * 0.3, cz + distance, cx, cy, cz);
  scene.setCameraAspect(camera, fb.width() / Math.max(1, fb.height()));

  pass.clearDraws();
  scene.render(camera, pass);
  pass.sort();
  ctx.render(pass);

  statsEl.textContent =
    `entities ${scene.entityCount()}  drawn ${scene.drawn()}  culled ${scene.culled()}  ` +
    `shader variants ${scene.shaderVariantCount()}  exposure ${exposure.toFixed(2)}\n` +
    `V: ${visible ? "hide" : "show"} model   R: ${spinning ? "pause" : "resume"}   +/-: exposure   ` +
    `B: background   E: turn the environment`;
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
