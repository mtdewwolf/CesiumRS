// Capture Primitive's shader sources, WebGL2 uploads, and draw commands from
// the pinned CesiumJS engine. Replay with cargo test --test scene_primitive.
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const engine = JSON.parse(readFileSync(new URL('./node_modules/@cesium/engine/package.json', import.meta.url)));
if (engine.version !== '23.0.0') throw new Error(`Expected @cesium/engine 23.0.0, found ${engine.version}`);

const modules = {
  Core: [
    'BoundingSphere', 'Cartesian3', 'Color', 'ComponentDatatype', 'Geometry', 'GeometryAttribute',
    'GeometryAttributes', 'GeometryInstance', 'GeographicProjection', 'Matrix4', 'PerspectiveFrustum',
  ],
  Renderer: ['Context', 'RenderState'],
  Scene: [
    'DebugCameraPrimitive', 'DebugModelMatrixPrimitive', 'EllipsoidSurfaceAppearance', 'Material',
    'MaterialAppearance', 'PerInstanceColorAppearance', 'Primitive', 'createTangentSpaceDebugPrimitive',
  ],
};
const contents = Object.entries(modules)
  .flatMap(([directory, names]) => names.map((name) =>
    `export {default as ${name}} from '@cesium/engine/Source/${directory}/${name}.js';`))
  .join('\n');
