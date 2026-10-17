// Fixtures for Ellipsoid, Cartographic, Rectangle, projections,
// BoundingRectangle, BoundingSphere, Plane, Transforms (local frames), Color.

import {
  BoundingRectangle,
  BoundingSphere,
  Cartesian2,
  Cartesian3,
  Cartesian4,
  Cartographic,
  Color,
  Ellipsoid,
  GeographicProjection,
  HeadingPitchRoll,
  Math as CesiumMath,
  Matrix4,
  Plane,
  Rectangle,
  Transforms,
  WebMercatorProjection,
} from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

const N = 25;

function ellipsoidSuite() {
  const g = generators(makeRng(100));
  const s = suite("Ellipsoid");
  const E = Ellipsoid.WGS84;
  const ellipsoids = { WGS84: Ellipsoid.WGS84, MOON: Ellipsoid.MOON, MARS: Ellipsoid.MARS, UNIT_SPHERE: Ellipsoid.UNIT_SPHERE };
  for (const [name, e] of Object.entries(ellipsoids)) {
    s.add("constants", [name], () => [e.radii, e.radiiSquared, e.radiiToTheFourth, e.oneOverRadii, e.oneOverRadiiSquared, e.minimumRadius, e.maximumRadius]);
  }
  s.repeat("geodeticSurfaceNormalCartographic", N, () => [g.lonLat()], (c) => E.geodeticSurfaceNormalCartographic(c));
  s.repeat("geodeticSurfaceNormal", N, () => [g.surface()], (c) => E.geodeticSurfaceNormal(c));
  s.add("geodeticSurfaceNormal", [Cartesian3.ZERO], (c) => E.geodeticSurfaceNormal(c));
  s.repeat("cartographicToCartesian", N, () => [g.lonLat()], (c) => E.cartographicToCartesian(c));
  s.repeat("cartesianToCartographic", N * 2, () => [g.pick([g.surface(), g.c3(1e7), g.c3(10)])], (c) => E.cartesianToCartographic(c));
  s.add("cartesianToCartographic", [Cartesian3.ZERO], (c) => E.cartesianToCartographic(c));
  s.add("cartesianToCartographic", [new Cartesian3(0, 0, 6356752.3142451793)], (c) => E.cartesianToCartographic(c));
  s.repeat("scaleToGeodeticSurface", N, () => [g.pick([g.surface(), g.c3(1e7), g.c3(1)])], (c) => E.scaleToGeodeticSurface(c));
  s.repeat("scaleToGeocentricSurface", N, () => [g.nonZero3(1e7)], (c) => E.scaleToGeocentricSurface(c));
  s.repeat("transformPositionToScaledSpace", N, () => [g.c3(1e7)], (c) => E.transformPositionToScaledSpace(c));
  s.repeat("transformPositionFromScaledSpace", N, () => [g.c3(2)], (c) => E.transformPositionFromScaledSpace(c));
  s.repeat("getSurfaceNormalIntersectionWithZAxis", N, () => [g.surface(), g.pick([0, 10, 1e5])], (p, b) => E.getSurfaceNormalIntersectionWithZAxis(p, b));
  s.repeat("getLocalCurvature", N, () => [E.scaleToGeodeticSurface(g.surface())], (p) => E.getLocalCurvature(p));
  s.repeat("surfaceArea", N, () => [g.rectangle()], (r) => E.surfaceArea(r));
  s.repeat("cartesianToCartographicMoon", N, () => [g.c3(3e6)], (c) => Ellipsoid.MOON.cartesianToCartographic(c));
  return s;
}

function cartographicSuite() {
  const g = generators(makeRng(110));
  const s = suite("Cartographic");
  s.repeat("fromDegrees", N, () => [g.num(360), g.num(90), g.num(1e4)], (a, b, c) => Cartographic.fromDegrees(a, b, c));
  s.repeat("fromCartesian", N * 2, () => [g.pick([g.surface(), g.c3(1e7)])], (c) => Cartographic.fromCartesian(c));
  s.add("fromCartesian", [Cartesian3.ZERO], (c) => Cartographic.fromCartesian(c));
  s.repeat("toCartesian", N, () => [g.lonLat()], (c) => Cartographic.toCartesian(c));
  return s;
}

