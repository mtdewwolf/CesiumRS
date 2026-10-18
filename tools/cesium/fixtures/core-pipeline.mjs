// Fixtures for GeometryPipeline and the geometry instance attributes. Input
// geometries travel as plain data (attributes in order, typed-array names
// kept) and are rebuilt into Cesium geometries before each call.

import * as Cesium from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

const {
  BoundingSphere,
  BoxGeometry,
  Cartesian3,
  Color,
  ColorGeometryInstanceAttribute,
  ComponentDatatype,
  CylinderOutlineGeometry,
  DistanceDisplayCondition,
  DistanceDisplayConditionGeometryInstanceAttribute,
  EllipsoidGeometry,
  GeographicProjection,
  Geometry,
  GeometryAttribute,
  GeometryInstance,
  GeometryPipeline,
  GeometryType,
  Matrix4,
  OffsetGeometryInstanceAttribute,
  PolylineGeometry,
  PrimitiveType,
  Quaternion,
  ShowGeometryInstanceAttribute,
  VertexFormat,
  WebMercatorProjection,
} = Cesium;

/** A geometry as plain data, attributes in iteration order. */
export function geomIn(g) {
  const attributes = [];
  for (const [name, a] of Object.entries(g.attributes)) {
    if (a && a.values) attributes.push([name, a.componentDatatype, a.componentsPerAttribute, a.normalize ?? false, a.values.constructor.name, Array.from(a.values)]);
  }
  return {
    attributes,
    indices: g.indices ? [g.indices.constructor.name, Array.from(g.indices)] : null,
    primitiveType: g.primitiveType,
    boundingSphere: g.boundingSphere ?? null,
    boundingSphereCV: g.boundingSphereCV ?? null,
    geometryType: g.geometryType ?? GeometryType.NONE,
    offsetAttribute: g.offsetAttribute ?? null,
  };
}

const array = (ctor, values) => (ctor === "Array" ? values.slice() : new globalThis[ctor](values));
const sphere = (b) => (b ? new BoundingSphere(new Cartesian3(b.center.x, b.center.y, b.center.z), b.radius) : undefined);

/** Rebuilds a Cesium geometry from `geomIn` data. */
function geomOut(d) {
  const attributes = {};
  for (const [name, datatype, cpa, normalize, ctor, values] of d.attributes) {
    attributes[name] = new GeometryAttribute({ componentDatatype: datatype, componentsPerAttribute: cpa, normalize, values: array(ctor, values) });
  }
  const g = new Geometry({
    attributes,
    indices: d.indices ? array(d.indices[0], d.indices[1]) : undefined,
    primitiveType: d.primitiveType,
    boundingSphere: sphere(d.boundingSphere),
    boundingSphereCV: sphere(d.boundingSphereCV),
    geometryType: d.geometryType,
    offsetAttribute: d.offsetAttribute ?? undefined,
  });
  return g;
}

/** Output geometry; plain arrays are reported as Float64 values and Uint32 indices. */
function enc(g) {
  if (!g) return null;
  const attributes = {};
  for (const [name, a] of Object.entries(g.attributes)) {
    if (a && a.values) {
      const ctor = Array.isArray(a.values) ? "Float64Array" : a.values.constructor.name;
      attributes[name] = [a.componentDatatype, a.componentsPerAttribute, a.normalize ?? false, ctor, Array.from(a.values)];
    }
  }
  return {
    attributes,
    indices: g.indices ? [Array.isArray(g.indices) ? "Uint32Array" : g.indices.constructor.name, Array.from(g.indices)] : null,
    primitiveType: g.primitiveType,
    boundingSphere: g.boundingSphere ?? null,
    boundingSphereCV: g.boundingSphereCV ?? null,
    geometryType: g.geometryType,
  };
}

/** A short digest of a large geometry: lengths, sums and the first values. */
function digest(g) {
  const attributes = {};
  for (const [name, a] of Object.entries(g.attributes)) {
    if (a && a.values) {
      const v = Array.from(a.values);
      attributes[name] = [v.length, v.reduce((s, x) => s + x, 0), v.slice(0, 12)];
    }
  }
  const idx = Array.from(g.indices ?? []);
  return { attributes, indices: [idx.length, idx.reduce((s, x) => s + x, 0), idx.slice(0, 24), idx.slice(-24)] };
}

