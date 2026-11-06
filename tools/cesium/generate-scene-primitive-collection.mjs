// PrimitiveCollection membership, reference counting, events, errors and
// destruction from the pinned CesiumJS engine. Primitives are plain objects
// whose destroy uses Cesium's destroyObject, so destroying twice throws.
import PrimitiveCollection from '@cesium/engine/Source/Scene/PrimitiveCollection.js';
import destroyObject from '@cesium/engine/Source/Core/destroyObject.js';
import { writeFileSync } from 'node:fs';

function scenario(name, collectionsOptions, script) {
  const log = [];
  const collections = collectionsOptions.map((options, i) => {
    const c = new PrimitiveCollection(options);
    c.primitiveAdded.addEventListener((p) => log.push(['added', i, p.name]));
    c.primitiveRemoved.addEventListener((p) => log.push(['removed', i, p.name]));
    return c;
  });
  const primitives = {};
  const make = (n) => {
    const p = {
      name: n,
      update: () => log.push(['update', n]),
      isDestroyed: () => false,
      destroy() { log.push(['destroy', n]); return destroyObject(this); },
    };
    primitives[n] = p;
    return p;
  };
  const steps = [];
  for (const [op, c, arg, extra] of script) {
    const before = log.length;
    let result = null;
    let error = null;
    try {
      const col = collections[c];
      const p = typeof arg === 'string' ? (primitives[arg] ?? make(arg)) : arg;
      switch (op) {
        case 'add': result = col.add(p, extra ?? undefined).name; break;
        case 'remove': result = col.remove(p); break;
        case 'removeAndDestroy': result = col.removeAndDestroy(p); break;
        case 'removeAll': col.removeAll(); break;
        case 'raise': case 'raiseToTop': case 'lower': case 'lowerToBottom': col[op](p); break;
        case 'get': result = col.get(arg)?.name ?? null; break;
        case 'update': col.update({}); break;
        case 'destroy': col.destroy(); break;
        default: throw new Error(`unknown op ${op}`);
      }
    } catch (e) {
      error = e.message;
    }
    steps.push({
      op, collection: c, arg: arg ?? null, index: extra ?? null, result, error,
      events: log.slice(before),
      order: collections.map((col) => (col.isDestroyed() ? null : col._primitives.map((p) => p.name))),
      contains: collections.map((col) => (col.isDestroyed() ? null : Object.fromEntries(Object.keys(primitives).map((n) => [n, col.contains(primitives[n])])))),
      referenceCounts: Object.fromEntries(Object.entries(primitives).map(([n, p]) => [n, p._external?._referenceCount ?? null])),
      destroyed: collections.map((col) => col.isDestroyed()),
    });
  }
  return { name, collections: collectionsOptions, steps };
}

const scenarios = [
  scenario('events-and-indices', [{}], [
    ['add', 0, 'a'], ['add', 0, 'b'], ['add', 0, 'c', 1], ['add', 0, 'd', 0], ['add', 0, 'e', 5], ['add', 0, 'e', 4],
    ['get', 0, 0], ['get', 0, 4], ['get', 0, 9], ['update', 0],
    ['raise', 0, 'e'], ['raiseToTop', 0, 'd'], ['lower', 0, 'd'], ['lowerToBottom', 0, 'a'], ['lower', 0, 'a'],
    ['raise', 0, 'zz'], ['remove', 0, 'zz'],
    ['remove', 0, 'c'], ['remove', 0, 'c'], ['update', 0],
  ]),
  scenario('destroy-policy', [{ destroyPrimitives: false }], [
    ['add', 0, 'a'], ['add', 0, 'b'], ['add', 0, 'c'],
    ['remove', 0, 'a'], ['removeAndDestroy', 0, 'b'], ['removeAll', 0], ['update', 0], ['destroy', 0],
  ]),
  scenario('shared-without-counting', [{}, {}], [
    ['add', 0, 'a'], ['add', 1, 'a'], ['add', 0, 'b'],
    ['remove', 0, 'a'], ['remove', 1, 'a'], ['removeAll', 0],
  ]),
  scenario('shared-with-counting', [{ countReferences: true }, { countReferences: true }], [
    ['add', 0, 'a'], ['add', 1, 'a'], ['add', 0, 'b'],
    ['remove', 0, 'a'], ['remove', 1, 'a'], ['removeAndDestroy', 0, 'b'],
  ]),
  scenario('counting-mixed', [{ countReferences: true }, {}], [
    ['add', 0, 'a'], ['add', 1, 'a'], ['remove', 1, 'a'], ['remove', 0, 'a'],
  ]),
  scenario('added-twice', [{}], [
    ['add', 0, 'a'], ['add', 0, 'a'], ['add', 0, 'b'], ['update', 0],
    ['remove', 0, 'a'], ['remove', 0, 'a'], ['raise', 0, 'a'], ['update', 0], ['destroy', 0],
  ]),
  scenario('added-twice-counting', [{ countReferences: true }], [
    ['add', 0, 'a'], ['add', 0, 'a'], ['removeAll', 0],
  ]),
  scenario('hidden', [{ show: false }], [['add', 0, 'a'], ['update', 0], ['destroy', 0]]),
];

writeFileSync('../../tests/data/scene-primitive-collection.json', JSON.stringify({ scenarios }, null, 1) + '\n');
