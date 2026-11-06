// BillboardCollection reference capture from the pinned CesiumJS engine, run
// on a real WebGL2 context (SwiftShader in headless Chromium). After every
// update it records the vertex-array-facade CPU buffers, every buffer upload,
// the draw commands, shader defines, the texture atlas and each billboard's
// image state. Every op is followed by a task-queue turn, as in an
// application, so image promises settle between ops; after each update the
// frame's afterRender callbacks (the atlas update) run.
// Usage: CHROMIUM_PATH=/path/to/chrome node generate-scene-billboard-collection.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const modules = {
  Scene: ['BillboardCollection', 'Billboard', 'Camera', 'SceneMode', 'BlendOption', 'BillboardLoadState'],
  Renderer: ['Context', 'ContextLimits', 'Pass'],
  Core: ['Cartesian2', 'Cartesian3', 'Color', 'NearFarScalar', 'DistanceDisplayCondition', 'Matrix4', 'BoundingRectangle', 'GeographicProjection', 'Ellipsoid', 'PerspectiveFrustum'],
};
const contents = Object.entries(modules)
  .flatMap(([dir, names]) => names.map((n) => `export {default as ${n}} from '@cesium/engine/Source/${dir}/${n}.js';`))
  .join('\n');
const bundle = await build({ stdin: { contents, resolveDir: fileURLToPath(new URL('.', import.meta.url)) }, bundle: true, write: false, format: 'esm', platform: 'browser' });
const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') { res.setHeader('Content-Type', 'text/javascript'); res.end(bundle.outputFiles[0].contents); return; }
  res.end('<!doctype html><title>BillboardCollection capture</title>');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

