import * as C from "@cesium/engine";
import { enc, suite } from "./lib.mjs";
import { installDataSourcesDom } from "./data-sources-dom.mjs";

const T0 = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");
const addSec = (s) => C.JulianDate.addSeconds(T0, s, new C.JulianDate());

function flushMicrotasks() {
  return Promise.resolve().then(() => Promise.resolve()).then(() => Promise.resolve());
}

function makeMockAtlas() {
  const rectangles = [];
  const byId = new Map();
  return {
    guid: "mock-atlas",
    rectangles,
    textureCoordinates: [],
    addImage(id, _image, width = 16, height = 16) {
      if (byId.has(id)) return byId.get(id);
      const idx = rectangles.length;
      rectangles.push(new C.BoundingRectangle(0, 0, width ?? 16, height ?? 16));
      byId.set(id, idx);
      return idx;
    },
    addImageSubRegion(_id, subRegion) {
      const idx = rectangles.length;
      rectangles.push(C.BoundingRectangle.clone(subRegion));
      return idx;
    },
    isDestroyed() {
      return false;
    },
    destroy() {},
  };
}

function makeCameraScene(options = {}) {
  const ellipsoid = options.ellipsoid ?? C.Ellipsoid.WGS84;
  const width = options.width ?? 800;
  const height = options.height ?? 600;
  const mapProjection = new C.GeographicProjection(ellipsoid);
  const scene = {
    drawingBufferWidth: width,
    drawingBufferHeight: height,
    canvas: { clientWidth: width, clientHeight: height },
    pixelRatio: 1,
    mapMode2D: 1,
    mapProjection,
    ellipsoid,
    mode: options.mode ?? C.SceneMode.SCENE3D,
    screenSpaceCameraController: { minimumZoomDistance: 1, maximumZoomDistance: Infinity, enableInputs: true },
    frameState: { mode: options.mode ?? C.SceneMode.SCENE3D, morphTime: 1.0, mapProjection },
    getHeight: options.getHeight ?? (() => undefined),
    updateHeight: options.updateHeight ?? (() => () => {}),
  };
  const camera = new C.Camera(scene);
  scene.camera = camera;
  camera.update(scene.mode);
  return { scene, camera };
}

function attachMockBillboardCollection(cluster, scene) {
  cluster._scene = scene;
  cluster._billboardCollection = new C.BillboardCollection({
    scene,
    textureAtlas: makeMockAtlas(),
  });
}

// ---------------------------------------------------------------------------
// 1. getElement
// ---------------------------------------------------------------------------

function getElementSuite() {
  const s = suite("getElement");
  const hadDoc = "document" in globalThis;
  const savedDoc = globalThis.document;
  globalThis.document = {
    getElementById(id) {
      return id === "map" ? { id: "map", tag: "div" } : null;
    },
  };
  try {
    s.add("resolve", ["map"], (id) => {
      const el = C.getElement(id);
      return { found: el.id, error: null };
    });
    s.add("resolve", [{ direct: "canvas-1" }], (input) => {
      const el = C.getElement(input);
      return { found: el.direct, error: null };
    });
    s.add("resolve", ["missing"], (id) => {
      try {
        const el = C.getElement(id);
        return { found: el?.id ?? null, error: null };
      } catch (e) {
        return { found: null, error: e.message };
      }
    });
  } finally {
    if (hadDoc) {
      globalThis.document = savedDoc;
    } else {
      delete globalThis.document;
    }
  }
  return s;
}

// ---------------------------------------------------------------------------
// 2. TerrainOffsetProperty
// ---------------------------------------------------------------------------

function makeTerrainScene(hasGlobe, initialHeight) {
  const terrainProviderChanged = new C.Event();
  const morphComplete = new C.Event();
  let currentHeight = initialHeight;
  let activeCallback = null;
  let getHeightCalls = 0;
  let updateHeightCalls = 0;
  let removedCallbacks = 0;

  const scene = {
    globe: hasGlobe ? {} : undefined,
    ellipsoid: C.Ellipsoid.WGS84,
    terrainProviderChanged,
    morphComplete,
    getHeight(_carto, _hr) {
      getHeightCalls++;
      return currentHeight;
    },
    updateHeight(_carto, cb, _hr) {
      updateHeightCalls++;
      activeCallback = cb;
      let removed = false;
      return () => {
        if (!removed) {
          removed = true;
          removedCallbacks++;
          if (activeCallback === cb) activeCallback = null;
        }
      };
    },
  };

  return {
    scene,
    setHeight(h) {
      currentHeight = h;
    },
    fireCallback(height) {
      if (activeCallback) activeCallback(new C.Cartographic(0, 0, height));
    },
    stats() {
      return {
        getHeightCalls,
        updateHeightCalls,
        removedCallbacks,
        terrainListeners: terrainProviderChanged.numberOfListeners,
        morphListeners: morphComplete.numberOfListeners,
      };
    },
  };
}

function terrainOffsetSuite() {
  const s = suite("TerrainOffsetProperty");

  s.add("constantClamped", [], () => {
    const host = makeTerrainScene(true, 250.0);
    const pos = C.Cartesian3.fromRadians(0.5, 0.3, 100.0);
    const posProp = new C.ConstantPositionProperty(pos);
    const hrProp = new C.ConstantProperty(C.HeightReference.CLAMP_TO_GROUND);
    const prop = new C.TerrainOffsetProperty(host.scene, posProp, hrProp, undefined);

    let defChanges = 0;
    prop.definitionChanged.addEventListener(() => defChanges++);

    const initial = prop.getValue(T0, new C.Cartesian3());
    host.fireCallback(310.5);
    const afterCallback = prop.getValue(T0, new C.Cartesian3());

    host.setHeight(180.0);
    host.scene.terrainProviderChanged.raiseEvent();
    const afterTerrain = prop.getValue(T0, new C.Cartesian3());

    host.setHeight(420.0);
    host.scene.morphComplete.raiseEvent();
    const afterMorph = prop.getValue(T0, new C.Cartesian3());

    const isConstant = prop.isConstant;
    const beforeDestroy = prop.isDestroyed();
    prop.destroy();
    const afterDestroy = prop.isDestroyed();

    return {
      isConstant,
      initial: enc(initial),
      defChanges,
      afterCallback: enc(afterCallback),
      afterTerrain: enc(afterTerrain),
      afterMorph: enc(afterMorph),
      beforeDestroy,
      afterDestroy,
      stats: host.stats(),
    };
  });

  s.add("dynamicPositions", [], () => {
    const host = makeTerrainScene(true, 100.0);
    const pos1 = C.Cartesian3.fromRadians(0.1, 0.2, 50.0);
    const pos1Near = new C.Cartesian3(pos1.x + 1e-12, pos1.y, pos1.z);
    const pos2 = C.Cartesian3.fromRadians(0.4, -0.2, 20.0);
    const posProp = new C.CallbackProperty((time) => {
      const dt = C.JulianDate.secondsDifference(time, T0);
      if (dt < 0.5) return pos1;
      if (dt < 1.5) return pos1Near;
      if (dt < 2.5) return pos2;
      if (dt < 3.5) return C.Cartesian3.ZERO;
      return undefined;
    }, false);
    const hrProp = new C.ConstantProperty(C.HeightReference.CLAMP_TO_GROUND);
    const prop = new C.TerrainOffsetProperty(host.scene, posProp, hrProp, undefined);

    const v0 = prop.getValue(addSec(0), new C.Cartesian3());
    const v1 = prop.getValue(addSec(1), new C.Cartesian3());
    host.setHeight(75.0);
    const v2 = prop.getValue(addSec(2), new C.Cartesian3());
    const v3 = prop.getValue(addSec(3), new C.Cartesian3());
    const v4 = prop.getValue(addSec(4), new C.Cartesian3());
    prop.destroy();

    return {
      values: [v0, v1, v2, v3, v4].map(enc),
      stats: host.stats(),
    };
  });

  s.add("heightReferenceCombinations", [], () => {
    const host = makeTerrainScene(true, 123.5);
    const pos = C.Cartesian3.fromRadians(0.2, 0.4, 0.0);
    const posProp = new C.ConstantPositionProperty(pos);
    const hr = new C.ConstantProperty(C.HeightReference.NONE);
    const ehr = new C.ConstantProperty(C.HeightReference.NONE);
    const prop = new C.TerrainOffsetProperty(host.scene, posProp, hr, ehr);

    const noneNone = prop.getValue(T0, new C.Cartesian3());
    ehr.setValue(C.HeightReference.CLAMP_TO_GROUND);
    const noneClamp = prop.getValue(T0, new C.Cartesian3());
    ehr.setValue(C.HeightReference.RELATIVE_TO_GROUND);
    const noneRel = prop.getValue(T0, new C.Cartesian3());
    ehr.setValue(C.HeightReference.NONE);
    hr.setValue(C.HeightReference.RELATIVE_TO_GROUND);
    const relNone = prop.getValue(T0, new C.Cartesian3());
    prop.destroy();

    return {
      noneNone: enc(noneNone),
      noneClamp: enc(noneClamp),
      noneRel: enc(noneRel),
      relNone: enc(relNone),
    };
  });

  s.add("noGlobe", [], () => {
    const host = makeTerrainScene(false, 500.0);
    const pos = C.Cartesian3.fromRadians(0.3, 0.3, 10.0);
    const hr = new C.ConstantProperty(C.HeightReference.CLAMP_TO_GROUND);
    const constProp = new C.TerrainOffsetProperty(
      host.scene,
      new C.ConstantPositionProperty(pos),
      hr,
      undefined,
    );
    const dynProp = new C.TerrainOffsetProperty(
      host.scene,
      new C.CallbackProperty(() => pos, false),
      hr,
      undefined,
    );
    const rConst = constProp.getValue(T0, new C.Cartesian3());
    const rDyn = dynProp.getValue(T0, new C.Cartesian3());
    constProp.destroy();
    dynProp.destroy();
    return {
      rConst: enc(rConst),
      rDyn: enc(rDyn),
      stats: host.stats(),
    };
  });

  return s;
}

