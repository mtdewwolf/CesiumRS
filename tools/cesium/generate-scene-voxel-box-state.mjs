import {writeFileSync} from 'node:fs';
import Shape from '@cesium/engine/Source/Scene/VoxelBoxShape.js';
import Matrix4 from '@cesium/engine/Source/Core/Matrix4.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
const vector = v => [v.x,v.y,v.z];
const cases = [];
for (const clipped of [false,true]) {
  const shape = new Shape();
  const before = shape.shaderMaximumIntersectionsLength;
  const min = new Cartesian3(-1,-1,-1), max = new Cartesian3(1,1,1);
  const lo = clipped ? new Cartesian3(-.75,-.5,-.25) : undefined;
  const hi = clipped ? new Cartesian3(.25,.5,.75) : undefined;
  const visible = shape.update(Matrix4.IDENTITY,min,max,lo,hi);
  cases.push({clipped,before,visible,after:shape.shaderMaximumIntersectionsLength,renderMin:vector(shape._renderMinBounds),renderMax:vector(shape._renderMaxBounds),distances:Array.from({length:6},(_,i)=>shape.renderBoundPlanes.get(i).distance)});
}
writeFileSync(new URL('../../tests/fixtures/scene/VoxelBoxState.json',import.meta.url),JSON.stringify({version:'23.0.0',cases},null,2)+'\n');