// Earth-fixed positions in metres.
const P = [
  [6378137.0, 0.0, 0.0],
  [4510023.92, 4510023.92, 1000.5],
  [-1000.25, 5000000.75, 3500000.5],
  [1215107.48, -4736682.9, 4081926.1],
];
// Images: { pixels: [width, height, seed] } is RGBA byte k = (k * 31 + seed * 17) & 255;
// 'reject' is a callback whose image promise rejects.
const img = (w, h, seed) => ({ pixels: [w, h, seed] });
const styled = [
  ['add', { position: P[0], image: img(4, 4, 1), imageId: 'a' }],
  ['add', { position: P[1], image: img(8, 2, 2), imageId: 'b', pixelOffset: [3, -7], eyeOffset: [1, 2, -30], horizontalOrigin: 1, verticalOrigin: -1, scale: 2.5, color: [0.25, 0.5, 0.75, 0.5], id: 'second' }],
  ['add', { position: P[2], image: img(3, 5, 3), imageId: 'c', rotation: 0.75, alignedAxis: [0, 0, 1], sizeInMeters: true, verticalOrigin: 2, horizontalOrigin: -1 }],
  ['add', { position: P[3], width: 12, height: 6, translucencyByDistance: [1e3, 1, 2e7, 0.25], scaleByDistance: [1e3, 2, 1e7, 0.5], pixelOffsetScaleByDistance: [10, 1.5, 1e6, 0], distanceDisplayCondition: [10, 3e7], disableDepthTestDistance: 'Infinity', splitDirection: 1 }],
  ['update', { mode: 3 }],
  ['update', { mode: 3 }],
  ['update', { mode: 3 }],
];
const many = [
  ...Array.from({ length: 30 }, (_, i) => ['add', { position: [P[i % 4][0] + i, P[i % 4][1] - i * 2, P[i % 4][2] + i * 3], image: img(2 + (i % 3), 2, 4), imageId: `m${i % 3}`, scale: 1 + (i % 4) * 0.25 }]),
  ['update', { mode: 3 }],
  ['update', { mode: 3 }],
  ['update', { mode: 3 }],
  ['set', 3, 'color', [0.5, 0.5, 0.5, 1]],
  ['update', { mode: 3 }],
  ['set', 7, 'pixelOffset', [4, 4]],
  ['set', 8, 'eyeOffset', [0, 0, 5]],
  ['set', 9, 'rotation', 0.5],
  ['update', { mode: 3 }],
  ...[0, 1, 2, 3, 4].map((i) => ['set', i, 'scale', 3]),
  ['set', 10, 'alignedAxis', [1, 0, 0]],
  ['set', 11, 'translucencyByDistance', [5, 0.5, 50, 1]],
  ['update', { mode: 3 }],
  ['set', 12, 'scaleByDistance', [5, 1, 50, 2]],
  ['set', 13, 'pixelOffsetScaleByDistance', [5, 1, 50, 3]],
  ['set', 14, 'distanceDisplayCondition', [0, 100]],
  ['set', 15, 'disableDepthTestDistance', 10],
  ['set', 16, 'splitDirection', -1],
  ['set', 17, 'horizontalOrigin', 1],
  ['set', 18, 'verticalOrigin', 1],
  ['set', 19, 'show', false],
  ['set', 20, 'sizeInMeters', true],
  ['set', 21, 'width', 9],
  ['set', 22, 'position', P[3]],
  ['update', { mode: 3 }],
  ['update', { mode: 3 }],
];
const scenarios = {
  styled,
  many,
  images: [
    ['add', { position: P[0], image: img(6, 6, 5), imageId: 'shared' }],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
    ['add', { position: P[1], image: img(6, 6, 5), imageId: 'shared' }],
    ['add', { position: P[2] }],
    ['update', { mode: 3 }],
    ['call', 2, 'setImage', 'late', img(5, 7, 6)],
    ['call', 0, 'setImageSubRegion', 'shared', [1, 2, 3, 4]],
    ['update', { mode: 3 }],
    ['call', 1, 'setImageSubRegion', 'shared', [1, 2, 3, 4]],
    ['update', { mode: 3 }],
    ['set', 0, 'image', null],
    ['set', 1, 'image', img(2, 9, 7)],
    ['update', { mode: 3 }],
    ['add', { position: P[3], image: 'reject', imageId: 'broken' }],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
  ],
  growth: [
    ...Array.from({ length: 6 }, (_, i) => ['add', { position: P[i % 4], image: img(5 + i, 4 + (i % 2), 8 + i), imageId: `g${i}` }]),
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
    ...Array.from({ length: 4 }, (_, i) => ['add', { position: P[(i + 1) % 4], image: img(11, 9, 20 + i), imageId: `big${i}` }]),
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
  ],
  removal: [
    ...[0, 1, 2, 3].map((i) => ['add', { position: P[i], image: img(2, 2, 9), imageId: 'r', id: `p${i}` }]),
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
    ['remove', 1],
    ['update', { mode: 3 }],
    ['add', { position: P[1], color: [1, 1, 0, 1] }],
    ['update', { mode: 3 }],
    ['removeAll'],
    ['update', { mode: 3 }],
  ],
  blendOptions: [
    ['add', { position: P[0], image: img(2, 2, 1), imageId: 'x' }],
    ['add', { position: P[1], color: [1, 1, 1, 0.5], image: img(2, 2, 1), imageId: 'x' }],
    ['collection', 'blendOption', 0],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
    ['collection', 'blendOption', 2],
    ['update', { mode: 3 }],
    ['collection', 'blendOption', 1],
    ['update', { mode: 3, minimumDisableDepthTestDistance: 100 }],
  ],
  projected: [
    ['add', { position: P[0], image: img(3, 3, 2), imageId: 'q', pixelOffset: [2, 2] }],
    ['add', { position: P[1], image: img(3, 3, 2), imageId: 'q', eyeOffset: [0, 0, 10] }],
    ['collection', 'modelMatrix', [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 10, 20, 30, 1]],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
    ['update', { mode: 1 }],
    ['set', 1, 'position', P[3]],
    ['update', { mode: 1 }],
    ['update', { mode: 2 }],
    ['update', { mode: 0, morphTime: 0.5 }],
    ['update', { mode: 3 }],
  ],
  picking: [
    ['add', { position: P[0], id: 'a', image: img(2, 2, 3), imageId: 'p' }],
    ['add', { position: P[1], id: 'b', color: [1, 1, 1, 0], image: img(2, 2, 3), imageId: 'p' }],
    ['update', { mode: 3 }],
    ['update', { mode: 3, pick: true, render: false }],
    ['set', 0, 'color', [0, 0, 1, 1]],
    ['update', { mode: 3, pick: true, render: false }],
    ['update', { mode: 3 }],
    ['collection', 'show', false],
    ['update', { mode: 3 }],
    ['collection', 'show', true],
    ['collection', 'debugShowBoundingVolume', true],
    ['update', { mode: 3 }],
  ],
  // Bottom-aligned but horizontally centered (no halving), a fractional
  // width, sub-regions requested by billboards without an image (a new one
  // loads; a cached one is set synchronously and stays NONE), and two image
  // loads in flight on one billboard.
  // The signed-distance-field outline attributes labels use (`_sdf`).
  sdf: [
    ['collection', '_sdf', true],
    ['add', { position: P[0], image: img(4, 4, 15), imageId: 's', outlineColor: [1, 0, 0, 0.5], outlineWidth: 2 }],
    ['add', { position: P[1], image: img(4, 4, 15), imageId: 's' }],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
    ['set', 1, 'outlineWidth', 4],
    ['update', { mode: 3 }],
    ['set', 0, 'outlineColor', [0, 1, 0, 1]],
    ['update', { mode: 3 }],
  ],
  edgeCases: [
    ['add', { position: P[0], image: img(4, 4, 11), imageId: 'e', verticalOrigin: 1, width: 7.4 }],
    ['add', { position: P[1] }],
    ['add', { position: P[2] }],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
    ['call', 1, 'setImageSubRegion', 'e', [0, 0, 2, 2]],
    ['update', { mode: 3 }],
    ['call', 2, 'setImageSubRegion', 'e', [0, 0, 2, 2]],
    ['add', { position: P[3], image: img(3, 3, 12), imageId: 'd1' }],
    ['call', 3, 'setImage', 'd2', img(5, 2, 13)],
    ['add', { position: P[0], image: img(6, 3, 14), imageId: 'late' }],
    ['call', 4, 'setImage', 'e', img(4, 4, 11)],
    ['update', { mode: 3 }],
    ['update', { mode: 3 }],
  ],
};
// The batched (non-instanced) writers on a context reporting no instancing.
for (const name of ['styled', 'many', 'images', 'edgeCases', 'sdf']) scenarios[`${name}Batched`] = scenarios[name];