// ---------------------------------------------------------------------------
// 3. EntityView
// ---------------------------------------------------------------------------

function cameraSnapshot(camera, view) {
  return {
    transform: enc(camera.transform),
    position: enc(camera.position),
    direction: enc(camera.direction),
    up: enc(camera.up),
    right: enc(camera.right),
    positionWC: enc(camera.positionWC),
    boundingSphere: enc(view.boundingSphere),
  };
}

function entityViewSuite() {
  const s = suite("EntityView");

  s.add("defaultOffsetAndEnu", [], () => {
    const { scene, camera } = makeCameraScene();
    const pos = C.Cartesian3.fromDegrees(-75.0, 40.0, 1000.0);
    const entity = new C.Entity({
      id: "enu",
      position: pos,
      trackingReferenceFrame: C.TrackingReferenceFrame.ENU,
    });
    const view = new C.EntityView(entity, scene, C.Ellipsoid.WGS84);
    view.update(T0);
    const first = cameraSnapshot(camera, view);
    view.update(addSec(5));
    const second = cameraSnapshot(camera, view);
    return {
      defaultOffset3D: enc(C.EntityView.defaultOffset3D),
      first,
      second,
    };
  });

  s.add("viewFromAndBoundingSphere", [], () => {
    const pos = C.Cartesian3.fromDegrees(10.0, 20.0, 500.0);
    const shifted = C.Cartesian3.fromDegrees(10.01, 20.01, 500.0);
    const sphere = new C.BoundingSphere(shifted, 50.0);

    // Entity WITH viewFrom ignores boundingSphere
    const { scene: s1, camera: c1 } = makeCameraScene();
    const e1 = new C.Entity({
      id: "with-view-from",
      position: pos,
      viewFrom: new C.Cartesian3(-500.0, 200.0, 300.0),
      trackingReferenceFrame: C.TrackingReferenceFrame.ENU,
    });
    const v1 = new C.EntityView(e1, s1, C.Ellipsoid.WGS84);
    v1.update(T0, sphere);
    const withViewFrom = cameraSnapshot(c1, v1);

    // Entity WITHOUT viewFrom uses viewBoundingSphere with altitude pitch scaling
    const { scene: s2, camera: c2 } = makeCameraScene();
    const highPos = C.Cartesian3.fromDegrees(10.0, 20.0, 2_000_000.0);
    const highSphere = new C.BoundingSphere(highPos, 200.0);
    const e2 = new C.Entity({
      id: "with-sphere",
      position: highPos,
      trackingReferenceFrame: C.TrackingReferenceFrame.ENU,
    });
    const v2 = new C.EntityView(e2, s2, C.Ellipsoid.WGS84);
    v2.update(T0, highSphere);
    const withSphere = cameraSnapshot(c2, v2);

    return { withViewFrom, withSphere };
  });

  s.add("deepSpaceAndVvlhAutodetect", [], () => {
    // Deep space (> 1.25 * maxRadius) -> North-up basis matrix
    const { scene: s1, camera: c1 } = makeCameraScene();
    const deepPos = new C.Cartesian3(2.0 * C.Ellipsoid.WGS84.maximumRadius, 0.0, 0.0);
    const e1 = new C.Entity({
      id: "deep",
      position: new C.ConstantPositionProperty(deepPos, C.ReferenceFrame.FIXED),
      trackingReferenceFrame: C.TrackingReferenceFrame.AUTODETECT,
    });
    const v1 = new C.EntityView(e1, s1, C.Ellipsoid.WGS84);
    v1.update(T0);
    const deepSpace = cameraSnapshot(c1, v1);

    // LEO circular orbit (0 < semiMajorAxis <= 1.25 * maxRadius) -> VVLH basis
    const { scene: s2, camera: c2 } = makeCameraScene();
    const r = C.Ellipsoid.WGS84.maximumRadius + 400_000.0;
    const omega = 0.00113; // ~7660 m/s
    const orbitPos = new C.CallbackPositionProperty((time, result) => {
      const dt = C.JulianDate.secondsDifference(time, T0);
      const angle = omega * dt;
      return C.Cartesian3.fromElements(r * Math.cos(angle), r * Math.sin(angle), 0.0, result);
    }, false, C.ReferenceFrame.FIXED);
    const e2 = new C.Entity({
      id: "leo",
      position: orbitPos,
      trackingReferenceFrame: C.TrackingReferenceFrame.AUTODETECT,
    });
    const v2 = new C.EntityView(e2, s2, C.Ellipsoid.WGS84);
    v2.update(T0);
    const vvlh = cameraSnapshot(c2, v2);

    return { deepSpace, vvlh };
  });

  s.add("inertialAndVelocityFrames", [], () => {
    const pos = C.Cartesian3.fromDegrees(0.0, 0.0, 1000.0);
    const q = C.Quaternion.normalize(new C.Quaternion(0.1, 0.2, 0.3, 0.9), new C.Quaternion());

    const { scene: s1, camera: c1 } = makeCameraScene();
    const e1 = new C.Entity({
      id: "inertial",
      position: pos,
      orientation: q,
      trackingReferenceFrame: C.TrackingReferenceFrame.INERTIAL,
    });
    const v1 = new C.EntityView(e1, s1, C.Ellipsoid.WGS84);
    v1.update(T0);
    const inertial = cameraSnapshot(c1, v1);

    const { scene: s2, camera: c2 } = makeCameraScene();
    const velPos = new C.CallbackPositionProperty((time, result) => {
      const dt = C.JulianDate.secondsDifference(time, T0);
      return C.Cartesian3.fromDegrees(dt * 0.001, 0.0, 1000.0, C.Ellipsoid.WGS84, result);
    }, false, C.ReferenceFrame.FIXED);
    const e2 = new C.Entity({
      id: "velocity",
      position: velPos,
      trackingReferenceFrame: C.TrackingReferenceFrame.VELOCITY,
    });
    const v2 = new C.EntityView(e2, s2, C.Ellipsoid.WGS84);
    v2.update(addSec(10));
    const velocity = cameraSnapshot(c2, v2);

    return { inertial, velocity };
  });

  s.add("sceneModes", [], () => {
    const { scene, camera } = makeCameraScene();
    const pos = C.Cartesian3.fromDegrees(30.0, 15.0, 500.0);
    const entity = new C.Entity({
      id: "modes",
      position: pos,
      trackingReferenceFrame: C.TrackingReferenceFrame.ENU,
    });
    const view = new C.EntityView(entity, scene, C.Ellipsoid.WGS84);

    // MORPHING is a no-op
    scene.mode = C.SceneMode.MORPHING;
    const beforeMorph = cameraSnapshot(camera, view);
    view.update(T0);
    const afterMorph = cameraSnapshot(camera, view);

    // COLUMBUS_VIEW
    scene.mode = C.SceneMode.COLUMBUS_VIEW;
    camera.update(C.SceneMode.COLUMBUS_VIEW);
    view.update(T0);
    const columbus = cameraSnapshot(camera, view);

    // SCENE2D
    scene.mode = C.SceneMode.SCENE2D;
    camera.frustum = new C.OrthographicOffCenterFrustum({
      left: -1e7,
      right: 1e7,
      top: 7.5e6,
      bottom: -7.5e6,
      near: 1.0,
      far: 1e8,
    });
    camera.update(C.SceneMode.SCENE2D);
    view.update(T0);
    const scene2d = cameraSnapshot(camera, view);

    return { beforeMorph, afterMorph, columbus, scene2d };
  });

  s.add("noPosition", [], () => {
    const { scene, camera } = makeCameraScene();
    const entity = new C.Entity({ id: "empty" });
    const view = new C.EntityView(entity, scene, C.Ellipsoid.WGS84);
    const before = cameraSnapshot(camera, view);
    view.update(T0);
    const after = cameraSnapshot(camera, view);
    return { before, after };
  });

  return s;
}

// ---------------------------------------------------------------------------
// 4. EntityCluster
// ---------------------------------------------------------------------------

