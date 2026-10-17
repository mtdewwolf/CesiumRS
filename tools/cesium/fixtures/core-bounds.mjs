// Fixtures for polynomial solvers, Ray, IntersectionTests, bounding boxes,
// culling volumes, frustums, tangent planes, occluders, rhumb lines, and
// geodesics.

import {
  AxisAlignedBoundingBox,
  BoundingSphere,
  Cartesian2,
  Cartesian3,
  Cartographic,
  CubicRealPolynomial,
  CullingVolume,
  Ellipsoid,
  EllipsoidGeodesic,
  EllipsoidRhumbLine,
  EllipsoidTangentPlane,
  EllipsoidalOccluder,
  IntersectionTests,
  Matrix3,
  Occluder,
  OrientedBoundingBox,
  OrthographicFrustum,
  OrthographicOffCenterFrustum,
  PerspectiveFrustum,
  PerspectiveOffCenterFrustum,
  QuadraticRealPolynomial,
  QuarticRealPolynomial,
  Ray,
  Rectangle,
} from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

const N = 25;
const E = Ellipsoid.WGS84;

const aabbEnc = (b) => (b ? [b.minimum, b.maximum, b.center] : b);
const obbEnc = (b) => (b ? [b.center, b.halfAxes] : b);

function polynomialSuite() {
  const g = generators(makeRng(200));
  const s = suite("Polynomials");
  const coef = () => g.pick([0, 0, g.num(10), g.num(10), g.num(1e3), g.num(1e-3)]);
  const root = () => g.pick([g.num(10), g.num(10), 0, 1, -1, g.num(1e-3)]);
  const scale = () => g.pick([1, -2, 0.5, g.num(10)]);
  // Quadratic from roots: k(x - r0)(x - r1).
  for (let i = 0; i < N * 2; i++) {
    const [r0, r1, k] = [root(), root(), scale()];
    s.add("quadraticRoots", [k, -k * (r0 + r1), k * r0 * r1], QuadraticRealPolynomial.computeRealRoots);
  }
  s.repeat("quadraticRoots", N * 2, () => [coef(), coef(), coef()], QuadraticRealPolynomial.computeRealRoots);
  s.repeat("quadraticDiscriminant", N, () => [coef(), coef(), coef()], QuadraticRealPolynomial.computeDiscriminant);
  for (let i = 0; i < N * 2; i++) {
    const [r0, r1, r2, k] = [root(), root(), root(), scale()];
    s.add("cubicRoots", [k, -k * (r0 + r1 + r2), k * (r0 * r1 + r0 * r2 + r1 * r2), -k * r0 * r1 * r2], CubicRealPolynomial.computeRealRoots);
  }
  s.repeat("cubicRoots", N * 3, () => [coef(), coef(), coef(), coef()], CubicRealPolynomial.computeRealRoots);
  s.add("cubicRoots", [1, 0, 1, 0], CubicRealPolynomial.computeRealRoots);
  s.repeat("cubicDiscriminant", N, () => [coef(), coef(), coef(), coef()], CubicRealPolynomial.computeDiscriminant);
  for (let i = 0; i < N * 2; i++) {
    const r = [root(), root(), root(), root()];
    const k = scale();
    const [a, b, c, d] = r;
    s.add(
      "quarticRoots",
      [k, -k * (a + b + c + d), k * (a * b + a * c + a * d + b * c + b * d + c * d), -k * (a * b * c + a * b * d + a * c * d + b * c * d), k * a * b * c * d],
      QuarticRealPolynomial.computeRealRoots,
    );
  }
  s.repeat("quarticRoots", N * 4, () => [coef(), coef(), coef(), coef(), coef()], QuarticRealPolynomial.computeRealRoots);
  s.repeat("quarticDiscriminant", N, () => [coef(), coef(), coef(), coef(), coef()], QuarticRealPolynomial.computeDiscriminant);
  return s;
}

