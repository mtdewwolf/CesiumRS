// Reference capture for the single-renderable scene primitives
// (DepthPlane, EllipsoidPrimitive, Moon) from the pinned CesiumJS engine on a
// real WebGL2 context (SwiftShader in headless Chromium). After every update it
// records the buffer uploads (vertex and index data), the vertex attribute
// pointers, the draw commands, their render states, bounding volumes, model
// matrices, shader defines and uniform values.
// Usage: CHROMIUM_PATH=/path/to/chrome node generate-scene-single-renderables.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const modules = {
  Scene: ['DepthPlane', 'EllipsoidPrimitive', 'Moon', 'Material', 'Camera', 'SceneMode'],
  Renderer: ['Context', 'Pass'],
  Core: ['Cartesian2', 'Cartesian3', 'Cartesian4', 'Matrix4', 'Color', 'Ellipsoid', 'PerspectiveFrustum', 'OrthographicFrustum', 'GeographicProjection', 'JulianDate', 'Transforms', 'TimeStandard'],
};
const contents = Object.entries(modules)
  .flatMap(([dir, names]) => names.map((n) => `export {default as ${n}} from '@cesium/engine/Source/${dir}/${n}.js';`))
  .join('\n');
const bundle = await build({ stdin: { contents, resolveDir: fileURLToPath(new URL('.', import.meta.url)) }, bundle: true, write: false, format: 'esm', platform: 'browser' });
const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') { res.setHeader('Content-Type', 'text/javascript'); res.end(bundle.outputFiles[0].contents); return; }
  res.end('<!doctype html><title>single renderables capture</title>');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

