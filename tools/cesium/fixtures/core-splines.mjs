import {
  Cartesian3, ConstantSpline, LinearSpline, MorphWeightSpline,
  Quaternion, QuaternionSpline, Spline, SteppedSpline,
} from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

function timeSuite() {
  const s = suite("Spline");
  for (const times of [[0, 1], [-3, -1, 4], [1, 2, 3, 4, 5], [-10, -9.9, 0.5, 2, 100]]) {
    const start = times[0], end = times.at(-1), stretch = end - start;
    const queries = [...times];
    for (let i = 0; i < times.length - 1; i++) queries.push((times[i] + times[i + 1]) / 2);
    for (const time of queries) {
      for (let hint = 0; hint < times.length - 1; hint++) {
        s.add("findTimeInterval", [times, time, hint], (t, q, h) => Spline.prototype.findTimeInterval.call({times: t}, q, h));
      }
    }
    for (const time of [...queries, start - stretch, start - 2 * stretch, end + stretch,
      end + 2 * stretch, start - 0.01, end + 0.01, NaN, Infinity, -Infinity]) {
      s.add("wrapTime", [times, time], (t, q) => Spline.prototype.wrapTime.call({times: t}, q));
      s.add("clampTime", [times, time], (t, q) => Spline.prototype.clampTime.call({times: t}, q));
    }
  }
  return s;
}

function sample(spline, points, queries, outside) {
  return [spline.times, points, queries.map(t => spline.evaluate(t)),
    outside.map(t => spline.wrapTime(t)), outside.map(t => spline.clampTime(t)),
    queries.map(t => spline.findTimeInterval(t))];
}

export default function coreSplines() {
  const g = generators(makeRng(301));
  const constant = suite("ConstantSpline"), linear = suite("LinearSpline"), stepped = suite("SteppedSpline");
  const quaternion = suite("QuaternionSpline"), morph = suite("MorphWeightSpline");
  const constantQueries = [-100, 0, 1, 100, NaN, Infinity, -Infinity];
  for (const [kind, value] of [["scalar", -0], ["scalar", 7.5], ["cartesian", new Cartesian3(1, -2, 3)],
    ["quaternion", new Quaternion(0.2, -0.3, 0.4, -0.5)]]) {
    constant.add(kind, [value, constantQueries], (v, queries) => {
      const spline = new ConstantSpline(v);
      return [spline.value, queries.map(t => spline.evaluate(t)), queries.map(t => spline.wrapTime(t)), queries.map(t => spline.clampTime(t))];
    });
  }
  for (let caseIndex = 0; caseIndex < 24; caseIndex++) {
    const length = 2 + caseIndex % 6;
    const times = [g.uniform(-10, 10)];
    for (let i = 1; i < length; i++) times.push(times.at(-1) + g.uniform(0.05, 10));
    const start = times[0], end = times.at(-1), stretch = end - start;
    const queries = [...times, ...times.slice().reverse()];
    for (let i = 0; i < 8; i++) queries.push(g.uniform(start, end));
    const outside = [start, end, start - stretch, end + stretch, start - 2.5 * stretch, end + 2.5 * stretch];
    const scalar = times.map(() => g.num(100));
    const cartesian = times.map(() => g.c3(1e6));
    const quaternions = times.map(() => g.quat());
    // Include antipodal endpoints and repeated rotations.
    if (caseIndex % 4 === 0) quaternions[1] = Quaternion.negate(quaternions[0], new Quaternion());
    if (caseIndex % 4 === 1) quaternions[1] = Quaternion.clone(quaternions[0]);
    for (const [kind, points] of [["scalar", scalar], ["cartesian", cartesian]]) {
      linear.add(kind, [times, points, queries, outside], (t, p, q, o) => sample(new LinearSpline({times: t, points: p}), p, q, o));
    }
    for (const [kind, points] of [["scalar", scalar], ["cartesian", cartesian], ["quaternion", quaternions]]) {
      stepped.add(kind, [times, points, queries, outside], (t, p, q, o) => sample(new SteppedSpline({times: t, points: p}), p, q, o));
    }
    quaternion.add("quaternion", [times, quaternions, queries, outside], (t, p, q, o) => sample(new QuaternionSpline({times: t, points: p}), p, q, o));
    for (const count of [1, 2, 5]) {
      if (times.length * count < 3) continue; // Cesium debug-build precondition.
      const weights = Array.from({length: times.length * count}, () => g.uniform(-1, 2));
      morph.add("weights", [times, weights, queries, outside], (t, w, q, o) => {
        const spline = new MorphWeightSpline({times: t, weights: w});
        return sample(spline, spline.weights, q, o);
      });
    }
  }
  return [timeSuite(), constant, linear, stepped, quaternion, morph];
}