function rectangleSuite() {
  const g = generators(makeRng(120));
  const s = suite("Rectangle");
  const r = () => g.rectangle();
  const carto = () => new Cartographic(g.uniform(-Math.PI, Math.PI), g.uniform(-1.5, 1.5), 0);
  s.repeat("computeWidth", N, () => [r()], (a) => Rectangle.computeWidth(a));
  s.repeat("computeHeight", N, () => [r()], (a) => Rectangle.computeHeight(a));
  s.repeat("fromDegrees", N, () => [g.num(180), g.num(90), g.num(180), g.num(90)], (...a) => Rectangle.fromDegrees(...a));
  s.repeat("fromCartographicArray", N, () => [Array.from({ length: 5 }, carto)], (a) => Rectangle.fromCartographicArray(a));
  s.add("fromCartographicArray", [[new Cartographic(3, 0, 0), new Cartographic(-3, 0.5, 0)]], (a) => Rectangle.fromCartographicArray(a));
  s.repeat("fromCartesianArray", N, () => [Array.from({ length: 5 }, () => g.surface())], (a) => Rectangle.fromCartesianArray(a));
  s.repeat("fromBoundingSphere", N, () => [new BoundingSphere(g.surface(), g.uniform(10, 1e6))], (b) => Rectangle.fromBoundingSphere(b));
  s.add("fromBoundingSphere", [new BoundingSphere(Cartesian3.ZERO, 10)], (b) => Rectangle.fromBoundingSphere(b));
  s.repeat("southwest", N, () => [r()], (a) => Rectangle.southwest(a));
  s.repeat("center", N, () => [r()], (a) => Rectangle.center(a));
  s.repeat("intersection", N * 2, () => [r(), r()], (a, b) => Rectangle.intersection(a, b));
  s.repeat("simpleIntersection", N, () => [r(), r()], (a, b) => Rectangle.simpleIntersection(a, b));
  s.repeat("union", N * 2, () => [r(), r()], (a, b) => Rectangle.union(a, b));
  s.repeat("expand", N, () => [r(), carto()], (a, c) => Rectangle.expand(a, c));
  s.repeat("contains", N * 2, () => [r(), carto()], (a, c) => Rectangle.contains(a, c));
  s.repeat("subsample", N, () => [r(), g.pick([0, 1000])], (a, h) => Rectangle.subsample(a, Ellipsoid.WGS84, h));
  s.repeat("subsection", N, () => {
    const [w, e] = [g.uniform(0, 1), g.uniform(0, 1)].sort((x, y) => x - y);
    const [so, n] = [g.uniform(0, 1), g.uniform(0, 1)].sort((x, y) => x - y);
    return [r(), w, so, e, n];
  }, (a, ...l) => Rectangle.subsection(a, ...l));
  s.add("subsection", [new Rectangle(0, 0, 1, 1), 1, 1, 1, 1], (a, ...l) => Rectangle.subsection(a, ...l));
  return s;
}

function projectionSuite() {
  const g = generators(makeRng(130));
  const s = suite("Projections");
  const geo = new GeographicProjection();
  const merc = new WebMercatorProjection();
  s.repeat("GeographicProjection.project", N, () => [g.lonLat()], (c) => geo.project(c));
  s.repeat("GeographicProjection.unproject", N, () => [g.c3(2e7)], (c) => geo.unproject(c));
  s.repeat("WebMercatorProjection.project", N, () => [g.lonLat()], (c) => merc.project(c));
  s.repeat("WebMercatorProjection.unproject", N, () => [g.c3(2e7)], (c) => merc.unproject(c));
  s.repeat("WebMercatorProjection.geodeticLatitudeToMercatorAngle", N, () => [g.uniform(-1.6, 1.6)], (l) => WebMercatorProjection.geodeticLatitudeToMercatorAngle(l));
  s.repeat("WebMercatorProjection.mercatorAngleToGeodeticLatitude", N, () => [g.uniform(-4, 4)], (l) => WebMercatorProjection.mercatorAngleToGeodeticLatitude(l));
  s.add("WebMercatorProjection.MaximumLatitude", [], () => WebMercatorProjection.MaximumLatitude);
  return s;
}

