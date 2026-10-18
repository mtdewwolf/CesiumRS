// Terrain vertex layouts, quantization thresholds, codecs and buffer conversion.
import { AttributeCompression, AxisAlignedBoundingBox, Cartesian2, Cartesian3, Ellipsoid, HeightmapTessellator, Matrix4, Rectangle, TerrainEncoding, Transforms, WebMercatorProjection } from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

const c3 = p => new Cartesian3(...p);
const create = o => new TerrainEncoding(
  o.center ? c3(o.center) : undefined,
  o.box ? new AxisAlignedBoundingBox(c3(o.box[0]), c3(o.box[1])) : undefined,
  o.heights?.[0], o.heights?.[1], o.frame ? Matrix4.fromArray(o.frame) : undefined,
  o.normals, o.merc, o.geo, o.exaggeration, o.relative,
);
const metadata = e => ({
  quantization: e.quantization, minimumHeight: e.minimumHeight ?? null, maximumHeight: e.maximumHeight ?? null,
  center: e.center ?? null, toScaledENU: e.toScaledENU ?? null, fromScaledENU: e.fromScaledENU ?? null, matrix: e.matrix ?? null,
  hasVertexNormals: e.hasVertexNormals, hasWebMercatorT: e.hasWebMercatorT, hasGeodeticSurfaceNormals: e.hasGeodeticSurfaceNormals,
  exaggeration: e.exaggeration, exaggerationRelativeHeight: e.exaggerationRelativeHeight, stride: e.stride,
  attributes: e.getAttributes(new Float32Array()).map(a => [a.index, a.componentDatatype, a.componentsPerAttribute, a.offsetInBytes, a.strideInBytes]),
  locations: e.getAttributeLocations(),
});
const encode = (e, vertices) => {
  const buffer = new Float32Array(e.stride * vertices.length);
  let i = 0;
  for (const [p, uv, height, oct, merc, geo] of vertices) i = e.encode(buffer, i, c3(p), new Cartesian2(...uv), height, new Cartesian2(...oct), merc, c3(geo));
  return buffer;
};
const decode = (e, b) => Array.from({ length: b.length / e.stride }, (_, i) => ({
  position: e.decodePosition(b, i), uv: e.decodeTextureCoordinates(b, i), height: e.decodeHeight(b, i),
  merc: e.hasWebMercatorT ? e.decodeWebMercatorT(b, i) : null,
  oct: e.hasVertexNormals ? e.getOctEncodedNormal(b, i) : null,
  normal: e.hasVertexNormals ? e.decodeNormal(b, i, new Cartesian3()) : null,
  geo: e.hasGeodeticSurfaceNormals ? e.decodeGeodeticSurfaceNormal(b, i, new Cartesian3()) : null,
  exaggerated: e.getExaggeratedPosition(b, i),
}));

export default function coreTerrain() {
  const s = suite("TerrainEncoding");
  const g = generators(makeRng(1700));
  for (const o of [{}, { center: [0, 0, 0] }, { center: [1, 2, 3], frame: Array.from(Matrix4.IDENTITY) }]) {
    s.add("constructor", [o], o => metadata(create(o)));
  }
  for (let i = 0; i < 80; i++) {
    const group = Math.floor(i / 8), flags = i % 8;
    const origin = Cartesian3.fromDegrees(g.uniform(-170, 170), g.uniform(-80, 80));
    const frame = Transforms.eastNorthUpToFixedFrame(origin);
    const center = Cartesian3.add(origin, g.c3(10), new Cartesian3());
    const width = [1000, 20000, 4094.999, 4095, 4095.001, 500, 500, 500, 500, g.uniform(200, 5000)][group];
    const min = new Cartesian3(-100, -200, -50);
    const max = new Cartesian3(min.x + width, min.y + width * 0.8, min.z + width * 0.2);
    const minimum = -100, maximum = group === 5 ? 10000 : group === 6 ? minimum : 1500;
    const o = {
      center: [center.x, center.y, center.z], box: group === 7 ? null : [[min.x, min.y, min.z], [max.x, max.y, max.z]],
      heights: [minimum, maximum], frame: group === 8 ? null : Array.from(frame),
      normals: Boolean(flags & 1), merc: Boolean(flags & 2), geo: Boolean(flags & 4),
      exaggeration: g.pick([0, 1, 2.5]), relative: g.uniform(-100, 100),
    };
    const vertices = Array.from({ length: 6 }, (_, j) => {
      const t = j < 3 ? [-0.1, 0.5, 1.1][j] : g.rng();
      const local = new Cartesian3(min.x + width * t, min.y + width * 0.8 * t, min.z + width * 0.2 * t);
      const p = Matrix4.multiplyByPoint(frame, local, new Cartesian3());
      const normal = AttributeCompression.octEncode(g.unit3(), new Cartesian2());
      const geo = Ellipsoid.WGS84.geodeticSurfaceNormal(p, new Cartesian3());
      return [[p.x, p.y, p.z], [g.rng(), g.rng()], minimum + (maximum - minimum) * t, [normal.x, normal.y], g.rng(), [geo.x, geo.y, geo.z]];
    });
    s.add("codec", [o, vertices], (o, v) => {
      const e = create(o), b = encode(e, v);
      return [metadata(e), Array.from(b), decode(e, b), metadata(TerrainEncoding.clone(e))];
    });
    if (!o.geo) {
      s.add("surfaceNormals", [o, vertices], (o, v) => {
        const e = create(o), old = encode(e, v);
        const b = new Float32Array((e.stride + 3) * v.length);
        e.addGeodeticSurfaceNormals(old, b, Ellipsoid.WGS84);
        const added = [metadata(e), Array.from(b), decode(e, b)];
        const noOp = new Float32Array(b.length).fill(-7);
        e.addGeodeticSurfaceNormals(b, noOp, Ellipsoid.WGS84);
        const removed = new Float32Array((e.stride - 3) * v.length);
        e.removeGeodeticSurfaceNormals(b, removed);
        const noOpRemove = new Float32Array(removed.length).fill(-7);
        e.removeGeodeticSurfaceNormals(removed, noOpRemove);
        return [added, metadata(e), Array.from(removed), Array.from(noOp), Array.from(noOpRemove)];
      });
    }
  }
  // Height range independently selects the quantization boundary.
  for (const span of [4094.999, 4095, 4095.001]) {
    const o = { center: [0, 0, 0], box: [[0, 0, 0], [1, 1, 1]], heights: [0, span], frame: Array.from(Matrix4.IDENTITY) };
    s.add("constructor", [o], o => metadata(create(o)));
  }
  return [s, heightmapSuite()];
}

