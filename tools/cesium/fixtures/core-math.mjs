// Fixtures for Math, Cartesian2/3/4, Matrix2/3/4, Quaternion,
// HeadingPitchRoll, and Spherical.

import {
  Cartesian2,
  Cartesian3,
  Cartesian4,
  HeadingPitchRoll,
  Math as CesiumMath,
  Matrix2,
  Matrix3,
  Matrix4,
  Quaternion,
  Spherical,
  BoundingRectangle,
  Ellipsoid,
} from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

const N = 25;

function mathSuite() {
  const g = generators(makeRng(1));
  const s = suite("Math");
  const nums = (k, range) => () => Array.from({ length: k }, () => g.num(range));
  s.repeat("sign", N, nums(1), CesiumMath.sign);
  s.repeat("signNotZero", N, nums(1), CesiumMath.signNotZero);
  s.repeat("toSNorm", N, () => [g.uniform(-1.5, 1.5), g.pick([255, 32767, 1023])], CesiumMath.toSNorm);
  for (const v of [-1, -0.5, 0, 0.5, 1, 0.501960784, 0.498039215]) s.add("toSNorm", [v, 255], CesiumMath.toSNorm);
  s.repeat("fromSNorm", N, () => [g.uniform(-10, 300), g.pick([255, 32767])], CesiumMath.fromSNorm);
  s.repeat("normalize", N, () => [g.num(10), g.num(10), g.num(10)], CesiumMath.normalize);
  s.repeat("lerp", N, nums(3, 100), CesiumMath.lerp);
  s.repeat("toRadians", N, nums(1, 720), CesiumMath.toRadians);
  s.repeat("toDegrees", N, nums(1, 10), CesiumMath.toDegrees);
  s.repeat("convertLongitudeRange", N * 2, () => [g.num(20)], CesiumMath.convertLongitudeRange);
  for (const v of [Math.PI, -Math.PI, 3 * Math.PI, -3 * Math.PI, 2 * Math.PI]) s.add("convertLongitudeRange", [v], CesiumMath.convertLongitudeRange);
  s.repeat("clampToLatitudeRange", N, () => [g.num(3)], CesiumMath.clampToLatitudeRange);
  s.repeat("negativePiToPi", N * 2, () => [g.num(20)], CesiumMath.negativePiToPi);
  s.repeat("zeroToTwoPi", N * 2, () => [g.num(20)], CesiumMath.zeroToTwoPi);
  for (const v of [2 * Math.PI, 4 * Math.PI, -2 * Math.PI, -Math.PI, 0]) {
    s.add("zeroToTwoPi", [v], CesiumMath.zeroToTwoPi);
    s.add("negativePiToPi", [v], CesiumMath.negativePiToPi);
  }
  s.repeat("mod", N * 2, () => [g.num(50), g.pick([1, -1, 3, -3, 2 * Math.PI, 0.7])], CesiumMath.mod);
  s.repeat("equalsEpsilon", N * 2, () => {
    const a = g.num(10);
    return [a, a + g.uniform(-1e-3, 1e-3), g.pick([0, 1e-6, 1e-3, 1e-2]), g.pick([undefined, 0, 1e-4])];
  }, CesiumMath.equalsEpsilon);
  for (const f of ["lessThan", "lessThanOrEquals", "greaterThan", "greaterThanOrEquals"]) {
    s.repeat(f, N, () => {
      const a = g.num(10);
      return [a, a + g.uniform(-0.01, 0.01), g.pick([0.001, 0.005, 0.0])];
    }, CesiumMath[f]);
  }
  for (let n = 0; n <= 30; n++) s.add("factorial", [n], CesiumMath.factorial);
  s.repeat("incrementWrap", N, () => [Math.round(g.uniform(-5, 15)), 10, g.pick([0, 2, -3])], CesiumMath.incrementWrap);
  for (const n of [0, 1, 2, 3, 4, 5, 6, 7, 8, 255, 256, 1023, 1024, 65536, 2147483647, 2147483648, 4294967295]) {
    s.add("isPowerOfTwo", [n], CesiumMath.isPowerOfTwo);
    s.add("previousPowerOfTwo", [n], CesiumMath.previousPowerOfTwo);
    if (n <= 2147483648) s.add("nextPowerOfTwo", [n], CesiumMath.nextPowerOfTwo);
  }
  s.repeat("clamp", N, () => {
    const lo = g.num(10);
    return [g.num(20), lo, lo + Math.abs(g.num(10))];
  }, CesiumMath.clamp);
  s.repeat("acosClamped", N, () => [g.uniform(-1.5, 1.5)], CesiumMath.acosClamped);
  s.repeat("asinClamped", N, () => [g.uniform(-1.5, 1.5)], CesiumMath.asinClamped);
  s.repeat("chordLength", N, () => [g.angle(), g.uniform(0, 1e7)], CesiumMath.chordLength);
  s.repeat("logBase", N, () => [g.uniform(0.001, 1e6), g.pick([2, 10, Math.E, 3])], CesiumMath.logBase);
  s.repeat("cbrt", N, nums(1, 1e6), CesiumMath.cbrt);
  s.repeat("log2", N, () => [g.uniform(1e-6, 1e9)], CesiumMath.log2);
  s.repeat("fog", N, () => [g.uniform(0, 1e5), g.uniform(0, 1e-3)], CesiumMath.fog);
  s.repeat("fastApproximateAtan", N, () => [g.uniform(-1, 1)], CesiumMath.fastApproximateAtan);
  s.repeat("fastApproximateAtan2", N * 2, () => [g.num(10), g.num(10)], CesiumMath.fastApproximateAtan2);
  // Seeded random sequence.
  for (const seed of [0, 1, 42, 123456789, 4294967295]) {
    s.add("randomSequence", [seed], (seedValue) => {
      CesiumMath.setRandomNumberSeed(seedValue);
      const out = [];
      for (let i = 0; i < 700; i++) out.push(CesiumMath.nextRandomNumber());
      return out.filter((_, i) => i < 5 || i % 97 === 0 || i > 620);
    });
  }
  return s;
}

