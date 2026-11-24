// Comprehensive deterministic reference fixture generator for Cluster 1
// (9 Scene 3D Tiles Core & Content modules) against pinned @cesium/engine 23.0.0.
//
// Modules covered:
//   1. Scene/Cesium3DTile
//   2. Scene/Cesium3DTileBatchTable
//   3. Scene/Cesium3DTileContent
//   4. Scene/Cesium3DTileFeature
//   5. Scene/Cesium3DTilePointFeature
//   6. Scene/Implicit3DTileContent
//   7. Scene/Multiple3DTileContent
//   8. Scene/Composite3DTileContent
//   9. Scene/Geometry3DTileContent
//
// Usage:
//   CHROMIUM_PATH=/path/to/chrome-headless-shell node tools/cesium/generate-scene-3d-tiles-core.mjs
import { chromium } from 'playwright-core';
import { build } from 'esbuild';
import { createServer } from 'node:http';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const C = await import('@cesium/engine');
const { default: Cesium3DTile } = await import('@cesium/engine/Source/Scene/Cesium3DTile.js');
const { default: Cesium3DTileContent } = await import('@cesium/engine/Source/Scene/Cesium3DTileContent.js');
const { default: Cesium3DTileContentState } = await import('@cesium/engine/Source/Scene/Cesium3DTileContentState.js');
const { default: Cesium3DTileRefine } = await import('@cesium/engine/Source/Scene/Cesium3DTileRefine.js');
const { default: Implicit3DTileContent } = await import('@cesium/engine/Source/Scene/Implicit3DTileContent.js');
const { default: ImplicitTileset } = await import('@cesium/engine/Source/Scene/ImplicitTileset.js');
const { default: ImplicitTileCoordinates } = await import('@cesium/engine/Source/Scene/ImplicitTileCoordinates.js');
const { default: Multiple3DTileContent } = await import('@cesium/engine/Source/Scene/Multiple3DTileContent.js');
const { default: Composite3DTileContent } = await import('@cesium/engine/Source/Scene/Composite3DTileContent.js');
const { default: Geometry3DTileContent } = await import('@cesium/engine/Source/Scene/Geometry3DTileContent.js');
const { default: Vector3DTileGeometry } = await import('@cesium/engine/Source/Scene/Vector3DTileGeometry.js');
const { default: TaskProcessor } = await import('@cesium/engine/Source/Core/TaskProcessor.js');
const { invoke, loadWorker } = await import('./fixtures/workers-terrain.mjs');

const num = (v) =>
  Number.isNaN(v) ? 'NaN' : v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Object.is(v, -0) ? 0 : v;
const vec3 = (c) => (c ? [num(c.x), num(c.y), num(c.z)] : null);
const mat4 = (m) => (m ? Array.from(C.Matrix4.pack(m, new Array(16)), num) : null);
const colorArr = (c) => (c ? [num(c.red), num(c.green), num(c.blue), num(c.alpha)] : null);
const sphereObj = (s) => (s ? { center: vec3(s.center), radius: num(s.radius) } : null);
const fnv = (bytes) => {
  let h = 0xcbf29ce484222325n;
  for (const b of bytes) h = BigInt.asUintN(64, (h ^ BigInt(b)) * 0x100000001b3n);
  return h.toString(16);
};
const toBase64 = (bytes) => Buffer.from(bytes).toString('base64');

// Suppress deprecation warnings during fixture capture.
Cesium3DTile._deprecationWarning = () => {};
C.Cesium3DTileBatchTable._deprecationWarning = () => {};

// ============================================================================
// 1. Scene/Cesium3DTile (pure Node)
// ============================================================================
function captureCesium3DTile() {
  const baseResource = new C.Resource({ url: 'https://example.com/tileset/tileset.json' });
  const rootTransform = C.Matrix4.fromTranslation(new C.Cartesian3(100, 200, 300));
  const childTransform = C.Matrix4.fromTranslationQuaternionRotationScale(
    new C.Cartesian3(10, -20, 30),
    C.Quaternion.fromAxisAngle(C.Cartesian3.UNIT_Z, Math.PI / 4),
    new C.Cartesian3(2, 3, 1.5),
  );
  const grandChildTransform = C.Matrix4.fromTranslation(new C.Cartesian3(0, 5, 0));
  const tilesetModelMatrix = C.Matrix4.fromTranslation(new C.Cartesian3(1000, 0, 0));

  const mockTileset = {
    modelMatrix: C.Matrix4.clone(tilesetModelMatrix),
    _geometricError: 500,
    _scaledGeometricError: 500,
    _updatedVisibilityFrame: 1,
    metadata: undefined,
    schema: undefined,
    clippingPlanes: undefined,
    clippingPolygons: undefined,
    isSkippingLevelOfDetail: false,
    foveatedScreenSpaceError: false,
    foveatedConeSize: 0.1,
    dynamicScreenSpaceError: false,
    progressiveResolutionHeightFraction: 0.3,
    memoryAdjustedScreenSpaceError: 16,
    _pass: 0,
    statistics: { numberOfTilesTotal: 0 },
  };

  const rootHeader = {
    boundingVolume: { box: [0, 0, 0, 50, 0, 0, 0, 40, 0, 0, 0, 30] },
    content: {
      uri: 'root.b3dm',
      boundingVolume: { sphere: [1, 2, 3, 25] },
    },
    viewerRequestVolume: { sphere: [0, 0, 0, 200] },
    geometricError: 120,
    refine: 'ADD',
    transform: mat4(rootTransform),
    extras: { author: 'cesium', level: 0 },
  };

  const childHeader = {
    boundingVolume: { sphere: [5, 0, 0, 10] },
    content: { uri: 'child.b3dm' },
    // geometricError omitted -> inherits parent
    // refine omitted -> inherits parent (ADD)
    transform: mat4(childTransform),
  };

  const grandChildHeader = {
    boundingVolume: { region: [-0.2, 0.1, 0.2, 0.4, 10, 150] },
    // no content -> empty content tile
    geometricError: 15,
    refine: 'replace',
    transform: mat4(grandChildTransform),
  };

  const root = new Cesium3DTile(mockTileset, baseResource, rootHeader, undefined);
  mockTileset.root = root;
  const child = new Cesium3DTile(mockTileset, baseResource, childHeader, root);
  root.children.push(child);
  const grandChild = new Cesium3DTile(mockTileset, baseResource, grandChildHeader, child);
  child.children.push(grandChild);

  // Also test a region tile at identity initialTransform vs modified modelMatrix, and S2 tile
  const regionTileset = {
    ...mockTileset,
    modelMatrix: C.Matrix4.clone(C.Matrix4.IDENTITY),
    _geometricError: 80,
    _scaledGeometricError: 80,
  };
  const regionHeader = {
    boundingVolume: { region: [-0.1, 0.2, 0.1, 0.35, 20, 220] },
    // refine omitted on root -> defaults to REPLACE
    // geometricError omitted on root -> inherits tileset._geometricError
    contents: [{ uri: 'a.b3dm' }, { uri: 'b.b3dm' }],
  };
  const regionTile = new Cesium3DTile(regionTileset, baseResource, regionHeader, undefined);
  const s2Header = {
    boundingVolume: {
      extensions: {
        '3DTILES_bounding_volume_S2': {
          token: '1',
          minimumHeight: 0,
          maximumHeight: 1000,
        },
      },
    },
    geometricError: 40,
    refine: 'add',
    content: { uri: '' }, // empty uri -> empty content!
  };
  const s2Tile = new Cesium3DTile(regionTileset, baseResource, s2Header, undefined);

  // Camera + FrameState helper
  const makeFrame = (pos, dir, up, exaggeration = 1.0, relHeight = 0.0) => {
    const frustum = new C.PerspectiveFrustum({
      fov: Math.PI / 3,
      aspectRatio: 1.0,
      near: 1.0,
      far: 100000.0,
    });
    const positionWC = new C.Cartesian3(...pos);
    const directionWC = C.Cartesian3.normalize(new C.Cartesian3(...dir), new C.Cartesian3());
    const upWC = C.Cartesian3.normalize(new C.Cartesian3(...up), new C.Cartesian3());
    const rightWC = C.Cartesian3.normalize(C.Cartesian3.cross(directionWC, upWC, new C.Cartesian3()), new C.Cartesian3());
    const orthoUp = C.Cartesian3.cross(rightWC, directionWC, new C.Cartesian3());
    const positionCartographic = C.Ellipsoid.WGS84.cartesianToCartographic(positionWC) ?? new C.Cartographic(0, 0, 0);
    const cullingVolume = frustum.computeCullingVolume(positionWC, directionWC, orthoUp);
    return {
      mode: C.SceneMode.SCENE3D,
      mapProjection: new C.GeographicProjection(C.Ellipsoid.WGS84),
      verticalExaggeration: exaggeration,
      verticalExaggerationRelativeHeight: relHeight,
      pixelRatio: 1.0,
      context: { drawingBufferWidth: 512, drawingBufferHeight: 512 },
      cullingVolume,
      camera: {
        positionWC,
        directionWC,
        upWC: orthoUp,
        rightWC,
        positionCartographic,
        frustum,
      },
    };
  };

  // Frame 1: camera looking straight at root tile from outside viewerRequestVolume (distance 500)
  // Root computed center is (1100, 200, 300)
  const frameFar = makeFrame([1100, 200, 800], [0, 0, -1], [0, 1, 0]);
  // Frame 2: camera inside viewerRequestVolume (at (1100, 200, 350), within radius 200 of (1100,200,300))
  const frameInside = makeFrame([1100, 200, 350], [0, 0, -1], [0, 1, 0]);
  // Frame 3: camera looking away from root tile (outside frustum)
  const frameAway = makeFrame([1100, 200, 800], [0, 0, 1], [0, 1, 0]);

  const tileSnapshot = (t, frame) => {
    const mask = t.visibility(frame, C.CullingVolume.MASK_INDETERMINATE);
    const visible = mask !== C.CullingVolume.MASK_OUTSIDE;
    const inReq = t.insideViewerRequestVolume(frame);
    return {
      computedTransform: mat4(t.computedTransform),
      boundingSphere: sphereObj(t.boundingSphere),
      contentBoundingSphere: sphereObj(t.contentBoundingVolume.boundingSphere),
      geometricError: num(t.geometricError),
      unscaledGeometricError: num(t._geometricError),
      refine: t.refine === Cesium3DTileRefine.ADD ? 'ADD' : 'REPLACE',
      hasEmptyContent: t.hasEmptyContent,
      hasMultipleContents: t.hasMultipleContents,
      hasRenderableContent: t.hasRenderableContent,
      extras: t.extras ?? null,
      distanceToTile: num(t.distanceToTile(frame)),
      distanceToTileCenter: num(t.distanceToTileCenter(frame)),
      insideViewerRequestVolume: inReq,
      visibleInFrustum: visible,
      isVisible: visible && inReq,
    };
  };

  const initial = {
    rootFar: tileSnapshot(root, frameFar),
    rootInside: tileSnapshot(root, frameInside),
    rootAway: tileSnapshot(root, frameAway),
    childFar: tileSnapshot(child, frameFar),
    grandChildFar: tileSnapshot(grandChild, frameFar),
    regionInitial: tileSnapshot(regionTile, frameFar),
    s2Initial: tileSnapshot(s2Tile, frameFar),
  };

  // Update transforms: shift modelMatrix and apply vertical exaggeration
  const shiftedModel = C.Matrix4.fromTranslationQuaternionRotationScale(
    new C.Cartesian3(2000, -500, 100),
    C.Quaternion.IDENTITY,
    new C.Cartesian3(2, 2, 2),
  );
  const frameExag = makeFrame([2200, -100, 1000], [0, 0, -1], [0, 1, 0], 2.0, 50.0);
  root.updateTransform(shiftedModel, frameExag);
  child.updateTransform(root.computedTransform, frameExag);

  // Also test region tile when modelMatrix changes from IDENTITY (converts Region -> OBB!)
  const regionDeltaModel = C.Matrix4.fromTranslation(new C.Cartesian3(50, -30, 10));
  const frameUnit = makeFrame([6378137 + 10000, 0, 0], [-1, 0, 0], [0, 0, 1], 1.0, 0.0);
  regionTile.updateTransform(regionDeltaModel, frameUnit);

  // And test region tile with vertical exaggeration without transform change
  const regionExagTile = new Cesium3DTile(
    regionTileset,
    baseResource,
    { boundingVolume: { region: [-0.1, 0.2, 0.1, 0.35, 20, 220] }, geometricError: 50 },
    undefined,
  );
  const frameRegionExag = makeFrame([6378137 + 10000, 0, 0], [-1, 0, 0], [0, 0, 1], 3.0, 100.0);
  regionExagTile.updateTransform(C.Matrix4.IDENTITY, frameRegionExag);

  const afterTransform = {
    root: tileSnapshot(root, frameExag),
    child: tileSnapshot(child, frameExag),
    regionShifted: {
      computedTransform: mat4(regionTile.computedTransform),
      boundingSphere: sphereObj(regionTile.boundingSphere),
      isObb: regionTile.boundingVolume instanceof C.TileOrientedBoundingBox,
    },
    regionExaggerated: {
      boundingSphere: sphereObj(regionExagTile.boundingSphere),
      minimumHeight: num(regionExagTile.boundingVolume.minimumHeight),
      maximumHeight: num(regionExagTile.boundingVolume.maximumHeight),
    },
  };

  // State lifecycle checks on a fresh tile
  const stateTile = new Cesium3DTile(mockTileset, baseResource, rootHeader, undefined);
  const readFlags = (t) => ({
    state: t._contentState,
    hasEmptyContent: t.hasEmptyContent,
    contentUnloaded: t.contentUnloaded,
    contentReady: t.contentReady,
    contentExpired: t.contentExpired,
    contentFailed: t.contentFailed,
    hasUnloadedRenderableContent: t.hasUnloadedRenderableContent,
    contentAvailable: t.contentAvailable,
  });
  const lifecycle = [];
  lifecycle.push({ step: 'initial', ...readFlags(stateTile) });
  stateTile._contentState = Cesium3DTileContentState.LOADING;
  lifecycle.push({ step: 'loading', ...readFlags(stateTile) });
  stateTile._contentState = Cesium3DTileContentState.PROCESSING;
  lifecycle.push({ step: 'processing', ...readFlags(stateTile) });
  stateTile._contentState = Cesium3DTileContentState.READY;
  lifecycle.push({ step: 'ready', ...readFlags(stateTile) });
  stateTile._expiredContent = {};
  stateTile._contentState = Cesium3DTileContentState.EXPIRED;
  lifecycle.push({ step: 'expiredWithFallback', ...readFlags(stateTile) });
  stateTile._contentState = Cesium3DTileContentState.FAILED;
  lifecycle.push({ step: 'failedWithExpired', ...readFlags(stateTile) });
  stateTile._expiredContent = undefined;
  lifecycle.push({ step: 'failedNoExpired', ...readFlags(stateTile) });
  lifecycle.push({ step: 'emptyTile', ...readFlags(grandChild) });

  return {
    rootHeader,
    childHeader,
    grandChildHeader,
    regionHeader,
    s2Header,
    tilesetModelMatrix: mat4(tilesetModelMatrix),
    shiftedModel: mat4(shiftedModel),
    regionDeltaModel: mat4(regionDeltaModel),
    initial,
    afterTransform,
    lifecycle,
  };
}

