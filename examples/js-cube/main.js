// @ts-check
// A grid of spinning cubes driven entirely from JS: the generated bindings,
// no Rust code of its own. Two programs and two render states, interleaved on
// purpose: sorting the draw list groups them, and the frame stats show it.
// Rendered through a 4× MSAA framebuffer resolved into the canvas.
//
// Build the bindings first (see README.md), then serve this folder.

import init, {
  AttrKind,
  ClearMask,
  Context,
  ContextOptions,
  CullFace,
  DrawCommand,
  DrawMode,
  IndexType,
  InvalidateMask,
  ParamLayoutBuilder,
  Pass,
  RenderState,
  RenderbufferFormat,
  UniformType,
  VaoBuilder,
} from "./pkg/rswebgl.js";

const VERT = `#version 300 es
precision highp float;
layout(location = 0) in vec3 a_pos;
layout(location = 1) in vec3 a_color;
uniform mat4 u_mvp;
out vec3 v_color;
void main() {
  gl_Position = u_mvp * vec4(a_pos, 1.0);
  v_color = a_color;
}`;

const FRAG_COLOR = `#version 300 es
precision highp float;
in vec3 v_color;
out vec4 frag_color;
void main() { frag_color = vec4(v_color, 1.0); }`;

const FRAG_TINT = `#version 300 es
precision highp float;
in vec3 v_color;
uniform vec3 u_tint;
out vec4 frag_color;
void main() { frag_color = vec4(mix(v_color, u_tint, 0.6), 1.0); }`;

const GRID = 12; // GRID × GRID cubes
const NULL_ID = 0;

await init();

const canvas = /** @type {HTMLCanvasElement} */ (document.getElementById("c"));
// No built-in antialiasing: MSAA comes from our own target (M toggles it).
const options = new ContextOptions();
options.antialias = false;
const ctx = Context.fromCanvas(canvas, options);
const fb = ctx.defaultFramebuffer();
fb.setClearColor(0.07, 0.07, 0.09, 1);
fb.enableAutoResize();

// --- resources: plain numbers from here on ---------------------------------

// GPU objects go with a lost context (programs, vertex arrays, buffers,
// framebuffers); `createGpu` makes them, at start and again on restore.
// Parameter groups and render states are CPU-side: they survive.
let colorProgram = NULL_ID;
let tintProgram = NULL_ID;
let cube = NULL_ID;
function createGpu() {
  colorProgram = ctx.createProgram(VERT, FRAG_COLOR);
  tintProgram = ctx.createProgram(VERT, FRAG_TINT);
  cube = buildCube();
}

function buildCube() {
  const vb = new VaoBuilder(ctx);
  // prettier-ignore
  vb.addF32(0, AttrKind.F32x3, new Float32Array([
    -.5, -.5, -.5,  .5, -.5, -.5,  .5, .5, -.5,  -.5, .5, -.5,
    -.5, -.5,  .5,  .5, -.5,  .5,  .5, .5,  .5,  -.5, .5,  .5,
  ]));
  // prettier-ignore
  vb.addF32(1, AttrKind.F32x3, new Float32Array([
    0, 0, 0,  1, 0, 0,  1, 1, 0,  0, 1, 0,
    0, 0, 1,  1, 0, 1,  1, 1, 1,  0, 1, 1,
  ]));
  // prettier-ignore
  vb.addIndicesU16(new Uint16Array([
    0, 2, 1, 0, 3, 2,  4, 5, 6, 4, 6, 7,  0, 4, 7, 0, 7, 3,
    1, 2, 6, 1, 6, 5,  0, 1, 5, 0, 5, 4,  3, 7, 6, 3, 6, 2,
  ]));
  // `build()` consumes the builder (it is freed); the result is a plain number.
  return vb.build();
}
createGpu();

// Render states are interned: build once, keep the number.
const solid = new RenderState();
solid.depthTest = true;
solid.cullFace = true;
solid.cullMode = CullFace.Back;
const solidState = ctx.renderState(solid);
const wire = new RenderState();
wire.depthTest = true; // no culling: inside faces show where the mix is visible
const openState = ctx.renderState(wire);

// Parameter layouts: resolve field names once, write by number every frame.
const objectLayout = ctx.createParamLayout(
  new ParamLayoutBuilder().uniform("u_mvp", UniformType.Mat4),
);
const fMvp = ctx.paramField(objectLayout, "u_mvp");
const tintLayout = ctx.createParamLayout(
  new ParamLayoutBuilder().uniform("u_tint", UniformType.Vec3),
);
const tint = ctx.createParamGroup(tintLayout);
ctx.paramSetFloats(tint, ctx.paramField(tintLayout, "u_tint"), new Float32Array([1, 0.5, 0.1]));

const draw = DrawCommand.elements(DrawMode.Triangles, 36, IndexType.UnsignedShort, 0);
const NULL = 0;

const objects = [];
for (let i = 0; i < GRID * GRID; i++) {
  const tinted = i % 2 === 1; // interleaved on purpose
  objects.push({
    group: ctx.createParamGroup(objectLayout),
    tinted,
    state: tinted ? openState : solidState,
    material: tinted ? tint : NULL,
    x: (i % GRID) - (GRID - 1) / 2,
    y: Math.floor(i / GRID) - (GRID - 1) / 2,
  });
}

