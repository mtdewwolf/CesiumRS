import * as C from "@cesium/engine";
import { suite, enc } from "./lib.mjs";
const epoch = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");
const date = (t) => C.JulianDate.addSeconds(epoch, t, new C.JulianDate());
const seconds = (t) => C.JulianDate.secondsDifference(t, epoch);
const vec = (v) => v === null ? void 0 : new C.Cartesian3(...v);
const xyz = [6378137, 200, 300];
const times = [-1 / 60, 0, 0.1, 0.5, 1, 1.999, 2, 2 + 1 / 60];
const xysSamples = [1e-3, 2e-3, 1e-4, 11e-4, 22e-4, 12e-5, 12e-4, 24e-4, 14e-5, 13e-4, 26e-4, 16e-5];
function body(mode, fn) {
  const saved = [C.Transforms.computeIcrfToFixedMatrix, C.Transforms.computeIcrfToCentralBodyFixedMatrix, C.Transforms.iau2006XysData, C.Transforms.earthOrientationParameters, C.Ellipsoid.default];
  try {
    if (mode === "moon") C.Transforms.computeIcrfToCentralBodyFixedMatrix = C.Transforms.computeIcrfToMoonFixedMatrix;
    else if (mode === "iau") {
      const data = new C.Iau2006XysData({ interpolationOrder: 1, sampleZeroJulianEphemerisDate: 24588485e-1, stepSizeDays: 1, samplesPerXysFile: 4, totalSamples: 4 });
      data._samples = xysSamples.slice();
      C.Transforms.iau2006XysData = data;
      C.Transforms.earthOrientationParameters = C.EarthOrientationParameters.NONE;
    } else C.Transforms.computeIcrfToFixedMatrix = () => void 0;
    return fn();
  } finally {
    [C.Transforms.computeIcrfToFixedMatrix, C.Transforms.computeIcrfToCentralBodyFixedMatrix, C.Transforms.iau2006XysData, C.Transforms.earthOrientationParameters, C.Ellipsoid.default] = saved;
  }
}
function snapshot(p, t) {
  return [enc(p.getValue(date(t))), ...[0, 1].map((frame) => enc(p.getValueInReferenceFrame(date(t), frame)))];
}
function child(kind, frame = 0) {
  if (kind === "absent") return void 0;
  if (kind === "constant") return new C.ConstantPositionProperty(new C.Cartesian3(...xyz), frame);
  if (kind === "sampled") {
    const p = new C.SampledPositionProperty(frame);
    p.addSamples([date(0), date(1), date(2)], [[6378137, 0, 0], [6378138, 3, 1], [6378140, 4, 2]].map(vec));
    return p;
  }
  return new C.CallbackPositionProperty((t) => {
    const x = seconds(t);
    if (kind === "bounded" && (x < 0 || x > 2) || kind === "missing" && x === 0 || kind === "isolated" && x !== 0) return void 0;
    return new C.Cartesian3(6378137 + (kind === "stationary" ? 0 : 120 * x), 200 + (kind === "stationary" ? 0 : 30 * x), 300 + (kind === "stationary" ? 0 : 5 * x));
  }, false, frame);
}
function dataSourcesPositions() {
  const all = [];
  const convert = suite("PositionProperty");
  for (const mode of ["teme", "moon", "iau"]) for (const input of [0, 1]) for (const output of [0, 1]) for (const t of [-60, 0, 123.5, 86400]) for (const value of [null, [0, 0, 0], xyz]) convert.add("convert", [mode, input, output, t, value], (mode2, input2, output2, t2, value2) => body(mode2, () => {
    const r = new C.Cartesian3(7, 8, 9);
    return [enc(C.PositionProperty.convertToReferenceFrame(date(t2), vec(value2), input2, output2, r)), enc(r)];
  }));
  all.push(convert);
  const constant = suite("ConstantPositionProperty");
  for (const mode of ["teme", "moon", "iau"]) for (const frame of [0, 1]) constant.add("sequence", [mode, frame, [[-60], [0], [10]]], (mode2, frame2, queries) => body(mode2, () => {
    const p = new C.ConstantPositionProperty(void 0, frame2);
    let changes = 0;
    p.definitionChanged.addEventListener(() => changes++);
    return [[null, null], [xyz, null], [xyz, null], [xyz, 1], [[0, 0, 0], 0], [null, 0], [xyz, 1]].map(([value, nextFrame]) => {
      p.setValue(vec(value), nextFrame === null ? void 0 : nextFrame);
      return [p.isConstant, p.referenceFrame, changes, queries.map(([t]) => snapshot(p, t)), p.equals(new C.ConstantPositionProperty(vec(value), p.referenceFrame))];
    });
  }));
  all.push(constant);
  const callback = suite("CallbackPositionProperty");
  for (const mode of ["teme", "moon", "iau"]) for (const frame of [0, 1]) for (const flag of [false, true]) callback.add("evaluate", [mode, frame, flag, times], (mode2, frame2, flag2, queries) => body(mode2, () => {
    let calls = 0, changes = 0;
    const fn = (t, r2) => {
      calls++;
      const x = seconds(t);
      if (x < 0) return void 0;
      r2 = r2 ?? new C.Cartesian3();
      r2.x = 6378137 + 120 * x;
      r2.y = 200 + 30 * x;
      r2.z = 300 + 5 * x;
      return r2;
    };
    const p = new C.CallbackPositionProperty(fn, flag2, frame2);
    p.definitionChanged.addEventListener(() => changes++);
    const r = new C.Cartesian3(7, 8, 9);
    const values = queries.map((t) => [enc(p.getValue(date(t), r)), enc(r)]);
    const same = p.equals(new C.CallbackPositionProperty(fn, flag2, frame2));
    const different = p.equals(new C.CallbackPositionProperty(fn, flag2, 1 - frame2));
    p.setCallback(fn, flag2);
    p.setCallback(fn, !flag2);
    p.setCallback(fn, !flag2);
    return [values, calls, changes, p.isConstant, p.referenceFrame, same, different];
  }));
  all.push(callback);
  const sampled = suite("SampledPositionProperty");
  for (const mode of ["teme", "moon", "iau"]) for (const frame of [0, 1]) for (const derivatives of [0, 1, 2]) for (const algorithm of ["LinearApproximation", "LagrangePolynomialApproximation", "HermitePolynomialApproximation"]) sampled.add("samples", [mode, frame, derivatives, algorithm, times], (mode2, frame2, derivatives2, algorithm2, queries) => body(mode2, () => {
    const p = new C.SampledPositionProperty(frame2, derivatives2);
    let changes = 0;
    p.definitionChanged.addEventListener(() => changes++);
    const ts = [2, 0, 1, 1];
    const values = ts.map((t, i) => new C.Cartesian3(6378137 + t * t + i, 200 + 3 * t, 300 + t));
    const d = ts.map((t) => Array.from({ length: derivatives2 }, (_, i) => new C.Cartesian3(i === 0 ? 2 * t : 2, i === 0 ? 3 : 0, i === 0 ? 1 : 0)));
    p.setInterpolationOptions({ interpolationAlgorithm: C[algorithm2], interpolationDegree: 3 });
    p.addSamples(ts.map(date), values, derivatives2 ? d : void 0);
    const initial = [p.isConstant, p.referenceFrame, p.numberOfDerivatives, p.interpolationDegree, p.interpolationAlgorithm.type, changes, queries.map((t) => snapshot(p, t))];
    p.forwardExtrapolationType = C.ExtrapolationType.HOLD;
    p.backwardExtrapolationType = C.ExtrapolationType.EXTRAPOLATE;
    p.backwardExtrapolationDuration = 0.02;
    p.forwardExtrapolationDuration = 0.02;
    const extrap = queries.map((t) => snapshot(p, t));
    const packed = [C.JulianDate.toIso8601(date(3)), 6378140, 210, 310, ...Array(derivatives2).fill([1, 2, 3]).flat()];
    p.addSamplesPackedArray(packed);
    const extra = snapshot(p, 3);
    const removed = [p.removeSample(date(0.25)), p.removeSample(date(1))];
    p.removeSamples(new C.TimeInterval({ start: date(2), stop: date(3), isStopIncluded: false }));
    return [initial, extrap, extra, removed, changes, queries.map((t) => snapshot(p, t))];
  }));
  all.push(sampled);
  for (const composite of [false, true]) {
    const s = suite(composite ? "CompositePositionProperty" : "TimeIntervalCollectionPositionProperty");
    for (const mode of ["teme", "moon", "iau"]) for (const frame of [0, 1]) s.add("intervals", [mode, frame, composite, times], (mode2, frame2, composite2, queries) => body(mode2, () => {
      const p = composite2 ? new C.CompositePositionProperty(frame2) : new C.TimeIntervalCollectionPositionProperty(frame2);
      let changes = 0;
      p.definitionChanged.addEventListener(() => changes++);
      const data = [0, 1].map((f) => composite2 ? new C.ConstantPositionProperty(new C.Cartesian3(...xyz), f) : new C.Cartesian3(...xyz));
      const out = [];
      const capture = () => [p.isConstant, p.referenceFrame, changes, p.intervals.length, queries.map((t) => snapshot(p, t)), composite2 ? data.map((p2) => p2.definitionChanged.numberOfListeners) : null];
      out.push(capture());
      p.intervals.addInterval(new C.TimeInterval({ start: date(0), stop: date(1), isStopIncluded: false, data: data[0] }));
      out.push(capture());
      p.intervals.addInterval(new C.TimeInterval({ start: date(1), stop: date(2), data: data[1] }));
      out.push(capture());
      if (composite2) {
        data[0].setValue(new C.Cartesian3(1, 2, 3));
        p.referenceFrame = 1 - frame2;
      }
      out.push(capture());
      p.intervals.removeInterval(new C.TimeInterval({ start: date(0.5), stop: date(1), isStopIncluded: false }));
      out.push(capture());
      p.intervals.removeAll();
      out.push(capture());
      return out;
    }));
    for (const frame of [0, 1]) for (const equal of [false, true]) s.add("equals", [frame, equal, composite], (frame2, equal2, composite2) => body("teme", () => {
      const a = composite2 ? new C.CompositePositionProperty(frame2) : new C.TimeIntervalCollectionPositionProperty(frame2), b = composite2 ? new C.CompositePositionProperty(frame2) : new C.TimeIntervalCollectionPositionProperty(frame2);
      for (const [p, value] of [[a, xyz], [b, equal2 ? xyz : [1, 2, 3]]]) p.intervals.addInterval(new C.TimeInterval({ start: date(0), stop: date(1), data: composite2 ? new C.ConstantPositionProperty(vec(value), frame2) : vec(value) }));
      return a.equals(b);
    }));
    all.push(s);
  }
  const array = suite("PositionPropertyArray");
  for (const mode of ["teme", "moon", "iau"]) for (const frame of [0, 1]) for (const empty of [false, true]) array.add("array", [mode, frame, empty, times], (mode2, frame2, empty2, queries) => body(mode2, () => {
    const a = new C.ConstantPositionProperty(void 0, 0), b = new C.ConstantPositionProperty(vec(xyz), 1);
    const children = empty2 ? [] : [a, b, a];
    const p = new C.PositionPropertyArray(children, frame2);
    let changes = 0;
    p.definitionChanged.addEventListener(() => changes++);
    const initial = [p.isConstant, p.referenceFrame, queries.map((t) => snapshot(p, t)), [a, b].map((p2) => p2.definitionChanged.numberOfListeners)];
    a.setValue(new C.Cartesian3(1, 2, 3));
    const after = [queries.map((t) => snapshot(p, t)), changes];
    const other = new C.PositionPropertyArray(children, frame2);
    const eq = p.equals(other);
    other.setValue(void 0);
    p.setValue(void 0);
    return [initial, after, eq, p.isConstant, enc(p.getValue(date(0))), changes, [a, b].map((p2) => p2.definitionChanged.numberOfListeners)];
  }));
  all.push(array);
  const scaled = suite("ScaledPositionProperty");
  for (const mode of ["teme", "moon", "iau"]) for (const frame of [0, 1]) for (const radii of [[6378137, 6378137, 6356752314245179e-9], [1, 1, 1], [3, 2, 1]]) scaled.add("scaled", [mode, frame, radii, times], (mode2, frame2, radii2, queries) => body(mode2, () => {
    C.Ellipsoid.default = new C.Ellipsoid(...radii2);
    const child2 = new C.ConstantPositionProperty(vec(xyz), frame2);
    const p = new C.ScaledPositionProperty(child2);
    let changes = 0;
    p.definitionChanged.addEventListener(() => changes++);
    const initial = [p.isConstant, p.referenceFrame, queries.map((t) => snapshot(p, t)), p.equals(new C.ScaledPositionProperty(child2))];
    child2.setValue(C.Cartesian3.ZERO);
    const r = new C.Cartesian3(7, 8, 9);
    const zero = [enc(p.getValue(date(0), r)), enc(r), changes];
    p.setValue(child2);
    p.setValue(void 0);
    return [initial, zero, p.isConstant, p.referenceFrame, enc(p.getValue(date(0))), changes];
  }));
  all.push(scaled);
  for (const orientation of [false, true]) {
    const s = suite(orientation ? "VelocityOrientationProperty" : "VelocityVectorProperty");
    for (const mode of ["teme", "moon", "iau"]) for (const kind of ["absent", "constant", "linear", "stationary", "bounded", "missing", "isolated", "sampled"]) for (const frame of [0, 1]) for (const normalize of orientation ? [true] : [false, true]) s.add("velocity", [mode, kind, frame, normalize, times], (mode2, kind2, frame2, normalize2, queries) => body(mode2, () => {
      const position = child(kind2, frame2);
      const p = orientation ? new C.VelocityOrientationProperty(position) : new C.VelocityVectorProperty(position, normalize2);
      let changes = 0;
      p.definitionChanged.addEventListener(() => changes++);
      const values = queries.map((t) => enc(p.getValue(date(t))));
      const eq = orientation ? p.equals(new C.VelocityOrientationProperty(position, new C.Ellipsoid(...[6378137, 6378137, 6356752314245179e-9]))) : p.equals(new C.VelocityVectorProperty(position, !normalize2));
      const initial = [p.isConstant, values, eq];
      if (orientation) {
        const old = p.ellipsoid;
        p.ellipsoid = old;
        p.ellipsoid = new C.Ellipsoid(1, 1, 1);
        p.ellipsoid = p.ellipsoid;
      } else {
        p.normalize = normalize2;
        p.normalize = !normalize2;
        p.normalize = !normalize2;
      }
      const changed = [changes, queries.map((t) => enc(p.getValue(date(t))))];
      p.position = void 0;
      p.position = void 0;
      return [initial, changed, changes, p.isConstant, enc(p.getValue(date(0)))];
    }));
    s.add("scratch", [], () => body("teme", () => {
      const trace = [];
      const position = new C.CallbackPositionProperty((time, result) => {
        const t = seconds(time);
        trace.push([t, result !== void 0]);
        if (t > 0 || result === void 0) return void 0;
        result.x = 6378137 + 120 * t;
        result.y = 200 + 30 * t;
        result.z = 300 + 5 * t;
        return result;
      }, false);
      const p = orientation ? new C.VelocityOrientationProperty(position) : new C.VelocityVectorProperty(position, false);
      const values = [0, -1 / 60, -1].map((t) => enc(p.getValue(date(t))));
      return [values, trace];
    }));
    all.push(s);
  }
  return all;
}
export {
  dataSourcesPositions as default
};
