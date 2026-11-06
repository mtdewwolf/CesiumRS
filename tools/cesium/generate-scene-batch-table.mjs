// BatchTable reference capture from the pinned CesiumJS engine on a real
// WebGL2 context (SwiftShader in headless Chromium). Each scenario builds a
// table, applies set operations and updates, and records the layout
// (offsets, stride, dimensions, step, datatypes), every get, the CPU batch
// values, the generated vertex shader, the uniform values and every texture
// call. `packFloats` scenarios report no float textures, so floats are packed
// into RGBA bytes.
// Usage: CHROMIUM_PATH=/path/to/chrome node generate-scene-batch-table.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const modules = {
  Scene: ['BatchTable'],
  Renderer: ['Context', 'ContextLimits'],
  Core: ['Cartesian2', 'Cartesian3', 'Cartesian4', 'ComponentDatatype'],
};
const contents = Object.entries(modules)
  .flatMap(([dir, names]) => names.map((n) => `export {default as ${n}} from '@cesium/engine/Source/${dir}/${n}.js';`))
  .join('\n');
const bundle = await build({ stdin: { contents, resolveDir: fileURLToPath(new URL('.', import.meta.url)) }, bundle: true, write: false, format: 'esm', platform: 'browser' });
const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') { res.setHeader('Content-Type', 'text/javascript'); res.end(bundle.outputFiles[0].contents); return; }
  res.end('<!doctype html><title>BatchTable capture</title>');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

// Attributes: [functionName, componentDatatype, componentsPerAttribute, normalize].
const UB = 5121; const FLOAT = 5126; const US = 5123;
const VS = 'in vec3 position;\nin float batchId;\nvoid main()\n{\n    gl_Position = vec4(position, 1.0);\n}\n';
const scenarios = {
  bytes: {
    attributes: [['czm_batchTable_show', UB, 1, false], ['czm_batchTable_color', UB, 4, true], ['czm_batchTable_pickColor', UB, 4, true]],
    instances: 3,
    ops: [
      ['set', 0, 0, 1], ['set', 1, 0, 1], ['set', 0, 1, [255, 0, 128, 255]], ['set', 2, 2, [1, 2, 3, 4]],
      ['get', 0, 1], ['get', 2, 2], ['get', 1, 0], ['update'], ['update'],
      ['set', 1, 0, 1], ['update'],
      ['set', 1, 1, [300, -1, 7.9, 0.5]], ['get', 1, 1], ['set', 0, 0, 256], ['get', 0, 0], ['update'],
    ],
  },
  floats: {
    attributes: [['czm_batchTable_offset', FLOAT, 3, false], ['czm_batchTable_distanceDisplayCondition', FLOAT, 2, false], ['czm_batchTable_show', UB, 1, false], ['czm_batchTable_width', FLOAT, 1, false], ['czm_batchTable_color', UB, 4, true]],
    instances: 4,
    ops: [
      ['set', 0, 0, [1.5, -2.25, 1e7]], ['set', 3, 1, [10, 1e30]], ['set', 2, 3, 0.1], ['set', 1, 4, [255, 128, 0, 64]], ['set', 1, 2, 1],
      ['get', 0, 0], ['get', 3, 1], ['get', 2, 3], ['get', 1, 4], ['update'],
      ['set', 0, 0, [1.5, -2.25, 1e7]], ['update'],
      ['set', 3, 3, -7.125], ['update'],
    ],
  },
  packFloats: {
    packFloats: true,
    attributes: [['czm_batchTable_offset', FLOAT, 3, false], ['czm_batchTable_show', UB, 1, false], ['czm_batchTable_width', FLOAT, 1, false], ['czm_batchTable_color', UB, 4, true], ['czm_batchTable_halfShorts', US, 2, true]],
    instances: 3,
    ops: [
      ['set', 0, 0, [1.5, -2.25, 1e7]], ['set', 2, 2, 0.1], ['set', 1, 3, [255, 128, 0, 64]], ['set', 1, 1, 1], ['set', 0, 4, [3, 4]],
      ['get', 0, 0], ['get', 2, 2], ['get', 1, 3], ['get', 0, 4], ['get', 1, 1], ['update'],
      ['set', 2, 2, 12345.678], ['get', 2, 2], ['update'],
    ],
  },
  tall: {
    attributes: [['czm_batchTable_a', FLOAT, 4, false], ['czm_batchTable_b', UB, 1, false], ['czm_batchTable_c', UB, 2, false]],
    instances: 3000,
    ops: [['set', 2999, 0, [1, 2, 3, 4]], ['set', 1500, 2, [9, 8]], ['get', 2999, 0], ['get', 1500, 2], ['update']],
  },
  tallPacked: {
    packFloats: true,
    attributes: [['czm_batchTable_a', FLOAT, 4, false], ['czm_batchTable_b', UB, 1, false]],
    instances: 2000,
    ops: [['set', 1999, 0, [1, 2, 3, 4]], ['get', 1999, 0], ['update']],
  },
  empty: { attributes: [], instances: 5, ops: [['update']] },
  single: { attributes: [['czm_batchTable_x', UB, 1, false]], instances: 1, ops: [['set', 0, 0, 7], ['update']] },
};