const scenarios = {
  depthPlane: [
    ['update', { mode: 3 }],
    ['update', { mode: 3, useLogDepth: true }],
    ['update', { mode: 3 }],
    ['update', { mode: 3, ortho: true }],
    ['update', { mode: 3, inside: true }],
    ['update', { mode: 3, offset: 100000 }],
    ['update', { mode: 2 }],
  ],
  ellipsoid: [
    ['update', { mode: 3, render: true }],
    ['update', { mode: 3, render: true, radii: [10, 20, 40], center: [100, 200, 300], modelMatrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 1000, 1] }],
    ['update', { mode: 3, render: true, pick: true }],
    ['update', { mode: 3, render: true, opaque: true }],
    ['update', { mode: 3, render: true, onlySunLighting: true }],
    ['update', { mode: 3, render: true, translucent: true, depthTestEnabled: false }],
    ['update', { mode: 3, render: true, radii: [5, 10, 20] }],
    ['update', { mode: 3, render: true, show: false }],
    ['update', { mode: 2, render: true }],
  ],
  moon: [
    ['update', { mode: 3, render: true, time: [2451545.0, 0.0] }],
    ['update', { mode: 3, render: true, time: [2468000.0, 0.0] }],
    ['update', { mode: 3, render: true, time: [2451545.0, 43200.0], onlySunLighting: false }],
    ['update', { mode: 3, render: true, time: [2451545.0, 0.0], show: false }],
    ['update', { mode: 2, render: true, time: [2451545.0, 0.0], show: true }],
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
    const c3 = (v) => (v ? [v.x, v.y, v.z].map(num) : null);

    function makeContext() {
      const canvas = document.createElement('canvas');
      canvas.width = 8; canvas.height = 8;
      return new ref.Context(canvas, { webgl: { antialias: false } });
    }
    // Wrap GL entrypoints to record uploads.
    function wrap(context, holder) {
      const gl = context._gl;
      const view = (data) => new Uint8Array(data.buffer ?? data, data.byteOffset ?? 0, data.byteLength);
      for (const [fn, record] of [
        ['bufferData', (a) => { const d = a[1]; if (typeof d === 'object') holder.uploads.push(['bufferData', a[0], a[2], d.byteLength, fnv(view(d))]); }],
        ['bufferSubData', (a) => { const d = a[2]; holder.uploads.push(['bufferSubData', a[0], a[1], d.byteLength, fnv(view(d))]); }],
        ['vertexAttribPointer', (a) => holder.uploads.push(['pointer', a[0], a[1], a[2], a[3], a[4], a[5]])],
        ['vertexAttrib3fv', (a) => holder.uploads.push(['constant', a[0], ...a[1]])],
      ]) {
        const original = gl[fn].bind(gl);
        gl[fn] = (...args) => { record(args); return original(...args); };
      }
    }

    function camera(frustum) {
      const scene = {
        drawingBufferWidth: 800, drawingBufferHeight: 600, pixelRatio: 1, mapMode2D: 1, ellipsoid: ref.Ellipsoid.WGS84,
        mapProjection: new ref.GeographicProjection(), canvas: { clientWidth: 800, clientHeight: 600 }, frameState: { mode: 3 }, mode: 3,
        screenSpaceCameraController: { minimumZoomDistance: 1, maximumZoomDistance: Infinity }, verticalExaggeration: 1, verticalExaggerationRelativeHeight: 0,
      };
      const c = new ref.Camera(scene);
      c.frustum = frustum;
      return c;
    }
    function orbitCamera() {
      return camera(new ref.PerspectiveFrustum({ fov: Math.PI / 3, aspectRatio: 4 / 3, near: 1, far: 1e9 }));
    }
    function orthoCamera() {
      return camera(new ref.OrthographicFrustum({ width: 1e7, aspectRatio: 4 / 3, near: 1, far: 1e9 }));
    }

    const renderState = (rs) => ({
      cull: rs.cull.enabled, cullFace: rs.cull.face, depthTest: rs.depthTest.enabled, depthFunc: rs.depthTest.func,
      depthMask: rs.depthMask, colorMask: [rs.colorMask.red, rs.colorMask.green, rs.colorMask.blue, rs.colorMask.alpha],
      blending: rs.blending.enabled, blendFuncSrcRgb: rs.blending.functionSourceRgb, blendFuncDstRgb: rs.blending.functionDestinationRgb,
    });
    const defines = (sp) => sp ? { vs: sp._vertexShaderSource.defines, fs: sp._fragmentShaderSource.defines, pickColorQualifier: sp._fragmentShaderSource.pickColorQualifier ?? null } : null;
    const uniformValue = (v) => {
      if (v == null) return null;
      if (typeof v === 'number') return num(v);
      if (v instanceof ref.Cartesian2) return [v.x, v.y].map(num);
      if (v instanceof ref.Cartesian3) return [v.x, v.y, v.z].map(num);
      if (v instanceof ref.Cartesian4) return [v.x, v.y, v.z, v.w].map(num);
      if (v instanceof ref.Color) return [v.red, v.green, v.blue, v.alpha].map(num);
      if (Array.isArray(v)) return v.map(num);
      return String(v);
    };

    const out = {};

    // ---- DepthPlane ----
    {
      const name = 'depthPlane';
      const script = scenarios[name];
      const steps = [];
      const context = makeContext();
      const holder = { uploads: [] };
      wrap(context, holder);
      const plane = new ref.DepthPlane(0.0);
      const posAt = (c, inside) => {
        c.position = inside ? new ref.Cartesian3(1_000_000, 1_000_000, 1_000_000) : ref.Cartesian3.fromDegrees(0, 0, 10_000_000);
        c.direction = ref.Cartesian3.normalize(ref.Cartesian3.negate(c.position, new ref.Cartesian3()), new ref.Cartesian3());
        c.right = ref.Cartesian3.normalize(ref.Cartesian3.cross(c.direction, ref.Cartesian3.UNIT_Z, new ref.Cartesian3()), new ref.Cartesian3());
        c.up = ref.Cartesian3.cross(c.right, c.direction, new ref.Cartesian3());
      };
      // CesiumRS keeps the inside-ellipsoid fallback (the pinned engine emits an
      // implementation-defined NaN quad for a perspective camera inside the
      // ellipsoid). Recompute that finite quad to compare against.
      const fallbackHash = (ellipsoid, camera) => {
        const radii = ellipsoid.radii;
        const mul = (v) => new ref.Cartesian3(v.x * radii.x, v.y * radii.y, v.z * radii.z);
        const sub = (a, b) => ref.Cartesian3.subtract(a, b, new ref.Cartesian3());
        const add = (a, b) => ref.Cartesian3.add(a, b, new ref.Cartesian3());
        const north = camera.upWC, east = camera.rightWC;
        const upperLeft = mul(sub(add(ref.Cartesian3.ZERO, north), east));
        const lowerLeft = mul(sub(sub(ref.Cartesian3.ZERO, north), east));
        const upperRight = mul(add(add(ref.Cartesian3.ZERO, north), east));
        const lowerRight = mul(add(sub(ref.Cartesian3.ZERO, north), east));
        const f32 = new Float32Array([upperLeft.x, upperLeft.y, upperLeft.z, lowerLeft.x, lowerLeft.y, lowerLeft.z, upperRight.x, upperRight.y, upperRight.z, lowerRight.x, lowerRight.y, lowerRight.z]);
        return fnv(new Uint8Array(f32.buffer));
      };
      for (const op of script) {
        const o = op[1];
        holder.uploads = [];
        plane._ellipsoidOffset = o.offset ?? 0;
        const cam = o.ortho ? orthoCamera() : orbitCamera();
        posAt(cam, o.inside ?? false);
        const frameState = {
          mode: o.mode, useLogDepth: o.useLogDepth ?? false, context, camera: cam,
          mapProjection: new ref.GeographicProjection(), commandList: [], passes: { render: true, pick: false },
        };
        plane.update(frameState);
        if (o.inside) {
          // Replace the engine's NaN quad with the deterministic fallback quad.
          const h = fallbackHash(frameState.mapProjection.ellipsoid, cam);
          for (const u of holder.uploads) { if (u[0] === 'bufferSubData') u[4] = h; }
        }
        const command = plane._command;
        steps.push({
          op, uploads: holder.uploads, mode: plane._mode, useLogDepth: plane._useLogDepth,
          renderState: renderState(plane._rs),
          boundingVolume: sphere(command.boundingVolume),
          cull: command.cull, occlude: command.occlude, pass: command.pass,
          defines: defines(plane._sp),
        });
      }
      out[name] = steps;
      plane.destroy();
      context.destroy();
    }

    // ---- EllipsoidPrimitive ----
    {
      const name = 'ellipsoid';
      const script = scenarios[name];
      const steps = [];
      const context = makeContext();
      const holder = { uploads: [] };
      wrap(context, holder);
      const cam = orbitCamera();
      const programs = [];
      const programIndex = (sp) => { let i = programs.indexOf(sp); if (i < 0) { programs.push(sp); i = programs.length - 1; } return i; };
      const primitive = new ref.EllipsoidPrimitive();
      for (const op of script) {
        const o = op[1];
        holder.uploads = [];
        if (o.radii) { primitive.radii = new ref.Cartesian3(...o.radii); }
        if (o.center) { primitive.center = new ref.Cartesian3(...o.center); }
        if (o.modelMatrix) { primitive.modelMatrix = ref.Matrix4.fromArray(o.modelMatrix); }
        if (o.onlySunLighting !== undefined) { primitive.onlySunLighting = o.onlySunLighting; }
        if (o.depthTestEnabled !== undefined) { primitive._depthTestEnabled = o.depthTestEnabled; }
        if (o.show !== undefined) { primitive.show = o.show; }
        if (o.opaque !== undefined) { primitive.material = ref.Material.fromType('Color', { color: new ref.Color(0, 1, 0, 1) }); }
        if (o.translucent !== undefined) { primitive.material = ref.Material.fromType('Color', { color: new ref.Color(1, 0, 0, 0.5) }); }
        const frameState = {
          mode: o.mode, useLogDepth: false, context, camera: cam, commandList: [],
          mapProjection: new ref.GeographicProjection(),
          passes: { render: o.render ?? true, pick: o.pick ?? false },
        };
        primitive.update(frameState);
        const commands = frameState.commandList.map((command) => {
          const u = {};
          for (const k of Object.keys(command.uniformMap)) {
            const v = command.uniformMap[k]();
            if (v == null || typeof v === 'number' || v instanceof ref.Cartesian2 || v instanceof ref.Cartesian3 || v instanceof ref.Cartesian4 || v instanceof ref.Color) u[k] = uniformValue(v);
          }
          return {
            pass: command.pass, pickOnly: command.pickOnly, primitiveType: command.primitiveType,
            renderState: renderState(command.renderState),
            boundingVolume: sphere(command.boundingVolume), modelMatrix: ref.Matrix4.toArray(command.modelMatrix).map(num),
            debugShowBoundingVolume: command.debugShowBoundingVolume,
            executeInClosestFrustum: command.executeInClosestFrustum,
            program: programIndex(command.shaderProgram), defines: defines(command.shaderProgram),
            uniforms: u,
          };
        });
        steps.push({
          op, uploads: holder.uploads, commands,
          boundingSphere: sphere(primitive._boundingSphere),
          oneOverRadiiSquared: c3(primitive._oneOverEllipsoidRadiiSquared),
          computedModelMatrix: ref.Matrix4.toArray(primitive._computedModelMatrix).map(num),
          pickColor: primitive._pickId ? uniformValue(primitive._pickId.color) : null,
          pickId: primitive._pickId ? primitive._pickId.key : null,
        });
      }
      out[name] = steps;
      primitive.destroy();
      context.destroy();
    }

    // ---- Moon ----
    {
      const name = 'moon';
      const script = scenarios[name];
      const steps = [];
      const context = makeContext();
      const holder = { uploads: [] };
      wrap(context, holder);
      const cam = orbitCamera();
      const moon = new ref.Moon();
      const programs = [];
      const programIndex = (sp) => { let i = programs.indexOf(sp); if (i < 0) { programs.push(sp); i = programs.length - 1; } return i; };
      for (const op of script) {
        const o = op[1];
        holder.uploads = [];
        if (o.show !== undefined) { moon.show = o.show; }
        if (o.onlySunLighting !== undefined) { moon.onlySunLighting = o.onlySunLighting; }
        const date = new ref.JulianDate(o.time[0], o.time[1], ref.TimeStandard.TAI);
        const frameState = {
          mode: o.mode, time: date, useLogDepth: false, context, camera: cam, commandList: [],
          mapProjection: new ref.GeographicProjection(), passes: { render: o.render ?? true, pick: false },
        };
        const command = moon.update(frameState);
        const ep = moon._ellipsoidPrimitive;
        const u = {};
        if (command) { for (const k of Object.keys(command.uniformMap)) { const v = command.uniformMap[k](); if (v == null || typeof v === 'number' || v instanceof ref.Cartesian2 || v instanceof ref.Cartesian3 || v instanceof ref.Cartesian4 || v instanceof ref.Color) u[k] = uniformValue(v); } }
        steps.push({
          op, uploads: holder.uploads,
          modelMatrix: ref.Matrix4.toArray(ep.modelMatrix).map(num),
          computedModelMatrix: ref.Matrix4.toArray(ep._computedModelMatrix).map(num),
          boundingSphere: sphere(ep._boundingSphere),
          radii: c3(ep.radii), oneOverRadiiSquared: c3(ep._oneOverEllipsoidRadiiSquared),
          materialType: ep.material.type, translucent: ep.material.isTranslucent(), onlySunLighting: ep.onlySunLighting,
          depthTestEnabled: ep._depthTestEnabled,
          command: command ? {
            pass: command.pass, pickOnly: command.pickOnly,
            renderState: renderState(command.renderState),
            boundingVolume: sphere(command.boundingVolume), modelMatrix: ref.Matrix4.toArray(command.modelMatrix).map(num),
            program: programIndex(command.shaderProgram), defines: defines(command.shaderProgram), uniforms: u,
          } : null,
        });
      }
      out[name] = steps;
      moon.destroy();
      context.destroy();
    }

    return out;
  }, scenarios);
  writeFileSync(new URL('../../tests/data/scene-single-renderables.json', import.meta.url), JSON.stringify({ scenarios, capture: result }) + '\n');
  console.log('captured', Object.keys(result).length, 'scenarios');
} finally {
  await browser?.close();
  server.close();
}
