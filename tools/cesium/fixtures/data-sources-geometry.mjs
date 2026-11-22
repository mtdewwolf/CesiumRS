import * as C from "@cesium/engine";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { enc } from "./lib.mjs";
import { installDataSourcesDom } from "./data-sources-dom.mjs";

const time0 = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");
const time1 = C.JulianDate.addSeconds(time0, 10, new C.JulianDate());

function makeScene(opts = {}) {
  const depthTexture = opts.depthTexture ?? true;
  const maxLineWidth = opts.maxLineWidth ?? 5;
  const mode = opts.mode ?? C.SceneMode.SCENE3D;
  const context = {
    depthTexture,
    stencilBuffer: depthTexture,
    fragmentDepth: depthTexture,
    floatingPointTexture: true,
    drawBuffers: true,
    webgl2: depthTexture,
    aliasedLineWidthRange: [1, maxLineWidth],
  };
  return {
    id: opts.id ?? "scene",
    mode,
    ellipsoid: C.Ellipsoid.WGS84,
    context,
    frameState: { context, mode },
    getHeight: () => opts.terrainHeight ?? 0,
    updateHeight: () => () => {},
    terrainProviderChanged: new C.Event(),
    morphComplete: new C.Event(),
    globe: opts.noGlobe
      ? undefined
      : {
          ellipsoid: C.Ellipsoid.WGS84,
          getHeight: () => opts.terrainHeight ?? 0,
          _surface: {
            updateHeight: (_c, _cb) => () => {},
          },
        },
    clampLineWidth: (w) => Math.max(1, Math.min(w, maxLineWidth)),
    primitives: new C.PrimitiveCollection(),
    groundPrimitives: new C.OrderedGroundPrimitiveCollection(),
    requestRender() {},
  };
}

function summarizeUpdater(u, t = time0) {
  return {
    id: u.id,
    fillEnabled: u.fillEnabled,
    hasConstantFill: u.hasConstantFill,
    outlineEnabled: u.outlineEnabled,
    hasConstantOutline: u.hasConstantOutline,
    outlineWidth: u.outlineWidth ?? 1,
    isDynamic: u.isDynamic,
    isClosed: u.isClosed,
    onTerrain: u.onTerrain ?? false,
    clampToGround: u.clampToGround ?? false,
    isFilled: u.isFilled(t),
    isOutlineVisible: u.isOutlineVisible(t),
    hasTerrainOffset: u.terrainOffsetProperty !== undefined,
    zIndex: u.zIndex ? u.zIndex.getValue(t) : null,
  };
}

function summarizeInstance(inst) {
  const geom = inst.geometry
    ? inst.geometry.constructor.createGeometry(inst.geometry)
    : undefined;
  const posAttr = geom?.attributes?.position;
  const vertexCount = posAttr ? posAttr.values.length / 3 : 0;
  const indexCount = geom?.indices ? geom.indices.length : 0;
  const bs = geom?.boundingSphere;
  const attrs = inst.attributes ?? {};
  return {
    id: inst.id?.id ?? null,
    modelMatrix: enc(inst.modelMatrix),
    show: attrs.show ? attrs.show.value[0] : null,
    color: attrs.color ? Array.from(attrs.color.value) : null,
    depthFailColor: attrs.depthFailColor ? Array.from(attrs.depthFailColor.value) : null,
    ddc: attrs.distanceDisplayCondition ? Array.from(attrs.distanceDisplayCondition.value) : null,
    offset: attrs.offset ? Array.from(attrs.offset.value) : null,
    hasGeometry: geom !== undefined,
    vertexCount,
    indexCount,
    boundingSphere: bs ? enc(bs) : null,
  };
}

function single(module, fn) {
  return { module, cases: [{ f: "lifecycle", a: [], r: enc(fn()) }] };
}

function suiteGeometryUpdater() {
  const scene = makeScene();
  const pos = C.Cartesian3.fromDegrees(-105, 40, 100);
  const e = new C.Entity({
    id: "gu-1",
    position: pos,
    box: {
      dimensions: new C.Cartesian3(10, 20, 30),
      fill: true,
      outline: true,
      outlineWidth: 2,
      outlineColor: C.Color.YELLOW,
      material: C.Color.RED,
    },
  });
  const u = new C.BoxGeometryUpdater(e, scene);
  const s0 = summarizeUpdater(u, time0);
  const fill0 = summarizeInstance(u.createFillGeometryInstance(time0));
  const out0 = summarizeInstance(u.createOutlineGeometryInstance(time0));

  // Hide via constant show=false
  e.box.show = false;
  u._onEntityPropertyChanged(e, "box", e.box, undefined);
  const sHidden = summarizeUpdater(u, time0);

  // Restore with non-constant show -> hasConstantFill becomes false
  e.box.show = new C.CallbackProperty((t) => C.JulianDate.equals(t, time0), false);
  u._onEntityPropertyChanged(e, "box", e.box, undefined);
  const sDynamicShow0 = summarizeUpdater(u, time0);
  const sDynamicShow1 = summarizeUpdater(u, time1);

  // Disable both fill & outline
  e.box.show = true;
  e.box.fill = false;
  e.box.outline = false;
  u._onEntityPropertyChanged(e, "box", e.box, undefined);
  const sDisabled = summarizeUpdater(u, time0);

  // Remove geometry component
  e.box = undefined;
  u._onEntityPropertyChanged(e, "box", undefined, undefined);
  const sRemoved = summarizeUpdater(u, time0);

  u.destroy();
  return { s0, fill0, out0, sHidden, sDynamicShow0, sDynamicShow1, sDisabled, sRemoved, destroyed: u.isDestroyed() };
}