function intersectionSuite() {
  const g = generators(makeRng(210));
  const s = suite("IntersectionTests");
  const ray = (range = 100) => new Ray(g.c3(range), g.nonZero3(1));
  const rayEnc = (r) => [r.origin, r.direction];
  s.repeat("Ray", N, () => [g.c3(100), g.pick([g.nonZero3(10), Cartesian3.ZERO])], (o, d) => rayEnc(new Ray(o, d)));
  s.repeat("Ray.getPoint", N, () => [ray(), g.num(100)], (r, t) => Ray.getPoint(r, t));
  s.repeat("rayPlane", N * 2, () => [ray(), g.plane()], (r, p) => IntersectionTests.rayPlane(r, p));
  const tri = () => [g.c3(10), g.c3(10), g.c3(10)];
  s.repeat("rayTriangleParametric", N * 3, () => {
    const [p0, p1, p2] = tri();
    const target = Cartesian3.lerp(Cartesian3.lerp(p0, p1, g.rng(), new Cartesian3()), p2, g.rng() * 0.5, new Cartesian3());
    const origin = g.c3(20);
    const dir = Cartesian3.subtract(g.pick([target, g.c3(10)]), origin, new Cartesian3());
    return [new Ray(origin, dir), p0, p1, p2, g.pick([true, false])];
  }, (r, a, b, c, cull) => IntersectionTests.rayTriangleParametric(r, a, b, c, cull));
  s.repeat("rayTriangle", N, () => {
    const [p0, p1, p2] = tri();
    const target = Cartesian3.lerp(p0, p1, g.rng(), new Cartesian3());
    const origin = g.c3(20);
    return [new Ray(origin, Cartesian3.subtract(target, origin, new Cartesian3())), p0, p1, p2, false];
  }, (r, a, b, c, cull) => IntersectionTests.rayTriangle(r, a, b, c, cull));
  s.repeat("lineSegmentTriangle", N, () => [g.c3(20), g.c3(20), ...tri(), g.pick([true, false])], (v0, v1, a, b, c, cull) => IntersectionTests.lineSegmentTriangle(v0, v1, a, b, c, cull));
  s.repeat("raySphere", N * 2, () => [ray(), new BoundingSphere(g.c3(50), g.uniform(1, 80))], (r, sp) => IntersectionTests.raySphere(r, sp));
  s.repeat("lineSegmentSphere", N * 2, () => [g.c3(100), g.c3(100), new BoundingSphere(g.c3(50), g.uniform(1, 80))], (a, b, sp) => IntersectionTests.lineSegmentSphere(a, b, sp));
  s.repeat("rayEllipsoid", N * 3, () => {
    const origin = g.pick([g.c3(2e7), g.surface(), g.c3(1e6)]);
    const toward = Cartesian3.subtract(g.c3(5e6), origin, new Cartesian3());
    return [new Ray(origin, g.pick([toward, g.nonZero3(1)]))];
  }, (r) => IntersectionTests.rayEllipsoid(r, E));
  s.repeat("rayAxisAlignedBoundingBox", N * 2, () => [ray(), AxisAlignedBoundingBox.fromPoints([g.c3(50), g.c3(50)])], (r, b) => IntersectionTests.rayAxisAlignedBoundingBox(r, b));
  s.repeat("grazingAltitudeLocation", N * 2, () => {
    const origin = g.c3(3e7);
    const toward = Cartesian3.subtract(g.c3(8e6), origin, new Cartesian3());
    return [new Ray(origin, toward)];
  }, (r) => IntersectionTests.grazingAltitudeLocation(r, E));
  s.repeat("lineSegmentPlane", N * 2, () => [g.c3(100), g.c3(100), g.plane()], (a, b, p) => IntersectionTests.lineSegmentPlane(a, b, p));
  s.repeat("trianglePlaneIntersection", N * 2, () => [g.c3(100), g.c3(100), g.c3(100), new (g.plane().constructor)(g.unit3(), g.num(30))], (a, b, c, p) => IntersectionTests.trianglePlaneIntersection(a, b, c, p));
  s.repeat("quadraticVectorExpression", N, () => [g.m3(10), g.c3(10), g.num(10), g.num(2), g.uniform(0.1, 3)], (A, b, c, x, w) => IntersectionTests.quadraticVectorExpression(A, b, c, x, w));
  return s;
}