// ============================================================================
// 3. Scene/Cesium3DTileContent interface contract (pure Node)
// ============================================================================
function captureCesium3DTileContent() {
  const iface = new Cesium3DTileContent();
  const getters = [
    'featuresLength', 'pointsLength', 'trianglesLength', 'geometryByteLength',
    'texturesByteLength', 'batchTableByteLength', 'innerContents', 'ready',
    'tileset', 'tile', 'url', 'batchTable', 'metadata', 'group',
  ];
  const methods = [
    ['hasProperty', [0, 'x']],
    ['getFeature', [0]],
    ['applyDebugSettings', [true, C.Color.WHITE]],
    ['applyStyle', [undefined]],
    ['update', [{}, {}]],
    ['pick', [undefined, {}]],
    ['isDestroyed', []],
    ['destroy', []],
  ];
  const getterThrows = {};
  for (const g of getters) {
    try { void iface[g]; getterThrows[g] = false; } catch (e) { getterThrows[g] = e instanceof C.DeveloperError; }
  }
  const methodThrows = {};
  for (const [m, args] of methods) {
    try { iface[m](...args); methodThrows[m] = false; } catch (e) { methodThrows[m] = e instanceof C.DeveloperError; }
  }
  return {
    initialFeaturePropertiesDirty: iface.featurePropertiesDirty,
    getterThrows,
    methodThrows,
  };
}

