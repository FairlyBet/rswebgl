// @ts-check
// The rswebgl viewer: glTF models from Khronos' sample assets (or yours),
// shaded by OpenPBR or the glTF reference, with HDR output, tone mapping,
// image-based light, an orbit camera, frame timing, and a material inspector
// that edits a part's material by OpenPBR parameter names.
//
// Build the bundle first (see README.md), then serve this folder.

import init, { Context, ContextOptions, Scene, Viewport, loadModel } from "./pkg/rswebgl_scene.js";

const SAMPLES = "https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/main/Models";
const glb = (/** @type {string} */ name) => `${SAMPLES}/${name}/glTF-Binary/${name}.glb`;
const MODELS = [
  ["DamagedHelmet", "Damaged helmet — base PBR, all five maps"],
  ["SheenChair", "Sheen chair — sheen / fuzz, texture transform"],
  ["GlamVelvetSofa", "Velvet sofa — sheen"],
  ["DragonAttenuation", "Dragon — transmission, volume"],
  ["DragonDispersion", "Dragon — dispersion"],
  ["MosquitoInAmber", "Mosquito in amber — transmission, volume"],
  ["GlassVaseFlowers", "Glass vase — transmission"],
  ["ClearCoatCarPaint", "Car paint — clearcoat"],
  ["CarbonFibre", "Carbon fibre — clearcoat, anisotropy"],
  ["AnisotropyBarnLamp", "Barn lamp — anisotropy"],
  ["IridescenceLamp", "Lamp — iridescence"],
  ["IridescentDishWithOlives", "Dish with olives — iridescence, transmission"],
  ["ChronographWatch", "Chronograph watch — metals, glass"],
  ["SunglassesKhronos", "Sunglasses — iridescence, transmission"],
  ["ToyCar", "Toy car — clearcoat, sheen, transmission"],
  ["CompareSheen", "Compare: sheen"],
  ["CompareClearcoat", "Compare: clearcoat"],
  ["CompareTransmission", "Compare: transmission"],
  ["CompareIridescence", "Compare: iridescence"],
  ["CompareAnisotropy", "Compare: anisotropy"],
  ["TransmissionRoughnessTest", "Test: transmission roughness"],
  ["WaterBottle", "Water bottle"],
  ["Corset", "Corset"],
  ["Avocado", "Avocado"],
  ["BoomBox", "Boom box"],
  ["Lantern", "Lantern"],
  ["MaterialsVariantsShoe", "Shoe"],
  ["Fox", "Fox"],
  ["ABeautifulGame", "A beautiful game — chess set (large)"],
];
const HDRS = "https://raw.githubusercontent.com/mrdoob/three.js/dev/examples/textures/equirectangular";
const ENVS = [
  ["venice_sunset_1k", "Venice sunset"],
  ["spruit_sunrise_1k", "Spruit sunrise"],
  ["blouberg_sunrise_2_1k", "Blouberg sunrise"],
  ["quarry_01_1k", "Quarry"],
  ["pedestrian_overpass_1k", "Pedestrian overpass"],
  ["moonless_golf_1k", "Moonless golf (night)"],
  ["san_giuseppe_bridge_2k", "San Giuseppe bridge (2k)"],
];

await init();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const canvas = /** @type {HTMLCanvasElement} */ ($("c"));
const status = (/** @type {string} */ text, error = false) => {
  $("status").textContent = text;
  $("status").classList.toggle("error", error);
};

// The scene does its own antialiasing (MSAA on its HDR target).
const options = new ContextOptions();
options.antialias = false;
const ctx = Context.fromCanvas(canvas, options);
const fb = ctx.defaultFramebuffer();
fb.setClearColor(0.05, 0.05, 0.06, 1);
const scene = new Scene(ctx);
// For poking from the devtools console.
Object.assign(globalThis, { viewer: { ctx, scene } });

// --- resolution ---------------------------------------------------------------

// Phones at 3× pay for 9× the pixels; start them at ¾.
let resolution = devicePixelRatio > 2 ? "0.75" : "1";
$("resolution").value = resolution;
function resize() {
  const scale = resolution === "css" ? 1 : devicePixelRatio * Number(resolution);
  const w = Math.max(1, Math.round(canvas.clientWidth * scale));
  const h = Math.max(1, Math.round(canvas.clientHeight * scale));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
    fb.setViewport(new Viewport(0, 0, w, h));
  }
}
addEventListener("resize", resize);
resize();

