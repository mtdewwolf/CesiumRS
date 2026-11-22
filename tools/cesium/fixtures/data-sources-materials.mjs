// DataSources property values and ownership observations from the pinned engine.
import * as C from "@cesium/engine";
import { enc, suite } from "./lib.mjs";

const epoch = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");
const time = (seconds) => C.JulianDate.addSeconds(epoch, seconds, new C.JulianDate());
const seconds = (date) => date && C.JulianDate.secondsDifference(date, epoch);
const color = new C.Color(0.1, 0.2, 0.3, 1);
const oddColor = new C.Color(0.8, 0.7, 0.6, 0.4);
const repeat = new C.Cartesian2(5, 3);

const options = {
  ImageMaterialProperty: { image: "textures/tile.png", repeat, color, transparent: true },
  GridMaterialProperty: { color, cellAlpha: 0.65, lineCount: repeat, lineThickness: new C.Cartesian2(2, 4), lineOffset: new C.Cartesian2(-2, 1) },
  StripeMaterialProperty: { orientation: C.StripeOrientation.VERTICAL, evenColor: color, oddColor, offset: -0.25, repeat: 8 },
  CheckerboardMaterialProperty: { evenColor: color, oddColor, repeat },
  PolylineArrowMaterialProperty: { color },
  PolylineDashMaterialProperty: { color, gapColor: oddColor, dashLength: 12, dashPattern: 4095 },
  PolylineGlowMaterialProperty: { color, glowPower: 0.4, taperPower: 0.7 },
  PolylineOutlineMaterialProperty: { color, outlineColor: oddColor, outlineWidth: 3 },
  NodeTransformationProperty: { translation: new C.Cartesian3(1, -2, 3), rotation: new C.Quaternion(0, 0, 1, 0), scale: new C.Cartesian3(3, 2, 1) },
};
const make = (name, fields) => new C[name](name === "PolylineArrowMaterialProperty" ? fields.color : fields);

function definition(name, fields) {
  const empty = make(name, {});
  const p = make(name, fields);
  const reused = { extra: 12 };
  const value = p.getValue(epoch, reused);
  return [p.getType?.(epoch), empty.getValue(epoch), p.isConstant, value, value === reused,
    p.equals(make(name, fields)), p.equals(empty), p.equals(new C.ColorMaterialProperty()),
    Object.entries(fields).map(([key, v]) => {
      const callback = new C.CallbackProperty((t) => C.JulianDate.equals(t, epoch) ? (v.clone?.() ?? v) : undefined, false);
      const varying = make(name, { ...fields, [key]: callback });
      return [key, varying.isConstant, varying.equals(p), varying.getValue(epoch), varying.getValue(time(1))];
    })];
}

