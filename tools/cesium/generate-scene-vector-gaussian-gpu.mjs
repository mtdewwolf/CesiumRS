// SwiftShader WebGL2 reference captures from pinned @cesium/engine 23.0.0
// covering all 10 Cluster 3 Scene Vector 3D Tiles & Gaussian Splats modules:
// 1. createBillboardPointCallback
// 2. Vector3DTilePrimitive
// 3. Vector3DTileGeometry
// 4. Vector3DTilePolygons
// 5. Vector3DTilePolylines
// 6. Vector3DTileClampedPolylines
// 7. Vector3DTilePoints (+ Cesium3DTilePointFeature, BillboardCollection, LabelCollection, PolylineCollection)
// 8. Vector3DTileContent
// 9. GaussianSplat3DTileContent
// 10. GaussianSplatPrimitive
//
// Usage:
//   CHROMIUM_PATH=/home/deck/.cache/ms-playwright/chromium_headless_shell-1248/chrome-headless-shell-linux64/chrome-headless-shell \
//   node tools/cesium/generate-scene-vector-gaussian-gpu.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const modules = {
  Scene: [
    'Vector3DTilePrimitive',
    'Vector3DTileGeometry',
    'Vector3DTilePolygons',
    'Vector3DTilePolylines',
    'Vector3DTileClampedPolylines',
    'Vector3DTilePoints',
    'Vector3DTileContent',
    'Vector3DTileBatch',
    'Cesium3DTileBatchTable',
    'Cesium3DTileStyle',
    'Cesium3DTilePointFeature',
    'ClassificationType',
    'HeightReference',
    'HorizontalOrigin',
    'VerticalOrigin',
    'LabelStyle',
    'SceneMode',
    'SplitDirection',
    'StencilConstants',
    'StencilFunction',
    'StencilOperation',
    'BlendingState',
    'CullFace',
    'GaussianSplatPrimitive',
    'GaussianSplat3DTileContent',
    'GaussianSplatSorter',
    'GaussianSplatTextureGenerator',
    'createBillboardPointCallback',
    'Camera',
    'Model/ModelUtility',
    'Axis',
  ],
  Renderer: [
    'Context',
    'ContextLimits',
    'Pass',
    'PixelDatatype',
    'RenderState',
    'TextureAtlas',
  ],
  Core: [
    'ApproximateTerrainHeights',
    'BoundingSphere',
    'Cartesian2',
    'Cartesian3',
    'Cartesian4',
    'Cartographic',
    'Color',
    'ComponentDatatype',
    'Ellipsoid',
    'GeographicProjection',
    'IndexDatatype',
    'JulianDate',
    'Matrix4',
    'OrientedBoundingBox',
    'PixelFormat',
    'PrimitiveType',
    'Quaternion',
    'Rectangle',
    'Resource',
    'TaskProcessor',
    'Transforms',
    'buildModuleUrl',
  ],
};

const workerNames = [
  'createVectorTilePoints',
  'createVectorTilePolylines',
  'createVectorTileClampedPolylines',
  'createVectorTilePolygons',
  'createVectorTileGeometries',
  'gaussianSplatSorter',
  'gaussianSplatTextureGenerator',
];

const contents = [
  ...Object.entries(modules).flatMap(([dir, names]) =>
    names.map((n) => {
      const exportName = n.split('/').pop();
      return `export {default as ${exportName}} from '@cesium/engine/Source/${dir}/${n}.js';`;
    }),
  ),
  ...workerNames.map(
    (w) => `export {default as worker_${w}} from '@cesium/engine/Source/Workers/${w}.js';`,
  ),
].join('\n');

const resolveDir = fileURLToPath(new URL('.', import.meta.url));
const bundle = await build({
  stdin: { contents, resolveDir },
  bundle: true,
  write: false,
  format: 'esm',
  platform: 'browser',
  plugins: [
    {
      name: 'unwrap-task-processor-worker',
      setup(b) {
        b.onLoad({ filter: /createTaskProcessorWorker\.js$/ }, () => ({
          contents: 'export default function createTaskProcessorWorker(fn) { return fn; }',
          loader: 'js',
        }));
      },
    },
  ],
});

const approxHeightsJson = readFileSync(
  new URL('./node_modules/@cesium/engine/Source/Assets/approximateTerrainHeights.json', import.meta.url),
);
const wasmSplatsBytes = readFileSync(
  new URL('./node_modules/@cesium/engine/Source/ThirdParty/wasm_splats_bg.wasm', import.meta.url),
);

