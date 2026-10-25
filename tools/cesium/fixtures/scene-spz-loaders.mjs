// SPZ decoding (the pinned @spz-loader/core WASM) and the loaders built on it:
// GltfSpzLoader, ResourceCache.getSpzLoader and GltfVertexBufferLoader's SPZ
// attributes. Only GPU buffer creation and the upload scheduler are replaced.
import {gzipSync} from 'node:zlib';
import Resource from '@cesium/engine/Source/Core/Resource.js';
import Cache from '@cesium/engine/Source/Scene/ResourceCache.js';
import Buffer from '@cesium/engine/Source/Renderer/Buffer.js';
import {loadSpz} from '@spz-loader/core';
import {suite, enc, makeRng} from './lib.mjs';

// The WASM glue takes its Node path through createRequire, which its bundle
// stubs out; its browser path loads the embedded module. Its decoder prints
// the reason for a failure to stdout before the binding aborts with "divide by
// zero".
async function browserSpz(run) {
  const type = process.type, log = console.log;
  const logged = [];
  process.type = 'renderer';
  console.log = (...args) => logged.push(args.join(' '));
  try {
    return await run(logged);
  } finally {
    process.type = type;
    console.log = log;
  }
}

const MAGIC = 0x5053474e;
function packSpz({version = 2, points, shDegree = 0, frac = 12, flags = 0, magic = MAGIC, arrays, trailing = []}) {
  const header = new Uint8Array(16);
  const view = new DataView(header.buffer);
  view.setUint32(0, magic, true);
  view.setUint32(4, version, true);
  view.setUint32(8, points >>> 0, true);
  header[12] = shDegree;
  header[13] = frac;
  header[14] = flags;
  return Uint8Array.from([...header, ...arrays.flat(), ...trailing]);
}
const gzip = (raw) => new Uint8Array(gzipSync(raw));
const shDim = (degree) => [0, 3, 8, 15][degree] ?? 0;

// Arrays in file order: positions, alphas, colors, scales, rotations, sh.
function randomArrays(rng, points, version, degree) {
  const bytes = (n) => Array.from({length: n}, () => Math.floor(rng() * 256));
  const positionBytes = version === 1 ? 6 : 9;
  const rotationBytes = version >= 3 ? 4 : 3;
  return [bytes(points * positionBytes), bytes(points), bytes(points * 3), bytes(points * 3), bytes(points * rotationBytes), bytes(points * shDim(degree) * 3)];
}

function cloud(c) {
  const arrays = {positions: c.positions, scales: c.scales, rotations: c.rotations, alphas: c.alphas, colors: c.colors, sh: c.sh};
  return {numPoints: c.numPoints, shDegree: c.shDegree, antialiased: c.antialiased, ...Object.fromEntries(Object.entries(arrays).map(([k, v]) => [k, Array.from(v).map(enc)]))};
}

