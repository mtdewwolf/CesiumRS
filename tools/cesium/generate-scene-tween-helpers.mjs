// TweenCollection helpers (addProperty, addAlpha, addOffsetIncrement) and
// lifecycle (remove, removeAll, cancelTween, contains, get, zero durations,
// errors) from the pinned CesiumJS engine. Times are explicit seconds.
import { writeFileSync } from 'node:fs';

for (const name of ['HTMLCanvasElement', 'HTMLImageElement', 'HTMLVideoElement', 'ImageBitmap', 'OffscreenCanvas']) {
  globalThis[name] ??= class {};
}
const C = await import('@cesium/engine');
const { default: TweenCollection } = await import('@cesium/engine/Source/Scene/TweenCollection.js');

const times = [0, 0.25, 0.5, 0.75, 1, 1.5, 2, 2.5, 3, 3.25, 3.6, 5];
const attempt = (f) => { try { return { value: f(), error: null }; } catch (e) { return { value: null, error: e.message }; } };

// --- addProperty ------------------------------------------------------------
const property = (() => {
  const collection = new TweenCollection();
  const object = { x: 5, other: 1 };
  const events = [];
  collection.addProperty({
    object, property: 'x', startValue: 0, stopValue: 10, duration: 2, delay: 0.5,
    easingFunction: C.EasingFunction.QUADRATIC_IN_OUT, complete: () => events.push('complete'), cancel: () => events.push('cancel'),
  });
  return times.map((t) => { collection.update(t); return { time: t, x: object.x, other: object.other, length: collection.length, events: [...events] }; });
})();

// --- addAlpha ---------------------------------------------------------------
const alpha = (() => {
  const collection = new TweenCollection();
  const stripe = C.Material.fromType('Stripe');
  const color = C.Material.fromType('Color');
  collection.addAlpha({ material: stripe, startValue: 0.2, stopValue: 0.8, duration: 1, easingFunction: C.EasingFunction.CUBIC_OUT });
  collection.addAlpha({ material: color });
  return times.map((t) => {
    collection.update(t);
    return { time: t, even: stripe.uniforms.evenColor.alpha, odd: stripe.uniforms.oddColor.alpha, color: color.uniforms.color.alpha, length: collection.length };
  });
})();

// --- addOffsetIncrement -------------------------------------------------------
const offset = (() => {
  const collection = new TweenCollection();
  const stripe = C.Material.fromType('Stripe');
  stripe.uniforms.offset = 0.25;
  const events = [];
  collection.addOffsetIncrement({
    material: stripe, duration: 1, delay: 0.25,
    update: () => events.push('update'), cancel: () => events.push('cancel'),
  });
  const trace = times.map((t) => { collection.update(t); return { time: t, offset: stripe.uniforms.offset, length: collection.length, events: [...events] }; });
  collection.removeAll();
  trace.push({ time: null, offset: stripe.uniforms.offset, length: collection.length, events: [...events] });
  return trace;
})();

// --- Lifecycle ----------------------------------------------------------------
const lifecycle = (() => {
  const collection = new TweenCollection();
  const log = [];
  const make = (name, duration) => collection.add({
    startObject: { v: 0 }, stopObject: { v: 1 }, duration,
    update: (value) => log.push([name, 'update', value.v]), complete: () => log.push([name, 'complete']), cancel: () => log.push([name, 'cancel']),
  });
  const steps = [];
  const record = (op, result) => steps.push({ op, result, length: collection.length, log: log.splice(0) });
  const a = make('a', 1);
  const b = make('b', 2);
  const c = make('c', 3);
  record('add three', null);
  const zero = make('zero', 0);
  record('add zero', { contains: collection.contains(zero), hasCollectionTween: zero.tweenjs !== undefined });
  record('get', [0, 1, 2, 3].map((i) => (collection.get(i) === a ? 'a' : collection.get(i) === b ? 'b' : collection.get(i) === c ? 'c' : null)));
  collection.update(0);
  record('update 0', null);
  collection.update(0.5);
  record('update 0.5', null);
  record('remove b', collection.remove(b));
  record('remove b again', collection.remove(b));
  record('contains', { a: collection.contains(a), b: collection.contains(b), c: collection.contains(c) });
  record('cancelTween c', c.cancelTween());
  collection.update(1);
  record('update 1', null);
  const d = make('d', 1);
  const e = make('e', 1);
  record('add d e', null);
  collection.removeAll();
  record('removeAll', { d: collection.contains(d), e: collection.contains(e) });
  collection.update(2);
  record('update 2', null);
  return steps;
})();

const errors = [
  ['negative duration', () => new TweenCollection().add({ startObject: { v: 0 }, stopObject: { v: 1 }, duration: -1 })],
  ['missing property', () => new TweenCollection().addProperty({ object: { x: 1 }, property: 'y', startValue: 0, stopValue: 1 })],
  ['no alpha uniforms', () => new TweenCollection().addAlpha({ material: new C.Material({ fabric: { uniforms: { u: 1.0 }, source: 'czm_material czm_getMaterial(czm_materialInput m) { return czm_getDefaultMaterial(m); }' } }) })],
  ['no offset uniform', () => new TweenCollection().addOffsetIncrement({ material: C.Material.fromType('Color') })],
].map(([name, f]) => ({ name, error: attempt(f).error }));

writeFileSync('../../tests/data/scene-tween-helpers.json', JSON.stringify({ property, alpha, offset, lifecycle, errors }, null, 1) + '\n');
