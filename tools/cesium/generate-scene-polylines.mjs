// Direct CPU reference cases from the pinned CesiumJS engine.
globalThis.HTMLCanvasElement = class HTMLCanvasElement {};
globalThis.HTMLImageElement = class HTMLImageElement {};
globalThis.HTMLVideoElement = class HTMLVideoElement {};
globalThis.ImageData = class ImageData {};
globalThis.ImageBitmap = class ImageBitmap {};
globalThis.OffscreenCanvas = class OffscreenCanvas {};

import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import PolylineCollection from '@cesium/engine/Source/Scene/PolylineCollection.js';
import Polyline from '@cesium/engine/Source/Scene/Polyline.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Matrix4 from '@cesium/engine/Source/Core/Matrix4.js';
import DistanceDisplayCondition from '@cesium/engine/Source/Core/DistanceDisplayCondition.js';
import PolylinePipeline from '@cesium/engine/Source/Core/PolylinePipeline.js';

const here = dirname(fileURLToPath(import.meta.url));

const snapshot = p => ({
  show: p.show,
  width: p.width,
  loop: p.loop,
  materialType: p.material.type,
  positions: p.positions.map(pos => [pos.x, pos.y, pos.z]),
  actualPositions: p._actualPositions.map(pos => [pos.x, pos.y, pos.z]),
  distanceDisplayCondition: p.distanceDisplayCondition ? [p.distanceDisplayCondition.near, p.distanceDisplayCondition.far] : null,
  id: p.id ?? null,
  boundingVolume: {
    center: [p._boundingVolume.center.x, p._boundingVolume.center.y, p._boundingVolume.center.z],
    radius: p._boundingVolume.radius,
  },
  boundingVolumeWC: {
    center: [p._boundingVolumeWC.center.x, p._boundingVolumeWC.center.y, p._boundingVolumeWC.center.z],
    radius: p._boundingVolumeWC.radius,
  },
  segmentLengths: Array.from(p._segments.lengths),
  segmentPositionsCount: p._segments.positions.length,
});

const c = new PolylineCollection();
const pDefaults = c.add();

const customPositions = [
  new Cartesian3(1000, 2000, 3000),
  new Cartesian3(1000, 2000, 3000), // duplicate
  new Cartesian3(4000, 5000, 6000),
  new Cartesian3(7000, 8000, 9000),
];

const pCustom = c.add({
  positions: customPositions,
  width: 4.5,
  show: true,
  loop: true,
  id: 'custom-polyline-1',
  distanceDisplayCondition: new DistanceDisplayCondition(12.0, 850.0),
});

const initialLength = c.length;
const containsBefore = c.contains(pCustom);

// Test model matrix transform
const translation = Matrix4.fromTranslation(new Cartesian3(25, 50, 75));
c.modelMatrix = translation;
pCustom.update();

const transformedSnapshot = snapshot(pCustom);

// Test loop toggling
pCustom.loop = false;
const loopFalseSnapshot = snapshot(pCustom);

pCustom.loop = true;
const loopTrueAgainSnapshot = snapshot(pCustom);

// Test positions mutation
pCustom.positions = [
  new Cartesian3(10, 20, 30),
  new Cartesian3(40, 50, 60),
];
const mutatedPositionsSnapshot = snapshot(pCustom);

// Test antimeridian polyline wrapping
const antimeridianPositions = [
  Cartesian3.fromDegrees(179, 0),
  Cartesian3.fromDegrees(-179, 0),
];
const pAntimeridian = c.add({
  positions: antimeridianPositions,
  width: 2.0,
});
const antimeridianSnapshot = snapshot(pAntimeridian);

// Removal
const firstRemoval = c.remove(pCustom);
const secondRemoval = c.remove(pCustom);
const containsAfter = c.contains(pCustom);
const lengthAfter = c.length;

const output = {
  defaults: snapshot(pDefaults),
  transformed: transformedSnapshot,
  loopFalse: loopFalseSnapshot,
  loopTrueAgain: loopTrueAgainSnapshot,
  mutatedPositions: mutatedPositionsSnapshot,
  antimeridian: antimeridianSnapshot,
  initialLength,
  containsBefore,
  firstRemoval,
  secondRemoval,
  containsAfter,
  lengthAfter,
};

mkdirSync(join(here, '../../tests/data'), { recursive: true });
const target = join(here, '../../tests/data/scene-polylines.json');
writeFileSync(target, JSON.stringify(output, null, 2) + '\n');
console.log('Wrote', target);
