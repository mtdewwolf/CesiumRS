// Direct reference traces from the pinned CesiumJS engine for GlobeTranslucency,
// PerformanceDisplay, FrameRateMonitor and ImageBasedLighting. Browser globals
// (performance.now, document) are replaced by scripted stand-ins.
import { writeFileSync } from 'node:fs';

let now = 0;
Object.defineProperty(globalThis, 'performance', { value: { now: () => now }, configurable: true });
const listeners = {};
const fakeDocument = {
  hidden: false,
  addEventListener: (name, f) => { listeners[name] = f; },
  removeEventListener: (name) => { delete listeners[name]; },
  createElement: () => ({ appendChild() {}, className: '' }),
  createTextNode: (value) => ({ nodeValue: value }),
  location: { href: 'http://localhost/' },
};
globalThis.document = fakeDocument;

const C = await import('@cesium/engine');
const { default: GlobeTranslucency } = await import('@cesium/engine/Source/Scene/GlobeTranslucency.js');
const { default: PerformanceDisplay } = await import('@cesium/engine/Source/Scene/PerformanceDisplay.js');
const { default: FrameRateMonitor } = await import('@cesium/engine/Source/Scene/FrameRateMonitor.js');
const { default: ImageBasedLighting } = await import('@cesium/engine/Source/Scene/ImageBasedLighting.js');
const { default: SpecularEnvironmentCubeMap } = await import('@cesium/engine/Source/Scene/SpecularEnvironmentCubeMap.js');

const num = (v) => (v === undefined ? null : Number.isNaN(v) ? 'NaN' : v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Object.is(v, -0) ? '-0' : v);
const attempt = (f) => { try { f(); return null; } catch (e) { return e.message; } };

// --- GlobeTranslucency -----------------------------------------------------
const globeTranslucency = (() => {
  const t = new GlobeTranslucency();
  const nfs = (v) => (v === null ? undefined : new C.NearFarScalar(...v));
  const steps = [];
  const snapshot = () => ({
    enabled: t.enabled,
    frontFaceAlpha: num(t.frontFaceAlpha),
    backFaceAlpha: num(t.backFaceAlpha),
    frontFaceAlphaByDistance: t.frontFaceAlphaByDistance ? [t.frontFaceAlphaByDistance.near, t.frontFaceAlphaByDistance.nearValue, t.frontFaceAlphaByDistance.far, t.frontFaceAlphaByDistance.farValue].map(num) : null,
    backFaceAlphaByDistance: t.backFaceAlphaByDistance ? [t.backFaceAlphaByDistance.near, t.backFaceAlphaByDistance.nearValue, t.backFaceAlphaByDistance.far, t.backFaceAlphaByDistance.farValue].map(num) : null,
    rectangle: [t.rectangle.west, t.rectangle.south, t.rectangle.east, t.rectangle.north].map(num),
  });
  steps.push({ op: 'initial', error: null, state: snapshot() });
  const ops = [
    ['enabled', true],
    ['frontFaceAlpha', 0.5], ['frontFaceAlpha', 0], ['frontFaceAlpha', 1], ['frontFaceAlpha', -0.1], ['frontFaceAlpha', 1.5], ['frontFaceAlpha', NaN], ['frontFaceAlpha', -0],
    ['backFaceAlpha', 0.25], ['backFaceAlpha', 2], ['backFaceAlpha', -1e-9], ['backFaceAlpha', NaN],
    ['frontFaceAlphaByDistance', [10, 0.2, 100, 1]], ['frontFaceAlphaByDistance', [100, 0.2, 10, 1]], ['frontFaceAlphaByDistance', [50, 0.1, 50, 0.9]],
    ['frontFaceAlphaByDistance', [NaN, 0.1, 5, 0.9]], ['frontFaceAlphaByDistance', null],
    ['backFaceAlphaByDistance', [1, 0, 2, 1]], ['backFaceAlphaByDistance', [3, 0, 2, 1]],
    ['rectangle', [-1, -0.5, 1, 0.5]], ['rectangle', null], ['rectangle', [0.1, 0.2, 0.3, 0.4]],
  ];
  for (const [name, value] of ops) {
    const v = name.endsWith('ByDistance') ? nfs(value) : name === 'rectangle' ? (value === null ? undefined : new C.Rectangle(...value)) : value;
    const error = attempt(() => { t[name] = v; });
    steps.push({ op: name, value: Array.isArray(value) ? value.map(num) : num(value), error, state: snapshot() });
  }
  return steps;
})();

