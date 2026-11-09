// @ts-check
// Uniform upload benchmark, in raw WebGL2: how a device's driver takes the
// ways of getting per-draw uniform data to the GPU. Every case draws the
// same N quads, each with 80 bytes of its own (a mat4 and a vec4, std140),
// and changes that data each frame differently. Recorded with the library's
// profiler (`Trace`), so the report opens in Perfetto like the viewer's.
//
// The open question it answers (TODO.md, "Uniform uploads on mobile"):
// many small bufferSubData writes against one larger upload, and whether
// per-draw data should live in one shared UBO (bindBufferRange per draw)
// instead of plain uniforms.

import init, { Trace } from "./pkg/rswebgl_scene.js";

await init();

const $ = (/** @type {string} */ id) => /** @type {any} */ (document.getElementById(id));
const canvas = /** @type {HTMLCanvasElement} */ ($("c"));
const gl = /** @type {WebGL2RenderingContext} */ (canvas.getContext("webgl2", { antialias: false }));

const VERT_BLOCK = `#version 300 es
layout(std140) uniform Object { mat4 u_model; vec4 u_color; };
out vec4 v_color;
void main() {
  vec2 p = vec2(gl_VertexID & 1, gl_VertexID >> 1) - 0.5;
  gl_Position = u_model * vec4(p, 0.0, 1.0);
  v_color = u_color;
}`;
const VERT_UNIFORMS = `#version 300 es
uniform mat4 u_model;
uniform vec4 u_color;
out vec4 v_color;
void main() {
  vec2 p = vec2(gl_VertexID & 1, gl_VertexID >> 1) - 0.5;
  gl_Position = u_model * vec4(p, 0.0, 1.0);
  v_color = u_color;
}`;
const FRAG = `#version 300 es
precision mediump float;
in vec4 v_color;
out vec4 color;
void main() { color = v_color; }`;

function program(/** @type {string} */ vs, /** @type {string} */ fs) {
  const p = /** @type {WebGLProgram} */ (gl.createProgram());
  for (const [type, src] of [[gl.VERTEX_SHADER, vs], [gl.FRAGMENT_SHADER, fs]]) {
    const s = /** @type {WebGLShader} */ (gl.createShader(/** @type {number} */ (type)));
    gl.shaderSource(s, /** @type {string} */ (src));
    gl.compileShader(s);
    gl.attachShader(p, s);
  }
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p) ?? "link failed");
  return p;
}
const progBlock = program(VERT_BLOCK, FRAG);
gl.uniformBlockBinding(progBlock, gl.getUniformBlockIndex(progBlock, "Object"), 0);
const progUniforms = program(VERT_UNIFORMS, FRAG);
const locModel = gl.getUniformLocation(progUniforms, "u_model");
const locColor = gl.getUniformLocation(progUniforms, "u_color");
gl.bindVertexArray(gl.createVertexArray()); // no attributes: positions from gl_VertexID

const ALIGN = /** @type {number} */ (gl.getParameter(gl.UNIFORM_BUFFER_OFFSET_ALIGNMENT));
const BYTES = 80; // std140: mat4 + vec4
const STRIDE = Math.ceil(BYTES / ALIGN) * ALIGN; // bindBufferRange offsets are aligned
const STRIDE_F = STRIDE / 4;

function gpuName() {
  const info = gl.getExtension("WEBGL_debug_renderer_info");
  return info ? `${gl.getParameter(info.UNMASKED_VENDOR_WEBGL)} / ${gl.getParameter(info.UNMASKED_RENDERER_WEBGL)}` : "unknown";
}

// --- per-draw data --------------------------------------------------------

let n = 0; // draws
let grid = 1;
/** @type {Float32Array} */
let big = new Float32Array(0); // CPU copy of the big buffer, STRIDE per draw
const scratch = new Float32Array(20);
const scratchModel = scratch.subarray(0, 16);
const scratchColor = scratch.subarray(16, 20);
/** @type {WebGLBuffer[]} */
let bigBuffers = []; // [0] the big buffer; [0..3] the ring
const small = /** @type {WebGLBuffer} */ (gl.createBuffer());
gl.bindBuffer(gl.UNIFORM_BUFFER, small);
gl.bufferData(gl.UNIFORM_BUFFER, BYTES, gl.DYNAMIC_DRAW);