function boxesSuite() {
  const g = generators(makeRng(220));
  const s = suite("Boxes");
  const pts = (n) => Array.from({ length: n }, () => g.c3(100));
  s.repeat("AABB.fromPoints", N, () => [pts(1 + Math.floor(g.uniform(0, 8)))], (p) => aabbEnc(AxisAlignedBoundingBox.fromPoints(p)));
  s.add("AABB.fromPoints", [[]], (p) => aabbEnc(AxisAlignedBoundingBox.fromPoints(p)));
  s.repeat("AABB.intersectPlane", N * 2, () => [pts(2), g.plane()], (p, pl) => AxisAlignedBoundingBox.fromPoints(p).intersectPlane(pl));
  s.repeat("AABB.intersectAxisAlignedBoundingBox", N * 2, () => [pts(2), pts(2)], (a, b) => AxisAlignedBoundingBox.fromPoints(a).intersectAxisAlignedBoundingBox(AxisAlignedBoundingBox.fromPoints(b)));
  s.repeat("OBB.fromPoints", N, () => [pts(1 + Math.floor(g.uniform(0, 10)))], (p) => obbEnc(OrientedBoundingBox.fromPoints(p)));
  s.add("OBB.fromPoints", [[]], (p) => obbEnc(OrientedBoundingBox.fromPoints(p)));
  s.repeat("OBB.fromRectangle", N * 2, () => {
    const west = g.uniform(-Math.PI, Math.PI);
    const width = g.pick([g.uniform(0, Math.PI), g.uniform(Math.PI, 2 * Math.PI), 0.01]);
    const south = g.uniform(-1.5, 1.4);
    const north = Math.min(south + g.uniform(0, 1.5), Math.PI / 2);
    let east = west + width;
    if (east > Math.PI) east -= 2 * Math.PI;
    return [new Rectangle(west, south, east, north), g.pick([0, -100, 500]), g.pick([0, 1000, 1e5])];
  }, (r, lo, hi) => obbEnc(OrientedBoundingBox.fromRectangle(r, lo, hi, E)));
  const obb = () => OrientedBoundingBox.fromPoints(pts(5));
  s.repeat("OBB.fromTransformation", N, () => [g.affine4()], (m) => obbEnc(OrientedBoundingBox.fromTransformation(m)));
  s.repeat("OBB.intersectPlane", N * 2, () => [obbEnc(obb()), g.plane()], (b, p) => new OrientedBoundingBox(b[0], b[1]).intersectPlane(p));
  s.repeat("OBB.distanceSquaredTo", N * 2, () => {
    const b = obb();
    const h = Matrix3.clone(b.halfAxes);
    const kill = g.pick([[], [0], [1], [2], [0, 1], [1, 2], [0, 2], [0, 1, 2]]);
    for (const c of kill) for (let r = 0; r < 3; r++) h[c * 3 + r] = 0;
    return [[b.center, h], g.c3(200)];
  }, (b, p) => new OrientedBoundingBox(b[0], b[1]).distanceSquaredTo(p));
  s.repeat("OBB.computePlaneDistances", N, () => [obbEnc(obb()), g.c3(200), g.unit3()], (b, p, d) => new OrientedBoundingBox(b[0], b[1]).computePlaneDistances(p, d));
  s.repeat("OBB.computeCorners", N, () => [obbEnc(obb())], (b) => new OrientedBoundingBox(b[0], b[1]).computeCorners());
  s.repeat("OBB.computeTransformation", N, () => [obbEnc(obb())], (b) => new OrientedBoundingBox(b[0], b[1]).computeTransformation());
  s.repeat("BoundingSphere.fromOrientedBoundingBox", N, () => [obbEnc(obb())], (b) => BoundingSphere.fromOrientedBoundingBox(new OrientedBoundingBox(b[0], b[1])));
  return s;
}