// ============================================================================
// 6. Scene/Implicit3DTileContent (pure Node)
// ============================================================================
async function captureImplicit3DTileContent() {
  // Volume derivation cases
  const rootBox = [10, 20, 30, 80, 0, 0, 0, 60, 0, 0, 0, 40];
  const rootRegion = [-0.4, -0.2, 0.4, 0.6, 0, 200];

  const boxDerivations = [
    { level: 0, x: 0, y: 0, z: null },
    { level: 1, x: 0, y: 0, z: null },
    { level: 1, x: 1, y: 0, z: null },
    { level: 1, x: 1, y: 1, z: null },
    { level: 2, x: 2, y: 3, z: null },
    { level: 1, x: 0, y: 1, z: 0 },
    { level: 1, x: 1, y: 0, z: 1 },
    { level: 2, x: 3, y: 1, z: 2 },
  ].map((c) => ({
    ...c,
    box: Implicit3DTileContent._deriveBoundingBox(rootBox, c.level, c.x, c.y, c.z ?? undefined).map(num),
  }));

  const regionDerivations = [
    { level: 0, x: 0, y: 0, z: null },
    { level: 1, x: 0, y: 0, z: null },
    { level: 1, x: 1, y: 1, z: null },
    { level: 2, x: 3, y: 2, z: null },
    { level: 1, x: 1, y: 0, z: 1 },
    { level: 2, x: 1, y: 2, z: 3 },
  ].map((c) => ({
    ...c,
    region: Implicit3DTileContent._deriveBoundingRegion(rootRegion, c.level, c.x, c.y, c.z ?? undefined).map(num),
  }));

  const s2Derivations = [
    // Even face (face 0 -> token '1')
    { parentToken: '1', minH: 0, maxH: 100, childIndex: 0, level: 1, x: 0, y: 0, z: null },
    { parentToken: '1', minH: 0, maxH: 100, childIndex: 3, level: 1, x: 1, y: 1, z: null },
    { parentToken: '1', minH: 0, maxH: 100, childIndex: 5, level: 1, x: 1, y: 0, z: 1 },
    // Odd face (face 1 -> token '3')
    { parentToken: '3', minH: 10, maxH: 90, childIndex: 1, level: 1, x: 1, y: 0, z: null },
    { parentToken: '3', minH: 10, maxH: 90, childIndex: 2, level: 1, x: 0, y: 1, z: null },
    { parentToken: '3', minH: 10, maxH: 90, childIndex: 4, level: 1, x: 0, y: 0, z: 1 },
  ].map((c) => {
    const parentTile = {
      _boundingVolume: new C.TileBoundingS2Cell({
        token: c.parentToken,
        minimumHeight: c.minH,
        maximumHeight: c.maxH,
      }),
    };
    const out = Implicit3DTileContent._deriveBoundingVolumeS2(
      false,
      parentTile,
      c.childIndex,
      c.level,
      c.x,
      c.y,
      c.z ?? undefined,
    );
    return { ...c, result: out.extensions['3DTILES_bounding_volume_S2'] };
  });

  // Helper to collect expanded tree from a placeholder Cesium3DTile
  function dumpTileTree(tile) {
    return {
      coords: tile.implicitCoordinates
        ? {
            level: tile.implicitCoordinates.level,
            x: tile.implicitCoordinates.x,
            y: tile.implicitCoordinates.y,
            z: tile.implicitCoordinates.z ?? null,
          }
        : null,
      isPlaceholder: tile.implicitTileset !== undefined,
      geometricError: num(tile.geometricError),
      refine: tile.refine === Cesium3DTileRefine.ADD ? 'ADD' : 'REPLACE',
      boundingVolume: tile._header.boundingVolume,
      contents: (tile._header.contents ?? []).map((c) => ({
        uri: c.uri,
        boundingVolume: c.boundingVolume ?? null,
      })),
      boundingSphere: sphereObj(tile.boundingSphere),
      hasImplicitContentMetadata: tile.hasImplicitContentMetadata,
      tileMetaHeight: tile.metadata ? tile.metadata.getProperty('height') ?? null : null,
      children: tile.children.map(dumpTileTree),
    };
  }

  // Case A: Quadtree JSON subtree (subtreeLevels=2) with sparse tile availability,
  // partial content availability, child subtrees, and tile/content metadata semantics!
  const schemaJson = {
    id: 'implicit_schema',
    classes: {
      tileClass: {
        properties: {
          height: { type: 'SCALAR', componentType: 'FLOAT32' },
          minH: { type: 'SCALAR', componentType: 'FLOAT32', semantic: 'TILE_MINIMUM_HEIGHT' },
          maxH: { type: 'SCALAR', componentType: 'FLOAT32', semantic: 'TILE_MAXIMUM_HEIGHT' },
          customError: { type: 'SCALAR', componentType: 'FLOAT32', semantic: 'TILE_GEOMETRIC_ERROR' },
        },
      },
      contentClass: {
        properties: {
          cMinH: { type: 'SCALAR', componentType: 'FLOAT32', semantic: 'CONTENT_MINIMUM_HEIGHT' },
          cMaxH: { type: 'SCALAR', componentType: 'FLOAT32', semantic: 'CONTENT_MAXIMUM_HEIGHT' },
          label: { type: 'STRING' },
        },
      },
    },
  };
  const schema = C.MetadataSchema.fromJson(schemaJson);

  // Build binary buffer for quadtree subtree:
  // Level 0: 1 tile (bit 0 = 1)
  // Level 1: 4 tiles (bits 1..4 = [1, 0, 1, 1] -> morton 0, 2, 3 available -> 4 available tiles total)
  // Tile availability byte: 0b00011011 = 0x1b (bits: 0=1, 1=1, 2=0, 3=1, 4=1)
  // Content availability byte: 0b00010011 = 0x13 (bits: 0=1, 1=1, 2=0, 3=0, 4=1 -> 3 available contents)
  // Child subtree availability (16 bits = 2 bytes): morton 0's child 0 (idx 0) and morton 3's child 2 (idx 3*4+2=14)
  //   byte0 = 0b00000001 = 0x01, byte1 = 0b01000000 = 0x40
  const qtBuf = new ArrayBuffer(128);
  const qtBytes = new Uint8Array(qtBuf);
  const qtView = new DataView(qtBuf);
  // 0..1: tileAvailability (1 byte)
  qtBytes[0] = 0x1b;
  // 1..2: contentAvailability (1 byte)
  qtBytes[1] = 0x13;
  // 2..4: childSubtreeAvailability (2 bytes)
  qtBytes[2] = 0x01;
  qtBytes[3] = 0x40;
  // 4..20: tileClass height (4 x f32)
  [10.5, 20.5, 30.5, 40.5].forEach((v, i) => qtView.setFloat32(4 + i * 4, v, true));
  // 20..36: tileClass minH (4 x f32)
  [5, 10, 15, 20].forEach((v, i) => qtView.setFloat32(20 + i * 4, v, true));
  // 36..52: tileClass maxH (4 x f32)
  [105, 110, 115, 120].forEach((v, i) => qtView.setFloat32(36 + i * 4, v, true));
  // 52..68: tileClass customError (4 x f32)
  [90, 42, 38, 34].forEach((v, i) => qtView.setFloat32(52 + i * 4, v, true));
  // 68..80: contentClass cMinH (3 x f32)
  [12, 22, 32].forEach((v, i) => qtView.setFloat32(68 + i * 4, v, true));
  // 80..92: contentClass cMaxH (3 x f32)
  [82, 92, 102].forEach((v, i) => qtView.setFloat32(80 + i * 4, v, true));
  // 92..108: contentClass label stringOffsets (4 x u32): "c0"(2), "c1"(2), "c4"(2)
  [0, 2, 4, 6].forEach((v, i) => qtView.setUint32(92 + i * 4, v, true));
  // 108..114: contentClass label values "c0c1c4"
  qtBytes.set(new TextEncoder().encode('c0c1c4'), 108);

  const qtSubtreeJson = {
    buffers: [{ byteLength: 128, uri: `data:application/octet-stream;base64,${toBase64(qtBytes)}` }],
    bufferViews: [
      { buffer: 0, byteOffset: 0, byteLength: 1 }, // 0: tileAvail
      { buffer: 0, byteOffset: 1, byteLength: 1 }, // 1: contentAvail
      { buffer: 0, byteOffset: 2, byteLength: 2 }, // 2: childSubtreeAvail
      { buffer: 0, byteOffset: 4, byteLength: 16 }, // 3: height
      { buffer: 0, byteOffset: 20, byteLength: 16 }, // 4: minH
      { buffer: 0, byteOffset: 36, byteLength: 16 }, // 5: maxH
      { buffer: 0, byteOffset: 52, byteLength: 16 }, // 6: customError
      { buffer: 0, byteOffset: 68, byteLength: 12 }, // 7: cMinH
      { buffer: 0, byteOffset: 80, byteLength: 12 }, // 8: cMaxH
      { buffer: 0, byteOffset: 92, byteLength: 16 }, // 9: label stringOffsets
      { buffer: 0, byteOffset: 108, byteLength: 6 }, // 10: label values
    ],
    tileAvailability: { bitstream: 0, availableCount: 4 },
    contentAvailability: [{ bitstream: 1, availableCount: 3 }],
    childSubtreeAvailability: { bitstream: 2, availableCount: 2 },
    propertyTables: [
      {
        class: 'tileClass',
        count: 4,
        properties: {
          height: { values: 3 },
          minH: { values: 4 },
          maxH: { values: 5 },
          customError: { values: 6 },
        },
      },
      {
        class: 'contentClass',
        count: 3,
        properties: {
          cMinH: { values: 7 },
          cMaxH: { values: 8 },
          label: { values: 10, stringOffsets: 9 },
        },
      },
    ],
    tileMetadata: 0,
    contentMetadata: [1],
  };

  const qtSubtreeJsonSnapshot = JSON.parse(JSON.stringify(qtSubtreeJson));
  const qtBaseResource = new C.Resource({ url: 'https://example.com/implicit-qt/tileset.json' });
  const qtRootHeader = {
    boundingVolume: { region: rootRegion },
    geometricError: 100,
    refine: 'REPLACE',
    content: { uri: 'content/{level}_{x}_{y}.glb' },
    implicitTiling: {
      subdivisionScheme: 'QUADTREE',
      subtreeLevels: 2,
      availableLevels: 4,
      subtrees: { uri: 'subtrees/{level}_{x}_{y}.json' },
    },
  };
  const qtImplicitTileset = new ImplicitTileset(qtBaseResource, qtRootHeader, schema);
  const qtRootCoords = new ImplicitTileCoordinates({
    subdivisionScheme: qtImplicitTileset.subdivisionScheme,
    subtreeLevels: qtImplicitTileset.subtreeLevels,
    level: 0,
    x: 0,
    y: 0,
  });
  const qtTilesetMock = {
    modelMatrix: C.Matrix4.IDENTITY,
    _geometricError: 100,
    _scaledGeometricError: 100,
    metadata: undefined,
    schema,
    statistics: { numberOfTilesTotal: 1 },
  };
  const qtPlaceholder = new Cesium3DTile(qtTilesetMock, qtBaseResource, qtRootHeader, undefined);
  qtPlaceholder.implicitTileset = qtImplicitTileset;
  qtPlaceholder.implicitCoordinates = qtRootCoords;
  const qtSubtreeResource = qtImplicitTileset.subtreeUriTemplate.getDerivedResource({
    templateValues: qtRootCoords.getTemplateValues(),
  });
  const qtContent = await Implicit3DTileContent.fromSubtreeJson(
    qtTilesetMock,
    qtPlaceholder,
    qtSubtreeResource,
    qtSubtreeJson,
    undefined,
    0,
  );
  const qtSummary = {
    url: qtContent.url,
    ready: qtContent.ready,
    ownCounts: [
      qtContent.featuresLength,
      qtContent.pointsLength,
      qtContent.trianglesLength,
      qtContent.geometryByteLength,
      qtContent.texturesByteLength,
      qtContent.batchTableByteLength,
    ],
    hasProperty: qtContent.hasProperty(0, 'height'),
    getFeature: qtContent.getFeature(0) ?? null,
    pick: qtContent.pick(new C.Ray(C.Cartesian3.ZERO, C.Cartesian3.UNIT_X), {}) ?? null,
    totalTilesAfterExpand: qtTilesetMock.statistics.numberOfTilesTotal,
    tree: dumpTileTree(qtPlaceholder.children[0]),
  };
  qtContent.destroy();
  qtSummary.destroyed = qtContent.isDestroyed();

  // Case B: Octree binary `.subtree` payload (subtreeLevels=2, box bounding volume, geometric error halving)
  // Level 0: 1 tile (bit 0 = 1)
  // Level 1: 8 tiles (bits 1..8 -> morton 0, 5, 7 available => bit indices 1, 6, 8)
  //   byte0 (bits 0..7) = 0b01000011 = 0x43 (bits 0, 1, 6)
  //   byte1 (bits 8..15) = 0b00000001 = 0x01 (bit 8)
  // Content availability: constant 1
  // Child subtree availability: constant 0
  const octSubtreeJsonObj = {
    buffers: [{ byteLength: 8 }],
    bufferViews: [{ buffer: 0, byteOffset: 0, byteLength: 2 }],
    tileAvailability: { bitstream: 0, availableCount: 4 },
    contentAvailability: [{ constant: 1 }],
    childSubtreeAvailability: { constant: 0 },
  };
  let octJsonBytes = new TextEncoder().encode(JSON.stringify(octSubtreeJsonObj));
  while (octJsonBytes.length % 8 !== 0) octJsonBytes = new Uint8Array([...octJsonBytes, 0x20]);
  const octBinBytes = new Uint8Array(8);
  octBinBytes[0] = 0x43;
  octBinBytes[1] = 0x01;
  const octSubtreeBinary = new Uint8Array(24 + octJsonBytes.length + octBinBytes.length);
  const octView = new DataView(octSubtreeBinary.buffer);
  octSubtreeBinary.set(new TextEncoder().encode('subt'), 0);
  octView.setUint32(4, 1, true);
  octView.setBigUint64(8, BigInt(octJsonBytes.length), true);
  octView.setBigUint64(16, BigInt(octBinBytes.length), true);
  octSubtreeBinary.set(octJsonBytes, 24);
  octSubtreeBinary.set(octBinBytes, 24 + octJsonBytes.length);

  const octBaseResource = new C.Resource({ url: 'https://example.com/implicit-oct/tileset.json' });
  const octRootHeader = {
    boundingVolume: { box: rootBox },
    geometricError: 64,
    refine: 'ADD',
    content: { uri: 'content/{level}_{x}_{y}_{z}.b3dm' },
    implicitTiling: {
      subdivisionScheme: 'OCTREE',
      subtreeLevels: 2,
      availableLevels: 2,
      subtrees: { uri: 'subtrees/{level}_{x}_{y}_{z}.subtree' },
    },
  };
  const octImplicitTileset = new ImplicitTileset(octBaseResource, octRootHeader);
  const octRootCoords = new ImplicitTileCoordinates({
    subdivisionScheme: octImplicitTileset.subdivisionScheme,
    subtreeLevels: octImplicitTileset.subtreeLevels,
    level: 0,
    x: 0,
    y: 0,
    z: 0,
  });
  const octTilesetMock = {
    modelMatrix: C.Matrix4.IDENTITY,
    _geometricError: 64,
    _scaledGeometricError: 64,
    metadata: undefined,
    schema: undefined,
    statistics: { numberOfTilesTotal: 1 },
  };
  const octPlaceholder = new Cesium3DTile(octTilesetMock, octBaseResource, octRootHeader, undefined);
  octPlaceholder.implicitTileset = octImplicitTileset;
  octPlaceholder.implicitCoordinates = octRootCoords;
  const octSubtreeResource = octImplicitTileset.subtreeUriTemplate.getDerivedResource({
    templateValues: octRootCoords.getTemplateValues(),
  });
  const octContent = await Implicit3DTileContent.fromSubtreeJson(
    octTilesetMock,
    octPlaceholder,
    octSubtreeResource,
    undefined,
    octSubtreeBinary.buffer,
    0,
  );
  const octSummary = {
    url: octContent.url,
    ready: octContent.ready,
    totalTilesAfterExpand: octTilesetMock.statistics.numberOfTilesTotal,
    tree: dumpTileTree(octPlaceholder.children[0]),
  };
  octContent.destroy();

  return {
    rootBox,
    rootRegion,
    boxDerivations,
    regionDerivations,
    s2Derivations,
    schemaJson,
    qtRootHeader,
    qtSubtreeJson: qtSubtreeJsonSnapshot,
    qtSummary,
    octRootHeader,
    octSubtreeBinaryBase64: toBase64(octSubtreeBinary),
    octSummary,
  };
}