const instanceEnc = (i) => ({
  geometry: enc(i.geometry),
  west: enc(i.westHemisphereGeometry),
  east: enc(i.eastHemisphereGeometry),
  modelMatrix: Array.from(Matrix4.toArray(i.modelMatrix)),
});

/** A lon/lat grid on WGS84 as triangles, with normals, st and custom attributes. */
function grid(g, lon0, lat0, span, n, { extras = true, lines = false } = {}) {
  const positions = [];
  const normals = [];
  const st = [];
  const color = [];
  const scalar = [];
  const applyOffset = [];
  for (let r = 0; r <= n; r++) {
    for (let c = 0; c <= n; c++) {
      const lon = lon0 + (span * c) / n;
      const lat = lat0 + (span * r) / n;
      const p = Cartesian3.fromDegrees(lon, lat, g.uniform(0, 1000));
      positions.push(p.x, p.y, p.z);
      const nrm = Cesium.Ellipsoid.WGS84.geodeticSurfaceNormal(p, new Cartesian3());
      normals.push(nrm.x, nrm.y, nrm.z);
      st.push(c / n, r / n);
      color.push(Math.floor(g.uniform(0, 256)), Math.floor(g.uniform(0, 256)), Math.floor(g.uniform(0, 256)), 255);
      scalar.push(g.uniform(-5, 5));
      applyOffset.push(g.rng() < 0.5 ? 1 : 0);
    }
  }
  const indices = [];
  for (let r = 0; r < n; r++) {
    for (let c = 0; c < n; c++) {
      const a = r * (n + 1) + c;
      if (lines) indices.push(a, a + 1, a, a + n + 1);
      else indices.push(a, a + 1, a + n + 1, a + 1, a + n + 2, a + n + 1);
    }
  }
  const attributes = {
    position: new GeometryAttribute({ componentDatatype: ComponentDatatype.DOUBLE, componentsPerAttribute: 3, values: new Float64Array(positions) }),
  };
  if (!lines) {
    attributes.normal = new GeometryAttribute({ componentDatatype: ComponentDatatype.FLOAT, componentsPerAttribute: 3, values: new Float32Array(normals) });
    attributes.st = new GeometryAttribute({ componentDatatype: ComponentDatatype.FLOAT, componentsPerAttribute: 2, values: new Float32Array(st) });
  }
  if (extras && !lines) {
    attributes.color = new GeometryAttribute({ componentDatatype: ComponentDatatype.UNSIGNED_BYTE, componentsPerAttribute: 4, normalize: true, values: new Uint8Array(color) });
    attributes.scalar = new GeometryAttribute({ componentDatatype: ComponentDatatype.FLOAT, componentsPerAttribute: 1, values: new Float32Array(scalar) });
  }
  if (extras) {
    attributes.applyOffset = new GeometryAttribute({ componentDatatype: ComponentDatatype.UNSIGNED_BYTE, componentsPerAttribute: 1, values: new Uint8Array(applyOffset) });
  }
  return new Geometry({
    attributes,
    indices: new Uint16Array(indices),
    primitiveType: lines ? PrimitiveType.LINES : PrimitiveType.TRIANGLES,
    boundingSphere: BoundingSphere.fromVertices(positions),
  });
}

/** Unindexed custom geometries for the strip, fan, and line primitives. */
function unindexed(g, primitiveType, n) {
  const positions = Array.from({ length: n * 3 }, () => g.uniform(-10, 10));
  return new Geometry({
    attributes: { position: new GeometryAttribute({ componentDatatype: ComponentDatatype.DOUBLE, componentsPerAttribute: 3, values: new Float64Array(positions) }) },
    primitiveType,
  });
}