function suiteGroundGeometryUpdater() {
  const GGU = C.GroundGeometryUpdater;
  const HR = C.HeightReference;
  const cases = [];
  for (const h of [undefined, 0, 50]) {
    for (const hr of [HR.NONE, HR.CLAMP_TO_GROUND, HR.RELATIVE_TO_GROUND]) {
      cases.push([
        "height",
        h ?? null,
        hr,
        GGU.getGeometryHeight(h, hr) ?? null,
        GGU.getGeometryExtrudedHeight(h, hr) ?? null,
      ]);
    }
  }
  const offsets = [];
  for (const h of [undefined, 10]) {
    for (const hr of [HR.NONE, HR.CLAMP_TO_GROUND, HR.RELATIVE_TO_GROUND]) {
      for (const eh of [undefined, 30]) {
        for (const ehr of [HR.NONE, HR.CLAMP_TO_GROUND, HR.RELATIVE_TO_GROUND]) {
          offsets.push([
            h ?? null,
            hr,
            eh ?? null,
            ehr,
            GGU.computeGeometryOffsetAttribute(h, hr, eh, ehr) ?? null,
          ]);
        }
      }
    }
  }
  return {
    clampSentinel: GGU.CLAMP_TO_GROUND,
    heightCases: cases,
    offsetCases: offsets,
  };
}

function suiteDynamicGeometryUpdater() {
  const scene = makeScene();
  const primitives = new C.PrimitiveCollection();
  const groundPrimitives = new C.OrderedGroundPrimitiveCollection();
  const e = new C.Entity({
    id: "dyn-1",
    position: C.Cartesian3.fromDegrees(-105, 40, 100),
    box: {
      dimensions: new C.CallbackProperty(
        (t) =>
          C.JulianDate.equals(t, time0)
            ? new C.Cartesian3(10, 20, 30)
            : new C.Cartesian3(40, 50, 60),
        false,
      ),
      material: C.Color.CYAN,
      outline: true,
      outlineColor: C.Color.BLACK,
      outlineWidth: 3,
    },
  });
  const updater = new C.BoxGeometryUpdater(e, scene);
  const dyn = updater.createDynamicUpdater(primitives, groundPrimitives);
  dyn.update(time0);
  const after0 = [primitives.length, groundPrimitives.length];
  dyn.update(time1);
  const after1 = [primitives.length, groundPrimitives.length];
  e.show = false;
  dyn.update(time1);
  const afterHidden = [primitives.length, groundPrimitives.length];
  dyn.destroy();
  const afterDestroy = [primitives.length, groundPrimitives.length, dyn.isDestroyed()];
  updater.destroy();
  return { isDynamic: updater.isDynamic, after0, after1, afterHidden, afterDestroy };
}

function suiteDynamicGeometryBatch() {
  const scene = makeScene();
  const primitives = new C.PrimitiveCollection();
  const groundPrimitives = new C.OrderedGroundPrimitiveCollection();
  const batch = new C.DynamicGeometryBatch(primitives, groundPrimitives);

  const e1 = new C.Entity({
    id: "db-1",
    position: C.Cartesian3.fromDegrees(-105, 40, 100),
    box: {
      dimensions: new C.CallbackProperty(() => new C.Cartesian3(10, 10, 10), false),
      material: C.Color.RED,
    },
  });
  const e2 = new C.Entity({
    id: "db-2",
    rectangle: {
      coordinates: new C.CallbackProperty(() => C.Rectangle.fromDegrees(-110, 30, -100, 40), false),
      material: C.Color.BLUE,
    },
  });
  const u1 = new C.BoxGeometryUpdater(e1, scene);
  const u2 = new C.RectangleGeometryUpdater(e2, scene);

  batch.add(time0, u1);
  batch.add(time0, u2);
  const updated = batch.update(time0);
  const countsAfterAdd = [primitives.length, groundPrimitives.length];

  batch.remove(u1);
  const countsAfterRemove1 = [primitives.length, groundPrimitives.length];

  batch.removeAllPrimitives();
  const countsAfterClear = [primitives.length, groundPrimitives.length];

  u1.destroy();
  u2.destroy();
  return { updated, countsAfterAdd, countsAfterRemove1, countsAfterClear };
}

function suiteGeometryUpdaterSet() {
  const scene = makeScene();
  const e = new C.Entity({
    id: "set-1",
    position: C.Cartesian3.fromDegrees(-105, 40, 100),
    box: { dimensions: new C.Cartesian3(10, 20, 30) },
    rectangle: { coordinates: C.Rectangle.fromDegrees(-106, 39, -104, 41) },
  });
  const set = new C.GeometryUpdaterSet(e, scene);
  const changedIds = [];
  set.geometryChanged.addEventListener((u) => changedIds.push(u.id));

  const initialCount = set.updaters.length;
  const activeBefore = set.updaters.filter((u) => u.fillEnabled || u.outlineEnabled).map((u) => u.id);

  e.wall = new C.WallGraphics({
    positions: C.Cartesian3.fromDegreesArrayHeights([-105, 40, 100, -104, 40, 100]),
  });
  const activeAfterWall = set.updaters.filter((u) => u.fillEnabled || u.outlineEnabled).map((u) => u.id);

  e.box = undefined;
  const activeAfterRemoveBox = set.updaters.filter((u) => u.fillEnabled || u.outlineEnabled).map((u) => u.id);

  set.destroy();
  return { initialCount, activeBefore, activeAfterWall, activeAfterRemoveBox, changedIds, destroyed: set.isDestroyed() };
}