function cartesianSuite(name, Type, gen, dims) {
  const g = generators(makeRng(10 + dims));
  const s = suite(name);
  const v = () => gen(g);
  const r = () => new Type();
  s.repeat("magnitude", N, () => [v()], (a) => Type.magnitude(a));
  s.repeat("magnitudeSquared", N, () => [v()], (a) => Type.magnitudeSquared(a));
  s.repeat("distance", N, () => [v(), v()], (a, b) => Type.distance(a, b));
  s.repeat("distanceSquared", N, () => [v(), v()], (a, b) => Type.distanceSquared(a, b));
  s.repeat("normalize", N, () => [v()], (a) => Type.normalize(a, r()));
  s.add("normalize", [Type.ZERO], (a) => Type.normalize(a, r()));
  s.repeat("dot", N, () => [v(), v()], (a, b) => Type.dot(a, b));
  s.repeat("multiplyComponents", N, () => [v(), v()], (a, b) => Type.multiplyComponents(a, b, r()));
  s.repeat("divideComponents", N, () => [v(), v()], (a, b) => Type.divideComponents(a, b, r()));
  s.repeat("add", N, () => [v(), v()], (a, b) => Type.add(a, b, r()));
  s.repeat("subtract", N, () => [v(), v()], (a, b) => Type.subtract(a, b, r()));
  s.repeat("multiplyByScalar", N, () => [v(), g.num(100)], (a, k) => Type.multiplyByScalar(a, k, r()));
  s.repeat("divideByScalar", N, () => [v(), g.num(100)], (a, k) => Type.divideByScalar(a, k, r()));
  s.repeat("negate", N, () => [v()], (a) => Type.negate(a, r()));
  s.repeat("abs", N, () => [v()], (a) => Type.abs(a, r()));
  s.repeat("lerp", N, () => [v(), v(), g.uniform(-0.5, 1.5)], (a, b, t) => Type.lerp(a, b, t, r()));
  s.repeat("maximumComponent", N, () => [v()], (a) => Type.maximumComponent(a));
  s.repeat("minimumComponent", N, () => [v()], (a) => Type.minimumComponent(a));
  s.repeat("minimumByComponent", N, () => [v(), v()], (a, b) => Type.minimumByComponent(a, b, r()));
  s.repeat("maximumByComponent", N, () => [v(), v()], (a, b) => Type.maximumByComponent(a, b, r()));
  s.repeat("clamp", N, () => {
    const lo = v();
    const hi = Type.add(lo, Type.abs(v(), r()), r());
    return [v(), lo, hi];
  }, (a, lo, hi) => Type.clamp(a, lo, hi, r()));
  s.repeat("mostOrthogonalAxis", N, () => [v()], (a) => Type.mostOrthogonalAxis(a, r()));
  s.repeat("equalsEpsilon", N, () => {
    const a = v();
    const b = Type.add(a, Type.multiplyByScalar(v(), 1e-5, r()), r());
    return [a, b, g.pick([0, 1e-6, 1e-3]), g.pick([undefined, 1e-4])];
  }, (a, b, rel, abs) => Type.equalsEpsilon(a, b, rel, abs));
  if (Type.angleBetween) s.repeat("angleBetween", N, () => [v(), v()], (a, b) => Type.angleBetween(a, b));
  return s;
}

