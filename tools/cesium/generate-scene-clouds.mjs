// Cloud snapshots use the pinned engine; attribute and bounding-sphere expectations are calculated separately.
import CloudCollection from '@cesium/engine/Source/Scene/CloudCollection.js';
import CumulusCloud from '@cesium/engine/Source/Scene/CumulusCloud.js';
import CloudType from '@cesium/engine/Source/Scene/CloudType.js';
import Cartesian2 from '@cesium/engine/Source/Core/Cartesian2.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Color from '@cesium/engine/Source/Core/Color.js';
import EncodedCartesian3 from '@cesium/engine/Source/Core/EncodedCartesian3.js';
import BoundingSphere from '@cesium/engine/Source/Core/BoundingSphere.js';
import Matrix4 from '@cesium/engine/Source/Core/Matrix4.js';
import { writeFileSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));

const snapshot = (c) => ({
  show: c.show,
  position: [c.position.x, c.position.y, c.position.z],
  scale: [c.scale.x, c.scale.y],
  maximumSize: [c.maximumSize.x, c.maximumSize.y, c.maximumSize.z],
  slice: c.slice,
  brightness: c.brightness,
  color: [c.color.red, c.color.green, c.color.blue, c.color.alpha],
});

const encodeAttributes = (cloud) => {
  const encoded = EncodedCartesian3.fromCartesian(cloud.position);
  const color = cloud.color;
  const red = Color.floatToByte(color.red);
  const green = Color.floatToByte(color.green);
  const blue = Color.floatToByte(color.blue);
  const alpha = Color.floatToByte(color.alpha);

  const instanced = {
    positionHighAndScaleX: [encoded.high.x, encoded.high.y, encoded.high.z, cloud.scale.x],
    positionLowAndScaleY: [encoded.low.x, encoded.low.y, encoded.low.z, cloud.scale.y],
    packedAttribute0: [cloud.show ? 1.0 : 0.0, cloud.brightness, 0.0, 0.0],
    packedAttribute1: [cloud.maximumSize.x, cloud.maximumSize.y, cloud.maximumSize.z, cloud.slice],
    color: [red, green, blue, alpha],
  };

  const quadCoords = [
    [0.0, 0.0],
    [1.0, 0.0],
    [1.0, 1.0],
    [0.0, 1.0],
  ];

  const batched = quadCoords.map(([u, v]) => ({
    positionHighAndScaleX: [encoded.high.x, encoded.high.y, encoded.high.z, cloud.scale.x],
    positionLowAndScaleY: [encoded.low.x, encoded.low.y, encoded.low.z, cloud.scale.y],
    packedAttribute0: [cloud.show ? 1.0 : 0.0, cloud.brightness, u, v],
    packedAttribute1: [cloud.maximumSize.x, cloud.maximumSize.y, cloud.maximumSize.z, cloud.slice],
    color: [red, green, blue, alpha],
  }));

  return { instanced, batched };
};

// 1. Collection defaults and custom options
const defaultCollection = new CloudCollection();
const customCollection = new CloudCollection({
  show: false,
  noiseDetail: 32.0,
  noiseOffset: new Cartesian3(10.0, 20.0, 30.0),
  debugBillboards: true,
  debugEllipsoids: true,
});

const collectionDefaults = {
  show: defaultCollection.show,
  noiseDetail: defaultCollection.noiseDetail,
  noiseOffset: [
    defaultCollection.noiseOffset.x,
    defaultCollection.noiseOffset.y,
    defaultCollection.noiseOffset.z,
  ],
  debugBillboards: defaultCollection.debugBillboards,
  debugEllipsoids: defaultCollection.debugEllipsoids,
  noiseTextureDimensions: [128, 4, 0.25],
};

const collectionCustom = {
  show: customCollection.show,
  noiseDetail: customCollection.noiseDetail,
  noiseOffset: [
    customCollection.noiseOffset.x,
    customCollection.noiseOffset.y,
    customCollection.noiseOffset.z,
  ],
  debugBillboards: customCollection.debugBillboards,
  debugEllipsoids: customCollection.debugEllipsoids,
};

// 2. Cloud instances
const collection = new CloudCollection();
const defaultCloud = collection.add();
const customCloud = collection.add({
  show: true,
  position: new Cartesian3(131075.0, -65538.0, 3.5),
  scale: new Cartesian2(25.0, 15.0),
  maximumSize: new Cartesian3(28.0, 16.0, 12.0),
  slice: 0.45,
  brightness: 0.8,
  color: new Color(0.2, 0.4, 0.6, 0.8),
});
const onlyMaxCloud = collection.add({
  maximumSize: new Cartesian3(30.0, 18.0, 10.0),
});
const onlyScaleCloud = collection.add({
  scale: new Cartesian2(16.0, 9.0),
});

// 3. Collection lifecycle
const initialLength = collection.length;
const containsBefore = collection.contains(customCloud);
const firstRemoval = collection.remove(customCloud);
const secondRemoval = collection.remove(customCloud);
const containsAfter = collection.contains(customCloud);
const lengthAfter = collection.length;
const retainedFirst = snapshot(collection.get(0));

// Test removeAll and destroy
const testColl = new CloudCollection();
const t1 = testColl.add();
const t2 = testColl.add();
const testCollInitialLen = testColl.length;
testColl.removeAll();
const testCollAfterRemoveAllLen = testColl.length;
const isDestroyedBefore = testColl.isDestroyed();
testColl.destroy();
const isDestroyedAfter = testColl.isDestroyed();

// 4. Bounding sphere checks
const pts = [new Cartesian3(131075.0, -65538.0, 3.5), new Cartesian3(10.0, 20.0, 30.0)];
const sphere = BoundingSphere.fromPoints(pts);
const translation = Matrix4.fromTranslation(new Cartesian3(25.0, 50.0, 75.0));
const transformedSphere = BoundingSphere.transform(sphere, translation, new BoundingSphere());

const output = {
  collectionDefaults,
  collectionCustom,
  clouds: {
    defaultCloud: snapshot(defaultCloud),
    customCloud: snapshot(customCloud),
    onlyMaxCloud: snapshot(onlyMaxCloud),
    onlyScaleCloud: snapshot(onlyScaleCloud),
  },
  attributes: {
    defaultCloud: encodeAttributes(defaultCloud),
    customCloud: encodeAttributes(customCloud),
    onlyMaxCloud: encodeAttributes(onlyMaxCloud),
    onlyScaleCloud: encodeAttributes(onlyScaleCloud),
  },
  lifecycle: {
    initialLength,
    containsBefore,
    firstRemoval,
    secondRemoval,
    containsAfter,
    lengthAfter,
    retainedFirst,
    testCollInitialLen,
    testCollAfterRemoveAllLen,
    isDestroyedBefore,
    isDestroyedAfter,
  },
  boundingSphere: {
    center: [sphere.center.x, sphere.center.y, sphere.center.z],
    radius: sphere.radius,
  },
  transformedBoundingSphere: {
    center: [transformedSphere.center.x, transformedSphere.center.y, transformedSphere.center.z],
    radius: transformedSphere.radius,
  },
};

const target = join(here, '../../tests/data/scene-clouds.json');
mkdirSync(dirname(target), { recursive: true });
writeFileSync(target, JSON.stringify(output, null, 2) + '\n');
console.log('Wrote', target);