function entityClusterSuite() {
  const s = suite("EntityCluster");

  s.add("defaultsAndPoolReuse", [], () => {
    const { scene } = makeCameraScene();
    const cluster = new C.EntityCluster();
    attachMockBillboardCollection(cluster, scene);
    const defaults = {
      enabled: cluster.enabled,
      pixelRange: cluster.pixelRange,
      minimumClusterSize: cluster.minimumClusterSize,
      clusterBillboards: cluster.clusterBillboards,
      clusterLabels: cluster.clusterLabels,
      clusterPoints: cluster.clusterPoints,
      show: cluster.show,
    };

    const e1 = new C.Entity({ id: "e1" });
    const e2 = new C.Entity({ id: "e2" });
    const b1 = cluster.getBillboard(e1);
    const b1Again = cluster.getBillboard(e1);
    const l1 = cluster.getLabel(e1);
    const p1 = cluster.getPoint(e1);
    cluster.removeBillboard(e1);
    cluster.removeLabel(e1);
    cluster.removePoint(e1);
    const b2 = cluster.getBillboard(e2);
    const l2 = cluster.getLabel(e2);
    const p2 = cluster.getPoint(e2);

    const reuse = {
      sameBillboard: b1 === b1Again,
      reusedBillboard: b1 === b2,
      reusedLabel: l1 === l2,
      reusedPoint: p1 === p2,
      billboardLen: cluster._billboardCollection.length,
      labelLen: cluster._labelCollection.length,
      pointLen: cluster._pointCollection.length,
    };

    cluster.destroy();
    const afterDestroy = {
      hasBillboards: !!cluster._billboardCollection,
      hasLabels: !!cluster._labelCollection,
      hasPoints: !!cluster._pointCollection,
    };
    const p3 = cluster.getPoint(e1);
    return { defaults, reuse, afterDestroy, reusablePointLen: cluster._pointCollection.length, p3Show: p3.show };
  });

  s.add("pointClustering", [], () => {
    const { scene, camera } = makeCameraScene();
    camera.setView({ destination: C.Cartesian3.fromDegrees(0.0, 0.0, 10_000_000.0) });

    const cluster = new C.EntityCluster({ enabled: true, pixelRange: 80, minimumClusterSize: 2 });
    cluster._initialize(scene);

    const coords = [
      ["p1", 0.0, 0.0],
      ["p2", 0.05, 0.02],
      ["p3", -0.04, 0.03],
      ["far", 15.0, 15.0],
    ];
    const points = [];
    for (const [id, lon, lat] of coords) {
      const entity = new C.Entity({ id });
      const pt = cluster.getPoint(entity);
      pt.id = entity;
      pt.position = C.Cartesian3.fromDegrees(lon, lat, 0.0);
      pt.pixelSize = 10;
      points.push(pt);
    }

    const events = [];
    cluster.clusterEvent.addEventListener((entities, c) => {
      events.push({
        entities: entities.map((e) => e.id),
        labelText: c.label.text,
        labelShow: c.label.show,
        billboardShow: c.billboard.show,
        pointShow: c.point.show,
        position: enc(c.label.position),
      });
    });

    // Small camera change (< 0.05) should be ignored
    cluster._cluster(0.01);
    const afterSmallAmount = events.length;

    // Full clustering pass
    cluster._cluster();
    const afterCluster = {
      events: events.slice(),
      clusterLabelCount: cluster._clusterLabelCollection ? cluster._clusterLabelCollection.length : 0,
      sourceClusterShow: points.map((p) => p.clusterShow),
    };

    // Disable clustering via enabled = false + updateEnable
    cluster.enabled = false;
    cluster._enabledDirty = false;
    // Call updateEnable indirectly by setting enabled=false and checking via cluster.update or direct run:
    // Actually trigger through enabled=true -> enabled=false on a clean tick
    cluster.enabled = true;
    cluster.enabled = false;
    // Manually invoke _cluster when disabled (no-op)
    events.length = 0;
    cluster._cluster();
    const eventsWhenDisabled = events.length;

    cluster.destroy();
    return { afterSmallAmount, afterCluster, eventsWhenDisabled };
  });

  s.add("minimumClusterSizeAndPixelRange", [], () => {
    const { scene, camera } = makeCameraScene();
    camera.setView({ destination: C.Cartesian3.fromDegrees(0.0, 0.0, 10_000_000.0) });

    const cluster = new C.EntityCluster({ enabled: true, pixelRange: 80, minimumClusterSize: 4 });
    cluster._initialize(scene);

    const coords = [
      ["p1", 0.0, 0.0],
      ["p2", 0.05, 0.02],
      ["p3", -0.04, 0.03],
      ["far", 15.0, 15.0],
    ];
    const points = [];
    for (const [id, lon, lat] of coords) {
      const entity = new C.Entity({ id });
      const pt = cluster.getPoint(entity);
      pt.id = entity;
      pt.position = C.Cartesian3.fromDegrees(lon, lat, 0.0);
      pt.pixelSize = 10;
      points.push(pt);
    }

    cluster._cluster();
    const withMin4 = {
      clusterCount: cluster._clusterLabelCollection ? cluster._clusterLabelCollection.length : 0,
      sourceClusterShow: points.map((p) => p.clusterShow),
    };

    cluster.minimumClusterSize = 2;
    cluster.pixelRange = 1;
    // Spread points slightly farther so pixelRange=1 does not merge them
    points[1].position = C.Cartesian3.fromDegrees(2.0, 0.0, 0.0);
    points[2].position = C.Cartesian3.fromDegrees(-2.0, 0.0, 0.0);
    cluster._cluster();
    const withSmallRange = {
      clusterCount: cluster._clusterLabelCollection ? cluster._clusterLabelCollection.length : 0,
      sourceClusterShow: points.map((p) => p.clusterShow),
    };

    cluster.destroy();
    return { withMin4, withSmallRange };
  });

  s.add("zoomInPreviousClusters", [], () => {
    const { scene, camera } = makeCameraScene();
    camera.setView({ destination: C.Cartesian3.fromDegrees(0.0, 0.0, 10_000_000.0) });

    const cluster = new C.EntityCluster({ enabled: true, pixelRange: 80, minimumClusterSize: 2 });
    cluster._initialize(scene);

    const coords = [
      ["a", 0.0, 0.0],
      ["b", 0.05, 0.02],
      ["c", -0.04, 0.03],
    ];
    for (const [id, lon, lat] of coords) {
      const entity = new C.Entity({ id });
      const pt = cluster.getPoint(entity);
      pt.id = entity;
      pt.position = C.Cartesian3.fromDegrees(lon, lat, 0.0);
      pt.pixelSize = 10;
    }

    const passes = [];
    cluster.clusterEvent.addEventListener((entities, c) => {
      passes[passes.length - 1].push({
        entities: entities.map((e) => e.id),
        text: c.label.text,
        position: enc(c.label.position),
      });
    });

    passes.push([]);
    cluster._cluster();

    // Zoom in (currentHeight < previousHeight) to trigger previousClusters path
    camera.setView({ destination: C.Cartesian3.fromDegrees(0.0, 0.0, 6_000_000.0) });
    passes.push([]);
    cluster._cluster();

    cluster.destroy();
    return { passes };
  });

  s.add("mixedPointAndLabel", [], () => {
    const { scene, camera } = makeCameraScene();
    camera.setView({ destination: C.Cartesian3.fromDegrees(0.0, 0.0, 10_000_000.0) });

    const cluster = new C.EntityCluster({
      enabled: true,
      pixelRange: 80,
      minimumClusterSize: 2,
      clusterPoints: true,
      clusterLabels: false,
    });
    cluster._initialize(scene);

    // e1 & e2 have both point and label, close together -> clustered via point
    // e3 has both point and label, far away -> not clustered; both point and label clusterShow = true
    const makeBoth = (id, lon, lat) => {
      const entity = new C.Entity({ id, point: new C.PointGraphics(), label: new C.LabelGraphics() });
      const pt = cluster.getPoint(entity);
      pt.id = entity;
      pt.position = C.Cartesian3.fromDegrees(lon, lat, 0.0);
      pt.pixelSize = 8;
      const lb = cluster.getLabel(entity);
      lb.id = entity;
      lb.position = C.Cartesian3.fromDegrees(lon, lat, 0.0);
      lb.text = id;
      return { pt, lb };
    };

    const a = makeBoth("a", 0.0, 0.0);
    const b = makeBoth("b", 0.03, 0.01);
    const c = makeBoth("c", 18.0, 12.0);

    const events = [];
    cluster.clusterEvent.addEventListener((entities, cl) => {
      events.push({ entities: entities.map((e) => e.id), text: cl.label.text });
    });

    cluster._cluster();
    const result = {
      events,
      pointShow: [a.pt.clusterShow, b.pt.clusterShow, c.pt.clusterShow],
      labelShow: [a.lb.clusterShow, b.lb.clusterShow, c.lb.clusterShow],
    };
    cluster.destroy();
    return result;
  });

  return s;
}

// ---------------------------------------------------------------------------
// 5. BillboardVisualizer
// ---------------------------------------------------------------------------

