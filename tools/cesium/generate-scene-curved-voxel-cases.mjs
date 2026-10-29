import {writeFileSync} from 'node:fs';
import Cylinder from '@cesium/engine/Source/Scene/VoxelCylinderShape.js';
import Ellipsoid from '@cesium/engine/Source/Scene/VoxelEllipsoidShape.js';
import Matrix4 from '@cesium/engine/Source/Core/Matrix4.js';
import Matrix3 from '@cesium/engine/Source/Core/Matrix3.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import OBB from '@cesium/engine/Source/Core/OrientedBoundingBox.js';
const v=a=>Cartesian3.fromArray(a),pack=o=>OBB.pack(o,[]), cases=[];
for(const [kind,Shape] of [['cylinder',Cylinder],['ellipsoid',Ellipsoid]]) {
 const min=kind==='cylinder'?[0,-Math.PI,-1]:[-Math.PI,-Math.PI/2,-6356752.314245179];
 const max=kind==='cylinder'?[1,Math.PI,1]:[Math.PI,Math.PI/2,63781370];
 const bounds=kind==='cylinder'?[
  [min,max],[[.2,-1,-.7],[.9,1,.7]],[[.1,2,-1],[.8,-2,1]],[[.5,-.7,-.3],[.5,.7,.3]],[[0,0,-1],[1,0,1]],
 ]:[
  [min,max],[[-1,-.5,-1000],[1,.5,1000]],[[2,-.3,0],[-2,.6,10000]],[[-Math.PI,-Math.PI/2,0],[Math.PI,Math.PI/2,100]],[[0,0,0],[.5,.5,10000]],
 ];
 for(let i=0;i<bounds.length;i++) for(const transformed of [false,true]) for(const clip of [false,true]) {
  const [lo,hi]=bounds[i], s=new Shape();
  const model=transformed?Matrix4.fromRotationTranslation(Matrix3.fromScale(new Cartesian3(2,kind==='ellipsoid'?2:3,4)),new Cartesian3(10,-20,30)):Matrix4.clone(Matrix4.IDENTITY);
  const clipMin=clip?(kind==='cylinder'?[.3,-.5,-.4]:[-.7,-.3,-500]):null;
  const clipMax=clip?(kind==='cylinder'?[.7,.5,.4]:[.7,.3,500]):null;
  const visible=s.update(model,v(lo),v(hi),clipMin&&v(clipMin),clipMax&&v(clipMax));
  const c={kind,i,transformed,clip,min:lo,max:hi,clipMin,clipMax,model:Array.from(model),visible};
  if(visible){
   c.intersections=s.shaderMaximumIntersectionsLength;c.obb=pack(s.orientedBoundingBox);
   c.tile=pack(s.computeOrientedBoundingBoxForTile(2,1,2,1,new OBB()));
   c.sample=pack(s.computeOrientedBoundingBoxForSample({level:1,x:0,y:1,z:0},new Cartesian3(4,4,4),new Cartesian3(.25,.5,.75),new OBB()));
   c.point=[.2,.1,-.1];const uv=s.convertLocalToShapeUvSpace(v(c.point),new Cartesian3());c.uv=[uv.x,uv.y,uv.z];
   c.camera=[7000000,2000000,1000000]; const rotation=Matrix3.fromRotationZ(.4);
   s.updateViewTransforms({camera:{positionWC:v(c.camera)},context:{uniformState:{viewRotation:rotation,inverseView:Matrix4.fromRotationTranslation(Matrix3.transpose(rotation,new Matrix3()),v(c.camera)),inverseViewRotation:Matrix3.transpose(rotation,new Matrix3())}}});
   c.viewRotation=Array.from(rotation);
   c.cameraTransform=Array.from(kind==='cylinder'?s._shaderUniforms.cylinderEcToRadialTangentUp:s._shaderUniforms.ellipsoidEcToEastNorthUp);
  }
  cases.push(c);
 }
}
writeFileSync(new URL('../../tests/fixtures/scene/CurvedVoxelCases.json',import.meta.url),JSON.stringify({version:'23.0.0',cases},null,2)+'\n');
