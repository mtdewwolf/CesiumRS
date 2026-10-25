import B3dm from '@cesium/engine/Source/Scene/Model/B3dmLoader.js';
import I3dm from '@cesium/engine/Source/Scene/Model/I3dmLoader.js';
import Pnts from '@cesium/engine/Source/Scene/Model/PntsLoader.js';
import GeoJson from '@cesium/engine/Source/Scene/Model/GeoJsonLoader.js';
import Buffer from '@cesium/engine/Source/Renderer/Buffer.js';
import Resource from '@cesium/engine/Source/Core/Resource.js';
import Cache from '@cesium/engine/Source/Scene/ResourceCache.js';
import FeatureDetection from '@cesium/engine/Source/Core/FeatureDetection.js';
import {enc} from './lib.mjs';

const text=new TextEncoder();
function padded(v,offset=0) {const b=text.encode(JSON.stringify(v));const out=new Uint8Array(b.length+(8-(offset+b.length)%8)%8).fill(32);out.set(b);return out;}
function join(parts) {const a=new Uint8Array(parts.reduce((n,p)=>n+p.length,0));let o=0;for(const p of parts){a.set(p,o);o+=p.length;}return a;}
function words(v) {const a=new Uint8Array(v.length*4),d=new DataView(a.buffer);v.forEach((x,i)=>d.setUint32(i*4,x,true));return a;}
function glb() {
 const data=new Float32Array([0,0,0,1,0,0,0,1,0]);
 const bytes=new Uint8Array(data.buffer);
 const g={asset:{version:'2.0'},buffers:[{byteLength:bytes.length,uri:'data:application/octet-stream;base64,'+globalThis.Buffer.from(bytes).toString('base64')}],bufferViews:[{buffer:0,byteLength:bytes.length}],accessors:[{bufferView:0,componentType:5126,type:'VEC3',count:3,min:[0,0,0],max:[1,1,0]},{componentType:5126,type:'SCALAR',count:3}],meshes:[{primitives:[{attributes:{POSITION:0,_BATCHID:1}}]}],nodes:[{mesh:0}],scenes:[{nodes:[0]}],scene:0};
 const j=padded(g);return join([words([0x46546c67,2,20+j.length]),words([j.length,0x4e4f534a]),j]);
}
function tile(kind,ft,bt=null,binary=new Uint8Array()) {
 const header=kind==='i3dm'?32:28;
 const f=padded(ft,header),b=bt===null?new Uint8Array():padded(bt);
 const payload=kind==='pnts'?new Uint8Array():glb();
 const body=join([f,binary,b,payload]);
 return Array.from(join([text.encode(kind),words([1,header+body.length,f.length,binary.length,b.length,0,...(kind==='i3dm'?[1]:[])]),body]));
}
function math(v){return v===undefined?null:enc(v);}
function attribute(a) {return {name:a.name??null,semantic:a.semantic??null,setIndex:a.setIndex??null,type:a.type??null,datatype:a.componentDatatype??null,count:a.count??null,min:math(a.min),max:math(a.max),constant:math(a.constant),normalized:a.normalized??false,array:a.typedArray?Array.from(a.typedArray):null,buffer:a.buffer?.bytes??null,quantization:a.quantization?{oct:a.quantization.octEncoded,zxy:a.quantization.octEncodedZXY,range:math(a.quantization.normalizationRange),offset:math(a.quantization.quantizedVolumeOffset),step:math(a.quantization.quantizedVolumeStepSize)}:null};}
function snapshot(c) {const m=c.structuralMetadata;return {transform:enc(c.transform),up:c.upAxis??null,forward:c.forwardAxis??null,metadata:m?{tables:m.propertyTableCount,attributes:m.propertyAttributes.length}:null,nodes:c.nodes.map(n=>({instances:n.instances?{world:n.instances.transformInWorldSpace,attributes:n.instances.attributes.map(attribute),features:n.instances.featureIds.map(f=>({count:f.featureCount??null,table:f.propertyTableId??null,set:f.setIndex??null,positional:f.positionalLabel??null}))}:null,primitives:n.primitives.map(p=>({type:p.primitiveType,attributes:p.attributes.map(attribute),indices:p.indices?{count:p.indices.count,type:p.indices.indexDatatype??p.indices.buffer?.indexDatatype,bytes:p.indices.buffer?.bytes??null}:null,features:p.featureIds.map(f=>({count:f.featureCount??null,table:f.propertyTableId??null,set:f.setIndex??null,positional:f.positionalLabel??null})),material:{unlit:p.material.unlit,alpha:p.material.alphaMode,metallic:p.material.metallicRoughness.metallicFactor,roughness:p.material.metallicRoughness.roughnessFactor}}))}))};}
export async function sceneModelTileLoaders() {
 const cases=[];
 const oldV=Buffer.createVertexBuffer,oldI=Buffer.createIndexBuffer,oldType=process.type;
 const oldWebP=[FeatureDetection.supportsWebP._result,FeatureDetection.supportsWebP._promise];
 process.type='renderer';FeatureDetection.supportsWebP._result=false;FeatureDetection.supportsWebP._promise=Promise.resolve(false);
 const gpu=({typedArray})=>({sizeInBytes:typedArray.byteLength,bytes:Array.from(new Uint8Array(typedArray.buffer,typedArray.byteOffset,typedArray.byteLength)),isDestroyed(){return !!this.dead;},destroy(){this.dead=true;}});
 Buffer.createVertexBuffer=gpu;Buffer.createIndexBuffer=({indexDatatype,...o})=>({...gpu(o),indexDatatype});
 const inputs=[];
 for(const batch of [0,2]) for(const rtc of [undefined,[10,20,30]]) inputs.push({kind:'b3dm',bytes:tile('b3dm',{BATCH_LENGTH:batch,RTC_CENTER:rtc},batch?{name:['a','b']}:null)});
 for(const f of [
  {INSTANCES_LENGTH:2,POSITION:[10,20,30,40,50,60]},
  {INSTANCES_LENGTH:2,POSITION:[1,2,3,4,5,6],RTC_CENTER:[10,20,30],SCALE:[2,3],SCALE_NON_UNIFORM:[1,2,3,4,5,6],BATCH_ID:[1,0]},
  {INSTANCES_LENGTH:2,POSITION:[1,2,3,4,5,6],NORMAL_UP:[0,0,1,0,0,1],NORMAL_RIGHT:[1,0,0,1,0,0]},
  {INSTANCES_LENGTH:2,POSITION_QUANTIZED:[0,32767,65535,65535,0,32767],QUANTIZED_VOLUME_OFFSET:[1,2,3],QUANTIZED_VOLUME_SCALE:[10,20,30]},
 ]) inputs.push({kind:'i3dm',bytes:tile('i3dm',f,{name:['a','b']})});
 const pos=[1,2,3,4,5,6,7,8,9];
 for(const f of [
  {POSITION:pos}, {POSITION:pos,RGB:[255,0,0,0,255,0,0,0,255]},
  {POSITION:pos,RGBA:[255,0,0,128,0,255,0,255,0,0,255,100],NORMAL:[0,0,1,0,1,0,1,0,0]},
  {POSITION:pos,RGB565:[65535,0,2016]}, {POSITION:pos,CONSTANT_RGBA:[20,40,60,128],RTC_CENTER:[10,20,30]},
  {POSITION:pos,NORMAL_OCT16P:[0,0,128,128,255,255]},
  {POSITION_QUANTIZED:[0,32767,65535,65535,0,32767,1,2,3],QUANTIZED_VOLUME_OFFSET:[1,2,3],QUANTIZED_VOLUME_SCALE:[10,20,30]},
  {POSITION:pos,BATCH_ID:[0,1,0],BATCH_LENGTH:2},
 ]) inputs.push({kind:'pnts',keep:true,bytes:tile('pnts',{POINTS_LENGTH:3,...f},f.BATCH_ID?{name:['a','b']}:null)});
 for(const geometry of [
  {type:'Point',coordinates:[-105,40,100]},
  {type:'MultiPoint',coordinates:[[-105,40],[-104,41,10]]},
  {type:'LineString',coordinates:[[-105,40],[-104,41],[-103,40]]},
  {type:'MultiLineString',coordinates:[[[-105,40],[-104,41]],[[-103,41],[-102,40]]]},
  {type:'Polygon',coordinates:[[[-105,40],[-104,41],[-103,40],[-105,40]]]},
  {type:'MultiPolygon',coordinates:[[[[-105,40],[-104,41],[-103,40],[-105,40]]]]},
 ]) inputs.push({kind:'geo',geo:{type:'Feature',geometry,properties:{name:'test'}}});
 try {
  for(const c of inputs) {
   const bytes=c.bytes?new Uint8Array(c.bytes).buffer:undefined;
   const options={arrayBuffer:bytes,loadAttributesFor2D:!!c.keep,asynchronous:false,incrementallyLoadTextures:false};
   let loader;
   if(c.kind==='b3dm') loader=new B3dm({...options,b3dmResource:new Resource('https://loader.test/model.b3dm')});
   else if(c.kind==='i3dm') loader=new I3dm({...options,i3dmResource:new Resource('https://loader.test/model.i3dm')});
   else if(c.kind==='pnts') loader=new Pnts(options);
   else loader=new GeoJson({geoJson:c.geo});
   await loader.load();
   let done=false;
   for(let i=0;i<50;i++) {await new Promise(r=>setImmediate(r));if(loader.process({context:{webgl2:true},frameNumber:i,scene3DOnly:false,jobScheduler:{execute(job){job.execute();return true;}}})){done=true;break;}}
   if(!done) throw Error('never ready '+c.kind);
   cases.push({f:'load',a:[c],r:snapshot(loader.components)});
   loader.destroy();Cache.clearForSpecs();
  }
 } finally {Buffer.createVertexBuffer=oldV;Buffer.createIndexBuffer=oldI;process.type=oldType;[FeatureDetection.supportsWebP._result,FeatureDetection.supportsWebP._promise]=oldWebP;}
 return {module:'ModelTileLoaders',cases};
}
