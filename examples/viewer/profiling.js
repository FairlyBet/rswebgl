// @ts-check
// The viewer's Profiling panel: records a CPU trace (the library's spans,
// the viewer's own, frame intervals, GPU fences, the browser's long
// animation frames) for a few seconds, then offers it as a download or
// sends it to the computer serving the page (serve.mjs saves it under
// traces/). Open a report in https://ui.perfetto.dev.

import { Trace } from "./pkg/rswebgl_scene.js";

const TRACK_BROWSER = 3;

/**
 * @param {{
 *   wasm: { memory: WebAssembly.Memory },
 *   canvas: HTMLCanvasElement,
 *   $: (id: string) => any,
 *   describe: () => Record<string, string>,
 *   scene?: any,
 *   ctx?: any,
 *   viewCamera?: () => number,
 *   getRoot?: () => number,
 * }} opts
 */
export function createProfiling({ wasm, canvas, $, describe, scene, ctx, viewCamera = () => 0, getRoot = () => 0 }) {
  let until = 0; // performance.now() the recording ends at; 0 when idle
  let report = "";
  let reportName = "";
  /** @type {number[]} */
  let intervals = [];
  let lastFrame = -1;

  Trace.trackName(TRACK_BROWSER, "browser: long animation frames");
  // The browser's view of slow frames (Chrome 123+): where a frame's time
  // went, scripts and rendering included, even outside our code.
  try {
    new PerformanceObserver((list) => {
      if (!until) return;
      for (const e of /** @type {any[]} */ (list.getEntries())) {
        Trace.complete(`long frame (blocking ${e.blockingDuration.toFixed(0)} ms)`, e.startTime, e.duration, TRACK_BROWSER);
        for (const s of e.scripts ?? []) {
          Trace.complete(`script: ${s.invoker || s.name}`, s.startTime, s.duration, TRACK_BROWSER);
        }
        if (e.renderStart > 0) {
          Trace.complete("rendering (rAF, style, layout, paint)", e.renderStart, e.startTime + e.duration - e.renderStart, TRACK_BROWSER);
        }
      }
    }).observe({ type: "long-animation-frame", buffered: false });
  } catch {
    try {
      new PerformanceObserver((list) => {
        if (!until) return;
        for (const e of list.getEntries()) Trace.complete("long task", e.startTime, e.duration, TRACK_BROWSER);
      }).observe({ type: "longtask", buffered: false });
    } catch {
      // Neither: frame intervals and our own spans only.
    }
  }
  document.addEventListener("visibilitychange", () => Trace.instant(`page ${document.visibilityState}`));

  function gpuName() {
    const gl = /** @type {WebGL2RenderingContext | null} */ (canvas.getContext("webgl2"));
    const info = gl?.getExtension("WEBGL_debug_renderer_info");
    return info && gl ? `${gl.getParameter(info.UNMASKED_VENDOR_WEBGL)} / ${gl.getParameter(info.UNMASKED_RENDERER_WEBGL)}` : "unknown";
  }

  function start() {
    const seconds = Number($("trace-seconds").value);
    const nav = /** @type {any} */ (navigator);
    const meta = {
      userAgent: navigator.userAgent,
      gpu: gpuName(),
      extensions: (/** @type {WebGL2RenderingContext | null} */ (canvas.getContext("webgl2"))?.getSupportedExtensions() ?? []).join(" "),
      devicePixelRatio: String(devicePixelRatio),
      canvas: `${canvas.width}x${canvas.height}`,
      hardwareConcurrency: String(navigator.hardwareConcurrency ?? ""),
      deviceMemory: String(nav.deviceMemory ?? ""),
      crossOriginIsolated: String(crossOriginIsolated),
      date: new Date().toISOString(),
      ...describe(),
    };
    for (const [k, v] of Object.entries(meta)) Trace.metadata(k, v);
    intervals = [];
    lastFrame = -1;
    report = "";
    $("trace-download").disabled = true;
    $("trace-send").disabled = true;
    $("trace-record").disabled = true;
    $("trace-summary").textContent = `Recording ${seconds} s…`;
    reportName = `rswebgl-${(meta.model ?? "trace").replace(/[^\w.-]+/g, "_")}-${meta.date.replace(/[:.]/g, "-")}.json`;
    Trace.start();
    until = performance.now() + seconds * 1000;
  }

  function finish() {
    until = 0;
    Trace.stop();
    report = Trace.report();
    $("trace-record").disabled = false;
    $("trace-download").disabled = false;
    $("trace-send").disabled = false;
    $("trace-summary").textContent = summarize(intervals) + `\n${Trace.eventCount()} events, ${(report.length / 1e6).toFixed(1)} MB`;
  }

  $("trace-record").onclick = start;
  $("cost-measure").onclick = () => measureView().catch((e) => ($("trace-summary").textContent = `Failed: ${e}`));

  // What this view costs, and what each feature adds: frames rendered
  // back to back, each waited for (a one-pixel read of the canvas, which
  // can't return before the GPU is done; gl.finish doesn't wait in Chrome),
  // so a time is the whole frame's CPU + GPU work. Settings are switched one at a time, then
  // restored.
  async function measureView() {
    if (!scene) return;
    const gl = /** @type {WebGL2RenderingContext} */ (canvas.getContext("webgl2"));
    const cam = viewCamera();
    const root = getRoot();
    const out = $("trace-summary");
    const pixel = new Uint8Array(4);
    const sync = () => {
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, pixel);
      ctx?.invalidateState(); // the library's GL state cache saw none of this
    };
    for (const id of ["cost-measure", "trace-record"]) $(id).disabled = true;
    /** @type {[string, number][]} */
    const rows = [];
    const time = async (/** @type {string} */ label) => {
      out.textContent = `Measuring: ${label}…`;
      await scene.whenReady();
      for (let i = 0; i < 10; i++) scene.renderFrame(cam); // warm up (TAA history, mips)
      sync();
      const ms = [];
      for (let i = 0; i < 25; i++) {
        const t0 = performance.now();
        scene.renderFrame(cam);
        sync();
        ms.push(performance.now() - t0);
      }
      ms.sort((a, b) => a - b);
      rows.push([label, ms[ms.length >> 1]]);
      await new Promise((r) => setTimeout(r, 50)); // let the page breathe
    };
    // Materials with a property on, to turn it off for a row.
    const withProperty = (/** @type {string} */ name) =>
      root
        ? [...scene.materials(root)]
            .map((m) => [m, scene.materialProperty(m, name)])
            .filter(([, v]) => v && /** @type {any} */ (v)[0] > 0)
        : [];
    const without = async (/** @type {string} */ label, /** @type {string} */ name) => {
      const on = withProperty(name);
      if (!on.length) return;
      for (const [m] of on) scene.setMaterialProperty(m, name, 0);
      await time(label);
      for (const [m, v] of on) scene.setMaterialProperty(m, name, /** @type {any} */ (v).length === 1 ? /** @type {any} */ (v)[0] : [.../** @type {any} */ (v)]);
    };

    Trace.start();
    // Clocks ramp up: a second of frames before anything counts.
    for (const end = performance.now() + 1000; performance.now() < end; ) {
      scene.renderFrame(cam);
      sync();
    }
    await time("this view, as set");
    await without("no thin film (iridescence)", "thin_film_weight");
    await without("no transmission", "transmission_weight");
    await without("no coat", "coat_weight");
    await without("no fuzz (sheen)", "fuzz_weight");
    const trans = scene.transmissionResolution();
    if (withProperty("transmission_weight").length) {
      for (const s of [0.5, 0.25]) {
        if (s >= trans) continue;
        scene.setTransmissionResolution(s);
        await time(`scene copy for glass at ${s}`);
      }
      scene.setTransmissionResolution(trans);
    }
    const shading = scene.shadingModel();
    scene.setShadingModel(shading === "openpbr" ? "gltf" : "openpbr");
    await time(`${scene.shadingModel()} shading`);
    scene.setShadingModel(shading);
    if (scene.isTaa()) {
      scene.setTaa(false);
      scene.setSamples(0);
      await time("no TAA, no MSAA");
      scene.setTaa(true);
    }
    // FXAA's own cost: without it if on, else with it.
    if (scene.isFxaa()) {
      scene.setFxaa(false);
      await time("no FXAA");
      scene.setFxaa(true);
    } else if (scene.isHdr()) {
      scene.setFxaa(true);
      await scene.whenReady();
      await time("+ FXAA");
      scene.setFxaa(false);
    }
    if (root) {
      scene.setVisible(root, false);
      await time("model hidden (background only)");
      scene.setVisible(root, true);
    }
    await time("this view again (drift check)");
    await scene.whenReady();
    const base = rows[0][1];
    const text = rows.map(([l, ms]) => `${l}: ${ms.toFixed(1)} ms${l === rows[0][0] || l.startsWith("this view again") ? "" : ` (${ms - base >= 0 ? "+" : ""}${(ms - base).toFixed(1)})`}`).join("\n");
    Trace.metadata("view cost (ms, frame + 1-pixel read, median of 25)", text.replace(/\n/g, " | "));
    Trace.stop();
    report = Trace.report();
    reportName = `rswebgl-view-cost-${(describe().model ?? "model").replace(/[^\w.-]+/g, "_")}-${new Date().toISOString().replace(/[:.]/g, "-")}.json`;
    out.textContent = `${canvas.width}x${canvas.height}\n${text}`;
    for (const id of ["cost-measure", "trace-record", "trace-download", "trace-send"]) $(id).disabled = false;
  }
  $("trace-download").onclick = () => {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([report], { type: "application/json" }));
    a.download = reportName;
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 10_000);
  };
  $("trace-send").onclick = async () => {
    try {
      const res = await fetch(`trace?name=${encodeURIComponent(reportName)}`, { method: "POST", body: report });
      $("trace-summary").textContent += res.ok ? `\nSaved on the computer: ${await res.text()}` : `\nNot saved: HTTP ${res.status}`;
    } catch (e) {
      $("trace-summary").textContent += `\nNot sent: ${e}`;
    }
  };

  return {
    /** Once a frame, at its end. */
    frameEnd() {
      if (!until) return;
      const now = performance.now();
      if (lastFrame >= 0) intervals.push(now - lastFrame);
      lastFrame = now;
      Trace.counter("wasm memory (MB)", wasm.memory.buffer.byteLength / 1048576);
      if (now >= until || !Trace.isRecording()) finish();
    },
  };
}

// Frame interval percentiles and how many frames ran long.
function summarize(/** @type {number[]} */ ms) {
  if (!ms.length) return "No frames.";
  const s = [...ms].sort((a, b) => a - b);
  const q = (/** @type {number} */ p) => s[Math.min(s.length - 1, Math.floor(p * s.length))].toFixed(1);
  const over = (/** @type {number} */ t) => ms.filter((x) => x > t).length;
  return (
    `${ms.length} frames: median ${q(0.5)} ms, p95 ${q(0.95)}, p99 ${q(0.99)}, max ${s[s.length - 1].toFixed(1)}\n` +
    `over 20 ms: ${over(20)}, over 34 ms: ${over(34)}, over 50 ms: ${over(50)}`
  );
}
