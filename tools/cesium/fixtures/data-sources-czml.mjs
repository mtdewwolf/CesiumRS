globalThis.Image ??= class Image {};
globalThis.HTMLCanvasElement ??= class HTMLCanvasElement {};
globalThis.HTMLVideoElement ??= class HTMLVideoElement {};

import * as C from "@cesium/engine";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const T0 = "2020-01-01T00:00:00Z";
const T1 = "2020-01-01T00:00:10Z";
const T2 = "2020-01-01T00:00:20Z";
const T3 = "2020-01-01T00:00:30Z";
const T4 = "2020-01-01T00:01:00Z";
const T_BEFORE = "2019-12-31T23:59:55Z";
const T_MID = "2020-01-01T00:00:05Z";
const T_AFTER = "2020-01-01T00:00:35Z";
const T_FAR = "2020-01-01T00:05:00Z";

function encodeVal(v) {
  if (v === undefined || v === null) return null;
  if (typeof v === "number") {
    if (Number.isNaN(v)) return "NaN";
    if (v === Infinity) return "Infinity";
    if (v === -Infinity) return "-Infinity";
    if (Object.is(v, -0)) return "-0";
    return v;
  }
  if (typeof v === "boolean" || typeof v === "string") return v;
  if (v instanceof C.Resource) return v.url;
  if (v instanceof C.JulianDate) return C.JulianDate.toIso8601(v);
  if (v instanceof C.Cartesian2) return [v.x, v.y].map(encodeVal);
  if (v instanceof C.Cartesian3) return [v.x, v.y, v.z].map(encodeVal);
  if (v instanceof C.Cartesian4) return [v.x, v.y, v.z, v.w].map(encodeVal);
  if (v instanceof C.Quaternion) return [v.x, v.y, v.z, v.w].map(encodeVal);
  if (v instanceof C.Color) return [v.red, v.green, v.blue, v.alpha].map(encodeVal);
  if (v instanceof C.Rectangle) {
    return {
      west: encodeVal(v.west),
      south: encodeVal(v.south),
      east: encodeVal(v.east),
      north: encodeVal(v.north),
    };
  }
  if (v instanceof C.BoundingRectangle) {
    return {
      x: encodeVal(v.x),
      y: encodeVal(v.y),
      width: encodeVal(v.width),
      height: encodeVal(v.height),
    };
  }
  if (v instanceof C.NearFarScalar) {
    return {
      near: encodeVal(v.near),
      nearValue: encodeVal(v.nearValue),
      far: encodeVal(v.far),
      farValue: encodeVal(v.farValue),
    };
  }
  if (v instanceof C.DistanceDisplayCondition) {
    return { near: encodeVal(v.near), far: encodeVal(v.far) };
  }
  if (v instanceof C.PolygonHierarchy) {
    return {
      positions: (v.positions ?? []).map(encodeVal),
      holes: (v.holes ?? []).map(encodeVal),
    };
  }
  if (v instanceof C.TranslationRotationScale) {
    return {
      translation: encodeVal(v.translation),
      rotation: encodeVal(v.rotation),
      scale: encodeVal(v.scale),
    };
  }
  if (Array.isArray(v)) return v.map(encodeVal);
  if (typeof v === "object") {
    const out = {};
    for (const [k, val] of Object.entries(v)) {
      if (val !== undefined) out[k] = encodeVal(val);
    }
    return out;
  }
  return v;
}

function encodeClock(clock) {
  if (!clock) return null;
  return {
    start: C.JulianDate.toIso8601(clock.startTime),
    stop: C.JulianDate.toIso8601(clock.stopTime),
    current: C.JulianDate.toIso8601(clock.currentTime),
    range: clock.clockRange,
    step: clock.clockStep,
    multiplier: clock.multiplier,
  };
}

function evalProp(p, time) {
  if (!p) return null;
  return encodeVal(p.getValue(time));
}

function evalMat(m, time) {
  if (!m) return null;
  const type = m.getType(time) ?? null;
  const value = encodeVal(m.getValue(time));
  return { type, value };
}

function evalFields(obj, keys, time, materialKeys = []) {
  if (!obj) return null;
  const out = {};
  for (const k of keys) {
    if (materialKeys.includes(k)) {
      out[k] = evalMat(obj[k], time);
    } else {
      out[k] = evalProp(obj[k], time);
    }
  }
  return out;
}