// Scene fabric stores some colors/vectors as plain objects. Encode their components.
function uniforms(material) {
  return Object.fromEntries(Object.entries(material.uniforms).map(([k, v]) => {
    if (v && typeof v === "object" && "red" in v) return [k, [v.red, v.green, v.blue, v.alpha]];
    if (v && typeof v === "object" && "x" in v) return [k, [v.x, v.y]];
    return [k, v];
  }));
}
function resolve(name, fields) {
  const p = make(name, fields);
  const old = C.Material.fromType("Color");
  const material = C.MaterialProperty.getValue(epoch, p, old);
  const reused = C.MaterialProperty.getValue(time(1), p, material);
  const fallback = C.MaterialProperty.getValue(epoch, undefined, reused);
  return [material.type, uniforms(material), material === old, reused === material, fallback.type, uniforms(fallback)];
}
function descriptor() {
  const resource = new C.Resource({ url: "textures/water.png?v=2", headers: { Demo: "kept" } });
  const box = new C.BoxGraphics({ material: resource });
  const image = box.material.getValue(epoch).image;
  const line = new C.PolylineGraphics({ depthFailMaterial: "textures/fail.png", material: color });
  return [box.material.getType(epoch), image instanceof C.Resource, image !== resource, image.url, image.headers,
    line.depthFailMaterial.getType(epoch), line.depthFailMaterial.getValue(epoch), line.material.getType(epoch)];
}
function composite() {
  const p = new C.CompositeMaterialProperty();
  let events = 0;
  p.definitionChanged.addEventListener(() => ++events);
  const grid = new C.GridMaterialProperty();
  const add = (target, a, b, data) => target.intervals.addInterval(new C.TimeInterval({
    start: time(a), stop: time(b), isStopIncluded: false, data,
  }));
  const out = [p.isConstant];
  add(p, 0, 4, grid);
  add(p, 4, 8, new C.StripeMaterialProperty());
  add(p, 10, 12, grid);
  const copy = new C.CompositeMaterialProperty();
  add(copy, 0, 4, new C.GridMaterialProperty());
  add(copy, 4, 8, new C.StripeMaterialProperty());
  add(copy, 10, 12, new C.GridMaterialProperty());
  out.push(p.isConstant, p.equals(copy), grid.definitionChanged.numberOfListeners);
  grid.cellAlpha = 0.9;
  out.push(events, p.equals(copy), [-1, 0, 3, 4, 8, 10, 12].map(t => [p.getType(time(t)), p.getValue(time(t))]));
  p.intervals.removeAll();
  out.push(events, p.isConstant, grid.definitionChanged.numberOfListeners);
  return out;
}
function nodes(fields) {
  const source = new C.ModelGraphics({ nodeTransformations: { arm: fields, root: {} } });
  const copy = source.clone();
  const assigned = new C.ModelGraphics();
  source.clone(assigned);
  const target = new C.ModelGraphics({ nodeTransformations: { arm: {} } });
  target.merge(source);
  const before = enc(target.nodeTransformations.getValue(epoch));
  let events = 0;
  copy.definitionChanged.addEventListener(() => ++events);
  source.nodeTransformations.arm.translation = new C.Cartesian3(9, 8, 7);
  return [source.nodeTransformations.arm instanceof C.NodeTransformationProperty, before, copy.nodeTransformations !== source.nodeTransformations,
    assigned.nodeTransformations === source.nodeTransformations, copy.nodeTransformations.arm === source.nodeTransformations.arm,
    target.nodeTransformations.root === source.nodeTransformations.root, events, copy.nodeTransformations.getValue(epoch)];
}
function rotation(a, b) {
  const packed = [99, 0, 0];
  C.Rotation.pack(a, packed, 1);
  C.Rotation.pack(b, packed, 2);
  const converted = [];
  C.Rotation.convertPackedArrayForInterpolation(packed, 1, 2, converted);
  const p = new C.SampledProperty(C.Rotation);
  const other = new C.SampledProperty(C.Rotation);
  let events = 0;
  p.definitionChanged.addEventListener(() => ++events);
  const empty = p.isConstant;
  p.addSamples([time(0), time(4)], [a, b]);
  other.addSamples([time(0), time(4)], [a, b]);
  return [C.Rotation.packedLength, packed, C.Rotation.unpack(packed, 2), converted,
    C.Rotation.unpackInterpolationResult(converted, packed, 1, 2), empty, p.isConstant, events, p.equals(other),
    [-1, 0, 1, 2, 3, 4, 5].map(t => p.getValue(time(t)))];
}
function clock() {
  const p = new C.DataSourceClock();
  const trace = [];
  p.definitionChanged.addEventListener((_o, name) => trace.push(name));
  const start = time(-10);
  p.startTime = start;
  p.startTime = start;
  p.startTime = C.JulianDate.clone(start);
  p.stopTime = time(20);
  p.currentTime = time(4);
  p.clockRange = C.ClockRange.LOOP_STOP;
  p.clockStep = C.ClockStep.TICK_DEPENDENT;
  p.multiplier = -2;
  const clone = p.clone();
  const merged = new C.DataSourceClock();
  merged.multiplier = 3;
  merged.merge(p);
  const snap = c => [seconds(c.startTime), seconds(c.stopTime), seconds(c.currentTime), c.clockRange, c.clockStep, c.multiplier];
  const result = new C.Clock({ startTime: time(0), shouldAnimate: true });
  const full = merged.getValue(result);
  const fullState = snap(full);
  const sparse = new C.DataSourceClock();
  sparse.currentTime = time(8);
  sparse.getValue(result);
  sparse.clone(clone);
  return [trace, snap(p), p.equals(p.clone()), p.clone().startTime === p.startTime, merged.equals(p), snap(merged),
    full === result, fullState, snap(result), result.shouldAnimate, snap(clone), snap(p.getValue())];
}

function cases() {
  const s = suite("Materials");
  for (const [name, fields] of Object.entries(options)) {
    s.add("definition", [name, fields], definition);
    if (name !== "NodeTransformationProperty") s.add("resolve", [name, fields], resolve);
  }
  s.add("definition", ["ImageMaterialProperty", { ...options.ImageMaterialProperty, color: oddColor }], definition);
  s.add("reuseColor", [], () => {
    const m = C.Material.fromType("Color");
    C.MaterialProperty.getValue(epoch, new C.ColorMaterialProperty(oddColor), m);
    const before = uniforms(m);
    const fallback = C.MaterialProperty.getValue(epoch, new C.CompositeMaterialProperty(), m);
    return [before, fallback === m, uniforms(fallback)];
  });
  s.add("stripeOrientation", [], () => [C.StripeOrientation.HORIZONTAL, C.StripeOrientation.VERTICAL]);
  s.add("descriptor", [], descriptor);
  s.add("composite", [], composite);
  s.add("nodes", [options.NodeTransformationProperty], nodes);
  for (const angles of [[0, 6.1], [6.1, 0], [0, Math.PI], [-1, -0.5]]) s.add("rotation", angles, rotation);
  s.add("clock", [], clock);
  return [s];
}

export default function dataSourcesMaterials() {
  // Material's texture type checks reference DOM constructors in Node.
  const missing = ["HTMLCanvasElement", "HTMLImageElement", "ImageBitmap", "OffscreenCanvas", "HTMLVideoElement"]
    .filter(name => !(name in globalThis));
  try {
    for (const name of missing) globalThis[name] = class {};
    return cases();
  } finally {
    for (const name of missing) delete globalThis[name];
  }
}
