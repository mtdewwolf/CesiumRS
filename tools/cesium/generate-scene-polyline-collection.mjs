// PolylineCollection reference capture from the pinned CesiumJS engine on a
// real WebGL2 context (SwiftShader in headless Chromium). After every update
// it records the buffer uploads (vertex and index data), the vertex attribute
// pointers, the batch-table texture data, the draw commands, the buckets and
// their shader defines, the buffer usage and every polyline's state.
// Usage: CHROMIUM_PATH=/path/to/chrome node generate-scene-polyline-collection.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const modules = {
  Scene: ['PolylineCollection', 'Polyline', 'Material', 'Camera'],
  Renderer: ['Context'],
  Core: ['Cartesian3', 'Color', 'DistanceDisplayCondition', 'Matrix4', 'GeographicProjection', 'Ellipsoid', 'PerspectiveFrustum'],
};
const contents = Object.entries(modules)
  .flatMap(([dir, names]) => names.map((n) => `export {default as ${n}} from '@cesium/engine/Source/${dir}/${n}.js';`))
  .join('\n');
const bundle = await build({ stdin: { contents, resolveDir: fileURLToPath(new URL('.', import.meta.url)) }, bundle: true, write: false, format: 'esm', platform: 'browser' });
const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') { res.setHeader('Content-Type', 'text/javascript'); res.end(bundle.outputFiles[0].contents); return; }
  res.end('<!doctype html><title>PolylineCollection capture</title>');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

// Positions are [lon, lat, height] in degrees, converted with
// Cartesian3.fromDegrees on both sides. Materials: ['Color', [r, g, b, a]],
// ['PolylineGlow', glowPower], ['PolylineOutline', [r, g, b, a], outlineWidth],
// ['PolylineDash', [r, g, b, a]].
const line = (lon0, lat0, n, dLon, dLat, h = 0) => Array.from({ length: n }, (_, i) => [lon0 + i * dLon, lat0 + i * dLat, h + i * 100]);
const scenarios = {
  basic: [
    ['add', { positions: line(-75, 40, 4, 1, 0.5), width: 2 }],
    ['add', { positions: line(10, -20, 3, -2, 1), width: 5, material: ['Color', [1, 0, 0, 0.5]], id: 'translucent' }],
    ['add', { positions: line(100, 10, 5, 3, -1, 1000), width: 3.5, material: ['PolylineGlow', 0.3] }],
    ['add', { positions: line(0, 0, 2, 5, 5), width: 1, material: ['Color', [0, 1, 0, 1]] }],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
    ['set', 0, 'width', 7],
    ['set', 3, 'show', false],
    ['update', { mode: 3 }],
    ['set', 0, 'positions', line(-74, 41, 4, 1, 0.25)],
    ['update', { mode: 3 }],
    ['set', 1, 'distanceDisplayCondition', [10, 1e7]],
    ['update', { mode: 3 }],
    ['set', 2, 'positions', line(100, 10, 7, 3, -1, 1000)],
    ['update', { mode: 3 }],
    ['set', 3, 'material', ['PolylineDash', [1, 1, 0, 1]]],
    ['update', { mode: 3 }],
    ['set', 0, 'width', 300],
    ['update', { mode: 3, pick: true, render: false }],
    // A toggle that ends where it started still rewrites the width/show texel.
    ['set', 3, 'show', true],
    ['set', 3, 'show', false],
    ['update', { mode: 3 }],
  ],
  projected: [
    ['add', { positions: [[170, 10, 0], [-170, 12, 0], [-160, 15, 0]], width: 2 }],
    ['add', { positions: line(-30, -30, 4, 5, 5, 500), width: 4, material: ['PolylineOutline', [0, 0, 1, 1], 2] }],
    ['add', { positions: [[179, -5, 0], [-179, -5, 0]], width: 3, material: ['Color', [1, 0, 1, 0.75]] }],
    ['collection', 'modelMatrix', [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1000, 1]],
    ['update', { mode: 3 }],
    ['update', { mode: 2 }],
    ['set', 1, 'positions', line(-31, -30, 4, 5, 5, 500)],
    ['update', { mode: 2 }],
    ['update', { mode: 1 }],
    ['set', 0, 'positions', [[175, 10, 0], [-175, 12, 0], [-165, 15, 0]]],
    ['update', { mode: 1 }],
    ['update', { mode: 0, morphTime: 0.5 }],
    ['collection', 'modelMatrix', [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]],
    ['update', { mode: 3 }],
  ],
  loops: [
    ['add', { positions: line(20, 20, 4, 2, 1), loop: true, width: 2 }],
    ['add', { positions: [[0, 0, 0], [0, 0, 0], [1, 1, 0], [1, 1, 0], [2, 0, 0]], width: 2 }],
    ['add', { positions: [[5, 5, 0]] }],
    ['update', { mode: 3 }],
    ['set', 1, 'loop', true],
    ['update', { mode: 3 }],
    ['set', 0, 'loop', false],
    ['update', { mode: 3 }],
    ['set', 2, 'positions', [[5, 5, 0], [6, 6, 0]]],
    ['update', { mode: 3 }],
  ],
  removal: [
    ...[0, 1, 2, 3].map((i) => ['add', { positions: line(i * 10, i * 5, 3, 1, 1), width: 1 + i, id: `p${i}` }]),
    ['update', { mode: 3 }],
    ['remove', 1],
    ['update', { mode: 3 }],
    ['set', 2, 'width', 9],
    ['update', { mode: 3 }],
    ['removeAll'],
    ['update', { mode: 3 }],
    ['add', { positions: line(1, 1, 3, 1, 1), width: 2 }],
    ['update', { mode: 3 }],
  ],
  usage: [
    ['add', { positions: line(0, 0, 3, 1, 1), width: 2 }],
    ['update', { mode: 3 }],
    ['set', 0, 'positions', line(0, 0, 3, 1, 1.5)],
    ['update', { mode: 3 }],
    ['set', 0, 'positions', line(0, 0, 3, 1, 2)],
    ['update', { mode: 3 }],
    ...Array.from({ length: 101 }, () => ['update', { mode: 3, record: false }]),
    ['update', { mode: 3 }],
  ],
  large: [
    ['add', { positions: line(-80, -60, 9000, 0.01, 0.0125), width: 2 }],
    ['add', { positions: line(10, 10, 9000, 0.005, 0.001), width: 3, material: ['Color', [0, 1, 1, 1]] }],
    ['add', { positions: line(-10, 0, 4, 1, 1), width: 1 }],
    ['update', { mode: 3 }],
    ['set', 2, 'positions', line(-10, 0, 4, 1, 2)],
    ['update', { mode: 3 }],
  ],
};

