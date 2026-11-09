// Dependency-free static server for this folder (serves .wasm with the right
// MIME type, which streaming instantiation needs).
//
//   node serve.mjs [port] [--lan]
//
// Without --lan it listens on 127.0.0.1 only. With --lan it listens on every
// interface and prints this machine's local-network addresses, so a phone on
// the same Wi-Fi can open the viewer (allow Node through the firewall if
// asked).
//
// POST /trace saves a profiling report the page sends (its Profiling panel)
// under traces/ at the repository root.
import { createServer } from "node:http";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { networkInterfaces } from "node:os";
import { extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL(".", import.meta.url));
const traces = fileURLToPath(new URL("../../traces/", import.meta.url));
const args = process.argv.slice(2);
const lan = args.includes("--lan");
const port = Number(args.find((a) => /^\d+$/.test(a)) ?? 8091);
const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript",
  ".wasm": "application/wasm",
  ".css": "text/css",
  ".json": "application/json",
};

createServer(async (req, res) => {
  if (req.method === "POST") {
    const url = new URL(req.url ?? "/", "http://x");
    if (url.pathname !== "/trace") {
      res.writeHead(404).end();
      return;
    }
    const chunks = [];
    for await (const c of req) chunks.push(c);
    // A plain file name in traces/, nothing else.
    const name = (url.searchParams.get("name") ?? "trace").replace(/[^\w.-]+/g, "_").replace(/^\.+/, "").replace(/(\.json)?$/, ".json");
    await mkdir(traces, { recursive: true });
    const file = join(traces, name);
    await writeFile(file, Buffer.concat(chunks));
    console.log(`trace saved: ${file}`);
    res.writeHead(200, { "content-type": "text/plain" }).end(`traces/${name}`);
    return;
  }
  const path = normalize(decodeURIComponent(new URL(req.url ?? "/", "http://x").pathname));
  const file = join(root, /[\\/]$/.test(path) ? path + "index.html" : path);
  if (!file.startsWith(root)) {
    res.writeHead(403).end();
    return;
  }
  try {
    const body = await readFile(file);
    res.writeHead(200, {
      "content-type": types[extname(file)] ?? "application/octet-stream",
      "cache-control": "no-cache",
    });
    res.end(body);
  } catch {
    res.writeHead(404).end();
  }
}).listen(port, lan ? "0.0.0.0" : "127.0.0.1", () => {
  console.log(`http://127.0.0.1:${port}/`);
  if (lan) {
    for (const list of Object.values(networkInterfaces())) {
      for (const a of list ?? []) {
        if (a.family === "IPv4" && !a.internal) console.log(`http://${a.address}:${port}/`);
      }
    }
  }
});