function suiteStaticGeometryColorBatch() {
  const scene = makeScene();
  const primitives = new C.PrimitiveCollection();
  const batch = new C.StaticGeometryColorBatch(
    primitives,
    C.PerInstanceColorAppearance,
    undefined,
    true,
    C.ShadowMode.DISABLED,
  );
  const eSolid1 = new C.Entity({
    id: "cb-s1",
    position: C.Cartesian3.fromDegrees(-105, 40, 100),
    box: { dimensions: new C.Cartesian3(10, 10, 10), material: C.Color.RED },
  });
  const eSolid2 = new C.Entity({
    id: "cb-s2",
    position: C.Cartesian3.fromDegrees(-104, 40, 100),
    box: { dimensions: new C.Cartesian3(10, 10, 10), material: C.Color.GREEN },
  });
  const eTrans = new C.Entity({
    id: "cb-t1",
    position: C.Cartesian3.fromDegrees(-103, 40, 100),
    box: { dimensions: new C.Cartesian3(10, 10, 10), material: C.Color.BLUE.withAlpha(0.5) },
  });
  const u1 = new C.BoxGeometryUpdater(eSolid1, scene);
  const u2 = new C.BoxGeometryUpdater(eSolid2, scene);
  const u3 = new C.BoxGeometryUpdater(eTrans, scene);

  batch.add(time0, u1);
  batch.add(time0, u2);
  batch.add(time0, u3);
  const firstUpdate = batch.update(time0);
  const primsAfterAdd = primitives.length;
  const solidBuckets = batch._solidItems.length;
  const transBuckets = batch._translucentItems.length;

  batch.remove(u3);
  const afterRemoveTrans = batch.update(time0);
  const primsAfterRemoveTrans = primitives.length;

  batch.removeAllPrimitives();
  const primsAfterClear = primitives.length;

  u1.destroy();
  u2.destroy();
  u3.destroy();
  return {
    firstUpdate,
    primsAfterAdd,
    solidBuckets,
    transBuckets,
    afterRemoveTrans,
    primsAfterRemoveTrans,
    primsAfterClear,
  };
}

function suiteStaticGeometryPerMaterialBatch() {
  const scene = makeScene();
  const primitives = new C.PrimitiveCollection();
  const batch = new C.StaticGeometryPerMaterialBatch(
    primitives,
    C.MaterialAppearance,
    undefined,
    true,
    C.ShadowMode.DISABLED,
  );
  const grid1 = new C.GridMaterialProperty({ color: C.Color.RED });
  const grid2 = new C.GridMaterialProperty({ color: C.Color.RED });
  const stripe = new C.StripeMaterialProperty({ evenColor: C.Color.BLUE, oddColor: C.Color.WHITE });

  const e1 = new C.Entity({
    id: "mb-1",
    position: C.Cartesian3.fromDegrees(-105, 40, 100),
    box: { dimensions: new C.Cartesian3(10, 10, 10), material: grid1 },
  });
  const e2 = new C.Entity({
    id: "mb-2",
    position: C.Cartesian3.fromDegrees(-104, 40, 100),
    box: { dimensions: new C.Cartesian3(10, 10, 10), material: grid2 },
  });
  const e3 = new C.Entity({
    id: "mb-3",
    position: C.Cartesian3.fromDegrees(-103, 40, 100),
    box: { dimensions: new C.Cartesian3(10, 10, 10), material: stripe },
  });
  const u1 = new C.BoxGeometryUpdater(e1, scene);
  const u2 = new C.BoxGeometryUpdater(e2, scene);
  const u3 = new C.BoxGeometryUpdater(e3, scene);

  batch.add(time0, u1);
  batch.add(time0, u2);
  batch.add(time0, u3);
  const firstUpdate = batch.update(time0);
  const itemCount = batch._items.length;
  const primsAfterAdd = primitives.length;

  batch.remove(u3);
  batch.update(time0);
  const itemCountAfterRemove = batch._items.length;
  const primsAfterRemove = primitives.length;

  batch.removeAllPrimitives();
  const primsAfterClear = primitives.length;

  u1.destroy();
  u2.destroy();
  u3.destroy();
  return { firstUpdate, itemCount, primsAfterAdd, itemCountAfterRemove, primsAfterRemove, primsAfterClear };
}

function suiteStaticGroundGeometryColorBatch() {
  const scene = makeScene();
  const groundPrimitives = new C.OrderedGroundPrimitiveCollection();
  const batch = new C.StaticGroundGeometryColorBatch(groundPrimitives, C.ClassificationType.BOTH);

  // Two non-overlapping rectangles at zIndex 0 -> 1 batch; one overlapping -> 2nd batch; one at zIndex 5 -> 3rd batch.
  const e1 = new C.Entity({
    id: "gcb-1",
    rectangle: { coordinates: C.Rectangle.fromDegrees(-110, 30, -105, 35), material: C.Color.RED, zIndex: 0 },
  });
  const e2 = new C.Entity({
    id: "gcb-2",
    rectangle: { coordinates: C.Rectangle.fromDegrees(-95, 30, -90, 35), material: C.Color.GREEN, zIndex: 0 },
  });
  const e3 = new C.Entity({
    id: "gcb-3",
    rectangle: { coordinates: C.Rectangle.fromDegrees(-108, 32, -102, 37), material: C.Color.BLUE, zIndex: 0 },
  });
  const e4 = new C.Entity({
    id: "gcb-4",
    rectangle: { coordinates: C.Rectangle.fromDegrees(-80, 30, -75, 35), material: C.Color.YELLOW, zIndex: 5 },
  });
  const updaters = [e1, e2, e3, e4].map((e) => new C.RectangleGeometryUpdater(e, scene));
  for (const u of updaters) batch.add(time0, u);
  const firstUpdate = batch.update(time0);
  const batchCount = batch._batches.length;
  const groundCount = groundPrimitives.length;

  batch.remove(updaters[3]);
  batch.update(time0);
  const batchCountAfterRemove = batch._batches.length;
  const groundCountAfterRemove = groundPrimitives.length;

  batch.removeAllPrimitives();
  const groundCountAfterClear = groundPrimitives.length;
  for (const u of updaters) u.destroy();
  return {
    firstUpdate,
    batchCount,
    groundCount,
    batchCountAfterRemove,
    groundCountAfterRemove,
    groundCountAfterClear,
  };
}

