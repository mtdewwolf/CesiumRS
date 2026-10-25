// CesiumJS side of the parity benchmark: the same workloads, inputs, calls and
// checksums as examples/parity_bench/workloads.rs. Runs in Node (node.mjs) and
// in Chromium (browser.mjs), so nothing here may import Node-only modules.
//
// Fixture workloads decode `tests/fixtures` arguments with the generators' own
// helpers and check the timed call's output against the recorded result. Hot
// loops pass Cesium's `result` scratch objects where the API takes one, which
// is how performance-minded CesiumJS code is written.

import {
  BoundingSphere,
  Cartesian3,
  Cartographic,
  CornerType,
  CorridorGeometry,
  Ellipsoid,
  EllipsoidGeodesic,
  HeightmapTessellator,
  Matrix4,
  PolygonGeometry,
  Rectangle,
  Transforms,
  VertexFormat,
} from "@cesium/engine";
import earcut from "earcut";
import { enc } from "../fixtures/lib.mjs";
import { geometryEnc } from "../fixtures/core-geometries.mjs";
import { polygonOptions } from "../fixtures/core-surfaces.mjs";
import { filledOptions } from "../fixtures/core-corridors.mjs";
import { EXACT, GEOMETRY, TIGHT, close, dec } from "./close.mjs";

const E = Ellipsoid.WGS84;
const FRACTIONS = [0, 0.25, 0.5, 0.9, 1];

async function loadJson(source, path) {
  return JSON.parse(await source.text(path));
}

/** Cases of one fixture function, without recorded throws (both sides skip them). */
async function fixtureCases(source, path, f) {
  const doc = await loadJson(source, path);
  const cases = doc.cases.filter((c) => c.f === f && !(c.r && typeof c.r === "object" && "throws" in c.r));
  if (cases.length === 0) throw new Error(`${path}: no ${f} cases`);
  return cases.map((c) => ({ a: dec(c.a), r: c.r }));
}

function check(pairs, tol) {
  const mismatches = [];
  let checked = 0;
  pairs.forEach(([got, expected], i) => {
    checked++;
    if (!close(got, expected, tol)) mismatches.push(`case ${i}: got ${JSON.stringify(got).slice(0, 160)}`);
  });
  return { checked, mismatches };
}

const CHECKSUM_ONLY = () => ({ checked: 0, mismatches: [] });

function sum(values) {
  let s = 0;
  for (let i = 0; i < values.length; i++) s += values[i];
  return s;
}

/** Sum of the position values plus the index count (0 for no geometry), as geometry_checksum. */
function geometryChecksum(g) {
  if (!g) return 0;
  const values = g.attributes.position?.values;
  return (values ? sum(values) : 0) + (g.indices ? g.indices.length : 0);
}

/** Rust omits position attributes whose values are undefined in JavaScript (core-corridors.mjs). */
function filledEnc(g) {
  if (g && g.attributes.position && !g.attributes.position.values) g.attributes.position = undefined;
  return geometryEnc(g);
}

const TYPES = { Int8Array, Uint8Array, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array };

/** HeightmapTessellator options from the fixture's plain-data form (core-terrain.mjs). */
function heightmapOptions(o) {
  return {
    ...o,
    heightmap: new TYPES[o.type](o.data),
    nativeRectangle: new Rectangle(...o.native),
    rectangle: o.rectangle ? new Rectangle(...o.rectangle) : undefined,
    relativeToCenter: o.center ? new Cartesian3(...o.center) : undefined,
    ellipsoid: o.ellipsoid === "MOON" ? Ellipsoid.MOON : Ellipsoid.WGS84,
  };
}

function heightmapChecksum(options) {
  const r = HeightmapTessellator.computeVertices(options);
  return r ? sum(r.vertices) + r.minimumHeight + r.maximumHeight : 0;
}

function largeCorridor() {
  return new CorridorGeometry({
    positions: [Cartesian3.fromDegrees(0, 0, 0), Cartesian3.fromDegrees(20, 0, 0)],
    width: 1000,
    granularity: 0.00001,
    cornerType: CornerType.MITERED,
    vertexFormat: VertexFormat.POSITION_ONLY,
  });
}

