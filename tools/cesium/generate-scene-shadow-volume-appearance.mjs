// Direct ShadowVolumeAppearance reference cases from the pinned CesiumJS
// engine: color/pick fragment and vertex ShaderSources for per-instance-color
// and material appearances, first-projection caching of the extent defines,
// and the static planar/spherical texture-coordinate attribute helpers.
import * as C from '@cesium/engine';
import ShadowVolumeAppearance from '@cesium/engine/Source/Scene/ShadowVolumeAppearance.js';
import ShadowVolumeAppearanceFS from '@cesium/engine/Source/Shaders/ShadowVolumeAppearanceFS.js';
import { writeFileSync } from 'node:fs';

// Material's uniform typing tests these browser classes with instanceof.
for (const name of ['HTMLCanvasElement', 'HTMLImageElement', 'HTMLVideoElement', 'ImageBitmap', 'OffscreenCanvas']) {
  globalThis[name] ??= class {};
}

const num = (v) => (Number.isNaN(v) ? 'NaN' : v === Infinity ? 'Infinity' : v === -Infinity ? '-Infinity' : Object.is(v, -0) ? '-0' : v);
const nums = (v) => Array.from(v, num);

// Sources are recorded with the shared fragment stage replaced by a marker so
// the fixture stays small; the Rust test substitutes its pinned copy.
const FS = '<ShadowVolumeAppearanceFS>';
const shader = (s) => ({
  defines: s.defines,
  sources: s.sources.map((source) => (source === ShadowVolumeAppearanceFS ? FS : source)),
  pickColorQualifier: s.pickColorQualifier ?? null,
});

const fabric = (source) => new C.Material({ fabric: { source } });
const appearances = [
  { name: 'perInstance', make: (flat) => new C.PerInstanceColorAppearance({ flat }) },
  { name: 'color', make: (flat) => new C.MaterialAppearance({ flat, material: C.Material.fromType('Color') }) },
  { name: 'grid', make: (flat) => new C.MaterialAppearance({ flat, material: C.Material.fromType('Grid') }) },
  { name: 'checkerboard', make: (flat) => new C.MaterialAppearance({ flat, material: C.Material.fromType('Checkerboard') }) },
  { name: 'normalEC', make: (flat) => new C.MaterialAppearance({ flat, material: fabric('czm_material czm_getMaterial(czm_materialInput materialInput) { czm_material m; m.diffuse = materialInput.normalEC; return m; }') }) },
  { name: 'positionToEyeEC', make: (flat) => new C.MaterialAppearance({ flat, material: fabric('czm_material czm_getMaterial(czm_materialInput materialInput) { czm_material m; m.diffuse = materialInput.positionToEyeEC; m.alpha = 1.0; m.specular = 0.0; m.shininess = 1.0; m.normal = vec3(0.0); m.emission = vec3(0.0); return m; }') }) },
  { name: 'tangentToEyeMatrix', make: (flat) => new C.MaterialAppearance({ flat, material: fabric('czm_material czm_getMaterial(czm_materialInput materialInput) { czm_material m; m.normal = materialInput.tangentToEyeMatrix * vec3(0.0, 0.0, 1.0); m.diffuse = vec3(1.0); m.alpha = 1.0; m.specular = 0.0; m.shininess = 1.0; m.emission = vec3(0.0); return m; }') }) },
  { name: 'st', make: (flat) => new C.MaterialAppearance({ flat, material: fabric('czm_material czm_getMaterial(czm_materialInput materialInput) { czm_material m; m.diffuse = vec3(materialInput.st, 0.0); m.alpha = 1.0; m.specular = 0.0; m.shininess = 1.0; m.normal = vec3(0.0); m.emission = vec3(0.0); return m; }') }) },
  { name: 'customFragment', make: (flat) => new C.MaterialAppearance({ flat, material: fabric('czm_material czm_getMaterial(czm_materialInput i) { czm_material m; m.diffuse = vec3(1.0); m.alpha = 1.0; m.specular = 0.0; m.shininess = 1.0; m.normal = vec3(0.0); m.emission = vec3(0.0); return m; }'), fragmentShaderSource: 'void main() { czm_materialInput materialInput; materialInput.st = vec2(0.0); materialInput.positionToEyeEC = vec3(0.0); }' }) },
];

const shaders = [];
for (const spec of appearances) {
  const cases = [];
  for (let mask = 0; mask < 16; mask++) {
    const cull = !!(mask & 1), planar = !!(mask & 2), flat = !!(mask & 4), cv = !!(mask & 8);
    const appearance = new ShadowVolumeAppearance(cull, planar, spec.make(flat));
    const projection = new C.GeographicProjection();
    cases.push({
      mask,
      color: shader(appearance.createFragmentShader(cv)),
      pick: shader(appearance.createPickFragmentShader(cv)),
      vertex: shader(appearance.createVertexShader(['EXTERNAL'], 'void main() {}', cv, projection)),
      pickVertex: shader(appearance.createPickVertexShader(['EXTERNAL'], 'void main() {}', cv, projection)),
    });
  }
  shaders.push({ name: spec.name, cases });
}

