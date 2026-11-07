// ParticleSystem reference capture from the pinned CesiumJS engine on a real
// WebGL2 context (SwiftShader in headless Chromium), with a seeded random
// number generator. After every frame (update, then afterRender and a
// task-queue turn) it records the system's timing and pool, every particle
// with the index of its billboard, every billboard's properties and the
// billboard collection's vertex buffers.
// Usage: CHROMIUM_PATH=/path/to/chrome node generate-scene-particle-system.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const modules = {
  Scene: ['ParticleSystem', 'ParticleBurst', 'CircleEmitter', 'SphereEmitter', 'ConeEmitter', 'BoxEmitter', 'Camera'],
  Renderer: ['Context'],
  Core: ['Cartesian2', 'Cartesian3', 'Color', 'Matrix4', 'JulianDate', 'Math', 'GeographicProjection', 'Ellipsoid', 'PerspectiveFrustum'],
};
const contents = Object.entries(modules)
  .flatMap(([dir, names]) => names.map((n) => `export {default as ${n}} from '@cesium/engine/Source/${dir}/${n}.js';`))
  .join('\n');
const bundle = await build({ stdin: { contents, resolveDir: fileURLToPath(new URL('.', import.meta.url)) }, bundle: true, write: false, format: 'esm', platform: 'browser' });
const server = createServer((req, res) => {
  if (req.url === '/reference.mjs') { res.setHeader('Content-Type', 'text/javascript'); res.end(bundle.outputFiles[0].contents); return; }
  res.end('<!doctype html><title>ParticleSystem capture</title>');
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));

// Each scenario: system options, then frames [seconds, frameNumber, changes].
// Options use the generator's encoding: emitter [kind, ...args]; matrices as
// column-major arrays; colors as [r, g, b, a]; sizes as [x, y].
const scenarios = {
  looping: {
    seed: 7,
    options: {
      emitter: ['circle', 2.0], emissionRate: 4, minimumParticleLife: 1, maximumParticleLife: 2,
      minimumSpeed: 1, maximumSpeed: 3, minimumMass: 0.5, maximumMass: 2,
      minimumImageSize: [2, 3], maximumImageSize: [6, 5], startColor: [1, 0, 0, 1], endColor: [0, 0, 1, 0.25],
      startScale: 1, endScale: 3, lifetime: 3, bursts: [[0.5, 2, 5], [2.5, 1, 1]],
      modelMatrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 6378137, 0, 0, 1],
      emitterModelMatrix: [0, 1, 0, 0, -1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 10, 1],
    },
    frames: [0, 0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 2.5, 3, 3.25, 3.5, 4, 4.5].map((t, i) => [t, 115 + i]),
  },
  oneShot: {
    seed: 11,
    options: {
      emitter: ['sphere', 1.5], emissionRate: 6, particleLife: 0.75, speed: 2, mass: 1, imageSize: [4, 4],
      color: [0.5, 1, 0.5, 1], scale: 2, lifetime: 1.5, loop: false, sizeInMeters: true, image: [3, 3, 5],
      modelMatrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 4510023.92, 4510023.92, 1000.5, 1],
    },
    frames: [[0, 1], [0.3, 2], [0.6, 3], [0.9, 4], [1.2, 5], [1.6, 6], [2.0, 7], [2.4, 8]],
  },
  changes: {
    seed: 3,
    options: { emitter: ['cone', 0.5], emissionRate: 3, updateCallback: 'drag', modelMatrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -1000.25, 5000000.75, 3500000.5, 1] },
    frames: [
      [0, 1], [0.5, 2],
      [1, 3, { emissionRate: 8 }],
      [1.5, 4, { maximumParticleLife: 3, emitterModelMatrix: [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 5, 5, 5, 1] }],
      [1.25, 5],
      [2, 6, { show: false }],
      [2.5, 7, { show: true, startScale: 4, endColor: [0, 0, 0, 0] }],
      [3, 120, { emitter: ['box', 2, 3, 4] }],
      [3.5, 121],
    ],
  },
};