// --- camera and light ----------------------------------------------------------

const camera = scene.spawn();
Object.assign(/** @type {any} */ (globalThis).viewer, { camera });
const orbit = { yaw: 0.6, pitch: 0.25, distance: 3, target: [0, 0, 0], radius: 1, fov: 45 };
Object.assign(/** @type {any} */ (globalThis).viewer, { orbit });
function setCameraLens() {
  const r = orbit.radius;
  scene.setCamera(camera, { type: "perspective", yfov: (orbit.fov * Math.PI) / 180, znear: r * 0.01, zfar: r * 200 });
}
function eye() {
  const { yaw, pitch, distance, target } = orbit;
  return [
    target[0] + distance * Math.cos(pitch) * Math.sin(yaw),
    target[1] + distance * Math.sin(pitch),
    target[2] + distance * Math.cos(pitch) * Math.cos(yaw),
  ];
}
function placeCamera() {
  const e = eye();
  scene.lookAt(camera, e, orbit.target);
  // A key light above and to the left of the eye, aimed at the model.
  const r = orbit.radius;
  scene.lookAt(light, [e[0] - r * 2, e[1] + r * 3, e[2] + r], orbit.target);
}
const light = scene.spawn();
let lightIntensity = 2;
const setLight = () => scene.setLight(light, { type: "directional", intensity: lightIntensity });
setLight();

// Orbit with one pointer, zoom with the wheel or two; a tap picks.
const pointers = new Map();
let moved = 0;
let pinch = 0;
canvas.addEventListener("pointerdown", (e) => {
  canvas.setPointerCapture(e.pointerId);
  pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
  moved = 0;
  if (pointers.size === 2) {
    const [a, b] = [...pointers.values()];
    pinch = Math.hypot(a.x - b.x, a.y - b.y);
  }
});
canvas.addEventListener("pointermove", (e) => {
  const p = pointers.get(e.pointerId);
  if (!p) return;
  const dx = e.clientX - p.x;
  const dy = e.clientY - p.y;
  p.x = e.clientX;
  p.y = e.clientY;
  moved += Math.abs(dx) + Math.abs(dy);
  if (pointers.size === 1) {
    orbit.yaw -= dx * 0.008;
    orbit.pitch = Math.max(-1.5, Math.min(1.5, orbit.pitch + dy * 0.008));
  } else if (pointers.size === 2) {
    const [a, b] = [...pointers.values()];
    const d = Math.hypot(a.x - b.x, a.y - b.y);
    if (pinch > 0) zoom(pinch / d);
    pinch = d;
  }
});
const release = (/** @type {PointerEvent} */ e) => {
  const single = pointers.size === 1;
  pointers.delete(e.pointerId);
  if (single && moved < 6 && e.type === "pointerup") pickAt(e);
};
canvas.addEventListener("pointerup", release);
canvas.addEventListener("pointercancel", release);
canvas.addEventListener("wheel", (e) => {
  e.preventDefault();
  zoom(Math.exp(e.deltaY * 0.001));
}, { passive: false });
function zoom(/** @type {number} */ factor) {
  orbit.distance = Math.max(orbit.radius * 0.2, Math.min(orbit.radius * 50, orbit.distance * factor));
}

// --- models --------------------------------------------------------------------

let root = 0;
let loading = 0;
async function load(/** @type {string} */ url, /** @type {string} */ label) {
  const ticket = ++loading;
  status(`Loading ${label}…`);
  const t0 = performance.now();
  try {
    const model = await loadModel(ctx, url);
    if (ticket !== loading) {
      model.destroy(ctx); // a newer load won
      return;
    }
    if (root) scene.despawn(root);
    root = scene.addModel(model);
    const [cx, cy, cz, r] = scene.modelBounds(root);
    orbit.target = [cx, cy, cz];
    orbit.radius = r || 1;
    resetView();
    setCameraLens();
    inspect(0, "");
    status(`Compiling shaders for ${label}…`);
    await scene.whenReady();
    if (ticket === loading) {
      status(`${label}: ${((performance.now() - t0) / 1000).toFixed(1)} s`);
      setTimeout(() => ticket === loading && status(""), 2500);
    }
  } catch (err) {
    if (ticket === loading) status(`Can't load ${label}: ${err instanceof Error ? err.message : err}`, true);
  }
}
function resetView() {
  orbit.yaw = 0.6;
  orbit.pitch = 0.25;
  orbit.distance = (orbit.radius / Math.sin(((orbit.fov / 2) * Math.PI) / 180)) * 1.15;
}