function snapshotBillboard(b) {
  if (!b) return null;
  return {
    show: b.show,
    id: b.id?.id ?? b.id ?? null,
    position: enc(b.position),
    color: enc(b.color),
    eyeOffset: enc(b.eyeOffset),
    heightReference: b.heightReference,
    pixelOffset: enc(b.pixelOffset),
    scale: b.scale,
    rotation: b.rotation,
    alignedAxis: enc(b.alignedAxis),
    horizontalOrigin: b.horizontalOrigin,
    verticalOrigin: b.verticalOrigin,
    width: b._width ?? null,
    height: b._height ?? null,
    scaleByDistance: b.scaleByDistance
      ? [b.scaleByDistance.near, b.scaleByDistance.nearValue, b.scaleByDistance.far, b.scaleByDistance.farValue]
      : null,
    translucencyByDistance: b.translucencyByDistance
      ? [
          b.translucencyByDistance.near,
          b.translucencyByDistance.nearValue,
          b.translucencyByDistance.far,
          b.translucencyByDistance.farValue,
        ]
      : null,
    pixelOffsetScaleByDistance: b.pixelOffsetScaleByDistance
      ? [
          b.pixelOffsetScaleByDistance.near,
          b.pixelOffsetScaleByDistance.nearValue,
          b.pixelOffsetScaleByDistance.far,
          b.pixelOffsetScaleByDistance.farValue,
        ]
      : null,
    sizeInMeters: b.sizeInMeters,
    distanceDisplayCondition: b.distanceDisplayCondition
      ? [b.distanceDisplayCondition.near, b.distanceDisplayCondition.far]
      : null,
    disableDepthTestDistance: b.disableDepthTestDistance ?? null,
    splitDirection: b.splitDirection,
    image: b.image ?? null,
  };
}

function billboardVisualizerSuite() {
  const s = suite("BillboardVisualizer");

  s.add("defaultsAndAllProperties", [], () => {
    const { scene } = makeCameraScene();
    const cluster = new C.EntityCluster();
    attachMockBillboardCollection(cluster, scene);
    const collection = new C.EntityCollection();

    const p1 = new C.Cartesian3(6378137.0, 0.0, 0.0);
    const p2 = new C.Cartesian3(0.0, 6378137.0, 0.0);

    collection.add({
      id: "default",
      position: p1,
      billboard: { image: "pin.png" },
    });
    collection.add({
      id: "custom",
      position: p2,
      billboard: {
        image: "icon.png",
        color: new C.Color(0.2, 0.4, 0.6, 0.8),
        eyeOffset: new C.Cartesian3(1, 2, 3),
        pixelOffset: new C.Cartesian2(4, -5),
        scale: 2.5,
        rotation: 0.75,
        alignedAxis: new C.Cartesian3(0, 0, 1),
        horizontalOrigin: C.HorizontalOrigin.LEFT,
        verticalOrigin: C.VerticalOrigin.BOTTOM,
        width: 32,
        height: 48,
        scaleByDistance: new C.NearFarScalar(100, 2.0, 1000, 0.5),
        translucencyByDistance: new C.NearFarScalar(50, 1.0, 500, 0.1),
        pixelOffsetScaleByDistance: new C.NearFarScalar(10, 1.5, 200, 0.25),
        sizeInMeters: true,
        distanceDisplayCondition: new C.DistanceDisplayCondition(10.0, 5000.0),
        disableDepthTestDistance: 1234.0,
        splitDirection: C.SplitDirection.LEFT,
        imageSubRegion: new C.BoundingRectangle(2, 4, 16, 16),
      },
    });

    const vis = new C.BillboardVisualizer(cluster, collection);
    const updated = vis.update(T0);
    const defItem = vis._items.get("default")?.billboard;
    const cusItem = vis._items.get("custom")?.billboard;
    const out = {
      updated,
      defaultBillboard: snapshotBillboard(defItem),
      customBillboard: snapshotBillboard(cusItem),
    };
    vis.destroy();
    cluster.destroy();
    return out;
  });

  s.add("visibilityAvailabilityAndCollection", [], () => {
    const { scene } = makeCameraScene();
    const cluster = new C.EntityCluster();
    attachMockBillboardCollection(cluster, scene);
    const collection = new C.EntityCollection();
    const vis = new C.BillboardVisualizer(cluster, collection);

    const e = collection.add({
      id: "e1",
      position: new C.Cartesian3(6378137.0, 0.0, 0.0),
      availability: new C.TimeIntervalCollection([new C.TimeInterval({ start: T0, stop: addSec(60) })]),
      billboard: {},
    });

    vis.update(T0);
    const noImage = !!vis._items.get("e1")?.billboard;

    e.billboard.image = "a.png";
    vis.update(T0);
    const shown = !!vis._items.get("e1")?.billboard;

    vis.update(addSec(120));
    const outsideAvail = !!vis._items.get("e1")?.billboard;

    vis.update(T0);
    e.billboard.show = false;
    vis.update(T0);
    const graphicsHidden = !!vis._items.get("e1")?.billboard;

    e.billboard.show = true;
    e.show = false;
    vis.update(T0);
    const entityHidden = !!vis._items.get("e1")?.billboard;

    e.show = true;
    vis.update(T0);
    const reshown = !!vis._items.get("e1")?.billboard;

    collection.remove(e);
    const afterRemove = vis._items.contains("e1");

    const beforeDestroy = vis.isDestroyed();
    vis.destroy();
    const afterDestroy = vis.isDestroyed();
    cluster.destroy();

    return {
      noImage,
      shown,
      outsideAvail,
      graphicsHidden,
      entityHidden,
      reshown,
      afterRemove,
      beforeDestroy,
      afterDestroy,
    };
  });

  s.add("boundingSphereAndClamping", [], () => {
    const { scene } = makeCameraScene({
      getHeight: () => 125.0,
    });
    const cluster = new C.EntityCluster();
    attachMockBillboardCollection(cluster, scene);
    const collection = new C.EntityCollection();

    const pos = C.Cartesian3.fromRadians(0.4, 0.2, 50.0);
    const e1 = collection.add({
      id: "unclamped",
      position: pos,
      billboard: { image: "a.png" },
    });
    const e2 = collection.add({
      id: "clamped",
      position: pos,
      billboard: { image: "b.png", heightReference: C.HeightReference.CLAMP_TO_GROUND },
    });
    const unknown = new C.Entity({ id: "unknown" });

    const vis = new C.BillboardVisualizer(cluster, collection);
    vis.update(T0);

    const bs1 = new C.BoundingSphere();
    const s1 = vis.getBoundingSphere(e1, bs1);
    const bs2 = new C.BoundingSphere();
    const s2 = vis.getBoundingSphere(e2, bs2);
    const bs3 = new C.BoundingSphere();
    const s3 = vis.getBoundingSphere(unknown, bs3);

    vis.destroy();
    cluster.destroy();
    return {
      unclamped: [s1, enc(bs1)],
      clamped: [s2, enc(bs2)],
      unknown: s3,
    };
  });

  return s;
}

// ---------------------------------------------------------------------------
// 6. LabelVisualizer
// ---------------------------------------------------------------------------

function snapshotLabel(l) {
  if (!l) return null;
  return {
    show: l.show,
    id: l.id?.id ?? l.id ?? null,
    position: enc(l.position),
    text: l.text,
    scale: l.scale,
    font: l.font,
    style: l.style,
    fillColor: enc(l.fillColor),
    outlineColor: enc(l.outlineColor),
    outlineWidth: l.outlineWidth,
    showBackground: l.showBackground,
    backgroundColor: enc(l.backgroundColor),
    backgroundPadding: enc(l.backgroundPadding),
    pixelOffset: enc(l.pixelOffset),
    eyeOffset: enc(l.eyeOffset),
    heightReference: l.heightReference,
    horizontalOrigin: l.horizontalOrigin,
    verticalOrigin: l.verticalOrigin,
    scaleByDistance: l.scaleByDistance
      ? [l.scaleByDistance.near, l.scaleByDistance.nearValue, l.scaleByDistance.far, l.scaleByDistance.farValue]
      : null,
    translucencyByDistance: l.translucencyByDistance
      ? [
          l.translucencyByDistance.near,
          l.translucencyByDistance.nearValue,
          l.translucencyByDistance.far,
          l.translucencyByDistance.farValue,
        ]
      : null,
    pixelOffsetScaleByDistance: l.pixelOffsetScaleByDistance
      ? [
          l.pixelOffsetScaleByDistance.near,
          l.pixelOffsetScaleByDistance.nearValue,
          l.pixelOffsetScaleByDistance.far,
          l.pixelOffsetScaleByDistance.farValue,
        ]
      : null,
    distanceDisplayCondition: l.distanceDisplayCondition
      ? [l.distanceDisplayCondition.near, l.distanceDisplayCondition.far]
      : null,
    disableDepthTestDistance: l.disableDepthTestDistance ?? null,
  };
}