function cartesian2Suite() {
  const s = cartesianSuite("Cartesian2", Cartesian2, (g) => g.c2(1000), 2);
  const g = generators(makeRng(99));
  s.repeat("cross", N, () => [g.c2(100), g.c2(100)], (a, b) => Cartesian2.cross(a, b));
  return s;
}

function cartesian3Suite() {
  const s = cartesianSuite("Cartesian3", Cartesian3, (g) => g.c3(1000), 3);
  const g = generators(makeRng(98));
  s.repeat("cross", N, () => [g.c3(100), g.c3(100)], (a, b) => Cartesian3.cross(a, b, new Cartesian3()));
  s.repeat("midpoint", N, () => [g.c3(100), g.c3(100)], (a, b) => Cartesian3.midpoint(a, b, new Cartesian3()));
  s.repeat("projectVector", N, () => [g.c3(100), g.nonZero3(100)], (a, b) => Cartesian3.projectVector(a, b, new Cartesian3()));
  s.repeat("fromSpherical", N, () => [new Spherical(g.angle(), g.angle(), g.uniform(0, 100))], (sp) => Cartesian3.fromSpherical(sp));
  s.repeat("fromDegrees", N, () => [g.uniform(-360, 360), g.uniform(-90, 90), g.num(1e5)], (lon, lat, h) => Cartesian3.fromDegrees(lon, lat, h));
  s.repeat("fromRadians", N, () => [g.angle(), g.uniform(-2, 2), g.num(1e5)], (lon, lat, h) => Cartesian3.fromRadians(lon, lat, h));
  s.repeat("fromRadiansMoon", N, () => [g.angle(), g.uniform(-2, 2), g.num(1e5)], (lon, lat, h) => Cartesian3.fromRadians(lon, lat, h, Ellipsoid.MOON));
  s.repeat("fromDegreesArrayHeights", 5, () => [Array.from({ length: 9 }, (_, i) => (i % 3 === 2 ? g.num(1e4) : g.uniform(-90, 90)))], (a) => Cartesian3.fromDegreesArrayHeights(a));
  s.repeat("fromDegreesArray", 5, () => [Array.from({ length: 8 }, () => g.uniform(-90, 90))], (a) => Cartesian3.fromDegreesArray(a));
  return s;
}

function cartesian4Suite() {
  const s = cartesianSuite("Cartesian4", Cartesian4, (g) => g.c4(1000), 4);
  const g = generators(makeRng(97));
  s.repeat("packFloat", N, () => [g.num(1e5)], (v) => Cartesian4.packFloat(v, new Cartesian4()));
  for (const v of [0, 1, -1, 0.1, 3.4e38, 1e-40, Infinity, -Infinity, NaN]) s.add("packFloat", [v], (x) => Cartesian4.packFloat(x, new Cartesian4()));
  s.repeat("unpackFloat", N, () => [new Cartesian4(Math.floor(g.uniform(0, 256)), Math.floor(g.uniform(0, 256)), Math.floor(g.uniform(0, 256)), Math.floor(g.uniform(0, 128)))], (c) => Cartesian4.unpackFloat(c));
  return s;
}

