// PointPrimitiveCollection reference capture from the pinned CesiumJS engine,
// run on a real WebGL2 context (SwiftShader in headless Chromium). After every
// update it records the vertex-array-facade CPU buffers Cesium uploads, the
// draw commands, shader defines and the collection's bookkeeping.
// Usage: CHROMIUM_PATH=/path/to/chrome node generate-scene-point-primitive-collection.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const modules = {
  Scene: ['PointPrimitiveCollection', 'PointPrimitive', 'Camera', 'SceneMode', 'BlendOption'],
  Renderer: ['Context', 'ContextLimits', 'Pass'],
  Core: ['Cartesian3', 'Color', 'NearFarScalar', 'DistanceDisplayCondition', 'Matrix4', 'BoundingSphere', 'GeographicProjection', 'Ellipsoid', 'PerspectiveFrustum', 'Event'],
};
const contents = Object.entries(modules)
  .flatMap(([dir, names]) => names.map((n) => `export {default as ${n}} from '@cesium/engine/Source/${dir}/${n}.js';`))
  .join('\n');
const bundle = await build({ stdin: { contents, resolveDir: fileURLToPath(new URL('.', import.meta.url)) }, bundle: true, write: false, format: 'esm', platform: 'browser' });
const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') { res.setHeader('Content-Type', 'text/javascript'); res.end(bundle.outputFiles[0].contents); return; }
  res.end('<!doctype html><title>PointPrimitiveCollection capture</title>');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