const modelSelect = $("model");
for (const [name, label] of MODELS) modelSelect.add(new Option(label, name));
modelSelect.addEventListener("change", () => {
  const label = modelSelect.selectedOptions[0].text;
  load(glb(modelSelect.value), label);
});
$("load-url").addEventListener("click", () => {
  const url = $("url").value.trim();
  if (url) load(url, url.split("/").pop() ?? url);
});
$("open-file").addEventListener("click", () => $("file").click());
$("file").addEventListener("change", () => openFile($("file").files?.[0]));
function openFile(/** @type {File | undefined} */ file) {
  if (!file) return;
  // A .glb is self-contained: an object URL of it is all the loader needs.
  load(URL.createObjectURL(file), file.name);
}
addEventListener("dragover", (e) => {
  e.preventDefault();
  document.body.classList.add("dragging");
});
addEventListener("dragleave", () => document.body.classList.remove("dragging"));
addEventListener("drop", (e) => {
  e.preventDefault();
  document.body.classList.remove("dragging");
  openFile(e.dataTransfer?.files?.[0]);
});

// --- environment ---------------------------------------------------------------

let envIntensity = 1;
async function loadEnv(/** @type {string} */ name, /** @type {string} */ label) {
  status(`Loading ${label}…`);
  try {
    const bytes = new Uint8Array(await (await fetch(`${HDRS}/${name}.hdr`)).arrayBuffer());
    scene.setEnvironmentHdr(bytes, 256);
    scene.setEnvironmentIntensity(envIntensity, envIntensity, envIntensity);
    status("");
  } catch (err) {
    status(`Can't load ${label}: ${err instanceof Error ? err.message : err}`, true);
  }
}
const envSelect = $("env");
for (const [name, label] of ENVS) envSelect.add(new Option(label, name));
envSelect.addEventListener("change", () => loadEnv(envSelect.value, envSelect.selectedOptions[0].text));

// --- controls ------------------------------------------------------------------

// A range input with its value shown beside it.
function slider(/** @type {string} */ id, /** @type {(v: number) => string} */ show, /** @type {(v: number) => void} */ apply) {
  const input = $(id);
  const out = $(`${id}-v`);
  const update = () => {
    const v = Number(input.value);
    if (out) out.textContent = show(v);
    apply(v);
  };
  input.addEventListener("input", update);
  update();
}

const shading = $("shading");
const setShading = (/** @type {string} */ m) => {
  scene.setShadingModel(m);
  for (const b of shading.children) b.classList.toggle("on", b.dataset.v === m);
};
shading.addEventListener("click", (/** @type {MouseEvent} */ e) => {
  const v = /** @type {HTMLElement} */ (e.target).dataset?.v;
  if (v) setShading(v);
});
setShading("openpbr");

$("tone").addEventListener("change", () => scene.setToneMapping($("tone").value));
slider("exposure", (v) => `${v >= 0 ? "+" : ""}${v.toFixed(1)} EV`, (v) => scene.setExposure(2 ** v));
$("hdr").addEventListener("change", () => {
  $("hdr").checked = scene.setHdr($("hdr").checked);
});
$("samples").addEventListener("change", () => scene.setSamples(Number($("samples").value)));
$("resolution").addEventListener("change", () => {
  resolution = $("resolution").value;
  resize();
});
// Variance; the threshold follows it from Filament's 0.2 up to 0.5.
slider("spec-aa", (v) => v.toFixed(2), (v) => scene.setSpecularAntialiasing(v, Math.max(0.2, Math.min(0.5, v))));
$("dither").addEventListener("change", () => scene.setDither($("dither").checked));
$("reverse-z").addEventListener("change", () => {
  $("reverse-z").checked = scene.setReverseZ($("reverse-z").checked);
  setCameraLens();
});