// Draw i's data at time t into `dst` from float `at`: a quad in its grid
// cell, turning, and a color.
function fill(/** @type {Float32Array} */ dst, /** @type {number} */ at, /** @type {number} */ i, /** @type {number} */ t) {
  const cell = 2 / grid;
  const x = -1 + cell * ((i % grid) + 0.5);
  const y = -1 + cell * (Math.floor(i / grid) + 0.5);
  const a = t * 1.5 + i * 0.37;
  const k = cell * 0.7;
  const c = Math.cos(a) * k;
  const s = Math.sin(a) * k;
  dst[at] = c; dst[at + 1] = s; dst[at + 2] = 0; dst[at + 3] = 0;
  dst[at + 4] = -s; dst[at + 5] = c; dst[at + 6] = 0; dst[at + 7] = 0;
  dst[at + 8] = 0; dst[at + 9] = 0; dst[at + 10] = 1; dst[at + 11] = 0;
  dst[at + 12] = x; dst[at + 13] = y; dst[at + 14] = 0; dst[at + 15] = 1;
  dst[at + 16] = 0.5 + 0.5 * Math.sin(a);
  dst[at + 17] = 0.5 + 0.5 * Math.sin(a + 2.1);
  dst[at + 18] = 0.5 + 0.5 * Math.sin(a + 4.2);
  dst[at + 19] = 1;
}

function setup(/** @type {number} */ draws) {
  n = draws;
  grid = Math.ceil(Math.sqrt(n));
  big = new Float32Array(n * STRIDE_F);
  for (const b of bigBuffers) gl.deleteBuffer(b);
  bigBuffers = [0, 1, 2].map(() => {
    const b = /** @type {WebGLBuffer} */ (gl.createBuffer());
    gl.bindBuffer(gl.UNIFORM_BUFFER, b);
    gl.bufferData(gl.UNIFORM_BUFFER, big.byteLength, gl.DYNAMIC_DRAW);
    return b;
  });
  for (let i = 0; i < n; i++) fill(big, i * STRIDE_F, i, 0);
  for (const b of bigBuffers) {
    gl.bindBuffer(gl.UNIFORM_BUFFER, b);
    gl.bufferSubData(gl.UNIFORM_BUFFER, 0, big);
  }
}

// Draws every quad from `buffer`, a range per draw.
function drawRanges(/** @type {WebGLBuffer} */ buffer) {
  for (let i = 0; i < n; i++) {
    gl.bindBufferRange(gl.UNIFORM_BUFFER, 0, buffer, i * STRIDE, BYTES);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
  }
}

// The changing tenth: draws i with i % 10 == frame % 10 (scattered, so a
// write per draw can't merge).
const changes = (/** @type {number} */ i, /** @type {number} */ frame) => i % 10 === frame % 10;

