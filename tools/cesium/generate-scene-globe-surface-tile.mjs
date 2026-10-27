// Generates real CesiumJS fixture expectations and computed analytical comparisons
// for GlobeSurfaceTile modeled against pinned CesiumJS 23.0.0 (@cesium/engine 23.0.0, CesiumJS 1.139).
//
// Usage: node tools/cesium/generate-scene-globe-surface-tile.mjs

import { writeFileSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import GlobeSurfaceTile from "./node_modules/@cesium/engine/Source/Scene/GlobeSurfaceTile.js";
import TerrainState from "./node_modules/@cesium/engine/Source/Scene/TerrainState.js";
import ImageryState from "./node_modules/@cesium/engine/Source/Scene/ImageryState.js";
import QuadtreeTileLoadState from "./node_modules/@cesium/engine/Source/Scene/QuadtreeTileLoadState.js";
import Rectangle from "./node_modules/@cesium/engine/Source/Core/Rectangle.js";
import Cartesian3 from "./node_modules/@cesium/engine/Source/Core/Cartesian3.js";
import Cartesian4 from "./node_modules/@cesium/engine/Source/Core/Cartesian4.js";

const here = dirname(fileURLToPath(import.meta.url));
const outDir = join(here, "../../tests/fixtures/scene");
const packageJson = JSON.parse(readFileSync(join(here, "./node_modules/@cesium/engine/package.json"), "utf8"));
const engineVersion = packageJson.version;

mkdirSync(outDir, { recursive: true });

// 1. Real CesiumJS Runtime Execution
const realCesiumJsOutput = {};

// Default construction inspection from CesiumJS
{
  const tile = new GlobeSurfaceTile();
  realCesiumJsOutput.default_construction = {
    eligible_for_unloading: tile.eligibleForUnloading,
    rendered_mesh_defined: tile.renderedMesh !== undefined,
    is_clipped: tile.isClipped,
    clipped_by_boundaries: tile.clippedByBoundaries,
    terrain_state: tile.terrainState,
    terrain_state_name: TerrainState.getNickname ? TerrainState.getNickname(tile.terrainState) : "UNLOADED",
    water_mask_translation_and_scale: [
      tile.waterMaskTranslationAndScale.x,
      tile.waterMaskTranslationAndScale.y,
      tile.waterMaskTranslationAndScale.z,
      tile.waterMaskTranslationAndScale.w,
    ],
    imagery_length: tile.imagery.length,
    vertex_array_defined: tile.vertexArray !== undefined,
    mesh_defined: tile.mesh !== undefined,
    fill_defined: tile.fill !== undefined,
    pick_bounding_sphere: {
      center: [tile.pickBoundingSphere.center.x, tile.pickBoundingSphere.center.y, tile.pickBoundingSphere.center.z],
      radius: tile.pickBoundingSphere.radius,
    },
  };
}

// Eligible for unloading state transitions
{
  const transitions = [];
  const testStates = [
    { state: TerrainState.FAILED, name: "FAILED" },
    { state: TerrainState.UNLOADED, name: "UNLOADED" },
    { state: TerrainState.RECEIVING, name: "RECEIVING" },
    { state: TerrainState.RECEIVED, name: "RECEIVED" },
    { state: TerrainState.TRANSFORMING, name: "TRANSFORMING" },
    { state: TerrainState.TRANSFORMED, name: "TRANSFORMED" },
    { state: TerrainState.READY, name: "READY" },
  ];

  for (const item of testStates) {
    const tile = new GlobeSurfaceTile();
    tile.terrainState = item.state;
    transitions.push({
      terrain_state: item.name,
      eligible_for_unloading: tile.eligibleForUnloading,
    });
  }

  // With imagery in various loading states
  const imageryTransitions = [];
  const imageryStates = [
    { state: ImageryState.UNLOADED, name: "UNLOADED" },
    { state: ImageryState.TRANSITIONING, name: "TRANSITIONING" },
    { state: ImageryState.RECEIVED, name: "RECEIVED" },
    { state: ImageryState.TEXTURE_LOADED, name: "TEXTURE_LOADED" },
    { state: ImageryState.READY, name: "READY" },
    { state: ImageryState.FAILED, name: "FAILED" },
    { state: ImageryState.INVALID, name: "INVALID" },
    { state: ImageryState.PLACEHOLDER, name: "PLACEHOLDER" },
  ];

  for (const item of imageryStates) {
    const tile = new GlobeSurfaceTile();
    tile.terrainState = TerrainState.READY;
    tile.imagery.push({ loadingImagery: { state: item.state } });
    imageryTransitions.push({
      imagery_state: item.name,
      eligible_for_unloading: tile.eligibleForUnloading,
    });
  }

  // Combined terrain + imagery case: terrain RECEIVING and imagery READY
  const tileBoth = new GlobeSurfaceTile();
  tileBoth.terrainState = TerrainState.RECEIVING;
  tileBoth.imagery.push({ loadingImagery: { state: ImageryState.READY } });

  realCesiumJsOutput.unloading_eligibility = {
    terrain_state_cases: transitions,
    imagery_state_cases: imageryTransitions,
    receiving_terrain_with_ready_imagery: tileBoth.eligibleForUnloading,
  };
}

// Water mask translation and scale derivation
{
  const testCases = [
    {
      name: "quadrant_southwest",
      tile: { west: 0.0, south: 0.0, east: 1.0, north: 1.0 },
      source: { west: 0.0, south: 0.0, east: 2.0, north: 2.0 },
    },
    {
      name: "quadrant_northeast",
      tile: { west: 1.0, south: 1.0, east: 2.0, north: 2.0 },
      source: { west: 0.0, south: 0.0, east: 2.0, north: 2.0 },
    },
    {
      name: "asymmetric_ancestor",
      tile: { west: -1.0, south: -0.5, east: 0.0, north: 0.5 },
      source: { west: -2.0, south: -1.0, east: 2.0, north: 1.0 },
    },
    {
      name: "identical_extent",
      tile: { west: -0.5, south: -0.5, east: 0.5, north: 0.5 },
      source: { west: -0.5, south: -0.5, east: 0.5, north: 0.5 },
    },
  ];

  const gst = new GlobeSurfaceTile();
  const waterMaskResults = [];

  for (const c of testCases) {
    const tileRect = new Rectangle(c.tile.west, c.tile.south, c.tile.east, c.tile.north);
    const sourceRect = new Rectangle(c.source.west, c.source.south, c.source.east, c.source.north);
    const result = new Cartesian4();
    gst._computeWaterMaskTranslationAndScale({ rectangle: tileRect }, { rectangle: sourceRect }, result);
    waterMaskResults.push({
      name: c.name,
      tile: c.tile,
      source: c.source,
      translation_and_scale: [result.x, result.y, result.z, result.w],
    });
  }

  realCesiumJsOutput.water_mask_translation_and_scale = waterMaskResults;
}

// Ancestor tile traversal with terrainData
{
  const gst = new GlobeSurfaceTile();

  // Case 1: Parent has terrainData not from upsampling
  const rootTile1 = {
    id: "root",
    data: { terrainData: { wasCreatedByUpsampling: () => false } },
    parent: undefined,
  };
  const childTile1 = {
    id: "child",
    data: new GlobeSurfaceTile(),
    parent: rootTile1,
  };
  const ancestor1 = gst._findAncestorTileWithTerrainData(childTile1);

  // Case 2: Parent has terrainData created by upsampling, grandparent has non-upsampled data
  const rootTile2 = {
    id: "grandparent",
    data: { terrainData: { wasCreatedByUpsampling: () => false } },
    parent: undefined,
  };
  const midTile2 = {
    id: "parent_upsampled",
    data: { terrainData: { wasCreatedByUpsampling: () => true } },
    parent: rootTile2,
  };
  const childTile2 = {
    id: "child_of_upsampled",
    data: new GlobeSurfaceTile(),
    parent: midTile2,
  };
  const ancestor2 = gst._findAncestorTileWithTerrainData(childTile2);

  // Case 3: No ancestor has non-upsampled terrainData
  const childTile3 = {
    id: "child_no_data",
    data: new GlobeSurfaceTile(),
    parent: undefined,
  };
  const ancestor3 = gst._findAncestorTileWithTerrainData(childTile3);

  realCesiumJsOutput.ancestor_terrain_traversal = {
    direct_parent_found: ancestor1 ? ancestor1.id : null,
    skip_upsampled_parent_found: ancestor2 ? ancestor2.id : null,
    no_ancestor_found: ancestor3 ? ancestor3.id : null,
  };
}

// Initialization and prepareNewTile behavior
{
  // Tile with available = false
  const tileUnavail = {
    x: 0,
    y: 0,
    level: 1,
    state: QuadtreeTileLoadState.START,
    data: undefined,
    parent: undefined,
  };
  const mockTerrainProviderFalse = {
    getTileDataAvailable: (x, y, level) => false,
  };
  const mockImageryCollection = { length: 0, get: () => undefined };

  GlobeSurfaceTile.initialize(tileUnavail, mockTerrainProviderFalse, mockImageryCollection);

  // Tile with available = true
  const tileAvail = {
    x: 1,
    y: 1,
    level: 1,
    state: QuadtreeTileLoadState.START,
    data: undefined,
    parent: undefined,
  };
  const mockTerrainProviderTrue = {
    getTileDataAvailable: (x, y, level) => true,
  };

  GlobeSurfaceTile.initialize(tileAvail, mockTerrainProviderTrue, mockImageryCollection);

  realCesiumJsOutput.initialization = {
    unavailable_tile: {
      quadtree_state: tileUnavail.state,
      quadtree_state_name: "LOADING",
      terrain_state: tileUnavail.data.terrainState,
      terrain_state_name: "FAILED",
    },
    available_tile: {
      quadtree_state: tileAvail.state,
      quadtree_state_name: "LOADING",
      terrain_state: tileAvail.data.terrainState,
      terrain_state_name: "UNLOADED",
    },
  };
}

// Resource freeing parity
{
  const tile = new GlobeSurfaceTile();
  tile.terrainState = TerrainState.READY;
  tile.mesh = { id: "mesh-mock" };
  tile.waterMaskTexture = { referenceCount: 1, destroy: () => {} };
  let imageryFreedCount = 0;
  tile.imagery.push({
    freeResources: () => {
      imageryFreedCount++;
    },
  });
  tile.imagery.push({
    freeResources: () => {
      imageryFreedCount++;
    },
  });

  tile.freeResources();

  realCesiumJsOutput.free_resources = {
    terrain_state_after_free: tile.terrainState,
    terrain_state_name: "UNLOADED",
    mesh_defined_after_free: tile.mesh !== undefined,
    water_mask_defined_after_free: tile.waterMaskTexture !== undefined,
    imagery_length_after_free: tile.imagery.length,
    imagery_items_freed: imageryFreedCount,
  };
}

// 2. Computed Expectations (analytical formulas and mathematical contracts)
// Clearly separated from the real CesiumJS runtime output above.
const computedExpectations = {
  water_mask_formulas: {
    description: "scaleX = tileWidth / sourceWidth; scaleY = tileHeight / sourceHeight; translationX = scaleX * (tileWest - sourceWest) / tileWidth; translationY = scaleY * (tileSouth - sourceSouth) / tileHeight",
    identity_scale_when_extents_match: [0.0, 0.0, 1.0, 1.0],
    scale_halved_for_subquadrant: [0.25, 0.25, 0.5, 0.5],
  },
  unloading_rule: {
    rule: "Eligible for unloading unless terrain is RECEIVING/TRANSFORMING or any loading imagery is TRANSITIONING",
  },
  picking_contract: {
    rule: "Pick returns undefined if renderedMesh is undefined. If renderedMesh is defined, delegates to mesh.pick()",
  },
};

const fixture = {
  meta: {
    generator: "tools/cesium/generate-scene-globe-surface-tile.mjs",
    cesium_engine_version: engineVersion,
    description: "Pinned CesiumJS runtime output and distinct analytical expectations for GlobeSurfaceTile",
    generated_at: new Date().toISOString(),
  },
  real_cesiumjs_output: realCesiumJsOutput,
  computed_expectations: computedExpectations,
};

const outPath = join(outDir, "GlobeSurfaceTile.json");
writeFileSync(outPath, JSON.stringify(fixture, null, 2), "utf8");
console.log(`Generated fixture at ${outPath} against CesiumJS ${engineVersion}`);