slider("env-intensity", (v) => v.toFixed(2), (v) => {
  envIntensity = v;
  scene.setEnvironmentIntensity(v, v, v);
});
slider("env-rotation", (v) => `${v}°`, (v) => scene.setEnvironmentRotation((v * Math.PI) / 180));
let blur = 0.3;
const applyBackground = () => ($("background").checked ? scene.setBackground(blur) : scene.clearBackground());
$("background").addEventListener("change", applyBackground);
slider("blur", (v) => v.toFixed(2), (v) => {
  blur = v;
  applyBackground();
});
slider("light", (v) => v.toFixed(1), (v) => {
  lightIntensity = v;
  setLight();
});
slider("fov", (v) => `${v}°`, (v) => {
  orbit.fov = v;
  setCameraLens();
});
$("reset-view").addEventListener("click", resetView);
$("lose").addEventListener("click", () => {
  if (ctx.simulateContextLoss()) {
    status("Context lost — restoring in a second…");
    setTimeout(() => {
      ctx.simulateContextRestore();
      status("");
    }, 1000);
  }
});
$("toggle").addEventListener("click", () => $("panel").classList.toggle("hidden"));
if (innerWidth < 700) $("panel").classList.add("hidden");

// --- material inspector ----------------------------------------------------------

// OpenPBR parameters worth a slider: [name, min, max].
const SCALARS = [
  ["base_weight", 0, 1],
  ["base_metalness", 0, 1],
  ["base_diffuse_roughness", 0, 1],
  ["specular_weight", 0, 1],
  ["specular_roughness", 0, 1],
  ["specular_ior", 1, 3],
  ["specular_roughness_anisotropy", 0, 1],
  ["transmission_weight", 0, 1],
  ["coat_weight", 0, 1],
  ["coat_roughness", 0, 1],
  ["coat_ior", 1, 3],
  ["coat_darkening", 0, 1],
  ["fuzz_weight", 0, 1],
  ["fuzz_roughness", 0, 1],
  ["thin_film_weight", 0, 1],
  ["thin_film_thickness", 0, 1],
  ["thin_film_ior", 1, 3],
  ["emission_luminance", 0, 10],
  ["geometry_opacity", 0, 1],
];
const COLORS = ["base_color", "specular_color", "coat_color", "fuzz_color", "transmission_color", "emission_color"];