function labelVisualizerSuite() {
  const s = suite("LabelVisualizer");

  s.add("defaultsAndAllProperties", [], () => {
    const cluster = new C.EntityCluster();
    const collection = new C.EntityCollection();

    const p1 = new C.Cartesian3(6378137.0, 0.0, 0.0);
    const p2 = new C.Cartesian3(0.0, 6378137.0, 0.0);
    collection.add({
      id: "default",
      position: p1,
      label: { text: "Hello" },
    });
    collection.add({
      id: "custom",
      position: p2,
      label: {
        text: "Custom",
        font: "20px monospace",
        style: C.LabelStyle.FILL_AND_OUTLINE,
        scale: 1.75,
        fillColor: new C.Color(0.9, 0.1, 0.2, 1.0),
        outlineColor: new C.Color(0.1, 0.2, 0.3, 0.9),
        outlineWidth: 3.0,
        showBackground: true,
        backgroundColor: new C.Color(0.2, 0.3, 0.4, 0.7),
        backgroundPadding: new C.Cartesian2(10, 6),
        pixelOffset: new C.Cartesian2(-3, 8),
        eyeOffset: new C.Cartesian3(4, 5, 6),
        horizontalOrigin: C.HorizontalOrigin.RIGHT,
        verticalOrigin: C.VerticalOrigin.TOP,
        scaleByDistance: new C.NearFarScalar(100, 2.0, 1000, 0.5),
        translucencyByDistance: new C.NearFarScalar(50, 1.0, 500, 0.2),
        pixelOffsetScaleByDistance: new C.NearFarScalar(10, 1.2, 200, 0.4),
        distanceDisplayCondition: new C.DistanceDisplayCondition(20.0, 4000.0),
        disableDepthTestDistance: 500.0,
      },
    });

    const vis = new C.LabelVisualizer(cluster, collection);
    const updated = vis.update(T0);
    const out = {
      updated,
      defaultLabel: snapshotLabel(vis._items.get("default")?.label),
      customLabel: snapshotLabel(vis._items.get("custom")?.label),
    };
    vis.destroy();
    cluster.destroy();
    return out;
  });

  s.add("visibilityAvailabilityAndCollection", [], () => {
    const cluster = new C.EntityCluster();
    const collection = new C.EntityCollection();
    const vis = new C.LabelVisualizer(cluster, collection);

    const e = collection.add({
      id: "l1",
      position: new C.Cartesian3(6378137.0, 0.0, 0.0),
      availability: new C.TimeIntervalCollection([new C.TimeInterval({ start: T0, stop: addSec(60) })]),
      label: {},
    });

    vis.update(T0);
    const noText = !!vis._items.get("l1")?.label;

    e.label.text = "Visible";
    vis.update(T0);
    const shown = !!vis._items.get("l1")?.label;

    vis.update(addSec(90));
    const outsideAvail = !!vis._items.get("l1")?.label;

    vis.update(T0);
    e.label.show = false;
    vis.update(T0);
    const graphicsHidden = !!vis._items.get("l1")?.label;

    e.label.show = true;
    collection.show = false;
    vis.update(T0);
    const collectionHidden = !!vis._items.get("l1")?.label;

    collection.show = true;
    vis.update(T0);
    const reshown = !!vis._items.get("l1")?.label;

    e.label = undefined;
    const afterPropRemoved = vis._items.contains("l1");

    const beforeDestroy = vis.isDestroyed();
    vis.destroy();
    const afterDestroy = vis.isDestroyed();
    cluster.destroy();

    return {
      noText,
      shown,
      outsideAvail,
      graphicsHidden,
      collectionHidden,
      reshown,
      afterPropRemoved,
      beforeDestroy,
      afterDestroy,
    };
  });

  s.add("boundingSphere", [], () => {
    const cluster = new C.EntityCluster();
    const collection = new C.EntityCollection();
    const pos = new C.Cartesian3(6378137.0, 100.0, 200.0);
    const e = collection.add({ id: "l", position: pos, label: { text: "Tag" } });
    const unknown = new C.Entity({ id: "missing" });

    const vis = new C.LabelVisualizer(cluster, collection);
    vis.update(T0);
    const bs = new C.BoundingSphere();
    const st = vis.getBoundingSphere(e, bs);
    const missing = vis.getBoundingSphere(unknown, new C.BoundingSphere());
    vis.destroy();
    cluster.destroy();
    return { active: [st, enc(bs)], missing };
  });

  return s;
}

// ---------------------------------------------------------------------------
// 7. PointVisualizer
// ---------------------------------------------------------------------------

function snapshotPoint(p) {
  if (!p) return null;
  return {
    show: p.show,
    id: p.id?.id ?? p.id ?? null,
    position: enc(p.position),
    color: enc(p.color),
    outlineColor: enc(p.outlineColor),
    outlineWidth: p.outlineWidth,
    pixelSize: p.pixelSize,
    scaleByDistance: p.scaleByDistance
      ? [p.scaleByDistance.near, p.scaleByDistance.nearValue, p.scaleByDistance.far, p.scaleByDistance.farValue]
      : null,
    translucencyByDistance: p.translucencyByDistance
      ? [
          p.translucencyByDistance.near,
          p.translucencyByDistance.nearValue,
          p.translucencyByDistance.far,
          p.translucencyByDistance.farValue,
        ]
      : null,
    distanceDisplayCondition: p.distanceDisplayCondition
      ? [p.distanceDisplayCondition.near, p.distanceDisplayCondition.far]
      : null,
    disableDepthTestDistance: p.disableDepthTestDistance,
    splitDirection: p.splitDirection,
  };
}

function pointVisualizerSuite() {
  const s = suite("PointVisualizer");

  s.add("unclampedDefaultsAndProperties", [], () => {
    const cluster = new C.EntityCluster();
    const collection = new C.EntityCollection();
    const p1 = new C.Cartesian3(6378137.0, 0.0, 0.0);
    const p2 = new C.Cartesian3(0.0, 6378137.0, 0.0);

    const e1 = collection.add({ id: "default", position: p1, point: {} });
    collection.add({
      id: "custom",
      position: p2,
      point: {
        color: new C.Color(0.3, 0.6, 0.9, 0.75),
        outlineColor: new C.Color(0.9, 0.2, 0.1, 1.0),
        outlineWidth: 2.5,
        pixelSize: 14.0,
        scaleByDistance: new C.NearFarScalar(100, 2.0, 1000, 0.5),
        translucencyByDistance: new C.NearFarScalar(50, 1.0, 500, 0.2),
        distanceDisplayCondition: new C.DistanceDisplayCondition(10.0, 2000.0),
        disableDepthTestDistance: 750.0,
        splitDirection: C.SplitDirection.RIGHT,
      },
    });

    const vis = new C.PointVisualizer(cluster, collection);
    const updated = vis.update(T0);
    const bs = new C.BoundingSphere();
    const bsState = vis.getBoundingSphere(e1, bs);
    const out = {
      updated,
      defaultPoint: snapshotPoint(vis._items.get("default")?.pointPrimitive),
      customPoint: snapshotPoint(vis._items.get("custom")?.pointPrimitive),
      boundingSphere: [bsState, enc(bs)],
    };
    vis.destroy();
    cluster.destroy();
    return out;
  });

  s.add("clampedBillboardPoint", [], () => {
    const { scene } = makeCameraScene({
      getHeight: () => 85.0,
    });
    const cluster = new C.EntityCluster();
    attachMockBillboardCollection(cluster, scene);
    const collection = new C.EntityCollection();

    const pos = C.Cartesian3.fromRadians(0.3, 0.4, 20.0);
    const e = collection.add({
      id: "clamped",
      position: pos,
      point: {
        heightReference: C.HeightReference.CLAMP_TO_GROUND,
        color: C.Color.RED,
        outlineColor: C.Color.BLUE,
        outlineWidth: 0,
        pixelSize: 10,
      },
    });

    const vis = new C.PointVisualizer(cluster, collection);
    vis.update(T0);
    const b = vis._items.get("clamped").billboard;
    const step1 = {
      hasPoint: !!vis._items.get("clamped").pointPrimitive,
      hasBillboard: !!b,
      scale: b.scale,
      image: b.image,
      heightReference: b.heightReference,
    };

    // Change pixelSize while outlineWidth == 0 -> scale updates, textureId stays 50px
    e.point.pixelSize = 25;
    vis.update(T0);
    const step2 = { scale: b.scale, image: b.image };

    // Enable outlineWidth > 0 -> scale = 1.0, textureId encodes exact pixelSize & outlineWidth
    e.point.outlineWidth = 3;
    e.point.pixelSize = 12;
    vis.update(T0);
    const step3 = { scale: b.scale, image: b.image };

    // Sub-1 pixelSize rounds to 0, clamped to 1
    e.point.outlineWidth = 0;
    e.point.pixelSize = 0.2;
    vis.update(T0);
    const step4 = { scale: b.scale, image: b.image };

    // Bounding sphere on clamped billboard
    const bs = new C.BoundingSphere();
    const bsState = vis.getBoundingSphere(e, bs);

    // Switch back to HeightReference.NONE -> transitions to PointPrimitive
    e.point.heightReference = C.HeightReference.NONE;
    vis.update(T0);
    const step5 = {
      hasPoint: !!vis._items.get("clamped").pointPrimitive,
      hasBillboard: !!vis._items.get("clamped").billboard,
      pixelSize: vis._items.get("clamped").pointPrimitive.pixelSize,
    };

    vis.destroy();
    cluster.destroy();
    return { step1, step2, step3, step4, clampedSphere: [bsState, enc(bs)], step5 };
  });

  s.add("visibilityAndLifecycle", [], () => {
    const cluster = new C.EntityCluster();
    const collection = new C.EntityCollection();
    const vis = new C.PointVisualizer(cluster, collection);

    const e = collection.add({
      id: "pt",
      position: new C.Cartesian3(6378137.0, 0.0, 0.0),
      availability: new C.TimeIntervalCollection([new C.TimeInterval({ start: T0, stop: addSec(30) })]),
      point: { show: true },
    });

    vis.update(T0);
    const shown = !!vis._items.get("pt")?.pointPrimitive;

    vis.update(addSec(60));
    const outsideAvail = !!vis._items.get("pt")?.pointPrimitive;

    vis.update(T0);
    e.point.show = false;
    vis.update(T0);
    const hidden = !!vis._items.get("pt")?.pointPrimitive;

    const failedSphere = vis.getBoundingSphere(e, new C.BoundingSphere());
    collection.remove(e);
    const afterRemove = vis._items.contains("pt");

    const beforeDestroy = vis.isDestroyed();
    vis.destroy();
    const afterDestroy = vis.isDestroyed();
    cluster.destroy();

    return { shown, outsideAvail, hidden, failedSphere, afterRemove, beforeDestroy, afterDestroy };
  });

  return s;
}