function snapshotEntity(e, time, normId) {
  const availability = e.availability
    ? Array.from({ length: e.availability.length }, (_, i) => {
        const iv = e.availability.get(i);
        return [C.JulianDate.toIso8601(iv.start), C.JulianDate.toIso8601(iv.stop)];
      })
    : null;

  const model = e.model
    ? {
        ...evalFields(
          e.model,
          [
            "show",
            "uri",
            "scale",
            "minimumPixelSize",
            "maximumScale",
            "incrementallyLoadTextures",
            "runAnimations",
            "clampAnimations",
            "shadows",
            "heightReference",
            "silhouetteColor",
            "silhouetteSize",
            "color",
            "colorBlendMode",
            "colorBlendAmount",
            "distanceDisplayCondition",
          ],
          time,
        ),
        nodeTransformations: evalProp(e.model.nodeTransformations, time),
        articulations: evalProp(e.model.articulations, time),
      }
    : null;

  return {
    id: normId(e.id),
    name: e.name ?? null,
    parent: e.parent ? normId(e.parent.id) : null,
    description: evalProp(e.description, time),
    availability,
    position: evalProp(e.position, time),
    positionInertial: e.position
      ? encodeVal(e.position.getValueInReferenceFrame(time, C.ReferenceFrame.INERTIAL))
      : null,
    orientation: evalProp(e.orientation, time),
    viewFrom: evalProp(e.viewFrom, time),
    properties: evalProp(e.properties, time),
    billboard: evalFields(
      e.billboard,
      [
        "show",
        "image",
        "scale",
        "pixelOffset",
        "eyeOffset",
        "horizontalOrigin",
        "verticalOrigin",
        "heightReference",
        "color",
        "rotation",
        "alignedAxis",
        "sizeInMeters",
        "width",
        "height",
        "scaleByDistance",
        "translucencyByDistance",
        "pixelOffsetScaleByDistance",
        "imageSubRegion",
        "distanceDisplayCondition",
        "disableDepthTestDistance",
      ],
      time,
    ),
    box: evalFields(
      e.box,
      [
        "show",
        "dimensions",
        "heightReference",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "shadows",
        "distanceDisplayCondition",
      ],
      time,
      ["material"],
    ),
    corridor: evalFields(
      e.corridor,
      [
        "show",
        "positions",
        "width",
        "height",
        "heightReference",
        "extrudedHeight",
        "extrudedHeightReference",
        "cornerType",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex",
      ],
      time,
      ["material"],
    ),
    cylinder: evalFields(
      e.cylinder,
      [
        "show",
        "length",
        "topRadius",
        "bottomRadius",
        "heightReference",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "numberOfVerticalLines",
        "slices",
        "shadows",
        "distanceDisplayCondition",
      ],
      time,
      ["material"],
    ),
    ellipse: evalFields(
      e.ellipse,
      [
        "show",
        "semiMajorAxis",
        "semiMinorAxis",
        "height",
        "heightReference",
        "extrudedHeight",
        "extrudedHeightReference",
        "rotation",
        "stRotation",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "numberOfVerticalLines",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex",
      ],
      time,
      ["material"],
    ),
    ellipsoid: evalFields(
      e.ellipsoid,
      [
        "show",
        "radii",
        "innerRadii",
        "minimumClock",
        "maximumClock",
        "minimumCone",
        "maximumCone",
        "heightReference",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "stackPartitions",
        "slicePartitions",
        "subdivisions",
        "shadows",
        "distanceDisplayCondition",
      ],
      time,
      ["material"],
    ),
    label: evalFields(
      e.label,
      [
        "show",
        "text",
        "font",
        "style",
        "scale",
        "showBackground",
        "backgroundColor",
        "backgroundPadding",
        "pixelOffset",
        "eyeOffset",
        "horizontalOrigin",
        "verticalOrigin",
        "heightReference",
        "fillColor",
        "outlineColor",
        "outlineWidth",
        "translucencyByDistance",
        "pixelOffsetScaleByDistance",
        "scaleByDistance",
        "distanceDisplayCondition",
        "disableDepthTestDistance",
      ],
      time,
    ),
    model,
    path: evalFields(
      e.path,
      ["show", "leadTime", "trailTime", "width", "resolution", "material", "distanceDisplayCondition"],
      time,
      ["material"],
    ),
    point: evalFields(
      e.point,
      [
        "show",
        "pixelSize",
        "heightReference",
        "color",
        "outlineColor",
        "outlineWidth",
        "scaleByDistance",
        "translucencyByDistance",
        "distanceDisplayCondition",
        "disableDepthTestDistance",
      ],
      time,
    ),
    polygon: evalFields(
      e.polygon,
      [
        "show",
        "hierarchy",
        "height",
        "heightReference",
        "extrudedHeight",
        "extrudedHeightReference",
        "stRotation",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "perPositionHeight",
        "closeTop",
        "closeBottom",
        "arcType",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex",
      ],
      time,
      ["material"],
    ),
    polyline: evalFields(
      e.polyline,
      [
        "show",
        "positions",
        "width",
        "granularity",
        "material",
        "depthFailMaterial",
        "arcType",
        "clampToGround",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex",
      ],
      time,
      ["material", "depthFailMaterial"],
    ),
    polylineVolume: evalFields(
      e.polylineVolume,
      [
        "positions",
        "shape",
        "show",
        "cornerType",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "granularity",
        "shadows",
        "distanceDisplayCondition",
      ],
      time,
      ["material"],
    ),
    rectangle: evalFields(
      e.rectangle,
      [
        "show",
        "coordinates",
        "height",
        "heightReference",
        "extrudedHeight",
        "extrudedHeightReference",
        "rotation",
        "stRotation",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex",
      ],
      time,
      ["material"],
    ),
    tileset: evalFields(e.tileset, ["show", "uri", "maximumScreenSpaceError"], time),
    wall: evalFields(
      e.wall,
      [
        "show",
        "positions",
        "minimumHeights",
        "maximumHeights",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "shadows",
        "distanceDisplayCondition",
      ],
      time,
      ["material"],
    ),
  };
}

