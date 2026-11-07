// Primitive reference capture from the pinned CesiumJS engine on a real
// WebGL2 context (SwiftShader in headless Chromium), with `asynchronous:
// false`. After every update (then afterRender) it records the state, every
// buffer and texture upload, the attribute pointers, the shader text
// Primitive builds, the render states, the draw commands, the bounding
// volumes, the batch table and the per-instance attribute values; ops read
// and write per-instance attributes between frames.
// Usage: CHROMIUM_PATH=/path/to/chrome node generate-scene-primitive.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const modules = {
  Scene: ['Primitive', 'PerInstanceColorAppearance', 'MaterialAppearance', 'Material', 'Camera'],
  Renderer: ['Context', 'ContextLimits'],
  Core: ['Cartesian3', 'Color', 'Matrix4', 'GeographicProjection', 'Ellipsoid', 'PerspectiveFrustum', 'GeometryInstance', 'BoxGeometry', 'RectangleGeometry', 'EllipsoidGeometry', 'Rectangle', 'VertexFormat', 'ColorGeometryInstanceAttribute', 'ShowGeometryInstanceAttribute', 'DistanceDisplayConditionGeometryInstanceAttribute', 'OffsetGeometryInstanceAttribute', 'GeometryOffsetAttribute', 'Transforms', 'BoundingSphere'],
};
const contents = Object.entries(modules)
  .flatMap(([dir, names]) => names.map((n) => `export {default as ${n}} from '@cesium/engine/Source/${dir}/${n}.js';`))
  .join('\n') + "\nexport {default as ShadowMode} from '@cesium/engine/Source/Scene/ShadowMode.js';";
const bundle = await build({ stdin: { contents, resolveDir: fileURLToPath(new URL('.', import.meta.url)) }, bundle: true, write: false, format: 'esm', platform: 'browser' });
const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') { res.setHeader('Content-Type', 'text/javascript'); res.end(bundle.outputFiles[0].contents); return; }
  res.end('<!doctype html><title>Primitive capture</title>');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

