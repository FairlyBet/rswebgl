// Worker pools for the decoders that come as JS + wasm: the Basis Universal
// transcoder (KHR_texture_basisu) and Draco (KHR_draco_mesh_compression).
// After three.js (MIT): the pools as in examples/jsm/utils/WorkerPool.js,
// the worker bodies ported from KTX2Loader.BasisWorker and DRACOWorker —
// the decoder's script and the worker body concatenated into a Blob, the
// wasm binary sent with an init message, jobs and results transferred.
// Decoding off the main thread: a 2k Basis texture or a heavy Draco mesh
// would otherwise hold a frame for tens to hundreds of milliseconds.

// The KTX2 worker: transcodes one file to the best format `config` allows
// (three.js' FORMAT_OPTIONS, without ETC1-only, PVRTC and HDR).
const KTX2_WORKER = `
let ready;
let Basis;
const TF = { ETC1: 0, ETC2: 1, BC1: 2, BC3: 3, BC7_M5: 7, ASTC_4x4: 10, RGBA32: 13 };
const OPTIONS = [
  { if: 'astc', uastcOnly: true, tf: [TF.ASTC_4x4, TF.ASTC_4x4], etc1s: Infinity, uastc: 1 },
  { if: 'bptc', tf: [TF.BC7_M5, TF.BC7_M5], etc1s: 3, uastc: 2 },
  { if: 'dxt', srgbIf: 'dxtSrgb', tf: [TF.BC1, TF.BC3], etc1s: 4, uastc: 5 },
  { if: 'etc2', tf: [TF.ETC1, TF.ETC2], etc1s: 1, uastc: 3 },
  { tf: [TF.RGBA32, TF.RGBA32], etc1s: 100, uastc: 100 },
];
function target(uastc, alpha, srgb, config) {
  const options = OPTIONS
    .filter((o) => uastc || !o.uastcOnly)
    .sort((a, b) => (uastc ? a.uastc - b.uastc : a.etc1s - b.etc1s));
  for (const o of options) {
    if (o.if && !config[o.if]) continue;
    if (srgb && o.srgbIf && !config[o.srgbIf]) continue;
    return o.tf[alpha ? 1 : 0];
  }
  return TF.RGBA32;
}
function transcode(bytes, srgb, config) {
  const file = new Basis.KTX2File(bytes);
  try {
    if (!file.isValid()) throw new Error('invalid or unsupported .ktx2 file');
    const uastc = file.isUASTC();
    if (!uastc && !file.isETC1S()) throw new Error('not ETC1S or UASTC (HDR is not supported)');
    const width = file.getWidth();
    const height = file.getHeight();
    const levelCount = file.getLevels();
    const alpha = file.getHasAlpha();
    if (!width || !height || !levelCount) throw new Error('invalid texture');
    if (!file.startTranscoding()) throw new Error('.startTranscoding failed');
    const format = target(uastc, alpha, srgb, config);
    const levels = [];
    for (let mip = 0; mip < levelCount; mip++) {
      const info = file.getImageLevelInfo(mip, 0, 0);
      // The level's own size: a compressed region may end off the block
      // grid where it reaches the level's edge (three.js passes the padded
      // size, #25908, as it sizes levels by their data).
      const w = info.origWidth;
      const h = info.origHeight;
      const data = new Uint8Array(file.getImageTranscodedSizeInBytes(mip, 0, 0, format));
      if (!file.transcodeImage(data, mip, 0, 0, format, 0, -1, -1)) throw new Error('.transcodeImage failed');
      levels.push({ data, width: w, height: h });
    }
    return { width, height, alpha, format, levels };
  } finally {
    file.close();
    file.delete();
  }
}
self.onmessage = (e) => {
  const m = e.data;
  if (m.type === 'init') {
    ready = new Promise((resolve) => {
      Basis = { wasmBinary: m.wasm, onRuntimeInitialized: resolve };
      BASIS(Basis);
    }).then(() => Basis.initializeBasis());
    return;
  }
  ready.then(() => {
    try {
      const r = transcode(new Uint8Array(m.bytes), m.srgb, m.config);
      self.postMessage({ id: m.id, data: r }, r.levels.map((l) => l.data.buffer));
    } catch (err) {
      self.postMessage({ id: m.id, error: String(err && err.message || err) });
    }
  });
};
`;

