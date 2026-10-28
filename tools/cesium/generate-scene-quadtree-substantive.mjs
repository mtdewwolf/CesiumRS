// Generates calculated fixture expectations for QuadtreePrimitive substantive fixes
// modeled against pinned CesiumJS 23.0.0 (@cesium/engine 23.0.0, CesiumJS 1.139).
//
// Usage: node tools/cesium/generate-scene-quadtree-substantive.mjs

import { writeFileSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import GeographicTilingScheme from "./node_modules/@cesium/engine/Source/Core/GeographicTilingScheme.js";
import Cartographic from "./node_modules/@cesium/engine/Source/Core/Cartographic.js";
import PerspectiveFrustum from "./node_modules/@cesium/engine/Source/Core/PerspectiveFrustum.js";
import MathCesium from "./node_modules/@cesium/engine/Source/Core/Math.js";

const here = dirname(fileURLToPath(import.meta.url));
const outDir = join(here, "../../tests/data");
const version = JSON.parse(readFileSync(join(here, "./node_modules/@cesium/engine/package.json"), "utf8")).version;

mkdirSync(outDir, { recursive: true });

// 1. Screen-Space Error distance calculations
const frustum = new PerspectiveFrustum();
frustum.near = 1.0;
frustum.far = 10000000.0;
frustum.fov = Math.PI / 3.0;
frustum.aspectRatio = 800.0 / 600.0;
const sseDenominator = frustum.sseDenominator;

const drawingBufferHeight = 600;
const pixelRatio = 1.0;
const levelZeroError = 10000.0;

// SSE formula in CesiumJS QuadtreePrimitive.js:
// let error = (maxGeometricError * height) / (distance * sseDenominator);
// if (fog.enabled) error -= Math.fog(distance, fog.density) * fog.sse;
// error /= pixelRatio;

const distanceZero = 0.0;
const sseDistanceZero = (levelZeroError * drawingBufferHeight) / (distanceZero * sseDenominator);

const distance1000 = 1000.0;
const sseDistance1000 = (levelZeroError * drawingBufferHeight) / (distance1000 * sseDenominator);

const distance50000 = 50000.0;
const sseDistance50000 = (levelZeroError * drawingBufferHeight) / (distance50000 * sseDenominator);

// 2. Spatial hashing and height cache keys
const scheme = new GeographicTilingScheme();
const rectangle = scheme.tileXYToRectangle(0, 0, 0);
const tileSize = 256.0;
const maxSse = 2.0;
const maxError = (rectangle.width / tileSize) * maxSse;

const samplePoints = [
  { lon: -1.0, lat: 0.2 },
  { lon: -2.0, lat: 0.5 },
  { lon: 0.0, lat: 0.0 },
];

const cacheKeys = samplePoints.map((pt) => {
  let lonGrid = Math.floor(pt.lon / maxError) * maxError;
  let latGrid = Math.floor(pt.lat / maxError) * maxError;
  if (lonGrid === 0.0) lonGrid = 0.0;
  if (latGrid === 0.0) latGrid = 0.0;
  return {
    lon: pt.lon,
    lat: pt.lat,
    expected_key: `${lonGrid.toFixed(10)},${latGrid.toFixed(10)}`,
  };
});

const output = {
  module: "QuadtreeSubstantive",
  cesium: version,
  fixture_type: "substantive_review_fixes_expectations",
  description:
    "Expectations for QuadtreePrimitive SSE zero-distance, height callback distribution and cache keys against pinned CesiumJS",
  screen_space_error: {
    sse_denominator: sseDenominator,
    drawing_buffer_height: drawingBufferHeight,
    level_zero_error: levelZeroError,
    distance_zero_is_infinite: !Number.isFinite(sseDistanceZero),
    distance_1000: sseDistance1000,
    distance_50000: sseDistance50000,
  },
  cache_keys: cacheKeys,
};

const outFile = join(outDir, "scene-quadtree-substantive.json");
writeFileSync(outFile, JSON.stringify(output, null, 2) + "\n");
console.log(`Wrote substantive quadtree fixture to ${outFile}`);