// Geometry: ['box', [dx, dy, dz], vertexFormat, offsetAttribute?],
// ['rectangle', [w, s, e, n] degrees, vertexFormat], ['ellipsoid', [rx, ry, rz], vertexFormat, stackPartitions, slicePartitions].
// Vertex formats: 'pc' (PerInstanceColorAppearance.VERTEX_FORMAT), 'flat'
// (FLAT_VERTEX_FORMAT), 'basic' (MaterialAppearance BASIC), 'all' (VertexFormat.ALL).
// Attributes: color [r,g,b,a], show bool, ddc [near, far], offset [x,y,z], depthFailColor [r,g,b,a].
// Appearances: ['perInstance', {flat, translucent, closed}], ['material', [r,g,b,a], {translucent, closed, faceForward}].
const enu = (lon, lat, h) => ['enu', lon, lat, h];
const scenarios = {
  colorBoxes: {
    options: { appearance: ['perInstance', { translucent: false, closed: true }], modelMatrix: enu(-75, 40, 1000) },
    instances: [
      { geometry: ['box', [100, 200, 300], 'pc'], id: 'a', attributes: { color: [1, 0, 0, 1], show: true } },
      { geometry: ['box', [50, 50, 50], 'pc'], id: 'b', modelMatrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 200, 0, 0, 1], attributes: { color: [0, 1, 0, 1], show: true } },
    ],
    ops: [
      ['update', { mode: 3 }],
      ['update', { mode: 3 }],
      ['get', 'b'],
      ['set', 'b', 'color', [0, 0, 255, 128]],
      ['set', 'a', 'show', [0]],
      ['update', { mode: 3 }],
      ['get', 'a'],
      ['get', 'missing'],
      ['primitive', 'modelMatrix', enu(-74, 41, 0)],
      ['update', { mode: 3 }],
      ['primitive', 'show', false],
      ['update', { mode: 3 }],
      ['primitive', 'show', true],
      ['update', { mode: 3, pick: true, render: false }],
    ],
  },
  translucentProjected: {
    options: { appearance: ['perInstance', { translucent: true, closed: true, flat: true }] },
    instances: [
      { geometry: ['rectangle', [-80, 30, -70, 35], 'flat'], id: 'r', attributes: { color: [1, 1, 0, 0.5], distanceDisplayCondition: [10, 1e8] } },
      { geometry: ['rectangle', [170, -10, 175, -5], 'flat'], id: 's', attributes: { color: [0, 1, 1, 0.75], distanceDisplayCondition: [0, 5e7] } },
    ],
    ops: [
      ['update', { mode: 3 }],
      ['update', { mode: 2 }],
      ['update', { mode: 1 }],
      ['update', { mode: 0, morphTime: 0.5 }],
      ['set', 's', 'distanceDisplayCondition', [100, 200]],
      ['update', { mode: 3 }],
    ],
  },
  material: {
    options: { appearance: ['material', [0.2, 0.4, 0.6, 1], { translucent: false, closed: false, faceForward: true }], modelMatrix: enu(10, 10, 0) },
    instances: [{ geometry: ['ellipsoid', [500, 400, 300], 'basic', 6, 8], id: 'e' }],
    ops: [
      ['update', { mode: 3 }],
      ['appearance', ['material', [1, 0, 0, 0.5], { translucent: true, closed: true }]],
      ['update', { mode: 3 }],
      ['update', { mode: 3 }],
    ],
  },
  depthFail: {
    options: {
      appearance: ['perInstance', { translucent: false, closed: true }],
      depthFailAppearance: ['perInstance', { translucent: true, closed: true }],
      modelMatrix: enu(0, 0, 0),
    },
    instances: [{ geometry: ['box', [10, 10, 10], 'pc'], id: 'd', attributes: { color: [1, 1, 1, 1], depthFailColor: [1, 0, 0, 0.5] } }],
    ops: [['update', { mode: 3 }], ['update', { mode: 3 }]],
  },
  offsets: {
    options: { appearance: ['perInstance', { translucent: false, closed: true }] },
    instances: [
      { geometry: ['box', [1000, 1000, 1000], 'pc', 'ALL'], id: 'o', modelMatrix: enu(5, 5, 0), attributes: { color: [1, 0, 1, 1], offset: [0, 0, 0] } },
      { geometry: ['box', [800, 800, 800], 'pc', 'ALL'], id: 'p', modelMatrix: enu(6, 6, 0), attributes: { color: [0, 0, 1, 1], offset: [10, 20, 30] } },
    ],
    ops: [
      ['update', { mode: 3 }],
      ['get', 'o'],
      ['set', 'o', 'offset', [100, 0, 50]],
      ['update', { mode: 3 }],
      ['get', 'o'],
      ['update', { mode: 1 }],
    ],
  },
  options: {
    options: { appearance: ['perInstance', { translucent: false, closed: false }], allowPicking: false, cull: false, debugShowBoundingVolume: true, shadows: 1, compressVertices: false, interleave: true, modelMatrix: enu(-120, 45, 0) },
    instances: [{ geometry: ['box', [30, 30, 30], 'pc'], attributes: { color: [0.5, 0.5, 0.5, 1] } }],
    ops: [['update', { mode: 3 }]],
  },
  scene3DOnly: {
    options: { appearance: ['perInstance', { translucent: false, closed: true }], modelMatrix: enu(30, 30, 0) },
    instances: [{ geometry: ['box', [30, 30, 30], 'pc'], id: 'x', attributes: { color: [0.5, 0.5, 0.5, 1], distanceDisplayCondition: [0, 1e9] } }],
    ops: [['update', { mode: 3, scene3DOnly: true }], ['update', { mode: 3, scene3DOnly: true }]],
  },
  noAttributes: {
    options: { appearance: ['material', [0, 1, 0, 1], { translucent: false, closed: true }], modelMatrix: enu(1, 2, 3) },
    instances: [{ geometry: ['box', [5, 6, 7], 'basic'] }],
    ops: [['update', { mode: 3 }]],
  },
  empty: { options: { appearance: ['perInstance', {}] }, instances: [], ops: [['update', { mode: 3 }]] },
};