// ---------------------------------------------------------------------------
// 8. PathVisualizer
// ---------------------------------------------------------------------------

function pathVisualizerSuite() {
  const s = suite("PathVisualizer");

  s.add("subSampleSampled", [], () => {
    const prop = new C.SampledPositionProperty(C.ReferenceFrame.FIXED);
    prop.addSample(addSec(0), new C.Cartesian3(1000, 0, 0));
    prop.addSample(addSec(120), new C.Cartesian3(2000, 1200, 0));
    prop.addSample(addSec(240), new C.Cartesian3(3000, 0, 600));

    const withUpdateInside = C.PathVisualizer._subSample(
      prop,
      addSec(0),
      addSec(240),
      addSec(50),
      C.ReferenceFrame.FIXED,
      40,
    );
    const withUpdateOutside = C.PathVisualizer._subSample(
      prop,
      addSec(20),
      addSec(200),
      addSec(300),
      C.ReferenceFrame.FIXED,
      60,
    );
    const inertial = C.PathVisualizer._subSample(
      prop,
      addSec(0),
      addSec(120),
      addSec(60),
      C.ReferenceFrame.INERTIAL,
      60,
    );
    return {
      withUpdateInside: withUpdateInside.map(enc),
      withUpdateOutside: withUpdateOutside.map(enc),
      inertial: inertial.map(enc),
    };
  });

  s.add("subSampleCallback", [], () => {
    const prop = new C.CallbackPositionProperty((time, result) => {
      const dt = C.JulianDate.secondsDifference(time, T0);
      return C.Cartesian3.fromElements(1000 + dt * 10, dt * 5, 0, result);
    }, false, C.ReferenceFrame.FIXED);
    const pts = C.PathVisualizer._subSample(
      prop,
      addSec(0),
      addSec(100),
      addSec(45),
      C.ReferenceFrame.FIXED,
      30,
    );
    return pts.map(enc);
  });

  s.add("subSampleComposite", [], () => {
    const comp = new C.CompositePositionProperty(C.ReferenceFrame.FIXED);
    comp.intervals.addInterval(
      new C.TimeInterval({
        start: addSec(0),
        stop: addSec(60),
        data: new C.ConstantPositionProperty(new C.Cartesian3(10, 20, 30)),
      }),
    );
    const sampled = new C.SampledPositionProperty(C.ReferenceFrame.FIXED);
    sampled.addSample(addSec(60), new C.Cartesian3(100, 0, 0));
    sampled.addSample(addSec(180), new C.Cartesian3(220, 120, 0));
    comp.intervals.addInterval(
      new C.TimeInterval({
        start: addSec(60),
        stop: addSec(180),
        data: sampled,
      }),
    );
    const pts = C.PathVisualizer._subSample(
      comp,
      addSec(30),
      addSec(150),
      addSec(90),
      C.ReferenceFrame.FIXED,
      60,
    );
    return pts.map(enc);
  });

  s.add("subSampleInterval", [], () => {
    const prop = new C.TimeIntervalCollectionPositionProperty(C.ReferenceFrame.FIXED);
    prop.intervals.addInterval(
      new C.TimeInterval({
        start: addSec(0),
        stop: addSec(30),
        isStartIncluded: true,
        isStopIncluded: false,
        data: new C.Cartesian3(1, 2, 3),
      }),
    );
    prop.intervals.addInterval(
      new C.TimeInterval({
        start: addSec(30),
        stop: addSec(60),
        isStartIncluded: false,
        isStopIncluded: true,
        data: new C.Cartesian3(4, 5, 6),
      }),
    );
    prop.intervals.addInterval(
      new C.TimeInterval({
        start: addSec(60),
        stop: addSec(90),
        isStartIncluded: false,
        isStopIncluded: false,
        data: new C.Cartesian3(7, 8, 9),
      }),
    );
    const pts = C.PathVisualizer._subSample(
      prop,
      addSec(0),
      addSec(90),
      addSec(45),
      C.ReferenceFrame.FIXED,
      10,
    );
    return pts.map(enc);
  });

  s.add("subSampleConstantAndReference", [], () => {
    const cProp = new C.ConstantPositionProperty(new C.Cartesian3(6378137, 0, 0));
    const constPts = C.PathVisualizer._subSample(
      cProp,
      addSec(0),
      addSec(60),
      addSec(30),
      C.ReferenceFrame.FIXED,
      10,
    );

    const scaledConst = new C.ScaledPositionProperty(cProp);
    const scaledConstPts = C.PathVisualizer._subSample(
      scaledConst,
      addSec(0),
      addSec(60),
      addSec(30),
      C.ReferenceFrame.FIXED,
      10,
    );

    const col = new C.EntityCollection();
    const sampled = new C.SampledPositionProperty(C.ReferenceFrame.FIXED);
    sampled.addSample(addSec(0), new C.Cartesian3(10, 0, 0));
    sampled.addSample(addSec(60), new C.Cartesian3(70, 0, 0));
    col.add({ id: "target", position: sampled });
    const refProp = C.ReferenceProperty.fromString(col, "target#position");
    const refPts = C.PathVisualizer._subSample(
      refProp,
      addSec(0),
      addSec(60),
      addSec(25),
      C.ReferenceFrame.FIXED,
      30,
    );

    return {
      constPts: constPts.map(enc),
      scaledConstPts: scaledConstPts.map(enc),
      refPts: refPts.map(enc),
    };
  });

  s.add("subSampleGeneric", [], () => {
    const cb = new C.CallbackPositionProperty((time, result) => {
      const dt = C.JulianDate.secondsDifference(time, T0);
      return C.Cartesian3.fromElements(6378137 + dt * 10, 100, 0, result);
    }, false, C.ReferenceFrame.FIXED);
    const scaled = new C.ScaledPositionProperty(cb);
    const pts = C.PathVisualizer._subSample(
      scaled,
      addSec(0),
      addSec(180),
      addSec(90),
      C.ReferenceFrame.FIXED,
      60,
    );
    return pts.map(enc);
  });

  s.add("lifecycle", [], () => {
    const scene = { mode: C.SceneMode.SCENE3D, primitives: new C.PrimitiveCollection() };
    const collection = new C.EntityCollection();

    const sampled = new C.SampledPositionProperty(C.ReferenceFrame.FIXED);
    sampled.addSample(addSec(0), new C.Cartesian3(6378137, 0, 0));
    sampled.addSample(addSec(120), new C.Cartesian3(6378137, 1200, 0));

    const eMain = collection.add({
      id: "main",
      position: sampled,
      availability: new C.TimeIntervalCollection([new C.TimeInterval({ start: addSec(0), stop: addSec(120) })]),
      path: {
        leadTime: 30,
        trailTime: 20,
        resolution: 60,
        width: 3.5,
        material: C.Color.CYAN,
        distanceDisplayCondition: new C.DistanceDisplayCondition(10, 500),
      },
    });
    const eZero = collection.add({
      id: "zero",
      position: sampled,
      path: { leadTime: 0, trailTime: 0 },
    });
    const eNoBounds = collection.add({
      id: "nobounds",
      position: sampled,
      path: { leadTime: 10 },
    });

    const inertialPos = new C.SampledPositionProperty(C.ReferenceFrame.INERTIAL);
    inertialPos.addSample(addSec(0), new C.Cartesian3(7000000, 0, 0));
    inertialPos.addSample(addSec(120), new C.Cartesian3(7000000, 1000, 0));
    collection.add({
      id: "inertial",
      position: inertialPos,
      availability: new C.TimeIntervalCollection([new C.TimeInterval({ start: addSec(0), stop: addSec(120) })]),
      path: { resolution: 60 },
    });

    const vis = new C.PathVisualizer(scene, collection);
    vis.update(addSec(10));
    const mainPoly10 = vis._items.get("main").polyline;
    const at10 = {
      show: mainPoly10.show,
      width: mainPoly10.width,
      materialType: mainPoly10.material.type,
      materialColor: enc(mainPoly10.material.uniforms.color),
      ddc: [mainPoly10.distanceDisplayCondition.near, mainPoly10.distanceDisplayCondition.far],
      positions: mainPoly10.positions.map(enc),
      zeroHasPoly: !!vis._items.get("zero").polyline,
      noBoundsHasPoly: !!vis._items.get("nobounds").polyline,
      updaterCount3D: Object.keys(vis._updaters).length,
      inertialModelMatrix: enc(vis._updaters[C.ReferenceFrame.INERTIAL]._polylineCollection.modelMatrix),
    };

    vis.update(addSec(50));
    const at50Positions = vis._items.get("main").polyline.positions.map(enc);

    // Switch to 2D: inertial entity migrates to FIXED updater
    scene.mode = C.SceneMode.SCENE2D;
    vis.update(addSec(50));
    const inertialIn2DFrame = vis._items.get("inertial").updater._referenceFrame;

    // Remove all entities -> update destroys updaters
    collection.removeAll();
    vis.update(addSec(50));
    const updatersAfterClear = Object.keys(vis._updaters).length;

    const beforeDestroy = vis.isDestroyed();
    vis.destroy();
    const afterDestroy = vis.isDestroyed();

    return {
      at10,
      at50Positions,
      inertialIn2DFrame,
      updatersAfterClear,
      beforeDestroy,
      afterDestroy,
      _keep: [eMain.id, eZero.id, eNoBounds.id],
    };
  });

  return s;
}