function matrixCommon(s, g, Type, gen, Vec, vec) {
  const r = () => new Type();
  s.repeat("multiply", N, () => [gen(), gen()], (a, b) => Type.multiply(a, b, r()));
  s.repeat("add", N, () => [gen(), gen()], (a, b) => Type.add(a, b, r()));
  s.repeat("subtract", N, () => [gen(), gen()], (a, b) => Type.subtract(a, b, r()));
  s.repeat("multiplyByScalar", N, () => [gen(), g.num(10)], (a, k) => Type.multiplyByScalar(a, k, r()));
  s.repeat("negate", N, () => [gen()], (a) => Type.negate(a, r()));
  s.repeat("abs", N, () => [gen()], (a) => Type.abs(a, r()));
  s.repeat("transpose", N, () => [gen()], (a) => Type.transpose(a, r()));
  s.repeat("fromRowMajorArray", N, () => [Array.from(gen())], (a) => Type.fromRowMajorArray(a));
  s.repeat("getColumn", N, () => [gen(), Math.floor(g.uniform(0, Type.packedLength === 4 ? 2 : Type.packedLength === 9 ? 3 : 4))], (m, i) => Type.getColumn(m, i, new Vec()));
  s.repeat("getRow", N, () => [gen(), Math.floor(g.uniform(0, Type.packedLength === 4 ? 2 : Type.packedLength === 9 ? 3 : 4))], (m, i) => Type.getRow(m, i, new Vec()));
  s.repeat("setColumn", N, () => [gen(), Math.floor(g.uniform(0, Type.packedLength === 4 ? 2 : Type.packedLength === 9 ? 3 : 4)), vec()], (m, i, v) => Type.setColumn(m, i, v, r()));
  s.repeat("setRow", N, () => [gen(), Math.floor(g.uniform(0, Type.packedLength === 4 ? 2 : Type.packedLength === 9 ? 3 : 4)), vec()], (m, i, v) => Type.setRow(m, i, v, r()));
  s.repeat("equalsEpsilon", N, () => {
    const a = gen();
    const b = Type.add(a, Type.multiplyByScalar(gen(), 1e-4, r()), r());
    return [a, b, g.pick([0, 1e-5, 1e-3, 0.1])];
  }, (a, b, e) => Type.equalsEpsilon(a, b, e));
}

function matrix2Suite() {
  const g = generators(makeRng(20));
  const s = suite("Matrix2");
  const gen = () => g.m2(100);
  matrixCommon(s, g, Matrix2, gen, Cartesian2, () => g.c2(100));
  const scale = () => new Cartesian2(g.uniform(0.1, 10), g.uniform(-10, -0.1));
  s.repeat("fromRotation", N, () => [g.angle()], (a) => Matrix2.fromRotation(a));
  s.repeat("fromScale", N, () => [scale()], (v) => Matrix2.fromScale(v));
  s.repeat("fromUniformScale", N, () => [g.num(10)], (v) => Matrix2.fromUniformScale(v));
  s.repeat("getScale", N, () => [gen()], (m) => Matrix2.getScale(m, new Cartesian2()));
  s.repeat("getMaximumScale", N, () => [gen()], (m) => Matrix2.getMaximumScale(m));
  s.repeat("setScale", N, () => [gen(), scale()], (m, v) => Matrix2.setScale(m, v, new Matrix2()));
  s.repeat("setUniformScale", N, () => [gen(), g.uniform(0.1, 10)], (m, v) => Matrix2.setUniformScale(m, v, new Matrix2()));
  s.repeat("setRotation", N, () => [gen(), Matrix2.fromRotation(g.angle())], (m, rot) => Matrix2.setRotation(m, rot, new Matrix2()));
  s.repeat("getRotation", N, () => [gen()], (m) => Matrix2.getRotation(m, new Matrix2()));
  s.repeat("multiplyByVector", N, () => [gen(), g.c2(100)], (m, v) => Matrix2.multiplyByVector(m, v, new Cartesian2()));
  s.repeat("multiplyByScale", N, () => [gen(), scale()], (m, v) => Matrix2.multiplyByScale(m, v, new Matrix2()));
  s.repeat("multiplyByUniformScale", N, () => [gen(), g.num(10)], (m, v) => Matrix2.multiplyByUniformScale(m, v, new Matrix2()));
  return s;
}