function decodeInputs() {
  const rng = makeRng(0x5b2);
  const inputs = [];
  const add = (name, bytes) => inputs.push({name, bytes});
  // Every byte value through each per-byte transform.
  const all = Array.from({length: 256}, (_, i) => i);
  add('exhaustive-v2', gzip(packSpz({points: 256, shDegree: 1, arrays: [all.flatMap((i) => [i, (i * 7) & 255, (i * 13) & 255, i, 255 - i, (i * 3) & 255, (i * 5) & 255, i, (i * 11) & 255]), all, all.flatMap((i) => [i, 255 - i, (i * 17) & 255]), all.flatMap((i) => [i, (i * 29) & 255, 255 - i]), all.flatMap((i) => [i, (i * 7) & 255, (i * 13) & 255]), all.flatMap((i) => Array.from({length: 9}, (_, k) => (i + k * 31) & 255))]})));
  for (const version of [1, 2, 3])
    for (const degree of [0, 1, 2, 3]) {
      const points = 3 + Math.floor(rng() * 6);
      add(`random-v${version}-sh${degree}`, gzip(packSpz({version, points, shDegree: degree, frac: 4 + Math.floor(rng() * 16), flags: degree & 1, arrays: randomArrays(rng, points, version, degree)})));
    }
  // Half-float specials and subnormals in version 1 positions.
  const halves = [0x0000, 0x8000, 0x0001, 0x8001, 0x03ff, 0x0400, 0x3c00, 0xc000, 0x7bff, 0x7c00, 0xfc00, 0x7e00, 0x7c01, 0xfe01, 0x3555, 0x2e66];
  const halfBytes = halves.flatMap((h) => [h & 255, h >> 8]);
  const halfPoints = halves.length / 3 | 0;
  add('v1-halves', gzip(packSpz({version: 1, points: halfPoints, arrays: [halfBytes.slice(0, halfPoints * 6), ...randomArrays(rng, halfPoints, 1, 0).slice(1)]})));
  // Smallest-three rotations, including sums of squares above one.
  const words = [0, 0xffffffff, 0x1ff7fdff, 0xc03c0601, 0x40000000, 0x80000000, 0x3fffffff, 0x7fffffff, 0xbfffffff, 0x12345678, 0x78563412];
  for (let i = 0; i < 37; i++) words.push(Math.floor(rng() * 2 ** 32));
  add('v3-rotations', gzip(packSpz({version: 3, points: words.length, arrays: [Array(words.length * 9).fill(0), Array(words.length).fill(128), Array(words.length * 3).fill(128), Array(words.length * 3).fill(160), words.flatMap((w) => [w & 255, (w >>> 8) & 255, (w >>> 16) & 255, w >>> 24]), []]})));
  // Fixed-point positions across every fractional shift, including C's
  // shift-count wrap and 1 << 31.
  const fixed = [[1, 0, 0], [0, 16, 0], [255, 255, 255], [0, 0, 128], [255, 255, 127], [0x34, 0x12, 0x80]].flat();
  for (const frac of [0, 1, 12, 23, 24, 30, 31, 32, 33, 40, 255]) add(`frac-${frac}`, gzip(packSpz({points: 2, frac, arrays: [fixed.slice(0, 18), [10, 250], [1, 2, 3, 4, 5, 6], [160, 96, 0, 255, 128, 16], [255, 0, 128, 3, 200, 90], []]})));
  const one = [[1, 0, 0, 0, 16, 0, 255, 255, 255], [200], [10, 128, 250], [160, 96, 0], [255, 0, 128]];
  add('flags', gzip(packSpz({points: 1, flags: 0xff, arrays: [...one, []]})));
  add('trailing', gzip(packSpz({points: 1, arrays: [...one, []], trailing: [1, 2, 3]})));
  const single = gzip(packSpz({points: 1, arrays: [...one, []]}));
  add('gzip-trailing', Uint8Array.from([...single, 9, 9, 9, 9]));
  add('gzip-two-members', Uint8Array.from([...single, ...single]));
  add('max-points', gzip(packSpz({points: 10000000, arrays: [...one, []]})));
  // Failures.
  add('too-many-points', gzip(packSpz({points: 10000001, arrays: [...one, []]})));
  add('negative-points', gzip(packSpz({points: -1, arrays: [...one, []]})));
  add('zero-points', gzip(packSpz({points: 0, arrays: []})));
  add('version-0', gzip(packSpz({version: 0, points: 1, arrays: [...one, []]})));
  add('version-4', gzip(packSpz({version: 4, points: 1, arrays: [...one, []]})));
  add('bad-magic', gzip(packSpz({magic: 0x5053474f, points: 1, arrays: [...one, []]})));
  add('sh-degree-4', gzip(packSpz({points: 1, shDegree: 4, arrays: [...one, Array(45).fill(1)]})));
  add('truncated-body', gzip(packSpz({points: 2, arrays: [...one, []]})));
  add('truncated-header', gzip(packSpz({points: 1, arrays: []}).slice(0, 12)));
  add('not-gzipped', packSpz({points: 1, arrays: [...one, []]}));
  add('gzip-truncated', single.slice(0, single.length - 4));
  const crc = single.slice();
  crc[crc.length - 6] ^= 1;
  add('gzip-crc', crc);
  const size = single.slice();
  size[size.length - 1] ^= 1;
  add('gzip-size', size);
  const header = gzip(packSpz({points: 1, arrays: []}));
  add('gzip-split-members', Uint8Array.from([...header, ...gzip(Uint8Array.from(one.flat()))]));
  add('empty', new Uint8Array(0));
  return inputs;
}