/** @type {{ name: string, frame: (t: number, frame: number) => void }[]} */
const CASES = [
  {
    name: "uniforms: uniform* per draw",
    frame(t) {
      gl.useProgram(progUniforms);
      for (let i = 0; i < n; i++) {
        fill(scratch, 0, i, t);
        gl.uniformMatrix4fv(locModel, false, scratchModel);
        gl.uniform4fv(locColor, scratchColor);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      }
    },
  },
  {
    name: "ubo small: write before each draw",
    frame(t) {
      gl.useProgram(progBlock);
      gl.bindBufferBase(gl.UNIFORM_BUFFER, 0, small);
      for (let i = 0; i < n; i++) {
        fill(scratch, 0, i, t);
        gl.bufferSubData(gl.UNIFORM_BUFFER, 0, scratch);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      }
    },
  },
  {
    name: "ubo big: one upload",
    frame(t) {
      gl.useProgram(progBlock);
      for (let i = 0; i < n; i++) fill(big, i * STRIDE_F, i, t);
      gl.bindBuffer(gl.UNIFORM_BUFFER, bigBuffers[0]);
      gl.bufferSubData(gl.UNIFORM_BUFFER, 0, big);
      drawRanges(bigBuffers[0]);
    },
  },
  {
    name: "ubo big: a write per draw",
    frame(t) {
      gl.useProgram(progBlock);
      gl.bindBuffer(gl.UNIFORM_BUFFER, bigBuffers[0]);
      for (let i = 0; i < n; i++) {
        fill(big, i * STRIDE_F, i, t);
        gl.bufferSubData(gl.UNIFORM_BUFFER, i * STRIDE, big, i * STRIDE_F, 20);
      }
      drawRanges(bigBuffers[0]);
    },
  },
  {
    name: "ubo ring of 3: one upload",
    frame(t, frame) {
      gl.useProgram(progBlock);
      const b = bigBuffers[frame % 3];
      for (let i = 0; i < n; i++) fill(big, i * STRIDE_F, i, t);
      gl.bindBuffer(gl.UNIFORM_BUFFER, b);
      gl.bufferSubData(gl.UNIFORM_BUFFER, 0, big);
      drawRanges(b);
    },
  },
  {
    name: "ubo big: orphan + one upload",
    frame(t) {
      gl.useProgram(progBlock);
      for (let i = 0; i < n; i++) fill(big, i * STRIDE_F, i, t);
      gl.bindBuffer(gl.UNIFORM_BUFFER, bigBuffers[0]);
      gl.bufferData(gl.UNIFORM_BUFFER, big, gl.DYNAMIC_DRAW);
      drawRanges(bigBuffers[0]);
    },
  },
  {
    name: "10% change: one upload of all",
    frame(t, frame) {
      gl.useProgram(progBlock);
      for (let i = 0; i < n; i++) if (changes(i, frame)) fill(big, i * STRIDE_F, i, t);
      gl.bindBuffer(gl.UNIFORM_BUFFER, bigBuffers[0]);
      gl.bufferSubData(gl.UNIFORM_BUFFER, 0, big);
      drawRanges(bigBuffers[0]);
    },
  },
  {
    name: "10% change: a write per changed",
    frame(t, frame) {
      gl.useProgram(progBlock);
      gl.bindBuffer(gl.UNIFORM_BUFFER, bigBuffers[0]);
      for (let i = 0; i < n; i++) {
        if (!changes(i, frame)) continue;
        fill(big, i * STRIDE_F, i, t);
        gl.bufferSubData(gl.UNIFORM_BUFFER, i * STRIDE, big, i * STRIDE_F, 20);
      }
      drawRanges(bigBuffers[0]);
    },
  },
  {
    name: "10% change: uniform* for all",
    frame(t, frame) {
      // Plain uniforms can't keep unchanged data per draw: all are set.
      gl.useProgram(progUniforms);
      for (let i = 0; i < n; i++) {
        if (changes(i, frame)) fill(big, i * STRIDE_F, i, t);
        gl.uniformMatrix4fv(locModel, false, big, i * STRIDE_F, 16);
        gl.uniform4fv(locColor, big, i * STRIDE_F + 16, 4);
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
      }
    },
  },
];

// --- run ------------------------------------------------------------------------

/** @type {Map<string, { submit: number[], frame: number[], gpu: number[] }>} */
let stats = new Map();
/** @type {{ sync: WebGLSync, set: number, name: string }[]} */
let fences = [];
let report = "";
let reportName = "";

// Fences passed since last time: their latency to the case that set them.
function pollFences(/** @type {boolean} */ record) {
  const now = performance.now();
  while (fences.length) {
    const f = fences[0];
    const s = gl.clientWaitSync(f.sync, 0, 0);
    if (s !== gl.ALREADY_SIGNALED && s !== gl.CONDITION_SATISFIED) break;
    fences.shift();
    gl.deleteSync(f.sync);
    Trace.complete("gpu frame", f.set, now - f.set, 2);
    if (record) stats.get(f.name)?.gpu.push(now - f.set);
  }
}

function resize() {
  const w = Math.round(canvas.clientWidth * devicePixelRatio);
  const h = Math.round(canvas.clientHeight * devicePixelRatio);
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  gl.viewport(0, 0, w, h);
}

const nextFrame = () => new Promise((r) => requestAnimationFrame(r));