function matrix3Suite() {
  const g = generators(makeRng(30));
  const s = suite("Matrix3");
  const gen = () => g.m3(100);
  matrixCommon(s, g, Matrix3, gen, Cartesian3, () => g.c3(100));
  const scale = () => new Cartesian3(g.uniform(0.1, 10), g.uniform(-10, -0.1), g.uniform(0.1, 5));
  s.repeat("fromQuaternion", N, () => [g.quat()], (q) => Matrix3.fromQuaternion(q));
  s.repeat("fromHeadingPitchRoll", N, () => [g.hpr()], (h) => Matrix3.fromHeadingPitchRoll(h));
  s.repeat("fromScale", N, () => [scale()], (v) => Matrix3.fromScale(v));
  s.repeat("fromUniformScale", N, () => [g.num(10)], (v) => Matrix3.fromUniformScale(v));
  s.repeat("fromCrossProduct", N, () => [g.c3(100)], (v) => Matrix3.fromCrossProduct(v));
  s.repeat("fromRotationX", N, () => [g.angle()], (a) => Matrix3.fromRotationX(a));
  s.repeat("fromRotationY", N, () => [g.angle()], (a) => Matrix3.fromRotationY(a));
  s.repeat("fromRotationZ", N, () => [g.angle()], (a) => Matrix3.fromRotationZ(a));
  s.repeat("getScale", N, () => [gen()], (m) => Matrix3.getScale(m, new Cartesian3()));
  s.repeat("getMaximumScale", N, () => [gen()], (m) => Matrix3.getMaximumScale(m));
  s.repeat("setScale", N, () => [gen(), scale()], (m, v) => Matrix3.setScale(m, v, new Matrix3()));
  s.repeat("setUniformScale", N, () => [gen(), g.uniform(0.1, 10)], (m, v) => Matrix3.setUniformScale(m, v, new Matrix3()));
  s.repeat("setRotation", N, () => [gen(), g.rotation3()], (m, rot) => Matrix3.setRotation(m, rot, new Matrix3()));
  s.repeat("getRotation", N, () => [gen()], (m) => Matrix3.getRotation(m, new Matrix3()));
  s.repeat("multiplyByVector", N, () => [gen(), g.c3(100)], (m, v) => Matrix3.multiplyByVector(m, v, new Cartesian3()));
  s.repeat("multiplyByScale", N, () => [gen(), scale()], (m, v) => Matrix3.multiplyByScale(m, v, new Matrix3()));
  s.repeat("multiplyByUniformScale", N, () => [gen(), g.num(10)], (m, v) => Matrix3.multiplyByUniformScale(m, v, new Matrix3()));
  s.repeat("determinant", N, () => [gen()], (m) => Matrix3.determinant(m));
  s.repeat("inverse", N, () => [gen()], (m) => Matrix3.inverse(m, new Matrix3()));
  s.repeat("inverseTranspose", N, () => [gen()], (m) => Matrix3.inverseTranspose(m, new Matrix3()));
  s.repeat("computeEigenDecomposition", N, () => {
    const a = g.m3(10);
    return [Matrix3.add(a, Matrix3.transpose(a, new Matrix3()), new Matrix3())];
  }, (m) => {
    const d = Matrix3.computeEigenDecomposition(m);
    return [d.unitary, d.diagonal];
  });
  return s;
}

