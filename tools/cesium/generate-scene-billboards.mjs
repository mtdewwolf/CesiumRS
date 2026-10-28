// Billboard snapshots use the pinned engine. Bounding and packing expectations below are computed separately; they do not capture GPU vertex writers.
globalThis.HTMLCanvasElement = class HTMLCanvasElement {};
globalThis.HTMLImageElement = class HTMLImageElement {};
globalThis.HTMLVideoElement = class HTMLVideoElement {};
globalThis.ImageData = class ImageData {};
globalThis.ImageBitmap = class ImageBitmap {};
globalThis.OffscreenCanvas = class OffscreenCanvas {};

import { writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import BillboardCollection from '@cesium/engine/Source/Scene/BillboardCollection.js';
import Billboard from '@cesium/engine/Source/Scene/Billboard.js';
import Cartesian2 from '@cesium/engine/Source/Core/Cartesian2.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Color from '@cesium/engine/Source/Core/Color.js';
import NearFarScalar from '@cesium/engine/Source/Core/NearFarScalar.js';
import DistanceDisplayCondition from '@cesium/engine/Source/Core/DistanceDisplayCondition.js';
import HeightReference from '@cesium/engine/Source/Scene/HeightReference.js';
import HorizontalOrigin from '@cesium/engine/Source/Scene/HorizontalOrigin.js';
import VerticalOrigin from '@cesium/engine/Source/Scene/VerticalOrigin.js';
import SplitDirection from '@cesium/engine/Source/Scene/SplitDirection.js';
import BoundingSphere from '@cesium/engine/Source/Core/BoundingSphere.js';
import Matrix4 from '@cesium/engine/Source/Core/Matrix4.js';
import EncodedCartesian3 from '@cesium/engine/Source/Core/EncodedCartesian3.js';
import AttributeCompression from '@cesium/engine/Source/Core/AttributeCompression.js';
import CesiumMath from '@cesium/engine/Source/Core/Math.js';

const here = dirname(fileURLToPath(import.meta.url));

const snapshot = (b) => {
  const scalar = (s) => (s ? [s.near, s.nearValue, s.far, s.farValue] : null);
  return {
    show: b.show,
    position: [b.position.x, b.position.y, b.position.z],
    pixelOffset: [b.pixelOffset.x, b.pixelOffset.y],
    eyeOffset: [b.eyeOffset.x, b.eyeOffset.y, b.eyeOffset.z],
    horizontalOrigin: b.horizontalOrigin,
    verticalOrigin: b.verticalOrigin,
    heightReference: b.heightReference,
    scale: b.scale,
    color: [b.color.red, b.color.green, b.color.blue, b.color.alpha],
    rotation: b.rotation,
    alignedAxis: [b.alignedAxis.x, b.alignedAxis.y, b.alignedAxis.z],
    width: b.width ?? null,
    height: b.height ?? null,
    scaleByDistance: scalar(b.scaleByDistance),
    translucencyByDistance: scalar(b.translucencyByDistance),
    pixelOffsetScaleByDistance: scalar(b.pixelOffsetScaleByDistance),
    sizeInMeters: b.sizeInMeters,
    distanceDisplayCondition: b.distanceDisplayCondition
      ? [b.distanceDisplayCondition.near, b.distanceDisplayCondition.far]
      : null,
    disableDepthTestDistance: b.disableDepthTestDistance ?? null,
    id: b.id ?? null,
    image: b.image ?? null,
    ready: b.ready,
    clusterShow: b.clusterShow,
    outlineColor: [b.outlineColor.red, b.outlineColor.green, b.outlineColor.blue, b.outlineColor.alpha],
    outlineWidth: b.outlineWidth,
    splitDirection: b.splitDirection,
  };
};

const c = new BillboardCollection();
const a = c.add();
const b = c.add({
  position: new Cartesian3(131075, -65538, 3.5),
  show: true,
  scale: 2.5,
  pixelOffset: new Cartesian2(12, -24),
  eyeOffset: new Cartesian3(5, 10, -50),
  horizontalOrigin: HorizontalOrigin.RIGHT,
  verticalOrigin: VerticalOrigin.BOTTOM,
  heightReference: HeightReference.NONE,
  color: new Color(0.25, 0.5, 0.75, 0.8),
  rotation: 0.7853981633974483,
  alignedAxis: new Cartesian3(0, 1, 0),
  width: 64,
  height: 32,
  scaleByDistance: new NearFarScalar(100, 2.0, 1000, 0.5),
  translucencyByDistance: new NearFarScalar(50, 1.0, 500, 0.2),
  pixelOffsetScaleByDistance: new NearFarScalar(20, 1.5, 200, 0.5),
  sizeInMeters: true,
  distanceDisplayCondition: new DistanceDisplayCondition(15, 2500),
  disableDepthTestDistance: 100,
  splitDirection: SplitDirection.LEFT,
  id: 'test-billboard-custom',
  outlineColor: new Color(1, 0, 0, 1),
  outlineWidth: 2.0,
});

// Bounding box test cases
const bboxCases = [
  {
    name: 'custom_right_bottom',
    horizontalOrigin: HorizontalOrigin.RIGHT,
    verticalOrigin: VerticalOrigin.BOTTOM,
    width: 64,
    height: 32,
    scale: 2.5,
    center: [200, 300],
    expected: (() => {
      const rect = Billboard.getScreenSpaceBoundingBox(b, new Cartesian2(200, 300));
      return [rect.x, rect.y, rect.width, rect.height];
    })(),
  },
  {
    name: 'center_center',
    horizontalOrigin: HorizontalOrigin.CENTER,
    verticalOrigin: VerticalOrigin.CENTER,
    width: 100,
    height: 50,
    scale: 2.0,
    center: [500, 500],
    expected: (() => {
      const rect = Billboard.getScreenSpaceBoundingBox(
        { width: 100, height: 50, scale: 2.0, horizontalOrigin: HorizontalOrigin.CENTER, verticalOrigin: VerticalOrigin.CENTER },
        new Cartesian2(500, 500)
      );
      return [rect.x, rect.y, rect.width, rect.height];
    })(),
  },
  {
    name: 'left_top',
    horizontalOrigin: HorizontalOrigin.LEFT,
    verticalOrigin: VerticalOrigin.TOP,
    width: 100,
    height: 50,
    scale: 2.0,
    center: [500, 500],
    expected: (() => {
      const rect = Billboard.getScreenSpaceBoundingBox(
        { width: 100, height: 50, scale: 2.0, horizontalOrigin: HorizontalOrigin.LEFT, verticalOrigin: VerticalOrigin.TOP },
        new Cartesian2(500, 500)
      );
      return [rect.x, rect.y, rect.width, rect.height];
    })(),
  },
  {
    name: 'left_baseline',
    horizontalOrigin: HorizontalOrigin.LEFT,
    verticalOrigin: VerticalOrigin.BASELINE,
    width: 100,
    height: 50,
    scale: 2.0,
    center: [500, 500],
    expected: (() => {
      const rect = Billboard.getScreenSpaceBoundingBox(
        { width: 100, height: 50, scale: 2.0, horizontalOrigin: HorizontalOrigin.LEFT, verticalOrigin: VerticalOrigin.BASELINE },
        new Cartesian2(500, 500)
      );
      return [rect.x, rect.y, rect.width, rect.height];
    })(),
  },
];

// Equality
const defEqualsDef = a.equals(c.get(0));
const defEqualsCustom = a.equals(b);

// Collection operations
const initialLength = c.length;
const containsBefore = c.contains(b);
const firstRemoval = c.remove(b);
const secondRemoval = c.remove(b);
const containsAfter = c.contains(b);
const lengthAfter = c.length;
const retained = a.equals(c.get(0));

// Bounding sphere
const pts = [new Cartesian3(131075, -65538, 3.5), new Cartesian3(10, 20, 30)];
const sphere = BoundingSphere.fromPoints(pts);
const translation = Matrix4.fromTranslation(new Cartesian3(25, 50, 75));
const transformedSphere = BoundingSphere.transform(sphere, translation, new BoundingSphere());

// Vertex attributes computation
const UPPER_BOUND = 32768.0;
const LEFT_SHIFT16 = 65536.0;
const LEFT_SHIFT12 = 4096.0;
const LEFT_SHIFT8 = 256.0;
const LEFT_SHIFT7 = 128.0;
const LEFT_SHIFT5 = 32.0;
const LEFT_SHIFT3 = 8.0;
const LEFT_SHIFT2 = 4.0;
const RIGHT_SHIFT8 = 1.0 / 256.0;

const pos = new Cartesian3(131075, -65538, 3.5);
const encoded = EncodedCartesian3.fromCartesian(pos);

let c0 = Math.floor(CesiumMath.clamp(12, -UPPER_BOUND, UPPER_BOUND) + UPPER_BOUND) * LEFT_SHIFT7;
c0 += (-1 + 1.0) * LEFT_SHIFT5;
c0 += (1 + 1.0) * LEFT_SHIFT3;
c0 += 1.0 * LEFT_SHIFT2;

let c1 = Math.floor(CesiumMath.clamp(-24, -UPPER_BOUND, UPPER_BOUND) + UPPER_BOUND) * LEFT_SHIFT8;
let c2 = Math.floor(CesiumMath.clamp(0, -UPPER_BOUND, UPPER_BOUND) + UPPER_BOUND) * LEFT_SHIFT8;
const tempTranslateY = (CesiumMath.clamp(0, -UPPER_BOUND, UPPER_BOUND) + UPPER_BOUND) * RIGHT_SHIFT8;
const upperTranslateY = Math.floor(tempTranslateY);
const lowerTranslateY = Math.floor((tempTranslateY - upperTranslateY) * LEFT_SHIFT8);
c1 += upperTranslateY;
c2 += lowerTranslateY;

const axis = new Cartesian3(0, 1, 0);
let ca1_0 = CesiumMath.clamp(64, 0.0, LEFT_SHIFT16);
let ca1_1 = AttributeCompression.octEncodeFloat(axis);
let nearVal = 255.0;
let farVal = Math.floor(0.2 * 255.0);
ca1_0 = ca1_0 * LEFT_SHIFT8 + nearVal;
ca1_1 = ca1_1 * LEFT_SHIFT8 + farVal;

const col = new Color(0.25, 0.5, 0.75, 0.8);
const colRGB8 = AttributeCompression.encodeRGB8(col);
const pickRGB8 = AttributeCompression.encodeRGB8(Color.WHITE);
const colAlphaByte = Color.floatToByte(col.alpha);
const pickAlphaByte = Color.floatToByte(1.0);
const ca2_2 = colAlphaByte * LEFT_SHIFT16 + pickAlphaByte * LEFT_SHIFT8 + (1.0 * 2.0 + 1.0);
const ca2_3 = 32 * LEFT_SHIFT2 + 0;

const dim = 64 * LEFT_SHIFT12 + 32;

const output = {
  defaults: snapshot(a),
  custom: snapshot(b),
  bboxCases,
  defEqualsDef,
  defEqualsCustom,
  initialLength,
  containsBefore,
  firstRemoval,
  secondRemoval,
  containsAfter,
  lengthAfter,
  retained,
  boundingSphere: {
    center: [sphere.center.x, sphere.center.y, sphere.center.z],
    radius: sphere.radius,
  },
  transformedBoundingSphere: {
    center: [transformedSphere.center.x, transformedSphere.center.y, transformedSphere.center.z],
    radius: transformedSphere.radius,
  },
  encodedAttributes: {
    positionHighAndScale: [encoded.high.x, encoded.high.y, encoded.high.z, 2.5],
    positionLowAndRotation: [encoded.low.x, encoded.low.y, encoded.low.z, 0.7853981633974483],
    compressedAttribute0: [c0, c1, c2, 0.0],
    compressedAttribute1: [ca1_0, ca1_1, 50.0, 500.0],
    compressedAttribute2: [colRGB8, pickRGB8, ca2_2, ca2_3],
    eyeOffset: [5.0, 10.0, -50.0, 0.0],
    scaleByDistance: [100.0, 2.0, 1000.0, 0.5],
    pixelOffsetScaleByDistance: [20.0, 1.5, 200.0, 0.5],
    compressedAttribute3: [225.0, 6250000.0, 10000.0, dim],
    splitDirection: -1.0,
  },
};

mkdirSync(join(here, '../../tests/data'), { recursive: true });
const target = join(here, '../../tests/data/scene-billboards.json');
writeFileSync(target, JSON.stringify(output, null, 2) + '\n');
console.log('Wrote', target);