// ============================================================================
// 7. Scene/Multiple3DTileContent (pure Node)
// ============================================================================
function captureMultiple3DTileContent() {
  const calls = [];
  const makeChild = (id, initialReady, hitX) => ({
    ready: initialReady,
    featurePropertiesDirty: false,
    update() { calls.push(`update${id}`); },
    applyStyle(s) { calls.push(`style${id}:${s?.name ?? 'none'}`); },
    applyDebugSettings(e, c) { calls.push(`debug${id}:${e}:${c.red}`); },
    pick(_ray, _frame, result) {
      if (hitX === null) return undefined;
      return C.Cartesian3.clone(new C.Cartesian3(hitX, 0, 0), result);
    },
    destroy() { calls.push(`destroy${id}`); },
  });

  const baseResource = new C.Resource({ url: 'https://example.com/multi/tileset.json' });
  const contentsJson = {
    contents: [{ uri: 'buildings.b3dm' }, { uri: 'trees.i3dm' }],
  };
  const mc = new Multiple3DTileContent({}, {}, baseResource, contentsJson);
  const ownCounts = [
    mc.featuresLength, mc.pointsLength, mc.trianglesLength,
    mc.geometryByteLength, mc.texturesByteLength, mc.batchTableByteLength,
  ];
  const innerContentUrls = mc.innerContentUrls;
  const initialReadyBeforeCreated = mc.ready;

  // Attach mock inner contents and mark created (simulating resolved inner fetches)
  const c0 = makeChild(0, true, 9);
  const c1 = makeChild(1, false, 3);
  mc._contents = [c0, c1];
  mc._contentsCreated = true;

  const ray = new C.Ray(C.Cartesian3.ZERO, C.Cartesian3.UNIT_X);
  const initialReady = mc.ready;
  const pickUnready = mc.pick(ray, {}, new C.Cartesian3()) ?? null;

  mc.update({}, {});
  const afterFirstUpdateReady = mc.ready;

  c1.ready = true;
  mc.update({}, {});
  const afterSecondUpdateReady = mc.ready;
  const pickReady = mc.pick(ray, {}, new C.Cartesian3()).x;

  // Sticky ready even if c0 flips to false
  c0.ready = false;
  mc.update({}, {});
  const stickyReady = mc.ready;

  // featurePropertiesDirty get/set
  const dirtyInitial = mc.featurePropertiesDirty;
  c1.featurePropertiesDirty = true;
  const dirtyWhenChildDirty = mc.featurePropertiesDirty;
  mc.featurePropertiesDirty = false;
  const dirtyAfterClear = [mc.featurePropertiesDirty, c0.featurePropertiesDirty, c1.featurePropertiesDirty];

  // Forwarding style & debug
  mc.applyStyle({ name: 'testStyle' });
  mc.applyDebugSettings(true, new C.Color(0.5, 0.25, 0.75, 1.0));

  const hasProperty = mc.hasProperty(0, 'x');
  const getFeature = mc.getFeature(0) ?? null;
  const url = mc.url ?? null;

  mc.destroy();
  const destroyed = mc.isDestroyed();

  return {
    ownCounts,
    innerContentUrls,
    initialReadyBeforeCreated,
    initialReady,
    pickUnready,
    afterFirstUpdateReady,
    afterSecondUpdateReady,
    pickReady,
    stickyReady,
    dirtyInitial,
    dirtyWhenChildDirty,
    dirtyAfterClear,
    hasProperty,
    getFeature,
    url,
    calls,
    destroyed,
  };
}

// ============================================================================
// 8. Scene/Composite3DTileContent (pure Node - nested + errors + metadata)
// ============================================================================
async function captureComposite3DTileContent() {
  const enc = new TextEncoder();
  const makeLeaf = (magic) => {
    const b = new Uint8Array(12);
    const v = new DataView(b.buffer);
    b.set(enc.encode(magic), 0);
    v.setUint32(4, 1, true);
    v.setUint32(8, 12, true);
    return b;
  };
  const makeCmpt = (children, version = 1) => {
    const total = 16 + children.reduce((n, c) => n + c.length, 0);
    const b = new Uint8Array(total);
    const v = new DataView(b.buffer);
    b.set(enc.encode('cmpt'), 0);
    v.setUint32(4, version, true);
    v.setUint32(8, total, true);
    v.setUint32(12, children.length, true);
    let o = 16;
    for (const c of children) { b.set(c, o); o += c.length; }
    return b;
  };

  // Nested composite: outer cmpt contains [inner cmpt(b3dm, i3dm), vctr], embedded at byteOffset 8
  const inner = makeCmpt([makeLeaf('b3dm'), makeLeaf('i3dm')]);
  const outer = makeCmpt([inner, makeLeaf('vctr')]);
  const padded = new Uint8Array(8 + outer.length);
  padded.set(outer, 8);

  const visited = [];
  const factory = {
    b3dm(_ts, _t, res, _buf, off) {
      visited.push({ magic: 'b3dm', offset: off, url: res.getUrlComponent(true) });
      return { ready: true, featurePropertiesDirty: false, metadata: undefined, group: undefined, update() {}, destroy() {} };
    },
    i3dm(_ts, _t, res, _buf, off) {
      visited.push({ magic: 'i3dm', offset: off, url: res.getUrlComponent(true) });
      return { ready: true, featurePropertiesDirty: false, metadata: undefined, group: undefined, update() {}, destroy() {} };
    },
    vctr(_ts, _t, res, _buf, off) {
      visited.push({ magic: 'vctr', offset: off, url: res.getUrlComponent(true) });
      return { ready: true, featurePropertiesDirty: false, metadata: undefined, group: undefined, update() {}, destroy() {} };
    },
    cmpt(ts, t, res, buf, off) {
      visited.push({ magic: 'cmpt', offset: off, url: res.getUrlComponent(true) });
      return Composite3DTileContent.fromTileType(ts, t, res, buf, off, factory);
    },
  };

  const nested = await Composite3DTileContent.fromTileType(
    {}, {}, new C.Resource({ url: 'https://example.com/nested.cmpt?k=v' }), padded.buffer, 8, factory,
  );
  const metaToken = { tag: 'content-meta' };
  const groupToken = { tag: 'group-meta' };
  nested.metadata = metaToken;
  nested.group = groupToken;
  const metaPropagated = [
    nested.innerContents[0].metadata === metaToken,
    nested.innerContents[0].innerContents[0].metadata === metaToken,
    nested.innerContents[0].innerContents[1].metadata === metaToken,
    nested.innerContents[1].metadata === metaToken,
    nested.innerContents[0].innerContents[0].group === groupToken,
    nested.innerContents[1].group === groupToken,
  ];
  nested.innerContents[0].innerContents[1].featurePropertiesDirty = true;
  const dirtyNested = nested.featurePropertiesDirty;
  nested.featurePropertiesDirty = false;
  const dirtyCleared = nested.featurePropertiesDirty;
  const ownCounts = [
    nested.featuresLength, nested.pointsLength, nested.trianglesLength,
    nested.geometryByteLength, nested.texturesByteLength, nested.batchTableByteLength,
  ];
  const hasProperty = nested.hasProperty(0, 'x');
  const getFeature = nested.getFeature(0) ?? null;
  nested.destroy();

  // Unknown inner content type error
  const badChild = makeCmpt([makeLeaf('xxxx')]);
  let unknownError = null;
  try {
    await Composite3DTileContent.fromTileType({}, {}, new C.Resource({ url: 'https://example.com/bad.cmpt' }), badChild.buffer, 0, factory);
  } catch (e) { unknownError = e.message; }

  // Version error
  const badVer = makeCmpt([makeLeaf('b3dm')], 2);
  let versionError = null;
  try {
    await Composite3DTileContent.fromTileType({}, {}, new C.Resource({ url: 'https://example.com/badv.cmpt' }), badVer.buffer, 0, factory);
  } catch (e) { versionError = e.message; }

  return {
    paddedBase64: toBase64(padded),
    badChildBase64: toBase64(badChild),
    badVerBase64: toBase64(badVer),
    visited,
    metaPropagated,
    dirtyNested,
    dirtyCleared,
    ownCounts,
    hasProperty,
    getFeature,
    unknownError,
    versionError,
  };
}

