// @ts-check
// A glTF model in a scene, driven from plain JS. The scene owns the entities
// (plain numbers), their transforms and the draw list; JS only says what goes
// where. One `scene.render(camera, pass)` per frame updates world matrices,
// culls, and appends the draws.
//
// Build the bundle first (see README.md), then serve this folder.

import init, { ClearMask, Context, ContextOptions, Pass, Scene, loadModel } from "./pkg/rswebgl_scene.js";

const MODEL_URL =
  "https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Models/main/2.0/Avocado/glTF-Binary/Avocado.glb";

// Matrices come from the scene: the "Frame" block (camera) and u_model /
// u_normal_matrix (object). Material values follow the loader's convention.
const VERT = `#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec2 a_uv;
layout(std140) uniform Frame {
  mat4 u_view;
  mat4 u_proj;
  mat4 u_view_proj;
  vec4 u_camera_pos;
};
uniform mat4 u_model;
uniform mat3 u_normal_matrix;
out vec3 v_normal;
out vec3 v_world;
out vec2 v_uv;
void main() {
  vec4 world = u_model * vec4(a_pos, 1.0);
  v_world = world.xyz;
  v_normal = u_normal_matrix * a_normal;
  v_uv = a_uv;
  gl_Position = u_view_proj * world;
}`;

const FRAG = `#version 300 es
precision highp float;
in vec3 v_normal;
in vec3 v_world;
in vec2 v_uv;
uniform vec4 u_base_color_factor;
uniform bool u_has_base_color_tex;
uniform sampler2D u_base_color_tex;
uniform float u_alpha_cutoff;
uniform int u_alpha_mode;
out vec4 frag_color;
void main() {
  vec4 base = u_base_color_factor;
  if (u_has_base_color_tex) base *= texture(u_base_color_tex, v_uv);
  if (u_alpha_mode == 1 && base.a < u_alpha_cutoff) discard;
  // A primitive without normals reads (0,0,0): fall back to the face normal.
  vec3 n = dot(v_normal, v_normal) > 1e-8
    ? normalize(v_normal)
    : normalize(cross(dFdx(v_world), dFdy(v_world)));
  if (!gl_FrontFacing) n = -n;
  float ndl = max(dot(n, normalize(vec3(0.4, 0.8, 0.6))), 0.0);
  // sRGB textures sample as linear: encode for the canvas (tone mapping
  // comes with PBR).
  vec3 linear = base.rgb * (0.25 + 0.75 * ndl);
  frag_color = vec4(pow(linear, vec3(1.0 / 2.2)), base.a);
}`;

await init();

const canvas = /** @type {HTMLCanvasElement} */ (document.getElementById("c"));
const statsEl = /** @type {HTMLElement} */ (document.getElementById("stats"));
const ctx = Context.fromCanvas(canvas, new ContextOptions());
const fb = ctx.defaultFramebuffer();
fb.setClearColor(0.08, 0.08, 0.1, 1);
fb.enableAutoResize();

const scene = new Scene(ctx);
const program = ctx.createProgram(VERT, FRAG);

const url = new URLSearchParams(location.search).get("model") ?? MODEL_URL;
statsEl.textContent = `loading ${url}…`;
// The scene takes the model over: its GPU data goes with the root entity.
const root = scene.addModel(await loadModel(ctx, url), program);

// Frame the model: a camera looking at its bounding sphere.
const [cx, cy, cz, radius] = scene.modelBounds(root);
const camera = scene.spawn();
scene.setPerspectiveCamera(camera, Math.PI / 4, radius * 0.05, radius * 20);
const distance = radius / Math.sin(Math.PI / 8) * 1.1;

// A pivot at the model's center carries the turntable rotation; the model
// hangs off it, moved so its center sits on the pivot.
const pivot = scene.spawn();
scene.setTranslation(pivot, cx, cy, cz);
scene.setParent(root, pivot);
scene.setTranslation(root, -cx, -cy, -cz);

const pass = Pass.toDefault();
pass.setClear(ClearMask.colorDepth());

let visible = true;
let spinning = true;
let angle = 0;
window.addEventListener("keydown", (e) => {
  if (e.key === "v" || e.key === "V") scene.setVisible(root, (visible = !visible));
  if (e.key === "r" || e.key === "R") spinning = !spinning;
});

let last = performance.now();
function frame(now) {
  if (spinning) angle += (now - last) / 1000 * 0.6;
  last = now;
  scene.setRotationEuler(pivot, 0, angle, 0);
  scene.lookAt(camera, cx, cy + radius * 0.6, cz + distance, cx, cy, cz);
  scene.setCameraAspect(camera, fb.width() / Math.max(1, fb.height()));

  pass.clearDraws();
  scene.render(camera, pass);
  pass.sort();
  ctx.render(pass);

  statsEl.textContent =
    `entities ${scene.entityCount()}  drawn ${scene.drawn()}  culled ${scene.culled()}  ` +
    `matrices written ${scene.objectsWritten()}\n` +
    `V: ${visible ? "hide" : "show"} model   R: ${spinning ? "pause" : "resume"}`;
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
