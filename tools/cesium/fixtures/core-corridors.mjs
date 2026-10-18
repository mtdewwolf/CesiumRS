// Corridor edges, corner fans, rounded caps and extruded outline buffers.
import { Cartesian3, CornerType, CorridorGeometry, CorridorOutlineGeometry, VertexFormat } from "@cesium/engine";
import CorridorGeometryLibrary from "@cesium/engine/Source/Core/CorridorGeometryLibrary.js";
import { generators, makeRng, suite } from "./lib.mjs";
import { geometryEnc } from "./core-geometries.mjs";
import { ellipsoids, pathArgs, formats } from "./core-lines.mjs";

const DEG = Math.PI / 180;
const c3 = p => new Cartesian3(...p);
const packed = (value, offset = 0) => {
  const a = new Array(value.packedLength + offset + 2).fill(-7);
  CorridorOutlineGeometry.pack(value, a, offset);
  return a;
};
const options = ([path, width, height, extruded, corner, granularity, name, offset]) => ({
  positions: path.map(c3), width, height,
  extrudedHeight: extruded ?? undefined, cornerType: corner,
  granularity: granularity ?? undefined, ellipsoid: ellipsoids[name],
  offsetAttribute: offset ?? undefined,
});
const fixed = (degrees, name = "WGS84") => degrees.map(([lon, lat, height = 0]) => {
  const p = Cartesian3.fromDegrees(lon, lat, height, ellipsoids[name]);
  return [p.x, p.y, p.z];
});

function librarySuite() {
  const s = suite("CorridorGeometryLibrary");
  const g = generators(makeRng(1600));
  for (let i = 0; i < 36; i++) {
    const name = i % 7 === 0 ? "MOON" : "WGS84";
    const path = pathArgs(g, { name, heights: false, cross: i % 5 === 0 });
    const a = [path, g.uniform(100, 80000), i % 3, g.uniform(0.5, 3) * DEG, name, i % 2 === 0];
    s.add("computePositions", a, (ps, width, cornerType, granularity, name, saveAttributes) => CorridorGeometryLibrary.computePositions({
      positions: ps.map(c3), width, cornerType, granularity, ellipsoid: ellipsoids[name], saveAttributes,
    }));
  }
  for (const [front, back] of [[0, null], [null, 8], [0, 8], [null, null]]) {
    s.add("addAttribute", [[3, -2, 7], front, back], (v, f, b) => {
      const a = new Array(12).fill(-7);
      CorridorGeometryLibrary.addAttribute(a, c3(v), f ?? undefined, b ?? undefined);
      return a;
    });
  }
  return s;
}

function outlineSuite() {
  const s = suite("CorridorOutlineGeometry");
  const g = generators(makeRng(1601));
  const add = a => s.add("outline", a, (...a) => {
    const v = new CorridorOutlineGeometry(options(a));
    const p = packed(v, 2); // createGeometry projects the inputs in place
    return [p, geometryEnc(CorridorOutlineGeometry.createGeometry(v))];
  });
  for (let i = 0; i < 60; i++) {
    const name = i % 8 === 0 ? "MOON" : "WGS84";
    const path = pathArgs(g, { name, repeat: i % 5 === 0, cross: i % 7 === 0 });
    const a = [path, g.uniform(100, 80000), g.uniform(-200, 1500), i % 2 ? null : g.uniform(-500, 2000), i % 3, i % 4 ? g.uniform(0.5, 3) * DEG : null, name, [null, 0, 1, 2][i % 4]];
    add(a);
    if (i < 8) {
      s.add("unpack", [packed(new CorridorOutlineGeometry(options(a)), 2)], a => packed(CorridorOutlineGeometry.unpack(a, 2), 2));
    }
  }
  const paths = [
    [[0, 0], [1, 0]],
    [[0, 0], [1, 0], [2, 0]],
    [[0, 0], [1, 0], [1, 1], [2, 1]],
    [[0, 0], [1, 0], [1, -1], [2, -1]],
    [[179, 10], [-179, 10], [-178, 11]],
    [[20, 85], [50, 85], [80, 86]],
  ];
  for (const path of paths) {
    for (const corner of [CornerType.ROUNDED, CornerType.MITERED, CornerType.BEVELED]) {
      add([fixed(path), 1000, 500, -200, corner, null, "WGS84", 1]);
    }
  }
  // Projection precedes duplicate removal, including equal lon/lat at different heights.
  add([fixed([[10, 30, 0], [10, 30, 3000], [11, 30, 2000]]), 1000, 0, null, 0, null, "WGS84", null]);
  for (const path of [[], fixed([[0, 0]]), fixed([[0, 0], [0, 0]])]) {
    add([path, 1000, 0, null, 0, null, "WGS84", null]);
  }
  for (const width of [0, -100]) {
    add([fixed([[0, 0], [1, 0]]), width, 0, null, 0, null, "WGS84", null]);
  }
  // Check 32-bit index selection without storing several megabytes of buffers.
  s.add("largeOutline", [], () => {
    const v = new CorridorOutlineGeometry({ positions: fixed([[0, 0], [20, 0]]).map(c3), width: 1000, granularity: 0.00001, cornerType: CornerType.MITERED });
    const g = CorridorOutlineGeometry.createGeometry(v);
    return [g.attributes.position.values.length / 3, g.indices.constructor.name, g.indices.length, Array.from(g.indices.slice(-8))];
  });
  return s;
}

