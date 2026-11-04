// How ImageryLayer._requestImagery creates requests and handles outcomes, and
// per-server throttling and cancellation through the real RequestScheduler,
// from the pinned CesiumJS engine.
import * as C from '@cesium/engine';
import Imagery from '@cesium/engine/Source/Scene/Imagery.js';
import RequestScheduler from '@cesium/engine/Source/Core/RequestScheduler.js';
import { writeFileSync } from 'node:fs';

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const stateName = (s) => Object.keys(C.ImageryState).find((k) => C.ImageryState[k] === s);
const requestStateName = (s) => Object.keys(C.RequestState).find((k) => C.RequestState[k] === s);

// --- 1. Scripted outcomes ----------------------------------------------------
const outcomes = [];
for (const script of [
  ['undefined'],
  ['resolve'],
  ['resolveUndefined'],
  ['reject'],
  ['cancelReject'],
  ['reject', 'reject', 'resolve'],
]) {
  const requests = [];
  const errors = [];
  let creditsCalls = 0;
  let attempt = 0;
  const provider = {
    rectangle: C.Rectangle.MAX_VALUE,
    tilingScheme: new C.GeographicTilingScheme(),
    errorEvent: new C.Event(),
    getTileCredits() { creditsCalls++; return [new C.Credit('tile')]; },
    requestImage(x, y, level, request) {
      const outcome = script[Math.min(attempt++, script.length - 1)];
      requests.push({ throttle: request.throttle, throttleByServer: request.throttleByServer, type: request.type, state: requestStateName(request.state) });
      if (outcome === 'undefined') return undefined;
      if (outcome === 'resolve') return Promise.resolve({ width: 1, height: 1 });
      if (outcome === 'resolveUndefined') return Promise.resolve(undefined);
      if (outcome === 'cancelReject') { request.state = C.RequestState.CANCELLED; return Promise.reject(new Error('cancelled')); }
      return Promise.reject(new Error(`failed ${attempt}`));
    },
  };
  // Retry while the script still has later outcomes, as a listener would.
  provider.errorEvent.addEventListener((e) => {
    errors.push({ message: e.message, timesRetried: e.timesRetried, x: e.x, y: e.y, level: e.level });
    e.retry = attempt < script.length;
  });
  const layer = new C.ImageryLayer(provider);
  const imagery = new Imagery(layer, 1, 2, 3);
  layer._requestImagery(imagery);
  const afterCall = { state: stateName(imagery.state), hasRequest: imagery.request !== undefined, credits: imagery.credits?.length ?? null };
  for (let i = 0; i < 5; i++) await flush();
  outcomes.push({
    script,
    requests,
    afterCall,
    final: { state: stateName(imagery.state), hasRequest: imagery.request !== undefined, hasImage: imagery.image !== undefined },
    creditsCalls,
    errors,
  });
}

// --- 2. Per-server throttling and cancellation ------------------------------
// Image loads never complete, so requests stay active until cancelled.
// isCrossOriginUrl parses URLs through a DOM anchor; a URL-backed stand-in
// gives it the same protocol and host.
globalThis.window ??= { location: { href: 'http://localhost/' } };
globalThis.document ??= {
  createElement: () => {
    let url;
    return {
      set href(v) { url = new URL(v, 'http://localhost/'); },
      get href() { return url.href; },
      get host() { return url.host; },
      get protocol() { return url.protocol; },
    };
  },
};
const pending = [];
C.Resource._Implementations.createImage = (request, crossOrigin, deferred) => { pending.push(deferred); };
C.Resource._Implementations.loadWithXhr = () => {};
RequestScheduler.clearForSpecs();
const provider = new C.UrlTemplateImageryProvider({ url: 'https://tiles.example.com/{z}/{x}/{y}.png' });
const layer = new C.ImageryLayer(provider);
const imageries = Array.from({ length: 22 }, (_, i) => new Imagery(layer, i, 0, 5));
for (const imagery of imageries) layer._requestImagery(imagery);
const snapshot = () => ({
  states: imageries.map((im) => stateName(im.state)),
  requestStates: imageries.map((im) => (im.request ? requestStateName(im.request.state) : null)),
  active: RequestScheduler.statistics.numberOfActiveRequests,
  serverKey: imageries[0].request?.serverKey ?? null,
});
const throttling = { maximumRequestsPerServer: RequestScheduler.maximumRequestsPerServer, afterRequests: snapshot() };
// Cancel two active requests; the scheduler acts on its next update.
imageries[0].request.cancel();
imageries[3].request.cancel();
RequestScheduler.update();
for (let i = 0; i < 5; i++) await flush();
throttling.afterCancel = snapshot();
// The freed slots let two throttled tiles start.
for (const imagery of imageries) if (imagery.state === C.ImageryState.UNLOADED) layer._requestImagery(imagery);
throttling.afterRetry = snapshot();

writeFileSync('../../tests/data/scene-imagery-requests.json', JSON.stringify({ outcomes, throttling }, null, 1) + '\n');
