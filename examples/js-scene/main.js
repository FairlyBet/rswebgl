// @ts-check
// A glTF model in a scene, driven from plain JS. The scene owns the entities
// (plain numbers), their transforms and the draw list; JS only says what goes
// where. Shading is the built-in glTF PBR (a shader variant per primitive and
// material); lights are entities too. One `scene.renderFrame(camera)` per
// frame updates world matrices, culls, sorts and draws to the canvas.
//
// Build the bundle first (see README.md), then serve this folder.

import init, { Context, ContextOptions, Scene, loadModel } from "./pkg/rswebgl_scene.js";

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
// Shaders compile in the background; wait so the model appears whole
// (frames before this would leave its parts out, never stall).
await scene.whenReady();

// Frame the model: a camera looking at its bounding sphere.
const [cx, cy, cz, radius] = scene.modelBounds(root);
const center = [cx, cy, cz];
const camera = scene.spawn();
scene.setCamera(camera, { type: "perspective", yfov: Math.PI / 4, znear: radius * 0.05, zfar: radius * 20 });
const distance = (radius / Math.sin(Math.PI / 8)) * 1.1;
scene.lookAt(camera, [cx, cy + radius * 0.3, cz + distance], center);

// A pivot at the model's center carries the turntable rotation; the model
// hangs off it, moved so its center sits on the pivot.
const pivot = scene.spawn();
scene.setTranslation(pivot, cx, cy, cz);
scene.setParent(root, pivot);
scene.setTranslation(root, -cx, -cy, -cz);

// A key light on top of the environment: lights are entities aimed with lookAt.
const key = scene.spawn();
scene.setLight(key, { type: "directional", color: [1, 0.96, 0.9], intensity: 2 });
scene.lookAt(key, [cx + radius, cy + radius * 1.5, cz + radius * 1.2], center);

// Click a part: its name shows; C paints it a new color (on a copy of its
// material, so parts sharing the original keep theirs).
let picked = 0;
canvas.addEventListener("click", async (e) => {
  const x = (e.offsetX / canvas.clientWidth) * 2 - 1;
  const y = 1 - (e.offsetY / canvas.clientHeight) * 2;
  // Picked on the GPU: the answer comes a frame or so later.
  const hit = await scene.pick(camera, x, y);
  picked = hit ? hit.entity : 0;
});
const ownMaterial = new Map(); // picked entity → its own copy
const recolor = () => {
  if (!picked) return;
  let own = ownMaterial.get(picked);
  if (!own) ownMaterial.set(picked, (own = scene.cloneMaterial(scene.meshMaterial(picked))));
  scene.setMaterialProperty(own, "baseColorFactor", [Math.random(), Math.random(), Math.random(), 1]);
  scene.setMaterial(picked, own);
};

let visible = true;
let background = true;
let envAngle = 0;
let spinning = true;
let exposure = 1;
let angle = 0;
window.addEventListener("keydown", (e) => {
  const k = e.key.toLowerCase();
  if (k === "v") scene.setVisible(root, (visible = !visible));
  if (k === "r") spinning = !spinning;
  if (k === "+" || k === "=") scene.setExposure((exposure *= 1.25));
  if (k === "-") scene.setExposure((exposure /= 1.25));
  if (k === "b") {
    background = !background;
    if (background) scene.setBackground(0.3);
    else scene.clearBackground();
  }
  if (k === "e") scene.setEnvironmentRotation((envAngle += Math.PI / 4));
  if (k === "c") recolor();
  // Simulate a context loss; the scene rebuilds itself after the restore
  // (model fetched again, environment re-baked, materials as they were).
  if (k === "l" && ctx.simulateContextLoss()) {
    setTimeout(() => ctx.simulateContextRestore(), 1000);
  }
});

let last = performance.now();
function frame(now) {
  if (spinning) angle += ((now - last) / 1000) * 0.6;
  last = now;
  scene.setRotationEuler(pivot, 0, angle, 0);
  scene.renderFrame(camera);

  const part = picked ? (scene.name(scene.parent(picked)) ?? `entity ${picked}`) : "—";
  statsEl.textContent =
    `entities ${scene.entityCount()}  drawn ${scene.drawn()}  culled ${scene.culled()}  ` +
    `shader variants ${scene.shaderVariantCount()}  exposure ${exposure.toFixed(2)}\n` +
    `picked: ${part}   click: pick   C: recolor it   V: ${visible ? "hide" : "show"} model   ` +
    `R: ${spinning ? "pause" : "resume"}   +/-: exposure   B: background   E: turn the environment`;
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