function suiteStaticGroundGeometryPerMaterialBatch() {
  const scene = makeScene();
  const groundPrimitives = new C.OrderedGroundPrimitiveCollection();
  const batch = new C.StaticGroundGeometryPerMaterialBatch(
    groundPrimitives,
    C.ClassificationType.BOTH,
    C.MaterialAppearance,
  );
  const gridA = new C.GridMaterialProperty({ color: C.Color.RED });
  const gridB = new C.GridMaterialProperty({ color: C.Color.RED });
  const stripe = new C.StripeMaterialProperty({ evenColor: C.Color.GREEN, oddColor: C.Color.BLACK });

  // Non-overlapping small rectangles with equal material -> 1 batch; overlapping with same material -> 2nd batch; different material -> 3rd batch
  const e1 = new C.Entity({
    id: "gmb-1",
    rectangle: { coordinates: C.Rectangle.fromDegrees(-110, 30, -108, 32), material: gridA, zIndex: 0 },
  });
  const e2 = new C.Entity({
    id: "gmb-2",
    rectangle: { coordinates: C.Rectangle.fromDegrees(-95, 30, -93, 32), material: gridB, zIndex: 0 },
  });
  const e3 = new C.Entity({
    id: "gmb-3",
    rectangle: { coordinates: C.Rectangle.fromDegrees(-109, 31, -107, 33), material: gridB, zIndex: 0 },
  });
  const e4 = new C.Entity({
    id: "gmb-4",
    rectangle: { coordinates: C.Rectangle.fromDegrees(-85, 30, -83, 32), material: stripe, zIndex: 0 },
  });
  const updaters = [e1, e2, e3, e4].map((e) => new C.RectangleGeometryUpdater(e, scene));
  for (const u of updaters) batch.add(time0, u);
  const firstUpdate = batch.update(time0);
  const batchCount = batch._items.length;
  const groundCount = groundPrimitives.length;

  batch.remove(updaters[3]);
  batch.update(time0);
  const batchCountAfterRemove = batch._items.length;

  batch.removeAllPrimitives();
  const groundCountAfterClear = groundPrimitives.length;
  for (const u of updaters) u.destroy();
  return { firstUpdate, batchCount, groundCount, batchCountAfterRemove, groundCountAfterClear };
}

function suiteStaticGroundPolylinePerMaterialBatch() {
  const scene = makeScene();
  const groundPrimitives = new C.OrderedGroundPrimitiveCollection();
  const batch = new C.StaticGroundPolylinePerMaterialBatch(groundPrimitives, C.ClassificationType.BOTH, true);

  const p1 = C.Cartesian3.fromDegreesArray([-110, 35, -109, 36]);
  const p2 = C.Cartesian3.fromDegreesArray([-105, 35, -104, 36]);
  const p3 = C.Cartesian3.fromDegreesArray([-100, 35, -99, 36]);
  const e1 = new C.Entity({
    id: "gpb-1",
    polyline: { positions: p1, clampToGround: true, material: C.Color.RED, zIndex: 0 },
  });
  const e2 = new C.Entity({
    id: "gpb-2",
    polyline: { positions: p2, clampToGround: true, material: C.Color.BLUE, zIndex: 0 },
  });
  const e3 = new C.Entity({
    id: "gpb-3",
    polyline: { positions: p3, clampToGround: true, material: C.Color.GREEN, zIndex: 2 },
  });
  const e4 = new C.Entity({
    id: "gpb-4",
    polyline: {
      positions: p3,
      clampToGround: true,
      material: new C.PolylineDashMaterialProperty({ color: C.Color.YELLOW }),
      zIndex: 0,
    },
  });
  const updaters = [e1, e2, e3, e4].map((e) => new C.PolylineGeometryUpdater(e, scene));
  for (const u of updaters) batch.add(time0, u);
  const firstUpdate = batch.update(time0);
  // e1 & e2 share one ColorMaterial batch at zIndex 0; e3 has zIndex 2; e4 has dash material at zIndex 0 -> 3 batches
  const batchCount = batch._items.length;
  const groundCount = groundPrimitives.length;

  batch.remove(updaters[3]);
  batch.update(time0);
  const batchCountAfterRemove = batch._items.length;

  batch.removeAllPrimitives();
  const groundCountAfterClear = groundPrimitives.length;
  for (const u of updaters) u.destroy();
  return { firstUpdate, batchCount, groundCount, batchCountAfterRemove, groundCountAfterClear };
}

function suiteStaticOutlineGeometryBatch() {
  const scene = makeScene({ maxLineWidth: 4 });
  const primitives = new C.PrimitiveCollection();
  const batch = new C.StaticOutlineGeometryBatch(primitives, scene, C.ShadowMode.DISABLED, false);

  const e1 = new C.Entity({
    id: "ob-1",
    position: C.Cartesian3.fromDegrees(-105, 40, 100),
    box: { dimensions: new C.Cartesian3(10, 10, 10), outline: true, outlineWidth: 2, outlineColor: C.Color.WHITE },
  });
  const e2 = new C.Entity({
    id: "ob-2",
    position: C.Cartesian3.fromDegrees(-104, 40, 100),
    box: { dimensions: new C.Cartesian3(10, 10, 10), outline: true, outlineWidth: 8, outlineColor: C.Color.YELLOW },
  });
  const e3 = new C.Entity({
    id: "ob-3",
    position: C.Cartesian3.fromDegrees(-103, 40, 100),
    box: {
      dimensions: new C.Cartesian3(10, 10, 10),
      outline: true,
      outlineWidth: 2,
      outlineColor: C.Color.CYAN.withAlpha(0.4),
    },
  });
  const u1 = new C.BoxGeometryUpdater(e1, scene);
  const u2 = new C.BoxGeometryUpdater(e2, scene);
  const u3 = new C.BoxGeometryUpdater(e3, scene);

  batch.add(time0, u1);
  batch.add(time0, u2);
  batch.add(time0, u3);
  const firstUpdate = batch.update(time0);
  const solidWidths = batch._solidBatches.values.length;
  const transWidths = batch._translucentBatches.values.length;
  const primCount = primitives.length;

  batch.remove(u2);
  batch.update(time0);
  const primCountAfterRemove = primitives.length;

  batch.removeAllPrimitives();
  const primCountAfterClear = primitives.length;
  u1.destroy();
  u2.destroy();
  u3.destroy();
  return { firstUpdate, solidWidths, transWidths, primCount, primCountAfterRemove, primCountAfterClear };
}