let browser;
try {
  browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, headless: true, args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const result = await page.evaluate(async (scenarios) => {
    const ref = await import('/reference.mjs');
    const fnv = (bytes) => {
      let h = 0xcbf29ce484222325n;
      for (const b of bytes) h = BigInt.asUintN(64, (h ^ BigInt(b)) * 0x100000001b3n);
      return h.toString(16);
    };
    const num = (v) => (v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Number.isNaN(v) ? 'NaN' : v);
    const sphere = (s) => (s ? [s.center.x, s.center.y, s.center.z, s.radius].map(num) : null);
    const positions = (list) => list.map(([lon, lat, h]) => ref.Cartesian3.fromDegrees(lon, lat, h));
    const material = ([type, a, b]) => {
      if (type === 'Color') return ref.Material.fromType('Color', { color: new ref.Color(...a) });
      if (type === 'PolylineGlow') return ref.Material.fromType('PolylineGlow', { glowPower: a });
      if (type === 'PolylineOutline') return ref.Material.fromType('PolylineOutline', { outlineColor: new ref.Color(...a), outlineWidth: b });
      return ref.Material.fromType('PolylineDash', { color: new ref.Color(...a) });
    };
    const value = (k, v) => {
      if (k === 'positions') return positions(v);
      if (k === 'material') return material(v);
      if (k === 'distanceDisplayCondition') return new ref.DistanceDisplayCondition(...v);
      if (k === 'modelMatrix') return ref.Matrix4.fromArray(v);
      return v;
    };
    function camera() {
      const scene = {
        drawingBufferWidth: 800, drawingBufferHeight: 600, pixelRatio: 1, mapMode2D: 1, ellipsoid: ref.Ellipsoid.WGS84,
        mapProjection: new ref.GeographicProjection(), canvas: { clientWidth: 800, clientHeight: 600 }, frameState: { mode: 3 }, mode: 3,
        screenSpaceCameraController: { minimumZoomDistance: 1, maximumZoomDistance: Infinity }, verticalExaggeration: 1, verticalExaggerationRelativeHeight: 0,
      };
      const c = new ref.Camera(scene);
      c.frustum = new ref.PerspectiveFrustum({ fov: Math.PI / 3, aspectRatio: 4 / 3, near: 1, far: 1e9 });
      return c;
    }
    const out = {};
    for (const [name, script] of Object.entries(scenarios)) {
      const canvas = document.createElement('canvas');
      canvas.width = 8; canvas.height = 8;
      const context = new ref.Context(canvas, { webgl: { antialias: false } });
      let uploads = [];
      const gl = context._gl;
      const view = (data) => new Uint8Array(data.buffer ?? data, data.byteOffset ?? 0, data.byteLength);
      const wrap = (fn, record) => { const original = gl[fn].bind(gl); gl[fn] = (...args) => { record(...args); return original(...args); }; };
      wrap('bufferData', (target, data, usage) => { if (typeof data === 'object') uploads.push(['bufferData', target, usage, data.byteLength, fnv(view(data))]); });
      wrap('bufferSubData', (target, offset, data) => uploads.push(['bufferSubData', target, offset, data.byteLength, fnv(view(data))]));
      wrap('texImage2D', (...a) => { const d = a[8]; if (ArrayBuffer.isView(d)) uploads.push(['texture', a[3], a[4], a[7], d.byteLength, fnv(view(d))]); });
      wrap('texSubImage2D', (...a) => { const d = a[8]; if (ArrayBuffer.isView(d)) uploads.push(['texture', a[4], a[5], a[7], d.byteLength, fnv(view(d))]); });
      wrap('vertexAttribPointer', (index, size, type, normalized, stride, offset) => uploads.push(['pointer', index, size, type, normalized, stride, offset]));
      wrap('vertexAttrib3fv', (index, v) => uploads.push(['constant', index, ...v]));
      const collection = new ref.PolylineCollection();
      const polylines = [];
      const cam = camera();
      const steps = [];
      const programs = [];
      const programIndex = (sp) => { let i = programs.indexOf(sp); if (i < 0) { programs.push(sp); i = programs.length - 1; } return i; };
      for (const op of script) {
        const [kind, a, b, c] = op;
        if (kind === 'add') {
          const options = Object.fromEntries(Object.entries(a).map(([k, v]) => [k, value(k, v)]));
          polylines.push(collection.add(options));
        } else if (kind === 'set') polylines[a][b] = value(b, c);
        else if (kind === 'remove') steps.push({ op, removed: collection.remove(polylines[a]) });
        else if (kind === 'removeAll') collection.removeAll();
        else if (kind === 'collection') collection[a] = value(a, b);
        else {
          uploads = [];
          const frameState = {
            mode: a.mode, morphTime: a.morphTime ?? (a.mode === 3 ? 1 : 0), mapProjection: new ref.GeographicProjection(), context, camera: cam,
            commandList: [], passes: { render: a.render ?? true, pick: a.pick ?? false },
          };
          collection.update(frameState);
          if (a.record === false) { steps.push({ op }); continue; }
          const vas = collection._vertexArrays;
          const commands = frameState.commandList.map((command) => {
            const rs = command.renderState;
            const u = command.uniformMap;
            return {
              pass: command.pass, count: command.count, offset: command.offset, primitiveType: command.primitiveType,
              vaIndex: vas.findIndex((v) => v.va === command.vertexArray), program: programIndex(command.shaderProgram),
              renderState: { depthTest: rs.depthTest.enabled, depthFunc: rs.depthTest.func, depthMask: rs.depthMask, blending: rs.blending.enabled },
              boundingVolume: sphere(command.boundingVolume), modelMatrix: ref.Matrix4.toArray(command.modelMatrix),
              pickId: command.pickId, debugShowBoundingVolume: command.debugShowBoundingVolume,
              uniforms: Object.keys(u).sort(), highlightColor: (({ red, green, blue, alpha }) => [red, green, blue, alpha])(u.u_highlightColor()),
            };
          });
          const buckets = Object.entries(collection._polylineBuckets).map(([type, bucket]) => ({
            type, lengthOfPositions: bucket.lengthOfPositions, polylines: bucket.polylines.map((p) => p._index),
            defines: bucket.shaderProgram ? { vs: bucket.shaderProgram._vertexShaderSource.defines, fs: bucket.shaderProgram._fragmentShaderSource.defines } : null,
          }));
          const bt = collection._batchTable;
          steps.push({
            op, uploads, commands, buckets, vaCount: vas.length,
            vaBuckets: vas.map((v) => v.buckets.map((l) => [l.count, l.offset, Object.keys(collection._polylineBuckets).find((k) => collection._polylineBuckets[k] === l.bucket) ?? null])),
            usage: [collection._positionBufferUsage.bufferUsage, collection._positionBufferUsage.frameCount],
            batchValues: bt ? fnv(new Uint8Array(bt._batchValues.buffer)) : null, batchInstances: bt ? bt.numberOfInstances : null,
            polylinesUpdated: collection._polylinesUpdated, toUpdate: collection._polylinesToUpdate.map((p) => p._index),
            polylines: polylines.map((p) => ({
              index: p._index, destroyed: p.isDestroyed, dirty: p._dirty, properties: [...p._propertiesChanged],
              actualPositions: p._actualPositions.length, length: p._length, actualLength: p._actualLength ?? null,
              segments: p._segments.lengths, segmentPositions: p._segments.positions.length,
              boundingVolume: sphere(p._boundingVolume), boundingVolumeWC: sphere(p._boundingVolumeWC), boundingVolume2D: sphere(p._boundingVolume2D),
              locators: (p._locatorBuckets ?? []).map((l) => l.count), bucket: p._bucket ? Object.keys(collection._polylineBuckets).find((k) => collection._polylineBuckets[k] === p._bucket) ?? 'stale' : null,
            })),
          });
        }
      }
      out[name] = steps;
      collection.destroy();
      context.destroy();
    }
    return out;
  }, scenarios);
  writeFileSync(new URL('../../tests/data/scene-polyline-collection.json', import.meta.url), JSON.stringify({ scenarios, capture: result }) + '\n');
  console.log('captured', Object.keys(result).length, 'scenarios');
} finally {
  await browser?.close();
  server.close();
}
