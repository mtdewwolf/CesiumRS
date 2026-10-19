import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import {read,write} from 'ktx-parse';
import {enc} from './lib.mjs';
export default async function coreTextures(){
 const prior={self:globalThis.self,post:globalThis.postMessage,dirname:globalThis.__dirname,require:globalThis.require};
 globalThis.self={};globalThis.__dirname='.';globalThis.require=createRequire(import.meta.url);
 const worker=(await import('@cesium/engine/Source/Workers/transcodeKTX2.js')).default;
 globalThis.self=worker;
 const process=async parameters=>{let response;globalThis.postMessage=r=>{response=r;};await worker.onmessage({data:{id:0,parameters,canTransferArrayBuffer:false}});if(response.error){const e=new Error(response.error.message);e.name=response.error.name;throw e;}return response.result;};
 const s={module:'KTX2Transcoder',cases:[]};
 const add=async(args,bytes,caps)=>{let r;try{const result=await process({ktx2Buffer:bytes,supportedTargetFormats:caps});r=enc(result.map(level=>Object.entries(level).map(([face,b])=>[face,b.internalFormat,b.datatype,b.width,b.height,new Uint8Array(b.levelBuffer.buffer,b.levelBuffer.byteOffset,b.levelBuffer.byteLength)])));}catch(e){r={throws:e.message,kind:e.name};}s.cases.push({f:'transcode',a:args,r});};
 try {
  await process({webAssemblyConfig:{wasmBinaryFile:'basis_transcoder.wasm',wasmBinary:readFileSync(new URL('../node_modules/@cesium/engine/Source/ThirdParty/basis_transcoder.wasm',import.meta.url))}});
  for(const name of ['Green4x4.ktx2','Green4x4Mipmap.ktx2','Green4x4_ETC1S.ktx2','Green4x4Mipmap_ETC1S.ktx2','Logo32x32_UASTC_Zstd.ktx2','Cesium_Logo_ETC1S.ktx2','Cesium_Logo_UASTC.ktx2'])
   for(const caps of [{},{etc:true},{etc1:true},{s3tc:true},{pvrtc:true},{astc:true},{bc7:true},{etc:true,etc1:true,s3tc:true,pvrtc:true,astc:true,bc7:true}]){
    await add([name,caps],new Uint8Array(readFileSync(new URL('../../../tests/data/ktx2/'+name,import.meta.url))),caps);
   }
  // Build valid alpha inputs from the same pinned codebooks. Reusing the
  // color slice for alpha gives nonconstant coverage without another encoder.
  for(const name of ['Green4x4_ETC1S.ktx2','Cesium_Logo_ETC1S.ktx2','Logo32x32_UASTC_Zstd.ktx2']){
   const container=read(new Uint8Array(readFileSync(new URL('../../../tests/data/ktx2/'+name,import.meta.url))));
   const dfd=container.dataFormatDescriptor[0];
   if(dfd.colorModel===163){
    dfd.samples.push({...dfd.samples[0],bitOffset:64,channelType:15});
    container.levels.forEach((level,i)=>{const original=level.levelData;level.levelData=new Uint8Array(original.length*2);level.levelData.set(original);level.levelData.set(original,original.length);Object.assign(container.globalData.imageDescs[i],{alphaSliceByteOffset:original.length,alphaSliceByteLength:original.length});});
   }else dfd.samples[0].channelType=3;
   const bytes=write(container);
   for(const caps of [{},{etc:true},{etc1:true},{s3tc:true},{pvrtc:true},{astc:true},{bc7:true},{etc:true,s3tc:true,pvrtc:true,astc:true,bc7:true}])await add([null,caps,Array.from(bytes)],bytes.slice(),caps);
  }
  for(const [vkFormat,typeSize,components] of [[37,1,4],[97,2,4],[109,4,4],[29,1,3]])
   for(const faceCount of [1,6]) for(const levels of [1,3]){
    const container=read(new Uint8Array(readFileSync(new URL('../../../tests/data/ktx2/Green4x4.ktx2',import.meta.url))));
    Object.assign(container,{vkFormat,typeSize,pixelWidth:4,pixelHeight:4,faceCount,levelCount:levels});
    container.levels=Array.from({length:levels},(_,i)=>({levelData:Uint8Array.from({length:(4>>i)*(4>>i)*components*typeSize*faceCount},(_,j)=>(j*13+5)%256),uncompressedByteLength:0}));
    const bytes=write(container);await add([null,{},Array.from(bytes)],bytes,{});
   }
  return [s];
 } finally {globalThis.self=prior.self;globalThis.postMessage=prior.post;globalThis.__dirname=prior.dirname;globalThis.require=prior.require;}
}
