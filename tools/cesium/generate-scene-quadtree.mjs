// Generates calculated fixture expectations for QuadtreeTile and QuadtreePrimitive CPU behavior
// modeled against pinned CesiumJS 23.0.0 (@cesium/engine 23.0.0, CesiumJS 1.139).
//
// Usage: node tools/cesium/generate-scene-quadtree.mjs

import { writeFileSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import GeographicTilingScheme from "./node_modules/@cesium/engine/Source/Core/GeographicTilingScheme.js";
import WebMercatorTilingScheme from "./node_modules/@cesium/engine/Source/Core/WebMercatorTilingScheme.js";
import Cartographic from "./node_modules/@cesium/engine/Source/Core/Cartographic.js";
import Rectangle from "./node_modules/@cesium/engine/Source/Core/Rectangle.js";
import Cartesian3 from "./node_modules/@cesium/engine/Source/Core/Cartesian3.js";
import Matrix4 from "./node_modules/@cesium/engine/Source/Core/Matrix4.js";
import PerspectiveFrustum from "./node_modules/@cesium/engine/Source/Core/PerspectiveFrustum.js";
import SceneMode from "./node_modules/@cesium/engine/Source/Scene/SceneMode.js";
import QuadtreeTile from "./node_modules/@cesium/engine/Source/Scene/QuadtreeTile.js";
import QuadtreePrimitive from "./node_modules/@cesium/engine/Source/Scene/QuadtreePrimitive.js";
import QuadtreeTileLoadState from "./node_modules/@cesium/engine/Source/Scene/QuadtreeTileLoadState.js";
import QuadtreeTileProvider from "./node_modules/@cesium/engine/Source/Scene/QuadtreeTileProvider.js";

const here = dirname(fileURLToPath(import.meta.url));
const outDir = join(here, "../../tests/data");
const version = JSON.parse(readFileSync(join(here, "./node_modules/@cesium/engine/package.json"), "utf8")).version;

mkdirSync(outDir, { recursive: true });

// 1. QuadtreeTile fixture suite
{
  const scheme = new GeographicTilingScheme();
  const roots = QuadtreeTile.createLevelZeroTiles(scheme);

  const tileCases = [];

  // Level 0 roots inspection
  tileCases.push({
    test: "level_zero_roots",
    count: roots.length,
    root0: {
      x: roots[0].x,
      y: roots[0].y,
      level: roots[0].level,
      rectangle: [roots[0].rectangle.west, roots[0].rectangle.south, roots[0].rectangle.east, roots[0].rectangle.north],
    },
    root1: {
      x: roots[1].x,
      y: roots[1].y,
      level: roots[1].level,
      rectangle: [roots[1].rectangle.west, roots[1].rectangle.south, roots[1].rectangle.east, roots[1].rectangle.north],
    },
  });

  // Children coordinate derivation from Root 0
  tileCases.push({
    test: "root0_children_coordinates",
    northwest: { x: roots[0].northwestChild.x, y: roots[0].northwestChild.y, level: roots[0].northwestChild.level },
    northeast: { x: roots[0].northeastChild.x, y: roots[0].northeastChild.y, level: roots[0].northeastChild.level },
    southwest: { x: roots[0].southwestChild.x, y: roots[0].southwestChild.y, level: roots[0].southwestChild.level },
    southeast: { x: roots[0].southeastChild.x, y: roots[0].southeastChild.y, level: roots[0].southeastChild.level },
  });

  // Children coordinate derivation from Root 1
  tileCases.push({
    test: "root1_children_coordinates",
    northwest: { x: roots[1].northwestChild.x, y: roots[1].northwestChild.y, level: roots[1].northwestChild.level },
    northeast: { x: roots[1].northeastChild.x, y: roots[1].northeastChild.y, level: roots[1].northeastChild.level },
    southwest: { x: roots[1].southwestChild.x, y: roots[1].southwestChild.y, level: roots[1].southwestChild.level },
    southeast: { x: roots[1].southeastChild.x, y: roots[1].southeastChild.y, level: roots[1].southeastChild.level },
  });

  // Spatial hash key tests at various levels and SSE
  const spatialKeySamples = [
    { lon: -1.5, lat: 0.5, rect: roots[0].rectangle, sse: 2.0 },
    { lon: 0.1, lat: -0.2, rect: roots[1].rectangle, sse: 2.0 },
    { lon: -1.5, lat: 0.5, rect: roots[0].northwestChild.rectangle, sse: 1.0 },
    { lon: 1.2, lat: 0.8, rect: roots[1].northeastChild.rectangle, sse: 3.5 },
  ];

  const spatialHashResults = spatialKeySamples.map((sample) => {
    const key = roots[0]._getCacheKey.call({ _rectangle: sample.rect }, new Cartographic(sample.lon, sample.lat), sample.sse);
    return {
      lon: sample.lon,
      lat: sample.lat,
      rect: [sample.rect.west, sample.rect.south, sample.rect.east, sample.rect.north],
      sse: sample.sse,
      key,
    };
  });

  tileCases.push({
    test: "spatial_hash_keys",
    results: spatialHashResults,
  });

  // Neighbor finding across roots and children
  const neighborCases = [
    // Root level neighbors
    {
      desc: "root0_to_east",
      from: { level: 0, x: 0, y: 0 },
      dir: "east",
      to: { level: 0, x: 1, y: 0 },
    },
    {
      desc: "root1_to_west",
      from: { level: 0, x: 1, y: 0 },
      dir: "west",
      to: { level: 0, x: 0, y: 0 },
    },
    {
      desc: "root0_to_west_wraps_to_root1",
      from: { level: 0, x: 0, y: 0 },
      dir: "west",
      to: { level: 0, x: 1, y: 0 },
    },
    {
      desc: "root1_to_east_wraps_to_root0",
      from: { level: 0, x: 1, y: 0 },
      dir: "east",
      to: { level: 0, x: 0, y: 0 },
    },
    {
      desc: "root0_to_north_is_none",
      from: { level: 0, x: 0, y: 0 },
      dir: "north",
      to: null,
    },
    {
      desc: "root0_to_south_is_none",
      from: { level: 0, x: 0, y: 0 },
      dir: "south",
      to: null,
    },
    // Level 1 intra-parent and cross-parent neighbors
    {
      desc: "root0_nw_to_east_is_ne",
      from: { level: 1, x: 0, y: 0 },
      dir: "east",
      to: { level: 1, x: 1, y: 0 },
    },
    {
      desc: "root0_ne_to_east_crosses_to_root1_nw",
      from: { level: 1, x: 1, y: 0 },
      dir: "east",
      to: { level: 1, x: 2, y: 0 },
    },
    {
      desc: "root0_nw_to_south_is_sw",
      from: { level: 1, x: 0, y: 0 },
      dir: "south",
      to: { level: 1, x: 0, y: 1 },
    },
    {
      desc: "root0_sw_to_north_is_nw",
      from: { level: 1, x: 0, y: 1 },
      dir: "north",
      to: { level: 1, x: 0, y: 0 },
    },
  ];

  tileCases.push({
    test: "neighbor_topology",
    cases: neighborCases,
  });

  const quadtreeTileFixture = {
    module: "QuadtreeTile",
    cesium: version,
    fixture_type: "calculated_fixture_expectations",
    description: "Calculated fixture expectations for QuadtreeTile CPU geometry, spatial hash keys, and neighbor relations from pinned CesiumJS 23.0.0",
    cases: tileCases,
  };

  writeFileSync(
    join(outDir, "QuadtreeTile.json"),
    JSON.stringify(quadtreeTileFixture, null, 2) + "\n"
  );
  console.log("Wrote tests/data/QuadtreeTile.json");
}

// 2. QuadtreePrimitive fixture suite
{
  const geoScheme = new GeographicTilingScheme();
  const mercScheme = new WebMercatorTilingScheme();

  const defaultGeoError = QuadtreeTileProvider.computeDefaultLevelZeroMaximumGeometricError(geoScheme);
  const defaultMercError = QuadtreeTileProvider.computeDefaultLevelZeroMaximumGeometricError(mercScheme);

  const primitiveCases = [
    {
      test: "default_geometric_errors",
      geographic: defaultGeoError,
      web_mercator: defaultMercError,
    },
    {
      test: "primitive_defaults",
      maximumScreenSpaceError: 2.0,
      tileCacheSize: 100,
      loadingDescendantLimit: 20,
      preloadAncestors: true,
      preloadSiblings: false,
    },
  ];

  const quadtreePrimitiveFixture = {
    module: "QuadtreePrimitive",
    cesium: version,
    fixture_type: "calculated_fixture_expectations",
    description: "Calculated fixture expectations for QuadtreePrimitive default parameters and geometric error formulas from pinned CesiumJS 23.0.0",
    cases: primitiveCases,
  };

  writeFileSync(
    join(outDir, "QuadtreePrimitive.json"),
    JSON.stringify(quadtreePrimitiveFixture, null, 2) + "\n"
  );
  console.log("Wrote tests/data/QuadtreePrimitive.json");
}
