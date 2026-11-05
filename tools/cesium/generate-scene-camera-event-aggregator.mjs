// Broad CameraEventAggregator sequences from the pinned CesiumJS engine: every
// button with every modifier, overlapping buttons, modifier handoff, unmatched
// ups, wheels, pinches and a long seeded mix. Uses the same command format and
// state encoding as generate-scene-input-tweens-morphing.mjs.
import fs from 'node:fs';
import CameraEventAggregator from '@cesium/engine/Source/Scene/CameraEventAggregator.js';
import { Cartesian2 } from '@cesium/engine';

let now = 0;
const RealDate = globalThis.Date;
globalThis.Date = class extends RealDate {
  constructor(...args) { super(...(args.length ? args : [now * 1000])); }
  static now() { return now * 1000; }
};
function element() {
  return { clientWidth: 800, clientHeight: 600, disableRootEvents: true, onwheel: null, listeners: new Map(), addEventListener(k, f) { this.listeners.set(k, f); }, removeEventListener(k) { this.listeners.delete(k); } };
}
globalThis.document = element();
globalThis.window = element();
const v2 = (p) => [p.x, p.y];
const c2 = (p) => new Cartesian2(...p);
const movementValue = (m) => (m?.startPosition ? { start: v2(m.startPosition), end: v2(m.endPosition) } : null);
function aggregateState(a) {
  const states = [];
  for (let type = 0; type < 5; type++) {
    for (const modifier of [undefined, 0, 1, 2]) {
      const m = a.getMovement(type, modifier);
      states.push({
        type, modifier: modifier ?? null, moving: a.isMoving(type, modifier), down: !!a.isButtonDown(type, modifier),
        start: v2(a.getStartMousePosition(type, modifier)),
        press: a.getButtonPressTime(type, modifier)?.getTime() / 1000 ?? null,
        release: a.getButtonReleaseTime(type, modifier)?.getTime() / 1000 ?? null,
        movement: movementValue(m), distance: movementValue(m.distance), angle: movementValue(m.angleAndHeight),
        last: movementValue(a.getLastMovement(type, modifier)),
      });
    }
  }
  return { mouse: v2(a.currentMousePosition), any: a.anyButtonDown, states };
}
function fire(a, cmd) {
  now = cmd.time;
  if (cmd.reset) { a.reset(); return true; }
  const data = cmd.position ? { position: c2(cmd.position) }
    : cmd.start ? { startPosition: c2(cmd.start), endPosition: c2(cmd.end) }
      : cmd.delta !== undefined ? cmd.delta
        : cmd.p1 ? { position1: c2(cmd.p1), position2: c2(cmd.p2) }
          : cmd.distance ? { distance: { startPosition: c2(cmd.distance[0]), endPosition: c2(cmd.distance[1]) }, angleAndHeight: { startPosition: c2(cmd.angle[0]), endPosition: c2(cmd.angle[1]) } }
            : undefined;
  const action = a._eventHandler.getInputAction(cmd.kind, cmd.modifier);
  if (!action) throw new Error(`unregistered action ${cmd.kind}/${cmd.modifier}`);
  action(data);
  return true;
}

// ScreenSpaceEventType values.
const K = { LEFT_DOWN: 0, LEFT_UP: 1, RIGHT_DOWN: 5, RIGHT_UP: 6, MIDDLE_DOWN: 10, MIDDLE_UP: 11, MOVE: 15, WHEEL: 16, PINCH_START: 17, PINCH_END: 18, PINCH_MOVE: 19 };
const down = (kind, time, position, modifier) => ({ kind, time, position, ...(modifier !== undefined && { modifier }) });
const move = (time, start, end, modifier) => ({ kind: K.MOVE, time, start, end, ...(modifier !== undefined && { modifier }) });

