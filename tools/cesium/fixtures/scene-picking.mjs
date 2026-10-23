import * as C from '@cesium/engine';
import defined from '@cesium/engine/Source/Core/defined.js';
import PickFramebuffer from '@cesium/engine/Source/Scene/PickFramebuffer.js';
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
  return [p,fb];
}