function suiteBoxGeometryUpdater() {
  const scene = makeScene();
  const pos = C.Cartesian3.fromDegrees(-105, 40, 500);
  const eStatic = new C.Entity({
    id: "box-static",
    position: pos,
    box: {
      dimensions: new C.Cartesian3(100, 200, 300),
      material: C.Color.RED,
      outline: true,
      outlineColor: C.Color.BLACK,
      outlineWidth: 2,
    },
  });
  const uStatic = new C.BoxGeometryUpdater(eStatic, scene);
  const staticSummary = summarizeUpdater(uStatic);
  const staticFill = summarizeInstance(uStatic.createFillGeometryInstance(time0));
  const staticOutline = summarizeInstance(uStatic.createOutlineGeometryInstance(time0));

  const eClamped = new C.Entity({
    id: "box-clamp",
    position: pos,
    box: {
      dimensions: new C.Cartesian3(100, 200, 300),
      heightReference: C.HeightReference.CLAMP_TO_GROUND,
      material: C.Color.GREEN,
    },
  });
  const uClamped = new C.BoxGeometryUpdater(eClamped, scene);
  const clampedSummary = summarizeUpdater(uClamped);
  const clampedFill = summarizeInstance(uClamped.createFillGeometryInstance(time0));

  uStatic.destroy();
  uClamped.destroy();
  return { staticSummary, staticFill, staticOutline, clampedSummary, clampedFill };
}

function suiteCorridorGeometryUpdater() {
  const scene = makeScene();
  const positions = C.Cartesian3.fromDegreesArray([-105, 40, -104, 40, -104, 41]);
  const eGround = new C.Entity({
    id: "cor-ground",
    corridor: { positions, width: 1000, material: C.Color.ORANGE, zIndex: 3 },
  });
  const uGround = new C.CorridorGeometryUpdater(eGround, scene);
  const groundSummary = summarizeUpdater(uGround);
  const groundFill = summarizeInstance(uGround.createFillGeometryInstance(time0));

  const eExtruded = new C.Entity({
    id: "cor-extruded",
    corridor: {
      positions,
      width: 1000,
      height: 100,
      extrudedHeight: 500,
      cornerType: C.CornerType.MITERED,
      outline: true,
      outlineColor: C.Color.WHITE,
      material: C.Color.BLUE,
    },
  });
  const uExtruded = new C.CorridorGeometryUpdater(eExtruded, scene);
  const extrudedSummary = summarizeUpdater(uExtruded);
  const extrudedFill = summarizeInstance(uExtruded.createFillGeometryInstance(time0));
  const extrudedOutline = summarizeInstance(uExtruded.createOutlineGeometryInstance(time0));

  uGround.destroy();
  uExtruded.destroy();
  return { groundSummary, groundFill, extrudedSummary, extrudedFill, extrudedOutline };
}

function suiteCylinderGeometryUpdater() {
  const scene = makeScene();
  const pos = C.Cartesian3.fromDegrees(-105, 40, 200);
  const e = new C.Entity({
    id: "cyl-1",
    position: pos,
    cylinder: {
      length: 400,
      topRadius: 50,
      bottomRadius: 100,
      slices: 16,
      material: C.Color.MAGENTA,
      outline: true,
      outlineColor: C.Color.BLACK,
      numberOfVerticalLines: 8,
    },
  });
  const u = new C.CylinderGeometryUpdater(e, scene);
  const summary = summarizeUpdater(u);
  const fill = summarizeInstance(u.createFillGeometryInstance(time0));
  const outline = summarizeInstance(u.createOutlineGeometryInstance(time0));
  u.destroy();
  return { summary, fill, outline };
}

function suiteEllipseGeometryUpdater() {
  const scene = makeScene();
  const pos = C.Cartesian3.fromDegrees(-105, 40, 0);
  const eGround = new C.Entity({
    id: "ell-ground",
    position: pos,
    ellipse: { semiMajorAxis: 5000, semiMinorAxis: 3000, rotation: 0.5, material: C.Color.LIME, zIndex: 2 },
  });
  const uGround = new C.EllipseGeometryUpdater(eGround, scene);
  const groundSummary = summarizeUpdater(uGround);
  const groundFill = summarizeInstance(uGround.createFillGeometryInstance(time0));

  const eHeight = new C.Entity({
    id: "ell-height",
    position: pos,
    ellipse: {
      semiMajorAxis: 5000,
      semiMinorAxis: 3000,
      height: 100,
      extrudedHeight: 600,
      outline: true,
      outlineColor: C.Color.BLACK,
      numberOfVerticalLines: 12,
    },
  });
  const uHeight = new C.EllipseGeometryUpdater(eHeight, scene);
  const heightSummary = summarizeUpdater(uHeight);
  const heightFill = summarizeInstance(uHeight.createFillGeometryInstance(time0));
  const heightOutline = summarizeInstance(uHeight.createOutlineGeometryInstance(time0));

  uGround.destroy();
  uHeight.destroy();
  return { groundSummary, groundFill, heightSummary, heightFill, heightOutline };
}