// --- PerformanceDisplay ----------------------------------------------------
const performanceDisplay = (() => {
  const traces = [];
  const scripts = [
    // Ties: 400.25 ms over two frames is 200.125, which toFixed rounds up.
    { start: 0, frames: [[200.125, true], [400.25, true], [1000.5, true], [1001, true], [1201.375, true], [1401.4, false], [2600, true]] },
    // Many frames in one window: the frame rate truncates toward zero.
    { start: 10, frames: Array.from({ length: 70 }, (_, i) => [10 + (i + 1) * 15.7, i % 13 !== 5]) },
    // A non-finite clock: 0 FPS through ToInt32, "Infinity" through toFixed.
    { start: 0, frames: [[5, true], [Infinity, true], [Infinity, true]] },
  ];
  for (const script of scripts) {
    now = script.start;
    const display = new PerformanceDisplay({ container: { appendChild() {} } });
    const trace = [];
    for (const [time, rendered] of script.frames) {
      now = time;
      display.update(rendered);
      trace.push({ time: num(time), rendered, fps: display._fpsText.nodeValue, ms: display._msText.nodeValue });
    }
    display.throttled = true;
    const throttledOn = display._throttledText.nodeValue;
    display.throttled = false;
    traces.push({ start: script.start, trace, throttledOn, throttledOff: display._throttledText.nodeValue });
  }
  return traces;
})();

// --- FrameRateMonitor ------------------------------------------------------
const frameRateMonitor = (() => {
  const scenarios = [];
  const scripts = [
    { name: 'defaults-60fps-then-drop', options: {}, ops: [...Array.from({ length: 500 }, (_, i) => ['frame', i * 1000 / 60]), ...Array.from({ length: 80 }, (_, i) => ['frame', 8400 + i * 400]), ...Array.from({ length: 900 }, (_, i) => ['frame', 40500 + i * 1000 / 60])] },
    { name: 'custom-warmup', options: { samplingWindow: 1, quietPeriod: 0.5, warmupPeriod: 1, minimumFrameRateDuringWarmup: 10, minimumFrameRateAfterWarmup: 20 }, ops: [...Array.from({ length: 60 }, (_, i) => ['frame', i * 1000 / 12]), ...Array.from({ length: 80 }, (_, i) => ['frame', 5000 + i * 1000 / 15]), ...Array.from({ length: 80 }, (_, i) => ['frame', 11000 + i * 1000 / 40])] },
    { name: 'pause-visibility', options: { samplingWindow: 0.5, quietPeriod: 0.1, warmupPeriod: 0 }, ops: [...Array.from({ length: 40 }, (_, i) => ['frame', i * 50]), ['pause'], ['frame', 2100], ['pause'], ['unpause'], ['frame', 2150], ['unpause'], ['unpause'], ['frame', 2200], ...Array.from({ length: 30 }, (_, i) => ['frame', 2250 + i * 30]), ['hidden', true], ['frame', 3500], ['hidden', false], ...Array.from({ length: 30 }, (_, i) => ['frame', 3600 + i * 250])] },
    { name: 'zero-interval', options: { samplingWindow: 0, quietPeriod: 0, warmupPeriod: 0 }, ops: [['frame', 0], ['frame', 5], ['frame', 5], ['frame', 5], ['frame', 6], ['frame', 1000], ['frame', 1000]] },
  ];
  for (const script of scripts) {
    const scene = { preUpdate: new C.Event() };
    const monitor = new FrameRateMonitor({ scene, ...script.options });
    const events = [];
    monitor.lowFrameRate.addEventListener((s, fps) => events.push(['low', num(fps)]));
    monitor.nominalFrameRate.addEventListener((s, fps) => events.push(['nominal', num(fps)]));
    const trace = [];
    for (const [op, value] of script.ops) {
      events.length = 0;
      if (op === 'frame') { now = value; scene.preUpdate.raiseEvent(scene, undefined); }
      else if (op === 'pause') monitor.pause();
      else if (op === 'unpause') monitor.unpause();
      else if (op === 'hidden') { fakeDocument.hidden = value; listeners.visibilitychange(); }
      trace.push({ op, value: value ?? null, fps: num(monitor.lastFramesPerSecond), events: [...events] });
    }
    fakeDocument.hidden = false;
    monitor.destroy();
    scenarios.push({ name: script.name, options: script.options, trace });
  }
  return { defaults: FrameRateMonitor.defaultSettings, scenarios };
})();