function matrix4Suite() {
  const g = generators(makeRng(40));
  const s = suite("Matrix4");
  const gen = () => g.m4(100);
  matrixCommon(s, g, Matrix4, gen, Cartesian4, () => g.c4(100));
  const scale = () => new Cartesian3(g.uniform(0.1, 10), g.uniform(-10, -0.1), g.uniform(0.1, 5));
  s.repeat("fromRotationTranslation", N, () => [g.rotation3(), g.c3(1e5)], (m, t) => Matrix4.fromRotationTranslation(m, t));
  s.repeat("fromTranslationQuaternionRotationScale", N, () => [g.c3(1e5), g.quat(), scale()], (t, q, sc) => Matrix4.fromTranslationQuaternionRotationScale(t, q, sc));
  s.repeat("fromTranslation", N, () => [g.c3(1e5)], (t) => Matrix4.fromTranslation(t));
  s.repeat("fromScale", N, () => [scale()], (v) => Matrix4.fromScale(v));
  s.repeat("fromUniformScale", N, () => [g.num(10)], (v) => Matrix4.fromUniformScale(v));
  s.repeat("fromRotation", N, () => [g.rotation3()], (m) => Matrix4.fromRotation(m));
  s.repeat("fromCamera", N, () => [g.c3(1e6), g.nonZero3(1), g.nonZero3(1)], (p, d, u) => Matrix4.fromCamera({ position: p, direction: d, up: u }));
  s.repeat("computePerspectiveFieldOfView", N, () => [g.uniform(0.1, 3), g.uniform(0.3, 3), g.uniform(0.1, 10), g.uniform(100, 1e8)], (a, b, c, d) => Matrix4.computePerspectiveFieldOfView(a, b, c, d, new Matrix4()));
  s.repeat("computeOrthographicOffCenter", N, () => [g.num(100), g.num(100) + 200, g.num(100), g.num(100) + 200, g.uniform(0.1, 10), g.uniform(100, 1e5)], (...a) => Matrix4.computeOrthographicOffCenter(...a, new Matrix4()));
  s.repeat("computePerspectiveOffCenter", N, () => [g.num(1), g.num(1) + 2, g.num(1), g.num(1) + 2, g.uniform(0.1, 10), g.uniform(100, 1e5)], (...a) => Matrix4.computePerspectiveOffCenter(...a, new Matrix4()));
  s.repeat("computeInfinitePerspectiveOffCenter", N, () => [g.num(1), g.num(1) + 2, g.num(1), g.num(1) + 2, g.uniform(0.1, 10)], (...a) => Matrix4.computeInfinitePerspectiveOffCenter(...a, new Matrix4()));
  s.repeat("computeViewportTransformation", N, () => [new BoundingRectangle(g.num(100), g.num(100), g.uniform(1, 2000), g.uniform(1, 2000)), g.uniform(0, 0.5), g.uniform(0.5, 1)], (v, n, f) => Matrix4.computeViewportTransformation(v, n, f));
  s.repeat("computeView", N, () => [g.c3(1e6), g.unit3(), g.unit3(), g.unit3()], (p, d, u, r) => Matrix4.computeView(p, d, u, r, new Matrix4()));
  s.repeat("setTranslation", N, () => [gen(), g.c3(1e5)], (m, t) => Matrix4.setTranslation(m, t, new Matrix4()));
  s.repeat("getScale", N, () => [gen()], (m) => Matrix4.getScale(m, new Cartesian3()));
  s.repeat("getMaximumScale", N, () => [gen()], (m) => Matrix4.getMaximumScale(m));
  s.repeat("setScale", N, () => [gen(), scale()], (m, v) => Matrix4.setScale(m, v, new Matrix4()));
  s.repeat("setUniformScale", N, () => [gen(), g.uniform(0.1, 10)], (m, v) => Matrix4.setUniformScale(m, v, new Matrix4()));
  s.repeat("setRotation", N, () => [gen(), g.rotation3()], (m, rot) => Matrix4.setRotation(m, rot, new Matrix4()));
  s.repeat("getRotation", N, () => [gen()], (m) => Matrix4.getRotation(m, new Matrix3()));
  s.repeat("multiplyTransformation", N, () => [g.affine4(), g.affine4()], (a, b) => Matrix4.multiplyTransformation(a, b, new Matrix4()));
  s.repeat("multiplyByMatrix3", N, () => [gen(), g.m3(10)], (a, b) => Matrix4.multiplyByMatrix3(a, b, new Matrix4()));
  s.repeat("multiplyByTranslation", N, () => [gen(), g.c3(1e4)], (a, t) => Matrix4.multiplyByTranslation(a, t, new Matrix4()));
  s.repeat("multiplyByScale", N, () => [gen(), g.pick([scale(), new Cartesian3(1, 1, 1)])], (a, v) => Matrix4.multiplyByScale(a, v, new Matrix4()));
  s.repeat("multiplyByUniformScale", N, () => [gen(), g.num(10)], (a, v) => Matrix4.multiplyByUniformScale(a, v, new Matrix4()));
  s.repeat("multiplyByVector", N, () => [gen(), g.c4(100)], (m, v) => Matrix4.multiplyByVector(m, v, new Cartesian4()));
  s.repeat("multiplyByPointAsVector", N, () => [gen(), g.c3(100)], (m, v) => Matrix4.multiplyByPointAsVector(m, v, new Cartesian3()));
  s.repeat("multiplyByPoint", N, () => [gen(), g.c3(100)], (m, v) => Matrix4.multiplyByPoint(m, v, new Cartesian3()));
  s.repeat("getTranslation", N, () => [gen()], (m) => Matrix4.getTranslation(m, new Cartesian3()));
  s.repeat("getMatrix3", N, () => [gen()], (m) => Matrix4.getMatrix3(m, new Matrix3()));
  s.repeat("inverse", N, () => [g.pick([gen(), g.affine4(), g.rigid4()])], (m) => Matrix4.inverse(m, new Matrix4()));
  // Zero upper 3x3 with a translation: Cesium's special case.
  s.add("inverse", [Matrix4.fromColumnMajorArray([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 1])], (m) => Matrix4.inverse(m, new Matrix4()));
  s.add("inverse", [Matrix4.ZERO], (m) => Matrix4.inverse(m, new Matrix4()));
  s.repeat("inverseTransformation", N, () => [g.rigid4()], (m) => Matrix4.inverseTransformation(m, new Matrix4()));
  s.repeat("inverseTranspose", N, () => [g.affine4()], (m) => Matrix4.inverseTranspose(m, new Matrix4()));
  return s;
}