function heightmapSuite() {
  const s = suite("HeightmapTessellator");
  const g = generators(makeRng(1701));
  const types = { Int8Array, Uint8Array, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array };
  const run = o => {
    const result = HeightmapTessellator.computeVertices({
      ...o, heightmap: new types[o.type](o.data),
      nativeRectangle: new Rectangle(...o.native), rectangle: o.rectangle ? new Rectangle(...o.rectangle) : undefined,
      relativeToCenter: o.center ? c3(o.center) : undefined,
      ellipsoid: o.ellipsoid === "MOON" ? Ellipsoid.MOON : Ellipsoid.WGS84,
    });
    const box = result.orientedBoundingBox;
    return {
      vertices: Array.from(result.vertices), minimumHeight: result.minimumHeight, maximumHeight: result.maximumHeight,
      encoding: metadata(result.encoding), boundingSphere3D: result.boundingSphere3D,
      orientedBoundingBox: box ? { center: box.center, halfAxes: box.halfAxes } : null,
      occludeePointInScaledSpace: result.occludeePointInScaledSpace ?? null,
      decoded: decode(result.encoding, result.vertices),
    };
  };
  for (let i = 0; i < 48; i++) {
    const width = 2 + i % 5, height = 2 + Math.floor(i / 5) % 5;
    const ellipsoid = i % 7 === 0 ? Ellipsoid.MOON : Ellipsoid.WGS84;
    const west = g.uniform(-160, 150), south = g.uniform(-60, 60);
    const span = i % 2 === 0 ? 0.0005 : 2;
    const rect = Rectangle.fromDegrees(west, south, west + span, south + span);
    const geographic = i % 3 !== 0;
    const native = geographic ? [west, south, west + span, south + span] : [rect.west * ellipsoid.maximumRadius, WebMercatorProjection.geodeticLatitudeToMercatorAngle(rect.south) * ellipsoid.maximumRadius, rect.east * ellipsoid.maximumRadius, WebMercatorProjection.geodeticLatitudeToMercatorAngle(rect.north) * ellipsoid.maximumRadius];
    const packed = i % 4 === 0;
    const structure = packed ? { elementsPerHeight: 3, stride: 4, elementMultiplier: 256, isBigEndian: i % 8 === 0, heightScale: 0.0001, heightOffset: -1000 } : { heightScale: 0.5, heightOffset: -100 };
    const type = packed ? "Uint8Array" : Object.keys(types)[i % 8];
    const data = Array.from(new types[type](Array.from({ length: width * height * (structure.stride ?? 1) }, () => packed ? Math.floor(g.uniform(0, 256)) : g.uniform(-100, 1000))));
    const center = Cartesian3.fromRadians((rect.west + rect.east) * 0.5, (rect.south + rect.north) * 0.5, 0, ellipsoid);
    const o = { data, type, width, height, native, rectangle: i % 4 === 1 ? null : [rect.west, rect.south, rect.east, rect.north],
      center: i % 5 === 0 ? null : [center.x, center.y, center.z], isGeographic: geographic, ellipsoid: ellipsoid === Ellipsoid.MOON ? "MOON" : "WGS84",
      skirtHeight: i % 3 === 0 ? 0 : 30, includeWebMercatorT: i % 2 === 0,
      exaggeration: [1, 0, 2.5][i % 3], exaggerationRelativeHeight: -20, structure };
    s.add("computeVertices", [o], run);
  }
  for (const value of [0, 100000, -100000]) {
    for (const skirtHeight of [0, 50]) {
      s.add("computeVertices", [{ data: [value,value,value,value], type: "Float64Array", width: 2, height: 2,
        native: [10,20,10.001,20.001], skirtHeight, rectangle: null, center: null, isGeographic: true }], run);
    }
  }
  for (const type of Object.keys(types)) {
    s.add("computeVertices", [{ data: Array.from(new types[type]([-1,2,3,4])), type, width: 2, height: 2,
      native: [10,20,10.001,20.001], skirtHeight: 30, rectangle: null, center: null, isGeographic: true,
      includeWebMercatorT: true, exaggeration: 2 }], run);
  }
  return s;
}