function suiteEllipsoidGeometryUpdater() {
  const scene = makeScene();
  const pos = C.Cartesian3.fromDegrees(-105, 40, 500);
  const e = new C.Entity({
    id: "elpd-1",
    position: pos,
    ellipsoid: {
      radii: new C.Cartesian3(200, 300, 400),
      innerRadii: new C.Cartesian3(50, 60, 70),
      stackPartitions: 16,
      slicePartitions: 16,
      material: C.Color.CYAN,
      outline: true,
      outlineColor: C.Color.WHITE,
      subdivisions: 32,
    },
  });
  const u = new C.EllipsoidGeometryUpdater(e, scene);
  const summary = summarizeUpdater(u);
  const fill = summarizeInstance(u.createFillGeometryInstance(time0));
  const outline = summarizeInstance(u.createOutlineGeometryInstance(time0));

  // Dynamic ellipsoid in 3D reuses unit sphere & scales modelMatrix
  const eDyn = new C.Entity({
    id: "elpd-dyn",
    position: pos,
    ellipsoid: {
      radii: new C.CallbackProperty(
        (t) =>
          C.JulianDate.equals(t, time0)
            ? new C.Cartesian3(100, 200, 300)
            : new C.Cartesian3(400, 500, 600),
        false,
      ),
      material: C.Color.YELLOW,
    },
  });
  const uDyn = new C.EllipsoidGeometryUpdater(eDyn, scene);
  const prims = new C.PrimitiveCollection();
  const gprims = new C.OrderedGroundPrimitiveCollection();
  const dyn = uDyn.createDynamicUpdater(prims, gprims);
  dyn.update(time0);
  const m0 = enc(prims.get(0).modelMatrix);
  dyn.update(time1);
  const m1 = enc(prims.get(0).modelMatrix);
  const dynPrimCount = prims.length;
  dyn.destroy();
  const dynAfterDestroy = prims.length;

  u.destroy();
  uDyn.destroy();
  return { summary, fill, outline, m0, m1, dynPrimCount, dynAfterDestroy };
}

function suitePlaneGeometryUpdater() {
  const scene = makeScene();
  const pos = C.Cartesian3.fromDegrees(-105, 40, 200);
  const plane = new C.Plane(C.Cartesian3.UNIT_Z, 25);
  const dims = new C.Cartesian2(100, 200);
  const e = new C.Entity({
    id: "plane-1",
    position: pos,
    plane: { plane, dimensions: dims, material: C.Color.AQUA, outline: true, outlineColor: C.Color.BLACK },
  });
  const u = new C.PlaneGeometryUpdater(e, scene);
  const summary = summarizeUpdater(u);
  const fill = summarizeInstance(u.createFillGeometryInstance(time0));
  const outline = summarizeInstance(u.createOutlineGeometryInstance(time0));
  u.destroy();
  return { summary, fill, outline };
}

function suitePolygonGeometryUpdater() {
  const scene = makeScene();
  const ring = C.Cartesian3.fromDegreesArray([-105, 40, -104, 40, -104, 41, -105, 41]);
  const eGround = new C.Entity({
    id: "poly-ground",
    polygon: { hierarchy: ring, material: C.Color.RED, zIndex: 1 },
  });
  const uGround = new C.PolygonGeometryUpdater(eGround, scene);
  const groundSummary = summarizeUpdater(uGround);
  const groundFill = summarizeInstance(uGround.createFillGeometryInstance(time0));

  const eExtruded = new C.Entity({
    id: "poly-ext",
    polygon: {
      hierarchy: ring,
      height: 50,
      extrudedHeight: 300,
      closeTop: true,
      closeBottom: false,
      outline: true,
      outlineColor: C.Color.WHITE,
    },
  });
  const uExtruded = new C.PolygonGeometryUpdater(eExtruded, scene);
  const extrudedSummary = summarizeUpdater(uExtruded);
  const extrudedFill = summarizeInstance(uExtruded.createFillGeometryInstance(time0));
  const extrudedOutline = summarizeInstance(uExtruded.createOutlineGeometryInstance(time0));

  const eCoplanar = new C.Entity({
    id: "poly-coplanar",
    polygon: {
      hierarchy: C.Cartesian3.fromDegreesArrayHeights([-105, 40, 100, -104, 40, 200, -104, 41, 150]),
      perPositionHeight: true,
      outline: true,
      material: C.Color.YELLOW,
    },
  });
  const uCoplanar = new C.PolygonGeometryUpdater(eCoplanar, scene);
  const coplanarSummary = summarizeUpdater(uCoplanar);
  const coplanarFill = summarizeInstance(uCoplanar.createFillGeometryInstance(time0));
  const coplanarOutline = summarizeInstance(uCoplanar.createOutlineGeometryInstance(time0));

  const center = enc(uGround._computeCenter(time0, new C.Cartesian3()));

  uGround.destroy();
  uExtruded.destroy();
  uCoplanar.destroy();
  return {
    groundSummary,
    groundFill,
    extrudedSummary,
    extrudedFill,
    extrudedOutline,
    coplanarSummary,
    coplanarFill,
    coplanarOutline,
    center,
  };
}

