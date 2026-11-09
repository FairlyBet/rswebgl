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
 * }} opts
 */
export function createProfiling({ wasm, canvas, $, describe }) {
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