export default function coreCorridors() {
  return [librarySuite(), outlineSuite(), filledSuite()];
}

const filledFormats = {
  ...formats,
  NONE: new VertexFormat(),
  NORMAL_ONLY: new VertexFormat({ normal: true }),
  POSITION_AND_COLOR: VertexFormat.POSITION_AND_COLOR,
};
const filledOptions = a => ({ ...options(a), vertexFormat: filledFormats[a[8]], shadowVolume: a[9] });
const packedFilled = v => {
  const a = new Array(v.packedLength + 4).fill(-7);
  CorridorGeometry.pack(v, a, 2);
  return a;
};
// Rust omits position attributes whose values are undefined in JavaScript.
const filledEnc = g => {
  if (g && g.attributes.position && !g.attributes.position.values) g.attributes.position = undefined;
  return geometryEnc(g);
};

function filledSuite() {
  const s = suite("CorridorGeometry");
  const g = generators(makeRng(1602));
  const names = Object.keys(filledFormats);
  const add = a => s.add("filled", a, (...a) => {
    const v = new CorridorGeometry(filledOptions(a));
    const p = packedFilled(v);
    return [p, filledEnc(CorridorGeometry.createGeometry(v))];
  });
  for (let i = 0; i < 80; i++) {
    const name = i % 8 === 0 ? "MOON" : "WGS84";
    const path = pathArgs(g, { name, repeat: i % 5 === 0, cross: i % 7 === 0 });
    const a = [path, g.uniform(100, 80000), g.uniform(-200, 1500), i % 2 ? null : g.uniform(-500, 2000), i % 3, i % 4 ? g.uniform(0.5, 3) * DEG : null, name, [null, 0, 1, 2][i % 4], names[i % names.length], false];
    add(a);
    s.add("properties", a, (...a) => {
      const v = new CorridorGeometry(filledOptions(a));
      return [v.rectangle, CorridorGeometry.computeRectangle(filledOptions(a)), v.textureCoordinateRotationPoints];
    });
    if (i < 10) s.add("filledUnpack", [packedFilled(new CorridorGeometry(filledOptions(a)))], a => packedFilled(CorridorGeometry.unpack(a, 2)));
    if (i < 12) {
      s.add("shadowVolume", a, (...a) => {
        const v = CorridorGeometry.createShadowVolume(new CorridorGeometry(filledOptions(a)),
          (gr, e) => -100 - gr * 1000 - e.maximumRadius / 1e6,
          (gr, e) => 400 + gr * 2000 + e.maximumRadius / 1e6);
        return [packedFilled(v), filledEnc(CorridorGeometry.createGeometry(v))];
      });
    }
  }
  for (const path of [
    [[0, 0], [1, 0]],
    [[0, 0], [1, 0], [2, 0]],
    [[0, 0], [1, 0], [1, 1], [2, 1]],
    [[0, 0], [1, 0], [1, -1], [2, -1]],
    [[179, 10], [-179, 10], [-178, 11]],
    [[20, 85], [50, 85], [80, 86]],
  ]) {
    for (const corner of [0, 1, 2]) {
      for (const extruded of [null, -200]) add([fixed(path), 1000, 500, extruded, corner, null, "WGS84", 1, "ALL", false]);
    }
  }
  for (const height of [500, 500.009, 500.011]) add([fixed([[10, 30, 0], [10, 30, 3000], [11, 30, 2000]]), 1000, 500, height, 0, null, "WGS84", 2, "ALL", false]);
  for (const path of [[], fixed([[0, 0]]), fixed([[0, 0], [0, 0]])]) {
    add([path, 1000, 0, null, 0, null, "WGS84", null, "ALL", false]);
    s.add("properties", [path, 1000, 0, null, 0, null, "WGS84", null, "ALL", false], (...a) => {
      const v = new CorridorGeometry(filledOptions(a));
      return [v.rectangle, CorridorGeometry.computeRectangle(filledOptions(a)), v.textureCoordinateRotationPoints];
    });
  }
  for (const width of [0, -100]) add([fixed([[0, 0], [1, 0]]), width, 0, null, 0, null, "WGS84", null, "ALL", false]);
  for (const format of ["POSITION_ONLY", "ALL", "POSITION_AND_ST"]) add([fixed([[0, 20], [1, 20], [1, 21]]), 1000, 1000, -1000, 0, null, "WGS84", 1, format, true]);
  // Exercise each format both flat and extruded; random choices above are
  // deliberately supplemented so format and extrusion are independent.
  for (const format of names) {
    for (const corner of [0, 1, 2]) {
      for (const extruded of [null, -100]) add([fixed([[0, 20], [1, 20], [1, 21]]), 1000, 500, extruded, corner, null, "WGS84", 2, format, false]);
    }
  }
  s.add("largeFilled", [], () => {
    const v = new CorridorGeometry({ positions: fixed([[0, 0], [20, 0]]).map(c3), width: 1000, granularity: 0.00001, cornerType: CornerType.MITERED, vertexFormat: VertexFormat.POSITION_ONLY });
    const g = CorridorGeometry.createGeometry(v);
    return [g.attributes.position.values.length / 3, g.indices.constructor.name, g.indices.length, Array.from(g.indices.slice(-12))];
  });
  return s;
}
