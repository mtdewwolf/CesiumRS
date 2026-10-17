// Shared helpers for fixture generators: a seeded RNG, input generators,
// and an encoder that turns Cesium values into plain JSON the Rust tests read.

import * as Cesium from "@cesium/engine";

const {
  BoundingRectangle,
  BoundingSphere,
  Cartesian2,
  Cartesian3,
  Cartesian4,
  Cartographic,
  Color,
  HeadingPitchRoll,
  Interval,
  Matrix2,
  Matrix3,
  Matrix4,
  Plane,
  Quaternion,
  Rectangle,
  Spherical,
} = Cesium;

/** mulberry32: small, fast, and identical on every platform. */
export function makeRng(seed) {
  let a = seed >>> 0;
  return function rng() {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function generators(rng) {
  const uniform = (lo, hi) => lo + (hi - lo) * rng();
  const pick = (list) => list[Math.floor(rng() * list.length)];
  const num = (range = 1000) => {
    const r = rng();
    if (r < 0.7) return uniform(-range, range);
    if (r < 0.8) return uniform(-1e-6, 1e-6);
    if (r < 0.9) return Math.round(uniform(-20, 20));
    return pick([0, 1, -1, 0.5, -0.5, 2, 1e-15]);
  };
  const angle = () => (rng() < 0.85 ? uniform(-2 * Math.PI, 2 * Math.PI) : pick([0, Math.PI, -Math.PI, Math.PI / 2, -Math.PI / 2, 2 * Math.PI]));
  const c2 = (range) => new Cartesian2(num(range), num(range));
  const c3 = (range) => new Cartesian3(num(range), num(range), num(range));
  const c4 = (range) => new Cartesian4(num(range), num(range), num(range), num(range));
  const nonZero3 = (range) => {
    let v;
    do v = c3(range);
    while (Cartesian3.magnitude(v) < 1e-3);
    return v;
  };
  const unit3 = () => Cartesian3.normalize(nonZero3(1), new Cartesian3());
  const quat = () => {
    const q = new Quaternion(uniform(-1, 1), uniform(-1, 1), uniform(-1, 1), uniform(-1, 1));
    return Quaternion.normalize(q, q);
  };
  const m2 = (range) => Matrix2.fromColumnMajorArray([num(range), num(range), num(range), num(range)]);
  const m3 = (range) => Matrix3.fromColumnMajorArray(Array.from({ length: 9 }, () => num(range)));
  const m4 = (range) => Matrix4.fromColumnMajorArray(Array.from({ length: 16 }, () => num(range)));
  const rotation3 = () => Matrix3.fromQuaternion(quat());
  const affine4 = () =>
    Matrix4.fromTranslationQuaternionRotationScale(
      c3(1e6),
      quat(),
      new Cartesian3(uniform(0.1, 10), uniform(0.1, 10), uniform(0.1, 10)),
    );
  const rigid4 = () => Matrix4.fromRotationTranslation(rotation3(), c3(1e6));
  const hpr = () => new HeadingPitchRoll(angle(), uniform(-Math.PI / 2, Math.PI / 2), angle());
  const lonLat = () => new Cartographic(uniform(-Math.PI, Math.PI), uniform(-Math.PI / 2, Math.PI / 2), uniform(-1000, 1e5));
  const surface = () => Cartesian3.fromRadians(uniform(-Math.PI, Math.PI), uniform(-1.5, 1.5), uniform(-5000, 2e6));
  const rectangle = () => {
    const south = uniform(-Math.PI / 2, Math.PI / 2);
    const north = uniform(south, Math.PI / 2);
    return new Rectangle(uniform(-Math.PI, Math.PI), south, uniform(-Math.PI, Math.PI), north);
  };
  const color = () => new Color(rng(), rng(), rng(), rng());
  const sphere = () => new BoundingSphere(c3(1e6), uniform(0, 1e5));
  const plane = () => new Plane(unit3(), num(1e5));
  return {
    rng,
    uniform,
    pick,
    num,
    angle,
    c2,
    c3,
    c4,
    nonZero3,
    unit3,
    quat,
    m2,
    m3,
    m4,
    rotation3,
    affine4,
    rigid4,
    hpr,
    lonLat,
    surface,
    rectangle,
    color,
    sphere,
    plane,
  };
}

/** Plain JSON for a Cesium value. Non-finite numbers become strings. */
export function enc(v) {
  if (v === undefined || v === null) return null;
  if (typeof v === "number") {
    if (Number.isNaN(v)) return "NaN";
    if (v === Infinity) return "Infinity";
    if (v === -Infinity) return "-Infinity";
    // JSON has no negative zero; keep it, since 1 / -0 is -Infinity.
    if (Object.is(v, -0)) return "-0";
    return v;
  }
  if (typeof v === "boolean" || typeof v === "string") return v;
  if (v instanceof Matrix2 || v instanceof Matrix3 || v instanceof Matrix4) return Array.from(v).map(enc);
  if (v instanceof Quaternion || v instanceof Cartesian4) return [v.x, v.y, v.z, v.w].map(enc);
  if (v instanceof Cartesian3) return [v.x, v.y, v.z].map(enc);
  if (v instanceof Cartesian2) return [v.x, v.y].map(enc);
  if (v instanceof Cartographic) return [v.longitude, v.latitude, v.height].map(enc);
  if (v instanceof HeadingPitchRoll) return [v.heading, v.pitch, v.roll].map(enc);
  if (v instanceof Spherical) return [v.clock, v.cone, v.magnitude].map(enc);
  if (v instanceof Color) return [v.red, v.green, v.blue, v.alpha].map(enc);
  if (v instanceof Rectangle) return [v.west, v.south, v.east, v.north].map(enc);
  if (v instanceof BoundingSphere) return [v.center.x, v.center.y, v.center.z, v.radius].map(enc);
  if (v instanceof Plane) return [v.normal.x, v.normal.y, v.normal.z, v.distance].map(enc);
  if (v instanceof Interval) return [v.start, v.stop].map(enc);
  if (v instanceof BoundingRectangle) return [v.x, v.y, v.width, v.height].map(enc);
  if (Array.isArray(v) || ArrayBuffer.isView(v)) return Array.from(v).map(enc);
  if (typeof v === "object") {
    const out = {};
    for (const [k, val] of Object.entries(v)) out[k] = enc(val);
    return out;
  }
  throw new Error(`cannot encode ${v}`);
}

/**
 * Collects cases for one module. `add(name, args, fn)` calls `fn(...args)`
 * and records the encoded inputs and output. A throw is recorded as
 * `{ "throws": message, "kind": error name }`. `DeveloperError`s are
 * debug-build precondition checks; `RuntimeError`s are real behavior the
 * Rust port must reproduce as an `Err`.
 */
export function suite(module) {
  const cases = [];
  return {
    module,
    cases,
    add(f, args, fn) {
      let r;
      try {
        r = enc(fn(...args));
      } catch (e) {
        r = { throws: String(e.message ?? e), kind: e.name ?? "Error" };
      }
      cases.push({ f, a: args.map(enc), r });
    },
    /** Runs `add` `count` times with fresh arguments from `makeArgs`. */
    repeat(f, count, makeArgs, fn) {
      for (let i = 0; i < count; i++) this.add(f, makeArgs(), fn);
    },
  };
}
