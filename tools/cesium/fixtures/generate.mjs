// Writes tests/fixtures/<package>/<Module>.json from real CesiumJS.
//
//   npm run fixtures
//
// Run with TZ=UTC (the npm script sets it) so nothing depends on local time.
//
// Each file is `{ "module", "cesium", "cases": [{ "f", "a", "r" }] }`: the
// function name, encoded arguments, and encoded result (or `{ "throws" }`).

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import shadersBuiltin from "./shaders-builtin.mjs";
import workersDecodeDraco from "./workers-decode-draco.mjs";
import workersGeometry from "./workers-geometry.mjs";
import workersTerrain from "./workers-terrain.mjs";
import workersVector from "./workers-vector.mjs";
import workersSplats from "./workers-splats.mjs";
import workersI3s from "./workers-i3s.mjs";
import workersCombine from "./workers-combine.mjs";
import workersTask from "./workers-task.mjs";
import rendererCompletion from "./renderer-completion.mjs";
import rendererExtended from "./renderer-extended.mjs";
import rendererExecution from "./renderer-execution.mjs";
import rendererFoundations from "./renderer-foundations.mjs";
import coreBounds from "./core-bounds.mjs";
import coreGeometry from "./core-geometry.mjs";
import coreMath from "./core-math.mjs";
import coreTime from "./core-time.mjs";
import corePacking from "./core-packing.mjs";
import coreSplines from "./core-splines.mjs";
import coreAlgorithms from "./core-algorithms.mjs";
import coreAstronomy from "./core-astronomy.mjs";
import coreGeometries from "./core-geometries.mjs";
import coreUtilities from "./core-utilities.mjs";
import corePolygon from "./core-polygon.mjs";
import corePipeline from "./core-pipeline.mjs";
import coreSurfaces from "./core-surfaces.mjs";
import coreLines from "./core-lines.mjs";
import coreCorridors from "./core-corridors.mjs";
import coreTerrain from "./core-terrain.mjs";
import coreTerrainProvider from "./core-terrain-provider.mjs";
import coreTerrainMesh from "./core-terrain-mesh.mjs";
import coreHeightmapData from "./core-heightmap-data.mjs";
import coreS2 from "./core-s2.mjs";
import coreQuantizedTerrain from "./core-quantized-terrain.mjs";
import coreTerrainBoundsProviders from "./core-terrain-bounds-providers.mjs";

import coreFoundations from "./core-foundations.mjs";
import coreTextures from "./core-textures.mjs";
import coreServices from "./core-services.mjs";
import coreRequests from "./core-requests.mjs";
import coreContracts from "./core-contracts.mjs";

import corePlatforms from "./core-platforms.mjs";

import coreNetworkTerrain from "./core-network-terrain.mjs";

import coreGee from "./core-gee.mjs";

import coreTilesTerrain from "./core-tiles-terrain.mjs";

import coreTilesProvider from "./core-tiles-provider.mjs";

import coreGroundPolyline from "./core-ground-polyline.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, "../../../tests/fixtures");
const version = JSON.parse(readFileSync(join(here, "../node_modules/@cesium/engine/package.json"), "utf8")).version;

const packages = { workers: [...await workersDecodeDraco(), ...await workersGeometry(), ...await workersTerrain(), ...await workersVector(), ...await workersSplats(), ...await workersI3s(), ...await workersCombine(), ...await workersTask()], renderer: [...rendererCompletion(), ...rendererFoundations(), ...rendererExtended(), ...await rendererExecution()], core: [...await coreGroundPolyline(), ...await coreTilesProvider(), ...await coreTilesTerrain(), ...await coreGee(), ...await coreNetworkTerrain(), ...await corePlatforms(), ...coreContracts(), ...await coreRequests(), ...await coreServices(), ...await coreTextures(), ...coreFoundations(), ...coreMath(), ...coreGeometry(), ...coreBounds(), ...coreTime(), ...corePacking(), ...coreSplines(), ...coreAlgorithms(), ...coreGeometries(), ...coreUtilities(), ...corePolygon(), ...corePipeline(), ...coreSurfaces(), ...coreLines(), ...coreCorridors(), ...coreTerrain(), ...coreTerrainProvider(), ...await coreTerrainMesh(), ...await coreHeightmapData(), ...coreS2(), ...await coreQuantizedTerrain(), ...await coreTerrainBoundsProviders(), ...coreAstronomy()], shaders: [...shadersBuiltin()] };
// coreAstronomy runs last: loading EOP data adds leap seconds to JulianDate.leapSeconds.

// Optional module names avoid rewriting unrelated fixtures during porting.
const selected = new Set(process.argv.slice(2));
const known = new Set(Object.values(packages).flat().map(s => s.module));
for (const name of selected) {
  if (!known.has(name)) throw new Error(`Unknown fixture module: ${name}`);
}

let total = 0;
for (const [pkg, suites] of Object.entries(packages)) {
  mkdirSync(join(out, pkg), { recursive: true });
  for (const s of suites) {
    if (selected.size && !selected.has(s.module)) continue;
    const file = join(out, pkg, `${s.module}.json`);
    writeFileSync(file, JSON.stringify({ module: s.module, cesium: version, cases: s.cases }) + "\n");
    total += s.cases.length;
    console.log(`${pkg}/${s.module}: ${s.cases.length} cases`);
  }
}
console.log(`${total} cases`);