function frustumSuite() {
  const g = generators(makeRng(230));
  const s = suite("Frustums");
  const cam = () => {
    const d = g.unit3();
    const up = Cartesian3.normalize(Cartesian3.cross(Cartesian3.cross(d, g.unit3(), new Cartesian3()), d, new Cartesian3()), new Cartesian3());
    return [g.c3(1e3), d, up];
  };
  const persp = () => {
    const f = new PerspectiveFrustum({ fov: g.uniform(0.2, 2.5), aspectRatio: g.uniform(0.4, 3), near: g.uniform(0.1, 10), far: g.uniform(100, 1e6) });
    f.xOffset = g.pick([0, g.num(1)]);
    f.yOffset = g.pick([0, g.num(1)]);
    return f;
  };
  const pEnc = (f) => [f.fov, f.aspectRatio, f.near, f.far, f.xOffset, f.yOffset];
  s.repeat("PerspectiveFrustum", N, () => [pEnc(persp())], (p) => {
    const f = new PerspectiveFrustum({ fov: p[0], aspectRatio: p[1], near: p[2], far: p[3], xOffset: p[4], yOffset: p[5] });
    const o = f.offCenterFrustum;
    return [f.fovy, f.sseDenominator, [o.left, o.right, o.top, o.bottom, o.near, o.far], f.projectionMatrix, f.infiniteProjectionMatrix];
  });
  s.repeat("PerspectiveFrustum.computeCullingVolume", N, () => [pEnc(persp()), ...cam()], (p, pos, d, up) => {
    const f = new PerspectiveFrustum({ fov: p[0], aspectRatio: p[1], near: p[2], far: p[3], xOffset: p[4], yOffset: p[5] });
    return f.computeCullingVolume(pos, d, up).planes;
  });
  s.repeat("PerspectiveFrustum.getPixelDimensions", N, () => [pEnc(persp()), g.uniform(100, 4000), g.uniform(100, 4000), g.uniform(1, 1e5), g.pick([1, 2, 1.5])], (p, w, h, d, r) => {
    const f = new PerspectiveFrustum({ fov: p[0], aspectRatio: p[1], near: p[2], far: p[3], xOffset: p[4], yOffset: p[5] });
    return f.getPixelDimensions(w, h, d, r, new Cartesian2());
  });
  const off = () => {
    const l = g.num(5);
    const b = g.num(5);
    return [l, l + g.uniform(0.5, 10), b + g.uniform(0.5, 10), b, g.uniform(0.1, 5), g.uniform(100, 1e5)];
  };
  const mk = (T, a) => new T({ left: a[0], right: a[1], top: a[2], bottom: a[3], near: a[4], far: a[5] });
  s.repeat("PerspectiveOffCenterFrustum", N, () => [off(), ...cam()], (a, pos, d, up) => {
    const f = mk(PerspectiveOffCenterFrustum, a);
    return [f.projectionMatrix, f.infiniteProjectionMatrix, f.computeCullingVolume(pos, d, up).planes, f.getPixelDimensions(800, 600, 50, 1, new Cartesian2())];
  });
  s.repeat("OrthographicOffCenterFrustum", N, () => [off(), ...cam()], (a, pos, d, up) => {
    const f = mk(OrthographicOffCenterFrustum, a);
    return [f.projectionMatrix, f.computeCullingVolume(pos, d, up).planes, f.getPixelDimensions(800, 600, 50, 2, new Cartesian2())];
  });
  s.repeat("OrthographicFrustum", N, () => [[g.uniform(1, 1e4), g.uniform(0.3, 3), g.uniform(0.1, 5), g.uniform(100, 1e6)], ...cam()], (a, pos, d, up) => {
    const f = new OrthographicFrustum({ width: a[0], aspectRatio: a[1], near: a[2], far: a[3] });
    const o = f.offCenterFrustum;
    return [[o.left, o.right, o.top, o.bottom, o.near, o.far], f.projectionMatrix, f.computeCullingVolume(pos, d, up).planes, f.getPixelDimensions(640, 480, 10, 1, new Cartesian2())];
  });
  // Culling volumes.
  s.repeat("CullingVolume.fromBoundingSphere", N, () => [g.sphere()], (b) => CullingVolume.fromBoundingSphere(b).planes);
  s.repeat("CullingVolume.computeVisibility", N * 3, () => [pEnc(persp()), ...cam(), new BoundingSphere(g.c3(2e3), g.uniform(1, 500))], (p, pos, d, up, b) => {
    const f = new PerspectiveFrustum({ fov: p[0], aspectRatio: p[1], near: p[2], far: p[3], xOffset: p[4], yOffset: p[5] });
    const cv = f.computeCullingVolume(pos, d, up);
    const box = AxisAlignedBoundingBox.fromPoints([Cartesian3.subtract(b.center, new Cartesian3(b.radius, b.radius, b.radius), new Cartesian3()), Cartesian3.add(b.center, new Cartesian3(b.radius, b.radius, b.radius), new Cartesian3())]);
    const masks = [CullingVolume.MASK_INDETERMINATE, 0b101010, 0b111111, CullingVolume.MASK_INSIDE].map((m) => cv.computeVisibilityWithPlaneMask(b, m));
    return [cv.computeVisibility(b), cv.computeVisibility(box), ...masks];
  });
  return s;
}

