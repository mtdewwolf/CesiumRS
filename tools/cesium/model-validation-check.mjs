// Side-by-side SwiftShader WebGL2 pixel validation of Rust `Model` and
// `Model3DTileContent` against pinned `@cesium/engine` 23.0.0.
// Covers: glTF/GLB (unlit baseColor, color tint REPLACE/MIX, scale & modelMatrix,
// show toggle & transform update, CustomShader), B3DM (features, metrics, styling,
// debug color), I3DM (instanced), PNTS (point cloud), and GeoJSON tile content.
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { readFile, writeFile } from 'node:fs/promises';
import { resolve, extname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('../..', import.meta.url)));

const modules = {
  Scene: [
    'Model/Model',
    'Model/CustomShader',
    'Model/CustomShaderMode',
    'Model/CustomShaderTranslucencyMode',
    'Model/LightingModel',
    'Model/B3dmLoader',
    'Model/I3dmLoader',
    'Model/PntsLoader',
    'Model/GeoJsonLoader',
    'Atmosphere',
    'BrdfLutGenerator',
    'Camera',
    'Cesium3DTileStyle',
    'ColorBlendMode',
    'FrameState',
    'JobScheduler',
    'SceneMode',
    'ShadowMode',
    'ResourceCache',
  ],
  Renderer: ['ComputeEngine', 'Context', 'ClearCommand', 'Pass', 'PassState'],
  Core: [
    'BoundingRectangle',
    'Cartesian3',
    'Color',
    'Ellipsoid',
    'GeographicProjection',
    'JulianDate',
    'Matrix4',
    'PerspectiveFrustum',
    'Resource',
    'Transforms',
  ],
};

const contents = Object.entries(modules)
  .flatMap(([dir, names]) =>
    names.map((n) => {
      const exportName = n.split('/').pop();
      return `export {default as ${exportName}} from './node_modules/@cesium/engine/Source/${dir}/${n}.js';`;
    }),
  )
  .join('\n');

const bundle = await build({
  stdin: { contents, resolveDir: resolve(root, 'tools/cesium') },
  bundle: true,
  format: 'esm',
  platform: 'browser',
  write: false,
});