// Scenario scripts, interpreted in the page. Positions are Earth-fixed metres.
const P = [
  [6378137.0, 0.0, 0.0],
  [4510023.92, 4510023.92, 1000.5],
  [-1000.25, 5000000.75, 3500000.5],
  [1215107.48, -4736682.9, 4081926.1],
];
const scenarios = {
  create: [
    ['add', {}],
    ['add', { position: P[1], color: [0.25, 0.5, 0.75, 1], outlineColor: [1, 0, 0, 0.5], outlineWidth: 2, pixelSize: 7, id: 'b' }],
    ['add', { position: P[2], color: [0, 1, 0, 0.5], pixelSize: 3.5, show: false }],
    ['update', { mode: 3 }],
  ],
  dirty: [
    ...[0, 1, 2].map((i) => ['add', { position: P[i], pixelSize: 4 + i }]),
    ['update', { mode: 3 }],
    ['set', 1, 'color', [1, 0, 1, 1]],
    ['update', { mode: 3 }],
    ['set', 0, 'position', P[3]],
    ['set', 2, 'pixelSize', 12],
    ['set', 2, 'outlineWidth', 1.5],
    ['update', { mode: 3 }],
    ['set', 1, 'show', false],
    ['set', 0, 'splitDirection', 1],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
  ],
  many: [
    ...Array.from({ length: 40 }, (_, i) => ['add', { position: [P[i % 4][0] + i, P[i % 4][1] - i * 2, P[i % 4][2] + i * 3], pixelSize: 1 + (i % 7) }]),
    ['update', { mode: 3 }],
    ['set', 3, 'color', [0.5, 0.5, 0.5, 1]],
    ['update', { mode: 3 }],
    ['set', 7, 'color', [0.1, 0.2, 0.3, 1]],
    ['set', 30, 'pixelSize', 20],
    ['update', { mode: 3 }],
  ],
  removal: [
    ...[0, 1, 2, 3].map((i) => ['add', { position: P[i], id: `p${i}` }]),
    ['update', { mode: 3 }],
    ['remove', 1],
    ['update', { mode: 3 }],
    ['add', { position: P[1], color: [1, 1, 0, 1] }],
    ['update', { mode: 3 }],
    ['removeAll'],
    ['update', { mode: 3 }],
  ],
  blendOptions: [
    ['add', { position: P[0] }],
    ['add', { position: P[1], color: [1, 1, 1, 0.5] }],
    ['collection', 'blendOption', 0],
    ['update', { mode: 3 }],
    ['collection', 'blendOption', 2],
    ['update', { mode: 3 }],
    ['collection', 'blendOption', 1],
    ['update', { mode: 3 }],
  ],
  shaderDefines: [
    ['add', { position: P[0] }],
    ['update', { mode: 3 }],
    ['set', 0, 'scaleByDistance', [1e3, 2, 1e7, 0.5]],
    ['update', { mode: 3 }],
    ['set', 0, 'translucencyByDistance', [1e3, 1, 2e7, 0.25]],
    ['update', { mode: 3 }],
    ['set', 0, 'distanceDisplayCondition', [10, 3e7]],
    ['update', { mode: 3 }],
    ['set', 0, 'disableDepthTestDistance', 'Infinity'],
    ['update', { mode: 3 }],
    ['add', { position: P[1], translucencyByDistance: [5, 1, 10, 1], scaleByDistance: [1, 1, 2, 1], disableDepthTestDistance: 50 }],
    ['update', { mode: 3 }],
  ],
  minimumDisableDepth: [
    ['add', { position: P[2] }],
    ['update', { mode: 3, minimumDisableDepthTestDistance: 100 }],
  ],
  projected: [
    ['add', { position: P[0], pixelSize: 5 }],
    ['add', { position: P[1], pixelSize: 9, outlineWidth: 3 }],
    ['collection', 'modelMatrix', [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 10, 20, 30, 1]],
    ['update', { mode: 3 }],
    ['update', { mode: 1 }],
    ['set', 1, 'position', P[3]],
    ['update', { mode: 1 }],
    ['update', { mode: 2 }],
    ['update', { mode: 0, morphTime: 0.5 }],
    ['update', { mode: 3 }],
  ],
  picking: [
    ['add', { position: P[0], id: 'a' }],
    ['add', { position: P[1], id: 'b', color: [1, 1, 1, 0.25] }],
    ['update', { mode: 3, pick: true, render: false }],
    ['set', 0, 'color', [0, 0, 1, 1]],
    ['update', { mode: 3, pick: true, render: false }],
    ['update', { mode: 3 }],
  ],
  hidden: [
    ['add', { position: P[0], color: [1, 1, 1, 0], outlineColor: [0, 0, 0, 0] }],
    ['add', { position: P[1], outlineColor: [0, 0, 0, 0.5], color: [1, 1, 1, 0] }],
    ['update', { mode: 3 }],
    ['collection', 'show', false],
    ['update', { mode: 3 }],
    ['collection', 'show', true],
    ['collection', 'debugShowBoundingVolume', true],
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
    const b64 = (bytes) => btoa(String.fromCharCode(...bytes));
    const num = (v) => (v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Number.isNaN(v) ? 'NaN' : v);
    const c3 = (v) => new ref.Cartesian3(...v);
    const value = (prop, v) => {
      if (v === null) return undefined;
      if (v === 'Infinity') return Infinity;
      if (prop === 'position') return c3(v);
      if (prop === 'color' || prop === 'outlineColor') return new ref.Color(...v);
      if (prop === 'scaleByDistance' || prop === 'translucencyByDistance') return new ref.NearFarScalar(...v);
      if (prop === 'distanceDisplayCondition') return new ref.DistanceDisplayCondition(...v);
      if (prop === 'modelMatrix') return ref.Matrix4.fromArray(v);
      return v;
    };
    const options = (o) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k, value(k, v)]));

    function camera() {
      const scene = {
        drawingBufferWidth: 800, drawingBufferHeight: 600, pixelRatio: 1, mapMode2D: 1,
        ellipsoid: ref.Ellipsoid.WGS84, mapProjection: new ref.GeographicProjection(),
        canvas: { clientWidth: 800, clientHeight: 600 }, frameState: { mode: 3 }, mode: 3,
        screenSpaceCameraController: { minimumZoomDistance: 1, maximumZoomDistance: Infinity },
        verticalExaggeration: 1, verticalExaggerationRelativeHeight: 0,
      };
      const c = new ref.Camera(scene);
      c.frustum = new ref.PerspectiveFrustum({ fov: Math.PI / 3, aspectRatio: 4 / 3, near: 1, far: 1e9 });
      c.position = new ref.Cartesian3(2e7, 1e6, 5e6);
      c.direction = ref.Cartesian3.normalize(ref.Cartesian3.negate(c.position, new ref.Cartesian3()), new ref.Cartesian3());
      c.right = ref.Cartesian3.normalize(ref.Cartesian3.cross(c.direction, ref.Cartesian3.UNIT_Z, new ref.Cartesian3()), new ref.Cartesian3());
      c.up = ref.Cartesian3.cross(c.right, c.direction, new ref.Cartesian3());
      return c;
    }

    const out = { maximumAliasedPointSize: null, scenarios: {} };
    for (const [name, script] of Object.entries(scenarios)) {
      const canvas = document.createElement('canvas');
      canvas.width = 8; canvas.height = 8;
      const context = new ref.Context(canvas, { webgl: { antialias: false } });
      out.maximumAliasedPointSize = ref.ContextLimits.maximumAliasedPointSize;
      // Every vertex upload: [call, usage or byte offset, byte length, hash].
      let uploads = [];
      const gl = context._gl;
      const view = (data) => new Uint8Array(data.buffer ?? data, data.byteOffset ?? 0, data.byteLength);
      const bufferData = gl.bufferData.bind(gl);
      gl.bufferData = (target, data, usage) => {
        if (target === gl.ARRAY_BUFFER && typeof data === 'object') uploads.push(['bufferData', usage, data.byteLength, fnv(view(data))]);
        return bufferData(target, data, usage);
      };
      const bufferSubData = gl.bufferSubData.bind(gl);
      gl.bufferSubData = (target, offset, data) => {
        if (target === gl.ARRAY_BUFFER) uploads.push(['bufferSubData', offset, data.byteLength, fnv(view(data))]);
        return bufferSubData(target, offset, data);
      };
      const collection = new ref.PointPrimitiveCollection();
      const points = [];
      const cam = camera();
      const steps = [];
      let lastSp; let lastSpTranslucent;
      for (const op of script) {
        const [kind, a, b, c] = op;
        if (kind === 'add') { points.push(collection.add(options(a))); continue; }
        if (kind === 'set') { points[a][b] = value(b, c); continue; }
        if (kind === 'remove') { steps.push({ op, removed: collection.remove(points[a]) }); continue; }
        if (kind === 'removeAll') { collection.removeAll(); continue; }
        if (kind === 'collection') { collection[a] = value(a, b); continue; }
        // update
        const frameState = {
          mode: a.mode, morphTime: a.morphTime ?? (a.mode === 3 ? 1 : 0), mapProjection: new ref.GeographicProjection(),
          context, camera: cam, commandList: [], passes: { render: a.render ?? true, pick: a.pick ?? false },
          minimumDisableDepthTestDistance: a.minimumDisableDepthTestDistance ?? 0,
        };
        uploads = [];
        collection.update(frameState);
        const vaf = collection._vaf;
        const va = vaf?.va ?? [];
        const buffers = (vaf?._allBuffers ?? []).map((buffer) => {
          const bytes = new Uint8Array(buffer.arrayBuffer ?? new ArrayBuffer(0));
          return { usage: buffer.usage, vertexSizeInBytes: buffer.vertexSizeInBytes, hash: fnv(bytes), length: bytes.length, bytes: bytes.length <= 2048 ? b64(bytes) : null };
        });
        const commands = frameState.commandList.map((command) => {
          const rs = command.renderState;
          return {
            pass: command.pass, primitiveType: command.primitiveType,
            vaIndex: va.findIndex((v) => v.va === command.vertexArray),
            opaqueProgram: command.shaderProgram === collection._sp,
            renderState: { depthTest: rs.depthTest.enabled, depthFunc: rs.depthTest.func, depthMask: rs.depthMask, blending: rs.blending.enabled },
            boundingVolume: [command.boundingVolume.center.x, command.boundingVolume.center.y, command.boundingVolume.center.z, command.boundingVolume.radius].map(num),
            modelMatrix: ref.Matrix4.toArray(command.modelMatrix),
            pickId: command.pickId, debugShowBoundingVolume: command.debugShowBoundingVolume,
            maxTotalPointSize: command.uniformMap.u_maxTotalPointSize(),
          };
        });
        // computeScreenSpacePosition through a scene stub sharing the camera (3D only).
        const sceneStub = { frameState: { mode: 3, morphTime: 1, mapProjection: frameState.mapProjection }, canvas: { clientWidth: 800, clientHeight: 600 }, camera: cam, mapProjection: frameState.mapProjection };
        const screen = a.mode !== 3 ? null : points.map((p) => {
          if (!p._pointPrimitiveCollection) return null;
          const w = p.computeScreenSpacePosition(sceneStub);
          return w ? [w.x, w.y] : null;
        });
        const program = (sp) => sp && { vs: sp._vertexShaderSource.defines, fs: sp._fragmentShaderSource.defines };
        steps.push({
          op,
          buffers, uploads, vaCount: va.length, commands,
          sp: program(collection._sp), spTranslucent: program(collection._spTranslucent),
          spChanged: collection._sp !== lastSp, spTranslucentChanged: collection._spTranslucent !== lastSpTranslucent,
          flags: {
            scaleByDistance: collection._shaderScaleByDistance, translucencyByDistance: collection._shaderTranslucencyByDistance,
            distanceDisplayCondition: collection._shaderDistanceDisplayCondition, disableDepthDistance: collection._shaderDisableDepthDistance,
            maxPixelSize: collection._maxPixelSize, buffersUsage: [...collection._buffersUsage], propertiesChanged: [...collection._propertiesChanged],
            toUpdateIndex: collection._pointPrimitivesToUpdateIndex, length: collection.length,
          },
          screen,
          points: points.map((p) => ({ index: p._index, dirty: p._dirty, actual: [p._actualPosition.x, p._actualPosition.y, p._actualPosition.z] })),
        });
        lastSp = collection._sp; lastSpTranslucent = collection._spTranslucent;
      }
      out.scenarios[name] = { camera: { position: [cam.position.x, cam.position.y, cam.position.z], direction: [cam.direction.x, cam.direction.y, cam.direction.z], up: [cam.up.x, cam.up.y, cam.up.z], right: [cam.right.x, cam.right.y, cam.right.z] }, steps };
      collection.destroy();
      context.destroy();
    }
    return out;
  }, scenarios);
  writeFileSync(new URL('../../tests/data/scene-point-primitive-collection.json', import.meta.url), JSON.stringify({ positions: P, scenarios, capture: result }, null, 1) + '\n');
  console.log('captured', Object.keys(result.scenarios).length, 'scenarios');
} finally {
  await browser?.close();
  server.close();
}