// ---------------------------------------------------------------------------
// 9. ModelVisualizer
// ---------------------------------------------------------------------------

function makeFakeModel(opts = {}) {
  const originalMatrix = C.Matrix4.fromTranslation(new C.Cartesian3(10, 0, 0));
  const nodes = {
    root: {
      originalMatrix: C.Matrix4.clone(originalMatrix),
      matrix: C.Matrix4.clone(originalMatrix),
    },
  };
  const articulations = {};
  let appliedArticulations = 0;
  let animAddAllCount = 0;
  let animRemoveAllCount = 0;
  let _modelMatrix = C.Matrix4.clone(C.Matrix4.IDENTITY);
  let _silhouetteColor = C.Color.clone(C.Color.RED);
  let _color = C.Color.clone(C.Color.WHITE);
  const model = {
    id: undefined,
    show: true,
    ready: opts.ready ?? true,
    scale: 1,
    enableVerticalExaggeration: true,
    minimumPixelSize: 0,
    maximumScale: undefined,
    get modelMatrix() {
      return _modelMatrix;
    },
    set modelMatrix(v) {
      _modelMatrix = C.Matrix4.clone(v, _modelMatrix);
    },
    shadows: C.ShadowMode.ENABLED,
    heightReference: C.HeightReference.NONE,
    distanceDisplayCondition: undefined,
    get silhouetteColor() {
      return _silhouetteColor;
    },
    set silhouetteColor(v) {
      _silhouetteColor = C.Color.clone(v, _silhouetteColor);
    },
    silhouetteSize: 0,
    get color() {
      return _color;
    },
    set color(v) {
      _color = C.Color.clone(v, _color);
    },
    colorBlendMode: C.ColorBlendMode.HIGHLIGHT,
    colorBlendAmount: 0.5,
    clippingPlanes: undefined,
    clampAnimations: true,
    imageBasedLighting: { imageBasedLightingFactor: new C.Cartesian2(1, 1) },
    lightColor: undefined,
    customShader: undefined,
    incrementallyLoadTextures: opts.incrementallyLoadTextures ?? true,
    boundingSphere: new C.BoundingSphere(opts.center ?? new C.Cartesian3(6378137, 0, 0), opts.radius ?? 5.0),
    errorEvent: new C.Event(),
    activeAnimations: {
      length: 0,
      addAll() {
        animAddAllCount++;
        this.length = 1;
      },
      removeAll() {
        animRemoveAllCount++;
        this.length = 0;
      },
    },
    getNode(name) {
      return nodes[name];
    },
    setArticulationStage(k, v) {
      articulations[k] = v;
    },
    applyArticulations() {
      appliedArticulations++;
    },
    _destroyed: false,
    isDestroyed() {
      return this._destroyed;
    },
    destroy() {
      this._destroyed = true;
    },
    _stats() {
      return {
        nodeMatrix: enc(nodes.root.matrix),
        articulationStage: articulations["Wing Deploy"] ?? null,
        activeAnimations: model.activeAnimations.length,
      };
    },
  };
  return model;
}

function snapshotModel(m) {
  return {
    show: m.show,
    scale: m.scale,
    enableVerticalExaggeration: m.enableVerticalExaggeration,
    minimumPixelSize: m.minimumPixelSize,
    maximumScale: m.maximumScale ?? null,
    modelMatrix: enc(m.modelMatrix),
    shadows: m.shadows,
    heightReference: m.heightReference,
    distanceDisplayCondition: m.distanceDisplayCondition
      ? [m.distanceDisplayCondition.near, m.distanceDisplayCondition.far]
      : null,
    silhouetteColor: enc(m.silhouetteColor),
    silhouetteSize: m.silhouetteSize,
    color: enc(m.color),
    colorBlendMode: m.colorBlendMode,
    colorBlendAmount: m.colorBlendAmount,
    clampAnimations: m.clampAnimations,
    imageBasedLightingFactor: enc(m.imageBasedLighting.imageBasedLightingFactor),
  };
}

async function modelVisualizerSuite() {
  const cases = [];
  const savedFromGltf = C.Model.fromGltfAsync;
  const savedLog = console.log;

  try {
    // Case 1: defaultsAndProperties
    {
      const requests = [];
      const created = new Map();
      C.Model.fromGltfAsync = async (opts) => {
        requests.push({
          url: opts.url.url,
          incrementallyLoadTextures: opts.incrementallyLoadTextures,
          maxPosEps: opts.environmentMapOptions?.maximumPositionEpsilon ?? null,
        });
        const m = makeFakeModel({ incrementallyLoadTextures: opts.incrementallyLoadTextures });
        created.set(opts.url.url, m);
        return m;
      };

      const scene = {
        primitives: new C.PrimitiveCollection(),
        ellipsoid: C.Ellipsoid.WGS84,
        getHeight: () => undefined,
      };
      const collection = new C.EntityCollection();
      const pos1 = new C.Cartesian3(6378137, 0, 0);
      const pos2 = new C.Cartesian3(0, 6378137, 0);

      collection.add({
        id: "def",
        position: pos1,
        model: { uri: "https://example.test/def.glb" },
      });

      const nodeBag = new C.PropertyBag();
      nodeBag.addProperty(
        "root",
        new C.NodeTransformationProperty({
          translation: new C.Cartesian3(1, 2, 3),
          rotation: C.Quaternion.IDENTITY,
          scale: new C.Cartesian3(2, 2, 2),
        }),
      );
      const artBag = new C.PropertyBag();
      artBag.addProperty("Wing Deploy", 45.0);

      const eCustom = collection.add({
        id: "custom",
        position: pos2,
        model: {
          uri: "https://example.test/custom.glb",
          scale: 3.0,
          enableVerticalExaggeration: false,
          minimumPixelSize: 64.0,
          maximumScale: 100.0,
          incrementallyLoadTextures: false,
          runAnimations: true,
          clampAnimations: false,
          shadows: C.ShadowMode.CAST_ONLY,
          heightReference: C.HeightReference.NONE,
          silhouetteColor: C.Color.LIME,
          silhouetteSize: 2.0,
          color: new C.Color(0.5, 0.6, 0.7, 0.8),
          colorBlendMode: C.ColorBlendMode.MIX,
          colorBlendAmount: 0.75,
          imageBasedLightingFactor: new C.Cartesian2(0.5, 0.25),
          lightColor: new C.Color(0.8, 0.6, 0.4, 1.0),
          distanceDisplayCondition: new C.DistanceDisplayCondition(5, 500),
          nodeTransformations: nodeBag,
          articulations: artBag,
        },
      });

      const vis = new C.ModelVisualizer(scene, collection);
      vis.update(T0);
      await flushMicrotasks();
      vis.update(T0);

      const mDef = created.get("https://example.test/def.glb");
      const mCus = created.get("https://example.test/custom.glb");
      const animWhileRunning = mCus.activeAnimations.length;

      // Stop animations on second tick
      eCustom.model.runAnimations = false;
      vis.update(T0);

      cases.push({
        f: "defaultsAndProperties",
        a: [],
        r: enc({
          requests,
          defaultModel: snapshotModel(mDef),
          customModel: snapshotModel(mCus),
          animWhileRunning,
          customStats: mCus._stats(),
        }),
      });
      vis.destroy();
    }

    // Case 2: boundingSphereStates
    {
      let resolvePending;
      const pendingPromise = new Promise((r) => {
        resolvePending = r;
      });
      const pos = C.Cartesian3.fromRadians(0.2, 0.3, 40.0);
      const fakeModel = makeFakeModel({ ready: false, center: pos, radius: Math.SQRT2 });
      C.Model.fromGltfAsync = async (opts) => {
        if (opts.url.url.includes("fail")) throw new Error("boom");
        return await pendingPromise;
      };
      console.log = () => {};

      const scene = {
        primitives: new C.PrimitiveCollection(),
        ellipsoid: C.Ellipsoid.WGS84,
        getHeight: () => 150.0,
      };
      const collection = new C.EntityCollection();
      const eOk = collection.add({
        id: "ok",
        position: pos,
        model: { uri: "https://example.test/ok.glb" },
      });
      const eFail = collection.add({
        id: "fail",
        position: pos,
        model: { uri: "https://example.test/fail.glb" },
      });
      const eMissing = new C.Entity({ id: "missing" });

      const vis = new C.ModelVisualizer(scene, collection);
      const missingBefore = vis.getBoundingSphere(eMissing, new C.BoundingSphere());

      vis.update(T0);
      const whilePending = vis.getBoundingSphere(eOk, new C.BoundingSphere());
      await flushMicrotasks();
      const afterFail = vis.getBoundingSphere(eFail, new C.BoundingSphere());

      resolvePending(fakeModel);
      await flushMicrotasks();
      vis.update(T0);
      const whileNotReady = vis.getBoundingSphere(eOk, new C.BoundingSphere());

      fakeModel.ready = true;
      vis.update(T0);
      const bsNone = new C.BoundingSphere();
      const sNone = vis.getBoundingSphere(eOk, bsNone);

      eOk.model.show = false;
      vis.update(T0);
      const whileHidden = vis.getBoundingSphere(eOk, new C.BoundingSphere());

      eOk.model.show = true;
      eOk.model.heightReference = C.HeightReference.CLAMP_TO_GROUND;
      vis.update(T0);
      const bsClamp = new C.BoundingSphere();
      const sClamp = vis.getBoundingSphere(eOk, bsClamp);

      eOk.model.heightReference = C.HeightReference.RELATIVE_TO_GROUND;
      vis.update(T0);
      const bsRel = new C.BoundingSphere();
      const sRel = vis.getBoundingSphere(eOk, bsRel);

      vis.destroy();
      cases.push({
        f: "boundingSphereStates",
        a: [],
        r: enc({
          missingBefore,
          whilePending,
          afterFail,
          whileNotReady,
          none: [sNone, enc(bsNone)],
          whileHidden,
          clamp: [sClamp, enc(bsClamp)],
          relative: [sRel, enc(bsRel)],
        }),
      });
    }

    // Case 3: uriChangeAndLifecycle
    {
      const urls = [];
      C.Model.fromGltfAsync = async (opts) => {
        urls.push(opts.url.url);
        return makeFakeModel();
      };
      const scene = { primitives: new C.PrimitiveCollection(), ellipsoid: C.Ellipsoid.WGS84 };
      const collection = new C.EntityCollection();
      const e = collection.add({
        id: "swap",
        position: new C.Cartesian3(6378137, 0, 0),
        model: { uri: "https://example.test/v1.glb" },
      });

      const vis = new C.ModelVisualizer(scene, collection);
      vis.update(T0);
      await flushMicrotasks();
      vis.update(T0);
      const countV1 = scene.primitives.length;

      e.model.uri = "https://example.test/v2.glb";
      vis.update(T0);
      await flushMicrotasks();
      vis.update(T0);
      const countV2 = scene.primitives.length;

      collection.remove(e);
      const countAfterRemove = scene.primitives.length;
      const beforeDestroy = vis.isDestroyed();
      vis.destroy();
      const afterDestroy = vis.isDestroyed();

      cases.push({
        f: "uriChangeAndLifecycle",
        a: [],
        r: enc({ urls, countV1, countV2, countAfterRemove, beforeDestroy, afterDestroy }),
      });
    }
  } finally {
    C.Model.fromGltfAsync = savedFromGltf;
    console.log = savedLog;
  }

  return { module: "ModelVisualizer", cases };
}

