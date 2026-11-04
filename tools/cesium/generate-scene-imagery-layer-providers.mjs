// ImageryLayer.fromProviderAsync and ImageryLayerCollection provider-ready
// forwarding from the pinned CesiumJS engine. Providers are plain objects
// resolved or rejected by the script; every event is recorded in order.
import * as C from '@cesium/engine';
import { writeFileSync } from 'node:fs';

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((a, b) => { resolve = a; reject = b; });
  return { promise, resolve, reject };
};
const fakeProvider = (name) => ({ name, rectangle: C.Rectangle.MAX_VALUE, tilingScheme: new C.GeographicTilingScheme(), errorEvent: new C.Event() });

const scenarios = [];
async function scenario(name, script) {
  const log = [];
  const collection = new C.ImageryLayerCollection();
  collection.layerAdded.addEventListener((layer, index) => log.push(['layerAdded', layer.name, index]));
  collection.layerRemoved.addEventListener((layer, index) => log.push(['layerRemoved', layer.name, index]));
  collection.layerMoved.addEventListener((layer, to, from) => log.push(['layerMoved', layer.name, to, from]));
  collection.layerShownOrHidden.addEventListener((layer, index, show) => log.push(['layerShownOrHidden', layer.name, index, show]));
  const consoleError = console.error;
  console.error = (e) => log.push(['consoleError', String(e?.message ?? e)]);
  const layers = {};
  const providers = {};
  const make = (layerName, options = {}, listen = { ready: true, error: true }) => {
    providers[layerName] = deferred();
    const layer = C.ImageryLayer.fromProviderAsync(providers[layerName].promise, options);
    layer.name = layerName;
    if (listen.ready) layer.readyEvent.addEventListener((p) => log.push(['readyEvent', layerName, p.name]));
    if (listen.error) layer.errorEvent.addEventListener((e) => log.push(['errorEvent', layerName, String(e?.message ?? e)]));
    layers[layerName] = layer;
    return layer;
  };
  const steps = [];
  const ops = {
    make: (l, options, listen) => make(l, options, listen),
    add: (l, index) => collection.add(layers[l], index),
    remove: (l, destroy) => collection.remove(layers[l], destroy),
    raise: (l) => collection.raise(layers[l]),
    lowerToBottom: (l) => collection.lowerToBottom(layers[l]),
    destroy: (l) => layers[l].destroy(),
    resolve: (l) => providers[l].resolve(fakeProvider(`provider-${l}`)),
    reject: (l) => providers[l].reject(new Error(`failed ${l}`)),
    flush: () => {},
  };
  for (const [op, ...args] of script) {
    const before = log.length;
    ops[op](...args);
    await flush();
    const state = Object.fromEntries(Object.entries(layers).map(([n, l]) => [n, {
      ready: l.isDestroyed() ? null : l.ready,
      provider: l.isDestroyed() ? null : l.imageryProvider?.name ?? null,
      index: l._layerIndex,
      destroyed: l.isDestroyed(),
    }]));
    steps.push({ op, args: args.map((a) => a ?? null), events: log.slice(before), state, order: collection._layers.map((l) => l.name) });
  }
  console.error = consoleError;
  scenarios.push({ name, steps });
}

await scenario('resolve-before-add', [['make', 'a'], ['resolve', 'a'], ['add', 'a']]);
await scenario('add-then-resolve', [['make', 'a'], ['make', 'b', { show: false }], ['add', 'a'], ['add', 'b'], ['resolve', 'b'], ['resolve', 'a']]);
await scenario('moved-before-ready', [['make', 'a'], ['make', 'b'], ['make', 'c'], ['add', 'a'], ['add', 'b'], ['add', 'c'], ['lowerToBottom', 'c'], ['raise', 'a'], ['resolve', 'c'], ['resolve', 'a'], ['resolve', 'b']]);
await scenario('removed-before-ready', [['make', 'a'], ['make', 'b'], ['add', 'a'], ['add', 'b'], ['remove', 'a', false], ['resolve', 'a'], ['resolve', 'b']]);
await scenario('added-twice', [['make', 'a'], ['add', 'a'], ['add', 'a', 0], ['resolve', 'a']]);
await scenario('destroyed-before-ready', [['make', 'a'], ['add', 'a'], ['remove', 'a', true], ['resolve', 'a']]);
await scenario('destroyed-then-rejected', [['make', 'a'], ['destroy', 'a'], ['reject', 'a']]);
await scenario('rejected-with-listener', [['make', 'a'], ['add', 'a'], ['reject', 'a']]);
await scenario('rejected-without-listener', [['make', 'a', {}, { ready: true, error: false }], ['add', 'a'], ['reject', 'a']]);

writeFileSync('../../tests/data/scene-imagery-layer-providers.json', JSON.stringify({ scenarios }, null, 1) + '\n');