// --- ImageBasedLighting ----------------------------------------------------
const imageBasedLighting = (() => {
  // Replace the KTX2 loader: a cube map becomes ready once the script says so.
  const readyUrls = new Set();
  const created = [];
  const destroyed = [];
  SpecularEnvironmentCubeMap.isSupported = (context) => context.supported;
  SpecularEnvironmentCubeMap.prototype.update = function () { if (readyUrls.has(this._url)) this._ready = true; };
  SpecularEnvironmentCubeMap.prototype.destroy = function () { destroyed.push(this._url); return undefined; };

  const sh = { A: Array.from({ length: 9 }, (_, i) => new C.Cartesian3(i, 0, 0)), B: Array.from({ length: 9 }, (_, i) => new C.Cartesian3(0, i, 0)) };
  const contexts = { A: { supported: true }, B: { supported: true }, U: { supported: false } };
  const constructors = [
    {}, { imageBasedLightingFactor: [0.5, 0] }, { imageBasedLightingFactor: [1.5, 0] }, { imageBasedLightingFactor: [0, -0.5] }, { imageBasedLightingFactor: [NaN, 1] },
    { sphericalHarmonicCoefficients: 'A' }, { sphericalHarmonicCoefficients: 'short' }, { specularEnvironmentMaps: 'a.ktx2' },
  ].map((o) => {
    const options = {
      imageBasedLightingFactor: o.imageBasedLightingFactor && new C.Cartesian2(...o.imageBasedLightingFactor),
      sphericalHarmonicCoefficients: o.sphericalHarmonicCoefficients === 'short' ? [new C.Cartesian3()] : sh[o.sphericalHarmonicCoefficients],
      specularEnvironmentMaps: o.specularEnvironmentMaps,
    };
    let ibl; const error = attempt(() => { ibl = new ImageBasedLighting(options); });
    return { options: { ...o, imageBasedLightingFactor: o.imageBasedLightingFactor?.map(num) }, error, factor: ibl ? [ibl.imageBasedLightingFactor.x, ibl.imageBasedLightingFactor.y].map(num) : null, enabled: ibl ? ibl.enabled : null };
  });

  const scripts = [
    { name: 'factor-transitions', ops: [['update', 1, 'A'], ['factor', [0, 1]], ['update', 2, 'A'], ['factor', [0.5, 1]], ['update', 3, 'A'], ['factor', [0, 0]], ['factor', [0.25, 1]], ['update', 4, 'A'], ['update', 4, 'A'], ['factor', [0, 0]], ['update', 4, 'B'], ['factor', [2, 0]], ['factor', [0, NaN]], ['update', 5, 'B']] },
    { name: 'spherical-harmonics', ops: [['update', 1, 'A'], ['sh', 'A'], ['update', 2, 'A'], ['sh', 'B'], ['update', 3, 'A'], ['sh', null], ['sh', 'A'], ['update', 4, 'A'], ['sh', null], ['update', 5, 'A'], ['update', 6, 'A', { frameSH: true }], ['update', 7, 'A', { frameSH: true }], ['sh', 'B'], ['update', 8, 'A', { frameSH: true }], ['sh', null], ['update', 9, 'A']] },
    { name: 'specular-maps', ops: [['update', 1, 'A', { frameMaps: true }], ['update', 2, 'A', { frameMaps: true }], ['maps', 'a.ktx2'], ['update', 3, 'A', { frameMaps: true }], ['ready', 'a.ktx2'], ['update', 4, 'A'], ['update', 5, 'A'], ['maps', 'a.ktx2'], ['update', 6, 'A'], ['maps', 'b.ktx2'], ['update', 7, 'A'], ['maps', null], ['update', 8, 'A', { frameMaps: true }], ['update', 9, 'A'], ['maps', 'c.ktx2'], ['update', 10, 'U', { frameMaps: true }], ['update', 11, 'A'], ['ready', 'c.ktx2'], ['update', 12, 'A'], ['destroy']] },
    { name: 'unsupported-context', ops: [['maps', 'a.ktx2'], ['update', 1, 'U', { frameMaps: true, frameSH: true }], ['update', 2, 'U'], ['update', 3, 'A'], ['update', 4, 'U', { frameMaps: true }]] },
  ];
  const traces = [];
  for (const script of scripts) {
    created.length = 0; destroyed.length = 0; readyUrls.clear();
    const ibl = new ImageBasedLighting();
    const trace = [];
    for (const [op, a, b, extra] of script.ops) {
      let error = null;
      const before = ibl._specularEnvironmentCubeMap;
      if (op === 'factor') error = attempt(() => { ibl.imageBasedLightingFactor = new C.Cartesian2(...a); });
      else if (op === 'sh') ibl.sphericalHarmonicCoefficients = a === null ? undefined : sh[a];
      else if (op === 'maps') ibl.specularEnvironmentMaps = a ?? undefined;
      else if (op === 'ready') readyUrls.add(a);
      else if (op === 'destroy') ibl.destroy();
      else if (op === 'update') {
        ibl.update({ frameNumber: a, context: contexts[b], brdfLutGenerator: { update() {} }, specularEnvironmentMaps: extra?.frameMaps ? 'default.ktx2' : undefined, sphericalHarmonicCoefficients: extra?.frameSH ? sh.B : undefined });
      }
      const cubeMap = op === 'destroy' ? undefined : ibl.specularEnvironmentCubeMap;
      if (cubeMap && cubeMap !== before) created.push(cubeMap.url);
      trace.push({
        op, a: Array.isArray(a) ? a.map(num) : a ?? null, context: op === 'update' ? b : null, frameMaps: !!extra?.frameMaps, frameSH: !!extra?.frameSH, error,
        factor: op === 'destroy' ? null : [ibl.imageBasedLightingFactor.x, ibl.imageBasedLightingFactor.y].map(num),
        enabled: op === 'destroy' ? null : ibl.enabled,
        shouldRegenerateShaders: op === 'destroy' ? null : ibl.shouldRegenerateShaders,
        useDefaultSpecularMaps: op === 'destroy' ? null : ibl.useDefaultSpecularMaps,
        useDefaultSphericalHarmonics: op === 'destroy' ? null : ibl.useDefaultSphericalHarmonics,
        useSpecularEnvironmentMaps: op === 'destroy' ? null : ibl.useSpecularEnvironmentMaps,
        cubeMap: cubeMap ? cubeMap.url : null,
        created: [...created], destroyed: [...destroyed],
      });
    }
    traces.push({ name: script.name, trace });
  }
  return { constructors, traces };
})();

writeFileSync('../../tests/data/scene-cpu-utilities.json', JSON.stringify({ globeTranslucency, performanceDisplay, frameRateMonitor, imageBasedLighting }) + '\n');