let browser;
try {
  browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, headless: true, args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const result = await page.evaluate(async ({ scenarios, VS }) => {
    const ref = await import('/reference.mjs');
    const fnv = (bytes) => {
      let h = 0xcbf29ce484222325n;
      for (const b of bytes) h = BigInt.asUintN(64, (h ^ BigInt(b)) * 0x100000001b3n);
      return h.toString(16);
    };
    const num = (v) => (v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Number.isNaN(v) ? 'NaN' : v);
    const encode = (v) => (typeof v === 'number' ? num(v) : [v.x, v.y, v.z, v.w].filter((c) => c !== undefined).map(num));
    const value = (v) => (typeof v === 'number' ? v : v.length === 2 ? new ref.Cartesian2(...v) : v.length === 3 ? new ref.Cartesian3(...v) : new ref.Cartesian4(...v));
    const out = { maximumTextureSize: null, scenarios: {} };
    for (const [name, s] of Object.entries(scenarios)) {
      const canvas = document.createElement('canvas');
      canvas.width = 8; canvas.height = 8;
      const context = new ref.Context(canvas, { webgl: { antialias: false } });
      if (s.packFloats) Object.defineProperty(context, 'floatingPointTexture', { get: () => false });
      out.maximumTextureSize = ref.ContextLimits.maximumTextureSize;
      // Every texture call: [name, ...numeric args, data length and hash].
      let calls = [];
      const gl = context._gl;
      for (const fn of ['texImage2D', 'texSubImage2D', 'texParameteri', 'pixelStorei']) {
        const original = gl[fn].bind(gl);
        gl[fn] = (...args) => {
          calls.push([fn, ...args.map((a) => (ArrayBuffer.isView(a) ? [a.byteLength, fnv(new Uint8Array(a.buffer, a.byteOffset, a.byteLength))] : a === null ? null : a))]);
          return original(...args);
        };
      }
      const attributes = s.attributes.map(([functionName, componentDatatype, componentsPerAttribute, normalize]) => ({ functionName, componentDatatype, componentsPerAttribute, normalize }));
      const table = new ref.BatchTable(context, attributes, s.instances);
      const layout = {
        offsets: table._offsets ?? null, stride: table._stride ?? null, packFloats: table._packFloats ?? null,
        pixelDatatype: table._pixelDatatype ?? null,
        dimensions: table._textureDimensions ? [table._textureDimensions.x, table._textureDimensions.y] : null,
        step: table._textureStep ? [table._textureStep.x, table._textureStep.y, table._textureStep.z, table._textureStep.w] : null,
        batchValuesType: table._batchValues ? table._batchValues.constructor.name : null,
      };
      const steps = [];
      for (const op of s.ops) {
        calls = [];
        const step = { op };
        if (op[0] === 'set') table.setBatchedAttribute(op[1], op[2], value(op[3]));
        else if (op[0] === 'get') step.value = encode(table.getBatchedAttribute(op[1], op[2]));
        else table.update({ context });
        step.dirty = table._batchValuesDirty ?? null;
        const values = table._batchValues;
        step.batchValues = values ? fnv(new Uint8Array(values.buffer, values.byteOffset, values.byteLength)) : null;
        step.texture = table._texture ? [table._texture.width, table._texture.height, table._texture.pixelDatatype] : null;
        step.calls = calls;
        steps.push(step);
      }
      const uniforms = table.getUniformMapCallback()({ existing: () => 1 });
      out.scenarios[name] = {
        layout, steps,
        shader: table.getVertexShaderCallback()(VS),
        uniforms: Object.keys(uniforms),
        uniformValues: table._attributes.length === 0 ? null : {
          dimensions: [uniforms.batchTextureDimensions().x, uniforms.batchTextureDimensions().y],
          step: [uniforms.batchTextureStep().x, uniforms.batchTextureStep().y, uniforms.batchTextureStep().z, uniforms.batchTextureStep().w],
          texture: uniforms.batchTexture() === table._texture,
        },
      };
      table.destroy();
      context.destroy();
    }
    return out;
  }, { scenarios, VS });
  writeFileSync(new URL('../../tests/data/scene-batch-table.json', import.meta.url), JSON.stringify({ vertexShader: VS, scenarios, capture: result }, null, 1) + '\n');
  console.log('captured', Object.keys(result.scenarios).length, 'scenarios');
} finally {
  await browser?.close();
  server.close();
}
