// Exercise the pinned loaders with the real Draco worker. Only the GPU and
// worker scheduling boundaries are replaced; browser tests cover real uploads.
import {readFileSync} from 'node:fs';
import Resource from '@cesium/engine/Source/Core/Resource.js';
import Cache from '@cesium/engine/Source/Scene/ResourceCache.js';
import DracoLoader from '@cesium/engine/Source/Scene/DracoLoader.js';
import Buffer from '@cesium/engine/Source/Renderer/Buffer.js';
import {suite,enc} from './lib.mjs';
import {invoke,loadWorker} from './workers-terrain.mjs';

export default async function geometryLoaders(){
 const suites=Object.fromEntries(['GltfDracoLoader','GltfVertexBufferLoader','GltfIndexBufferLoader'].map(n=>[n,suite(n)]));
 const fixture=JSON.parse(readFileSync(new URL('../../../tests/fixtures/workers/decodeDraco.json',import.meta.url),'utf8'));
 const compressed=fixture.cases.find(c=>c.a[0].compressedAttributes?.POSITION!==undefined && c.r?.indexArray).a[0];
 const worker=await loadWorker('decodeDraco');await invoke(worker,{webAssemblyConfig:{}});
 const originalDecode=DracoLoader.decodeBufferView,originalVertex=Buffer.createVertexBuffer,originalIndex=Buffer.createIndexBuffer;
 let pending;
 DracoLoader.decodeBufferView=options=>{pending=invoke(worker,options).then(r=>r.result);return pending;};
 const gpu=({typedArray})=>({sizeInBytes:typedArray.byteLength,vertexArrayDestroyable:true,destroyed:false,destroy(){this.destroyed=true;},isDestroyed(){return this.destroyed;}});
 Buffer.createVertexBuffer=gpu;Buffer.createIndexBuffer=gpu;
 try{
 for(const kind of ['draco','vertex','index'])for(const mode of kind==='draco'?['compressed']:['plain','compressed'])for(const retain of kind==='draco'?[true]:[false,true])for(const upload of kind==='draco'?[false]:[false,true]){
  if(!retain&&!upload)continue;
  for(const float of mode==='compressed'?[false,true]:[false])for(const semantic of kind==='vertex'&&mode==='compressed'?['POSITION','NORMAL','TEX_COORD','COLOR','GENERIC']:['POSITION']){
   const accessorId={POSITION:1,NORMAL:2,TEX_COORD:3,GENERIC:4,COLOR:5}[semantic];
   Cache.clearForSpecs();pending=undefined;
   const raw=mode==='compressed'?compressed.array:[99,0,1,0,2,0,3,0,88];
   const gltf={buffers:[{byteLength:raw.length}],bufferViews:[{buffer:0,byteOffset:0,byteLength:raw.length}],accessors:[{bufferView:0,byteOffset:1,componentType:5123,type:'SCALAR',count:3},{componentType:float?5126:5123,type:'VEC3'},{componentType:float?5126:5121,type:'VEC3'},{componentType:float?5126:5123,type:'VEC2'},{componentType:5126,type:'SCALAR'},{componentType:float?5126:5121,type:'VEC3'}]};
   const primitive={attributes:{POSITION:1,NORMAL:2,TEXCOORD_0:3,GENERIC:4,COLOR_0:5}},draco={bufferView:0,attributes:compressed.compressedAttributes};
   const resource=new Resource('https://geometry.test/model.gltf');
   const seed=Cache.getEmbeddedBufferLoader({parentResource:resource,bufferId:0,typedArray:new Uint8Array(raw)});
   const frame={context:{id:'gpu'},jobScheduler:{execute(job){if(!allow)return false;job.execute();return true;}}};let allow=false;
   const options={gltf,primitive,draco,gltfResource:resource,baseResource:resource,frameState:frame,loadTypedArray:retain,loadBuffer:upload,accessorId:kind==='vertex'?accessorId:0,attributeSemantic:semantic};
   if(mode==='plain'){delete options.draco;options.bufferViewId=0;}
   const loader=kind==='draco'?Cache.getDracoLoader(options):kind==='vertex'?Cache.getVertexBufferLoader(options):Cache.getIndexBufferLoader(options);
   const snapshot=()=>({state:loader._state,cpu:kind==='draco'?null:loader.typedArray?Array.from(loader.typedArray):null,gpu:loader.buffer?.sizeInBytes??null,geometry:Cache.statistics.geometryByteLength});
   const initial=snapshot();await loader.load();const loaded=snapshot();
   let first=loader.process(frame);if(pending){await pending;await Promise.resolve();}
   // Processing after decode may attempt an upload, which the scheduler defers.
   if(mode==='compressed')first=loader.process(frame);
   const deferred=snapshot();allow=true;const ready=loader.process(frame),after=snapshot();
   const decoded=kind==='draco'?loader.decodedData:null;
   // Preserve the typed-array element type at the native JSON wire boundary.
   if(decoded)decoded.indices.indexDatatype=decoded.indices.typedArray.BYTES_PER_ELEMENT===2?5123:5125;
   const quantization=kind==='vertex'?loader.quantization??null:null;
   const buffer=loader.buffer;Cache.unload(loader);Cache.unload(seed);
   const result={initial,loaded,first,deferred,ready,after,decoded,quantization,destroyed:loader.isDestroyed(),gpuDestroyed:buffer?buffer.isDestroyed():null,remaining:Object.keys(Cache.cacheEntries).length,geometry:Cache.statistics.geometryByteLength};
   suites[kind==='draco'?'GltfDracoLoader':kind==='vertex'?'GltfVertexBufferLoader':'GltfIndexBufferLoader'].cases.push({f:'lifecycle',a:[{kind,mode,retain,upload,float,semantic,accessorId,gltf,primitive,draco,raw}],r:enc(result)});
  }
 }
 }finally{Cache.clearForSpecs();DracoLoader.decodeBufferView=originalDecode;Buffer.createVertexBuffer=originalVertex;Buffer.createIndexBuffer=originalIndex;}
 return Object.values(suites);
}
