import Resource from '@cesium/engine/Source/Core/Resource.js';
import BufferLoader from '@cesium/engine/Source/Scene/BufferLoader.js';
import MetadataSchemaLoader from '@cesium/engine/Source/Scene/MetadataSchemaLoader.js';
import GltfJsonLoader from '@cesium/engine/Source/Scene/GltfJsonLoader.js';
import ResourceLoader from '@cesium/engine/Source/Scene/ResourceLoader.js';
import {suite,enc} from './lib.mjs';
const url='https://loader.test/models/model.gltf';
const bytes=[1,2,3,4,5];
const schema={id:'s',classes:{test:{properties:{height:{type:'SCALAR',componentType:'FLOAT32'}}}}};
const json={asset:{version:'2.0'},nodes:[{translation:[1,2,3]}],scenes:[{nodes:[0]}],scene:0};
export default async function resourceLoaders(){
 const b=suite('BufferLoader'),m=suite('MetadataSchemaLoader'),g=suite('GltfJsonLoader'),r=suite('ResourceLoader');
 for(const child of [null,'child',''])r.add('error',['parent',child],(parent,child)=>new ResourceLoader().getError(parent,child===null?undefined:new Error(child)).message);
 const original=BufferLoader._fetchArrayBuffer;
 try{
 for(const module of ['buffer','schema'])for(const external of [false,true])for(const fail of [false,true])for(const action of ['none','unload','destroy']){
  if(!external&&fail)continue;
  let resolve,reject,calls=0;
  const pending=new Promise((yes,no)=>{resolve=yes;reject=no;});
  BufferLoader._fetchArrayBuffer=()=>{calls++;return pending;};
  const resource=new Resource(url);resource.fetchJson=()=>{calls++;return pending;};
  const loader=module==='buffer'?new BufferLoader({cacheKey:'k',...(external?{resource}:{typedArray:new Uint8Array(bytes)})}):new MetadataSchemaLoader({cacheKey:'k',...(external?{resource}:{schema})});
  const snapshot=()=>({state:loader._state,available:module==='buffer'?!!loader.typedArray:!!loader.schema,destroyed:loader.isDestroyed()});
  const initial=snapshot(),first=loader.load(),second=loader.load();
  const loading=snapshot();
  if(action==='destroy')loader.destroy();
  if(external){if(fail)reject(new Error('Request has failed. Status Code: 503'));else resolve(module==='buffer'?new Uint8Array(bytes).buffer:schema);}
  let error=null,completed;
  try{completed=!!await first;await second;}catch(e){error=e.message;await second.catch(()=>{});}
  if(action==='unload')loader.unload();
  const result={initial,loading,after:snapshot(),calls,error,completed:completed??null,cacheKey:loader.cacheKey};
  if(module==='buffer')result.bytes=loader.typedArray?Array.from(loader.typedArray):null;
  else result.schemaId=loader.schema?.id??null;
  (module==='buffer'?b:m).cases.push({f:'lifecycle',a:[external,fail,action],r:enc(result)});
 }
 BufferLoader._fetchArrayBuffer=original;
 const documents=[json,{asset:{version:'1.0'},buffers:{buffer:{byteLength:5,uri:'data:application/octet-stream;base64,AQIDBAU='}},nodes:{root:{translation:[1,2,3]}},scenes:{main:{nodes:['root']}},scene:'main'},{asset:{version:'2.0'},buffers:[{byteLength:5,uri:'data:application/octet-stream;base64,AQIDBAU='}]},{asset:{version:'2.0'},extensionsRequired:['DOES_NOT_EXIST']},{asset:{version:'2.0'},extensionsRequired:['KHR_materials_unlit']},{asset:{version:'2.0'},buffers:[{byteLength:5,uri:'external.bin'}]},{asset:{version:'2.0'},extras:{keep:1},buffers:[{byteLength:5,uri:'data:application/octet-stream;base64,AQIDBAU=',extras:{keep:2}}]}];
 for(const document of documents)for(const kind of ['json','bytes','external','glb']){
  const embedded={},external=[];
  const cache=Object.assign(function(){},{getEmbeddedBufferLoader({bufferId,typedArray}){embedded[bufferId]=Array.from(typedArray);return new BufferLoader({typedArray});},getExternalBufferLoader({resource}){external.push(resource.url);return new BufferLoader({typedArray:new Uint8Array(bytes)});},unload(loader){loader.destroy();}});
  let typedArray=new TextEncoder().encode(JSON.stringify(document));
  if(kind==='glb'){
   const length=(typedArray.length+3)&~3,buffer=new ArrayBuffer(20+length),view=new DataView(buffer);view.setUint32(0,0x46546c67,true);view.setUint32(4,2,true);view.setUint32(8,buffer.byteLength,true);view.setUint32(12,length,true);view.setUint32(16,0x4e4f534a,true);const out=new Uint8Array(buffer);out.fill(32,20);out.set(typedArray,20);typedArray=out;
  }
  const resource=new Resource(url);resource.fetchArrayBuffer=()=>Promise.resolve(typedArray.buffer);
  const loader=new GltfJsonLoader({resourceCache:cache,gltfResource:resource,baseResource:resource,cacheKey:'g',...(kind==='json'?{gltfJson:structuredClone(document)}:kind!=='external'?{typedArray}:{})});
  let result;try{await loader.load();result={json:loader.gltf,embedded,external,state:loader._state};}catch(e){result={error:e.message,state:loader._state};}
  g.cases.push({f:'load',a:[document,kind],r:enc(result)});loader.destroy();
 }
 }finally{BufferLoader._fetchArrayBuffer=original;}
 return [r,b,m,g];
}
