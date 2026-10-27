// Direct CPU reference cases from the pinned CesiumJS engine.
import PointPrimitiveCollection from '@cesium/engine/Source/Scene/PointPrimitiveCollection.js';
import PointPrimitive from '@cesium/engine/Source/Scene/PointPrimitive.js';
import Cartesian2 from '@cesium/engine/Source/Core/Cartesian2.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Color from '@cesium/engine/Source/Core/Color.js';
import NearFarScalar from '@cesium/engine/Source/Core/NearFarScalar.js';
import DistanceDisplayCondition from '@cesium/engine/Source/Core/DistanceDisplayCondition.js';
import { writeFileSync } from 'node:fs';
const c = new PointPrimitiveCollection();
const a = c.add();
const b = c.add({ position: new Cartesian3(131075, -65538, 3.5), color: new Color(.25,.5,.75,.125), outlineColor: new Color(1,0,0,.5), pixelSize: 21, outlineWidth: 3, scaleByDistance: new NearFarScalar(10,2,100,4), translucencyByDistance: new NearFarScalar(2,.25,20,.75), distanceDisplayCondition: new DistanceDisplayCondition(3,40), disableDepthTestDistance: 13, splitDirection: -1, id: 'b' });
const snapshot = p => ({ show:p.show, position:[p.position.x,p.position.y,p.position.z], color:[p.color.red,p.color.green,p.color.blue,p.color.alpha], outlineColor:[p.outlineColor.red,p.outlineColor.green,p.outlineColor.blue,p.outlineColor.alpha], outlineWidth:p.outlineWidth, pixelSize:p.pixelSize, scale:(p.scaleByDistance && [p.scaleByDistance.near,p.scaleByDistance.nearValue,p.scaleByDistance.far,p.scaleByDistance.farValue]) ?? null, translucency:(p.translucencyByDistance && [p.translucencyByDistance.near,p.translucencyByDistance.nearValue,p.translucencyByDistance.far,p.translucencyByDistance.farValue]) ?? null, distance:(p.distanceDisplayCondition && [p.distanceDisplayCondition.near,p.distanceDisplayCondition.far]) ?? null, depth:p.disableDepthTestDistance, id:p.id ?? null, split:p.splitDirection });
const bbox = PointPrimitive.getScreenSpaceBoundingBox(b, new Cartesian2(123, 456));
const output = { defaults:snapshot(a), custom:snapshot(b), bbox:[bbox.x,bbox.y,bbox.width,bbox.height], initialLength:c.length, containsBefore:c.contains(b), firstRemoval:c.remove(b), secondRemoval:c.remove(b), containsAfter:c.contains(b), lengthAfter:c.length, retained:a.equals(c.get(0)) };
writeFileSync('../../tests/data/scene-point-primitives.json', JSON.stringify(output, null, 2)+'\n');