function quaternionSuite() {
  const g = generators(makeRng(50));
  const s = suite("Quaternion");
  const q = () => g.quat();
  const anyQ = () => new Quaternion(g.num(10), g.num(10), g.num(10), g.num(10));
  const r = () => new Quaternion();
  s.repeat("fromAxisAngle", N, () => [g.nonZero3(10), g.angle()], (a, t) => Quaternion.fromAxisAngle(a, t));
  s.repeat("fromRotationMatrix", N * 2, () => [g.rotation3()], (m) => Quaternion.fromRotationMatrix(m));
  for (const m of [Matrix3.fromRotationX(Math.PI), Matrix3.fromRotationY(Math.PI), Matrix3.fromRotationZ(Math.PI), Matrix3.fromRotationX(3)]) {
    s.add("fromRotationMatrix", [m], (x) => Quaternion.fromRotationMatrix(x));
  }
  s.repeat("fromHeadingPitchRoll", N, () => [g.hpr()], (h) => Quaternion.fromHeadingPitchRoll(h));
  s.repeat("conjugate", N, () => [anyQ()], (a) => Quaternion.conjugate(a, r()));
  s.repeat("magnitude", N, () => [anyQ()], (a) => Quaternion.magnitude(a));
  s.repeat("normalize", N, () => [anyQ()], (a) => Quaternion.normalize(a, r()));
  s.repeat("inverse", N, () => [anyQ()], (a) => Quaternion.inverse(a, r()));
  s.repeat("add", N, () => [anyQ(), anyQ()], (a, b) => Quaternion.add(a, b, r()));
  s.repeat("subtract", N, () => [anyQ(), anyQ()], (a, b) => Quaternion.subtract(a, b, r()));
  s.repeat("negate", N, () => [anyQ()], (a) => Quaternion.negate(a, r()));
  s.repeat("dot", N, () => [anyQ(), anyQ()], (a, b) => Quaternion.dot(a, b));
  s.repeat("multiply", N, () => [anyQ(), anyQ()], (a, b) => Quaternion.multiply(a, b, r()));
  s.repeat("multiplyByScalar", N, () => [anyQ(), g.num(10)], (a, k) => Quaternion.multiplyByScalar(a, k, r()));
  s.repeat("divideByScalar", N, () => [anyQ(), g.num(10)], (a, k) => Quaternion.divideByScalar(a, k, r()));
  s.repeat("computeAxis", N, () => [g.pick([q(), Quaternion.IDENTITY])], (a) => Quaternion.computeAxis(a, new Cartesian3()));
  s.repeat("computeAngle", N, () => [g.pick([q(), Quaternion.IDENTITY])], (a) => Quaternion.computeAngle(a));
  s.repeat("lerp", N, () => [q(), q(), g.uniform(0, 1)], (a, b, t) => Quaternion.lerp(a, b, t, r()));
  s.repeat("slerp", N * 2, () => {
    const a = q();
    return [a, g.pick([q(), a, Quaternion.negate(a, r())]), g.uniform(0, 1)];
  }, (a, b, t) => Quaternion.slerp(a, b, t, r()));
  s.repeat("log", N, () => [q()], (a) => Quaternion.log(a, new Cartesian3()));
  s.repeat("exp", N, () => [g.pick([g.c3(2), Cartesian3.ZERO])], (c) => Quaternion.exp(c, r()));
  s.repeat("computeInnerQuadrangle", N, () => [q(), q(), q()], (a, b, c) => Quaternion.computeInnerQuadrangle(a, b, c, r()));
  s.repeat("squad", N, () => [q(), q(), q(), q(), g.uniform(0, 1)], (a, b, c, d, t) => Quaternion.squad(a, b, c, d, t, r()));
  s.repeat("fastSlerp", N, () => [q(), q(), g.uniform(0, 1)], (a, b, t) => Quaternion.fastSlerp(a, b, t, r()));
  s.repeat("fastSquad", N, () => [q(), q(), q(), q(), g.uniform(0, 1)], (a, b, c, d, t) => Quaternion.fastSquad(a, b, c, d, t, r()));
  s.repeat("convertPackedArrayForInterpolation", 5, () => [Array.from({ length: 4 }, () => q())], (qs) => {
    const packed = [];
    qs.forEach((x, i) => Quaternion.pack(x, packed, i * 4));
    const out = [];
    Quaternion.convertPackedArrayForInterpolation(packed, 0, 3, out);
    return out;
  });
  s.repeat("unpackInterpolationResult", 5, () => [g.c3(1), q()], (v, q0) => {
    const source = [];
    Quaternion.pack(q0, source, 0);
    return Quaternion.unpackInterpolationResult([v.x, v.y, v.z], source, 0, 0, r());
  });
  return s;
}