let browser;
try {
  browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, headless: true, args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  const result = await page.evaluate(async (scenarios) => {
    const ref = await import('/reference.mjs');
    const tick = () => new Promise((r) => setTimeout(r, 0));
    const fnv = (bytes) => {
      let h = 0xcbf29ce484222325n;
      for (const b of bytes) h = BigInt.asUintN(64, (h ^ BigInt(b)) * 0x100000001b3n);
      return h.toString(16);
    };
    const canon = (bytes) => {
      const out = new Uint8Array(bytes);
      const view = new DataView(out.buffer);
      for (let k = 0; k + 4 <= out.length; k += 4) if (Number.isNaN(view.getFloat32(k, true))) view.setUint32(k, 0x7fc00000, true);
      return out;
    };
    const c3 = (v) => [v.x, v.y, v.z];
    const col = (c) => [c.red, c.green, c.blue, c.alpha];
    const emitter = ([kind, ...a]) => {
      if (kind === 'circle') return new ref.CircleEmitter(a[0]);
      if (kind === 'sphere') return new ref.SphereEmitter(a[0]);
      if (kind === 'cone') return new ref.ConeEmitter(a[0]);
      return new ref.BoxEmitter(new ref.Cartesian3(...a));
    };
    const image = ([w, h, seed]) => {
      const bytes = new Uint8ClampedArray(w * h * 4);
      for (let k = 0; k < bytes.length; k++) bytes[k] = (k * 31 + seed * 17) & 255;
      return new ImageData(bytes, w, h);
    };
    const value = (k, v) => {
      if (k === 'emitter') return emitter(v);
      if (k.endsWith('Matrix')) return ref.Matrix4.fromArray(v);
      if (k.endsWith('olor')) return new ref.Color(...v);
      if (k.endsWith('mageSize')) return new ref.Cartesian2(...v);
      if (k === 'bursts') return v.map(([time, minimum, maximum]) => new ref.ParticleBurst({ time, minimum, maximum }));
      if (k === 'image') return image(v);
      if (k === 'updateCallback') return (p, dt) => { p.velocity = ref.Cartesian3.multiplyByScalar(p.velocity, 1 - 0.5 * dt, p.velocity); p.mass += dt; };
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
      c.position = new ref.Cartesian3(2e7, 1e6, 5e6);
      c.direction = ref.Cartesian3.normalize(ref.Cartesian3.negate(c.position, new ref.Cartesian3()), new ref.Cartesian3());
      c.right = ref.Cartesian3.normalize(ref.Cartesian3.cross(c.direction, ref.Cartesian3.UNIT_Z, new ref.Cartesian3()), new ref.Cartesian3());
      c.up = ref.Cartesian3.cross(c.right, c.direction, new ref.Cartesian3());
      return c;
    }
    const out = {};
    for (const [name, scenario] of Object.entries(scenarios)) {
      const canvas = document.createElement('canvas');
      canvas.width = 8; canvas.height = 8;
      const context = new ref.Context(canvas, { webgl: { antialias: false } });
      ref.Math.setRandomNumberSeed(scenario.seed);
      const system = new ref.ParticleSystem(Object.fromEntries(Object.entries(scenario.options).map(([k, v]) => [k, value(k, v)])));
      let completed = 0;
      system.complete.addEventListener(() => completed++);
      const cam = camera();
      const start = ref.JulianDate.fromIso8601('2024-01-01T00:00:00Z');
      const frames = [];
      for (const [seconds, frameNumber, changes] of scenario.frames) {
        for (const [k, v] of Object.entries(changes ?? {})) system[k] = value(k, v);
        const frameState = {
          mode: 3, morphTime: 1, mapProjection: new ref.GeographicProjection(), context, camera: cam, commandList: [], afterRender: [],
          passes: { render: true, pick: false }, minimumDisableDepthTestDistance: 0,
          time: ref.JulianDate.addSeconds(start, seconds, new ref.JulianDate()), frameNumber,
        };
        system.update(frameState);
        for (const callback of frameState.afterRender) callback();
        await tick();
        const collection = system._billboardCollection;
        const vaf = collection?._vaf;
        frames.push({
          seconds, frameNumber, changes: changes ?? null,
          currentTime: system._currentTime, carryOver: system._carryOver, complete: system.isComplete, completed,
          particleEstimate: system._particleEstimate, commands: frameState.commandList.length,
          particles: system._particles.map((p) => ({
            position: c3(p.position), velocity: c3(p.velocity), age: p.age, normalizedAge: p.normalizedAge, life: p.life, mass: p.mass,
            imageSize: [p.imageSize.x, p.imageSize.y], billboard: p._billboard?._index ?? null,
          })),
          pool: system._particlePool.map((p) => p._billboard?._index ?? null),
          billboards: collection ? (collection.length, collection._billboards.map((b) => ({
            show: b.show, position: c3(b.position), color: col(b.color), scale: b.scale, width: b.width ?? null, height: b.height ?? null,
            sizeInMeters: b.sizeInMeters, hasImage: b.image !== undefined, ready: b.ready,
          }))) : null,
          buffers: (vaf?._allBuffers ?? []).map((buffer) => {
            const bytes = canon(new Uint8Array(buffer.arrayBuffer ?? new ArrayBuffer(0)));
            return { usage: buffer.usage, length: bytes.length, hash: fnv(bytes) };
          }),
        });
      }
      out[name] = frames;
      system.destroy();
      context.destroy();
    }
    return out;
  }, scenarios);
  writeFileSync(new URL('../../tests/data/scene-particle-system.json', import.meta.url), JSON.stringify({ scenarios, capture: result }, null, 1) + '\n');
  console.log('captured', Object.keys(result).length, 'scenarios');
} finally {
  await browser?.close();
  server.close();
}