// Color pickers speak sRGB; materials, linear.
const toLinear = (/** @type {number} */ c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const toSrgb = (/** @type {number} */ c) => (c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055);
const hex = (/** @type {ArrayLike<number>} */ rgb) =>
  "#" + [0, 1, 2].map((i) => Math.round(Math.min(1, Math.max(0, toSrgb(rgb[i]))) * 255).toString(16).padStart(2, "0")).join("");
const unhex = (/** @type {string} */ h) => [1, 3, 5].map((i) => toLinear(parseInt(h.slice(i, i + 2), 16) / 255));

async function pickAt(/** @type {PointerEvent} */ e) {
  const rect = canvas.getBoundingClientRect();
  const x = ((e.clientX - rect.left) / rect.width) * 2 - 1;
  const y = 1 - ((e.clientY - rect.top) / rect.height) * 2;
  const hit = await scene.pick(camera, x, y);
  if (!hit) return inspect(0, "");
  inspect(hit.entity, scene.name(scene.parent(hit.entity)) ?? `entity ${hit.entity}`);
}

function inspect(/** @type {number} */ part, /** @type {string} */ partName) {
  const box = $("inspector");
  box.textContent = "";
  if (!part) {
    box.innerHTML = '<div class="note">Tap a part of the model.</div>';
    return;
  }
  const material = scene.meshMaterial(part);
  const title = document.createElement("div");
  title.innerHTML = `<b>${partName}</b><div class="note">material: ${scene.materialName(material) ?? `#${material}`} — edits change every part that shares it</div>`;
  box.append(title);
  const own = document.createElement("button");
  own.textContent = "Give this part its own copy";
  own.onclick = () => {
    const copy = scene.cloneMaterial(material);
    scene.setMaterial(part, copy);
    inspect(part, partName);
  };
  box.append(own);
  for (const name of COLORS) {
    const value = scene.materialProperty(material, name);
    if (!value) continue;
    const row = document.createElement("label");
    row.className = "row";
    row.innerHTML = `<span>${name}</span>`;
    const input = document.createElement("input");
    input.type = "color";
    input.value = hex(value);
    input.oninput = () => scene.setMaterialProperty(material, name, unhex(input.value));
    row.append(input, document.createElement("span"));
    box.append(row);
  }
  for (const [name, min, max] of SCALARS) {
    const value = scene.materialProperty(material, /** @type {string} */ (name));
    if (!value) continue;
    const row = document.createElement("label");
    row.className = "row";
    row.innerHTML = `<span>${name}</span>`;
    const input = document.createElement("input");
    input.type = "range";
    input.min = String(min);
    input.max = String(max);
    input.step = "0.01";
    input.value = String(value[0]);
    const out = document.createElement("output");
    out.textContent = value[0].toFixed(2);
    input.oninput = () => {
      out.textContent = Number(input.value).toFixed(2);
      scene.setMaterialProperty(material, /** @type {string} */ (name), Number(input.value));
    };
    row.append(input, out);
    box.append(row);
  }
  $("inspector-box").open = true;
}

// --- frame loop and stats ----------------------------------------------------------

const graph = /** @type {CanvasRenderingContext2D} */ ($("graph").getContext("2d"));
const frameTimes = /** @type {number[]} */ ([]); // ms between frames, newest last
const cpuTimes = /** @type {number[]} */ ([]);
let last = -1; // no frame yet
let fpsShown = 0;
let lastStats = 0;

function frame(/** @type {number} */ now) {
  const dt = last < 0 ? 16.7 : Math.max(0, now - last);
  last = now;
  frameTimes.push(dt);
  if (frameTimes.length > 180) frameTimes.shift();
  if ($("spin").checked) orbit.yaw += dt * 0.0003;
  resize();
  placeCamera();
  const t0 = performance.now();
  scene.renderFrame(camera);
  cpuTimes.push(performance.now() - t0);
  if (cpuTimes.length > 60) cpuTimes.shift();

  if (now - lastStats > 250) {
    lastStats = now;
    const recent = frameTimes.slice(-60);
    const avg = recent.reduce((a, b) => a + b, 0) / recent.length;
    const worst = Math.max(...recent);
    const cpu = cpuTimes.reduce((a, b) => a + b, 0) / cpuTimes.length;
    fpsShown = 1000 / avg;
    $("fps").textContent = fpsShown.toFixed(0);
    $("fps").style.color = fpsShown >= 55 ? "var(--good)" : fpsShown >= 30 ? "var(--warn)" : "var(--bad)";
    $("frame").textContent = `frame ${avg.toFixed(1)} ms (worst ${worst.toFixed(0)})  cpu ${cpu.toFixed(2)} ms`;
    $("info").textContent =
      `\n${canvas.width}×${canvas.height}  ${scene.isHdr() ? "HDR" : "direct"}  ${scene.shadingModel()}\n` +
      `drawn ${scene.drawn()}  culled ${scene.culled()}  pending ${scene.pending()}\n` +
      `variants ${scene.shaderVariantCount()}  entities ${scene.entityCount()}`;
    // Frame times, 60 fps line at the middle.
    const g = graph;
    g.clearRect(0, 0, 180, 36);
    g.fillStyle = "rgba(255,255,255,0.08)";
    g.fillRect(0, 18, 180, 1);
    frameTimes.forEach((t, i) => {
      const hgt = Math.min(36, (t / 33.3) * 36);
      g.fillStyle = t <= 17.5 ? "#6fd08c" : t <= 34 ? "#e8b85c" : "#e86c6c";
      g.fillRect(i, 36 - hgt, 1, hgt);
    });
  }
  requestAnimationFrame(frame);
}

// Start: a model (from ?model=<url> or the first sample) and an environment.
const params = new URLSearchParams(location.search);
setCameraLens();
placeCamera();
applyBackground();
requestAnimationFrame(frame);
const url = params.get("model");
await Promise.all([
  loadEnv(ENVS[0][0], ENVS[0][1]),
  url ? load(url, url.split("/").pop() ?? url) : load(glb(MODELS[0][0]), MODELS[0][1]),
]);