function hprSuite() {
  const g = generators(makeRng(60));
  const s = suite("HeadingPitchRoll");
  s.repeat("fromQuaternion", N * 2, () => [g.quat()], (q) => HeadingPitchRoll.fromQuaternion(q));
  s.repeat("fromDegrees", N, () => [g.num(360), g.num(90), g.num(360)], (h, p, r) => HeadingPitchRoll.fromDegrees(h, p, r));
  s.repeat("roundTrip", N, () => [g.hpr()], (h) => HeadingPitchRoll.fromQuaternion(Quaternion.fromHeadingPitchRoll(h)));
  return s;
}

function sphericalSuite() {
  const g = generators(makeRng(70));
  const s = suite("Spherical");
  s.repeat("fromCartesian3", N, () => [g.c3(100)], (c) => Spherical.fromCartesian3(c));
  s.repeat("normalize", N, () => [new Spherical(g.angle(), g.angle(), g.num(10))], (sp) => Spherical.normalize(sp));
  return s;
}

export default function coreMath() {
  return [
    mathSuite(),
    cartesian2Suite(),
    cartesian3Suite(),
    cartesian4Suite(),
    matrix2Suite(),
    matrix3Suite(),
    matrix4Suite(),
    quaternionSuite(),
    hprSuite(),
    sphericalSuite(),
  ];
}