function boundingRectangleSuite() {
  const g = generators(makeRng(140));
  const s = suite("BoundingRectangle");
  const br = () => new BoundingRectangle(g.num(100), g.num(100), g.uniform(0, 100), g.uniform(0, 100));
  s.repeat("fromPoints", N, () => [Array.from({ length: 6 }, () => g.c2(100))], (p) => BoundingRectangle.fromPoints(p));
  s.add("fromPoints", [[]], (p) => BoundingRectangle.fromPoints(p));
  s.repeat("fromRectangle", N, () => [g.rectangle()], (r) => BoundingRectangle.fromRectangle(r));
  s.repeat("fromRectangleMercator", N, () => [g.rectangle()], (r) => BoundingRectangle.fromRectangle(r, new WebMercatorProjection()));
  s.repeat("union", N, () => [br(), br()], (a, b) => BoundingRectangle.union(a, b));
  s.repeat("expand", N, () => [br(), g.c2(150)], (a, p) => BoundingRectangle.expand(a, p));
  s.repeat("intersect", N, () => [br(), br()], (a, b) => BoundingRectangle.intersect(a, b));
  return s;
}

function boundingSphereSuite() {
  const g = generators(makeRng(150));
  const s = suite("BoundingSphere");
  const sp = () => g.sphere();
  s.repeat("fromPoints", N, () => [Array.from({ length: 1 + Math.floor(g.uniform(0, 12)) }, () => g.c3(1e4))], (p) => BoundingSphere.fromPoints(p));
  s.add("fromPoints", [[]], (p) => BoundingSphere.fromPoints(p));
  s.repeat("fromRectangle2D", N, () => [g.rectangle()], (r) => BoundingSphere.fromRectangle2D(r));
  s.repeat("fromRectangleWithHeights2D", N, () => [g.rectangle(), g.num(1e3), g.num(1e4)], (r, a, b) => BoundingSphere.fromRectangleWithHeights2D(r, undefined, a, b));
  s.repeat("fromRectangle3D", N, () => [g.rectangle(), g.pick([0, 500])], (r, h) => BoundingSphere.fromRectangle3D(r, Ellipsoid.WGS84, h));
  s.repeat("fromVertices", N, () => [Array.from({ length: 3 * (1 + Math.floor(g.uniform(0, 8))) }, () => g.num(1e3)), g.c3(1e3)], (p, c) => BoundingSphere.fromVertices(p, c, 3));
  s.repeat("fromEncodedCartesianVertices", N, () => {
    const n = 3 * (1 + Math.floor(g.uniform(0, 8)));
    return [Array.from({ length: n }, () => g.num(1e6)), Array.from({ length: n }, () => g.num(1))];
  }, (h, l) => BoundingSphere.fromEncodedCartesianVertices(h, l));
  s.repeat("fromCornerPoints", N, () => [g.c3(1e3), g.c3(1e3)], (a, b) => BoundingSphere.fromCornerPoints(a, b));
  s.repeat("fromBoundingSpheres", N, () => [Array.from({ length: Math.floor(g.uniform(0, 6)) }, sp)], (a) => BoundingSphere.fromBoundingSpheres(a));
  s.repeat("fromTransformation", N, () => [g.affine4()], (m) => BoundingSphere.fromTransformation(m));
  s.repeat("union", N * 2, () => {
    const a = sp();
    return [a, g.pick([sp(), new BoundingSphere(a.center, a.radius / 2), new BoundingSphere(a.center, a.radius * 2)])];
  }, (a, b) => BoundingSphere.union(a, b));
  s.repeat("expand", N, () => [sp(), g.c3(1e6)], (a, p) => BoundingSphere.expand(a, p));
  s.repeat("intersectPlane", N, () => [sp(), g.plane()], (a, p) => BoundingSphere.intersectPlane(a, p));
  s.repeat("transform", N, () => [sp(), g.affine4()], (a, m) => BoundingSphere.transform(a, m));
  s.repeat("distanceSquaredTo", N, () => [sp(), g.c3(1e6)], (a, p) => BoundingSphere.distanceSquaredTo(a, p));
  s.repeat("transformWithoutScale", N, () => [sp(), g.affine4()], (a, m) => BoundingSphere.transformWithoutScale(a, m));
  s.repeat("computePlaneDistances", N, () => [sp(), g.c3(1e6), g.unit3()], (a, p, d) => BoundingSphere.computePlaneDistances(a, p, d));
  s.repeat("projectTo2D", N, () => [new BoundingSphere(g.surface(), g.uniform(1, 1e5))], (a) => BoundingSphere.projectTo2D(a));
  s.add("projectTo2D", [new BoundingSphere(Cartesian3.ZERO, 1e5)], (a) => BoundingSphere.projectTo2D(a));
  s.repeat("volume", N, () => [sp()], (a) => a.volume());
  return s;
}