function suitePolylineGeometryUpdater() {
  const scene = makeScene();
  const positions = C.Cartesian3.fromDegreesArrayHeights([-105, 40, 100, -104, 40.5, 200]);
  const eStatic = new C.Entity({
    id: "pl-static",
    polyline: {
      positions,
      width: 4,
      material: C.Color.RED,
      depthFailMaterial: C.Color.BLUE,
      arcType: C.ArcType.GEODESIC,
    },
  });
  const uStatic = new C.PolylineGeometryUpdater(eStatic, scene);
  const staticSummary = summarizeUpdater(uStatic);
  const staticFill = summarizeInstance(uStatic.createFillGeometryInstance(time0));

  const eClamp = new C.Entity({
    id: "pl-clamp",
    polyline: {
      positions,
      width: 3,
      clampToGround: true,
      material: C.Color.GREEN,
      zIndex: 4,
    },
  });
  const uClamp = new C.PolylineGeometryUpdater(eClamp, scene);
  const clampSummary = summarizeUpdater(uClamp);
  const clampFill = summarizeInstance(uClamp.createFillGeometryInstance(time0));

  // Single position (< 2) disables fillEnabled
  const eShort = new C.Entity({
    id: "pl-short",
    polyline: { positions: [positions[0]] },
  });
  const uShort = new C.PolylineGeometryUpdater(eShort, scene);
  const shortSummary = summarizeUpdater(uShort);

  // Dynamic non-clamped polyline
  const eDyn = new C.Entity({
    id: "pl-dyn",
    polyline: {
      positions: new C.CallbackProperty(() => positions, false),
      width: 5,
      material: C.Color.YELLOW,
      arcType: C.ArcType.NONE,
    },
  });
  const uDyn = new C.PolylineGeometryUpdater(eDyn, scene);
  const prims = new C.PrimitiveCollection();
  const gprims = new C.OrderedGroundPrimitiveCollection();
  const dyn = uDyn.createDynamicUpdater(prims, gprims);
  dyn.update(time0);
  const dynPrims = [prims.length, gprims.length];
  const bs = new C.BoundingSphere();
  const bsState = dyn.getBoundingSphere(bs);
  dyn.destroy();
  const dynAfterDestroy = [prims.length, gprims.length];

  uStatic.destroy();
  uClamp.destroy();
  uShort.destroy();
  uDyn.destroy();
  return {
    staticSummary,
    staticFill,
    clampSummary,
    clampFill,
    shortSummary,
    dynPrims,
    bsState,
    bs: enc(bs),
    dynAfterDestroy,
  };
}

function suitePolylineVolumeGeometryUpdater() {
  const scene = makeScene();
  const positions = C.Cartesian3.fromDegreesArray([-105, 40, -104, 40, -104, 41]);
  const shape = [
    new C.Cartesian2(-50, -50),
    new C.Cartesian2(50, -50),
    new C.Cartesian2(50, 50),
    new C.Cartesian2(-50, 50),
  ];
  const e = new C.Entity({
    id: "pv-1",
    polylineVolume: {
      positions,
      shape,
      cornerType: C.CornerType.BEVELED,
      material: C.Color.ORANGE,
      outline: true,
      outlineColor: C.Color.BLACK,
    },
  });
  const u = new C.PolylineVolumeGeometryUpdater(e, scene);
  const summary = summarizeUpdater(u);
  const fill = summarizeInstance(u.createFillGeometryInstance(time0));
  const outline = summarizeInstance(u.createOutlineGeometryInstance(time0));
  u.destroy();
  return { summary, fill, outline };
}

function suiteRectangleGeometryUpdater() {
  const scene = makeScene();
  const coords = C.Rectangle.fromDegrees(-106, 39, -104, 41);
  const eGround = new C.Entity({
    id: "rect-ground",
    rectangle: { coordinates: coords, material: C.Color.RED, zIndex: 2 },
  });
  const uGround = new C.RectangleGeometryUpdater(eGround, scene);
  const groundSummary = summarizeUpdater(uGround);
  const groundFill = summarizeInstance(uGround.createFillGeometryInstance(time0));
  const center = enc(uGround._computeCenter(time0, new C.Cartesian3()));

  const eExtruded = new C.Entity({
    id: "rect-ext",
    rectangle: {
      coordinates: coords,
      height: 100,
      extrudedHeight: 500,
      rotation: 0.25,
      outline: true,
      outlineColor: C.Color.WHITE,
      material: C.Color.BLUE,
    },
  });
  const uExtruded = new C.RectangleGeometryUpdater(eExtruded, scene);
  const extrudedSummary = summarizeUpdater(uExtruded);
  const extrudedFill = summarizeInstance(uExtruded.createFillGeometryInstance(time0));
  const extrudedOutline = summarizeInstance(uExtruded.createOutlineGeometryInstance(time0));

  uGround.destroy();
  uExtruded.destroy();
  return { groundSummary, groundFill, center, extrudedSummary, extrudedFill, extrudedOutline };
}

function suiteWallGeometryUpdater() {
  const scene = makeScene();
  const positions = C.Cartesian3.fromDegreesArray([-105, 40, -104, 40, -104, 41]);
  const e = new C.Entity({
    id: "wall-1",
    wall: {
      positions,
      minimumHeights: [0, 0, 0],
      maximumHeights: [1000, 1500, 1000],
      material: C.Color.GREEN,
      outline: true,
      outlineColor: C.Color.BLACK,
    },
  });
  const u = new C.WallGeometryUpdater(e, scene);
  const summary = summarizeUpdater(u);
  const fill = summarizeInstance(u.createFillGeometryInstance(time0));
  const outline = summarizeInstance(u.createOutlineGeometryInstance(time0));
  u.destroy();
  return { summary, fill, outline };
}

function suiteGeometryVisualizer() {
  const scene = makeScene();
  const entities = new C.EntityCollection();
  entities.add({
    id: "gv-box",
    position: C.Cartesian3.fromDegrees(-105, 40, 100),
    box: { dimensions: new C.Cartesian3(10, 20, 30), material: C.Color.RED, outline: true },
  });
  entities.add({
    id: "gv-rect-ground",
    rectangle: { coordinates: C.Rectangle.fromDegrees(-110, 30, -105, 35), material: C.Color.BLUE },
  });
  entities.add({
    id: "gv-rect-mat",
    rectangle: {
      coordinates: C.Rectangle.fromDegrees(-95, 30, -90, 35),
      material: new C.GridMaterialProperty({ color: C.Color.GREEN }),
    },
  });
  entities.add({
    id: "gv-wall-dyn",
    wall: {
      positions: new C.CallbackProperty(
        () => C.Cartesian3.fromDegreesArrayHeights([-105, 40, 500, -104, 40, 500]),
        false,
      ),
      material: C.Color.YELLOW,
    },
  });

  const viz = new C.GeometryVisualizer(scene, entities);
  const totalBatches = viz._batches.length;
  const firstUpdate = viz.update(time0);
  const afterFirst = [scene.primitives.length, scene.groundPrimitives.length];

  // Remove one ground entity and update
  entities.removeById("gv-rect-ground");
  viz.update(time0);
  const afterRemove = [scene.primitives.length, scene.groundPrimitives.length];

  viz.destroy();
  const afterDestroy = [scene.primitives.length, scene.groundPrimitives.length, viz.isDestroyed()];
  return { totalBatches, firstUpdate, afterFirst, afterRemove, afterDestroy };
}