// ============================================================================
// 9. Scene/Geometry3DTileContent (Node with real createVectorTileGeometries worker)
// ============================================================================
async function captureGeometry3DTileContent() {
  const prevMaxTex = C.ContextLimits._maximumTextureSize;
  C.ContextLimits._maximumTextureSize = 4096;
  const geomWorker = await loadWorker('createVectorTileGeometries');
  const origSchedule = TaskProcessor.prototype.scheduleTask;
  TaskProcessor.prototype.scheduleTask = function (parameters) {
    const flushed = {};
    for (const [k, v] of Object.entries(parameters)) {
      if (ArrayBuffer.isView(v)) flushed[k] = v.buffer;
      else flushed[k] = v;
    }
    return invoke(geomWorker, flushed).then(({ result }) => result);
  };

  function buildGeomTile({ version = 1, byteLengthOverride, featureTable, featureBinary = new Uint8Array(0), batchTable, batchTableBinary = new Uint8Array(0), emptyFeatureJson = false }) {
    const enc = new TextEncoder();
    let ftJson = emptyFeatureJson ? new Uint8Array(0) : enc.encode(JSON.stringify(featureTable));
    while (ftJson.length % 8 !== 0) ftJson = new Uint8Array([...ftJson, 0x20]);
    let btJson = batchTable === undefined ? new Uint8Array(0) : enc.encode(JSON.stringify(batchTable));
    while (btJson.length % 8 !== 0) btJson = new Uint8Array([...btJson, 0x20]);
    const headerLen = 28;
    const total = headerLen + ftJson.length + featureBinary.length + btJson.length + batchTableBinary.length;
    const bytes = new Uint8Array(total);
    const view = new DataView(bytes.buffer);
    bytes.set(enc.encode('geom'), 0);
    view.setUint32(4, version, true);
    view.setUint32(8, byteLengthOverride ?? total, true);
    view.setUint32(12, ftJson.length, true);
    view.setUint32(16, featureBinary.length, true);
    view.setUint32(20, btJson.length, true);
    view.setUint32(24, batchTableBinary.length, true);
    let o = headerLen;
    bytes.set(ftJson, o); o += ftJson.length;
    bytes.set(featureBinary, o); o += featureBinary.length;
    bytes.set(btJson, o); o += btJson.length;
    bytes.set(batchTableBinary, o);
    return bytes;
  }

  const makeMockTilesetTile = () => {
    const tileset = {
      classificationType: C.ClassificationType.BOTH,
      debugWireframe: false,
      statistics: { batchTableByteLength: 0 },
    };
    const tile = {
      computedTransform: C.Matrix4.IDENTITY,
      boundingVolume: { boundingVolume: new C.BoundingSphere(C.Cartesian3.ZERO, 100) },
      tileset,
    };
    return { tileset, tile };
  };

  try {
    // Case 1: Empty geom (byteLength = 0)
    const emptyBytes = new Uint8Array(28);
    emptyBytes.set(new TextEncoder().encode('geom'), 0);
    const ev = new DataView(emptyBytes.buffer);
    ev.setUint32(4, 1, true);
    ev.setUint32(8, 0, true);
    const { tileset: ts0, tile: t0 } = makeMockTilesetTile();
    const emptyContent = new Geometry3DTileContent(ts0, t0, new C.Resource({ url: 'https://example.com/empty.geom' }), emptyBytes.buffer, 0);
    const emptyCase = {
      bytesBase64: toBase64(emptyBytes),
      ready: emptyContent.ready,
      featuresLength: emptyContent.featuresLength,
      pointsLength: emptyContent.pointsLength,
      trianglesLength: emptyContent.trianglesLength,
      geometryByteLength: emptyContent.geometryByteLength,
      texturesByteLength: emptyContent.texturesByteLength,
      batchTableByteLength: emptyContent.batchTableByteLength,
      pick: emptyContent.pick(new C.Ray(C.Cartesian3.ZERO, C.Cartesian3.UNIT_X), {}) ?? null,
    };
    emptyContent.destroy();
    emptyCase.destroyed = emptyContent.isDestroyed();

    // Case 2: All 4 solid types (1 box + 1 cylinder + 1 ellipsoid + 1 sphere) with RTC_CENTER + JSON & binary batchTable + hierarchy
    const boxMat = C.Matrix4.fromTranslation(new C.Cartesian3(105, 200, 300));
    const cylMat = C.Matrix4.fromTranslation(new C.Cartesian3(110, 205, 300));
    const ellMat = C.Matrix4.fromTranslation(new C.Cartesian3(115, 210, 305));
    const boxF32 = new Float32Array([2, 4, 6, ...C.Matrix4.pack(boxMat, new Array(16))]);
    const cylF32 = new Float32Array([1.5, 5.0, ...C.Matrix4.pack(cylMat, new Array(16))]);
    const ellF32 = new Float32Array([2.5, 3.0, 1.5, ...C.Matrix4.pack(ellMat, new Array(16))]);
    const sphF32 = new Float32Array([3.5, 120, 215, 310]);

    const ftBinLen = boxF32.byteLength + cylF32.byteLength + ellF32.byteLength + sphF32.byteLength;
    const ftBin = new Uint8Array(ftBinLen);
    let fo = 0;
    const offBoxes = fo; ftBin.set(new Uint8Array(boxF32.buffer), fo); fo += boxF32.byteLength;
    const offCyls = fo; ftBin.set(new Uint8Array(cylF32.buffer), fo); fo += cylF32.byteLength;
    const offElls = fo; ftBin.set(new Uint8Array(ellF32.buffer), fo); fo += ellF32.byteLength;
    const offSphs = fo; ftBin.set(new Uint8Array(sphF32.buffer), fo);

    const ftJson = {
      RTC_CENTER: [100, 200, 300],
      BOXES_LENGTH: 1,
      BOXES: { byteOffset: offBoxes },
      CYLINDERS_LENGTH: 1,
      CYLINDERS: { byteOffset: offCyls },
      ELLIPSOIDS_LENGTH: 1,
      ELLIPSOIDS: { byteOffset: offElls },
      SPHERES_LENGTH: 1,
      SPHERES: { byteOffset: offSphs },
    };

    const btBin = new Uint8Array(16);
    const btView = new DataView(btBin.buffer);
    [11.5, 22.5, 33.5, 44.5].forEach((v, i) => btView.setFloat32(i * 4, v, true));
    const btJson = {
      kind: ['box', 'cylinder', 'ellipsoid', 'sphere'],
      weight: { byteOffset: 0, componentType: 'FLOAT', type: 'SCALAR' },
      extensions: {
        '3DTILES_batch_table_hierarchy': {
          classes: [
            { name: 'Solid', length: 4, instances: { category: ['geom', 'geom', 'geom', 'geom'] } },
          ],
          instancesLength: 4,
          classIds: [0, 0, 0, 0],
        },
      },
    };

    const allSolidsBytes = buildGeomTile({
      featureTable: ftJson,
      featureBinary: ftBin,
      batchTable: btJson,
      batchTableBinary: btBin,
    });
    const { tileset: ts1, tile: t1 } = makeMockTilesetTile();
    const allContent = new Geometry3DTileContent(ts1, t1, new C.Resource({ url: 'https://example.com/all.geom' }), allSolidsBytes.buffer, 0);
    // Run the Vector3DTileGeometry worker via update()
    allContent._geometries.update({ commandList: [] });
    await allContent._geometries._promise;
    const prim = allContent._geometries._primitive;
    // Feature properties + style
    const f0 = allContent.getFeature(0);
    const f3 = allContent.getFeature(3);
    const style = new C.Cesium3DTileStyle({
      color: "${weight} > 20 ? color('red') : color('blue', 0.5)",
      show: "${weight} > 20",
    });
    allContent.applyStyle(style);
    const styleShowAfter = [0, 1, 2, 3].map((i) => allContent.getFeature(i).show);
    const styleColorsAfter = [0, 1, 2, 3].map((i) => colorArr(allContent.getFeature(i).color));
    const allSolidsCase = {
      bytesBase64: toBase64(allSolidsBytes),
      rtcCenter: [100, 200, 300],
      featuresLength: allContent.featuresLength,
      pointsLength: allContent.pointsLength,
      trianglesLength: allContent.trianglesLength,
      geometryByteLength: allContent.geometryByteLength,
      texturesByteLength: allContent.texturesByteLength,
      batchTableByteLength: allContent.batchTableByteLength,
      positionsLength: prim._positions.length,
      positionsSample: Array.from(prim._positions.slice(0, 18), num),
      positionsHash: fnv(new Uint8Array(prim._positions.buffer, prim._positions.byteOffset, prim._positions.byteLength)),
      vertexBatchIdsLength: prim._vertexBatchIds.length,
      vertexBatchIdsHash: fnv(new Uint8Array(prim._vertexBatchIds.buffer, prim._vertexBatchIds.byteOffset, prim._vertexBatchIds.byteLength)),
      indicesLength: prim._indices.length,
      indicesHash: fnv(new Uint8Array(prim._indices.buffer, prim._indices.byteOffset, prim._indices.byteLength)),
      indexOffsets: Array.from(prim._indexOffsets),
      indexCounts: Array.from(prim._indexCounts),
      batchIds: Array.from(prim._batchIds),
      feature0: {
        kind: f0.getProperty('kind'),
        weight: num(f0.getProperty('weight')),
        category: f0.getProperty('category'),
        exactClass: f0.getExactClassName(),
        hasWeight: allContent.hasProperty(0, 'weight'),
        hasMissing: allContent.hasProperty(0, 'missing'),
      },
      feature3: {
        kind: f3.getProperty('kind'),
        weight: num(f3.getProperty('weight')),
      },
      styleShowAfter,
      styleColorsAfter,
    };
    allContent.destroy();
    allSolidsCase.destroyed = allContent.isDestroyed();

    // Case 3: Explicit batch IDs (2 boxes + 1 sphere with custom batch IDs [2, 0] and [1])
    const box2F32 = new Float32Array([
      1, 1, 1, ...C.Matrix4.pack(C.Matrix4.fromTranslation(new C.Cartesian3(1, 0, 0)), new Array(16)),
      2, 2, 2, ...C.Matrix4.pack(C.Matrix4.fromTranslation(new C.Cartesian3(5, 0, 0)), new Array(16)),
    ]);
    const sph1F32 = new Float32Array([2.0, 0, 4, 0]);
    const boxIds = new Uint16Array([2, 0]);
    const sphIds = new Uint16Array([1]);
    const expBin = new Uint8Array(box2F32.byteLength + sph1F32.byteLength + boxIds.byteLength + sphIds.byteLength);
    let eo = 0;
    const eBoxes = eo; expBin.set(new Uint8Array(box2F32.buffer), eo); eo += box2F32.byteLength;
    const eSphs = eo; expBin.set(new Uint8Array(sph1F32.buffer), eo); eo += sph1F32.byteLength;
    const eBoxIds = eo; expBin.set(new Uint8Array(boxIds.buffer), eo); eo += boxIds.byteLength;
    const eSphIds = eo; expBin.set(new Uint8Array(sphIds.buffer), eo);
    const explicitBytes = buildGeomTile({
      featureTable: {
        BOXES_LENGTH: 2,
        BOXES: { byteOffset: eBoxes },
        BOX_BATCH_IDS: { byteOffset: eBoxIds },
        SPHERES_LENGTH: 1,
        SPHERES: { byteOffset: eSphs },
        SPHERE_BATCH_IDS: { byteOffset: eSphIds },
      },
      featureBinary: expBin,
      batchTable: { id: ['zero', 'one', 'two'] },
    });
    const { tileset: ts2, tile: t2 } = makeMockTilesetTile();
    const expContent = new Geometry3DTileContent(ts2, t2, new C.Resource({ url: 'https://example.com/exp.geom' }), explicitBytes.buffer, 0);
    expContent._geometries.update({ commandList: [] });
    await expContent._geometries._promise;
    const expPrim = expContent._geometries._primitive;
    const explicitCase = {
      bytesBase64: toBase64(explicitBytes),
      featuresLength: expContent.featuresLength,
      trianglesLength: expContent.trianglesLength,
      geometryByteLength: expContent.geometryByteLength,
      batchIds: Array.from(expPrim._batchIds),
      indexOffsets: Array.from(expPrim._indexOffsets),
      indexCounts: Array.from(expPrim._indexCounts),
      featureIds: [0, 1, 2].map((i) => expContent.getFeature(i).getProperty('id')),
    };
    expContent.destroy();

    // Error cases
    const badVersionBytes = buildGeomTile({ version: 2, featureTable: { BOXES_LENGTH: 0 } });
    const emptyFtBytes = buildGeomTile({ emptyFeatureJson: true });
    const partialBatchIdsBytes = buildGeomTile({
      featureTable: {
        BOXES_LENGTH: 1,
        BOXES: { byteOffset: 0 },
        BOX_BATCH_IDS: { byteOffset: boxF32.byteLength },
        SPHERES_LENGTH: 1,
        SPHERES: { byteOffset: boxF32.byteLength + 4 },
      },
      featureBinary: new Uint8Array(boxF32.byteLength + 4 + sphF32.byteLength),
    });
    const errors = {};
    for (const [label, b] of [
      ['badVersion', badVersionBytes],
      ['emptyFeatureTable', emptyFtBytes],
      ['partialBatchIds', partialBatchIdsBytes],
    ]) {
      try {
        const { tileset: tse, tile: te } = makeMockTilesetTile();
        new Geometry3DTileContent(tse, te, new C.Resource({ url: `https://example.com/${label}.geom` }), b.buffer, 0);
        errors[label] = { bytesBase64: toBase64(b), error: null };
      } catch (e) {
        errors[label] = { bytesBase64: toBase64(b), error: e.message };
      }
    }

    return { emptyCase, allSolidsCase, explicitCase, errors };
  } finally {
    TaskProcessor.prototype.scheduleTask = origSchedule;
    C.ContextLimits._maximumTextureSize = prevMaxTex;
  }
}

