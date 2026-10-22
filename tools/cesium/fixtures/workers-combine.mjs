// combineGeometry through the real worker: packed createGeometry results,
// packed instances, and the packed bounding-sphere buffers the worker posts.
import * as C from "@cesium/engine";
import PrimitivePipeline from "@cesium/engine/Source/Scene/PrimitivePipeline.js";
import { loadWorker, invoke } from "./workers-terrain.mjs";

const num = (v) => (Number.isNaN(v) ? "NaN" : v === Infinity ? "Infinity" : v === -Infinity ? "-Infinity" : Object.is(v, -0) ? "-0" : v);
const nums = (v) => Array.from(v, num);

function encGeometry(geometry) {
  if (geometry == null) return null;
  const attributes = {};
  for (const name of Object.keys(geometry.attributes)) {
    const attribute = geometry.attributes[name];
    if (attribute == null) continue;
    attributes[name] = [
      attribute.componentDatatype,
      attribute.componentsPerAttribute,
      !!attribute.normalize,
      attribute.values.constructor.name,
      nums(attribute.values),
    ];
  }
  return {
    attributes,
    indices: geometry.indices ? [geometry.indices.constructor.name, nums(geometry.indices)] : null,
    primitiveType: geometry.primitiveType,
    boundingSphere: geometry.boundingSphere
      ? [geometry.boundingSphere.center.x, geometry.boundingSphere.center.y, geometry.boundingSphere.center.z, geometry.boundingSphere.radius].map(num)
      : null,
    boundingSphereCV: geometry.boundingSphereCV
      ? [geometry.boundingSphereCV.center.x, geometry.boundingSphereCV.center.y, geometry.boundingSphereCV.center.z, geometry.boundingSphereCV.radius].map(num)
      : null,
    geometryType: geometry.geometryType,
    offsetAttribute: geometry.offsetAttribute ?? null,
  };
}

function encResult(result) {
  return {
    geometries: result.geometries === undefined ? null : result.geometries.map(encGeometry),
    attributeLocations: result.attributeLocations ?? null,
    modelMatrix: nums(result.modelMatrix),
    pickOffsets: result.pickOffsets ?? null,
    offsetInstanceExtend: result.offsetInstanceExtend === undefined ? null : result.offsetInstanceExtend.map((v) => v ?? null),
    boundingSpheres: nums(result.boundingSpheres),
    boundingSpheresCV: nums(result.boundingSpheresCV),
  };
}

function box(dimensions, format) {
  return C.BoxGeometry.createGeometry(
    C.BoxGeometry.fromDimensions({
      vertexFormat: format ?? C.VertexFormat.POSITION_ONLY,
      dimensions: new C.Cartesian3(dimensions[0], dimensions[1], dimensions[2]),
    }),
  );
}

function enu(lon, lat, height = 0) {
  return C.Transforms.eastNorthUpToFixedFrame(C.Cartesian3.fromDegrees(lon, lat, height));
}

export default async function workersCombine() {
  const worker = await loadWorker("combineGeometry");
  const cases = [];
  const add = async (geometries, options = {}) => {
    const ellipsoid = options.ellipsoid ?? C.Ellipsoid.WGS84;
    const projection = options.webMercator ? new C.WebMercatorProjection(ellipsoid) : new C.GeographicProjection(ellipsoid);
    const groups = options.groups ?? [geometries];
    const instances = geometries.map((_, i) => {
      const instance = new C.GeometryInstance({
        geometry: geometries[i] ?? box([1, 1, 1]),
        modelMatrix: options.matrices?.[i] ?? C.Matrix4.IDENTITY,
      });
      if (options.offsets?.[i]) {
        const [x, y, z] = options.offsets[i];
        instance.attributes = { offset: new C.OffsetGeometryInstanceAttribute(x, y, z) };
      }
      return instance;
    });
    const parameters = {
      createGeometryResults: groups.map((group) => PrimitivePipeline.packCreateGeometryResults(group, [])),
      instances,
      ellipsoid,
      projection,
      elementIndexUintSupported: options.elementIndexUintSupported ?? true,
      scene3DOnly: options.scene3DOnly ?? true,
      vertexCacheOptimize: options.vertexCacheOptimize ?? false,
      compressVertices: options.compressVertices ?? false,
      modelMatrix: options.modelMatrix ?? C.Matrix4.IDENTITY,
      createPickOffsets: options.createPickOffsets ?? false,
    };
    const packed = PrimitivePipeline.packCombineGeometryParameters(parameters, []);
    const script = {
      createGeometryResults: packed.createGeometryResults.map((result) => ({
        stringTable: result.stringTable,
        packedData: nums(result.packedData),
      })),
      packedInstances: nums(packed.packedInstances),
      ellipsoid: [ellipsoid.radii.x, ellipsoid.radii.y, ellipsoid.radii.z].map(num),
      isGeographic: packed.isGeographic,
      elementIndexUintSupported: parameters.elementIndexUintSupported,
      scene3DOnly: parameters.scene3DOnly,
      vertexCacheOptimize: parameters.vertexCacheOptimize,
      compressVertices: parameters.compressVertices,
      modelMatrix: nums(parameters.modelMatrix),
      createPickOffsets: parameters.createPickOffsets,
    };
    let r;
    try {
      r = encResult((await invoke(worker, packed)).result);
    } catch (error) {
      r = { throws: String(error.message ?? error), kind: error.name ?? "Error" };
    }
    cases.push({ f: "combineGeometry", a: [script], r });
  };

  const unit = box([1, 1, 1]);
  const wide = box([400000, 1000, 1000]);
  const detailed = box([20, 30, 40], C.VertexFormat.ALL);
  await add([unit]);
  await add([unit, unit], { matrices: [enu(10, 20, 100), enu(-30, 5, 50)] });
  await add([unit, unit], { matrices: [enu(10, 20, 100), enu(10, 20, 100)], modelMatrix: C.Matrix4.fromTranslation(new C.Cartesian3(5, 0, 0)) });
  await add([unit], { scene3DOnly: false, matrices: [enu(0, 0, 0)] });
  await add([unit], { scene3DOnly: false, webMercator: true, matrices: [enu(20, 10, 0)] });
  await add([wide], { scene3DOnly: false, matrices: [enu(179.5, 0, 0)] });
  await add([unit], { vertexCacheOptimize: true });
  await add([detailed], { compressVertices: true, matrices: [enu(1, 2, 0)] });
  await add([unit, unit], { createPickOffsets: true, matrices: [C.Matrix4.IDENTITY, C.Matrix4.fromTranslation(new C.Cartesian3(3, 0, 0))] });
  const tagged = box([2, 2, 2]);
  tagged.offsetAttribute = C.GeometryOffsetAttribute.ALL;
  await add([tagged, unit], { offsets: [[1, 2, 3], null], matrices: [enu(4, 5, 0), enu(6, 7, 0)] });
  await add([unit, undefined]);
  await add([], { groups: [] });
  await add([unit], { elementIndexUintSupported: false });
  const smallEllipsoid = new C.Ellipsoid(1000, 1000, 500);
  await add([box([10, 10, 10])], {
    ellipsoid: smallEllipsoid,
    scene3DOnly: false,
    matrices: [C.Matrix4.fromTranslation(new C.Cartesian3(1000, 0, 0))],
  });
  const lines = C.SimplePolylineGeometry.createGeometry(new C.SimplePolylineGeometry({
    positions: C.Cartesian3.fromDegreesArrayHeights([0, 0, 0, 1, 1, 0]),
  }));
  await add([unit, lines]);

  return [{ module: "combineGeometry", cases }];
}