let browser;
try {
  browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, headless: true, args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
  const page = await browser.newPage();
  page.on('pageerror', (e) => console.error(e));
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
    const matrix = (m) => {
      if (m[0] === 'enu') return ref.Transforms.eastNorthUpToFixedFrame(ref.Cartesian3.fromDegrees(m[1], m[2], m[3]));
      return ref.Matrix4.fromArray(m);
    };
    const formats = { pc: ref.PerInstanceColorAppearance.VERTEX_FORMAT, flat: ref.PerInstanceColorAppearance.FLAT_VERTEX_FORMAT, basic: ref.MaterialAppearance.MaterialSupport.BASIC.vertexFormat, all: ref.VertexFormat.ALL };
    const geometry = ([kind, a, f, b, c]) => {
      if (kind === 'box') return ref.BoxGeometry.fromDimensions({ dimensions: new ref.Cartesian3(...a), vertexFormat: formats[f], offsetAttribute: b ? ref.GeometryOffsetAttribute[b] : undefined });
      if (kind === 'rectangle') return new ref.RectangleGeometry({ rectangle: ref.Rectangle.fromDegrees(...a), vertexFormat: formats[f] });
      return new ref.EllipsoidGeometry({ radii: new ref.Cartesian3(...a), vertexFormat: formats[f], stackPartitions: b, slicePartitions: c });
    };
    const attributes = (o) => {
      const out = {};
      for (const [k, v] of Object.entries(o ?? {})) {
        if (k === 'color' || k === 'depthFailColor') out[k] = ref.ColorGeometryInstanceAttribute.fromColor(new ref.Color(...v));
        else if (k === 'show') out[k] = new ref.ShowGeometryInstanceAttribute(v);
        else if (k === 'distanceDisplayCondition') out[k] = new ref.DistanceDisplayConditionGeometryInstanceAttribute(...v);
        else if (k === 'offset') out[k] = ref.OffsetGeometryInstanceAttribute.fromCartesian3(new ref.Cartesian3(...v));
      }
      return out;
    };
    const appearance = ([kind, a, b]) => {
      if (kind === 'perInstance') return new ref.PerInstanceColorAppearance(a);
      return new ref.MaterialAppearance({ material: ref.Material.fromType('Color', { color: new ref.Color(...a) }), ...b });
    };
    const rsSummary = (rs) => rs && {
      cull: [rs.cull.enabled, rs.cull.face], depthTest: [rs.depthTest.enabled, rs.depthTest.func], depthMask: rs.depthMask,
      blending: rs.blending.enabled, depthRange: [rs.depthRange.near, rs.depthRange.far], colorMask: [rs.colorMask.red, rs.colorMask.green, rs.colorMask.blue, rs.colorMask.alpha],
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
    const out = { limits: { maximumTextureSize: ref.ContextLimits.maximumTextureSize }, scenarios: {} };
    for (const [name, s] of Object.entries(scenarios)) {
      const canvas = document.createElement('canvas');
      canvas.width = 8; canvas.height = 8;
      const context = new ref.Context(canvas, { webgl: { antialias: false } });
      let uploads = [];
      const gl = context._gl;
      const view = (d) => new Uint8Array(d.buffer ?? d, d.byteOffset ?? 0, d.byteLength);
      const wrap = (fn, record) => { const original = gl[fn].bind(gl); gl[fn] = (...args) => { record(...args); return original(...args); }; };
      wrap('bufferData', (target, data, usage) => { if (typeof data === 'object') uploads.push(['bufferData', target, usage, data.byteLength, fnv(view(data))]); });
      wrap('bufferSubData', (target, offset, data) => uploads.push(['bufferSubData', target, offset, data.byteLength, fnv(view(data))]));
      wrap('texImage2D', (...a) => { const d = a[8]; if (ArrayBuffer.isView(d)) uploads.push(['texture', a[3], a[4], a[7], d.byteLength, fnv(view(d))]); });
      wrap('texSubImage2D', (...a) => { const d = a[8]; if (ArrayBuffer.isView(d)) uploads.push(['texture', a[4], a[5], a[7], d.byteLength, fnv(view(d))]); });
      wrap('vertexAttribPointer', (index, size, type, normalized, stride, offset) => uploads.push(['pointer', index, size, type, normalized, stride, offset]));
      const options = Object.fromEntries(Object.entries(s.options).map(([k, v]) => [k, k.endsWith('ppearance') ? appearance(v) : k === 'modelMatrix' ? matrix(v) : v]));
      const instances = s.instances.map((i) => new ref.GeometryInstance({ geometry: geometry(i.geometry), id: i.id, modelMatrix: i.modelMatrix ? matrix(i.modelMatrix) : undefined, attributes: attributes(i.attributes) }));
      const primitive = new ref.Primitive({ ...options, geometryInstances: instances, asynchronous: false });
      const cam = camera();
      const steps = [];
      const programs = [];
      const programIndex = (sp) => { if (!sp) return null; let i = programs.indexOf(sp); if (i < 0) { programs.push(sp); i = programs.length - 1; } return i; };
      const typed = (v) => (v ? Array.from(v) : null);
      for (const op of s.ops) {
        const [kind, a, b, c] = op;
        if (kind === 'get') {
          const attrs = primitive.getGeometryInstanceAttributes(a);
          if (!attrs) { steps.push({ op, attributes: null }); continue; }
          const values = {};
          for (const key of Object.keys(primitive._batchTableAttributeIndices)) values[key] = typed(attrs[key]);
          values.boundingSphere = sphere(attrs.boundingSphere);
          values.boundingSphereCV = sphere(attrs.boundingSphereCV);
          values.pickColor = (({ red, green, blue, alpha }) => [red, green, blue, alpha])(attrs.pickId.color);
          steps.push({ op, attributes: values });
          continue;
        }
        if (kind === 'set') { primitive.getGeometryInstanceAttributes(a)[b] = c; continue; }
        if (kind === 'primitive') { primitive[a] = a === 'modelMatrix' ? matrix(b) : b; continue; }
        if (kind === 'appearance') { primitive.appearance = appearance(a); continue; }
        uploads = [];
        const frameState = {
          mode: a.mode, morphTime: a.morphTime ?? (a.mode === 3 ? 1 : 0), mapProjection: new ref.GeographicProjection(), context, camera: cam,
          commandList: [], afterRender: [], passes: { render: a.render ?? true, pick: a.pick ?? false }, scene3DOnly: a.scene3DOnly ?? false,
        };
        primitive.update(frameState);
        for (const callback of frameState.afterRender) callback();
        const vas = primitive._va ?? [];
        const rsName = (rs) => (rs === primitive._frontFaceRS ? 'front' : rs === primitive._backFaceRS ? 'back' : rs === primitive._frontFaceDepthFailRS ? 'frontDepthFail' : rs === primitive._backFaceDepthFailRS ? 'backDepthFail' : 'other');
        const commands = frameState.commandList.map((command) => ({
          pass: command.pass, primitiveType: command.primitiveType, vaIndex: vas.indexOf(command.vertexArray),
          renderState: rsName(command.renderState), program: command.shaderProgram === primitive._sp ? 'color' : command.shaderProgram === primitive._spDepthFail ? 'depthFail' : 'other',
          boundingVolume: sphere(command.boundingVolume), modelMatrix: ref.Matrix4.toArray(command.modelMatrix),
          cull: command.cull, debugShowBoundingVolume: command.debugShowBoundingVolume, castShadows: command.castShadows, receiveShadows: command.receiveShadows,
          pickId: command.pickId ?? null, uniforms: Object.keys(command.uniformMap).sort(),
        }));
        const shader = (sp) => sp && { vs: sp._vertexShaderSource.sources, fs: sp._fragmentShaderSource.sources, defines: [sp._vertexShaderSource.defines, sp._fragmentShaderSource.defines] };
        const bt = primitive._batchTable;
        steps.push({
          op, state: primitive._state, ready: primitive.ready, uploads, commands,
          vaCount: vas.length, primitiveType: primitive._primitiveType ?? null, attributeLocations: primitive._attributeLocations ?? null,
          sp: shader(primitive._sp), spDepthFail: shader(primitive._spDepthFail), programs: [programIndex(primitive._sp), programIndex(primitive._spDepthFail)],
          renderStates: { front: rsSummary(primitive._frontFaceRS), back: rsSummary(primitive._backFaceRS), frontDepthFail: rsSummary(primitive._frontFaceDepthFailRS), backDepthFail: rsSummary(primitive._backFaceDepthFailRS), backIsFront: primitive._backFaceRS === primitive._frontFaceRS },
          boundingSpheres: { local: primitive._boundingSpheres.map(sphere), wc: primitive._boundingSphereWC.map(sphere), cv: primitive._boundingSphereCV.map(sphere), twoD: primitive._boundingSphere2D.map(sphere), morph: primitive._boundingSphereMorph.map(sphere) },
          instanceBoundingSpheres: (primitive._instanceBoundingSpheres ?? []).map(sphere), instanceBoundingSpheresCV: (primitive._instanceBoundingSpheresCV ?? []).map(sphere),
          batch: bt ? { attributes: bt.attributes.map((x) => x.functionName), values: bt._batchValues ? fnv(new Uint8Array(bt._batchValues.buffer)) : null, indices: primitive._batchTableAttributeIndices, boundingSphereIndices: primitive._batchTableBoundingSphereAttributeIndices, offset2D: primitive._batchTableOffsetAttribute2DIndex ?? null } : null,
          modelMatrix: ref.Matrix4.toArray(primitive.modelMatrix), geometryInstancesReleased: primitive.geometryInstances === undefined,
          offsetsUpdated: primitive._batchTableOffsetsUpdated, boundingSpheresUpdated: primitive._batchTableBoundingSpheresUpdated, recompute: primitive._recomputeBoundingSpheres,
        });
      }
      out.scenarios[name] = steps;
      primitive.destroy();
      context.destroy();
    }
    return out;
  }, scenarios);
  writeFileSync(new URL('../../tests/data/scene-primitive.json', import.meta.url), JSON.stringify({ scenarios, capture: result }) + '\n');
  console.log('captured', Object.keys(result.scenarios).length, 'scenarios');
} finally {
  await browser?.close();
  server.close();
}