// ---------------------------------------------------------------------------
// 10. Cesium3DTilesetVisualizer
// ---------------------------------------------------------------------------

function makeFakeTileset(center = new C.Cartesian3(6378137, 0, 0), radius = 10.0) {
  return {
    id: undefined,
    show: true,
    modelMatrix: C.Matrix4.clone(C.Matrix4.IDENTITY),
    maximumScreenSpaceError: 16.0,
    boundingSphere: new C.BoundingSphere(center, radius),
    _destroyed: false,
    isDestroyed() {
      return this._destroyed;
    },
    destroy() {
      this._destroyed = true;
    },
  };
}

async function tilesetVisualizerSuite() {
  const cases = [];
  const savedFromUrl = C.Cesium3DTileset.fromUrl;
  const savedError = console.error;

  try {
    // Case 1: updateAndProperties
    {
      const created = new Map();
      C.Cesium3DTileset.fromUrl = async (resource) => {
        const ts = makeFakeTileset();
        created.set(resource.url, ts);
        return ts;
      };
      const scene = { primitives: new C.PrimitiveCollection() };
      const collection = new C.EntityCollection();

      collection.add({
        id: "noPos",
        tileset: { uri: "https://example.test/a/tileset.json" },
      });
      const e2 = collection.add({
        id: "withPos",
        position: new C.Cartesian3(6378137, 0, 0),
        tileset: {
          uri: "https://example.test/b/tileset.json",
          maximumScreenSpaceError: 8.0,
        },
      });

      const vis = new C.Cesium3DTilesetVisualizer(scene, collection);
      vis.update(T0);
      await flushMicrotasks();
      vis.update(T0);

      const tsA = created.get("https://example.test/a/tileset.json");
      const tsB = created.get("https://example.test/b/tileset.json");
      const shownState = {
        aShow: tsA.show,
        aSse: tsA.maximumScreenSpaceError,
        aMatrix: enc(tsA.modelMatrix),
        bShow: tsB.show,
        bSse: tsB.maximumScreenSpaceError,
        bMatrix: enc(tsB.modelMatrix),
      };

      e2.tileset.show = false;
      vis.update(T0);
      const hiddenShow = tsB.show;

      e2.tileset.show = true;
      vis.update(T0);
      const reshownShow = tsB.show;

      vis.destroy();
      cases.push({
        f: "updateAndProperties",
        a: [],
        r: enc({ shownState, hiddenShow, reshownShow }),
      });
    }

    // Case 2: boundingSphereStates
    {
      let resolvePending;
      const pendingPromise = new Promise((r) => {
        resolvePending = r;
      });
      const fakeTs = makeFakeTileset(new C.Cartesian3(6378137, 50, 0), 25.0);
      C.Cesium3DTileset.fromUrl = async (resource) => {
        if (resource.url.includes("fail")) throw new Error("404");
        return await pendingPromise;
      };
      console.error = () => {};

      const scene = { primitives: new C.PrimitiveCollection() };
      const collection = new C.EntityCollection();
      const eOk = collection.add({
        id: "ok",
        tileset: { uri: "https://example.test/ok/tileset.json" },
      });
      const eFail = collection.add({
        id: "fail",
        tileset: { uri: "https://example.test/fail/tileset.json" },
      });
      const eMissing = new C.Entity({ id: "missing" });

      const vis = new C.Cesium3DTilesetVisualizer(scene, collection);
      const missingState = vis.getBoundingSphere(eMissing, new C.BoundingSphere());

      vis.update(T0);
      const pendingState = vis.getBoundingSphere(eOk, new C.BoundingSphere());
      await flushMicrotasks();
      const failedLoadState = vis.getBoundingSphere(eFail, new C.BoundingSphere());

      resolvePending(fakeTs);
      await flushMicrotasks();
      vis.update(T0);
      const bs = new C.BoundingSphere();
      const doneState = vis.getBoundingSphere(eOk, bs);

      eOk.tileset.show = false;
      vis.update(T0);
      const hiddenState = vis.getBoundingSphere(eOk, new C.BoundingSphere());

      vis.destroy();
      cases.push({
        f: "boundingSphereStates",
        a: [],
        r: enc({
          missingState,
          pendingState,
          failedLoadState,
          done: [doneState, enc(bs)],
          hiddenState,
        }),
      });
    }

    // Case 3: uriChangeAndDestroy
    {
      const urls = [];
      C.Cesium3DTileset.fromUrl = async (resource) => {
        urls.push(resource.url);
        return makeFakeTileset();
      };
      const scene = { primitives: new C.PrimitiveCollection() };
      const collection = new C.EntityCollection();
      const e = collection.add({
        id: "swap",
        tileset: { uri: "https://example.test/1/tileset.json" },
      });

      const vis = new C.Cesium3DTilesetVisualizer(scene, collection);
      vis.update(T0);
      await flushMicrotasks();
      vis.update(T0);
      const count1 = scene.primitives.length;

      e.tileset.uri = "https://example.test/2/tileset.json";
      vis.update(T0);
      await flushMicrotasks();
      vis.update(T0);
      const count2 = scene.primitives.length;

      e.tileset = undefined;
      const countAfterClear = scene.primitives.length;

      const beforeDestroy = vis.isDestroyed();
      vis.destroy();
      const afterDestroy = vis.isDestroyed();

      cases.push({
        f: "uriChangeAndDestroy",
        a: [],
        r: enc({ urls, count1, count2, countAfterClear, beforeDestroy, afterDestroy }),
      });
    }
  } finally {
    C.Cesium3DTileset.fromUrl = savedFromUrl;
    console.error = savedError;
  }

  return { module: "Cesium3DTilesetVisualizer", cases };
}

export default async function dataSourcesVisualizers() {
  const restoreDom = installDataSourcesDom();
  const savedIcrf = C.Transforms.computeIcrfToFixedMatrix;
  const savedFixedToIcrf = C.Transforms.computeFixedToIcrfMatrix;
  // Keep Earth orientation data offline so both engines use the TEME fallback.
  C.Transforms.computeIcrfToFixedMatrix = () => undefined;
  C.Transforms.computeFixedToIcrfMatrix = () => undefined;
  try {
    return [
      getElementSuite(),
      terrainOffsetSuite(),
      entityViewSuite(),
      entityClusterSuite(),
      billboardVisualizerSuite(),
      labelVisualizerSuite(),
      pointVisualizerSuite(),
      pathVisualizerSuite(),
      await modelVisualizerSuite(),
      await tilesetVisualizerSuite(),
    ];
  } finally {
    C.Transforms.computeIcrfToFixedMatrix = savedIcrf;
    C.Transforms.computeFixedToIcrfMatrix = savedFixedToIcrf;
    restoreDom();
  }
}