const server = createServer(async (req, res) => {
  try {
    if (req.url === '/ref.mjs') {
      res.setHeader('Content-Type', 'text/javascript');
      res.end(bundle.outputFiles[0].contents);
      return;
    }
    if (req.url === '/') {
      res.end('<!doctype html><title>Model WebGL2 validation</title>');
      return;
    }
    const path = resolve(root, '.' + req.url);
    if (!path.startsWith(root + '/')) throw Error('Invalid path');
    res.setHeader(
      'Content-Type',
      ({ '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm' })[extname(path)] ??
        'application/octet-stream',
    );
    res.end(await readFile(path));
  } catch (e) {
    res.writeHead(404);
    res.end(String(e));
  }
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
  await page.goto(`http://127.0.0.1:${server.address().port}/`);

  const report = await page.evaluate(async () => {
    try {
      const wasm = await import('/target/model-validation-gpu/model.js');
      await wasm.default();
      const ref = await import('/ref.mjs');

      const attrs = { antialias: false, alpha: true, depth: true, stencil: true, preserveDrawingBuffer: true };
      const SIZE = 64;
      const makeCanvas = () => {
        const c = document.createElement('canvas');
        c.width = SIZE;
        c.height = SIZE;
        return c;
      };

      const pad4 = (bytes) => {
        const rem = bytes.length % 4;
        if (rem === 0) return bytes;
        const out = new Uint8Array(bytes.length + (4 - rem));
        out.set(bytes);
        return out;
      };
      const pad8Json = (str) => {
        const raw = new TextEncoder().encode(str);
        const rem = raw.length % 8;
        if (rem === 0) return raw;
        const out = new Uint8Array(raw.length + (8 - rem));
        out.set(raw);
        out.fill(0x20, raw.length);
        return out;
      };
      const pad8Bin = (bytes) => {
        const rem = bytes.length % 8;
        if (rem === 0) return bytes;
        const out = new Uint8Array(bytes.length + (8 - rem));
        out.set(bytes);
        return out;
      };

      function buildGlb({ positions, indices, batchIds, baseColor = [0.8, 0.25, 0.15, 1.0] }) {
        const posBytes = new Uint8Array(new Float32Array(positions).buffer);
        const idxBytes = new Uint8Array(new Uint16Array(indices).buffer);
        const batchBytes = batchIds ? new Uint8Array(new Float32Array(batchIds).buffer) : null;

        const chunks = [pad4(posBytes), pad4(idxBytes)];
        if (batchBytes) chunks.push(pad4(batchBytes));
        const totalBin = chunks.reduce((s, c) => s + c.length, 0);
        const bin = new Uint8Array(totalBin);
        let offset = 0;
        const offsets = [];
        for (const c of chunks) {
          offsets.push(offset);
          bin.set(c, offset);
          offset += c.length;
        }

        let min = [Infinity, Infinity, Infinity];
        let max = [-Infinity, -Infinity, -Infinity];
        for (let i = 0; i < positions.length; i += 3) {
          for (let k = 0; k < 3; k++) {
            min[k] = Math.min(min[k], positions[i + k]);
            max[k] = Math.max(max[k], positions[i + k]);
          }
        }

        const bufferViews = [
          { buffer: 0, byteOffset: offsets[0], byteLength: posBytes.length, target: 34962 },
          { buffer: 0, byteOffset: offsets[1], byteLength: idxBytes.length, target: 34963 },
        ];
        const accessors = [
          { bufferView: 0, componentType: 5126, count: positions.length / 3, type: 'VEC3', min, max },
          { bufferView: 1, componentType: 5123, count: indices.length, type: 'SCALAR' },
        ];
        const primAttrs = { POSITION: 0 };
        if (batchBytes) {
          bufferViews.push({ buffer: 0, byteOffset: offsets[2], byteLength: batchBytes.length, target: 34962 });
          accessors.push({ bufferView: 2, componentType: 5126, count: batchIds.length, type: 'SCALAR' });
          primAttrs._BATCHID = 2;
        }

        const gltf = {
          asset: { version: '2.0' },
          extensionsUsed: ['KHR_materials_unlit'],
          scene: 0,
          scenes: [{ nodes: [0] }],
          nodes: [{ mesh: 0 }],
          meshes: [{ primitives: [{ attributes: primAttrs, indices: 1, material: 0 }] }],
          materials: [
            {
              pbrMetallicRoughness: { baseColorFactor: baseColor, metallicFactor: 0, roughnessFactor: 1 },
              doubleSided: true,
              extensions: { KHR_materials_unlit: {} },
            },
          ],
          buffers: [{ byteLength: bin.length }],
          bufferViews,
          accessors,
        };

        const jsonRaw = new TextEncoder().encode(JSON.stringify(gltf));
        const jsonPad = jsonRaw.length % 4 === 0 ? 0 : 4 - (jsonRaw.length % 4);
        const jsonChunkLen = jsonRaw.length + jsonPad;
        const binChunkLen = bin.length;
        const totalLen = 12 + 8 + jsonChunkLen + 8 + binChunkLen;
        const glb = new Uint8Array(totalLen);
        const dv = new DataView(glb.buffer);
        // Header: magic "glTF", version 2, length
        glb.set([0x67, 0x6c, 0x54, 0x46], 0);
        dv.setUint32(4, 2, true);
        dv.setUint32(8, totalLen, true);
        // JSON chunk
        dv.setUint32(12, jsonChunkLen, true);
        dv.setUint32(16, 0x4e4f534a, true);
        glb.set(jsonRaw, 20);
        glb.fill(0x20, 20 + jsonRaw.length, 20 + jsonChunkLen);
        // BIN chunk
        const binStart = 20 + jsonChunkLen;
        dv.setUint32(binStart, binChunkLen, true);
        dv.setUint32(binStart + 4, 0x004e4942, true);
        glb.set(bin, binStart + 8);
        return glb;
      }

      function buildB3dm(glb, featureTableJson, batchTableJson) {
        const ftJson = pad8Json(JSON.stringify(featureTableJson));
        const btJson = pad8Json(JSON.stringify(batchTableJson));
        const glbPad = pad8Bin(glb);
        const totalLen = 28 + ftJson.length + btJson.length + glbPad.length;
        const out = new Uint8Array(totalLen);
        const dv = new DataView(out.buffer);
        out.set([0x62, 0x33, 0x64, 0x6d], 0); // "b3dm"
        dv.setUint32(4, 1, true);
        dv.setUint32(8, totalLen, true);
        dv.setUint32(12, ftJson.length, true);
        dv.setUint32(16, 0, true);
        dv.setUint32(20, btJson.length, true);
        dv.setUint32(24, 0, true);
        let o = 28;
        out.set(ftJson, o);
        o += ftJson.length;
        out.set(btJson, o);
        o += btJson.length;
        out.set(glbPad, o);
        return out;
      }

      function buildI3dm(glb, positions) {
        const posBin = pad8Bin(new Uint8Array(new Float32Array(positions).buffer));
        const ftJson = pad8Json(JSON.stringify({ INSTANCES_LENGTH: positions.length / 3, POSITION: { byteOffset: 0 } }));
        const glbPad = pad8Bin(glb);
        const totalLen = 32 + ftJson.length + posBin.length + glbPad.length;
        const out = new Uint8Array(totalLen);
        const dv = new DataView(out.buffer);
        out.set([0x69, 0x33, 0x64, 0x6d], 0); // "i3dm"
        dv.setUint32(4, 1, true);
        dv.setUint32(8, totalLen, true);
        dv.setUint32(12, ftJson.length, true);
        dv.setUint32(16, posBin.length, true);
        dv.setUint32(20, 0, true);
        dv.setUint32(24, 0, true);
        dv.setUint32(28, 1, true); // gltfFormat = 1 (embedded binary)
        let o = 32;
        out.set(ftJson, o);
        o += ftJson.length;
        out.set(posBin, o);
        o += posBin.length;
        out.set(glbPad, o);
        return out;
      }

      function buildPnts(positions, rgb) {
        const posBytes = new Uint8Array(new Float32Array(positions).buffer);
        const rgbBytes = new Uint8Array(rgb);
        const ftBin = pad8Bin(new Uint8Array(posBytes.length + rgbBytes.length));
        ftBin.set(posBytes, 0);
        ftBin.set(rgbBytes, posBytes.length);
        const ftJson = pad8Json(
          JSON.stringify({
            POINTS_LENGTH: positions.length / 3,
            POSITION: { byteOffset: 0 },
            RGB: { byteOffset: posBytes.length },
          }),
        );
        const totalLen = 28 + ftJson.length + ftBin.length;
        const out = new Uint8Array(totalLen);
        const dv = new DataView(out.buffer);
        out.set([0x70, 0x6e, 0x74, 0x73], 0); // "pnts"
        dv.setUint32(4, 1, true);
        dv.setUint32(8, totalLen, true);
        dv.setUint32(12, ftJson.length, true);
        dv.setUint32(16, ftBin.length, true);
        dv.setUint32(20, 0, true);
        dv.setUint32(24, 0, true);
        out.set(ftJson, 28);
        out.set(ftBin, 28 + ftJson.length);
        return out;
      }

      // Helper to render a CesiumJS Model with our exact camera & return pixels
      function makeCesiumFrame(context, frameNumber) {
        const camera = new ref.Camera({
          drawingBufferWidth: SIZE,
          drawingBufferHeight: SIZE,
          mapProjection: new ref.GeographicProjection(),
        });
        camera.position = new ref.Cartesian3(0, 0, 5);
        camera.direction = new ref.Cartesian3(0, 0, -1);
        camera.up = new ref.Cartesian3(0, 1, 0);
        camera.right = new ref.Cartesian3(1, 0, 0);
        camera.frustum = new ref.PerspectiveFrustum({
          fov: Math.PI / 3,
          aspectRatio: 1.0,
          near: 0.1,
          far: 100.0,
        });
        if (!context._testJobScheduler) context._testJobScheduler = new ref.JobScheduler();
        context._testJobScheduler.resetBudgets();
        const fs = new ref.FrameState(context, { addStaticCredit() {}, addCreditToNextFrame() {} }, context._testJobScheduler);
        fs.mode = ref.SceneMode.SCENE3D;
        fs.mapProjection = new ref.GeographicProjection();
        fs.camera = camera;
        fs.cullingVolume = camera.frustum.computeCullingVolume(camera.position, camera.direction, camera.up);
        fs.frameNumber = frameNumber;
        fs.time = ref.JulianDate.fromIso8601('2000-01-01T12:00:00Z');
        fs.passes.render = true;
        fs.passes.pick = false;
        fs.commandList = [];
        fs.afterRender = [];
        fs.useLogDepth = false;
        fs.atmosphere = new ref.Atmosphere();
        fs.light = { color: ref.Color.WHITE, intensity: 1.0, direction: new ref.Cartesian3(0, 0, -1) };
        if (!context._testBrdf) context._testBrdf = new ref.BrdfLutGenerator();
        fs.brdfLutGenerator = context._testBrdf;
        return fs;
      }

      async function driveCesiumModel(context, model, steps) {
        let fn = 1;
        for (let i = 0; i < 16 && !model.ready; i++) {
          const fs = makeCesiumFrame(context, fn++);
          context.uniformState.update(fs);
          model.update(fs);
          for (const cb of fs.afterRender) cb();
          await new Promise((r) => setTimeout(r, 0));
        }
        if (!model.ready) throw new Error('CesiumJS Model did not become ready');

        const frames = [];
        const passState = new ref.PassState(context);
        passState.viewport = new ref.BoundingRectangle(0, 0, SIZE, SIZE);
        const computeEngine = new ref.ComputeEngine(context);
        for (const step of steps) {
          if (step.show !== undefined) model.show = step.show;
          if (step.modelMatrix !== undefined) model.modelMatrix = ref.Matrix4.unpack(step.modelMatrix);
          if (step.style !== undefined) {
            model.style = step.style ? new ref.Cesium3DTileStyle(step.style) : undefined;
          }
          if (step.debugColor) {
            const ft = model.featureTables[model.featureTableId];
            for (let fi = 0; fi < ft.featuresLength; fi++) {
              ft.getFeature(fi).color = step.debugColor;
            }
          }
          const fs = makeCesiumFrame(context, fn++);
          context.uniformState.update(fs);
          model.update(fs);
          new ref.ClearCommand({ color: ref.Color.TRANSPARENT, depth: 1, stencil: 0 }).execute(context, passState);
          for (const cmd of fs.commandList) {
            if (cmd.pass === ref.Pass.COMPUTE) cmd.execute(computeEngine);
            else cmd.execute(context, passState);
          }
          for (const cb of fs.afterRender) cb();
          const pixels = Array.from(context.readPixels({ width: SIZE, height: SIZE }));
          frames.push({ pixels, commands: fs.commandList.length });
        }
        return frames;
      }

      function comparePixels(name, actual, expected, maxTol = 1) {
        if (actual.length !== expected.length) {
          throw new Error(`${name}: pixel length ${actual.length} != ${expected.length}`);
        }
        let maxDiff = 0;
        let diffPixels = 0;
        let nonZeroActual = 0;
        let nonZeroExpected = 0;
        const firstDiffs = [];
        for (let i = 0; i < actual.length; i += 4) {
          if (actual[i] || actual[i + 1] || actual[i + 2] || actual[i + 3]) nonZeroActual++;
          if (expected[i] || expected[i + 1] || expected[i + 2] || expected[i + 3]) nonZeroExpected++;
          let pDiff = 0;
          for (let c = 0; c < 4; c++) {
            const d = Math.abs(actual[i + c] - expected[i + c]);
            if (d > pDiff) pDiff = d;
            if (d > maxDiff) maxDiff = d;
          }
          if (pDiff > maxTol) {
            diffPixels++;
            if (firstDiffs.length < 8) {
              firstDiffs.push({
                px: i / 4,
                actual: actual.slice(i, i + 4),
                expected: expected.slice(i, i + 4),
              });
            }
          }
        }
        if (diffPixels > 0) {
          throw new Error(
            `${name}: ${diffPixels} pixels exceeded tol=${maxTol} (maxDiff=${maxDiff}, nonZero actual=${nonZeroActual} expected=${nonZeroExpected}): ${JSON.stringify(firstDiffs)}`,
          );
        }
        return { name, pixels: SIZE * SIZE, nonZeroPixels: nonZeroActual, maxChannelDiff: maxDiff };
      }

      // Quad in XY plane at Z=0: [-1,-1,0] to [1,1,0]
      // Note: glTF is Y-up; CesiumJS applies Y-up -> Z-up axis transform on glTF by default!
      // Wait: to compare identically, use a symmetric cube or verify both sides apply identical Y-up->Z-up transform!
      // A flat square in the XZ plane [-1,0,-1]..[1,0,1] maps under Y-up->Z-up to XY plane [-1,-1,0]..[1,1,0]!
      const quadGlb = buildGlb({
        positions: [-1, 0, -1, 1, 0, -1, 1, 0, 1, -1, 0, 1],
        indices: [0, 1, 2, 0, 2, 3],
        baseColor: [0.8, 0.25, 0.15, 1.0],
      });

      const results = [];

      // --- Scenario 1: glTF unlit baseColor ---
      {
        const ctx = new ref.Context(makeCanvas(), { webgl: attrs });
        const model = await ref.Model.fromGltfAsync({ gltf: quadGlb, cull: false });
        const expected = await driveCesiumModel(ctx, model, [{}]);
        const gl = makeCanvas().getContext('webgl2', attrs);
        const actual = JSON.parse(await wasm.render_gltf_model(gl, quadGlb, JSON.stringify({ size: SIZE })));
        if (gl.getError() !== 0) throw new Error('gltf-unlit GL error');
        const r = comparePixels('gltf-unlit-base-color', actual.frames[0].pixels, expected[0].pixels, 1);
        if (r.nonZeroPixels === 0) throw new Error('gltf-unlit rendered empty!');
        results.push(r);
        model.destroy();
        ctx.destroy();
      }

      // --- Scenario 2: glTF color tint REPLACE + scale + modelMatrix ---
      {
        const m = ref.Matrix4.pack(ref.Matrix4.fromTranslation(new ref.Cartesian3(0.5, -0.25, 0)), []);
        const ctx = new ref.Context(makeCanvas(), { webgl: attrs });
        const model = await ref.Model.fromGltfAsync({
          gltf: quadGlb,
          cull: false,
          scale: 0.5,
          modelMatrix: ref.Matrix4.unpack(m),
          color: new ref.Color(0.2, 0.7, 0.9, 1.0),
          colorBlendMode: ref.ColorBlendMode.REPLACE,
        });
        const expected = await driveCesiumModel(ctx, model, [{}]);
        const gl = makeCanvas().getContext('webgl2', attrs);
        const actual = JSON.parse(
          await wasm.render_gltf_model(
            gl,
            quadGlb,
            JSON.stringify({
              size: SIZE,
              scale: 0.5,
              modelMatrix: m,
              color: [0.2, 0.7, 0.9, 1.0],
              colorBlendMode: 'replace',
            }),
          ),
        );
        if (gl.getError() !== 0) throw new Error('gltf-color-replace GL error');
        const r = comparePixels('gltf-color-replace-scale-transform', actual.frames[0].pixels, expected[0].pixels, 1);
        if (r.nonZeroPixels === 0) throw new Error('gltf-color-replace rendered empty!');
        results.push(r);
        model.destroy();
        ctx.destroy();
      }

      // --- Scenario 3: glTF color tint MIX ---
      {
        const ctx = new ref.Context(makeCanvas(), { webgl: attrs });
        const model = await ref.Model.fromGltfAsync({
          gltf: quadGlb,
          cull: false,
          color: new ref.Color(0.0, 1.0, 0.0, 1.0),
          colorBlendMode: ref.ColorBlendMode.MIX,
          colorBlendAmount: 0.5,
        });
        const expected = await driveCesiumModel(ctx, model, [{}]);
        const gl = makeCanvas().getContext('webgl2', attrs);
        const actual = JSON.parse(
          await wasm.render_gltf_model(
            gl,
            quadGlb,
            JSON.stringify({
              size: SIZE,
              color: [0.0, 1.0, 0.0, 1.0],
              colorBlendMode: 'mix',
              colorBlendAmount: 0.5,
            }),
          ),
        );
        if (gl.getError() !== 0) throw new Error('gltf-color-mix GL error');
        results.push(comparePixels('gltf-color-mix', actual.frames[0].pixels, expected[0].pixels, 2));
        model.destroy();
        ctx.destroy();
      }

      // --- Scenario 4: glTF show toggle + runtime modelMatrix update ---
      {
        const shifted = ref.Matrix4.pack(ref.Matrix4.fromTranslation(new ref.Cartesian3(-0.75, 0.5, 0)), []);
        const steps = [{ show: true }, { show: false }, { show: true, modelMatrix: shifted }];
        const ctx = new ref.Context(makeCanvas(), { webgl: attrs });
        const model = await ref.Model.fromGltfAsync({ gltf: quadGlb, cull: false });
        const expected = await driveCesiumModel(ctx, model, steps);
        const gl = makeCanvas().getContext('webgl2', attrs);
        const actual = JSON.parse(await wasm.render_gltf_model(gl, quadGlb, JSON.stringify({ size: SIZE, steps })));
        if (gl.getError() !== 0) throw new Error('gltf-show-transform GL error');
        for (let i = 0; i < steps.length; i++) {
          results.push(comparePixels(`gltf-lifecycle-step-${i}`, actual.frames[i].pixels, expected[i].pixels, 1));
        }
        model.destroy();
        ctx.destroy();
      }

      // --- Scenario 5: glTF CustomShader ---
      {
        const fsText = 'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) { material.diffuse = vec3(0.3, 0.6, 0.9); }';
        const ctx = new ref.Context(makeCanvas(), { webgl: attrs });
        const customShader = new ref.CustomShader({
          mode: ref.CustomShaderMode.MODIFY_MATERIAL,
          lightingModel: ref.LightingModel.UNLIT,
          translucencyMode: ref.CustomShaderTranslucencyMode.INHERIT,
          fragmentShaderText: fsText,
        });
        const model = await ref.Model.fromGltfAsync({ gltf: quadGlb, cull: false, customShader });
        const expected = await driveCesiumModel(ctx, model, [{}]);
        const gl = makeCanvas().getContext('webgl2', attrs);
        const actual = JSON.parse(
          await wasm.render_gltf_model(gl, quadGlb, JSON.stringify({ size: SIZE, customShaderFs: fsText })),
        );
        if (gl.getError() !== 0) throw new Error('gltf-custom-shader GL error');
        results.push(comparePixels('gltf-custom-shader', actual.frames[0].pixels, expected[0].pixels, 1));
        model.destroy();
        ctx.destroy();
      }

      // --- Scenario 6: B3DM Model3DTileContent (2 batched halves + style + debug color) ---
      {
        const b3dmGlb = buildGlb({
          positions: [
            // Feature 0: left half [-1.2..0, -1..1] in XY after Y-up->Z-up (so XZ plane)
            -1.2, 0, -1, 0, 0, -1, 0, 0, 1, -1.2, 0, 1,
            // Feature 1: right half [0..1.2, -1..1]
            0, 0, -1, 1.2, 0, -1, 1.2, 0, 1, 0, 0, 1,
          ],
          indices: [0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7],
          batchIds: [0, 0, 0, 0, 1, 1, 1, 1],
          baseColor: [0.6, 0.6, 0.6, 1.0],
        });
        const b3dm = buildB3dm(b3dmGlb, { BATCH_LENGTH: 2 }, { kind: [1, 2], name: ['left', 'right'] });
        const styleJson = { color: "${kind} === 1 ? color('red') : color('blue')" };

        const ctx = new ref.Context(makeCanvas(), { webgl: attrs });
        const b3dmRes = new ref.Resource({ url: 'https://validation.local/tile.b3dm' });
        const loader = new ref.B3dmLoader({
          b3dmResource: b3dmRes,
          baseResource: new ref.Resource({ url: 'https://validation.local/' }),
          arrayBuffer: b3dm.buffer,
          byteOffset: b3dm.byteOffset,
        });
        await loader.load();
        const model = new ref.Model({ loader, resource: b3dmRes, cull: false });
        const expected = await driveCesiumModel(ctx, model, [
          {},
          { style: styleJson },
          { style: null },
          { debugColor: ref.Color.LIME },
        ]);

        const gl = makeCanvas().getContext('webgl2', attrs);
        const actual = JSON.parse(
          await wasm.render_tile_content(gl, 'b3dm', b3dm, JSON.stringify({ size: SIZE, style: styleJson })),
        );
        if (gl.getError() !== 0) throw new Error('b3dm GL error');
        if (actual.featuresLength !== 2 || !actual.hasKind || actual.hasMissing || actual.metrics.triangles !== 4) {
          throw new Error('b3dm metadata/metrics mismatch: ' + JSON.stringify(actual.metrics));
        }
        results.push(comparePixels('b3dm-tile-content-base', actual.basePixels, expected[0].pixels, 1));
        results.push(comparePixels('b3dm-tile-content-styled', actual.styledPixels, expected[1].pixels, 1));
        results.push(comparePixels('b3dm-tile-content-debug-color', actual.debugPixels, expected[3].pixels, 1));
        model.destroy();
        ctx.destroy();
      }

      // --- Scenario 7: I3DM Model3DTileContent (2 instances via ModelTileFactory) ---
      {
        const smallQuad = buildGlb({
          positions: [-0.4, 0, -0.4, 0.4, 0, -0.4, 0.4, 0, 0.4, -0.4, 0, 0.4],
          indices: [0, 1, 2, 0, 2, 3],
          baseColor: [0.2, 0.8, 0.4, 1.0],
        });
        const i3dm = buildI3dm(smallQuad, [-1.0, 0, 0, 1.0, 0, 0]);
        const ctx = new ref.Context(makeCanvas(), { webgl: attrs });
        const i3dmRes = new ref.Resource({ url: 'https://validation.local/tile.i3dm' });
        const loader = new ref.I3dmLoader({
          i3dmResource: i3dmRes,
          baseResource: new ref.Resource({ url: 'https://validation.local/' }),
          arrayBuffer: i3dm.buffer,
          byteOffset: i3dm.byteOffset,
        });
        await loader.load();
        const model = new ref.Model({ loader, resource: i3dmRes, cull: false });
        const expected = await driveCesiumModel(ctx, model, [{}]);
        const gl = makeCanvas().getContext('webgl2', attrs);
        const actual = JSON.parse(await wasm.render_tile_content(gl, 'i3dm', i3dm, JSON.stringify({ size: SIZE })));
        if (gl.getError() !== 0) throw new Error('i3dm GL error');
        const r = comparePixels('i3dm-tile-content', actual.pixels, expected[0].pixels, 1);
        if (r.nonZeroPixels === 0) throw new Error('i3dm rendered 0 pixels');
        results.push(r);
        model.destroy();
        ctx.destroy();
      }

      // --- Scenario 8: PNTS Model3DTileContent (4 colored points via ModelTileFactory) ---
      {
        const pnts = buildPnts(
          [-0.6, -0.6, 0, 0.6, -0.6, 0, -0.6, 0.6, 0, 0.6, 0.6, 0],
          [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 0],
        );
        const ctx = new ref.Context(makeCanvas(), { webgl: attrs });
        const pntsRes = new ref.Resource({ url: 'https://validation.local/tile.pnts' });
        const loader = new ref.PntsLoader({
          arrayBuffer: pnts.buffer,
          byteOffset: pnts.byteOffset,
        });
        await loader.load();
        const model = new ref.Model({ loader, resource: pntsRes, cull: false });
        const expected = await driveCesiumModel(ctx, model, [{}]);
        const gl = makeCanvas().getContext('webgl2', attrs);
        const actual = JSON.parse(await wasm.render_tile_content(gl, 'pnts', pnts, JSON.stringify({ size: SIZE })));
        if (gl.getError() !== 0) throw new Error('pnts GL error');
        if (actual.metrics.points !== 4) throw new Error('pnts points count != 4: ' + JSON.stringify(actual.metrics));
        const r = comparePixels('pnts-tile-content', actual.pixels, expected[0].pixels, 1);
        if (r.nonZeroPixels === 0) throw new Error('pnts rendered 0 pixels');
        results.push(r);
        model.destroy();
        ctx.destroy();
      }

      // --- Scenario 9: GeoJSON Model3DTileContent via ModelTileFactory ---
      {
        const geojsonObj = {
          type: 'FeatureCollection',
          features: [
            {
              type: 'Feature',
              properties: { height: 10 },
              geometry: {
                type: 'Polygon',
                coordinates: [
                  [
                    [-0.00001, -0.00001, 0],
                    [0.00001, -0.00001, 0],
                    [0.00001, 0.00001, 0],
                    [-0.00001, 0.00001, 0],
                    [-0.00001, -0.00001, 0],
                  ],
                ],
              },
            },
          ],
        };
        const geojson = new TextEncoder().encode(JSON.stringify(geojsonObj));
        const ecefCenter = ref.Cartesian3.fromDegrees(0, 0, 0, ref.Ellipsoid.WGS84);
        const toGlobal = ref.Transforms.eastNorthUpToFixedFrame(ecefCenter, ref.Ellipsoid.WGS84);
        const toLocal = ref.Matrix4.inverseTransformation(toGlobal, new ref.Matrix4());
        const modelMatrixArr = ref.Matrix4.toArray(toLocal);

        const ctx = new ref.Context(makeCanvas(), { webgl: attrs });
        const geoRes = new ref.Resource({ url: 'https://validation.local/tile.geojson' });
        const loader = new ref.GeoJsonLoader({ geoJson: geojsonObj });
        await loader.load();
        const model = new ref.Model({ loader, resource: geoRes, modelMatrix: toLocal, cull: false });
        const expected = await driveCesiumModel(ctx, model, [{}]);

        const gl = makeCanvas().getContext('webgl2', attrs);
        const actual = JSON.parse(
          await wasm.render_tile_content(
            gl,
            'geojson',
            geojson,
            JSON.stringify({ size: SIZE, modelMatrix: modelMatrixArr }),
          ),
        );
        if (gl.getError() !== 0) throw new Error('geojson GL error');
        if (!actual.ready || !actual.destroyed) throw new Error('geojson lifecycle failed');
        const r = comparePixels('geojson-tile-content-factory', actual.pixels, expected[0].pixels, 1);
        if (r.nonZeroPixels === 0) throw new Error('geojson rendered 0 pixels');
        results.push({ ...r, commands: actual.commands, metrics: actual.metrics });
        model.destroy();
        ctx.destroy();
      }

      return results;
    } catch (e) {
      return { __error: e?.message ?? String(e), __stack: e?.stack ?? '' };
    }
  });

  if (report.__error) {
    throw new Error(`${report.__error}\n${report.__stack}`);
  }
  console.log(JSON.stringify(report, null, 2));
  await writeFile(resolve(root, 'docs/model-effects-evidence/webgl2-model-validation.json'), JSON.stringify(report, null, 2) + '\n');
} finally {
  if (browser) await browser.close();
  server.close();
}
