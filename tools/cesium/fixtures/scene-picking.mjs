import * as C from '@cesium/engine';
import defined from '@cesium/engine/Source/Core/defined.js';
import PickFramebuffer from '@cesium/engine/Source/Scene/PickFramebuffer.js';
import OrthographicFrustum from '@cesium/engine/Source/Core/OrthographicFrustum.js';
import Ray from '@cesium/engine/Source/Core/Ray.js';
import {readFileSync} from 'node:fs';
import {scene} from './scene-transforms.mjs';
import {suite,enc,makeRng} from './lib.mjs';

// Execute the pinned source's private helpers unchanged; inject only its imports.
const source=readFileSync(new URL('../node_modules/@cesium/engine/Source/Scene/Picking.js',import.meta.url),'utf8');
const cullingSource=source.slice(source.indexOf('const scratchOrthoPickingFrustum'),source.indexOf('// Pick position and rectangle'));
const start=source.indexOf('function computePickingDrawingBufferRectangle(');
const rectangleSource=source.slice(start,source.indexOf('\n}',start)+2);
const imports=['Cartesian2','Cartesian3','Matrix4','OrthographicFrustum','OrthographicOffCenterFrustum','PerspectiveOffCenterFrustum','SceneMode'];
const helpers=new Function('defined',...imports,`${cullingSource}\n${rectangleSource}\nreturn {getPickCullingVolume,computePickingDrawingBufferRectangle};`)(defined,...imports.map(n=>C[n]));
// Offscreen-camera and sample-height rays share scratch values declared in Picking.js.
function lineAt(text){const i=source.indexOf(text);return source.slice(i,source.indexOf('\n',i)+1);}
const scratchDecl=lineAt('const offscreenDefaultWidth')+lineAt('const scratchRight')+lineAt('const scratchUp')+lineAt('const scratchSurfacePosition')+lineAt('const scratchSurfaceNormal')+lineAt('const scratchSurfaceRay');
const camStart=source.indexOf('function updateOffscreenCameraFromRay(');
const rayStart=source.indexOf('function getRayForSampleHeight(');
const rayEnd=source.indexOf('/**',rayStart);
const rayHelpers=new Function('Cartesian3','Cartographic','Ray','ApproximateTerrainHeights','undefinedValue',
  `${scratchDecl}${source.slice(camStart,source.indexOf('function updateMostDetailedRayPick',camStart))}${source.slice(rayStart,rayEnd)}\nreturn {updateOffscreenCameraFromRay,getRayForSampleHeight};`
)(C.Cartesian3,C.Cartographic,Ray,C.ApproximateTerrainHeights);
export default function scenePicking() {
  const p=suite('Picking'),fb=suite('PickFramebuffer');
  for(const mode of [0,1,2,3])for(const ortho of [false,true])for(const position of [[0,0],[400,300],[799,599]])for(const size of [[1,1],[3,5],[10,4]]) {
    const o={mode,ortho,position:[mode===2?1.8e7:0,0,2e7]};
    p.add('culling',[o,position,size,[0,0,800,600]],(o,p,size,v)=>{const s=scene(o);return helpers.getPickCullingVolume(s,new C.Cartesian2(...p),...size,new C.BoundingRectangle(...v)).planes.map(enc);});
  }
  for(const p0 of [[0,0],[100,200],[.25,9.75]])for(const size of [[null,null],[5,null],[4,6]])p.add('rectangle',[900,p0,size],(h,p,size)=>enc(helpers.computePickingDrawingBufferRectangle(h,new C.Cartesian2(...p),size[0]??undefined,size[1]??undefined,new C.BoundingRectangle())));
  const rng=makeRng(0x936ab);
  for(const [width,height]of [[1,1],[3,3],[5,3],[3,5],[4,4],[2,6],[6,2]])for(const limit of [0,1,2,5])for(let seed=0;seed<4;seed++) {
    const keys=Array.from({length:width*height},()=>Math.floor(rng()*6));
    fb.add('objects',[width,height,keys,limit],(w,h,keys,limit)=>{
      const objects=[{id:0},{id:1},{id:2},{id:3}],mapping=[undefined,objects[0],objects[0],objects[1],objects[2],objects[3]];
      const pixels=new Uint8Array(keys.flatMap(k=>[k,0,0,0]));
      const context={readPixels:()=>pixels,getObjectByPickColor:color=>mapping[color]};
      return PickFramebuffer.prototype.end.call({_context:context,_fb:{}},new C.BoundingRectangle(0,0,w,h),limit).map(o=>o.id);
    });
  }
  const ray=suite('PickingRay'),height=suite('PickHeightRay');
  const rays=[[[0,0,0],[0,0,-1]],[[1,2,3],[1,0,0]],[[-4,5,6],[0,1,1]],[[100,-200,300],[-1,-1,-1]],[[0,0,2e7],[1e-7,0,-1]]];
  for(const [origin,direction]of rays)for(const width of [null,0.1,2,50])ray.add('offscreen',[origin,direction,width],(o,d,w)=>{
    const camera=scene({mode:3,position:[0,0,2e7]}).camera;
    camera.frustum=new OrthographicFrustum({width:1,aspectRatio:1,near:1,far:1e9});
    const volume=rayHelpers.updateOffscreenCameraFromRay({},new Ray(new C.Cartesian3(...o),new C.Cartesian3(...d)),w??undefined,camera);
    return {position:[camera.position.x,camera.position.y,camera.position.z],direction:[camera.direction.x,camera.direction.y,camera.direction.z],up:[camera.up.x,camera.up.y,camera.up.z],right:[camera.right.x,camera.right.y,camera.right.z],width:camera.frustum.width,planes:volume.planes.map(enc)};
  });
  for(const lonlat of [[0,0,0],[0.5,0.25,100],[-2,-0.5,1e5],[3,1.2,-200]])height.add('ray',[lonlat],lonlat=>{
    const s=scene({mode:3,position:[0,0,2e7]});
    const result=rayHelpers.getRayForSampleHeight({ellipsoid:s.ellipsoid},new C.Cartographic(...lonlat));
    return {origin:[result.origin.x,result.origin.y,result.origin.z],direction:[result.direction.x,result.direction.y,result.direction.z]};
  });
  return [p,fb,ray,height];
}