function tangentPlaneSuite() {
  const g = generators(makeRng(240));
  const s = suite("EllipsoidTangentPlane");
  const enc = (t) => [t.origin, t.xAxis, t.yAxis, t.zAxis, t.plane];
  s.repeat("new", N, () => [g.surface()], (o) => enc(new EllipsoidTangentPlane(o, E)));
  s.repeat("fromPoints", N, () => [Array.from({ length: 4 }, () => g.surface())], (p) => enc(EllipsoidTangentPlane.fromPoints(p, E)));
  s.repeat("projectPointOntoPlane", N * 2, () => [g.surface(), g.pick([g.surface(), g.c3(1e7)])], (o, p) => new EllipsoidTangentPlane(o, E).projectPointOntoPlane(p));
  s.repeat("projectPointsOntoPlane", N, () => [g.surface(), Array.from({ length: 5 }, () => g.c3(1e7))], (o, p) => new EllipsoidTangentPlane(o, E).projectPointsOntoPlane(p));
  s.repeat("projectPointToNearestOnPlane", N * 2, () => [g.surface(), g.c3(1e7)], (o, p) => new EllipsoidTangentPlane(o, E).projectPointToNearestOnPlane(p));
  s.repeat("projectPointOntoEllipsoid", N, () => [g.surface(), g.c2(1e6)], (o, p) => new EllipsoidTangentPlane(o, E).projectPointOntoEllipsoid(p));
  return s;
}

function occluderSuite() {
  const g = generators(makeRng(250));
  const s = suite("Occluders");
  const occ = () => new BoundingSphere(g.c3(1e3), g.uniform(10, 500));
  s.repeat("Occluder.isPointVisible", N * 2, () => [occ(), g.c3(3e3), g.c3(3e3)], (o, cam, p) => new Occluder(o, cam).isPointVisible(p));
  s.repeat("Occluder.isBoundingSphereVisible", N * 3, () => [occ(), g.c3(3e3), new BoundingSphere(g.c3(3e3), g.uniform(1, 800))], (o, cam, b) => new Occluder(o, cam).isBoundingSphereVisible(b));
  s.repeat("Occluder.computeVisibility", N * 3, () => {
    const o = occ();
    const cam = g.c3(3e3);
    const behind = Cartesian3.add(o.center, Cartesian3.multiplyByScalar(Cartesian3.subtract(o.center, cam, new Cartesian3()), g.uniform(0.2, 2), new Cartesian3()), new Cartesian3());
    return [o, cam, new BoundingSphere(g.pick([behind, g.c3(3e3)]), g.uniform(1, 300))];
  }, (o, cam, b) => new Occluder(o, cam).computeVisibility(b));
  s.repeat("Occluder.computeOccludeePoint", N * 2, () => {
    const o = new BoundingSphere(Cartesian3.ZERO, 6.3e6);
    const center = g.surface();
    const positions = Array.from({ length: 4 }, () => Cartesian3.add(center, g.c3(1e5), new Cartesian3()));
    return [o, center, positions];
  }, (o, c, p) => Occluder.computeOccludeePoint(o, c, p));
  s.repeat("Occluder.computeOccludeePointFromRectangle", N, () => [g.rectangle()], (r) => Occluder.computeOccludeePointFromRectangle(r, E));
  s.repeat("EllipsoidalOccluder.isPointVisible", N * 2, () => [g.c3(3e7), g.pick([g.surface(), g.c3(2e7)])], (cam, p) => new EllipsoidalOccluder(E, cam).isPointVisible(p));
  s.repeat("EllipsoidalOccluder.isScaledSpacePointVisiblePossiblyUnderEllipsoid", N * 2, () => [g.c3(3e7), g.c3(3), g.pick([undefined, -1000, 500, -1e7])], (cam, p, h) => new EllipsoidalOccluder(E, cam).isScaledSpacePointVisiblePossiblyUnderEllipsoid(p, h));
  s.repeat("EllipsoidalOccluder.computeHorizonCullingPoint", N * 2, () => {
    const c = g.surface();
    return [c, Array.from({ length: 4 }, () => Cartesian3.add(c, g.c3(1e5), new Cartesian3()))];
  }, (d, p) => new EllipsoidalOccluder(E).computeHorizonCullingPoint(d, p));
  s.repeat("EllipsoidalOccluder.computeHorizonCullingPointPossiblyUnderEllipsoid", N, () => {
    const c = g.surface();
    return [c, Array.from({ length: 4 }, () => Cartesian3.add(c, g.c3(1e5), new Cartesian3())), g.pick([undefined, -500, 100])];
  }, (d, p, h) => new EllipsoidalOccluder(E).computeHorizonCullingPointPossiblyUnderEllipsoid(d, p, h));
  s.repeat("EllipsoidalOccluder.computeHorizonCullingPointFromVertices", N, () => {
    const c = g.surface();
    return [c, Array.from({ length: 12 }, () => g.num(1e5)), c];
  }, (d, v, c) => new EllipsoidalOccluder(E).computeHorizonCullingPointFromVertices(d, v, 3, c));
  s.repeat("EllipsoidalOccluder.computeHorizonCullingPointFromRectangle", N, () => [g.rectangle()], (r) => new EllipsoidalOccluder(E).computeHorizonCullingPointFromRectangle(r, E));
  return s;
}