function planeSuite() {
  const g = generators(makeRng(160));
  const s = suite("Plane");
  s.repeat("fromPointNormal", N, () => [g.c3(1e3), g.unit3()], (p, n) => Plane.fromPointNormal(p, n));
  s.repeat("fromCartesian4", N, () => {
    const n = g.unit3();
    return [new Cartesian4(n.x, n.y, n.z, g.num(100))];
  }, (c) => Plane.fromCartesian4(c));
  s.repeat("getPointDistance", N, () => [g.plane(), g.c3(1e5)], (p, x) => Plane.getPointDistance(p, x));
  s.repeat("projectPointOntoPlane", N, () => [g.plane(), g.c3(1e5)], (p, x) => Plane.projectPointOntoPlane(p, x));
  s.repeat("transform", N, () => [g.plane(), g.affine4()], (p, m) => Plane.transform(p, m));
  return s;
}

function transformsSuite() {
  const g = generators(makeRng(170));
  const s = suite("Transforms");
  const origin = () => g.pick([g.surface(), g.surface(), Cartesian3.ZERO, new Cartesian3(0, 0, 6e6), new Cartesian3(0, 0, -6e6)]);
  const axes = ["east", "north", "up", "west", "south", "down"];
  for (const a of axes) {
    for (const b of axes) {
      let fn;
      try {
        fn = Transforms.localFrameToFixedFrameGenerator(a, b);
      } catch {
        s.add("localFrameToFixedFrame", [a, b, Cartesian3.UNIT_X], () => {
          throw new Error("invalid axes");
        });
        continue;
      }
      for (let i = 0; i < 4; i++) s.add("localFrameToFixedFrame", [a, b, origin()], (x, y, o) => fn(o));
    }
  }
  s.repeat("eastNorthUpToFixedFrame", N, () => [origin()], (o) => Transforms.eastNorthUpToFixedFrame(o));
  s.repeat("headingPitchRollToFixedFrame", N, () => [origin(), g.hpr()], (o, h) => Transforms.headingPitchRollToFixedFrame(o, h));
  s.repeat("headingPitchRollQuaternion", N, () => [g.surface(), g.hpr()], (o, h) => Transforms.headingPitchRollQuaternion(o, h));
  s.repeat("fixedFrameToHeadingPitchRoll", N, () => [Transforms.headingPitchRollToFixedFrame(g.surface(), g.hpr())], (m) => Transforms.fixedFrameToHeadingPitchRoll(m));
  s.add("fixedFrameToHeadingPitchRoll", [Matrix4.IDENTITY], (m) => Transforms.fixedFrameToHeadingPitchRoll(m));
  s.repeat("pointToWindowCoordinates", N, () => {
    const proj = Matrix4.computePerspectiveFieldOfView(1, 1.5, 1, 1e5, new Matrix4());
    const view = Matrix4.computeView(g.c3(100), g.unit3(), g.unit3(), g.unit3(), new Matrix4());
    const vp = Matrix4.computeViewportTransformation(new BoundingRectangle(0, 0, 1920, 1080), 0, 1);
    return [Matrix4.multiply(proj, view, new Matrix4()), vp, g.c3(1e3)];
  }, (mvp, vp, p) => [Transforms.pointToWindowCoordinates(mvp, vp, p), Transforms.pointToGLWindowCoordinates(mvp, vp, p)]);
  s.repeat("rotationMatrixFromPositionVelocity", N, () => [g.surface(), g.nonZero3(1e3)], (p, v) => Transforms.rotationMatrixFromPositionVelocity(p, v));
  s.repeat("basisTo2D", N, () => [g.pick([g.rigid4(), Matrix4.fromRotation(g.rotation3())])], (m) => Transforms.basisTo2D(new GeographicProjection(), m, new Matrix4()));
  s.repeat("ellipsoidTo2DModelMatrix", N, () => [g.surface()], (c) => Transforms.ellipsoidTo2DModelMatrix(new GeographicProjection(), c, new Matrix4()));
  return s;
}