async function runScenario(initialName, ops, sampleTimes) {
  const savedIcrf = C.Transforms.computeIcrfToFixedMatrix;
  C.Transforms.computeIcrfToFixedMatrix = () => undefined;
  try {
    const ds = new C.CzmlDataSource(initialName ?? undefined);
    const autoIds = new Map();
    const normId = (id) => {
      if (!/^[a-f0-9]{8}-[a-f0-9-]{27}$/i.test(id)) return id;
      if (!autoIds.has(id)) autoIds.set(id, `@${autoIds.size}`);
      return autoIds.get(id);
    };
    const trace = { changed: 0, loading: [], errors: [] };
    ds.changedEvent.addEventListener(() => trace.changed++);
    ds.loadingEvent.addEventListener((_, v) => trace.loading.push(v));
    ds.errorEvent.addEventListener((_, e) => trace.errors.push(e.message));

    const stepResults = [];
    for (const op of ops) {
      const opts = {};
      if (op.options?.sourceUri) opts.sourceUri = op.options.sourceUri;
      if (op.options?.credit) opts.credit = op.options.credit;
      let error = null;
      try {
        await ds[op.method](op.czml, opts);
      } catch (e) {
        error = e.message;
      }
      const frames = {};
      for (const iso of sampleTimes) {
        const jd = C.JulianDate.fromIso8601(iso);
        frames[iso] = ds.entities.values.map((e) => snapshotEntity(e, jd, normId));
      }
      stepResults.push({
        name: ds.name ?? null,
        clock: encodeClock(ds.clock),
        credit: ds.credit ? ds.credit.html : null,
        isLoading: ds.isLoading,
        error,
        frames,
      });
    }

    return { steps: stepResults, trace };
  } finally {
    C.Transforms.computeIcrfToFixedMatrix = savedIcrf;
  }
}

const DOC = { id: "document", version: "1.0" };