function curvesSuite() {
  const g = generators(makeRng(260));
  const s = suite("Curves");
  const carto = () => new Cartographic(g.uniform(-Math.PI, Math.PI), g.uniform(-1.5, 1.5), g.num(100));
  const fractions = [0, 0.25, 0.5, 0.9, 1];
  s.repeat("EllipsoidGeodesic", N * 2, () => [carto(), carto()], (a, b) => {
    const geo = new EllipsoidGeodesic(a, b, E);
    return [geo.surfaceDistance, geo.startHeading, geo.endHeading, ...fractions.map((f) => geo.interpolateUsingFraction(f))];
  });
  s.repeat("EllipsoidGeodesicMoon", N, () => [carto(), carto()], (a, b) => {
    const geo = new EllipsoidGeodesic(a, b, Ellipsoid.MOON);
    return [geo.surfaceDistance, geo.startHeading, geo.endHeading, geo.interpolateUsingSurfaceDistance(1000)];
  });
  s.repeat("EllipsoidRhumbLine", N * 2, () => [carto(), g.pick([carto(), new Cartographic(g.uniform(-3, 3), 0.3, 0)])], (a, b) => {
    const line = new EllipsoidRhumbLine(a, b, E);
    return [line.surfaceDistance, line.heading, line.start, line.end, ...fractions.map((f) => line.interpolateUsingFraction(f))];
  });
  s.repeat("EllipsoidRhumbLine.fromStartHeadingDistance", N * 2, () => [carto(), g.pick([g.angle(), Math.PI / 2, -Math.PI / 2, 0, Math.PI]), g.uniform(0, 5e6)], (a, h, d) => {
    const line = EllipsoidRhumbLine.fromStartHeadingDistance(a, h, d, E);
    return [line.surfaceDistance, line.heading, line.end];
  });
  s.repeat("EllipsoidRhumbLine.findIntersectionWithLongitude", N * 2, () => [carto(), carto(), g.uniform(-4, 4)], (a, b, lon) => new EllipsoidRhumbLine(a, b, E).findIntersectionWithLongitude(lon));
  s.repeat("EllipsoidRhumbLine.findIntersectionWithLatitude", N * 2, () => [carto(), carto(), g.uniform(-1.5, 1.5)], (a, b, lat) => new EllipsoidRhumbLine(a, b, E).findIntersectionWithLatitude(lat));
  s.repeat("EllipsoidRhumbLineSphere", N, () => [carto(), carto()], (a, b) => {
    const line = new EllipsoidRhumbLine(a, b, Ellipsoid.UNIT_SPHERE);
    return [line.surfaceDistance, line.heading, line.interpolateUsingFraction(0.3)];
  });
  return s;
}

export default function coreBounds() {
  return [polynomialSuite(), intersectionSuite(), boxesSuite(), frustumSuite(), tangentPlaneSuite(), occluderSuite(), curvesSuite()];
}