let browser;
try {
  browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, headless: true, args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
  const page = await browser.newPage();
  page.on('console', (m) => { if (m.type() === 'error' && !m.text().startsWith('Error loading image for billboard')) console.error(m.text()); });
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const result = await page.evaluate(async (scenarios) => {
    const ref = await import('/reference.mjs');
    const tick = () => new Promise((r) => setTimeout(r, 0));
    const fnv = (bytes) => {
      let h = 0xcbf29ce484222325n;
      for (const b of bytes) h = BigInt.asUintN(64, (h ^ BigInt(b)) * 0x100000001b3n);
      return h.toString(16);
    };
    // Vertex data is float32; every NaN is hashed as 0x7fc00000, since the
    // payload V8 leaves in an uninitialized attribute is engine-specific.
    const canon = (bytes) => {
      const out = new Uint8Array(bytes);
      const view = new DataView(out.buffer);
      for (let k = 0; k + 4 <= out.length; k += 4) if (Number.isNaN(view.getFloat32(k, true))) view.setUint32(k, 0x7fc00000, true);
      return out;
    };
    const b64 = (bytes) => { let s = ''; for (const b of bytes) s += String.fromCharCode(b); return btoa(s); };
    const num = (v) => (v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Number.isNaN(v) ? 'NaN' : v);
    const c3 = (v) => new ref.Cartesian3(...v);
    const image = (spec) => {
      if (spec === 'reject') return () => Promise.reject(new Error('missing image'));
      const [w, h, seed] = spec.pixels;
      const bytes = new Uint8ClampedArray(w * h * 4);
      for (let k = 0; k < bytes.length; k++) bytes[k] = (k * 31 + seed * 17) & 255;
      return new ImageData(bytes, w, h);
    };
    const value = (prop, v) => {
      if (v === null) return undefined;
      if (v === 'Infinity') return Infinity;
      if (['position', 'eyeOffset', 'alignedAxis'].includes(prop)) return c3(v);
      if (prop === 'pixelOffset') return new ref.Cartesian2(...v);
      if (prop === 'color' || prop === 'outlineColor') return new ref.Color(...v);
      if (['scaleByDistance', 'translucencyByDistance', 'pixelOffsetScaleByDistance'].includes(prop)) return new ref.NearFarScalar(...v);
      if (prop === 'distanceDisplayCondition') return new ref.DistanceDisplayCondition(...v);
      if (prop === 'modelMatrix') return ref.Matrix4.fromArray(v);
      if (prop === 'image') return image(v);
      if (prop === 'imageSubRegion') return new ref.BoundingRectangle(...v);
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

    const out = { limits: null, scenarios: {} };
    for (const [name, script] of Object.entries(scenarios)) {
      const canvas = document.createElement('canvas');
      canvas.width = 8; canvas.height = 8;
      const context = new ref.Context(canvas, { webgl: { antialias: false } });
      const batched = name.endsWith('Batched');
      if (batched) Object.defineProperty(context, 'instancedArrays', { get: () => false });
      out.limits = { maximumVertexTextureImageUnits: ref.ContextLimits.maximumVertexTextureImageUnits, maximumAliasedPointSize: ref.ContextLimits.maximumAliasedPointSize };
      // Every buffer upload: [call, target, usage or byte offset, byte length, hash].
      let uploads = [];
      const gl = context._gl;
      const view = (data) => new Uint8Array(data.buffer ?? data, data.byteOffset ?? 0, data.byteLength);
      const bufferData = gl.bufferData.bind(gl);
      gl.bufferData = (target, data, usage) => {
        if (typeof data === 'object') uploads.push(['bufferData', target, usage, data.byteLength, fnv(target === gl.ARRAY_BUFFER ? canon(view(data)) : view(data))]);
        return bufferData(target, data, usage);
      };
      const bufferSubData = gl.bufferSubData.bind(gl);
      gl.bufferSubData = (target, offset, data) => {
        uploads.push(['bufferSubData', target, offset, data.byteLength, fnv(target === gl.ARRAY_BUFFER ? canon(view(data)) : view(data))]);
        return bufferSubData(target, offset, data);
      };
      const collection = new ref.BillboardCollection();
      const atlas = collection.textureAtlas;
      const billboards = [];
      const cam = camera();
      const steps = [];
      let lastSp; let lastSpTranslucent; let lastGuid = atlas.guid;
      for (const op of script) {
        const [kind, a, b, c, d] = op;
        uploads = [];
        if (kind === 'add') billboards.push(collection.add(options(a)));
        else if (kind === 'set') billboards[a][b] = value(b, c);
        else if (kind === 'call' && b === 'setImage') billboards[a].setImage(c, image(d));
        else if (kind === 'call' && b === 'setImageSubRegion') billboards[a].setImageSubRegion(c, new ref.BoundingRectangle(...d));
        else if (kind === 'remove') steps.push({ op, removed: collection.remove(billboards[a]) });
        else if (kind === 'removeAll') collection.removeAll();
        else if (kind === 'collection') collection[a] = value(a, b);
        else if (kind === 'update') {
          const frameState = {
            mode: a.mode, morphTime: a.morphTime ?? (a.mode === 3 ? 1 : 0), mapProjection: new ref.GeographicProjection(),
            context, camera: cam, commandList: [], afterRender: [], passes: { render: a.render ?? true, pick: a.pick ?? false },
            minimumDisableDepthTestDistance: a.minimumDisableDepthTestDistance ?? 0,
          };
          collection.update(frameState);
          const updateUploads = uploads;
          const vaf = collection._vaf;
          const va = vaf?.va ?? [];
          const buffers = (vaf?._allBuffers ?? []).map((buffer) => {
            const bytes = canon(new Uint8Array(buffer.arrayBuffer ?? new ArrayBuffer(0)));
            return { usage: buffer.usage, vertexSizeInBytes: buffer.vertexSizeInBytes, hash: fnv(bytes), length: bytes.length, bytes: bytes.length <= 4096 ? b64(bytes) : null };
          });
          const commands = frameState.commandList.map((command) => {
            const rs = command.renderState;
            const u = command.uniformMap;
            const hc = u.u_highlightColor();
            return {
              pass: command.pass, primitiveType: command.primitiveType,
              vaIndex: va.findIndex((v) => v.va === command.vertexArray),
              opaqueProgram: command.shaderProgram === collection._sp,
              renderState: { depthTest: rs.depthTest.enabled, depthFunc: rs.depthTest.func, depthMask: rs.depthMask, blending: rs.blending.enabled },
              boundingVolume: [command.boundingVolume.center.x, command.boundingVolume.center.y, command.boundingVolume.center.z, command.boundingVolume.radius].map(num),
              modelMatrix: ref.Matrix4.toArray(command.modelMatrix),
              pickId: command.pickId, debugShowBoundingVolume: command.debugShowBoundingVolume,
              count: command.count, instanceCount: command.instanceCount,
              atlasTexture: u.u_atlas() === atlas.texture,
              uniforms: { highlightColor: [hc.red, hc.green, hc.blue, hc.alpha], coarseDepthTestDistance: u.u_coarseDepthTestDistance(), threePointDepthTestDistance: u.u_threePointDepthTestDistance() },
            };
          });
          const program = (sp) => sp && { vs: sp._vertexShaderSource.defines, fs: sp._fragmentShaderSource.defines };
          const sceneStub = { frameState: { mode: 3, morphTime: 1, mapProjection: frameState.mapProjection }, canvas: { clientWidth: 800, clientHeight: 600 }, camera: cam, mapProjection: frameState.mapProjection, mode: 3 };
          const screen = a.mode !== 3 ? null : billboards.map((p) => {
            if (!p._billboardCollection) return null;
            const w = p.computeScreenSpacePosition(sceneStub);
            return w ? [w.x, w.y] : null;
          });
          const step = {
            op, buffers, uploads: updateUploads, vaCount: va.length, commands,
            sp: program(collection._sp), spTranslucent: program(collection._spTranslucent),
            spChanged: collection._sp !== lastSp, spTranslucentChanged: collection._spTranslucent !== lastSpTranslucent,
            flags: {
              rotation: collection._shaderRotation, alignedAxis: collection._shaderAlignedAxis,
              scaleByDistance: collection._shaderScaleByDistance, translucencyByDistance: collection._shaderTranslucencyByDistance,
              pixelOffsetScaleByDistance: collection._shaderPixelOffsetScaleByDistance,
              distanceDisplayCondition: collection._shaderDistanceDisplayCondition, disableDepthDistance: collection._shaderDisableDepthDistance,
              clampToGround: collection._shaderClampToGround,
              maxSize: collection._maxSize, maxEyeOffset: collection._maxEyeOffset, maxScale: collection._maxScale, maxPixelOffset: collection._maxPixelOffset,
              allHorizontalCenter: collection._allHorizontalCenter, allVerticalCenter: collection._allVerticalCenter, allSizedInMeters: collection._allSizedInMeters,
              buffersUsage: [...collection._buffersUsage], propertiesChanged: [...collection._propertiesChanged],
              toUpdateIndex: collection._billboardsToUpdateIndex, length: collection.length, ready: collection.ready,
            },
            screen,
          };
          lastSp = collection._sp; lastSpTranslucent = collection._spTranslucent;
          // The frame ends: afterRender updates the atlas, then queued promises settle.
          uploads = [];
          for (const callback of frameState.afterRender) callback();
          await tick();
          step.afterRenderUploads = uploads;
          step.atlas = {
            texture: atlas.texture ? [atlas.texture.width, atlas.texture.height] : null,
            guidChanged: atlas.guid !== lastGuid,
            rectangles: atlas._rectangles.map((r) => (r ? [r.x, r.y, r.width, r.height] : null)),
          };
          lastGuid = atlas.guid;
          step.billboards = billboards.map((p) => ({
            index: p._index, dirty: p._dirty, textureDirty: p.textureDirty, loadState: p._imageTexture.loadState, ready: p.ready,
            image: p.image ?? null, imageIndex: p._imageTexture._index, width: p.width ?? null, height: p.height ?? null,
            loadError: p.loadError !== undefined, actual: [p._actualPosition.x, p._actualPosition.y, p._actualPosition.z],
          }));
          steps.push(step);
          continue;
        }
        await tick();
      }
      out.scenarios[name] = { camera: { position: [cam.position.x, cam.position.y, cam.position.z], direction: [cam.direction.x, cam.direction.y, cam.direction.z], up: [cam.up.x, cam.up.y, cam.up.z], right: [cam.right.x, cam.right.y, cam.right.z] }, steps };
      collection.destroy();
      context.destroy();
    }
    return out;
  }, scenarios);
  writeFileSync(new URL('../../tests/data/scene-billboard-collection.json', import.meta.url), JSON.stringify({ positions: P, scenarios, capture: result }, null, 1) + '\n');
  console.log('captured', Object.keys(result.scenarios).length, 'scenarios');
} finally {
  await browser?.close();
  server.close();
}