// --- target: a 4× MSAA framebuffer resolved into the canvas, or the canvas -
// The resolve happens right after the draws and the samples are then
// discarded (invalidate), so on tiled mobile GPUs they never leave tile memory.

let msaa = true;
let msaaFb = NULL;
let msaaW = 0;
let msaaH = 0;
/** @type {Pass | null} */
let pass = null; // rebuilt every frame into the same memory; replaced with the target

function createMsaaTarget(w, h) {
  const fbo = ctx.createFramebuffer(w, h);
  const color = ctx.createRenderbufferMultisample(RenderbufferFormat.Rgba8, 4, w, h);
  const depth = ctx.createRenderbufferMultisample(RenderbufferFormat.Depth24, 4, w, h);
  ctx.framebufferAttachColorRenderbuffer(fbo, 0, color);
  ctx.framebufferAttachDepthRenderbuffer(fbo, depth);
  // The framebuffer holds them now: they go when it is destroyed.
  ctx.destroyRenderbuffer(color);
  ctx.destroyRenderbuffer(depth);
  ctx.framebufferSetColorClearF32(fbo, 0, 0.07, 0.07, 0.09, 1);
  return fbo;
}

// The pass for this frame: follows the canvas size and the MSAA toggle.
function framePass(w, h) {
  const stale = msaa ? msaaFb === NULL || msaaW !== w || msaaH !== h : msaaFb !== NULL;
  if (stale) {
    if (msaaFb !== NULL) ctx.destroyFramebuffer(msaaFb);
    msaaFb = msaa ? createMsaaTarget(w, h) : NULL;
    msaaW = w;
    msaaH = h;
    pass?.free();
    pass = null;
  }
  if (!pass) {
    pass = msaa ? new Pass(msaaFb) : Pass.toDefault();
    pass.setClear(ClearMask.colorDepth());
    if (msaa) {
      pass.setResolve(NULL);
      pass.setInvalidate(InvalidateMask.all());
    }
  }
  return pass;
}

// After a context loss and restore: the GPU objects again, the MSAA target
// and pass rebuilt at the next frame. (WEBGL_lose_context simulates one:
// press L.)
ctx.setOnRestored(() => {
  createGpu();
  msaaFb = NULL;
  pass?.free();
  pass = null;
});

// --- math (column-major, like GLSL) ----------------------------------------

function perspective(fovy, aspect, near, far) {
  const f = 1 / Math.tan(fovy / 2), nf = 1 / (near - far);
  // prettier-ignore
  return [f / aspect, 0, 0, 0,  0, f, 0, 0,  0, 0, (far + near) * nf, -1,  0, 0, 2 * far * near * nf, 0];
}

function mul(a, b, out) {
  for (let c = 0; c < 4; c++)
    for (let r = 0; r < 4; r++) {
      let s = 0;
      for (let k = 0; k < 4; k++) s += a[k * 4 + r] * b[c * 4 + k];
      out[c * 4 + r] = s;
    }
  return out;
}

function model(x, y, t, out) {
  const cy = Math.cos(t), sy = Math.sin(t), cx = Math.cos(t * 0.7), sx = Math.sin(t * 0.7);
  // rotation Y·X, translation (x, y, -GRID * 2.6)
  // prettier-ignore
  out.set([cy, 0, -sy, 0,  sy * sx, cx, cy * sx, 0,  sy * cx, -sx, cy * cx, 0,  x * 1.6, y * 1.6, -GRID * 2.6, 1]);
  return out;
}

// --- frame -----------------------------------------------------------------

const m = new Float32Array(16);
const mvp = new Float32Array(16);
const statsEl = /** @type {HTMLElement} */ (document.getElementById("stats"));
let sorted = true;
window.addEventListener("keydown", (e) => {
  if (e.key === "s" || e.key === "S") sorted = !sorted;
  if (e.key === "m" || e.key === "M") msaa = !msaa;
  if (e.key === "l" || e.key === "L") {
    ctx.simulateContextLoss();
    setTimeout(() => ctx.simulateContextRestore(), 500);
  }
});

function frame(ms) {
  const t = ms / 1000;
  const proj = perspective(Math.PI / 4, fb.width() / Math.max(1, fb.height()), 0.1, 100);

  const pass = framePass(fb.width(), fb.height());
  pass.clearDraws();
  for (const o of objects) {
    mul(proj, model(o.x, o.y, t + o.x * 0.3, m), mvp);
    ctx.paramSetFloats(o.group, fMvp, mvp);
    // Slots: 0 frame, 1 material, 2 object.
    pass.push(o.tinted ? tintProgram : colorProgram, o.state, cube, NULL, o.material, o.group, NULL, draw);
  }
  if (sorted) pass.sort();

  ctx.resetFrameStats();
  ctx.render(pass);
  const s = ctx.frameStats();
  statsEl.textContent =
    `draws ${s.draws}  (S: sort ${sorted ? "on" : "off"}, M: MSAA ${msaa ? "4x" : "off"}, L: lose the context)\n` +
    `programs ${s.programs}  states ${s.states}  vaos ${s.vertexArrays}\n` +
    `uniforms ${s.uniforms}  textures ${s.textures}  blocks ${s.blocks}`;
  s.free();
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
