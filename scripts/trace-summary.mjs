// Summarizes a trace the viewer recorded (its Profiling panel): frame
// interval percentiles, the time per span name, and the slowest frames with
// what happened in and around them.
//
//   node scripts/trace-summary.mjs traces/<file>.json [slowest=15]
//
// The same file opens in https://ui.perfetto.dev for a timeline.

import { readFileSync } from "node:fs";

const [file, topArg] = process.argv.slice(2);
if (!file) {
  console.error("usage: node scripts/trace-summary.mjs <trace.json> [slowest]");
  process.exit(1);
}
const top = Number(topArg ?? 15);
const trace = JSON.parse(readFileSync(file, "utf8"));
const events = trace.traceEvents.filter((/** @type {any} */ e) => e.ph !== "M");
const ms = (/** @type {number} */ us) => us / 1000;
const fmt = (/** @type {number} */ v) => v.toFixed(2).padStart(7);

console.log("metadata:");
for (const [k, v] of Object.entries(trace.metadata ?? {})) console.log(`  ${k}: ${v}`);

const spans = events.filter((/** @type {any} */ e) => e.ph === "X");
const main = spans.filter((/** @type {any} */ e) => e.tid === 1).sort((a, b) => a.ts - b.ts);
const frames = main.filter((/** @type {any} */ e) => e.name === "frame");
const gpu = spans.filter((/** @type {any} */ e) => e.tid === 2);
const browser = spans.filter((/** @type {any} */ e) => e.tid === 3);
const counters = events.filter((/** @type {any} */ e) => e.ph === "C");
const instants = events.filter((/** @type {any} */ e) => e.ph === "i");

if (frames.length < 2) {
  console.log("fewer than two frames recorded");
  process.exit(0);
}

const pct = (/** @type {number[]} */ xs, /** @type {number} */ p) => {
  const s = [...xs].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(p * s.length))];
};
const describe = (/** @type {number[]} */ xs) =>
  `mean ${fmt(xs.reduce((a, b) => a + b, 0) / xs.length)}  p50 ${fmt(pct(xs, 0.5))}  p95 ${fmt(pct(xs, 0.95))}  p99 ${fmt(pct(xs, 0.99))}  max ${fmt(Math.max(...xs))}`;

const intervals = frames.slice(1).map((f, i) => ms(f.ts - frames[i].ts));
console.log(`\n${frames.length} frames over ${(ms(frames.at(-1).ts - frames[0].ts) / 1000).toFixed(1)} s`);
console.log(`frame interval (ms)  ${describe(intervals)}`);
console.log(`  over 20 ms: ${intervals.filter((x) => x > 20).length}, over 34 ms: ${intervals.filter((x) => x > 34).length}, over 50 ms: ${intervals.filter((x) => x > 50).length}`);
console.log(`frame callback (ms)  ${describe(frames.map((f) => ms(f.dur)))}`);
if (gpu.length) console.log(`gpu fence (ms)       ${describe(gpu.map((g) => ms(g.dur)))}`);

// Time per span name (main track), per occurrence.
/** @type {Map<string, number[]>} */
const byName = new Map();
for (const s of main) {
  if (s.name === "frame") continue;
  if (!byName.has(s.name)) byName.set(s.name, []);
  byName.get(s.name)?.push(ms(s.dur));
}
console.log("\nspans (ms per occurrence):");
const rows = [...byName].sort((a, b) => Math.max(...b[1]) - Math.max(...a[1]));
for (const [name, xs] of rows) console.log(`  ${name.padEnd(30)} n=${String(xs.length).padStart(5)}  ${describe(xs)}`);

// The slowest frames: the interval that ended at frame i.
const order = intervals.map((v, i) => [v, i + 1]).sort((a, b) => b[0] - a[0]).slice(0, top);
console.log(`\nslowest ${order.length} frames:`);
for (const [interval, i] of order) {
  const f = frames[i];
  const prev = frames[i - 1];
  const inside = main.filter((s) => s !== f && s.ts >= f.ts && s.ts + s.dur <= f.ts + f.dur);
  // Top-level children only (not nested in another child).
  const children = inside.filter((s) => !inside.some((o) => o !== s && o.ts <= s.ts && o.ts + o.dur >= s.ts + s.dur && o.dur > s.dur));
  const accounted = children.reduce((a, s) => a + s.dur, 0);
  console.log(
    `\n  frame ${i} at ${(ms(f.ts) / 1000).toFixed(3)} s: interval ${interval.toFixed(1)} ms, callback ${ms(f.dur).toFixed(2)} ms ` +
      `(previous callback ${ms(prev.dur).toFixed(2)} ms; idle between ${ms(f.ts - (prev.ts + prev.dur)).toFixed(1)} ms)`,
  );
  for (const s of children.sort((a, b) => b.dur - a.dur).slice(0, 6)) {
    const args = s.args ? ` ${JSON.stringify(s.args)}` : "";
    console.log(`    ${s.name.padEnd(28)} ${ms(s.dur).toFixed(2)} ms${args}`);
    // The biggest nested span under a big child.
    const sub = inside.filter((o) => o !== s && o.ts >= s.ts && o.ts + o.dur <= s.ts + s.dur).sort((a, b) => b.dur - a.dur)[0];
    if (sub && sub.dur > 1000) console.log(`      └ ${sub.name} ${ms(sub.dur).toFixed(2)} ms${sub.args ? ` ${JSON.stringify(sub.args)}` : ""}`);
  }
  console.log(`    (unaccounted inside the callback: ${ms(f.dur - accounted).toFixed(2)} ms)`);
  const window = (/** @type {any} */ e) => e.ts + (e.dur ?? 0) >= prev.ts && e.ts <= f.ts + f.dur;
  for (const g of gpu.filter(window)) console.log(`    gpu fence from ${(ms(g.ts - f.ts)).toFixed(1)} ms: ${ms(g.dur).toFixed(1)} ms`);
  for (const b of browser.filter(window)) console.log(`    browser: ${b.name} ${ms(b.dur).toFixed(1)} ms (from ${ms(b.ts - f.ts).toFixed(1)} ms)`);
  for (const e of instants.filter(window)) console.log(`    marker: ${e.name}`);
  const flight = counters.filter((c) => c.name === "gpu frames in flight" && window(c)).map((c) => c.args.value);
  if (flight.length) console.log(`    gpu frames in flight: ${flight.join(", ")}`);
}

// Wasm memory growth (each step is a grow of the linear memory).
const memory = counters.filter((c) => c.name === "wasm memory (MB)");
const grows = memory.filter((c, i) => i > 0 && c.args.value !== memory[i - 1].args.value);
if (grows.length) {
  console.log(`\nwasm memory grew ${grows.length} times:`);
  for (const g of grows) console.log(`  at ${(ms(g.ts) / 1000).toFixed(3)} s to ${g.args.value.toFixed(1)} MB`);
}
