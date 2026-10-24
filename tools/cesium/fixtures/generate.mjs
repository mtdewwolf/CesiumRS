// Writes tests/fixtures/<package>/<Module>.json from real CesiumJS.
//
//   npm run fixtures
//
// Run with TZ=UTC (the npm script sets it) so nothing depends on local time.
//
// Each file is `{ "module", "cesium", "cases": [{ "f", "a", "r" }] }`: the
// function name, encoded arguments, and encoded result (or `{ "throws" }`).

import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import dataSourcesProperties from "./data-sources-properties.mjs";
import sceneFoundations from "./scene-foundations.mjs";
import sceneGltfPipeline from "./scene-gltf-pipeline.mjs";
import sceneMetadata from "./scene-metadata.mjs";
import sceneMetadataTables from "./scene-metadata-tables.mjs";
import sceneMetadataEntities from "./scene-metadata-entities.mjs";
import sceneTiling from "./scene-tiling.mjs";
import sceneCompletion from "./scene-completion.mjs";
import sceneTransforms from "./scene-transforms.mjs";
import scenePicking from "./scene-picking.mjs";
import sceneView from "./scene-view.mjs";
import sceneGeometryLoaders from "./scene-geometry-loaders.mjs";
import sceneLoaderFoundations from "./scene-loader-foundations.mjs";
import sceneGpm from "./scene-gpm.mjs";
import sceneTileFormats from "./scene-tile-formats.mjs";
import sceneDracoLoaders from "./scene-draco-loaders.mjs";
import sceneImageTextureLoaders from "./scene-image-texture-loaders.mjs";
import sceneResourceCache from "./scene-resource-cache.mjs";
import sceneResourceLoaders from "./scene-resource-loaders.mjs";
import sceneBufferViewLoader from "./scene-buffer-view-loader.mjs";
import sceneMetadataPicking from "./scene-metadata-picking.mjs";
import sceneRuntime from "./scene-runtime.mjs";
import sceneCamera from "./scene-camera.mjs";
import sceneCameraFlights from "./scene-camera-flights.mjs";
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

// Shader fixtures are discovered. A new directory is a new
// `shaders-<directory>.mjs` in this folder (sorted by filename). Its default
// export returns suites. This file is not edited to add one. The directory
// name is the text between `shaders-` and `.mjs`, and it is the fixture
// subdirectory: `shaders-builtin.mjs` writes `tests/fixtures/shaders/builtin/`.
const shaders = [];
for (const name of readdirSync(here).filter((entry) => entry.startsWith("shaders-") && entry.endsWith(".mjs")).sort()) {
  const dir = name.slice("shaders-".length, -".mjs".length);
  if (!dir) throw new Error(`Shader fixture filename must be shaders-<directory>.mjs: ${name}`);
  const loaded = await import(`./${name}`);
  for (const suite of await loaded.default()) shaders.push({ ...suite, dir });
}

const packages = { data_sources: dataSourcesProperties(), scene: [...sceneGpm(),...sceneTileFormats(),...await sceneImageTextureLoaders(),...sceneLoaderFoundations(),...await sceneDracoLoaders(),...await sceneGeometryLoaders(),...sceneResourceCache(),...await sceneBufferViewLoader(),...await sceneResourceLoaders(),...sceneMetadataPicking(), ...sceneView(), ...scenePicking(), ...sceneTransforms(), ...await sceneCompletion(), ...await sceneRuntime(), ...sceneCamera(), ...sceneCameraFlights(), ...await sceneFoundations(), ...await sceneGltfPipeline(), ...await sceneMetadata(), ...await sceneMetadataTables(), ...await sceneMetadataEntities(), ...await sceneTiling()], workers: [...await workersDecodeDraco(), ...await workersGeometry(), ...await workersTerrain(), ...await workersVector(), ...await workersSplats(), ...await workersI3s(), ...await workersCombine(), ...await workersTask()], renderer: [...rendererCompletion(), ...rendererFoundations(), ...rendererExtended(), ...await rendererExecution()], core: [...await coreGroundPolyline(), ...await coreTilesProvider(), ...await coreTilesTerrain(), ...await coreGee(), ...await coreNetworkTerrain(), ...await corePlatforms(), ...coreContracts(), ...await coreRequests(), ...await coreServices(), ...await coreTextures(), ...coreFoundations(), ...coreMath(), ...coreGeometry(), ...coreBounds(), ...coreTime(), ...corePacking(), ...coreSplines(), ...coreAlgorithms(), ...coreGeometries(), ...coreUtilities(), ...corePolygon(), ...corePipeline(), ...coreSurfaces(), ...coreLines(), ...coreCorridors(), ...coreTerrain(), ...coreTerrainProvider(), ...await coreTerrainMesh(), ...await coreHeightmapData(), ...coreS2(), ...await coreQuantizedTerrain(), ...await coreTerrainBoundsProviders(), ...coreAstronomy()], shaders };
// coreAstronomy runs last: loading EOP data adds leap seconds to JulianDate.leapSeconds.

// Labels a suite can be selected by. Shader suites also accept
// `<directory>/<module>` so two directories can use the same stem.
function labels(pkg, suite) {
  const names = [suite.module, `${pkg}/${suite.module}`];
  if (suite.dir) names.push(`${suite.dir}/${suite.module}`, `${pkg}/${suite.dir}/${suite.module}`);
  return names;
}

// Optional module names avoid rewriting unrelated fixtures during porting.
const selected = new Set(process.argv.slice(2));
const known = new Set(Object.entries(packages).flatMap(([pkg, suites]) => suites.flatMap((suite) => labels(pkg, suite))));
for (const name of selected) {
  if (!known.has(name)) throw new Error(`Unknown fixture module: ${name}`);
}

let total = 0;
const written = new Set();
for (const [pkg, suites] of Object.entries(packages)) {
  for (const s of suites) {
    if (selected.size && !labels(pkg, s).some((label) => selected.has(label))) continue;
    const rel = s.dir ? `${pkg}/${s.dir}/${s.module}` : `${pkg}/${s.module}`;
    if (written.has(rel)) throw new Error(`Duplicate fixture ${rel}`);
    written.add(rel);
    const file = s.dir ? join(out, pkg, s.dir, `${s.module}.json`) : join(out, pkg, `${s.module}.json`);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, JSON.stringify({ module: s.module, cesium: version, cases: s.cases }) + "\n");
    total += s.cases.length;
    console.log(`${rel}: ${s.cases.length} cases`);
  }
}
console.log(`${total} cases`);