const SCENARIOS = [
  [
    "document_and_clock",
    null,
    [
      {
        method: "load",
        czml: [
          {
            id: "document",
            version: "1.0",
            name: "Mission Alpha",
            clock: {
              interval: `${T0}/${T4}`,
              currentTime: T1,
              multiplier: 5.5,
              range: "CLAMPED",
              step: "TICK_DEPENDENT",
            },
          },
        ],
        options: { credit: "CZML Credit" },
      },
      {
        method: "process",
        czml: [
          {
            id: "document",
            name: "Mission Beta",
            clock: {
              currentTime: T2,
              multiplier: 12,
              range: "UNBOUNDED",
              step: "SYSTEM_CLOCK",
            },
          },
        ],
      },
    ],
    [T0],
  ],
  [
    "clock_inferred_and_source_uri_name",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          {
            id: "sat",
            availability: `${T0}/${T4}`,
            position: { cartesian: [7000000, 0, 0] },
          },
        ],
        options: { sourceUri: "https://example.test/missions/orbit.czml?v=1" },
      },
    ],
    [T0],
  ],
  // Keep the clock bounds fixed so this case isolates entity lifecycle.
  [
    "entity_lifecycle_parent_delete",
    "InitialName",
    [
      {
        method: "load",
        czml: [
          DOC,
          { name: "AutoGuidEntity", description: "Created without id" },
          {
            id: "child",
            parent: "parentNode",
            name: "Child",
            description: { string: "Child desc" },
            viewFrom: { cartesian: [-100, -200, 300] },
            availability: [`${T0}/${T1}`, `${T2}/${T3}`],
          },
          { id: "parentNode", name: "Parent" },
        ],
      },
      {
        method: "process",
        czml: [
          { id: "child", delete: true },
          { id: "extra", name: "AddedInProcess", availability: `${T0}/${T3}` },
        ],
      },
      {
        method: "load",
        czml: [DOC, { id: "fresh", name: "AfterClear", availability: `${T0}/${T3}` }],
      },
    ],
    [T0],
  ],
  [
    "positions_frames_interpolation_extrapolation",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          {
            id: "const_carto_deg",
            position: { cartographicDegrees: [-105.0, 40.0, 1500.0] },
          },
          {
            id: "const_carto_rad",
            position: { cartographicRadians: [0.5, -0.25, 250.0] },
          },
          {
            id: "const_spherical",
            position: { spherical: [0.4, 1.1, 7000000.0] },
          },
          {
            id: "const_unit_spherical",
            position: { unitSpherical: [0.4, 1.1] },
          },
          {
            id: "const_inertial",
            position: { referenceFrame: "INERTIAL", cartesian: [7000000, 100000, -200000] },
          },
          {
            id: "sampled_linear_hold",
            position: {
              epoch: T0,
              interpolationAlgorithm: "LINEAR",
              interpolationDegree: 1,
              forwardExtrapolationType: "HOLD",
              forwardExtrapolationDuration: 15,
              backwardExtrapolationType: "EXTRAPOLATE",
              backwardExtrapolationDuration: 10,
              cartesian: [0, 1000, 2000, 3000, 10, 2000, 4000, 6000, 20, 3000, 6000, 9000],
            },
          },
          {
            id: "sampled_lagrange_iso",
            position: {
              interpolationAlgorithm: "LAGRANGE",
              interpolationDegree: 2,
              cartographicDegrees: [
                T0, -105.0, 40.0, 100.0,
                T1, -104.0, 41.0, 200.0,
                T2, -102.0, 42.5, 400.0,
              ],
            },
          },
          {
            id: "sampled_velocity_hermite",
            position: {
              epoch: T0,
              interpolationAlgorithm: "HERMITE",
              interpolationDegree: 2,
              cartesianVelocity: [
                0, 7000000, 0, 0, 0, 7500, 0,
                10, 7000000, 75000, 1000, -50, 7400, 100,
                20, 6999000, 148000, 2500, -100, 7300, 150,
              ],
            },
          },
          {
            id: "interval_pos",
            position: [
              { interval: `${T0}/${T1}`, cartesian: [100, 200, 300] },
              {
                interval: `${T1}/${T3}`,
                epoch: T1,
                cartesian: [0, 100, 200, 300, 20, 500, 600, 700],
              },
            ],
          },
        ],
      },
      {
        method: "process",
        czml: [
          {
            id: "interval_pos",
            position: { interval: `${T0}/${T1}`, delete: true },
          },
        ],
      },
    ],
    [T_BEFORE, T0, T_MID, T1, T2, T_AFTER, T_FAR],
  ],
  [
    "orientation_and_velocity_references",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          {
            id: "mover",
            position: {
              epoch: T0,
              cartesian: [
                0, 6378137, 0, 0,
                10, 6378137, 10000, 5000,
                20, 6378137, 25000, 12000,
              ],
            },
            orientation: { velocityReference: "#position" },
            viewFrom: { velocityReference: "#position" },
            billboard: {
              alignedAxis: { velocityReference: "#position" },
            },
          },
          {
            id: "quat_entity",
            orientation: {
              epoch: T0,
              interpolationAlgorithm: "LINEAR",
              unitQuaternion: [
                0, 0, 0, 0, 2,
                10, 0, 0, 0.7071067811865475, 0.7071067811865476,
              ],
            },
          },
        ],
      },
    ],
    [T0, T_MID, T1],
  ],
  [
    "references_intervals_and_deletes",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          {
            id: "p1",
            position: { cartesian: [1000, 2000, 3000] },
            point: { color: { rgba: [255, 128, 0, 255] } },
          },
          {
            id: "p2",
            position: { cartesian: [4000, 5000, 6000] },
          },
          {
            id: "follower",
            position: [
              { interval: `${T0}/${T1}`, reference: "p1#position" },
              { interval: `${T1}/${T2}`, reference: "p2#position" },
            ],
            point: {
              pixelSize: [
                { interval: `${T0}/${T1}`, number: 8 },
                { interval: `${T1}/${T2}`, number: 16 },
              ],
              color: { reference: "p1#point.color" },
            },
          },
          {
            id: "ref_line",
            polyline: {
              positions: {
                references: ["p1#position", "p2#position"],
              },
            },
          },
          {
            id: "sampled_delete_target",
            point: {
              pixelSize: {
                epoch: T0,
                number: [0, 10, 10, 20, 20, 30],
              },
            },
          },
        ],
      },
      {
        method: "process",
        czml: [
          {
            id: "follower",
            point: {
              pixelSize: { interval: `${T0}/${T1}`, delete: true },
              color: { delete: true },
            },
          },
          {
            id: "sampled_delete_target",
            point: {
              pixelSize: { interval: `${T1}/${T2}`, delete: true },
            },
          },
        ],
      },
    ],
    [T_MID, T1, "2020-01-01T00:00:15Z"],
  ],
  [
    "graphics_billboard_label_point",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          {
            id: "blp",
            position: { cartographicDegrees: [-105.0, 40.0, 0] },
            billboard: {
              show: true,
              image: { uri: "icons/pin.png" },
              scale: 1.5,
              pixelOffset: { cartesian2: [4, -8] },
              eyeOffset: { cartesian: [0, 0, -10] },
              horizontalOrigin: "LEFT",
              verticalOrigin: "BOTTOM",
              heightReference: "CLAMP_TO_GROUND",
              color: { rgba: [255, 200, 100, 204] },
              rotation: { epoch: T0, number: [0, 0.2, 10, 6.1] },
              alignedAxis: { unitCartesian: [0, 3, 4] },
              sizeInMeters: true,
              width: 32,
              height: 48,
              scaleByDistance: { nearFarScalar: [100, 2.0, 10000, 0.5] },
              translucencyByDistance: { nearFarScalar: [500, 1.0, 50000, 0.1] },
              pixelOffsetScaleByDistance: { nearFarScalar: [100, 1.0, 1000, 0.2] },
              imageSubRegion: { boundingRectangle: [10, 20, 30, 40] },
              distanceDisplayCondition: { distanceDisplayCondition: [10, 5000] },
              disableDepthTestDistance: 1000,
            },
            label: {
              show: true,
              text: "Hello CZML",
              font: "18px monospace",
              style: "FILL_AND_OUTLINE",
              scale: 1.25,
              showBackground: true,
              backgroundColor: { rgbaf: [0.1, 0.2, 0.3, 0.8] },
              backgroundPadding: { cartesian2: [7, 5] },
              pixelOffset: { cartesian2: [0, -12] },
              eyeOffset: { cartesian: [1, 2, 3] },
              horizontalOrigin: "RIGHT",
              verticalOrigin: "TOP",
              heightReference: "RELATIVE_TO_GROUND",
              fillColor: { rgba: [255, 255, 0, 255] },
              outlineColor: { rgba: [0, 0, 0, 255] },
              outlineWidth: 2,
              translucencyByDistance: { nearFarScalar: [10, 1, 1000, 0] },
              pixelOffsetScaleByDistance: { nearFarScalar: [10, 1, 1000, 0.5] },
              scaleByDistance: { nearFarScalar: [10, 1.5, 1000, 0.5] },
              distanceDisplayCondition: { distanceDisplayCondition: [0, 20000] },
              disableDepthTestDistance: 500,
            },
            point: {
              show: true,
              pixelSize: 12,
              heightReference: "NONE",
              color: { rgba: [0, 255, 128, 255] },
              outlineColor: { rgbaf: [1, 1, 1, 0.5] },
              outlineWidth: 3,
              scaleByDistance: { nearFarScalar: [100, 2, 5000, 0.5] },
              translucencyByDistance: { nearFarScalar: [100, 1, 5000, 0.2] },
              distanceDisplayCondition: { distanceDisplayCondition: [5, 8000] },
              disableDepthTestDistance: 250,
            },
          },
        ],
        options: { sourceUri: "https://cdn.example.test/assets/scene.czml" },
      },
    ],
    [T0, T_MID, T1],
  ],
  [
    "graphics_box_cylinder_ellipsoid",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          {
            id: "solids",
            position: { cartographicDegrees: [10, 20, 500] },
            box: {
              show: true,
              dimensions: { cartesian: [100, 200, 300] },
              heightReference: "RELATIVE_TO_GROUND",
              fill: true,
              material: { solidColor: { color: { rgba: [255, 0, 0, 128] } } },
              outline: true,
              outlineColor: { rgba: [0, 0, 0, 255] },
              outlineWidth: 2,
              shadows: "ENABLED",
              distanceDisplayCondition: { distanceDisplayCondition: [0, 100000] },
            },
            cylinder: {
              show: true,
              length: 500,
              topRadius: 50,
              bottomRadius: 120,
              heightReference: "NONE",
              fill: true,
              material: { solidColor: { color: { rgbaf: [0, 0.5, 1, 1] } } },
              outline: true,
              outlineColor: { rgba: [255, 255, 255, 255] },
              outlineWidth: 1.5,
              numberOfVerticalLines: 8,
              slices: 64,
              shadows: "CAST_ONLY",
              distanceDisplayCondition: { distanceDisplayCondition: [10, 50000] },
            },
            ellipsoid: {
              show: true,
              radii: { cartesian: [300, 200, 100] },
              innerRadii: { cartesian: [50, 40, 30] },
              minimumClock: 0.1,
              maximumClock: 3.1,
              minimumCone: 0.2,
              maximumCone: 2.5,
              heightReference: "NONE",
              fill: true,
              material: { solidColor: { color: { rgba: [0, 255, 0, 200] } } },
              outline: true,
              outlineColor: { rgba: [10, 20, 30, 255] },
              outlineWidth: 1,
              stackPartitions: 24,
              slicePartitions: 32,
              subdivisions: 64,
              shadows: "RECEIVE_ONLY",
              distanceDisplayCondition: { distanceDisplayCondition: [5, 25000] },
            },
          },
        ],
      },
    ],
    [T0],
  ],
  [
    "graphics_ellipse_rectangle_corridor_wall",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          {
            id: "surfaces",
            position: { cartographicDegrees: [-100, 35, 0] },
            ellipse: {
              show: true,
              semiMajorAxis: 5000,
              semiMinorAxis: 3000,
              height: 100,
              heightReference: "RELATIVE_TO_GROUND",
              extrudedHeight: 400,
              extrudedHeightReference: "NONE",
              rotation: 0.5,
              stRotation: 0.25,
              granularity: 0.02,
              fill: true,
              material: { solidColor: { color: { rgba: [200, 100, 50, 255] } } },
              outline: true,
              outlineColor: { rgba: [0, 0, 0, 255] },
              outlineWidth: 2,
              numberOfVerticalLines: 12,
              shadows: "DISABLED",
              distanceDisplayCondition: { distanceDisplayCondition: [0, 90000] },
              classificationType: "TERRAIN",
              zIndex: 3,
            },
            rectangle: {
              show: true,
              coordinates: { wsenDegrees: [-110, 30, -100, 40] },
              height: 50,
              heightReference: "NONE",
              extrudedHeight: 200,
              extrudedHeightReference: "NONE",
              rotation: 0.1,
              stRotation: 0.2,
              granularity: 0.01,
              fill: true,
              material: { solidColor: { color: { rgbaf: [0.2, 0.4, 0.6, 0.8] } } },
              outline: true,
              outlineColor: { rgba: [255, 255, 255, 255] },
              outlineWidth: 1,
              shadows: "ENABLED",
              distanceDisplayCondition: { distanceDisplayCondition: [100, 500000] },
              classificationType: "BOTH",
              zIndex: 5,
            },
            corridor: {
              show: true,
              positions: { cartographicDegrees: [-105, 40, 0, -104, 41, 0, -103, 40, 0] },
              width: 2500,
              height: 10,
              heightReference: "NONE",
              extrudedHeight: 150,
              extrudedHeightReference: "NONE",
              cornerType: "BEVELED",
              granularity: 0.03,
              fill: true,
              material: { solidColor: { color: { rgba: [0, 128, 255, 255] } } },
              outline: true,
              outlineColor: { rgba: [255, 255, 0, 255] },
              outlineWidth: 2,
              shadows: "DISABLED",
              distanceDisplayCondition: { distanceDisplayCondition: [0, 100000] },
              classificationType: "CESIUM_3D_TILE",
              zIndex: 1,
            },
            wall: {
              show: true,
              positions: { cartographicDegrees: [-105, 40, 1000, -104, 40, 1200] },
              minimumHeights: { array: [100, 200] },
              maximumHeights: { array: [1500, 1600] },
              granularity: 0.02,
              fill: true,
              material: { solidColor: { color: { rgba: [128, 0, 128, 255] } } },
              outline: true,
              outlineColor: { rgba: [0, 0, 0, 255] },
              outlineWidth: 1,
              shadows: "ENABLED",
              distanceDisplayCondition: { distanceDisplayCondition: [0, 200000] },
            },
          },
        ],
      },
    ],
    [T0],
  ],
  [
    "graphics_polygon_polyline_volume",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          { id: "v0", position: { cartographicDegrees: [-105, 40, 0] } },
          { id: "v1", position: { cartographicDegrees: [-104, 40, 0] } },
          { id: "v2", position: { cartographicDegrees: [-104, 41, 0] } },
          { id: "h0", position: { cartographicDegrees: [-104.6, 40.2, 0] } },
          { id: "h1", position: { cartographicDegrees: [-104.3, 40.2, 0] } },
          { id: "h2", position: { cartographicDegrees: [-104.3, 40.5, 0] } },
          {
            id: "poly",
            polygon: {
              show: true,
              positions: { cartographicDegrees: [-110, 30, 0, -100, 30, 0, -100, 40, 0, -110, 40, 0] },
              height: 100,
              heightReference: "NONE",
              extrudedHeight: 500,
              extrudedHeightReference: "NONE",
              stRotation: 0.3,
              granularity: 0.02,
              fill: true,
              material: { solidColor: { color: { rgba: [255, 100, 0, 200] } } },
              outline: true,
              outlineColor: { rgba: [0, 0, 0, 255] },
              outlineWidth: 2,
              perPositionHeight: false,
              closeTop: true,
              closeBottom: false,
              arcType: "RHUMB",
              shadows: "ENABLED",
              distanceDisplayCondition: { distanceDisplayCondition: [0, 500000] },
              classificationType: "TERRAIN",
              zIndex: 2,
            },
          },
          {
            id: "poly_refs",
            polygon: {
              positions: { references: ["v0#position", "v1#position", "v2#position"] },
              holes: { references: [["h0#position", "h1#position", "h2#position"]] },
            },
          },
          {
            id: "line_follow_false",
            polyline: {
              show: true,
              positions: { cartographicDegrees: [-105, 40, 100, -104, 41, 200] },
              width: 5,
              granularity: 0.01,
              material: { polylineOutline: { color: { rgba: [255, 0, 0, 255] }, outlineColor: { rgba: [0, 0, 0, 255] }, outlineWidth: 2 } },
              depthFailMaterial: { polylineDash: { color: { rgba: [255, 255, 0, 255] }, gapColor: { rgba: [0, 0, 0, 0] }, dashLength: 16, dashPattern: 255 } },
              followSurface: false,
              clampToGround: false,
              shadows: "DISABLED",
              distanceDisplayCondition: { distanceDisplayCondition: [0, 100000] },
              classificationType: "BOTH",
              zIndex: 4,
            },
          },
          {
            id: "line_follow_true",
            polyline: {
              positions: { cartographicDegrees: [-105, 40, 0, -104, 41, 0] },
              followSurface: true,
            },
          },
          {
            id: "volume",
            polylineVolume: {
              show: true,
              positions: { cartographicDegrees: [-105, 40, 0, -104, 41, 0] },
              shape: { cartesian2: [-50, -50, 50, -50, 50, 50, -50, 50] },
              cornerType: "MITERED",
              fill: true,
              material: { solidColor: { color: { rgba: [0, 255, 255, 255] } } },
              outline: true,
              outlineColor: { rgba: [0, 0, 0, 255] },
              outlineWidth: 1,
              granularity: 0.02,
              shadows: "CAST_ONLY",
              distanceDisplayCondition: { distanceDisplayCondition: [0, 80000] },
            },
          },
        ],
      },
      {
        method: "process",
        czml: [
          {
            id: "poly",
            polygon: {
              holes: {
                cartographicDegrees: [[-108, 32, 0, -102, 32, 0, -102, 38, 0, -108, 38, 0]],
              },
            },
          },
        ],
      },
    ],
    [T0],
  ],
  [
    "graphics_model_path_tileset",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          {
            id: "aircraft",
            position: { cartographicDegrees: [-105, 40, 5000] },
            model: {
              show: true,
              gltf: "models/plane.glb",
              scale: 2.0,
              minimumPixelSize: 64,
              maximumScale: 500,
              incrementallyLoadTextures: false,
              runAnimations: true,
              clampAnimations: false,
              shadows: "ENABLED",
              heightReference: "NONE",
              silhouetteColor: { rgba: [0, 255, 0, 255] },
              silhouetteSize: 2.5,
              color: { rgba: [255, 255, 255, 255] },
              colorBlendMode: "MIX",
              colorBlendAmount: 0.4,
              distanceDisplayCondition: { distanceDisplayCondition: [0, 100000] },
              nodeTransformations: {
                Propeller: {
                  translation: { cartesian: [1, 2, 3] },
                  rotation: { unitQuaternion: [0, 0, 0, 1] },
                  scale: { cartesian: [1, 1, 2] },
                },
              },
              articulations: {
                "Fairing Open": { epoch: T0, number: [0, 0, 10, 90] },
                "Solar Deploy": 45,
              },
            },
            path: {
              show: true,
              leadTime: 120,
              trailTime: 300,
              width: 3,
              resolution: 5,
              material: { polylineGlow: { color: { rgba: [255, 255, 0, 255] }, glowPower: 0.3, taperPower: 0.8 } },
              distanceDisplayCondition: { distanceDisplayCondition: [0, 500000] },
            },
            tileset: {
              show: true,
              uri: "tiles/tileset.json",
              maximumScreenSpaceError: 8,
            },
          },
        ],
        options: { sourceUri: "https://example.test/sim/flight.czml" },
      },
    ],
    [T0, T_MID, T1],
  ],
  [
    "materials_all_and_composite",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          {
            id: "m_image",
            rectangle: {
              coordinates: { wsenDegrees: [0, 0, 1, 1] },
              material: {
                image: {
                  image: { uri: "tex/brick.png" },
                  repeat: { cartesian2: [4, 2] },
                  color: { rgbaf: [1, 0.8, 0.6, 1] },
                  transparent: true,
                },
              },
            },
          },
          {
            id: "m_grid",
            rectangle: {
              coordinates: { wsenDegrees: [1, 0, 2, 1] },
              material: {
                grid: {
                  color: { rgba: [0, 255, 0, 255] },
                  cellAlpha: 0.25,
                  lineCount: { cartesian2: [10, 12] },
                  lineThickness: { cartesian2: [2, 3] },
                  lineOffset: { cartesian2: [0.1, 0.2] },
                },
              },
            },
          },
          {
            id: "m_stripe",
            rectangle: {
              coordinates: { wsenDegrees: [2, 0, 3, 1] },
              material: {
                stripe: {
                  orientation: "VERTICAL",
                  evenColor: { rgba: [255, 255, 255, 255] },
                  oddColor: { rgba: [0, 0, 255, 255] },
                  offset: 0.5,
                  repeat: 6,
                },
              },
            },
          },
          {
            id: "m_checker",
            rectangle: {
              coordinates: { wsenDegrees: [3, 0, 4, 1] },
              material: {
                checkerboard: {
                  evenColor: { rgba: [255, 0, 0, 255] },
                  oddColor: { rgba: [0, 0, 0, 255] },
                  repeat: { cartesian2: [8, 8] },
                },
              },
            },
          },
          {
            id: "m_arrow",
            polyline: {
              positions: { cartographicDegrees: [0, 0, 0, 1, 1, 0] },
              material: { polylineArrow: { color: { rgba: [255, 128, 0, 255] } } },
            },
          },
          {
            id: "m_composite",
            box: {
              dimensions: { cartesian: [10, 10, 10] },
              material: [
                {
                  interval: `${T0}/${T1}`,
                  solidColor: { color: { rgba: [255, 0, 0, 255] } },
                },
                {
                  interval: `${T1}/${T2}`,
                  grid: { color: { rgba: [0, 255, 255, 255] }, cellAlpha: 0.5 },
                },
              ],
            },
          },
        ],
        options: { sourceUri: "https://example.test/scenes/mat.czml" },
      },
      {
        method: "process",
        czml: [
          {
            id: "m_composite",
            box: {
              material: {
                interval: `${T0}/${T1}`,
                solidColor: { color: { rgba: [0, 255, 0, 255] } },
              },
            },
          },
        ],
      },
    ],
    [T_MID, "2020-01-01T00:00:15Z"],
  ],
  [
    "properties_bag",
    null,
    [
      {
        method: "load",
        czml: [
          DOC,
          { id: "src", properties: { shared: 99 } },
          {
            id: "bag_entity",
            properties: {
              flag: true,
              count: 42,
              label: "alpha",
              sampledNum: { epoch: T0, number: [0, 10, 10, 30] },
              rgbaCol: { rgba: [10, 20, 30, 255] },
              rgbafCol: { rgbaf: [0.2, 0.4, 0.6, 0.8] },
              c2: { cartesian2: [3, 4] },
              c3: { cartesian: [1, 2, 3] },
              deg: { cartographicDegrees: [-105, 40, 100] },
              unitVec: { unitCartesian: [0, 3, 4] },
              quat: { unitQuaternion: [0, 0, 0, 2] },
              rect: { wsenDegrees: [-10, -5, 10, 5] },
              brect: { boundingRectangle: [1, 2, 3, 4] },
              nfs: { nearFarScalar: [1, 2, 3, 4] },
              ddc: { distanceDisplayCondition: [5, 50] },
              when: { date: T1 },
              link: { uri: "info/doc.txt" },
              list: { array: [1, "two", true] },
              obj: { object: { nested: "ok", n: 7 } },
              refProp: { reference: "src#properties.shared" },
              arc: { arcType: "RHUMB" },
              ivStr: [
                { interval: `${T0}/${T1}`, string: "first" },
                { interval: `${T1}/${T2}`, string: "second" },
              ],
            },
          },
        ],
        options: { sourceUri: "https://example.test/data/bag.czml" },
      },
    ],
    [T_MID, "2020-01-01T00:00:15Z"],
  ],
  [
    "errors_invalid_czml",
    null,
    [
      { method: "load", czml: [{ id: "no_doc_first" }] },
      { method: "load", czml: [{ id: "document" }] },
      { method: "load", czml: [{ id: "document", version: "2.0" }] },
      { method: "load", czml: [{ id: "document", version: "1.0.0" }] },
      { method: "load", czml: [DOC, { id: "bad_pos", position: {} }] },
      { method: "load", czml: [DOC, { id: "bad_vel", description: { velocityReference: "x#position" } }] },
    ],
    [T0],
  ],
];

export default async function dataSourcesCzml() {
  const suite = { module: "CzmlDataSource", cases: [] };
  for (const [name, initialName, ops, sampleTimes] of SCENARIOS) {
    const r = await runScenario(initialName, ops, sampleTimes);
    suite.cases.push({
      f: name,
      a: [initialName, ops, sampleTimes],
      r,
    });
  }
  return [suite];
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const here = dirname(fileURLToPath(import.meta.url));
  const out = join(here, "../../../tests/fixtures/data_sources/CzmlDataSource.json");
  const version = JSON.parse(readFileSync(join(here, "../node_modules/@cesium/engine/package.json"), "utf8")).version;
  const [suite] = await dataSourcesCzml();
  mkdirSync(dirname(out), { recursive: true });
  writeFileSync(out, JSON.stringify({ module: suite.module, cesium: version, cases: suite.cases }) + "\n");
  console.log(`data_sources/${suite.module}: ${suite.cases.length} cases`);
}