function pipelineSuite() {
  const rng = makeRng(1300);
  const g = { ...generators(rng), rng };
  g.uniform = (lo, hi) => lo + (hi - lo) * rng();
  const s = suite("GeometryPipeline");

  const box = BoxGeometry.createGeometry(BoxGeometry.fromDimensions({ dimensions: new Cartesian3(2, 3, 4), vertexFormat: VertexFormat.ALL }));
  const boxPositions = BoxGeometry.createGeometry(BoxGeometry.fromDimensions({ dimensions: new Cartesian3(5, 1, 2), vertexFormat: VertexFormat.POSITION_ONLY }));
  const ellipsoid = EllipsoidGeometry.createGeometry(new EllipsoidGeometry({ radii: new Cartesian3(1e6, 2e6, 3e6), stackPartitions: 6, slicePartitions: 8, vertexFormat: VertexFormat.POSITION_NORMAL_AND_ST }));
  const cylinderOutline = CylinderOutlineGeometry.createGeometry(new CylinderOutlineGeometry({ length: 10, topRadius: 2, bottomRadius: 3, slices: 8 }));
  const bases = { box, boxPositions, ellipsoid, cylinderOutline };

  for (const [name, geom] of Object.entries(bases)) {
    const d = geomIn(geom);
    s.add("toWireframe", [d], (x) => enc(GeometryPipeline.toWireframe(geomOut(x))));
    s.add("createAttributeLocations", [d], (x) => Object.entries(GeometryPipeline.createAttributeLocations(geomOut(x))));
    s.add("reorderForPreVertexCache", [d], (x) => enc(GeometryPipeline.reorderForPreVertexCache(geomOut(x))));
    s.add("reorderForPostVertexCache", [d, null], (x) => enc(GeometryPipeline.reorderForPostVertexCache(geomOut(x))));
    s.add("reorderForPostVertexCache", [d, 6], (x, c) => enc(GeometryPipeline.reorderForPostVertexCache(geomOut(x), c)));
    s.add("fitToUnsignedShortIndices", [d], (x) => GeometryPipeline.fitToUnsignedShortIndices(geomOut(x)).map(enc));
    s.add("encodeAttribute", [d], (x) => enc(GeometryPipeline.encodeAttribute(geomOut(x), "position", "positionHigh", "positionLow")));
    s.add("compressVertices", [d], (x) => enc(GeometryPipeline.compressVertices(geomOut(x))));
    if (name !== "cylinderOutline") {
      s.add("computeNormal", [d], (x) => enc(GeometryPipeline.computeNormal(geomOut(x))));
    }
  }
  for (const d of [geomIn(box), geomIn(ellipsoid)]) {
    s.add("createLineSegmentsForVectors", [d, null, null], (x) => enc(GeometryPipeline.createLineSegmentsForVectors(geomOut(x))));
    s.add("createLineSegmentsForVectors", [d, "normal", 2.5], (x, a, l) => enc(GeometryPipeline.createLineSegmentsForVectors(geomOut(x), a, l)));
    s.add("computeTangentAndBitangent", [d], (x) => enc(GeometryPipeline.computeTangentAndBitangent(geomOut(x))));
  }
  // Vertex compression over each mix of normal, st, tangent and bitangent.
  for (const keep of [["normal"], ["st"], ["normal", "st"], ["normal", "tangent"], ["st", "bitangent"], ["normal", "tangent", "bitangent"], ["normal", "st", "tangent", "bitangent"]]) {
    const d = geomIn(box);
    d.attributes = d.attributes.filter(([n]) => n === "position" || keep.includes(n));
    s.add("compressVertices", [d], (x) => enc(GeometryPipeline.compressVertices(geomOut(x))));
  }
  {
    const d = geomIn(boxPositions);
    const n = d.attributes[0][5].length;
    const extrude = Array.from({ length: n }, (_, i) => (i % 9 < 3 ? 0 : g.uniform(-1, 1)));
    d.attributes.push(["extrudeDirection", ComponentDatatype.FLOAT, 3, false, "Float32Array", extrude]);
    s.add("compressVertices", [d], (x) => enc(GeometryPipeline.compressVertices(geomOut(x))));
  }

  // Indexed strips and fans for the wireframe conversions.
  for (const [type, n] of [[PrimitiveType.TRIANGLE_STRIP, 7], [PrimitiveType.TRIANGLE_FAN, 6], [PrimitiveType.TRIANGLE_STRIP, 2], [PrimitiveType.TRIANGLE_FAN, 0]]) {
    const geom = unindexed(g, type, Math.max(n, 3));
    geom.indices = new Uint16Array(Array.from({ length: n }, (_, i) => i));
    s.add("toWireframe", [geomIn(geom)], (x) => enc(GeometryPipeline.toWireframe(geomOut(x))));
  }

  // Projection and encoding on positions above the ellipsoid.
  for (const proj of ["geographic", "webMercator"]) {
    const d = geomIn(grid(g, 10, 20, 5, 3, { extras: false }));
    s.add("projectTo2D", [d, proj], (x, p) =>
      enc(GeometryPipeline.projectTo2D(geomOut(x), "position", "position3D", "position2D", p === "geographic" ? new GeographicProjection() : new WebMercatorProjection())),
    );
  }
  s.add("projectTo2D", [geomIn(grid(g, -50, -30, 8, 2, { extras: false })), null], (x) => enc(GeometryPipeline.projectTo2D(geomOut(x), "position", "position3D", "position2D")));

  // World transforms.
  for (let i = 0; i < 6; i++) {
    const d = geomIn(i % 2 ? box : ellipsoid);
    const m = i === 5 ? Matrix4.IDENTITY : Matrix4.fromTranslationQuaternionRotationScale(g.c3(1e5), Quaternion.normalize(new Quaternion(g.uniform(-1, 1), g.uniform(-1, 1), g.uniform(-1, 1), g.uniform(-1, 1)), new Quaternion()), new Cartesian3(g.uniform(0.5, 3), g.uniform(0.5, 3), g.uniform(0.5, 3)));
    const ma = Array.from(Matrix4.toArray(m));
    s.add("transformToWorldCoordinates", [d, ma], (x, mm) => instanceEnc(GeometryPipeline.transformToWorldCoordinates(new GeometryInstance({ geometry: geomOut(x), modelMatrix: Matrix4.fromArray(mm) }))));
  }

  // Combining instances.
  const combos = [
    [box, box],
    [box, ellipsoid],
    [boxPositions, box, ellipsoid],
    [cylinderOutline, cylinderOutline],
  ];
  for (const list of combos) {
    const ds = list.map(geomIn);
    s.add("combineInstances", [ds], (xs) => GeometryPipeline.combineInstances(xs.map((x) => new GeometryInstance({ geometry: geomOut(x) }))).map(enc));
  }
  {
    const noSphere = geomIn(box);
    noSphere.boundingSphere = null;
    s.add("combineInstances", [[geomIn(box), noSphere]], (xs) => GeometryPipeline.combineInstances(xs.map((x) => new GeometryInstance({ geometry: geomOut(x) }))).map(enc));
  }

  // Splitting at the antimeridian: grids across it, beside it, and lines.
  const splits = [
    grid(g, 175, -5, 10, 4),
    grid(g, 178, 30, 4, 3, { extras: false }),
    grid(g, 179.9, -2, 0.2, 2),
    grid(g, 10, 10, 5, 2),
    grid(g, -175, 0, 8, 2),
    grid(g, 176, -40, 8, 3, { lines: true }),
    grid(g, 176, 50, 8, 2, { lines: true, extras: false }),
  ];
  splits[1].geometryType = GeometryType.TRIANGLES;
  splits[5].geometryType = GeometryType.LINES;
  for (const geom of splits) {
    s.add("splitLongitude", [geomIn(geom)], (x) => instanceEnc(GeometryPipeline.splitLongitude(new GeometryInstance({ geometry: geomOut(x) }))));
  }
  {
    // Exactly on the antimeridian: the y = 0 nudges.
    const geom = grid(g, 180, -4, 8, 2);
    s.add("splitLongitude", [geomIn(geom)], (x) => instanceEnc(GeometryPipeline.splitLongitude(new GeometryInstance({ geometry: geomOut(x) }))));
  }
  for (const [lons, colors] of [[[170, 175, -175, -170], false], [[178, 180, -178], true], [[-170, 175, 170], true]]) {
    const positions = Cartesian3.fromDegreesArray(lons.flatMap((lon, i) => [lon, 10 + 3 * i]));
    const options = { positions, width: 5, vertexFormat: VertexFormat.ALL };
    if (colors) options.colors = positions.map((_, i) => Color.fromBytes(40 * i, 255 - 30 * i, 100, 255));
    const geom = PolylineGeometry.createGeometry(new PolylineGeometry(options));
    s.add("splitLongitude", [geomIn(geom)], (x) => instanceEnc(GeometryPipeline.splitLongitude(new GeometryInstance({ geometry: geomOut(x) }))));
  }
  // Unindexed primitives are indexed before splitting.
  for (const type of [PrimitiveType.TRIANGLE_STRIP, PrimitiveType.TRIANGLE_FAN, PrimitiveType.LINE_STRIP, PrimitiveType.LINE_LOOP]) {
    const pts = Cartesian3.fromDegreesArray([170, 0, -170, 1, 172, 3, -172, 4, 174, 6, -174, 7]);
    const geom = new Geometry({
      attributes: { position: new GeometryAttribute({ componentDatatype: ComponentDatatype.DOUBLE, componentsPerAttribute: 3, values: Cartesian3.packArray(pts, new Float64Array(18)) }) },
      primitiveType: type,
      boundingSphere: BoundingSphere.fromPoints(pts),
    });
    s.add("splitLongitude", [geomIn(geom)], (x) => instanceEnc(GeometryPipeline.splitLongitude(new GeometryInstance({ geometry: geomOut(x) }))));
  }

  // Splitting for 16-bit indices: a large ellipsoid, summarized.
  {
    const big = EllipsoidGeometry.createGeometry(new EllipsoidGeometry({ radii: new Cartesian3(1, 2, 3), stackPartitions: 260, slicePartitions: 260, vertexFormat: VertexFormat.POSITION_AND_NORMAL }));
    s.add("fitToUnsignedShortIndicesBig", [260], (n) => {
      const geom = EllipsoidGeometry.createGeometry(new EllipsoidGeometry({ radii: new Cartesian3(1, 2, 3), stackPartitions: n, slicePartitions: n, vertexFormat: VertexFormat.POSITION_AND_NORMAL }));
      return GeometryPipeline.fitToUnsignedShortIndices(geom).map(digest);
    });
    void big;
  }
  return s;
}