// ============================================================================
// 2, 4, 5. BatchTable, Feature, PointFeature on real SwiftShader WebGL2
// ============================================================================
async function captureWebGLModules() {
  const modules = {
    Scene: [
      'Cesium3DTileBatchTable',
      'Cesium3DTileFeature',
      'Cesium3DTilePointFeature',
      'Cesium3DTileStyle',
      'BillboardCollection',
      'LabelCollection',
      'PolylineCollection',
      'LabelStyle',
      'HorizontalOrigin',
      'VerticalOrigin',
      'Material',
      'MetadataSchema',
      'ContentMetadata',
      'TileMetadata',
      'GroupMetadata',
      'TilesetMetadata',
      'Cesium3DContentGroup',
      'ImplicitSubtreeMetadata',
    ],
    Renderer: ['Context', 'ContextLimits'],
    Core: [
      'Cartesian2',
      'Cartesian3',
      'Cartesian4',
      'Cartographic',
      'Color',
      'DistanceDisplayCondition',
      'Ellipsoid',
      'Matrix2',
      'Matrix3',
      'Matrix4',
      'NearFarScalar',
    ],
  };

  const contents = Object.entries(modules)
    .flatMap(([dir, names]) =>
      names.map((n) => `export {default as ${n}} from '@cesium/engine/Source/${dir}/${n}.js';`),
    )
    .join('\n');

  const bundle = await build({
    stdin: { contents, resolveDir: fileURLToPath(new URL('.', import.meta.url)) },
    bundle: true,
    write: false,
    format: 'esm',
    platform: 'browser',
  });

  const server = createServer((req, res) => {
    if (req.url === '/reference.mjs') {
      res.setHeader('Content-Type', 'text/javascript');
      res.end(bundle.outputFiles[0].contents);
      return;
    }
    res.end('<!doctype html><title>3D Tiles Core capture</title>');
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));

  let browser;
  try {
    browser = await chromium.launch({
      executablePath: process.env.CHROMIUM_PATH,
      headless: true,
      args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
    });
    const page = await browser.newPage();
    await page.goto(`http://127.0.0.1:${server.address().port}/`);

    const evalRes = await page.evaluate(async () => {
     try {
      const ref = await import('/reference.mjs');
      ref.Cesium3DTileBatchTable._deprecationWarning = () => {};

      const num = (v) => (Number.isNaN(v) ? 'NaN' : v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Object.is(v, -0) ? 0 : v);
      const colorArr = (c) => (c ? [num(c.red), num(c.green), num(c.blue), num(c.alpha)] : null);
      const encodeVal = (v) => {
        if (v === undefined) return { kind: 'undefined' };
        if (v === null) return { kind: 'null' };
        if (typeof v === 'boolean') return { kind: 'bool', value: v };
        if (typeof v === 'number') return { kind: 'number', value: num(v) };
        if (typeof v === 'string') return { kind: 'string', value: v };
        if (v instanceof ref.Cartesian2) return { kind: 'vec2', value: [num(v.x), num(v.y)] };
        if (v instanceof ref.Cartesian3) return { kind: 'vec3', value: [num(v.x), num(v.y), num(v.z)] };
        if (v instanceof ref.Cartesian4) return { kind: 'vec4', value: [num(v.x), num(v.y), num(v.z), num(v.w)] };
        if (Array.isArray(v)) return { kind: 'array', value: v.map(encodeVal) };
        if (typeof v === 'object') return { kind: 'object', value: v };
        return { kind: 'other', value: String(v) };
      };
      const toBase64 = (bytes) => {
        let s = '';
        for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
        return btoa(s);
      };

      const canvas = document.createElement('canvas');
      canvas.width = 16;
      canvas.height = 16;
      const context = new ref.Context(canvas, { webgl: { antialias: false } });
      const maxTextureSize = ref.ContextLimits.maximumTextureSize;

      // ---- 2. Cesium3DTileBatchTable ----
      const btBin = new ArrayBuffer(256);
      const btBytes = new Uint8Array(btBin);
      const dv = new DataView(btBin);
      // count = 3
      // offset 0: b_byte (BYTE SCALAR x 3)
      dv.setInt8(0, -10); dv.setInt8(1, 0); dv.setInt8(2, 127);
      // offset 4: b_ubyte (UNSIGNED_BYTE SCALAR x 3)
      dv.setUint8(4, 1); dv.setUint8(5, 128); dv.setUint8(6, 255);
      // offset 8: b_short2 (SHORT VEC2 x 3) -> 12 bytes
      [-300, 400, 0, -1, 32000, -32000].forEach((x, i) => dv.setInt16(8 + i * 2, x, true));
      // offset 20: b_ushort3 (UNSIGNED_SHORT VEC3 x 3) -> 18 bytes
      [1, 2, 3, 100, 200, 300, 65000, 65001, 65535].forEach((x, i) => dv.setUint16(20 + i * 2, x, true));
      // offset 40: b_int (INT SCALAR x 3) -> 12 bytes
      [-100000, 0, 200000].forEach((x, i) => dv.setInt32(40 + i * 4, x, true));
      // offset 52: b_uint (UNSIGNED_INT SCALAR x 3) -> 12 bytes
      [0, 500000, 4000000000].forEach((x, i) => dv.setUint32(52 + i * 4, x, true));
      // offset 64: b_float4 (FLOAT VEC4 x 3) -> 48 bytes
      [1.5, -2.5, 3.25, 4.0, 0.5, 0.25, 0.125, 1.0, -9.0, 8.5, 7.25, 6.5].forEach((x, i) => dv.setFloat32(64 + i * 4, x, true));
      // offset 112: b_double2 (DOUBLE VEC2 x 3) -> 48 bytes
      [123456.789, -987654.321, 0.0, 1e-6, 42.5, -0.125].forEach((x, i) => dv.setFloat64(112 + i * 8, x, true));
      // offset 160: hierarchy binary property door_width (FLOAT SCALAR x 2) -> 8 bytes
      dv.setFloat32(160, 0.9, true); dv.setFloat32(164, 1.2, true);

      const batchTableJson = {
        name: ['alpha', 'beta', 'gamma'],
        active: [true, false, true],
        tags: [['a', 'b'], [], ['c']],
        meta: [{ k: 1 }, { k: 2 }, { k: 3 }],
        nullableProp: [10, null, 30],
        extras: { ignored: true },
        b_byte: { byteOffset: 0, componentType: 'BYTE', type: 'SCALAR' },
        b_ubyte: { byteOffset: 4, componentType: 'UNSIGNED_BYTE', type: 'SCALAR' },
        b_short2: { byteOffset: 8, componentType: 'SHORT', type: 'VEC2' },
        b_ushort3: { byteOffset: 20, componentType: 'UNSIGNED_SHORT', type: 'VEC3' },
        b_int: { byteOffset: 40, componentType: 'INT', type: 'SCALAR' },
        b_uint: { byteOffset: 52, componentType: 'UNSIGNED_INT', type: 'SCALAR' },
        b_float4: { byteOffset: 64, componentType: 'FLOAT', type: 'VEC4' },
        b_double2: { byteOffset: 112, componentType: 'DOUBLE', type: 'VEC2' },
        extensions: {
          '3DTILES_batch_table_hierarchy': {
            classes: [
              {
                name: 'Door',
                length: 2,
                instances: {
                  door_width: { byteOffset: 160, componentType: 'FLOAT', type: 'SCALAR' },
                  door_color: ['brown', 'white'],
                },
              },
              {
                name: 'Wall',
                length: 1,
                instances: {
                  wall_material: ['brick'],
                },
              },
              {
                name: 'Building',
                length: 1,
                instances: {
                  building_name: ['HQ'],
                  building_floors: [12],
                },
              },
            ],
            // instances: 0=Door#0, 1=Door#1, 2=Wall#0, 3=Building#0
            instancesLength: 4,
            classIds: [0, 0, 1, 2],
            parentCounts: [1, 2, 1, 0],
            // Door#0 -> Wall#0(2); Door#1 -> Wall#0(2), Building#0(3); Wall#0 -> Building#0(3)
            parentIds: [2, 2, 3, 3],
          },
        },
      };

      const batchTableJsonSnapshot = JSON.parse(JSON.stringify(batchTableJson));
      const binarySnapshotBase64 = toBase64(btBytes.slice());

      const mockContent = {
        tileset: { statistics: { batchTableByteLength: 0 }, metadata: undefined },
        tile: { metadata: undefined, implicitSubtree: undefined },
        metadata: undefined,
        group: undefined,
        featurePropertiesDirty: false,
        getFeature(id) { return features[id]; },
      };
      const bt = new ref.Cesium3DTileBatchTable(mockContent, 3, batchTableJson, btBytes);
      mockContent.batchTable = bt;
      const features = [0, 1, 2].map((id) => new ref.Cesium3DTileFeature(mockContent, id));

      const btBeforeUpdate = {
        featuresLength: bt.featuresLength,
        batchTableByteLength: bt.batchTableByteLength,
        propertyIds: [0, 1, 2].map((id) => bt.getPropertyIds(id)),
        hasProperties: [0, 1, 2].map((id) => ({
          name: bt.hasProperty(id, 'name'),
          nullableProp: bt.hasProperty(id, 'nullableProp'),
          b_float4: bt.hasProperty(id, 'b_float4'),
          door_width: bt.hasProperty(id, 'door_width'),
          wall_material: bt.hasProperty(id, 'wall_material'),
          building_name: bt.hasProperty(id, 'building_name'),
          missing: bt.hasProperty(id, 'missing'),
        })),
        classes: [0, 1, 2].map((id) => ({
          exact: bt.getExactClassName(id) ?? null,
          isExactDoor: bt.isExactClass(id, 'Door'),
          isExactWall: bt.isExactClass(id, 'Wall'),
          isClassDoor: bt.isClass(id, 'Door'),
          isClassWall: bt.isClass(id, 'Wall'),
          isClassBuilding: bt.isClass(id, 'Building'),
          isClassUnknown: bt.isClass(id, 'Unknown'),
        })),
        initialValues: [0, 1, 2].map((id) => {
          const out = {};
          for (const k of [
            'name', 'active', 'tags', 'meta', 'nullableProp',
            'b_byte', 'b_ubyte', 'b_short2', 'b_ushort3', 'b_int', 'b_uint', 'b_float4', 'b_double2',
            'door_width', 'door_color', 'wall_material', 'building_name', 'building_floors', 'missing',
          ]) {
            out[k] = encodeVal(bt.getProperty(id, k));
          }
          return out;
        }),
      };

      // Mutations: JSON, binary, own hierarchy, new property, and inherited read-only error
      bt.setProperty(0, 'name', 'alpha_updated');
      bt.setProperty(1, 'b_short2', new ref.Cartesian2(77, -88));
      bt.setProperty(2, 'b_float4', new ref.Cartesian4(1, 2, 3, 4));
      bt.setProperty(0, 'door_width', 1.75);
      bt.setProperty(0, 'door_color', 'red');
      bt.setProperty(1, 'dynamic_prop', 999);
      let inheritedSetError = null;
      try {
        bt.setProperty(0, 'building_name', 'Forbidden');
      } catch (e) {
        inheritedSetError = e.message;
      }

      const btAfterMutate = {
        name0: encodeVal(bt.getProperty(0, 'name')),
        b_short2_1: encodeVal(bt.getProperty(1, 'b_short2')),
        b_float4_2: encodeVal(bt.getProperty(2, 'b_float4')),
        door_width_0: encodeVal(bt.getProperty(0, 'door_width')),
        door_color_0: encodeVal(bt.getProperty(0, 'door_color')),
        dynamic_prop: [0, 1, 2].map((i) => encodeVal(bt.getProperty(i, 'dynamic_prop'))),
        inheritedSetError,
      };

      // Show & Color & Style & GPU Update (batchTexture + pickTexture)
      const initialShow = [0, 1, 2].map((i) => bt.getShow(i));
      const initialColor = [0, 1, 2].map((i) => colorArr(bt.getColor(i, new ref.Color())));
      bt.setShow(1, false);
      bt.setColor(0, new ref.Color(1, 0, 0, 0.5));
      bt.setColor(2, new ref.Color(0, 0.5, 1, 1));
      const afterManualShowColor = {
        show: [0, 1, 2].map((i) => bt.getShow(i)),
        color: [0, 1, 2].map((i) => colorArr(bt.getColor(i, new ref.Color()))),
        translucentCount: bt._batchTexture.translucentFeaturesLength,
      };

      // Update with pick pass enabled so both batchTexture and pickTexture allocate
      bt.update({}, { context, passes: { render: true, pick: true, postProcess: false } });
      const afterGpuUpdate = {
        batchTableByteLength: bt.batchTableByteLength,
        hasBatchTexture: bt._batchTexture.batchTexture !== undefined,
        hasPickTexture: bt._batchTexture.pickTexture !== undefined,
        pickColors: [0, 1, 2].map((i) => colorArr(bt.getPickColor(i)?.color)),
      };

      // Apply style & reset style
      const style = new ref.Cesium3DTileStyle({
        color: "${active} ? color('lime') : color('blue', 0.25)",
        show: "${b_int} >= 0",
      });
      bt.applyStyle(style);
      const afterStyle = {
        show: [0, 1, 2].map((i) => bt.getShow(i)),
        color: [0, 1, 2].map((i) => colorArr(bt.getColor(i, new ref.Color()))),
      };
      bt.applyStyle(undefined);
      const afterStyleReset = {
        show: [0, 1, 2].map((i) => bt.getShow(i)),
        color: [0, 1, 2].map((i) => colorArr(bt.getColor(i, new ref.Color()))),
      };

      // Legacy HIERARCHY property test
      const legacyBt = new ref.Cesium3DTileBatchTable(
        { tileset: { statistics: { batchTableByteLength: 0 } } },
        2,
        {
          HIERARCHY: {
            classes: [
              { name: 'Room', length: 1, instances: { room_num: [101] } },
              { name: 'Floor', length: 1, instances: { floor_num: [2] } },
            ],
            instancesLength: 2,
            classIds: [0, 1],
            parentIds: [1, 1],
          },
        },
        undefined,
      );
      const legacyHierarchy = {
        exact0: legacyBt.getExactClassName(0),
        isClassFloor0: legacyBt.isClass(0, 'Floor'),
        room0: encodeVal(legacyBt.getProperty(0, 'room_num')),
        floor0: encodeVal(legacyBt.getProperty(0, 'floor_num')),
        ids0: legacyBt.getPropertyIds(0),
      };
      legacyBt.destroy();

      // ---- 4. Cesium3DTileFeature (with full 6-level getPropertyInherited precedence) ----
      const inheritSchemaJson = {
        id: 'inherit_schema',
        classes: {
          meta: {
            properties: {
              from_content: { type: 'STRING' },
              from_tile: { type: 'STRING' },
              from_subtree: { type: 'STRING' },
              from_group: { type: 'STRING' },
              from_tileset: { type: 'STRING' },
              shared: { type: 'STRING' },
              sem_prop: { type: 'STRING', semantic: 'SHARED_SEM' },
              SHARED_SEM: { type: 'STRING' },
            },
          },
        },
      };
      const inheritSchema = ref.MetadataSchema.fromJson(inheritSchemaJson);
      const metaClass = inheritSchema.classes.meta;
      const contentMeta = new ref.ContentMetadata({
        class: metaClass,
        content: {
          properties: {
            from_content: 'content_val',
            shared: 'content_shared',
            sem_prop: 'content_by_semantic',
            SHARED_SEM: 'content_by_id',
          },
        },
      });
      const tileMeta = new ref.TileMetadata({
        class: metaClass,
        tile: { properties: { from_tile: 'tile_val', shared: 'tile_shared' } },
      });
      const subtreeMeta = new ref.ImplicitSubtreeMetadata({
        class: metaClass,
        subtreeMetadata: { properties: { from_subtree: 'subtree_val', shared: 'subtree_shared' } },
      });
      const groupMeta = new ref.GroupMetadata({
        class: metaClass,
        group: { properties: { from_group: 'group_val', shared: 'group_shared' } },
      });
      const tilesetMeta = new ref.TilesetMetadata({
        class: metaClass,
        tileset: { properties: { from_tileset: 'tileset_val', shared: 'tileset_shared' } },
      });

      mockContent.metadata = contentMeta;
      mockContent.tile.metadata = tileMeta;
      mockContent.tile.implicitSubtree = { metadata: subtreeMeta };
      mockContent.group = new ref.Cesium3DContentGroup({ metadata: groupMeta });
      mockContent.tileset.metadata = tilesetMeta;

      const f0 = features[0];
      f0.show = false;
      f0.color = new ref.Color(0.2, 0.4, 0.6, 0.8);
      f0.setProperty('clicked', true);

      const featureCapture = {
        featureId: f0.featureId,
        show: f0.show,
        color: colorArr(f0.color),
        pickColor: colorArr(f0.pickId?.color),
        featurePropertiesDirtyAfterSet: mockContent.featurePropertiesDirty,
        hasClicked: f0.hasProperty('clicked'),
        clicked: encodeVal(f0.getProperty('clicked')),
        propertyIds: f0.getPropertyIds(),
        isClassBuilding: f0.isClass('Building'),
        isExactClassDoor: f0.isExactClass('Door'),
        isExactClassBuilding: f0.isExactClass('Building'),
        exactClassName: f0.getExactClassName(),
        inherited: {
          name: encodeVal(f0.getPropertyInherited('name')),
          from_content: encodeVal(f0.getPropertyInherited('from_content')),
          from_tile: encodeVal(f0.getPropertyInherited('from_tile')),
          from_subtree: encodeVal(f0.getPropertyInherited('from_subtree')),
          from_group: encodeVal(f0.getPropertyInherited('from_group')),
          from_tileset: encodeVal(f0.getPropertyInherited('from_tileset')),
          shared: encodeVal(f0.getPropertyInherited('shared')),
          SHARED_SEM: encodeVal(f0.getPropertyInherited('SHARED_SEM')),
          missing: encodeVal(f0.getPropertyInherited('does_not_exist')),
        },
        inheritSchemaJson,
      };

      // ---- 5. Cesium3DTilePointFeature ----
      const bbColl = new ref.BillboardCollection();
      const lblColl = new ref.LabelCollection();
      const plColl = new ref.PolylineCollection();
      const basePos = ref.Ellipsoid.WGS84.cartographicToCartesian(
        ref.Cartographic.fromDegrees(-105.0, 40.0, 100.0),
      );
      const bb = bbColl.add({ position: ref.Cartesian3.clone(basePos) });
      const lbl = lblColl.add({ position: ref.Cartesian3.clone(basePos) });
      const pl = plColl.add({
        positions: [ref.Cartesian3.clone(basePos), ref.Cartesian3.clone(basePos)],
        material: ref.Material.fromType('Color', { color: ref.Color.clone(ref.Color.WHITE) }),
      });

      const pfContent = {
        batchTable: bt,
        tileset: { ellipsoid: ref.Ellipsoid.WGS84, metadata: tilesetMeta },
        tile: { metadata: tileMeta, implicitSubtree: undefined },
        metadata: contentMeta,
        group: undefined,
        featurePropertiesDirty: false,
      };
      const pf = new ref.Cesium3DTilePointFeature(pfContent, 0, bb, lbl, pl);

      const pfDefaults = {
        staticDefaults: {
          defaultColor: colorArr(ref.Cesium3DTilePointFeature.defaultColor),
          defaultPointOutlineColor: colorArr(ref.Cesium3DTilePointFeature.defaultPointOutlineColor),
          defaultPointOutlineWidth: ref.Cesium3DTilePointFeature.defaultPointOutlineWidth,
          defaultPointSize: ref.Cesium3DTilePointFeature.defaultPointSize,
        },
        show: pf.show,
        color: pf.color ? colorArr(pf.color) : null,
        pointSize: pf.pointSize ?? null,
        pointOutlineColor: pf.pointOutlineColor ? colorArr(pf.pointOutlineColor) : null,
        pointOutlineWidth: pf.pointOutlineWidth ?? null,
        image: pf.image ?? null,
        heightOffset: pf.heightOffset ?? null,
        labelText: pf.labelText,
        labelColor: colorArr(pf.labelColor),
        labelOutlineColor: colorArr(pf.labelOutlineColor),
        labelOutlineWidth: num(pf.labelOutlineWidth),
        labelStyle: pf.labelStyle,
        font: pf.font,
        backgroundColor: colorArr(pf.backgroundColor),
        backgroundPadding: [num(pf.backgroundPadding.x), num(pf.backgroundPadding.y)],
        backgroundEnabled: pf.backgroundEnabled,
        anchorLineEnabled: pf.anchorLineEnabled,
        anchorLineColor: colorArr(pf.anchorLineColor),
        horizontalOrigin: pf.horizontalOrigin,
        verticalOrigin: pf.verticalOrigin,
        labelHorizontalOrigin: pf.labelHorizontalOrigin,
        labelVerticalOrigin: pf.labelVerticalOrigin,
        scaleByDistance: pf.scaleByDistance ?? null,
        translucencyByDistance: pf.translucencyByDistance ?? null,
        distanceDisplayCondition: pf.distanceDisplayCondition ?? null,
        disableDepthTestDistance: pf.disableDepthTestDistance ?? null,
      };

      // Mutations and side effects
      pf.show = false;
      const afterShowFalse = {
        show: pf.show,
        labelShow: lbl.show,
        billboardShow: bb.show,
        polylineShow: pl.show,
      };
      pf.labelColor = new ref.Color(1, 0.5, 0, 1);
      const labelColorWhileHidden = {
        labelColor: colorArr(pf.labelColor),
        polylineShow: pl.show,
      };
      pf.show = true;
      pf.labelColor = new ref.Color(0.2, 0.8, 0.4, 0.0);
      const labelColorZeroAlpha = {
        labelColor: colorArr(pf.labelColor),
        polylineShow: pl.show,
      };
      pf.labelColor = new ref.Color(0.2, 0.8, 0.4, 0.9);
      const labelColorPositiveAlpha = {
        labelColor: colorArr(pf.labelColor),
        polylineShow: pl.show,
      };

      pf.color = new ref.Color(0.9, 0.1, 0.2, 1.0);
      pf.pointSize = 14.0;
      pf.pointOutlineColor = new ref.Color(0.1, 0.2, 0.3, 1.0);
      pf.pointOutlineWidth = 2.5;
      pf.labelText = 'Peak A';
      pf.labelOutlineColor = new ref.Color(0, 0, 1, 1);
      pf.labelOutlineWidth = 3.0;
      pf.labelStyle = ref.LabelStyle.FILL_AND_OUTLINE;
      pf.font = 'bold 20px Helvetica';
      pf.backgroundColor = new ref.Color(0.1, 0.1, 0.1, 0.75);
      pf.backgroundPadding = new ref.Cartesian2(10, 6);
      pf.backgroundEnabled = true;
      pf.anchorLineEnabled = false;
      pf.anchorLineColor = new ref.Color(1, 1, 0, 0.8);
      pf.horizontalOrigin = ref.HorizontalOrigin.LEFT;
      pf.verticalOrigin = ref.VerticalOrigin.BOTTOM;
      pf.labelHorizontalOrigin = ref.HorizontalOrigin.RIGHT;
      pf.labelVerticalOrigin = ref.VerticalOrigin.TOP;
      pf.scaleByDistance = new ref.NearFarScalar(100, 2.0, 5000, 0.5);
      pf.translucencyByDistance = new ref.NearFarScalar(200, 1.0, 8000, 0.1);
      pf.distanceDisplayCondition = new ref.DistanceDisplayCondition(10.0, 2500.0);
      pf.disableDepthTestDistance = 1234.5;

      // Height offset transitions: 25.0 -> 60.0 -> -10.0
      const heightSteps = [];
      for (const h of [25.0, 60.0, -10.0]) {
        pf.heightOffset = h;
        heightSteps.push({
          heightOffset: pf.heightOffset,
          billboardPos: [num(bb.position.x), num(bb.position.y), num(bb.position.z)],
          labelPos: [num(lbl.position.x), num(lbl.position.y), num(lbl.position.z)],
          anchorStart: [num(pl.positions[0].x), num(pl.positions[0].y), num(pl.positions[0].z)],
          anchorEnd: [num(pl.positions[1].x), num(pl.positions[1].y), num(pl.positions[1].z)],
        });
      }

      // labelText undefined -> ""
      pf.labelText = undefined;
      const labelTextAfterUndefined = pf.labelText;

      // Batch table access via point feature
      pf.setProperty('pf_tag', 'summit');
      const pfBatch = {
        hasName: pf.hasProperty('name'),
        name: encodeVal(pf.getProperty('name')),
        pfTag: encodeVal(pf.getProperty('pf_tag')),
        exactClass: pf.getExactClassName(),
        isClassBuilding: pf.isClass('Building'),
        isExactDoor: pf.isExactClass('Door'),
        inheritedContent: encodeVal(pf.getPropertyInherited('from_content')),
      };

      const pfAfter = {
        afterShowFalse,
        labelColorWhileHidden,
        labelColorZeroAlpha,
        labelColorPositiveAlpha,
        color: colorArr(pf.color),
        pointSize: pf.pointSize,
        pointOutlineColor: colorArr(pf.pointOutlineColor),
        pointOutlineWidth: pf.pointOutlineWidth,
        labelOutlineColor: colorArr(pf.labelOutlineColor),
        labelOutlineWidth: pf.labelOutlineWidth,
        labelStyle: pf.labelStyle,
        font: pf.font,
        backgroundColor: colorArr(pf.backgroundColor),
        backgroundPadding: [num(pf.backgroundPadding.x), num(pf.backgroundPadding.y)],
        backgroundEnabled: pf.backgroundEnabled,
        anchorLineEnabled: pf.anchorLineEnabled,
        anchorLineColor: colorArr(pf.anchorLineColor),
        horizontalOrigin: pf.horizontalOrigin,
        verticalOrigin: pf.verticalOrigin,
        labelHorizontalOrigin: pf.labelHorizontalOrigin,
        labelVerticalOrigin: pf.labelVerticalOrigin,
        scaleByDistance: [
          pf.scaleByDistance.near, pf.scaleByDistance.nearValue,
          pf.scaleByDistance.far, pf.scaleByDistance.farValue,
        ],
        billboardScaleByDistance: [
          bb.scaleByDistance.near, bb.scaleByDistance.nearValue,
          bb.scaleByDistance.far, bb.scaleByDistance.farValue,
        ],
        translucencyByDistance: [
          pf.translucencyByDistance.near, pf.translucencyByDistance.nearValue,
          pf.translucencyByDistance.far, pf.translucencyByDistance.farValue,
        ],
        billboardTranslucencyByDistance: [
          bb.translucencyByDistance.near, bb.translucencyByDistance.nearValue,
          bb.translucencyByDistance.far, bb.translucencyByDistance.farValue,
        ],
        distanceDisplayCondition: [pf.distanceDisplayCondition.near, pf.distanceDisplayCondition.far],
        billboardDdc: [bb.distanceDisplayCondition.near, bb.distanceDisplayCondition.far],
        polylineDdc: [pl.distanceDisplayCondition.near, pl.distanceDisplayCondition.far],
        disableDepthTestDistance: pf.disableDepthTestDistance,
        billboardDisableDepthTestDistance: bb.disableDepthTestDistance,
        heightSteps,
        labelTextAfterUndefined,
        pfBatch,
      };

      // Destroy batch table and verify destroyed state
      bt.destroy();
      const btDestroyed = bt.isDestroyed();
      bbColl.destroy();
      lblColl.destroy();
      plColl.destroy();
      context.destroy();

      return {
        maxTextureSize,
        batchTable: {
          batchTableJson: batchTableJsonSnapshot,
          binaryBase64: binarySnapshotBase64,
          beforeUpdate: btBeforeUpdate,
          afterMutate: btAfterMutate,
          initialShow,
          initialColor,
          afterManualShowColor,
          afterGpuUpdate,
          afterStyle,
          afterStyleReset,
          legacyHierarchy,
          destroyed: btDestroyed,
        },
        feature: featureCapture,
        pointFeature: {
          basePos: [num(basePos.x), num(basePos.y), num(basePos.z)],
          defaults: pfDefaults,
          after: pfAfter,
        },
      };
     } catch (e) {
      return { __error: String(e), message: e?.message, stack: e?.stack };
     }
    });
    if (evalRes?.__error) {
      throw new Error(`${evalRes.__error}: ${evalRes.message}\n${evalRes.stack}`);
    }
    return evalRes;
  } finally {
    await browser?.close();
    server.close();
  }
}

const cesium3DTile = captureCesium3DTile();
const cesium3DTileContent = captureCesium3DTileContent();
const implicit3DTileContent = await captureImplicit3DTileContent();
const multiple3DTileContent = captureMultiple3DTileContent();
const composite3DTileContent = await captureComposite3DTileContent();
const geometry3DTileContent = await captureGeometry3DTileContent();
const webgl = await captureWebGLModules();

const fixture = {
  cesiumVersion: '23.0.0',
  cesium3DTile,
  cesium3DTileBatchTable: webgl.batchTable,
  maxTextureSize: webgl.maxTextureSize,
  cesium3DTileContent,
  cesium3DTileFeature: webgl.feature,
  cesium3DTilePointFeature: webgl.pointFeature,
  implicit3DTileContent,
  multiple3DTileContent,
  composite3DTileContent,
  geometry3DTileContent,
};

const outUrl = new URL('../../tests/data/scene-3d-tiles-core.json', import.meta.url);
writeFileSync(outUrl, JSON.stringify(fixture, null, 2) + '\n');
console.log('Wrote', fileURLToPath(outUrl), 'bytes:', JSON.stringify(fixture).length);