const resource = new Resource('https://spz.test/splats/model.gltf');
// Buffer view 0 holds the SPZ; GltfSpzLoader always reads buffer view 0.
function document(spz, prefix = 3) {
  const raw = [...Array(prefix).fill(7), ...spz, 5, 5];
  return {raw, gltf: {buffers: [{byteLength: raw.length}], bufferViews: [{buffer: 0, byteOffset: prefix, byteLength: spz.length}, {buffer: 0, byteOffset: 0, byteLength: 2}], accessors: [{componentType: 5126, type: 'VEC3', count: 1}]}};
}
const primitive = {attributes: {POSITION: 0}, extensions: {KHR_gaussian_splatting: {extensions: {KHR_gaussian_splatting_compression_spz_2: {}}}}};
const extension = primitive.extensions.KHR_gaussian_splatting.extensions.KHR_gaussian_splatting_compression_spz_2;
const entries = () => Object.fromEntries(Object.entries(Cache.cacheEntries).map(([k, e]) => [k, e.referenceCount]));
const bytesOf = (a) => (a ? Array.from(new Uint8Array(a.buffer, a.byteOffset, a.byteLength)) : null);
const failure = (e) => ({message: String(e.message).split('\n')[0], lines: String(e.message).split('\n').length});

export default async function spzLoaders() {
  const spz = suite('GltfSpzLoader'), vertex = suite('GltfVertexBufferLoader'), pool = suite('ResourceCache');
  const originalVertex = Buffer.createVertexBuffer;
  Buffer.createVertexBuffer = ({typedArray}) => {
    if (typedArray === undefined) throw new Error('Must specify either sizeInBytes or typedArray.');
    return {sizeInBytes: typedArray.byteLength, destroyed: false, destroy() { this.destroyed = true; }, isDestroyed() { return this.destroyed; }};
  };
  try {
    await browserSpz(async (logged) => {
      for (const {name, bytes} of decodeInputs()) {
        logged.length = 0;
        let r;
        try {
          r = cloud(await loadSpz(bytes, {unpackOptions: {coordinateSystem: 'UNSPECIFIED'}}));
        } catch (e) {
          r = {error: String(e.message), logged: logged.map((l) => l.replace('[SPZ ERROR] ', ''))};
        }
        spz.cases.push({f: 'decode', a: [{name, bytes: Array.from(bytes)}], r});
      }

      const rng = makeRng(0x5b3);
      const good = (version, degree, points) => gzip(packSpz({version, points, shDegree: degree, frac: 10, flags: 1, arrays: randomArrays(rng, points, version, degree)}));
      const broken = gzip(packSpz({points: 3, arrays: [[1, 2, 3]]}));
      for (const [label, bytes] of [['v2', good(2, 1, 4)], ['v3', good(3, 2, 3)], ['broken', broken]]) {
        Cache.clearForSpecs();
        const {raw, gltf} = document(Array.from(bytes));
        const seed = Cache.getEmbeddedBufferLoader({parentResource: resource, bufferId: 0, typedArray: new Uint8Array(raw)});
        const loader = Cache.getSpzLoader({gltf, primitive, spz: extension, gltfResource: resource, baseResource: resource});
        const again = Cache.getSpzLoader({gltf, primitive, spz: extension, gltfResource: resource, baseResource: resource});
        const frame = {context: {id: 'gpu'}};
        const state = () => loader._state;
        const r = {key: loader.cacheKey, shared: loader === again, initial: state(), entriesInitial: entries()};
        await loader.load();
        r.loaded = state();
        r.entriesLoaded = entries();
        r.first = loader.process(frame) ?? null;
        r.processing = state();
        // The decode settles between frames; states are compared at process calls.
        await loader._decodePromise;
        try {
          r.second = loader.process(frame);
        } catch (e) {
          r.second = failure(e);
        }
        r.after = state();
        r.entriesAfter = entries();
        r.cloud = loader.decodedData ? cloud(loader.decodedData.gcloud) : null;
        r.third = loader.process(frame);
        Cache.unload(again);
        r.entriesShared = entries();
        Cache.unload(loader);
        r.destroyed = loader.isDestroyed();
        Cache.unload(seed);
        r.remaining = Object.keys(Cache.cacheEntries).length;
        spz.cases.push({f: 'lifecycle', a: [{label, raw, gltf}], r: enc(r)});
      }

      const semantics = ['POSITION', '_SCALE', 'KHR_gaussian_splatting:SCALE', '_ROTATION', 'KHR_gaussian_splatting:ROTATION', 'COLOR_0', '_SH_DEGREE_1_COEF_0', '_SH_DEGREE_1_COEF_2', 'KHR_gaussian_splatting:SH_DEGREE_2_COEF_4', 'KHR_gaussian_splatting:SH_DEGREE_3_COEF_6', '_SH_DEGREE_3_COEF_0', '_SH_DEGREE_4_COEF_0', '_SH_DEGREE_0_COEF_0', '_SH_DEGREE_2_COEF_-1', 'X_SH_DEGREE_x', '_SH_DEGREE_ 2_COEF_ 1', 'NORMAL'];
      const clouds = [['v2-sh3', good(2, 3, 4)], ['v3-sh1', good(3, 1, 3)], ['v1-sh0', good(1, 0, 2)], ['v2-sh2', good(2, 2, 2)]];
      for (const [label, bytes] of clouds)
        for (const semantic of semantics)
          for (const [retain, upload] of [[true, false], [false, true], [true, true]]) {
            if (label !== 'v2-sh3' && !(retain && !upload)) continue;
            Cache.clearForSpecs();
            const {raw, gltf} = document(Array.from(bytes));
            const seed = Cache.getEmbeddedBufferLoader({parentResource: resource, bufferId: 0, typedArray: new Uint8Array(raw)});
            let allow = false;
            const frame = {context: {id: 'gpu'}, jobScheduler: {execute(job) { if (!allow) return false; job.execute(); return true; }}};
            const loader = Cache.getVertexBufferLoader({gltf, gltfResource: resource, baseResource: resource, frameState: frame, primitive, spz: extension, attributeSemantic: semantic, accessorId: 0, loadTypedArray: retain, loadBuffer: upload});
            const snapshot = () => ({state: loader._state, cpu: bytesOf(loader.typedArray), gpu: loader.buffer?.sizeInBytes ?? null, geometry: Cache.statistics.geometryByteLength, entries: entries()});
            const r = {key: loader.cacheKey, initial: snapshot()};
            await loader.load();
            r.loaded = snapshot();
            const step = () => {
              try {
                return loader.process(frame);
              } catch (e) {
                return failure(e);
              }
            };
            r.first = step();
            await loader._spzLoader?._decodePromise;
            r.second = step();
            r.deferred = snapshot();
            allow = true;
            r.ready = step();
            r.after = snapshot();
            const buffer = loader.buffer;
            Cache.unload(loader);
            Cache.unload(seed);
            r.destroyed = loader.isDestroyed();
            r.gpuDestroyed = buffer ? buffer.isDestroyed() : null;
            r.remaining = Object.keys(Cache.cacheEntries).length;
            r.geometryAfter = Cache.statistics.geometryByteLength;
            vertex.cases.push({f: 'spz', a: [{label, semantic, retain, upload, raw, gltf}], r: enc(r)});
          }

      // A failed SPZ decode fails each attribute that shares it; a failed SPZ
      // load leaves the attribute loading, as the source swallows the error.
      for (const mode of ['decode-failure', 'load-failure']) {
        Cache.clearForSpecs();
        const {raw, gltf} = document(Array.from(broken));
        if (mode === 'load-failure') gltf.buffers[0].uri = 'missing.bin';
        const seed = mode === 'decode-failure' ? Cache.getEmbeddedBufferLoader({parentResource: resource, bufferId: 0, typedArray: new Uint8Array(raw)}) : undefined;
        const frame = {context: {id: 'gpu'}, jobScheduler: {execute(job) { job.execute(); return true; }}};
        const make = (attributeSemantic) => Cache.getVertexBufferLoader({gltf, gltfResource: resource, baseResource: resource, frameState: frame, primitive, spz: extension, attributeSemantic, accessorId: 0, loadTypedArray: true});
        const loaders = [make('POSITION'), make('COLOR_0')];
        const r = {entries: entries()};
        const originalFetch = Resource.prototype.fetchArrayBuffer;
        Resource.prototype.fetchArrayBuffer = function () { return Promise.reject(new Error('Unreachable buffer')); };
        try {
          r.load = [];
          for (const l of loaders) r.load.push((await l.load()) === l);
        } finally {
          Resource.prototype.fetchArrayBuffer = originalFetch;
        }
        r.loaded = loaders.map((l) => l._state);
        r.spzState = loaders[0]._spzLoader?._state ?? null;
        const step = (l) => {
          try {
            return l.process(frame);
          } catch (e) {
            return failure(e);
          }
        };
        r.first = loaders.map(step);
        await loaders[0]._spzLoader?._decodePromise;
        r.second = loaders.map(step);
        r.third = loaders.map(step);
        r.states = loaders.map((l) => l._state);
        r.entriesFailed = entries();
        for (const l of loaders) Cache.unload(l);
        if (seed) Cache.unload(seed);
        r.remaining = Object.keys(Cache.cacheEntries).length;
        vertex.cases.push({f: 'spz-failure', a: [{mode, raw, gltf}], r: enc(r)});
      }

      // Attributes share one SPZ loader and buffer view; the last release
      // destroys them in dependency order.
      Cache.clearForSpecs();
      const {raw, gltf} = document(Array.from(good(2, 1, 2)));
      const seed = Cache.getEmbeddedBufferLoader({parentResource: resource, bufferId: 0, typedArray: new Uint8Array(raw)});
      const frame = {context: {id: 'gpu'}, jobScheduler: {execute(job) { job.execute(); return true; }}};
      const trace = [entries()];
      const make = (attributeSemantic) => Cache.getVertexBufferLoader({gltf, gltfResource: resource, baseResource: resource, frameState: frame, primitive, spz: extension, attributeSemantic, accessorId: 0, loadTypedArray: true});
      const position = make('POSITION'), color = make('COLOR_0'), positionAgain = make('POSITION');
      trace.push(entries());
      await Promise.all([position.load(), color.load()]);
      trace.push(entries());
      position.process(frame);
      await position._spzLoader._decodePromise;
      trace.push([position.process(frame), color.process(frame)]);
      trace.push(entries());
      const spzLoader = Cache.getSpzLoader({gltf, primitive, spz: extension, gltfResource: resource, baseResource: resource});
      trace.push(entries());
      Cache.unload(spzLoader);
      Cache.unload(positionAgain);
      Cache.unload(position);
      trace.push(entries());
      Cache.unload(color);
      trace.push(entries());
      Cache.unload(seed);
      trace.push(entries());
      pool.cases.push({f: 'spz-sharing', a: [{raw, gltf}], r: enc(trace)});
    });
  } finally {
    Cache.clearForSpecs();
    Buffer.createVertexBuffer = originalVertex;
  }
  return [spz, vertex, pool].map((s) => ({...s, dir: 'spz'}));
}