const sequences = {};
// Each button with each modifier: down, two moves, up.
for (const [name, d, u] of [['left', K.LEFT_DOWN, K.LEFT_UP], ['right', K.RIGHT_DOWN, K.RIGHT_UP], ['middle', K.MIDDLE_DOWN, K.MIDDLE_UP]]) {
  for (const modifier of [undefined, 0, 1, 2]) {
    sequences[`${name}-${modifier ?? 'none'}`] = [
      down(d, 1, [100, 120], modifier), move(1.01, [100, 120], [110, 125], modifier), move(1.02, [110, 125], [140, 100], modifier),
      { kind: u, time: 1.05, position: [140, 100], ...(modifier !== undefined && { modifier }) }, move(1.06, [140, 100], [150, 90], modifier),
    ];
  }
}
sequences.overlapping = [
  down(K.LEFT_DOWN, 1, [10, 10]), move(1.01, [10, 10], [20, 30]), down(K.RIGHT_DOWN, 1.02, [20, 30]), move(1.03, [20, 30], [45, 35]),
  { kind: K.LEFT_UP, time: 1.04, position: [45, 35] }, move(1.05, [45, 35], [60, 40]), down(K.MIDDLE_DOWN, 1.06, [60, 40]),
  move(1.07, [60, 40], [61, 70]), { kind: K.RIGHT_UP, time: 1.08, position: [61, 70] }, { kind: K.MIDDLE_UP, time: 1.09, position: [61, 70] },
];
sequences.handoff = [
  down(K.LEFT_DOWN, 1, [200, 200]), move(1.01, [200, 200], [210, 205]), move(1.02, [210, 205], [220, 230], 0),
  move(1.03, [220, 230], [240, 240], 1), { kind: K.LEFT_UP, time: 1.04, position: [240, 240], modifier: 1 },
  down(K.RIGHT_DOWN, 1.05, [240, 240], 2), move(1.06, [240, 240], [250, 250]), { kind: K.RIGHT_UP, time: 1.07, position: [250, 250] },
];
sequences.unmatched = [
  { kind: K.LEFT_UP, time: 1, position: [5, 5] }, down(K.LEFT_DOWN, 1.01, [5, 5]), down(K.LEFT_DOWN, 1.02, [6, 6]),
  move(1.03, [6, 6], [9, 9]), { kind: K.LEFT_UP, time: 1.04, position: [9, 9] }, { kind: K.LEFT_UP, time: 1.05, position: [9, 9] },
  move(1.06, [9, 9], [12, 3]),
];
sequences.wheels = [
  { kind: K.WHEEL, time: 1, delta: 120 }, { kind: K.WHEEL, time: 1, delta: 240 }, { kind: K.WHEEL, time: 1.01, delta: -40, modifier: 0 },
  { reset: true, time: 1.02 }, { kind: K.WHEEL, time: 1.03, delta: -360, modifier: 2 }, { kind: K.WHEEL, time: 1.04, delta: 0 },
];
sequences.pinches = [
  { kind: K.PINCH_MOVE, time: 0.9, distance: [[0, 1], [0, 2]], angle: [[0, 1], [0.1, 1]] },
  { kind: K.PINCH_START, time: 1, p1: [100, 100], p2: [200, 150] },
  { kind: K.PINCH_MOVE, time: 1.01, distance: [[0, 100], [0, 130]], angle: [[0.4, 5], [0.6, 8]] },
  { kind: K.PINCH_MOVE, time: 1.02, distance: [[0, 130], [0, 90]], angle: [[0.6, 8], [-3.1, 4]] },
  { kind: K.PINCH_END, time: 1.03 },
  { kind: K.PINCH_START, time: 1.04, p1: [10, 10], p2: [20, 20], modifier: 1 },
  { kind: K.PINCH_MOVE, time: 1.05, distance: [[0, 14], [0, 18]], angle: [[3.1, 2], [-3.1, 3]], modifier: 1 },
  { kind: K.PINCH_END, time: 1.06, modifier: 1 },
];
// A long seeded mix of registered actions.
{
  let seed = 7;
  const random = () => ((seed = (Math.imul(seed, 1103515245) + 12345) >>> 0) / 2 ** 32);
  const pick = (list) => list[Math.floor(random() * list.length)];
  const modifiers = [undefined, undefined, 0, 1, 2];
  let mouse = [400, 300];
  let time = 1;
  const steps = [];
  for (let i = 0; i < 80; i++) {
    time = Math.round((time + 0.01 + random() * 0.05) * 1000) / 1000;
    const modifier = pick(modifiers);
    const kind = pick([K.LEFT_DOWN, K.LEFT_UP, K.RIGHT_DOWN, K.RIGHT_UP, K.MIDDLE_DOWN, K.MIDDLE_UP, K.MOVE, K.MOVE, K.MOVE, K.WHEEL]);
    if (kind === K.MOVE) {
      const next = [Math.round(mouse[0] + (random() - 0.5) * 80), Math.round(mouse[1] + (random() - 0.5) * 80)];
      steps.push(move(time, mouse, next, modifier));
      mouse = next;
    } else if (kind === K.WHEEL) {
      steps.push({ kind, time, delta: pick([120, -120, 60]), ...(modifier !== undefined && { modifier }) });
    } else {
      steps.push({ kind, time, position: mouse, ...(modifier !== undefined && { modifier }) });
    }
    if (i % 27 === 26) steps.push({ reset: true, time });
  }
  sequences.mixed = steps;
}

const out = {};
for (const [name, commands] of Object.entries(sequences)) {
  const aggregator = new CameraEventAggregator(element());
  out[name] = commands.map((command) => { fire(aggregator, command); return { command, state: aggregateState(aggregator) }; });
  aggregator.destroy();
}
fs.writeFileSync(new URL('../../tests/data/scene-camera-event-aggregator.json', import.meta.url), JSON.stringify(out) + '\n');