function base64Bytes(text) {
  if (typeof Buffer !== "undefined") return new Uint8Array(Buffer.from(text, "base64"));
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export const workloads = [
  {
    id: "core.cartographicToCartesian",
    group: "Core math",
    title: "Ellipsoid.cartographicToCartesian",
    input: "tests/fixtures/core/Ellipsoid.json",
    async prepare(source) {
      const cases = await fixtureCases(source, this.input, "cartographicToCartesian");
      const inputs = cases.map((c) => new Cartographic(...c.a[0]));
      const scratch = new Cartesian3();
      return {
        cases: inputs.length,
        run() {
          let s = 0;
          for (const c of inputs) {
            const p = E.cartographicToCartesian(c, scratch);
            s += p.x + p.y + p.z;
          }
          return s;
        },
        verify: () => check(inputs.map((c, i) => [enc(E.cartographicToCartesian(c)), cases[i].r]), TIGHT),
      };
    },
  },
  {
    id: "core.cartesianToCartographic",
    group: "Core math",
    title: "Ellipsoid.cartesianToCartographic",
    input: "tests/fixtures/core/Ellipsoid.json",
    async prepare(source) {
      const cases = await fixtureCases(source, this.input, "cartesianToCartographic");
      const inputs = cases.map((c) => new Cartesian3(...c.a[0]));
      const scratch = new Cartographic();
      return {
        cases: inputs.length,
        run() {
          let s = 0;
          for (const p of inputs) {
            const c = E.cartesianToCartographic(p, scratch);
            if (c !== undefined) s += c.longitude + c.latitude + c.height;
          }
          return s;
        },
        verify: () => check(inputs.map((p, i) => [enc(E.cartesianToCartographic(p)), cases[i].r]), TIGHT),
      };
    },
  },
  {
    id: "core.ellipsoidGeodesic",
    group: "Core math",
    title: "EllipsoidGeodesic + 5 interpolations",
    input: "tests/fixtures/core/Curves.json",
    async prepare(source) {
      const cases = await fixtureCases(source, this.input, "EllipsoidGeodesic");
      const inputs = cases.map((c) => [new Cartographic(...c.a[0]), new Cartographic(...c.a[1])]);
      const scratch = new Cartographic();
      return {
        cases: inputs.length,
        run() {
          let s = 0;
          for (const [a, b] of inputs) {
            const g = new EllipsoidGeodesic(a, b, E);
            s += g.surfaceDistance + g.startHeading + g.endHeading;
            for (const t of FRACTIONS) {
              const p = g.interpolateUsingFraction(t, scratch);
              s += p.longitude + p.latitude;
            }
          }
          return s;
        },
        verify: () =>
          check(
            inputs.map(([a, b], i) => {
              const g = new EllipsoidGeodesic(a, b, E);
              return [enc([g.surfaceDistance, g.startHeading, g.endHeading, ...FRACTIONS.map((f) => g.interpolateUsingFraction(f))]), cases[i].r];
            }),
            TIGHT,
          ),
      };
    },
  },
  {
    id: "core.eastNorthUpToFixedFrame",
    group: "Core math",
    title: "Transforms.eastNorthUpToFixedFrame",
    input: "tests/fixtures/core/Transforms.json",
    async prepare(source) {
      const cases = await fixtureCases(source, this.input, "eastNorthUpToFixedFrame");
      const inputs = cases.map((c) => new Cartesian3(...c.a[0]));
      const scratch = new Matrix4();
      return {
        cases: inputs.length,
        run() {
          let s = 0;
          for (const p of inputs) {
            const m = Transforms.eastNorthUpToFixedFrame(p, E, scratch);
            for (let k = 0; k < 16; k++) s += m[k];
          }
          return s;
        },
        verify: () => check(inputs.map((p, i) => [enc(Transforms.eastNorthUpToFixedFrame(p, E)), cases[i].r]), TIGHT),
      };
    },
  },
  {
    id: "core.boundingSphereFromPoints",
    group: "Core math",
    title: "BoundingSphere.fromPoints",
    input: "tests/fixtures/core/BoundingSphere.json",
    async prepare(source) {
      const cases = await fixtureCases(source, this.input, "fromPoints");
      const inputs = cases.map((c) => c.a[0].map((p) => new Cartesian3(...p)));
      const scratch = new BoundingSphere();
      return {
        cases: inputs.length,
        run() {
          let s = 0;
          for (const points of inputs) {
            const b = BoundingSphere.fromPoints(points, scratch);
            s += b.center.x + b.center.y + b.center.z + b.radius;
          }
          return s;
        },
        verify: () => check(inputs.map((p, i) => [enc(BoundingSphere.fromPoints(p)), cases[i].r]), TIGHT),
      };
    },
  },
  {
    id: "geometry.earcut",
    group: "Geometry",
    title: "earcut triangulation",
    input: "tests/fixtures/core/Earcut.json",
    async prepare(source) {
      const cases = await fixtureCases(source, this.input, "earcut");
      const inputs = cases.map((c) => c.a);
      return {
        cases: inputs.length,
        run() {
          let s = 0;
          for (const [data, holes, dim] of inputs) s += sum(earcut(data, holes, dim));
          return s;
        },
        verify: () => check(inputs.map(([d, h, k], i) => [enc(earcut(d, h, k)), cases[i].r]), EXACT),
      };
    },
  },
  {
    id: "geometry.polygon",
    group: "Geometry",
    title: "PolygonGeometry.createGeometry",
    input: "tests/fixtures/core/PolygonGeometry.json",
    async prepare(source) {
      const cases = await fixtureCases(source, this.input, "polygon");
      // createGeometry scales the hierarchy in place, so each pass rebuilds the options.
      const create = (a) => PolygonGeometry.createGeometry(new PolygonGeometry(polygonOptions(a)));
      return {
        cases: cases.length,
        run() {
          let s = 0;
          for (const c of cases) s += geometryChecksum(create(c.a));
          return s;
        },
        verify: () => check(cases.map((c) => [enc(geometryEnc(create(c.a))), c.r[3]]), GEOMETRY),
      };
    },
  },
  {
    id: "geometry.corridor",
    group: "Geometry",
    title: "CorridorGeometry.createGeometry",
    input: "tests/fixtures/core/CorridorGeometry.json",
    async prepare(source) {
      const cases = await fixtureCases(source, this.input, "filled");
      // createGeometry projects the inputs in place, so each pass rebuilds the options.
      const create = (a) => CorridorGeometry.createGeometry(new CorridorGeometry(filledOptions(a)));
      return {
        cases: cases.length,
        run() {
          let s = 0;
          for (const c of cases) s += geometryChecksum(create(c.a));
          return s;
        },
        verify: () => check(cases.map((c) => [enc(filledEnc(create(c.a))), c.r[1]]), GEOMETRY),
      };
    },
  },
  {
    id: "geometry.corridorLarge",
    group: "Geometry",
    title: "CorridorGeometry, 20° at 0.00001 rad granularity",
    input: "tests/fixtures/core/CorridorGeometry.json",
    async prepare(source) {
      const [c] = await fixtureCases(source, this.input, "largeFilled");
      return {
        cases: 1,
        run: () => geometryChecksum(CorridorGeometry.createGeometry(largeCorridor())),
        verify() {
          const g = CorridorGeometry.createGeometry(largeCorridor());
          const got = [g.attributes.position.values.length / 3, g.indices.constructor.name, g.indices.length, Array.from(g.indices.slice(-12))];
          return check([[enc(got), c.r]], EXACT);
        },
      };
    },
  },
  {
    id: "terrain.heightmapFixtures",
    group: "Terrain",
    title: "HeightmapTessellator.computeVertices, fixture grids (2x2 to 6x6)",
    input: "tests/fixtures/core/HeightmapTessellator.json",
    async prepare(source) {
      const cases = await fixtureCases(source, this.input, "computeVertices");
      const inputs = cases.map((c) => heightmapOptions(c.a[0]));
      return {
        cases: inputs.length,
        run() {
          let s = 0;
          for (const o of inputs) s += heightmapChecksum(o);
          return s;
        },
        verify: () =>
          check(
            inputs.map((o, i) => {
              const r = HeightmapTessellator.computeVertices(o);
              const { vertices, minimumHeight, maximumHeight } = cases[i].r;
              return [enc({ vertices: Array.from(r.vertices), minimumHeight: r.minimumHeight, maximumHeight: r.maximumHeight }), { vertices, minimumHeight, maximumHeight }];
            }),
            TIGHT,
          ),
      };
    },
  },
  ...[65, 257].map((size) => ({
    id: `terrain.heightmap${size}`,
    group: "Terrain",
    title: `HeightmapTessellator.computeVertices, ${size}x${size} Uint16 tile`,
    input: `target/bench-inputs/heightmap${size}.json`,
    async prepare(source) {
      const doc = await loadJson(source, this.input);
      const options = heightmapOptions(doc.options);
      return { cases: 1, run: () => heightmapChecksum(options), verify: CHECKSUM_ONLY };
    },
  })),
  ...[
    ["5k", "5,000"],
    ["130k", "130,050"],
  ].map(([name, triangles]) => ({
    id: `workers.dracoMesh${name}`,
    group: "Workers",
    title: `Draco mesh decode, ${triangles} triangles`,
    input: `target/bench-inputs/draco${name}.json`,
    async prepare(source, env) {
      if (!env.decodeDraco) return { skip: "the decodeDraco worker imports draco3d's Node.js build; Node only" };
      const doc = await loadJson(source, this.input);
      const parameters = {
        array: base64Bytes(doc.base64),
        bufferView: { byteLength: doc.byteLength },
        compressedAttributes: doc.compressedAttributes,
      };
      return {
        cases: 1,
        async: true,
        async run() {
          const result = await env.decodeDraco(parameters);
          return sum(result.indexArray.typedArray) + sum(result.attributeData.POSITION.array);
        },
        verify: CHECKSUM_ONLY,
      };
    },
  })),
];