// The projection-extent defines are computed on the first vertex-shader call,
// even when it is not for 2D/Columbus View, and reused for every later call.
const caching = (() => {
  const appearance = new ShadowVolumeAppearance(true, false, new C.PerInstanceColorAppearance());
  const small = new C.Ellipsoid(1000.5, 1000.5, 900);
  const first = appearance.createVertexShader([], 'void main() {}', false, new C.WebMercatorProjection(small));
  const second = appearance.createPickVertexShader([], 'void main() {}', true, new C.GeographicProjection());
  const fresh = new ShadowVolumeAppearance(true, false, new C.PerInstanceColorAppearance());
  const direct = fresh.createVertexShader([], 'void main() {}', true, new C.WebMercatorProjection(small));
  return { ellipsoid: [1000.5, 1000.5, 900], first: first.defines, second: second.defines, direct: direct.defines };
})();

// --- Texture-coordinate attributes -----------------------------------------
const attribute = (a) => ({
  componentDatatype: a.componentDatatype,
  componentsPerAttribute: a.componentsPerAttribute,
  normalize: a.normalize,
  value: nums(a.value),
});
const encodeAttributes = (attributes) => Object.entries(attributes).map(([name, a]) => [name, attribute(a)]);

const rectangles = [
  ['tiny', [-0.1, -0.1, 0.1, 0.1]],
  ['small', [10, 20, 10.5, 20.4]],
  ['wide', [-10, -5, 30, 40]],
  ['antimeridian', [179.5, -1, -179.5, 1]],
  ['wideAntimeridian', [170, -20, -170, 20]],
  ['polar', [-180, 80, 180, 90]],
  ['southern', [100, -60, 101, -59.5]],
  ['exactlyOneDegree', [0, 0, 1, 0.5]],
];
const rotations = [
  ['identity', [0, 0, 0, 1, 1, 0]],
  ['rotated', [0.1, 0.2, -0.3, 1.1, 0.9, 0.4]],
  ['degenerate', [0, 0, 0, 0, 1, 0]],
];
const ellipsoids = [
  ['wgs84', C.Ellipsoid.WGS84],
  ['sphere', new C.Ellipsoid(6371000, 6371000, 6371000)],
];
const projections = [
  ['geographic', (e) => new C.GeographicProjection(e)],
  ['webMercator', (e) => new C.WebMercatorProjection(e)],
];

const attributes = [];
let index = 0;
for (const [rectangleName, degrees] of rectangles) {
  const rectangle = C.Rectangle.fromDegrees(...degrees);
  // Cycle the other inputs so every rectangle meets several combinations.
  for (let k = 0; k < 3; k++, index++) {
    const [rotationName, rotation] = rotations[(index + k) % rotations.length];
    const [ellipsoidName, ellipsoid] = ellipsoids[index % ellipsoids.length];
    const [projectionName, makeProjection] = projections[(index + k) % projections.length];
    const height = k === 2 ? 1000 : k === 1 ? -25.5 : undefined;
    const projection = makeProjection(ellipsoid);
    const planar = ShadowVolumeAppearance.getPlanarTextureCoordinateAttributes(rectangle, rotation, ellipsoid, projection, height);
    const spherical = ShadowVolumeAppearance.getSphericalExtentGeometryInstanceAttributes(rectangle, rotation, ellipsoid, projection);
    attributes.push({
      name: `${rectangleName}/${rotationName}/${ellipsoidName}/${projectionName}/${height ?? 'default'}`,
      rectangle: nums([rectangle.west, rectangle.south, rectangle.east, rectangle.north]),
      rotation,
      ellipsoid: nums([ellipsoid.radii.x, ellipsoid.radii.y, ellipsoid.radii.z]),
      projection: projectionName,
      height: height ?? null,
      shouldUseSpherical: ShadowVolumeAppearance.shouldUseSphericalCoordinates(rectangle),
      planar: encodeAttributes(planar),
      spherical: encodeAttributes(spherical),
      planarHasPlanes: ShadowVolumeAppearance.hasAttributesForTextureCoordinatePlanes(planar),
      planarHasSpherical: ShadowVolumeAppearance.hasAttributesForSphericalExtents(planar),
      sphericalHasPlanes: ShadowVolumeAppearance.hasAttributesForTextureCoordinatePlanes(spherical),
      sphericalHasSpherical: ShadowVolumeAppearance.hasAttributesForSphericalExtents(spherical),
    });
  }
}

writeFileSync(
  '../../tests/data/scene-shadow-volume-appearance.json',
  JSON.stringify({ maxWidthForPlanarExtents: ShadowVolumeAppearance.MAX_WIDTH_FOR_PLANAR_EXTENTS, shaders, caching, attributes }) + '\n',
);