function colorSuite() {
  const g = generators(makeRng(180));
  const s = suite("Color");
  const c = () => g.color();
  const css = [
    "red", " Blue ", "TRANSPARENT", "grey", "#f00", "#F00A", "#ff0000", "#00ff0080", "#12345", "#ggg",
    "rgb(255, 0, 0)", "rgba(10,20,30,0.5)", "rgb(100%, 50%, 0%)", "RGB(1 2 3)", "rgb(1 2 3 / 0.25)",
    "rgb(1,2,3,)", "rgb(1.5.5, 2, 3)", "rgb(.5, 2., 3)", "hsl(120, 100%, 50%)", "hsla(240, 50%, 25%, 0.3)",
    "hsl(120 100% 50%)", "hsl(120, 100, 50%)", "hsl(400, 20%, 70%)", "rgba(1, 2, 3, 50%)", "nonsense",
    "rgb( 1 , 2 , 3 )", "rgb(1,2,3 )", "rgb (1,2,3)", "rgba(1, 2, 3, .5)", "#ABCDEF", "#abcdef12",
  ];
  for (const str of css) s.add("fromCssColorString", [str], (x) => Color.fromCssColorString(x));
  s.repeat("fromBytes", N, () => [g.uniform(0, 255), g.uniform(0, 255), g.uniform(0, 255), g.uniform(0, 255)], (...a) => Color.fromBytes(...a));
  s.repeat("fromRgba", N, () => [Math.floor(g.uniform(0, 4294967295))], (v) => Color.fromRgba(v));
  s.repeat("fromHsl", N * 2, () => [g.uniform(-0.5, 1.5), g.pick([0, g.uniform(0, 1)]), g.uniform(0, 1), g.uniform(0, 1)], (...a) => Color.fromHsl(...a));
  s.repeat("floatToByte", N, () => [g.pick([g.uniform(-0.5, 1.5), 1, 0, 0.999999])], (v) => Color.floatToByte(v));
  s.repeat("toCssColorString", N, () => [g.pick([c(), new Color(g.rng(), g.rng(), g.rng(), 1)])], (x) => x.toCssColorString());
  s.add("toCssColorString", [new Color(0.1, 0.2, 0.3, 1e-8)], (x) => x.toCssColorString());
  s.repeat("toCssHexString", N, () => [g.pick([c(), new Color(g.rng(), g.rng(), g.rng(), 1), new Color(0.01, 0.02, 0, 0.03)])], (x) => x.toCssHexString());
  s.repeat("toBytes", N, () => [c()], (x) => x.toBytes());
  s.repeat("toRgba", N, () => [c()], (x) => x.toRgba());
  s.repeat("bytesToRgba", N, () => [Math.floor(g.uniform(-10, 300)), Math.floor(g.uniform(0, 256)), Math.floor(g.uniform(0, 256)), Math.floor(g.uniform(0, 256))], (...a) => Color.bytesToRgba(...a));
  s.repeat("brighten", N, () => [c(), g.uniform(0, 1)], (x, m) => x.brighten(m, new Color()));
  s.repeat("darken", N, () => [c(), g.uniform(0, 1)], (x, m) => x.darken(m, new Color()));
  s.repeat("add", N, () => [c(), c()], (a, b) => Color.add(a, b, new Color()));
  s.repeat("subtract", N, () => [c(), c()], (a, b) => Color.subtract(a, b, new Color()));
  s.repeat("multiply", N, () => [c(), c()], (a, b) => Color.multiply(a, b, new Color()));
  s.repeat("divide", N, () => [c(), c()], (a, b) => Color.divide(a, b, new Color()));
  s.repeat("mod", N, () => [c(), c()], (a, b) => Color.mod(a, b, new Color()));
  s.repeat("lerp", N, () => [c(), c(), g.uniform(0, 1)], (a, b, t) => Color.lerp(a, b, t, new Color()));
  s.repeat("multiplyByScalar", N, () => [c(), g.num(10)], (a, k) => Color.multiplyByScalar(a, k, new Color()));
  s.repeat("divideByScalar", N, () => [c(), g.num(10)], (a, k) => Color.divideByScalar(a, k, new Color()));
  for (const seed of [3, 77]) {
    s.add("fromRandomSeeded", [seed], (seedValue) => {
      CesiumMath.setRandomNumberSeed(seedValue);
      return [
        Color.fromRandom(),
        Color.fromRandom({ minimumRed: 0.5, maximumRed: 0.6, alpha: 1.0 }),
        Color.fromRandom({ red: 0.1, green: 0.2, blue: 0.3, minimumAlpha: 0.9 }),
      ];
    });
  }
  const named = Object.keys(Color).filter((k) => /^[A-Z_]+$/.test(k) && Color[k] instanceof Color);
  s.add("named", [named], (names) => names.map((n) => Color[n]));
  return s;
}

export default function coreGeometry() {
  return [
    ellipsoidSuite(),
    cartographicSuite(),
    rectangleSuite(),
    projectionSuite(),
    boundingRectangleSuite(),
    boundingSphereSuite(),
    planeSuite(),
    transformsSuite(),
    colorSuite(),
  ];
}
