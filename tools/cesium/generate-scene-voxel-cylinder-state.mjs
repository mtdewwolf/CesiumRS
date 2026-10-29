import {writeFileSync} from 'node:fs';
const Shape = (await import('@cesium/engine/Source/Scene/VoxelCylinderShape.js')).default;
const Matrix4 = (await import('@cesium/engine/Source/Core/Matrix4.js')).default;
const Cartesian3 = (await import('@cesium/engine/Source/Core/Cartesian3.js')).default;
const vector = v => [v.x,v.y,v.z];
const OBB = (await import("@cesium/engine/Source/Core/OrientedBoundingBox.js")).default;
const packObb = o => { const p=[]; OBB.pack(o,p,0); return p; };
const cases = [];
const s = new Shape();
const min = new Cartesian3(0,-Math.PI,-1);
const max = new Cartesian3(1,Math.PI,1);
const m = Matrix4.IDENTITY;
const visible = s.update(m,min,max,null,null);
cases.push({
  visible,
  minBounds: vector(s._minBounds),
  maxBounds: vector(s._maxBounds),
  obb: packObb(s.orientedBoundingBox),
  tile: packObb(s.computeOrientedBoundingBoxForTile(1,0,0,0,new OBB())),
  sample: packObb(s.computeOrientedBoundingBoxForSample({level:0,x:0,y:0,z:0}, new Cartesian3(2,2,2), new Cartesian3(0.25,0.25,0.25), new OBB())),
  uv: vector(s.convertLocalToShapeUvSpace(new Cartesian3(0.2,0.1,-0.1), new Cartesian3())),
  intersections: s.shaderMaximumIntersectionsLength,
});
const s2 = new Shape();
const lo = new Cartesian3(0.2,-1,-0.5);
const hi = new Cartesian3(0.8,1,0.5);
const visible2 = s2.update(m,min,max,lo,hi);
cases.push({clipped:true, visible:visible2, intersections: s2.shaderMaximumIntersectionsLength});
writeFileSync(new URL('../../tests/fixtures/scene/VoxelCylinderState.json', import.meta.url), JSON.stringify({version:'23.0.0', cases}, null, 2)+'\n');
console.log('done');
