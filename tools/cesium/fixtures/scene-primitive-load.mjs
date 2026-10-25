// Direct reference cases for glTF mesh grouping and outline post-processing.
import getMeshPrimitives from '@cesium/engine/Source/Scene/getMeshPrimitives.js';
import Generator from '@cesium/engine/Source/Scene/Model/PrimitiveOutlineGenerator.js';
import Plan from '@cesium/engine/Source/Scene/PrimitiveLoadPlan.js';
import Buffer from '@cesium/engine/Source/Renderer/Buffer.js';
import {suite, enc, makeRng} from './lib.mjs';
export function scenePrimitiveLoad() {
  const s = suite('primitive-load');
  const add = (f, a, fn) => {let r; try {r = enc(fn());} catch(e) {r = {error:e.message};} s.cases.push({f,a:[a],r});};
  const mesh = (mode=5) => ({primitives:[{mode,indices:0,attributes:{POSITION:0}},{mode,indices:1},{mode,indices:2}],extensions:{EXT_mesh_primitive_restart:{primitiveGroups:[{primitives:[0,1],indices:3}]}}});
  for (const a of [{}, {primitives:[]}, ...Array.from({length:7},(_,i)=>mesh(i))]) add('mesh',a,()=>getMeshPrimitives(a));
  for (const group of [[],[0],[0,0],[0,9],[1,0],[2,1,0]]) {const a=mesh();a.extensions.EXT_mesh_primitive_restart.primitiveGroups[0].primitives=group;add('mesh',a,()=>getMeshPrimitives(a));}
  for (const groups of [[{primitives:[0,1],indices:3},{primitives:[1,2],indices:4}],[{primitives:[0],indices:3},{primitives:[0,2],indices:4}]]) {const a=mesh();a.extensions.EXT_mesh_primitive_restart.primitiveGroups=groups;add('mesh',a,()=>getMeshPrimitives(a));}
  const rng=makeRng(511), constructors={u8:Uint8Array,u16:Uint16Array,u32:Uint32Array};
  const cases=[];
  for(let i=0;i<100;i++) {const count=3+Math.floor(rng()*15), triangles=Array.from({length:(1+Math.floor(rng()*25))*3},()=>Math.floor(rng()*count)), edges=Array.from({length:2*Math.floor(rng()*30)},()=>Math.floor(rng()*count)); cases.push({count,triangles,edges,kind:['u8','u16','u32'][i%3]});}
  cases.push({count:3,triangles:[0,1],edges:[0],kind:'u8'},{count:7,triangles:[1,2,3],edges:[],kind:'u16'});
  for(const count of [255,256,65534,65535]) cases.push({count,triangles:[0,1,2,0,2,3,0,3,1,1,3,2],edges:[0,1,1,2,2,0,0,3],kind:count<257?'u8':'u16'});
  for(const a of cases) add('outline',a,()=>{const g=new Generator({triangleIndices:new constructors[a.kind](a.triangles),outlineIndices:a.edges,originalVertexCount:a.count}); const attr=Float32Array.from({length:a.count*2},(_,i)=>i/8);return {indices:Array.from(g.updatedTriangleIndices),kind:g.updatedTriangleIndices.constructor.name,coordinates:Array.from(g.outlineCoordinates),attribute:Array.from(g.updateAttribute(attr))};});
  const oldV=Buffer.createVertexBuffer, oldI=Buffer.createIndexBuffer;
  const gpu=({typedArray})=>({bytes:Array.from(new Uint8Array(typedArray.buffer,typedArray.byteOffset,typedArray.byteLength))});
  Buffer.createVertexBuffer=gpu;Buffer.createIndexBuffer=gpu;
  try {for(const outline of [false,true]) for(const splat of [false,true]) for(const upload of [false,true]) for(const retain of [false,true]) {const a={outline,splat,upload,retain};add('plan',a,()=>{const attribute={count:4,typedArray:new Float32Array([1,2,3,4,5,6,7,8])},indices={typedArray:new Uint8Array([0,1,2,0,2,3]),indexDatatype:5121};const primitive={attributes:[attribute],indices};const p=new Plan(primitive),ap=new Plan.AttributeLoadPlan(attribute),ip=new Plan.IndicesLoadPlan(indices);ap.loadBuffer=ip.loadBuffer=upload;ap.loadTypedArray=ip.loadTypedArray=retain;p.attributePlans=[ap];p.indicesPlan=ip;p.needsOutlines=outline;p.outlineIndices=[0,1,1,2,2,3,3,0];p.needsGaussianSplats=splat;p.postProcess({});const summary=x=>({array:x.typedArray?Array.from(x.typedArray):null,buffer:x.buffer?.bytes??null,count:x.count??null});return {attribute:summary(attribute),indices:summary(indices),outline:primitive.outlineCoordinates?summary(primitive.outlineCoordinates):null,plans:p.attributePlans.map(x=>[x.loadBuffer,x.loadTypedArray])};});}} finally {Buffer.createVertexBuffer=oldV;Buffer.createIndexBuffer=oldI;}
  return s;
}