const server = createServer((req, res) => {
  const url = (req.url || '/').split('?')[0];
  if (url === '/reference.mjs') {
    res.setHeader('Content-Type', 'text/javascript');
    res.end(bundle.outputFiles[0].contents);
    return;
  }
  if (url.endsWith('approximateTerrainHeights.json')) {
    res.setHeader('Content-Type', 'application/json');
    res.end(approxHeightsJson);
    return;
  }
  if (url.endsWith('wasm_splats_bg.wasm')) {
    res.setHeader('Content-Type', 'application/wasm');
    res.end(wasmSplatsBytes);
    return;
  }
  res.setHeader('Content-Type', 'text/html');
  res.end('<!doctype html><title>Vector & Gaussian GPU Captures</title>');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

let browser;
try {
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_PATH,
    headless: true,
    args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
  });
  const page = await browser.newPage();
  const baseUrl = `http://127.0.0.1:${server.address().port}/`;
  await page.goto(baseUrl);

  const result = await page.evaluate(async (base) => {
    try {
      const ref = await import('/reference.mjs');
      ref.buildModuleUrl.setBaseUrl(base);

      const num = (v) =>
        Number.isNaN(v)
          ? 'NaN'
          : v === Infinity
            ? 'Infinity'
            : v === -Infinity
              ? '-Infinity'
              : Object.is(v, -0)
                ? '-0'
                : v;
      const arr = (v) => Array.from(v, num);
      const fnv = (bytes) => {
        let h = 0xcbf29ce484222325n;
        for (const b of bytes) h = BigInt.asUintN(64, (h ^ BigInt(b)) * 0x100000001b3n);
        return h.toString(16);
      };
      const tick = () => new Promise((r) => setTimeout(r, 0));

      // Wire bundled workers into TaskProcessor so real Cesium modules run their
      // real workers (including WASM splat generator/sorter) in-page.
      const workers = {
        createVectorTilePoints: ref.worker_createVectorTilePoints,
        createVectorTilePolylines: ref.worker_createVectorTilePolylines,
        createVectorTileClampedPolylines: ref.worker_createVectorTileClampedPolylines,
        createVectorTilePolygons: ref.worker_createVectorTilePolygons,
        createVectorTileGeometries: ref.worker_createVectorTileGeometries,
        gaussianSplatSorter: ref.worker_gaussianSplatSorter,
        gaussianSplatTextureGenerator: ref.worker_gaussianSplatTextureGenerator,
      };

      async function invokeWorker(workerFn, parameters) {
        const transferableObjects = [];
        return await workerFn(parameters, transferableObjects);
      }

      const workerLog = [];
      const wasmBinaryBytes = new Uint8Array(await (await fetch(`${base}ThirdParty/wasm_splats_bg.wasm`)).arrayBuffer());

      ref.TaskProcessor.prototype.initWebAssemblyModule = async function (webAssemblyOptions) {
        const name = String(this._workerPath).split('/').pop().replace('.js', '');
        const worker = workers[name];
        if (!worker) throw new Error(`unexpected wasm worker ${name}`);
        return invokeWorker(worker, { webAssemblyConfig: { wasmBinary: wasmBinaryBytes.buffer.slice(0) } });
      };

      ref.TaskProcessor.prototype.scheduleTask = function (parameters, transferableObjects) {
        const name = String(this._workerPath).split('/').pop().replace('.js', '');
        const worker = workers[name];
        if (!worker) throw new Error(`unexpected worker ${this._workerPath}`);
        const summary = { worker: name, transferCount: transferableObjects?.length ?? 0 };
        if (parameters.packedBuffer instanceof ArrayBuffer) {
          summary.packedBuffer = arr(new Float64Array(parameters.packedBuffer));
        }
        if (parameters.batchTableColors instanceof ArrayBuffer) {
          summary.batchTableColors = arr(new Uint32Array(parameters.batchTableColors));
        }
        if (parameters.batchIds instanceof ArrayBuffer) {
          summary.batchIds = arr(
            name === 'createVectorTilePolygons'
              ? new Uint32Array(parameters.batchIds)
              : new Uint16Array(parameters.batchIds),
          );
        }
        workerLog.push(summary);
        return invokeWorker(worker, parameters);
      };

      const makeCanvas = (w, h) => {
        const c = document.createElement('canvas');
        c.width = w;
        c.height = h;
        return c;
      };
      const glAttrs = {
        alpha: true,
        depth: true,
        stencil: true,
        antialias: false,
        preserveDrawingBuffer: true,
      };

      const makeFrameState = (context, opts = {}) => {
        const width = opts.width ?? 64;
        const height = opts.height ?? 64;
        const scene = {
          drawingBufferWidth: width,
          drawingBufferHeight: height,
          pixelRatio: 1.0,
          mapProjection: new ref.GeographicProjection(ref.Ellipsoid.WGS84),
          ellipsoid: ref.Ellipsoid.WGS84,
          mode: opts.mode ?? ref.SceneMode.SCENE3D,
          canvas: context.canvas,
          context,
        };
        const camera = new ref.Camera(scene);
        if (opts.cameraLookAt) {
          camera.lookAt(opts.cameraLookAt.target, opts.cameraLookAt.offset);
        } else {
          camera.setView({
            destination: new ref.Cartesian3(6378137.0 + 1000.0, 0.0, 0.0),
          });
        }
        const frustum = camera.frustum;
        const cullingVolume = frustum.computeCullingVolume(camera.positionWC, camera.directionWC, camera.upWC);
        const fs = {
          context,
          camera,
          cullingVolume,
          mapProjection: scene.mapProjection,
          mode: scene.mode,
          time: ref.JulianDate.fromIso8601('2025-01-01T00:00:00Z'),
          frameNumber: opts.frameNumber ?? 1,
          pixelRatio: 1.0,
          useLogDepth: false,
          invertClassification: opts.invertClassification ?? false,
          passes: {
            render: opts.render ?? true,
            pick: opts.pick ?? false,
          },
          commandList: [],
          afterRender: [],
          fog: { enabled: false },
          creditDisplay: { addCreditToNextFrame: () => {} },
          jobScheduler: {
            execute: (job) => {
              job.execute();
              return true;
            },
          },
        };
        context.uniformState.update(fs);
        return fs;
      };

      const rsSnap = (rs) =>
        rs
          ? {
              colorMask: rs.colorMask
                ? [rs.colorMask.red, rs.colorMask.green, rs.colorMask.blue, rs.colorMask.alpha]
                : null,
              depthMask: rs.depthMask,
              depthTest: { enabled: rs.depthTest.enabled, func: rs.depthTest.func },
              cull: { enabled: rs.cull.enabled, face: rs.cull.face },
              blending: {
                enabled: rs.blending.enabled,
                srcRgb: rs.blending.functionSourceRgb,
                dstRgb: rs.blending.functionDestinationRgb,
                srcAlpha: rs.blending.functionSourceAlpha,
                dstAlpha: rs.blending.functionDestinationAlpha,
              },
              polygonOffset: {
                enabled: rs.polygonOffset.enabled,
                factor: rs.polygonOffset.factor,
                units: rs.polygonOffset.units,
              },
              stencilMask: rs.stencilMask,
              stencilTest: {
                enabled: rs.stencilTest.enabled,
                frontFunction: rs.stencilTest.frontFunction,
                frontOp: [
                  rs.stencilTest.frontOperation.fail,
                  rs.stencilTest.frontOperation.zFail,
                  rs.stencilTest.frontOperation.zPass,
                ],
                backFunction: rs.stencilTest.backFunction,
                backOp: [
                  rs.stencilTest.backOperation.fail,
                  rs.stencilTest.backOperation.zFail,
                  rs.stencilTest.backOperation.zPass,
                ],
                reference: rs.stencilTest.reference,
                mask: rs.stencilTest.mask,
              },
            }
          : null;

      const vaSnap = (va) => {
        if (!va) return null;
        const attrs = [];
        for (let i = 0; i < va.numberOfAttributes; i++) {
          const a = va.getAttribute(i);
          attrs.push({
            index: a.index,
            components: a.componentsPerAttribute,
            datatype: a.componentDatatype,
            normalize: !!a.normalize,
            instanceDivisor: a.instanceDivisor ?? 0,
            bufferBytes: a.vertexBuffer ? a.vertexBuffer.sizeInBytes : 0,
            bufferUsage: a.vertexBuffer ? a.vertexBuffer.usage : null,
          });
        }
        return {
          numberOfAttributes: va.numberOfAttributes,
          attributes: attrs,
          hasIndexBuffer: !!va.indexBuffer,
          indexDatatype: va.indexBuffer ? va.indexBuffer.indexDatatype : null,
          indexBytes: va.indexBuffer ? va.indexBuffer.sizeInBytes : 0,
          indexUsage: va.indexBuffer ? va.indexBuffer.usage : null,
        };
      };

      const cmdSnap = (cmd) => ({
        pass: cmd.pass,
        primitiveType: cmd.primitiveType,
        offset: cmd.offset ?? 0,
        count: cmd.count ?? null,
        instanceCount: cmd.instanceCount ?? 0,
        cull: cmd.cull,
        pickOnly: !!cmd.pickOnly,
        pickId: cmd.pickId ?? null,
        renderState: rsSnap(cmd.renderState),
        vsDefines: cmd.shaderProgram ? [...cmd.shaderProgram.vertexShaderSource.defines] : [],
        fsDefines: cmd.shaderProgram ? [...cmd.shaderProgram.fragmentShaderSource.defines] : [],
        va: vaSnap(cmd.vertexArray),
      });

      const makeMockBatchTable = (colors) => ({
        featuresLength: colors.length,
        getColor: (i, result) => ref.Color.clone(colors[i] ?? ref.Color.WHITE, result),
        getVertexShaderCallback: () => (src) => src,
        getFragmentShaderCallback: () => (src) => src,
        getUniformMapCallback: () => (map) => map,
        getPickId: () => 'vec4(1.0, 0.0, 0.0, 1.0)',
      });

      function zigZag(values) {
        const out = new Uint16Array(values.length);
        let prev = 0;
        for (let i = 0; i < values.length; i++) {
          const d = values[i] - prev;
          prev = values[i];
          out[i] = ((d << 1) ^ (d >> 31)) & 0xffff;
        }
        return out;
      }
      function encodePositions(us, vs, hs) {
        const planes = [zigZag(us), zigZag(vs)];
        if (hs) planes.push(zigZag(hs));
        const out = new Uint16Array(planes.reduce((n, p) => n + p.length, 0));
        let o = 0;
        for (const p of planes) {
          out.set(p, o);
          o += p.length;
        }
        return out;
      }

      // =========================================================================
      // 1. createBillboardPointCallback (real Chromium Canvas2D + WebGL2 Atlas)
      // =========================================================================
      const createBillboardPointCallbackCapture = (() => {
        const canvas = makeCanvas(16, 16);
        const context = new ref.Context(canvas, { webgl: glAttrs });
        const gl = context._gl;
        const atlas = new ref.TextureAtlas({ context });

        const specs = [
          [1.0, '#ff0000', '#0000ff', 2, 8],
          [0.5, 'rgba(255, 0, 0, 0.5)', '#0000ff', 2, 8],
          [1.0, 'red', 'black', 0, 5],
          [0.75, '#123456', '#abcdef', 1.5, 6.9],
        ];
        const cases = specs.map((args, i) => {
          const cb = ref.createBillboardPointCallback(...args);
          const ptCanvas = cb();
          const ctx2d = ptCanvas.getContext('2d');
          const img = ctx2d.getImageData(0, 0, ptCanvas.width, ptCanvas.height).data;
          const px = (x, y) => {
            const idx = (y * ptCanvas.width + x) * 4;
            return [img[idx], img[idx + 1], img[idx + 2], img[idx + 3]];
          };
          const cx = Math.floor(ptCanvas.width / 2);
          const cy = Math.floor(ptCanvas.height / 2);
          atlas.addImage(`pt_${i}`, ptCanvas);
          return {
            args,
            width: ptCanvas.width,
            height: ptCanvas.height,
            centerPixel: px(cx, cy),
            cornerPixel: px(0, 0),
            leftMidPixel: px(0, cy),
          };
        });
        atlas.update(context);
        const atlasBytes = atlas.texture ? atlas.texture.sizeInBytes : 0;
        const glError = gl.getError();
        atlas.destroy();
        context.destroy();
        return { cases, atlasBytes, glError };
      })();

      // =========================================================================
      // 2. Vector3DTilePrimitive (SwiftShader WebGL2)
      // =========================================================================
      const vector3DTilePrimitiveCapture = (() => {
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: glAttrs });
        const gl = context._gl;

        const positions = new Float32Array([
          0, 0, 0, 1, 0, 0, 0, 1, 0,
          2, 0, 0, 3, 0, 0, 2, 1, 0,
          4, 0, 0, 5, 0, 0, 4, 1, 0,
        ]);
        const vertexBatchIds = new Uint16Array([0, 0, 0, 1, 1, 1, 2, 2, 2]);
        const indices = new Uint16Array([0, 1, 2, 3, 4, 5, 6, 7, 8]);
        const indexCounts = new Uint32Array([3, 3, 3]);
        const indexOffsets = new Uint32Array([0, 3, 6]);
        const batchedIndices = [
          new ref.Vector3DTileBatch({ offset: 0, count: 3, color: ref.Color.RED.clone(), batchIds: [0] }),
          new ref.Vector3DTileBatch({ offset: 3, count: 3, color: ref.Color.BLUE.clone(), batchIds: [1] }),
          new ref.Vector3DTileBatch({ offset: 6, count: 3, color: ref.Color.RED.clone(), batchIds: [2] }),
        ];
        const batchIds = new Uint16Array([0, 1, 2]);
        const bv = new ref.BoundingSphere(new ref.Cartesian3(2, 0.5, 0), 5);
        const bvs = [
          new ref.BoundingSphere(new ref.Cartesian3(0, 0, 0), 1),
          new ref.BoundingSphere(new ref.Cartesian3(2, 0, 0), 1),
          new ref.BoundingSphere(new ref.Cartesian3(4, 0, 0), 1),
        ];
        const center = new ref.Cartesian3(10, 20, 30);

        const prim = new ref.Vector3DTilePrimitive({
          positions,
          vertexBatchIds,
          indices,
          indexCounts,
          indexOffsets,
          batchedIndices,
          batchIds,
          boundingVolume: bv,
          boundingVolumes: bvs,
          center,
          batchTable: makeMockBatchTable([ref.Color.RED, ref.Color.BLUE, ref.Color.RED]),
          classificationType: ref.ClassificationType.BOTH,
        });

        // Frame 1: render BOTH (not yet rebatched because 120-frame grace)
        const fBoth = makeFrameState(context, { render: true, pick: false });
        prim.update(fBoth);
        const renderBothCommands = fBoth.commandList.map(cmdSnap);
        const mvp1 = arr(fBoth.commandList[0].uniformMap.u_modifiedModelViewProjection());
        const hl1 = fBoth.commandList[0].uniformMap.u_highlightColor();

        // Debug highlight color
        prim.applyDebugSettings(true, ref.Color.CYAN);
        const hlDebug = fBoth.commandList[0].uniformMap.u_highlightColor();
        prim.applyDebugSettings(false, ref.Color.CYAN);
        const hlReset = fBoth.commandList[0].uniformMap.u_highlightColor();

        // Frame 2: TERRAIN only
        prim.classificationType = ref.ClassificationType.TERRAIN;
        const fTerrain = makeFrameState(context, { render: true, pick: false });
        prim.update(fTerrain);
        const renderTerrainPasses = fTerrain.commandList.map((c) => c.pass);

        // Frame 3: CESIUM_3D_TILE only + invertClassification
        prim.classificationType = ref.ClassificationType.CESIUM_3D_TILE;
        const fInvert = makeFrameState(context, { render: true, pick: false, invertClassification: true });
        prim.update(fInvert);
        const renderInvertCommands = fInvert.commandList.map(cmdSnap);

        // Frame 4: debugWireframe
        prim.debugWireframe = true;
        const fWire = makeFrameState(context, { render: true, pick: false });
        prim.update(fWire);
        const wireframeCommands = fWire.commandList.map(cmdSnap);
        prim.debugWireframe = false;

        // Frame 5: pick pass (BOTH)
        prim.classificationType = ref.ClassificationType.BOTH;
        const fPick = makeFrameState(context, { render: false, pick: true });
        prim.update(fPick);
        const pickCommands = fPick.commandList.map(cmdSnap);

        // Frame 6: forceRebatch on WebGL2 -> merges the two RED batches
        prim.forceRebatch = true;
        const fRebatch = makeFrameState(context, { render: true, pick: false });
        prim.update(fRebatch);
        const afterRebatch = {
          batchedIndices: prim._batchedIndices.map((b) => ({
            offset: b.offset,
            count: b.count,
            rgba: b.color.toRgba(),
            batchIds: [...b.batchIds],
          })),
          indexOffsets: arr(prim._indexOffsets),
          commandCount: fRebatch.commandList.length,
          commandRanges: fRebatch.commandList.map((c) => [c.pass, c.offset, c.count]),
        };

        const glError = gl.getError();
        prim.destroy();
        const destroyed = prim.isDestroyed();
        context.destroy();
        return {
          trianglesLength: prim._trianglesLength ?? 3,
          geometryByteLength: 9 * 4 + 9 * 2 + 9 * 2,
          renderBothCommands,
          mvp1,
          hl1: [hl1.red, hl1.green, hl1.blue, hl1.alpha],
          hlDebug: [hlDebug.red, hlDebug.green, hlDebug.blue, hlDebug.alpha],
          hlReset: [hlReset.red, hlReset.green, hlReset.blue, hlReset.alpha],
          renderTerrainPasses,
          renderInvertCommands,
          wireframeCommands,
          pickCommands,
          afterRebatch,
          destroyed,
          glError,
        };
      })();

      // =========================================================================
      // 3. Vector3DTileGeometry (SwiftShader WebGL2 + real worker)
      // =========================================================================
      const vector3DTileGeometryCapture = await (async () => {
        workerLog.length = 0;
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: glAttrs });
        const gl = context._gl;

        const center = new ref.Cartesian3(6378137.0, 0.0, 0.0);
        const boxMat = ref.Matrix4.fromTranslation(new ref.Cartesian3(1, 0, 0));
        const boxes = new Float32Array(19);
        boxes[0] = 2.0; boxes[1] = 3.0; boxes[2] = 4.0;
        ref.Matrix4.pack(boxMat, boxes, 3);

        const cylMat = ref.Matrix4.fromTranslation(new ref.Cartesian3(0, 5, 0));
        const cylinders = new Float32Array(18);
        cylinders[0] = 2.0; cylinders[1] = 10.0;
        ref.Matrix4.pack(cylMat, cylinders, 2);

        const ellMat = ref.Matrix4.fromTranslation(new ref.Cartesian3(0, 0, 6));
        const ellipsoids = new Float32Array(19);
        ellipsoids[0] = 2.0; ellipsoids[1] = 3.0; ellipsoids[2] = 4.0;
        ref.Matrix4.pack(ellMat, ellipsoids, 3);

        const spheres = new Float32Array([2.5, 3.0, 4.0, 5.0]);

        const geom = new ref.Vector3DTileGeometry({
          boxes,
          boxBatchIds: new Uint16Array([0]),
          cylinders,
          cylinderBatchIds: new Uint16Array([1]),
          ellipsoids,
          ellipsoidBatchIds: new Uint16Array([2]),
          spheres,
          sphereBatchIds: new Uint16Array([3]),
          center,
          modelMatrix: ref.Matrix4.IDENTITY,
          batchTable: makeMockBatchTable([ref.Color.RED, ref.Color.GREEN, ref.Color.BLUE, ref.Color.RED]),
          boundingVolume: new ref.BoundingSphere(center, 50),
        });

        const preReady = {
          ready: geom.ready,
          trianglesLength: geom.trianglesLength,
          geometryByteLength: geom.geometryByteLength,
        };

        const f1 = makeFrameState(context, { render: true, pick: false });
        geom.update(f1);
        await geom._promise;
        await tick();

        const postDecode = {
          ready: geom.ready,
          trianglesLength: geom.trianglesLength,
          geometryByteLength: geom.geometryByteLength,
          packedLengths: {
            box: ref.Vector3DTileGeometry.packedBoxLength,
            cylinder: ref.Vector3DTileGeometry.packedCylinderLength,
            ellipsoid: ref.Vector3DTileGeometry.packedEllipsoidLength,
            sphere: ref.Vector3DTileGeometry.packedSphereLength,
          },
          workerCall: workerLog[0] ?? null,
          batchedIndices: geom._primitive._batchedIndices.map((b) => ({
            offset: b.offset,
            count: b.count,
            rgba: b.color.toRgba(),
            batchIds: [...b.batchIds],
          })),
          boundingVolumesCount: geom._primitive._boundingVolumes.length,
          boundingVolumes: geom._primitive._boundingVolumes.map((bs) => ({
            center: [bs.center.x, bs.center.y, bs.center.z],
            radius: bs.radius,
          })),
        };

        const f2 = makeFrameState(context, { render: true, pick: true });
        geom.update(f2);
        const commandsSummary = f2.commandList.map((c) => ({
          pass: c.pass,
          offset: c.offset,
          count: c.count,
          pickOnly: !!c.pickOnly,
          primitiveType: c.primitiveType,
        }));

        const glError = gl.getError();
        geom.destroy();
        const destroyed = geom.isDestroyed();
        context.destroy();
        return { preReady, postDecode, commandsSummary, destroyed, glError };
      })();

      // =========================================================================
      // 4. Vector3DTilePolygons (SwiftShader WebGL2 + real worker)
      // =========================================================================
      const vector3DTilePolygonsCapture = await (async () => {
        workerLog.length = 0;
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: glAttrs });
        const gl = context._gl;

        const rectangle = new ref.Rectangle(-0.1, -0.1, 0.1, 0.1);
        const positions = new Uint16Array([
          1000, 1000, 10000, 1000, 10000, 10000,
          20000, 20000, 30000, 20000, 30000, 30000,
        ]);
        const counts = new Uint32Array([3, 3]);
        const indices = new Uint32Array([0, 1, 2, 0, 1, 2]);
        const indexCounts = new Uint32Array([3, 3]);
        const batchIds = new Uint16Array([0, 1]);

        const polygons = new ref.Vector3DTilePolygons({
          positions,
          counts,
          indices,
          indexCounts,
          minimumHeight: -10.0,
          maximumHeight: 25.0,
          polygonMinimumHeights: new Float32Array([-5.0, 2.0]),
          polygonMaximumHeights: new Float32Array([15.0, 20.0]),
          rectangle,
          ellipsoid: ref.Ellipsoid.WGS84,
          center: new ref.Cartesian3(999, 999, 999), // discarded by Cesium constructor quirk
          batchTable: makeMockBatchTable([ref.Color.RED, ref.Color.LIME]),
          batchIds,
          boundingVolume: new ref.BoundingSphere(ref.Cartesian3.ZERO, 100000),
        });

        const f1 = makeFrameState(context, { render: true, pick: false });
        polygons.update(f1);
        await polygons._promise;
        await tick();

        const f2 = makeFrameState(context, { render: true, pick: false });
        polygons.update(f2);

        const out = {
          ready: polygons.ready,
          trianglesLength: polygons.trianglesLength,
          geometryByteLength: polygons.geometryByteLength,
          workerCall: workerLog[0] ?? null,
          primitiveCenter: [
            polygons._primitive._center.x,
            polygons._primitive._center.y,
            polygons._primitive._center.z,
          ],
          batchedIndices: polygons._primitive._batchedIndices.map((b) => ({
            offset: b.offset,
            count: b.count,
            rgba: b.color.toRgba(),
            batchIds: [...b.batchIds],
          })),
          obbCount: polygons._primitive._boundingVolumes.length,
          firstObbCenter: [
            polygons._primitive._boundingVolumes[0].center.x,
            polygons._primitive._boundingVolumes[0].center.y,
            polygons._primitive._boundingVolumes[0].center.z,
          ],
          commands: f2.commandList.map(cmdSnap),
          glError: gl.getError(),
        };
        polygons.destroy();
        out.destroyed = polygons.isDestroyed();
        context.destroy();
        return out;
      })();

      // =========================================================================
      // 5. Vector3DTilePolylines (SwiftShader WebGL2 + real worker)
      // =========================================================================
      const vector3DTilePolylinesCapture = await (async () => {
        workerLog.length = 0;
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: glAttrs });
        const gl = context._gl;

        const rectangle = new ref.Rectangle(-0.1, -0.1, 0.1, 0.1);
        const center = ref.Ellipsoid.WGS84.cartographicToCartesian(ref.Rectangle.center(rectangle));
        const positions = encodePositions(
          [1000, 4000, 8000, 12000, 16000],
          [2000, 5000, 9000, 13000, 17000],
          [3000, 6000, 10000, 14000, 18000],
        );
        const counts = new Uint32Array([3, 2]);
        const widths = new Uint16Array([4, 6]);
        const batchIds = new Uint16Array([0, 1]);

        const polylines = new ref.Vector3DTilePolylines({
          positions,
          counts,
          widths,
          minimumHeight: 10.0,
          maximumHeight: 100.0,
          rectangle,
          ellipsoid: ref.Ellipsoid.WGS84,
          center,
          batchTable: makeMockBatchTable([ref.Color.WHITE, ref.Color.YELLOW]),
          batchIds,
          boundingVolume: new ref.BoundingSphere(center, 100000),
          keepDecodedPositions: true,
        });

        const f1 = makeFrameState(context, { render: true, pick: false });
        polylines.update(f1);
        await polylines._promise;
        await tick();

        const f2 = makeFrameState(context, { render: true, pick: false });
        polylines.update(f2);
        const cmd = f2.commandList[0];
        const mmv = arr(cmd.uniformMap.u_modifiedModelView());
        const hlDefault = cmd.uniformMap.u_highlightColor();
        polylines.applyDebugSettings(true, ref.Color.MAGENTA);
        const hlDebug = cmd.uniformMap.u_highlightColor();

        const out = {
          ready: polylines.ready,
          trianglesLength: polylines.trianglesLength,
          geometryByteLength: polylines.geometryByteLength,
          workerCall: workerLog[0] ?? null,
          positionsBatch0: arr(polylines.getPositions(0)),
          positionsBatch1: arr(polylines.getPositions(1)),
          positionsMissing: polylines.getPositions(99) ?? null,
          commandCount: f2.commandList.length,
          command: cmdSnap(cmd),
          modifiedModelView: mmv,
          hlDefault: [hlDefault.red, hlDefault.green, hlDefault.blue, hlDefault.alpha],
          hlDebug: [hlDebug.red, hlDebug.green, hlDebug.blue, hlDebug.alpha],
          glError: gl.getError(),
        };
        polylines.destroy();
        out.destroyed = polylines.isDestroyed();
        context.destroy();
        return out;
      })();

      // =========================================================================
      // 6. Vector3DTileClampedPolylines (SwiftShader WebGL2 + real worker + ApproxHeights)
      // =========================================================================
      const vector3DTileClampedPolylinesCapture = await (async () => {
        workerLog.length = 0;
        const canvas = makeCanvas(32, 32);
        const context = new ref.Context(canvas, { webgl: glAttrs });
        const gl = context._gl;

        const rectangle = new ref.Rectangle(-0.1, -0.1, 0.1, 0.1);
        const center = ref.Ellipsoid.WGS84.cartographicToCartesian(ref.Rectangle.center(rectangle));
        const positions = encodePositions(
          [1000, 4000, 8000, 12000, 16000],
          [2000, 5000, 9000, 13000, 17000],
          [3000, 6000, 10000, 14000, 18000],
        );
        const counts = new Uint32Array([3, 2]);
        const widths = new Uint16Array([4, 6]);
        const batchIds = new Uint16Array([0, 1]);

        const clamped = new ref.Vector3DTileClampedPolylines({
          positions,
          counts,
          widths,
          minimumHeight: 10.0,
          maximumHeight: 100.0,
          rectangle,
          ellipsoid: ref.Ellipsoid.WGS84,
          center,
          batchTable: makeMockBatchTable([ref.Color.WHITE, ref.Color.YELLOW]),
          batchIds,
          classificationType: ref.ClassificationType.BOTH,
          keepDecodedPositions: true,
        });

        const initialHeights = [clamped._minimumMaximumVectorHeights.x, clamped._minimumMaximumVectorHeights.y];
        const f1 = makeFrameState(context, { render: true, pick: false });
        clamped.update(f1);
        await clamped._promise;
        // Also wait for worker promise scheduled inside createVertexArray
        await tick();
        await tick();

        const updatedHeights = [clamped._minimumMaximumVectorHeights.x, clamped._minimumMaximumVectorHeights.y];

        const fBoth = makeFrameState(context, { render: true, pick: false });
        clamped.update(fBoth);
        const bothCommands = fBoth.commandList.map(cmdSnap);
        const uHeights = fBoth.commandList[0].uniformMap.u_minimumMaximumVectorHeights();

        clamped._classificationType = ref.ClassificationType.TERRAIN;
        const fTerrain = makeFrameState(context, { render: true, pick: false });
        clamped.update(fTerrain);
        const terrainPasses = fTerrain.commandList.map((c) => c.pass);

        clamped._classificationType = ref.ClassificationType.CESIUM_3D_TILE;
        const f3DTiles = makeFrameState(context, { render: true, pick: false });
        clamped.update(f3DTiles);
        const tilesetPasses = f3DTiles.commandList.map((c) => c.pass);

        const out = {
          ready: clamped.ready,
          initialHeights,
          updatedHeights,
          uHeights: [uHeights.x, uHeights.y],
          obbCenter: [clamped._boundingVolume.center.x, clamped._boundingVolume.center.y, clamped._boundingVolume.center.z],
          trianglesLength: clamped.trianglesLength,
          geometryByteLength: clamped.geometryByteLength,
          workerCall: workerLog[0] ?? null,
          positionsBatch0: arr(clamped.getPositions(0)),
          bothCommands,
          terrainPasses,
          tilesetPasses,
          glError: gl.getError(),
        };
        clamped.destroy();
        out.destroyed = clamped.isDestroyed();
        context.destroy();
        return out;
      })();

      // =========================================================================
      // 7. Vector3DTilePoints (SwiftShader WebGL2 + Billboard/Label/PolylineCollections)
      // =========================================================================
      const vector3DTilePointsCapture = await (async () => {
        workerLog.length = 0;
        const canvas = makeCanvas(64, 64);
        const context = new ref.Context(canvas, { webgl: glAttrs });
        const gl = context._gl;

        const rectangle = new ref.Rectangle(-0.1, -0.1, 0.1, 0.1);
        const positions = encodePositions([10000, 20000], [15000, 25000], [5000, 12000]);
        const batchIds = new Uint16Array([0, 1]);
        const mockContent = {
          tileset: { ellipsoid: ref.Ellipsoid.WGS84 },
          batchTable: {
            hasProperty: () => false,
            getProperty: () => undefined,
            getPropertyIds: () => [],
          },
        };

        const points = new ref.Vector3DTilePoints({
          positions,
          batchIds,
          minimumHeight: 10.0,
          maximumHeight: 100.0,
          rectangle,
          heightReference: ref.HeightReference.NONE,
          batchTable: undefined,
          scene: undefined,
        });

        const beforeReady = {
          ready: points.ready,
          pointsLength: points.pointsLength,
          texturesByteLength: points.texturesByteLength,
        };

        const f1 = makeFrameState(context, { render: true, pick: false });
        points.update(f1);
        await points._promise;
        await tick();

        const afterDecodeBeforeFeatures = {
          ready: points.ready,
          pointsLength: points.pointsLength,
          texturesByteLength: points.texturesByteLength,
          workerCall: workerLog[0] ?? null,
          billboardPos0: [
            points._billboardCollection.get(0).position.x,
            points._billboardCollection.get(0).position.y,
            points._billboardCollection.get(0).position.z,
          ],
          billboardPos1: [
            points._billboardCollection.get(1).position.x,
            points._billboardCollection.get(1).position.y,
            points._billboardCollection.get(1).position.z,
          ],
        };

        const features = new Array(2);
        points.createFeatures(mockContent, features);
        const f0 = features[0];
        const f1Feat = features[1];
        const defaultFeatureState = {
          show: f0.show,
          billboardImageId: points._billboardCollection.get(0).image,
          labelText: f0.labelText,
          labelHorizontalOrigin: f0.labelHorizontalOrigin,
          labelVerticalOrigin: f0.labelVerticalOrigin,
          horizontalOrigin: f0.horizontalOrigin,
          verticalOrigin: f0.verticalOrigin,
          anchorLineEnabled: f0.anchorLineEnabled,
        };

        // Run update + afterRender to pack images into real TextureAtlases
        const runFrame = async (pick = false) => {
          const fs = makeFrameState(context, { render: !pick, pick });
          points.update(fs);
          for (const cb of fs.afterRender) {
            await cb();
          }
          await tick();
          return fs;
        };

        await runFrame(false);
        const fsAfterDefault = await runFrame(false);
        const afterDefaultAtlas = {
          billboardAtlasBytes: points._billboardCollection.sizeInBytes,
          labelAtlasBytes: points._labelCollection.sizeInBytes,
          texturesByteLength: points.texturesByteLength,
          commandCount: fsAfterDefault.commandList.length,
        };

        // Apply style with point styling, label text, heightOffset and anchor line
        const style = new ref.Cesium3DTileStyle({
          color: "color('red')",
          pointSize: 12,
          pointOutlineColor: "color('blue')",
          pointOutlineWidth: 2,
          labelText: "'P'",
          labelColor: "color('lime')",
          heightOffset: 25,
          anchorLineEnabled: true,
          anchorLineColor: "color('yellow')",
          backgroundEnabled: true,
        });
        points.applyStyle(style, features);
        points.applyDebugSettings(true, ref.Color.ORANGE);

        await runFrame(false);
        const fsStyled = await runFrame(false);

        // Allocate pick IDs via pick pass
        const fsPick = await runFrame(true);
        const bbPick0 = points._billboardCollection.get(0).pickId;
        const bbPick1 = points._billboardCollection.get(1).pickId;
        const plPick0 = points._polylineCollection.get(0).pickId;
        const lblPick0 = points._labelCollection.get(0).pickId;

        const afterStyled = {
          billboardImageId0: points._billboardCollection.get(0).image,
          billboardAtlasBytes: points._billboardCollection.sizeInBytes,
          labelAtlasBytes: points._labelCollection.sizeInBytes,
          texturesByteLength: points.texturesByteLength,
          billboardHighlight: [
            points._billboardCollection._highlightColor.red,
            points._billboardCollection._highlightColor.green,
            points._billboardCollection._highlightColor.blue,
            points._billboardCollection._highlightColor.alpha,
          ],
          labelHighlight: [
            points._labelCollection._highlightColor.red,
            points._labelCollection._highlightColor.green,
            points._labelCollection._highlightColor.blue,
            points._labelCollection._highlightColor.alpha,
          ],
          polylineHighlight: [
            points._polylineCollection._highlightColor.red,
            points._polylineCollection._highlightColor.green,
            points._polylineCollection._highlightColor.blue,
            points._polylineCollection._highlightColor.alpha,
          ],
          polylineUseHighlightColor: points._polylineCollection._useHighlightColor,
          styledCommandCount: fsStyled.commandList.length,
          pickCommandCount: fsPick.commandList.length,
          hasBillboardPickId0: !!bbPick0,
          hasBillboardPickId1: !!bbPick1,
          distinctBillboardPicks: !!(bbPick0 && bbPick1 && !ref.Color.equals(bbPick0.color, bbPick1.color)),
          hasPolylinePickId0: !!plPick0,
          hasLabelPickId0: !!lblPick0,
          f0PositionAfterOffset: [
            points._billboardCollection.get(0).position.x,
            points._billboardCollection.get(0).position.y,
            points._billboardCollection.get(0).position.z,
          ],
          f0PolylinePositions: points._polylineCollection.get(0).positions.map((p) => [p.x, p.y, p.z]),
          f0LabelText: f0.labelText,
          f0PointSize: f0.pointSize,
          f0AnchorLineEnabled: f0.anchorLineEnabled,
          f0AnchorLineColor: [
            f0.anchorLineColor.red,
            f0.anchorLineColor.green,
            f0.anchorLineColor.blue,
            f0.anchorLineColor.alpha,
          ],
        };

        // Clear style
        points.applyStyle(undefined, features);
        const afterClear = {
          show: f0.show,
          pointSize: f0.pointSize,
          labelText: f0.labelText,
          anchorLineEnabled: f0.anchorLineEnabled,
          heightOffset: f0.heightOffset,
          billboardImageId: points._billboardCollection.get(0).image,
        };

        const glError = gl.getError();
        points.destroy();
        const destroyed = points.isDestroyed();
        context.destroy();
        return {
          beforeReady,
          afterDecodeBeforeFeatures,
          defaultFeatureState,
          afterDefaultAtlas,
          afterStyled,
          afterClear,
          destroyed,
          glError,
        };
      })();

      // =========================================================================
      // 8. Vector3DTileContent (SwiftShader WebGL2 + binary & hierarchy batch table)
      // =========================================================================
      const vector3DTileContentCapture = await (async () => {
        const canvas = makeCanvas(64, 64);
        const context = new ref.Context(canvas, { webgl: glAttrs });
        const gl = context._gl;

        function buildVctr({ featureTable, batchTable, batchTableBinary = new Uint8Array(0), polygonIndices = [], polygonPositions = [], polylinePositions = [], pointsPositions = [] }) {
          const enc = new TextEncoder();
          let fJson = enc.encode(JSON.stringify(featureTable));
          while (fJson.length % 8 !== 0) fJson = new Uint8Array([...fJson, 0x20]);
          let bJson = batchTable ? enc.encode(JSON.stringify(batchTable)) : new Uint8Array(0);
          while (bJson.length % 8 !== 0) bJson = new Uint8Array([...bJson, 0x20]);
          const headerLen = 44;
          let off = headerLen + fJson.length + bJson.length + batchTableBinary.length;
          off += (4 - (off % 4)) % 4;
          const total = off + polygonIndices.length * 4 + polygonPositions.length * 2 + polylinePositions.length * 2 + pointsPositions.length * 2;
          const bytes = new Uint8Array(total);
          const view = new DataView(bytes.buffer);
          bytes.set(enc.encode('vctr'), 0);
          view.setUint32(4, 1, true);
          view.setUint32(8, total, true);
          view.setUint32(12, fJson.length, true);
          view.setUint32(16, 0, true);
          view.setUint32(20, bJson.length, true);
          view.setUint32(24, batchTableBinary.length, true);
          view.setUint32(28, polygonIndices.length * 4, true);
          view.setUint32(32, polygonPositions.length * 2, true);
          view.setUint32(36, polylinePositions.length * 2, true);
          view.setUint32(40, pointsPositions.length * 2, true);
          let c = headerLen;
          bytes.set(fJson, c); c += fJson.length;
          bytes.set(bJson, c); c += bJson.length;
          bytes.set(batchTableBinary, c); c += batchTableBinary.length;
          c += (4 - (c % 4)) % 4;
          new Uint32Array(bytes.buffer, c, polygonIndices.length).set(polygonIndices); c += polygonIndices.length * 4;
          new Uint16Array(bytes.buffer, c, polygonPositions.length).set(polygonPositions); c += polygonPositions.length * 2;
          new Uint16Array(bytes.buffer, c, polylinePositions.length).set(polylinePositions); c += polylinePositions.length * 2;
          new Uint16Array(bytes.buffer, c, pointsPositions.length).set(pointsPositions);
          return bytes;
        }

        // Build binary batch table: 6 features (2 polygons, 2 polylines, 2 points)
        // Binary property `height` (FLOAT SCALAR, 6 * 4 = 24 bytes) + HIERARCHY
        const heightBin = new Float32Array([10.5, 20.5, 30.5, 40.5, 50.5, 60.5]);
        const batchBinary = new Uint8Array(heightBin.buffer.slice(0));
        const batchTableJson = {
          name: ['poly0', 'poly1', 'line0', 'line1', 'pt0', 'pt1'],
          height: { byteOffset: 0, componentType: 'FLOAT', type: 'SCALAR' },
          HIERARCHY: {
            classes: [
              { name: 'Road', length: 3, instances: { surface: ['asphalt', 'gravel', 'dirt'] } },
              { name: 'POI', length: 3, instances: { category: ['park', 'shop', 'cafe'] } },
            ],
            instancesLength: 6,
            classIds: [0, 0, 0, 1, 1, 1],
          },
        };
        const featureTable = {
          POLYGONS_LENGTH: 2,
          POLYGON_COUNTS: [3, 3],
          POLYGON_INDEX_COUNTS: [3, 3],
          POLYLINES_LENGTH: 2,
          POLYLINE_COUNTS: [3, 2],
          POINTS_LENGTH: 2,
          REGION: [-0.1, -0.1, 0.1, 0.1, 10, 100],
        };
        const polygonIndices = [0, 1, 2, 0, 1, 2];
        const polygonPositions = [1000, 1000, 10000, 1000, 10000, 10000, 20000, 20000, 30000, 20000, 30000, 30000];
        const polylinePositions = encodePositions(
          [1000, 4000, 8000, 12000, 16000],
          [2000, 5000, 9000, 13000, 17000],
          [3000, 6000, 10000, 14000, 18000],
        );
        const pointsPositions = encodePositions([10000, 20000], [15000, 25000], [5000, 12000]);

        const rawBytes = buildVctr({
          featureTable,
          batchTable: batchTableJson,
          batchTableBinary: batchBinary,
          polygonIndices,
          polygonPositions,
          polylinePositions,
          pointsPositions,
        });

        const examined = [];
        const runContent = async (classificationType) => {
          examined.length = 0;
          const tileset = {
            ellipsoid: ref.Ellipsoid.WGS84,
            classificationType,
            debugWireframe: false,
            heightReference: ref.HeightReference.NONE,
            vectorKeepDecodedPositions: true,
            examineVectorLinesFunction: (pos, bid, url) => {
              examined.push({ batchId: bid, positionsLen: pos.length, url });
            },
            scene: undefined,
            _statistics: { batchTableByteLength: 0 },
          };
          const tile = {
            computedTransform: ref.Matrix4.IDENTITY,
            boundingVolume: { boundingVolume: new ref.BoundingSphere(ref.Cartesian3.ZERO, 100000) },
            tileset,
          };
          const resource = { getUrlComponent: () => 'https://example.test/rich.vctr' };
          const content = new ref.Vector3DTileContent(tileset, tile, resource, rawBytes.buffer.slice(0), 0);

          const beforeUpdate = {
            ready: content.ready,
            featuresLength: content.featuresLength,
            pointsLength: content.pointsLength,
            batchTableByteLength: content.batchTableByteLength,
            hasName0: content.hasProperty(0, 'name'),
            hasHeight2: content.hasProperty(2, 'height'),
            hasSurface0: content.hasProperty(0, 'surface'),
            hasCategory5: content.hasProperty(5, 'category'),
            hasCategory0: content.hasProperty(0, 'category'),
            heightVal2: content._batchTable.getProperty(2, 'height'),
            surfaceVal1: content._batchTable.getProperty(1, 'surface'),
            categoryVal5: content._batchTable.getProperty(5, 'category'),
            examined: examined.slice(),
          };

          // Drive frames until ready + afterRender atlas uploads
          let lastCmdCount = 0;
          for (let step = 0; step < 5; step++) {
            const fs = makeFrameState(context, { render: true, pick: false, frameNumber: step + 1 });
            content.update(tileset, fs);
            await tick();
            await tick();
            for (const cb of fs.afterRender) await cb();
            await tick();
            lastCmdCount = fs.commandList.length;
          }

          const afterReady = {
            ready: content.ready,
            pointsLength: content.pointsLength,
            trianglesLength: content.trianglesLength,
            geometryByteLength: content.geometryByteLength,
            texturesByteLength: content.texturesByteLength,
            batchTableByteLength: content.batchTableByteLength,
            polylinePositionsBatch2Len: content.getPolylinePositions(2)?.length ?? 0,
            lastCmdCount,
          };

          content.destroy();
          return { beforeUpdate, afterReady, destroyed: content.isDestroyed() };
        };

        const floating = await runContent(undefined);
        const clamped = await runContent(ref.ClassificationType.TERRAIN);
        const glError = gl.getError();
        context.destroy();
        return { floating, clamped, glError };
      })();

      // =========================================================================
      // 9 & 10. GaussianSplat3DTileContent + GaussianSplatPrimitive (SwiftShader WebGL2 + WASM)
      // =========================================================================
      const gaussianCapture = await (async () => {
        const canvas = makeCanvas(64, 64);
        const context = new ref.Context(canvas, { webgl: glAttrs });
        const gl = context._gl;

        // Build a minimal valid glTF JSON with embedded data URI containing
        // 4 Gaussian splats with POSITION, ROTATION, SCALE, COLOR_0, and degree-1 SH!
        function buildSplatGltf(withSh) {
          const count = 4;
          const pos = new Float32Array([
            0.0, 0.0, 0.0,
            1.0, 0.5, -2.0,
            -1.0, 2.0, 3.0,
            0.5, -1.5, 5.0,
          ]);
          const rot = new Float32Array([
            0, 0, 0, 1,
            0, 0, 0, 1,
            0, 0, 0, 1,
            0, 0, 0, 1,
          ]);
          const scl = new Float32Array([
            0.5, 0.5, 0.5,
            1.0, 1.0, 1.0,
            1.5, 0.75, 1.25,
            0.8, 0.8, 0.8,
          ]);
          const col = new Uint8Array([
            255, 0, 0, 255,
            0, 255, 0, 200,
            0, 0, 255, 180,
            255, 255, 0, 255,
          ]);
          const chunks = [
            new Uint8Array(pos.buffer),
            new Uint8Array(rot.buffer),
            new Uint8Array(scl.buffer),
            col,
          ];
          const shArrays = [];
          if (withSh) {
            for (let c = 0; c < 3; c++) {
              const sh = new Float32Array(count * 3);
              for (let i = 0; i < sh.length; i++) sh[i] = ((c * 11 + i * 3) % 19 - 9) * 0.1;
              shArrays.push(sh);
              chunks.push(new Uint8Array(sh.buffer));
            }
          }
          const totalLen = chunks.reduce((a, b) => a + b.byteLength, 0);
          const merged = new Uint8Array(totalLen);
          let off = 0;
          const offsets = [];
          for (const ch of chunks) {
            offsets.push([off, ch.byteLength]);
            merged.set(ch, off);
            off += ch.byteLength;
          }
          let binStr = '';
          for (let i = 0; i < merged.length; i++) binStr += String.fromCharCode(merged[i]);
          const uri = `data:application/octet-stream;base64,${btoa(binStr)}`;

          const bufferViews = offsets.map(([byteOffset, byteLength]) => ({ buffer: 0, byteOffset, byteLength }));
          const accessors = [
            { bufferView: 0, componentType: 5126, count, type: 'VEC3', min: [-1, -1.5, -2], max: [1, 2, 5] },
            { bufferView: 1, componentType: 5126, count, type: 'VEC4' },
            { bufferView: 2, componentType: 5126, count, type: 'VEC3' },
            { bufferView: 3, componentType: 5121, count, type: 'VEC4', normalized: true },
          ];
          const attributes = {
            POSITION: 0,
            _ROTATION: 1,
            _SCALE: 2,
            COLOR_0: 3,
          };
          if (withSh) {
            for (let c = 0; c < 3; c++) {
              const idx = 4 + c;
              accessors.push({ bufferView: idx, componentType: 5126, count, type: 'VEC3' });
              attributes[`_SH_DEGREE_1_COEF_${c}`] = idx;
            }
          }
          return {
            gltf: {
              asset: { version: '2.0' },
              extensionsUsed: ['KHR_gaussian_splatting'],
              scene: 0,
              scenes: [{ nodes: [0] }],
              nodes: [{ mesh: 0 }],
              meshes: [
                {
                  primitives: [
                    {
                      mode: 0,
                      attributes,
                      extensions: { KHR_gaussian_splatting: {} },
                    },
                  ],
                },
              ],
              buffers: [{ uri, byteLength: totalLen }],
              bufferViews,
              accessors,
            },
            pos: arr(pos),
            rot: arr(rot),
            scl: arr(scl),
            col: arr(col),
            sh: shArrays.map(arr),
          };
        }

        async function runSplatScenario(name, { withSh, modelMatrix, splitDirection }) {
          const spec = buildSplatGltf(withSh);
          const bs = new ref.BoundingSphere(new ref.Cartesian3(6378137.0, 0.0, 0.0), 10.0);
          const tileset = {
            show: true,
            modelMatrix: modelMatrix.clone(),
            _modelMatrixChanged: false,
            splitDirection,
            boundingSphere: bs,
            debugShowBoundingVolume: false,
            _selectedTiles: [],
            root: null,
            update: () => {},
            tileLoad: { addEventListener: () => {} },
            tileVisible: { addEventListener: () => {} },
            isGltfExtensionRequired: (ext) =>
              ext === 'KHR_gaussian_splatting' || ext === 'KHR_gaussian_splatting_compression_spz_2',
          };
          const tile = {
            computedTransform: ref.Transforms.eastNorthUpToFixedFrame(bs.center),
            tileset,
            content: null,
          };
          tileset.root = tile;

          const resource = new ref.Resource({ url: `https://example.test/${name}.gltf` });
          const content = await ref.GaussianSplat3DTileContent.fromGltf(tileset, tile, resource, spec.gltf);
          content._loader._loadAttributesAsTypedArray = true;
          tile.content = content;
          tileset._selectedTiles = [tile];
          const prim = tileset.gaussianSplatPrimitive;

          // Pump content.update until ready & transformed
          for (let i = 0; i < 6; i++) {
            const fs = makeFrameState(context, {
              render: true,
              pick: false,
              frameNumber: i + 1,
              cameraLookAt: { target: bs.center, offset: new ref.Cartesian3(0, 0, 20) },
            });
            content.update(tileset, fs);
            await tick();
          }

          const contentSnap = {
            ready: content.ready,
            transformed: content.transformed,
            pointsLength: content.pointsLength,
            featuresLength: content.featuresLength,
            trianglesLength: content.trianglesLength,
            geometryByteLength: content.geometryByteLength,
            batchTableByteLength: content.batchTableByteLength,
            shDegree: content.sphericalHarmonicsDegree,
            shCoefs: content.sphericalHarmonicsCoefficientCount,
            packedSh: content.packedSphericalHarmonicsData ? arr(content.packedSphericalHarmonicsData) : null,
            transformedPositions: arr(content.positions),
            transformedRotations: arr(content.rotations),
            transformedScales: arr(content.scales),
            requiresExtTrue: ref.GaussianSplat3DTileContent.tilesetRequiresGaussianSplattingExt(tileset),
          };

          // Pump primitive.update until draw command is built (WASM init + texture gen + sort)
          let frameNum = 10;
          for (let step = 0; step < 30; step++) {
            const fs = makeFrameState(context, {
              render: true,
              pick: false,
              frameNumber: frameNum++,
              cameraLookAt: { target: bs.center, offset: new ref.Cartesian3(0, 0, 20) },
            });
            prim.update(fs);
            await tick();
            if (prim._drawCommand) break;
          }

          const cmd = prim._drawCommand;
          const invRot = cmd ? arr(cmd.uniformMap.u_inverseModelRotation()) : null;
          const splatTex = prim.gaussianSplatTexture;
          const shTex = prim.sphericalHarmonicsTexture;

          // Verify steady-state re-sort after camera moves and >= 3 frames pass
          const indexesBeforeMove = prim._indexes ? arr(prim._indexes) : null;
          frameNum += 5;
          for (let step = 0; step < 15; step++) {
            const fs = makeFrameState(context, {
              render: true,
              pick: false,
              frameNumber: frameNum++,
              cameraLookAt: { target: bs.center, offset: new ref.Cartesian3(0, 0, -20) },
            });
            prim.update(fs);
            await tick();
            if (prim._sorterState === 0 && prim._indexes && indexesBeforeMove && prim._indexes[0] !== indexesBeforeMove[0]) {
              break;
            }
          }
          const indexesAfterMove = prim._indexes ? arr(prim._indexes) : null;

          const primSnap = {
            name,
            numSplats: prim._numSplats,
            selectedTileLength: prim.selectedTileLength,
            ready: prim.ready,
            isStable: prim.isStable,
            indexesBeforeMove,
            indexesAfterMove,
            contentTexturesByteLength: content.texturesByteLength,
            splatTexture: splatTex
              ? {
                  width: splatTex.width,
                  height: splatTex.height,
                  pixelFormat: splatTex.pixelFormat,
                  pixelDatatype: splatTex.pixelDatatype,
                  sizeInBytes: splatTex.sizeInBytes,
                }
              : null,
            shTexture: shTex
              ? {
                  width: shTex.width,
                  height: shTex.height,
                  pixelFormat: shTex.pixelFormat,
                  pixelDatatype: shTex.pixelDatatype,
                  sizeInBytes: shTex.sizeInBytes,
                }
              : null,
            command: cmd ? cmdSnap(cmd) : null,
            inverseModelRotation: invRot,
            uSplitDirection: cmd ? cmd.uniformMap.u_splitDirection() : null,
            uShDegree: cmd ? cmd.uniformMap.u_sphericalHarmonicsDegree() : null,
          };

          prim.destroy();
          content.destroy();
          return {
            input: { pos: spec.pos, rot: spec.rot, scl: spec.scl, col: spec.col, sh: spec.sh },
            contentSnap,
            primSnap,
          };
        }

        const degree0 = await runSplatScenario('degree0-identity', {
          withSh: false,
          modelMatrix: ref.Matrix4.IDENTITY,
          splitDirection: ref.SplitDirection.LEFT,
        });
        const degree1Mirrored = await runSplatScenario('degree1-mirrored', {
          withSh: true,
          modelMatrix: ref.Matrix4.fromScale(new ref.Cartesian3(-1, 1, 1)),
          splitDirection: ref.SplitDirection.NONE,
        });

        const maxTextureSize = ref.ContextLimits.maximumTextureSize;
        const glError = gl.getError();
        context.destroy();
        return { maxTextureSize, degree0, degree1Mirrored, glError };
      })();

      return {
        cesium: '23.0.0',
        createBillboardPointCallback: createBillboardPointCallbackCapture,
        vector3DTilePrimitive: vector3DTilePrimitiveCapture,
        vector3DTileGeometry: vector3DTileGeometryCapture,
        vector3DTilePolygons: vector3DTilePolygonsCapture,
        vector3DTilePolylines: vector3DTilePolylinesCapture,
        vector3DTileClampedPolylines: vector3DTileClampedPolylinesCapture,
        vector3DTilePoints: vector3DTilePointsCapture,
        vector3DTileContent: vector3DTileContentCapture,
        gaussian: gaussianCapture,
      };
    } catch (e) {
      return { __error: e?.message ?? String(e), __stack: e?.stack ?? '' };
    }
  }, baseUrl);

  if (result.__error) {
    throw new Error(`${result.__error}\n${result.__stack}`);
  }

  writeFileSync(
    new URL('../../tests/data/scene-vector-gaussian-gpu.json', import.meta.url),
    JSON.stringify(result, null, 2) + '\n',
  );
  console.log('Wrote tests/data/scene-vector-gaussian-gpu.json');
} finally {
  if (browser) await browser.close();
  server.close();
}