const bundle = await build({
  stdin: { contents, resolveDir: fileURLToPath(new URL('.', import.meta.url)) },
  bundle: true, write: false, format: 'esm', platform: 'browser',
});
const server = createServer((request, response) => {
  if (request.url === '/reference.mjs') {
    response.setHeader('Content-Type', 'text/javascript');
    response.end(bundle.outputFiles[0].contents);
  } else {
    response.end('<!doctype html><title>Primitive capture</title>');
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));

const fields = (color, show, offset) => ({
  color, show: [show], distanceDisplayCondition: [10, 10000000], offset,
});
const scenarios = {
  projected: {
    instances: [
      { id: 'top', base: [6378147, 0, 0], boundingSphereCV: [10, 32, 32, 48], offsetAttribute: 1, attributes: fields([255, 0, 0, 255], 1, [0, 0, 0]) },
      { id: 'all', base: [6378247, 120, 50], offsetAttribute: 2, attributes: fields([0, 255, 0, 255], 0, [10, 20, 30]) },
    ],
    steps: [
      { mode: 3 }, { mode: 3 },
      { mode: 1, mutate: { color: [0, 255, 255, 128], show: [0], offset: [7, 16, 24], distanceDisplayCondition: [30, 500000] } },
      { mode: 2, pick: true }, { mode: 0 }, { mode: 3 },
    ],
  },
  closedDepthFail: {
    scene3DOnly: true, closed: true, translucent: true, depthFail: true, interleave: true,
    shadows: 1, cull: false, debugShowBoundingVolume: true, createPickOffsets: true,
    instances: [
      { id: 'red', base: [6378147, 0, 0], vectors: true, attributes: { color: [255, 0, 0, 128], depthFailColor: [255, 255, 0, 255] } },
      { id: 'blue', base: [6378247, 120, 50], vectors: true, attributes: { color: [0, 0, 255, 128], depthFailColor: [0, 255, 255, 255] } },
    ],
    steps: [{ mode: 3 }, { mode: 3, translation: [100, 200, 300], pick: true }],
  },
  colorMaterial: {
    scene3DOnly: true, material: 'Color', compressVertices: false, allowPicking: false,
    instances: [{ id: 'material', base: [6378147, 20, 30], vectors: true }],
    steps: [{ mode: 3 }, { mode: 3, pick: true, materialColor: [0, 0.25, 0.5, 1] }],
  },
  checkerboard: {
    material: 'Checkerboard', appearance: 'ellipsoid', interleave: true,
    instances: [{ id: 'checkerboard', base: [6378147, 20, 30], vectors: true }],
    steps: [{ mode: 3 }, { mode: 1 }],
  },
  stripe: {
    material: 'Stripe', scene3DOnly: true, compressVertices: true, interleave: false, flat: false,
    instances: [{ id: 'stripe', base: [6378147, 20, 30], vectors: true }],
    steps: [{ mode: 3 }],
  },
  antimeridian: {
    flat: true,
    instances: [{ id: 'split', base: [-6378147, -10, 40], attributes: { color: [200, 120, 40, 255] } }],
    steps: [{ mode: 3 }, { mode: 1 }, { mode: 2 }, { mode: 0 }],
  },
  rtc: {
    scene3DOnly: true, flat: true, rtcCenter: [6378147, 20, 30],
    instances: [{ id: 'rtc', base: [0, 0, 0], rtc: true, attributes: { color: [0, 255, 0, 255] } }],
    steps: [{ mode: 3 }, { mode: 3, translation: [100, 200, 300] }],
  },
  camera: { kind: 'camera', scene3DOnly: true, steps: [{ mode: 3 }, { mode: 3 }] },
  cameraStatic: { kind: 'camera', updateOnChange: false, scene3DOnly: true, steps: [{ mode: 3 }, { mode: 3 }] },
  modelMatrix: { kind: 'modelMatrix', scene3DOnly: true, steps: [{ mode: 3 }, { mode: 3 }, { mode: 3, length: 160 }] },
  tangentSpace: {
    kind: 'tangentSpace', scene3DOnly: true,
    instances: [{ base: [6378147, 20, 30], vectors: true }],
    steps: [{ mode: 3 }],
  },
};

let browser;
try {
  browser = await chromium.launch({
    executablePath: process.env.CHROMIUM_PATH, headless: true,
    args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
  });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const capture = await page.evaluate(async (scenarios) => {
    const ref = await import('/reference.mjs');
    const fnv = (bytes) => {
      let hash = 0xcbf29ce484222325n;
      for (const byte of bytes) hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
      return hash.toString(16);
    };
    const array = (v) => Array.from(v);
    const sphere = (s) => s ? [s.center.x, s.center.y, s.center.z, s.radius] : null;
    const cart = (a) => new ref.Cartesian3(...a);
    const D = ref.ComponentDatatype;
    const attribute = (componentDatatype, componentsPerAttribute, values) =>
      new ref.GeometryAttribute({ componentDatatype, componentsPerAttribute, values });
    const instanceAttribute = (name, value) => {
      const bytes = ['color', 'depthFailColor', 'show'].includes(name);
      return {
        componentDatatype: bytes ? D.UNSIGNED_BYTE : D.FLOAT,
        componentsPerAttribute: value.length, normalize: bytes && name !== 'show',
        value: bytes ? Uint8Array.from(value) : Float32Array.from(value),
      };
    };
    function geometry(spec) {
      const [x, y, z] = spec.base;
      const positions = [[x, y, z], [x + 32, y + 64, z], [x, y, z + 64]];
      const attributes = new ref.GeometryAttributes({
        position: attribute(spec.rtc ? D.FLOAT : D.DOUBLE, 3,
          spec.rtc ? Float32Array.from(positions.flat()) : Float64Array.from(positions.flat())),
      });
      if (spec.vectors) {
        attributes.normal = attribute(D.FLOAT, 3, Float32Array.from([1, 0, 0, 1, 0, 0, 1, 0, 0]));
        attributes.st = attribute(D.FLOAT, 2, Float32Array.from([0, 0, 1, 0, 0, 1]));
        attributes.tangent = attribute(D.FLOAT, 3, Float32Array.from([0, 1, 0, 0, 1, 0, 0, 1, 0]));
        attributes.bitangent = attribute(D.FLOAT, 3, Float32Array.from([0, 0, 1, 0, 0, 1, 0, 0, 1]));
      }
      if (spec.offsetAttribute !== undefined) {
        attributes.applyOffset = attribute(D.UNSIGNED_BYTE, 1, Uint8Array.from(spec.offsetAttribute === 1 ? [0, 1, 1] : [1, 1, 1]));
      }
      const cv = spec.boundingSphereCV;
      return new ref.Geometry({
        attributes, indices: Uint16Array.from([0, 1, 2]),
        boundingSphere: ref.BoundingSphere.fromPoints(positions.map(cart)),
        boundingSphereCV: cv && new ref.BoundingSphere(cart(cv.slice(0, 3)), cv[3]),
        offsetAttribute: spec.offsetAttribute,
      });
    }
    function primitive(spec, camera) {
      if (spec.kind === 'camera') {
        return new ref.DebugCameraPrimitive({
          camera, frustumSplits: [1, 10, 32], id: 'camera', updateOnChange: spec.updateOnChange,
        });
      }
      if (spec.kind === 'modelMatrix') return new ref.DebugModelMatrixPrimitive({ length: 64, width: 3, id: 'axes' });
      if (spec.kind === 'tangentSpace') {
        return ref.createTangentSpaceDebugPrimitive({ geometry: geometry(spec.instances[0]), length: 32 });
      }
      const options = { closed: spec.closed ?? false, translucent: spec.translucent ?? false, flat: spec.flat ?? !spec.closed };
      let appearance = new ref.PerInstanceColorAppearance(options);
      if (spec.material) {
        const material = ref.Material.fromType(spec.material);
        const Appearance = spec.appearance === 'ellipsoid' ? ref.EllipsoidSurfaceAppearance : ref.MaterialAppearance;
        appearance = new Appearance({ ...options, material });
      }
      return new ref.Primitive({
        geometryInstances: spec.instances.map((instance) => new ref.GeometryInstance({
          id: instance.id,
          geometry: instance.offsetAttribute === undefined ? geometry(instance) : {
            constructor: { createGeometry: () => geometry(instance) },
          },
          attributes: Object.fromEntries(Object.entries(instance.attributes ?? {}).map(([n, v]) => [n, instanceAttribute(n, v)])),
        })),
        appearance, depthFailAppearance: spec.depthFail ? new ref.PerInstanceColorAppearance(options) : undefined,
        asynchronous: false, rtcCenter: spec.rtcCenter && cart(spec.rtcCenter), allowPicking: spec.allowPicking,
        interleave: spec.interleave, compressVertices: spec.compressVertices, cull: spec.cull, shadows: spec.shadows,
        debugShowBoundingVolume: spec.debugShowBoundingVolume, _createPickOffsets: spec.createPickOffsets,
      });
    }

    const capture = {};
    let renderer;
    for (const [name, spec] of Object.entries(scenarios)) {
      const canvas = document.createElement('canvas');
      canvas.width = canvas.height = 8;
      const context = new ref.Context(canvas, { webgl: { antialias: false } });
      const gl = context._gl;
      if (!context.webgl2) throw new Error('Capture requires WebGL2');
      const debug = gl.getExtension('WEBGL_debug_renderer_info');
      renderer = gl.getParameter(debug.UNMASKED_RENDERER_WEBGL);
      if (!renderer.includes('SwiftShader')) throw new Error(`Capture requires SwiftShader, got ${renderer}`);

      let uploads = [];
      const wrap = (method, record) => {
        const original = gl[method].bind(gl);
        gl[method] = (...args) => { record(...args); return original(...args); };
      };
      const dataHash = (data) => fnv(new Uint8Array(data.buffer ?? data, data.byteOffset ?? 0, data.byteLength));
      wrap('bufferData', (target, data, usage) => {
        if (ArrayBuffer.isView(data) || data instanceof ArrayBuffer) {
          uploads.push(['bufferData', target, usage, data.byteLength, dataHash(data)]);
        }
      });
      for (const [method, first] of [['texImage2D', 3], ['texSubImage2D', 4]]) {
        wrap(method, (...args) => {
          const data = args[8];
          if (args.length === 9) {
            uploads.push([method, args[first], args[first + 1], args[7], data?.byteLength ?? 0, data ? dataHash(data) : null]);
          }
        });
      }
      wrap('vertexAttribPointer', (...args) => uploads.push(['pointer', ...args]));

      const camera = {
        positionWC: new ref.Cartesian3(7000000, 0, 0), directionWC: new ref.Cartesian3(-1, 0, 0),
        upWC: new ref.Cartesian3(0, 0, 1), rightWC: new ref.Cartesian3(0, 1, 0),
        frustum: new ref.PerspectiveFrustum({ near: 1, far: 32, aspectRatio: 1, fov: Math.PI / 2 }),
      };
      const p = primitive(spec, camera);
      const shaders = [];
      const programs = [];
      const registerShader = (shader) => {
        const index = programs.indexOf(shader);
        if (index !== -1) return index;
        programs.push(shader);
        shaders.push({
          vertex: shader._vertexShaderSource.sources[0], fragment: shader._fragmentShaderSource.sources[0],
          locations: shader._attributeLocations,
          active: Object.keys(shader.vertexAttributes).sort(),
        });
        return programs.length - 1;
      };
      const steps = [];
      for (const step of spec.steps) {
        if (step.translation) p.modelMatrix = ref.Matrix4.fromTranslation(cart(step.translation));
        if (step.length) p.length = step.length;
        if (step.materialColor) p.appearance.material.uniforms.color = new ref.Color(...step.materialColor);
        if (step.mutate) {
          const attributes = p.getGeometryInstanceAttributes(spec.instances[0].id);
          for (const [key, value] of Object.entries(step.mutate)) attributes[key] = instanceAttribute(key, value).value;
        }
        uploads = [];
        const frame = {
          context, camera, scene3DOnly: spec.scene3DOnly ?? false, mode: step.mode,
          mapProjection: new ref.GeographicProjection(), commandList: [], afterRender: [],
          passes: { render: !step.pick, pick: step.pick ?? false },
        };
        try { p.update(frame); } catch (error) { throw new Error(`${name} mode ${step.mode}: ${error}`); }
        const readyBefore = p.ready ?? null;
        const requestedRender = frame.afterRender.map((callback) => callback());
        const commands = frame.commandList.map((command) => {
          const uniforms = command.uniformMap;
          return {
            pass: command.pass, primitiveType: command.primitiveType, program: registerShader(command.shaderProgram),
            renderState: ref.RenderState.getState(command.renderState),
            vertexArray: command.owner._va.indexOf(command.vertexArray),
            count: command.count ?? command.vertexArray.indexBuffer?.numberOfIndices ?? command.vertexArray.numberOfVertices,
            boundingVolume: sphere(command.boundingVolume), modelMatrix: ref.Matrix4.toArray(command.modelMatrix),
            cull: command.cull, debugShowBoundingVolume: command.debugShowBoundingVolume,
            castShadows: command.castShadows, receiveShadows: command.receiveShadows,
            pickId: command.pickId ?? null, uniforms: Object.keys(uniforms).sort(),
            uniformValues: Object.fromEntries(Object.entries(uniforms).filter(([key]) => key !== 'batchTexture').map(([key, read]) => {
              const value = read();
              const names = 'red' in Object(value) ? ['red', 'green', 'blue', 'alpha'] :
                ['x', 'y', 'z', 'w'].filter((component) => component in Object(value));
              if (typeof value === 'boolean') return [key, [Number(value)]];
              if (typeof value === 'number') return [key, [value]];
              return [key, names.length ? names.map((n) => value[n]) : ref.Matrix4.toArray(value)];
            })),
          };
        });
        const readbacks = spec.kind ? [] : spec.instances.map(({ id, attributes }) => {
          const a = p.getGeometryInstanceAttributes(id);
          return {
            id, attributes: Object.fromEntries(Object.keys(attributes ?? {}).filter((n) => a[n]).map((n) => [n, array(a[n])])),
            boundingSphere: sphere(a.boundingSphere), boundingSphereCV: sphere(a.boundingSphereCV), pickKey: a.pickId.key,
            pickColor: [a.pickId.color.red, a.pickId.color.green, a.pickId.color.blue, a.pickId.color.alpha],
          };
        });
        steps.push({ uploads, commands, readyBefore, readyAfter: p.ready ?? null, requestedRender, readbacks,
          pickOffsets: p._pickOffsets ?? [] });
      }
      capture[name] = { shaders, steps };
      p.destroy();
      context.destroy();
    }
    return { renderer, cases: capture };
  }, scenarios);
  writeFileSync(new URL('../../tests/data/scene-primitive.json', import.meta.url),
    JSON.stringify({ engine: engine.version, renderer: capture.renderer, scenarios, capture: capture.cases }) + '\n');
  console.log(`Captured ${Object.keys(capture.cases).length} scenarios on ${capture.renderer}`);
} finally {
  await browser?.close();
  server.close();
}