function suitePolylineVisualizer() {
  const scene = makeScene();
  const entities = new C.EntityCollection();
  const pts = C.Cartesian3.fromDegreesArrayHeights([-105, 40, 100, -104, 40.5, 100]);
  entities.add({
    id: "pv-color",
    polyline: { positions: pts, width: 3, material: C.Color.RED },
  });
  entities.add({
    id: "pv-df",
    polyline: { positions: pts, width: 3, material: C.Color.RED, depthFailMaterial: C.Color.BLUE },
  });
  entities.add({
    id: "pv-mat",
    polyline: {
      positions: pts,
      width: 3,
      material: new C.PolylineDashMaterialProperty({ color: C.Color.YELLOW }),
    },
  });
  entities.add({
    id: "pv-ground",
    polyline: { positions: pts, width: 3, clampToGround: true, material: C.Color.GREEN },
  });
  entities.add({
    id: "pv-dyn",
    polyline: {
      positions: new C.CallbackProperty(() => pts, false),
      width: 4,
      material: C.Color.CYAN,
    },
  });

  const viz = new C.PolylineVisualizer(scene, entities);
  const totalBatches = viz._batches.length;
  const firstUpdate = viz.update(time0);
  const afterFirst = [scene.primitives.length, scene.groundPrimitives.length];

  entities.removeById("pv-ground");
  viz.update(time0);
  const afterRemove = [scene.primitives.length, scene.groundPrimitives.length];

  viz.destroy();
  const afterDestroy = [scene.primitives.length, scene.groundPrimitives.length, viz.isDestroyed()];
  return { totalBatches, firstUpdate, afterFirst, afterRemove, afterDestroy };
}

export default function dataSourcesGeometry() {
  const restoreDom = installDataSourcesDom();
  const savedLineWidths = [C.ContextLimits._minimumAliasedLineWidth, C.ContextLimits._maximumAliasedLineWidth];
  C.ContextLimits._minimumAliasedLineWidth = 1;
  C.ContextLimits._maximumAliasedLineWidth = 5;
  const savedHeights = C.ApproximateTerrainHeights._terrainHeights;
  const savedInit = C.ApproximateTerrainHeights._initPromise;
  try {
    C.ApproximateTerrainHeights._terrainHeights = JSON.parse(
      readFileSync(new URL("../../../assets/approximateTerrainHeights.json", import.meta.url), "utf8"),
    );
    C.ApproximateTerrainHeights._initPromise = Promise.resolve();
    return [
      single("GeometryUpdater", suiteGeometryUpdater),
      single("GroundGeometryUpdater", suiteGroundGeometryUpdater),
      single("DynamicGeometryUpdater", suiteDynamicGeometryUpdater),
      single("DynamicGeometryBatch", suiteDynamicGeometryBatch),
      single("GeometryUpdaterSet", suiteGeometryUpdaterSet),
      single("StaticGeometryColorBatch", suiteStaticGeometryColorBatch),
      single("StaticGeometryPerMaterialBatch", suiteStaticGeometryPerMaterialBatch),
      single("StaticGroundGeometryColorBatch", suiteStaticGroundGeometryColorBatch),
      single("StaticGroundGeometryPerMaterialBatch", suiteStaticGroundGeometryPerMaterialBatch),
      single("StaticGroundPolylinePerMaterialBatch", suiteStaticGroundPolylinePerMaterialBatch),
      single("StaticOutlineGeometryBatch", suiteStaticOutlineGeometryBatch),
      single("BoxGeometryUpdater", suiteBoxGeometryUpdater),
      single("CorridorGeometryUpdater", suiteCorridorGeometryUpdater),
      single("CylinderGeometryUpdater", suiteCylinderGeometryUpdater),
      single("EllipseGeometryUpdater", suiteEllipseGeometryUpdater),
      single("EllipsoidGeometryUpdater", suiteEllipsoidGeometryUpdater),
      single("PlaneGeometryUpdater", suitePlaneGeometryUpdater),
      single("PolygonGeometryUpdater", suitePolygonGeometryUpdater),
      single("PolylineGeometryUpdater", suitePolylineGeometryUpdater),
      single("PolylineVolumeGeometryUpdater", suitePolylineVolumeGeometryUpdater),
      single("RectangleGeometryUpdater", suiteRectangleGeometryUpdater),
      single("WallGeometryUpdater", suiteWallGeometryUpdater),
      single("GeometryVisualizer", suiteGeometryVisualizer),
      single("PolylineVisualizer", suitePolylineVisualizer),
    ];
  } finally {
    restoreDom();
    [C.ContextLimits._minimumAliasedLineWidth, C.ContextLimits._maximumAliasedLineWidth] = savedLineWidths;
    C.ApproximateTerrainHeights._terrainHeights = savedHeights;
    C.ApproximateTerrainHeights._initPromise = savedInit;
  }
}

const isMain = process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1];
if (isMain) {
  const here = dirname(fileURLToPath(import.meta.url));
  const out = join(here, "../../../tests/fixtures/data_sources");
  const version = JSON.parse(readFileSync(join(here, "../node_modules/@cesium/engine/package.json"), "utf8")).version;
  mkdirSync(out, { recursive: true });
  const suites = dataSourcesGeometry();
  for (const s of suites) {
    const file = join(out, `${s.module}.json`);
    writeFileSync(file, JSON.stringify({ module: s.module, cesium: version, cases: s.cases }) + "\n");
    console.log(`data_sources/${s.module}: ${s.cases.length} cases`);
  }
  console.log(`Generated ${suites.length} geometry fixtures.`);
}
