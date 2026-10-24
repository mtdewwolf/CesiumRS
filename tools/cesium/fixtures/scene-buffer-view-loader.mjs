import GltfBufferViewLoader from '@cesium/engine/Source/Scene/GltfBufferViewLoader.js';
import BufferLoader from '@cesium/engine/Source/Scene/BufferLoader.js';
import Resource from '@cesium/engine/Source/Core/Resource.js';
import {MeshoptEncoder,MeshoptDecoder} from 'meshoptimizer';
import {suite,enc} from './lib.mjs';
export default async function bufferViewLoader(){
 await Promise.all([MeshoptEncoder.ready,MeshoptDecoder.ready]);
 const suiteResult=suite('GltfBufferViewLoader');
 const scenarios=[];
 for(const pad of [0,3])for(const start of [0,1,4])for(const length of [0,2,5]){
  const bytes=Array.from({length:16},(_,i)=>i*13%256);scenarios.push({bytes,pad,sourceLength:8,view:{buffer:0,byteOffset:start,byteLength:length}});
 }
 for(const version of [0,1])for(const stride of [4,8,12,16])for(const filter of ['NONE','OCTAHEDRAL','QUATERNION','EXPONENTIAL']){
  if(filter==='OCTAHEDRAL'&&![4,8].includes(stride)||filter==='QUATERNION'&&stride!==8)continue;
  const count=6;let bytes=new Uint8Array(count*stride);for(let i=0;i<bytes.length;i++)bytes[i]=i*13%256;
  if(filter==='OCTAHEDRAL')bytes=MeshoptEncoder.encodeFilterOct(new Float32Array(Array.from({length:count},(_,i)=>{const a=i*.7;return [Math.cos(a)*.8,Math.sin(a)*.8,i%2?.6:-.6,i/count];}).flat()),count,stride,stride===4?8:12);
  if(filter==='QUATERNION')bytes=MeshoptEncoder.encodeFilterQuat(new Float32Array(Array.from({length:count},(_,i)=>[Math.sin(i*.7)*.6,Math.cos(i*.7)*.6,Math.sin(i*.4)*.8,Math.cos(i*.4)*.8]).flat()),count,stride,12);
  if(filter==='EXPONENTIAL')bytes=MeshoptEncoder.encodeFilterExp(new Float32Array(Array.from({length:count*stride/4},(_,i)=>(i-10)*.125)),count,stride,12);
  const encoded=Array.from(MeshoptEncoder.encodeGltfBuffer(bytes,count,stride,'ATTRIBUTES',version));
  scenarios.push({bytes:encoded,pad:3,sourceLength:encoded.length,view:{buffer:0,byteOffset:0,byteLength:count*stride,extensions:{EXT_meshopt_compression:{buffer:0,byteOffset:0,byteLength:encoded.length,count,byteStride:stride,mode:'ATTRIBUTES',filter}}}});
 }
 for(const mode of ['TRIANGLES','INDICES'])for(const stride of [2,4]){
  const input=stride===2?new Uint16Array([0,1,2,2,1,3]):new Uint32Array([0,1,2,2,1,3]);
  const encoded=Array.from(MeshoptEncoder.encodeGltfBuffer(new Uint8Array(input.buffer),input.length,stride,mode));
  scenarios.push({bytes:encoded,pad:0,sourceLength:encoded.length,view:{buffer:0,byteLength:input.byteLength,extensions:{EXT_meshopt_compression:{buffer:0,byteLength:encoded.length,count:input.length,byteStride:stride,mode}}}});
 }
 for(const scenario of scenarios){
  const all=new Uint8Array(scenario.pad+scenario.bytes.length+3);all.fill(238);all.set(scenario.bytes,scenario.pad);const source=new Uint8Array(all.buffer,scenario.pad,scenario.sourceLength);
  let child;
  const cache=Object.assign(function(){},{getEmbeddedBufferLoader(){return child=new BufferLoader({typedArray:source});},unload(loader){loader.destroy();}});
  const resource=new Resource('https://loader.test/model.gltf');
  const loader=new GltfBufferViewLoader({resourceCache:cache,gltf:{buffers:[{}],bufferViews:[scenario.view]},bufferViewId:0,gltfResource:resource,baseResource:resource,cacheKey:'view'});
  await loader.load();const result={bytes:Array.from(loader.typedArray),state:loader._state,childDestroyed:child.isDestroyed()};loader.unload();result.unloaded=loader.typedArray===undefined;loader.destroy();result.destroyed=loader.isDestroyed();
  suiteResult.cases.push({f:'load',a:[scenario],r:enc(result)});
 }
 return [suiteResult];
}