async function runCase(/** @type {typeof CASES[number]} */ c, /** @type {number} */ seconds) {
  const s = stats.get(c.name) ?? { submit: [], frame: [], gpu: [] };
  stats.set(c.name, s);
  const warmup = 1000;
  let start = -1;
  let last = -1;
  let frame = 0;
  for (;;) {
    const now = await nextFrame();
    if (start < 0) start = now;
    const elapsed = now - start;
    if (elapsed > warmup + seconds * 1000) break;
    const measuring = elapsed > warmup;
    pollFences(true);
    Trace.begin(c.name);
    if (last >= 0) Trace.counter("frame interval (ms)", now - last);
    if (measuring && last >= 0) s.frame.push(now - last);
    last = now;
    const t0 = performance.now();
    gl.clearColor(0.05, 0.05, 0.06, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    c.frame(now / 1000, frame++);
    const sync = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
    gl.flush();
    const submit = performance.now() - t0;
    if (sync) fences.push({ sync, set: t0, name: measuring ? c.name : "" });
    if (measuring) s.submit.push(submit);
    Trace.end();
  }
  // Let its fences pass before the next case.
  for (let i = 0; i < 30 && fences.length; i++) {
    await nextFrame();
    pollFences(true);
  }
}

const mean = (/** @type {number[]} */ xs) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : NaN);
const p95 = (/** @type {number[]} */ xs) => {
  if (!xs.length) return NaN;
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(0.95 * s.length))];
};
const f2 = (/** @type {number} */ v) => (Number.isFinite(v) ? v.toFixed(2) : "—");

function results() {
  return [...stats].map(([name, s]) => ({
    name,
    submitMean: mean(s.submit),
    submitP95: p95(s.submit),
    frameMean: mean(s.frame),
    frameP95: p95(s.frame),
    gpuMean: mean(s.gpu),
  }));
}

function showResults() {
  const rows = results();
  $("results").innerHTML =
    "<tr><th>case</th><th>submit</th><th>p95</th><th>frame</th><th>p95</th><th>gpu</th></tr>" +
    rows
      .map((r) => `<tr><td>${r.name}</td><td>${f2(r.submitMean)}</td><td>${f2(r.submitP95)}</td><td>${f2(r.frameMean)}</td><td>${f2(r.frameP95)}</td><td>${f2(r.gpuMean)}</td></tr>`)
      .join("");
}

$("run").onclick = async () => {
  const draws = Number($("draws").value);
  const seconds = Number($("seconds").value);
  for (const id of ["run", "download", "send", "draws", "seconds"]) $(id).disabled = true;
  resize();
  setup(draws);
  stats = new Map();
  $("results").innerHTML = "";
  const meta = {
    userAgent: navigator.userAgent,
    gpu: gpuName(),
    devicePixelRatio: String(devicePixelRatio),
    canvas: `${canvas.width}x${canvas.height}`,
    draws: String(draws),
    secondsPerCase: String(seconds),
    uniformBufferOffsetAlignment: String(ALIGN),
    stride: String(STRIDE),
    date: new Date().toISOString(),
  };
  for (const [k, v] of Object.entries(meta)) Trace.metadata(k, v);
  Trace.start();
  // Fences checked between frames too: "gpu" finer than a frame.
  const poller = setInterval(() => pollFences(true), 2);
  // Twice, the second time in reverse: drift (heat, clocks) evens out.
  const order = [...CASES, ...[...CASES].reverse()];
  for (const [k, c] of order.entries()) {
    $("status").textContent = `Running ${k + 1}/${order.length}: ${c.name} (${draws} draws)…`;
    Trace.instant(`case: ${c.name}`);
    await runCase(c, seconds);
  }
  clearInterval(poller);
  Trace.metadata("results", JSON.stringify(results()));
  Trace.stop();
  report = Trace.report();
  reportName = `rswebgl-ubo-bench-${draws}-${meta.date.replace(/[:.]/g, "-")}.json`;
  showResults();
  $("status").textContent = `Done: ${draws} draws, align ${ALIGN} (stride ${STRIDE} bytes), ${meta.gpu}. Times in ms.`;
  for (const id of ["run", "download", "send", "draws", "seconds"]) $(id).disabled = false;
};

$("download").onclick = () => {
  const a = document.createElement("a");
  a.href = URL.createObjectURL(new Blob([report], { type: "application/json" }));
  a.download = reportName;
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
};
$("send").onclick = async () => {
  try {
    const res = await fetch(`trace?name=${encodeURIComponent(reportName)}`, { method: "POST", body: report });
    $("status").textContent += res.ok ? `\nSaved on the computer: ${await res.text()}` : `\nNot saved: HTTP ${res.status}`;
  } catch (e) {
    $("status").textContent += `\nNot sent: ${e}`;
  }
};

addEventListener("resize", resize);
resize();