// The Draco worker: decodes one mesh's attributes (by unique id, as glTF
// stores them) into the requested component types, and its triangles.
const DRACO_WORKER = `
let ready;
const TYPES = {
  5120: ['DT_INT8', Int8Array], 5121: ['DT_UINT8', Uint8Array],
  5122: ['DT_INT16', Int16Array], 5123: ['DT_UINT16', Uint16Array],
  5125: ['DT_UINT32', Uint32Array], 5126: ['DT_FLOAT32', Float32Array],
};
function decode(draco, decoder, array, attributes) {
  const type = decoder.GetEncodedGeometryType(array);
  if (type !== draco.TRIANGULAR_MESH) throw new Error('not a triangle mesh');
  const mesh = new draco.Mesh();
  try {
    const status = decoder.DecodeArrayToMesh(array, array.byteLength, mesh);
    if (!status.ok() || mesh.ptr === 0) throw new Error('decoding failed: ' + status.error_msg());
    const count = mesh.num_points();
    const out = [];
    for (const a of attributes) {
      const [dt, Typed] = TYPES[a.componentType];
      const attribute = decoder.GetAttributeByUniqueId(mesh, a.id);
      if (!attribute || attribute.ptr === 0) throw new Error('no attribute with unique id ' + a.id);
      const components = attribute.num_components();
      const byteLength = count * components * Typed.BYTES_PER_ELEMENT;
      const ptr = draco._malloc(byteLength);
      decoder.GetAttributeDataArrayForAllPoints(mesh, attribute, draco[dt], byteLength, ptr);
      out.push({ data: new Uint8Array(draco.HEAPF32.buffer, ptr, byteLength).slice(), components });
      draco._free(ptr);
    }
    const indexCount = mesh.num_faces() * 3;
    const byteLength = indexCount * 4;
    const ptr = draco._malloc(byteLength);
    decoder.GetTrianglesUInt32Array(mesh, byteLength, ptr);
    const index = new Uint32Array(draco.HEAPF32.buffer, ptr, indexCount).slice();
    draco._free(ptr);
    return { count, attributes: out, index };
  } finally {
    draco.destroy(mesh);
  }
}
self.onmessage = (e) => {
  const m = e.data;
  if (m.type === 'init') {
    ready = new Promise((resolve) => {
      DracoDecoderModule({ wasmBinary: m.wasm, onModuleLoaded: (draco) => resolve({ draco }) });
    });
    return;
  }
  ready.then(({ draco }) => {
    const decoder = new draco.Decoder();
    try {
      const r = decode(draco, decoder, new Int8Array(m.bytes), m.attributes);
      self.postMessage({ id: m.id, data: r }, [...r.attributes.map((a) => a.data.buffer), r.index.buffer]);
    } catch (err) {
      self.postMessage({ id: m.id, error: String(err && err.message || err) });
    } finally {
      draco.destroy(decoder);
    }
  });
};
`;

// Workers made on first use, up to `limit`; each job goes to the least
// busy one.
class Pool {
  constructor(source, wasm, limit) {
    this.url = URL.createObjectURL(new Blob([source], { type: 'text/javascript' }));
    this.wasm = wasm;
    this.limit = limit;
    this.workers = [];
    this.load = [];
    this.jobs = new Map();
    this.nextId = 0;
  }
  run(message, transfer) {
    let i = this.load.indexOf(Math.min(...this.load));
    if (this.workers.length < this.limit && (i < 0 || this.load[i] > 0)) {
      const w = new Worker(this.url);
      w.postMessage({ type: 'init', wasm: this.wasm.slice(0) });
      const index = this.workers.length;
      w.onmessage = (e) => {
        const job = this.jobs.get(e.data.id);
        this.jobs.delete(e.data.id);
        this.load[index]--;
        if (e.data.error !== undefined) job.reject(new Error(e.data.error));
        else job.resolve(e.data.data);
      };
      w.onerror = (e) => console.error('[rswebgl-gltf] decoder worker:', e.message);
      this.workers.push(w);
      this.load.push(0);
      i = index;
    }
    const id = this.nextId++;
    this.load[i]++;
    return new Promise((resolve, reject) => {
      this.jobs.set(id, { resolve, reject });
      this.workers[i].postMessage({ ...message, id }, transfer);
    });
  }
}

const KINDS = {
  ktx2: { js: 'basis_transcoder.js', wasm: 'basis_transcoder.wasm', body: KTX2_WORKER },
  draco: { js: 'draco_wasm_wrapper.js', wasm: 'draco_decoder.wasm', body: DRACO_WORKER },
};
const decoders = {};

function limit() {
  const n = (typeof navigator !== 'undefined' && navigator.hardwareConcurrency) || 4;
  return Math.max(1, Math.min(4, n - 1));
}

// Where the decoder's two files are (a directory URL, as three.js'
// setTranscoderPath / setDecoderPath).
export function setDecoderPath(kind, path) {
  const base = path.endsWith('/') ? path : path + '/';
  decoders[kind] = { base, pool: null };
}

export function hasDecoder(kind) {
  return decoders[kind] !== undefined;
}

function pool(kind) {
  const d = decoders[kind];
  if (!d) return Promise.reject(new Error('no decoder path set'));
  if (!d.pool) {
    const k = KINDS[kind];
    const text = (r) => { if (!r.ok) throw new Error(r.url + ': ' + r.status); return r; };
    d.pool = Promise.all([
      fetch(new URL(k.js, d.base, location.href)).then(text).then((r) => r.text()),
      fetch(new URL(k.wasm, d.base, location.href)).then(text).then((r) => r.arrayBuffer()),
    ]).then(([js, wasm]) => new Pool(js + '\n' + k.body, wasm, limit()));
    d.pool.catch(() => { d.pool = null; });
  }
  return d.pool;
}

// Transcodes a .ktx2 file: { width, height, alpha, format (Basis'
// transcoder format), levels: [{ data, width, height }] }.
export function ktx2Transcode(bytes, srgb, config) {
  const copy = bytes.slice();
  return pool('ktx2').then((p) => p.run({ bytes: copy.buffer, srgb, config }, [copy.buffer]));
}

// Decodes a Draco mesh: { count, attributes: [{ data, components }],
// index: Uint32Array }, the attributes as `attributes` asks
// ([{ id, componentType }]).
export function dracoDecode(bytes, attributes) {
  const copy = bytes.slice();
  return pool('draco').then((p) => p.run({ bytes: copy.buffer, attributes }, [copy.buffer]));
}