function attributeSuite() {
  const s = suite("GeometryInstanceAttributes");
  const colors = [[0.5, 0.25, 1, 1], [0, 0, 0, 0], [1.5, -0.2, 0.999, 0.004], [null, null, null, null]];
  for (const [r, gg, b, a] of colors) {
    s.add("color", [r, gg, b, a], (...c) => Array.from(new ColorGeometryInstanceAttribute(...c.map((x) => x ?? undefined)).value));
    if (r !== null) s.add("colorToValue", [r, gg, b, a], (...c) => Array.from(ColorGeometryInstanceAttribute.toValue(new Color(...c))));
  }
  for (const v of [true, false, null]) s.add("show", [v], (x) => Array.from(new ShowGeometryInstanceAttribute(x ?? undefined).value));
  for (const v of [[1.5, -2.25, 1e10], [null, 3, null], [0.1, 0.2, 0.3]]) s.add("offset", [v], (x) => Array.from(new OffsetGeometryInstanceAttribute(...x.map((y) => y ?? undefined)).value));
  for (const v of [[10, 1e6], [null, null], [0.1, 1e40]]) {
    s.add("distanceDisplayCondition", [v], (x) => Array.from(new DistanceDisplayConditionGeometryInstanceAttribute(...x.map((y) => y ?? undefined)).value));
  }
  s.add("distanceDisplayConditionToValue", [[5, 7.3]], ([n, f]) => Array.from(DistanceDisplayConditionGeometryInstanceAttribute.toValue(new DistanceDisplayCondition(n, f))));
  return s;
}

export default function corePipeline() {
  return [pipelineSuite(), attributeSuite()];
}
