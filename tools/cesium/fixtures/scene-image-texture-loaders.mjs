// Actual pinned image/texture loaders; image decoding and GPU calls are host
// boundaries here. KTX2 uses the real worker. Browser checks exercise real images.
import {readFileSync} from 'node:fs';
import {createRequire} from 'node:module';
import Resource from '@cesium/engine/Source/Core/Resource.js';
import Cache from '@cesium/engine/Source/Scene/ResourceCache.js';
import ImageLoader from '@cesium/engine/Source/Scene/GltfImageLoader.js';
import KTX2 from '@cesium/engine/Source/Core/KTX2Transcoder.js';
import loadKTX2 from '@cesium/engine/Source/Core/loadKTX2.js';
import Texture from '@cesium/engine/Source/Renderer/Texture.js';
import PixelFormat from '@cesium/engine/Source/Core/PixelFormat.js';
import {loadWorker,invoke} from './workers-terrain.mjs';
import {suite,enc} from './lib.mjs';
export default async function imageTextureLoaders(){
 const images=suite('GltfImageLoader'),textures=suite('GltfTextureLoader');
 const old={decode:ImageLoader._loadImageFromTypedArray,image:Resource.prototype.fetchImage,bytes:Resource.prototype.fetchArrayBuffer,ready:KTX2._readyPromise,create:Texture.create,document:globalThis.document,require:globalThis.require,dirname:globalThis.__dirname};
 globalThis.require=createRequire(import.meta.url);globalThis.__dirname='.';
 const worker=await loadWorker('transcodeKTX2');
 await invoke(worker,{webAssemblyConfig:{wasmBinaryFile:'basis_transcoder.wasm',wasmBinary:readFileSync(new URL('../node_modules/@cesium/engine/Source/ThirdParty/basis_transcoder.wasm',import.meta.url))}});
 KTX2._readyPromise=Promise.resolve({scheduleTask:params=>invoke(worker,params,false).then(r=>r.result)});
 const specs=[{name:'png',raw:[137,80,1,2]}, {name:'jpeg',raw:[255,216,1,2]}, {name:'webp',raw:[82,73,70,70,0,0,0,0,87,69,66,80]}, {name:'unknown',raw:[0,1,2,3]},...['Green4x4.ktx2','Green4x4Mipmap.ktx2','Green4x4_ETC1S.ktx2','Green4x4Mipmap_ETC1S.ktx2','Logo32x32_UASTC_Zstd.ktx2'].map(file=>({name:file,raw:Array.from(readFileSync(new URL('../../../tests/data/ktx2/'+file,import.meta.url)))}))];
 let log,config,raw;
 const raster=()=>({width:config.width??3,height:config.height??5});
 ImageLoader._loadImageFromTypedArray=options=>{log.push(['decode',options.format,options.flipY,options.skipColorSpaceConversion]);return config.fail?Promise.reject(new Error('codec failure')):Promise.resolve(raster());};
 Resource.prototype.fetchImage=function(options){log.push(['image',this.url]);if(config.fail)return Promise.reject(new Error('Request has failed.'));log.push(['fetch-options',options.preferImageBitmap,options.skipColorSpaceConversion]);return Promise.resolve(raster());};
 Resource.prototype.fetchArrayBuffer=function(){if(!this.url.startsWith('data:'))log.push(['bytes',this.url]);return config.fail?Promise.reject(new Error('Request has failed.')):Promise.resolve(Uint8Array.from(raw).buffer);};
 globalThis.document={baseURI:'https://image.test/',location:{href:'https://image.test/'},createElement(){return{width:0,height:0,getContext(){return{drawImage(){}};}};}};
 Texture.create=options=>{
  const image=options.source,format=options.pixelFormat??6408,width=options.width??image.width,height=options.height??image.height;
  const size=PixelFormat.isCompressedFormat(format)?PixelFormat.compressedTextureSizeInBytes(format,width,height):PixelFormat.textureSizeInBytes(format,5121,width,height);
  return{width,height,pixelFormat:format,sampler:options.sampler,sizeInBytes:size,flipY:options.flipY??true,generated:false,destroyed:false,
    generateMipmap(){this.generated=true;this.sizeInBytes=Math.floor(size*4/3);},isDestroyed(){return this.destroyed;},destroy(){this.destroyed=true;}};
 };
 const setup=c=>{
  Cache.clearForSpecs();config=c;log=[];raw=c.raw;
  loadKTX2.setKTX2SupportedFormats(!!c.s3tc,false,false,false,!!c.etc1,false);
  const resource=new Resource('https://image.test/models/model.gltf');
  const backing=new Uint8Array(raw.length+7);backing.set(raw,5);
  const gltf={buffers:[{byteLength:raw.length+3}],bufferViews:[{buffer:0,byteOffset:3,byteLength:raw.length}],images:[c.mode==='embedded'?{bufferView:0,mimeType:'deliberately/ignored'}:{uri:c.mode==='data'?`data:image/${c.name.includes('ktx2')?'ktx2':'png'};base64,${Buffer.from(raw).toString('base64')}`:`image.${c.name.includes('ktx2')?'KtX2':c.name}?variant=fixture`}],textures:[{source:0,sampler:0}],samplers:[{wrapS:c.wrap??10497,wrapT:c.wrap??10497,minFilter:c.min??9729}]};
  const seed=Cache.getEmbeddedBufferLoader({parentResource:resource,bufferId:0,typedArray:new Uint8Array(backing.buffer,2,raw.length+3)});
  return{resource,gltf,seed};
 };
 const imageInfo=l=>l.image?{width:l.image.width,height:l.image.height,format:l.image.internalFormat??null,bytes:l.image.bufferView?Array.from(l.image.bufferView):null,mips:l.mipLevels?.map(m=>Array.from(m))??null}:null;
 try{
 for(const spec of specs)for(const mode of ['embedded','uri','data'])for(const action of ['none','unload','destroy']){
  if(spec.name==='unknown'&&mode!=='embedded')continue;
  const c={...spec,mode,action,etc1:true};const {resource,gltf,seed}=setup(c);
  const loader=Cache.getImageLoader({gltf,imageId:0,gltfResource:resource,baseResource:resource});
  const initial=loader._state,promise=loader.load(),loading=loader._state;
  if(action==='destroy')Cache.unload(loader);
  let error=null,resolved;try{resolved=!!await promise;}catch(e){error=e.message;}
  const ready=imageInfo(loader),state=loader._state;if(action==='unload')loader.unload();
  const result={initial,loading,state,resolved:resolved??null,error,image:ready,after:imageInfo(loader),calls:action==='destroy'?[]:log,destroyed:loader.isDestroyed()};
  if(action!=='destroy')Cache.unload(loader);Cache.unload(seed);result.remaining=Object.keys(Cache.cacheEntries).length;
  images.cases.push({f:'lifecycle',a:[c],r:enc(result)});
 }
 for(const mode of ['embedded','uri'])for(const name of ['png','Green4x4.ktx2']){
  const c={...specs.find(s=>s.name===name),mode,action:'none',fail:true,etc1:true};const{resource,gltf,seed}=setup(c);
  const loader=Cache.getImageLoader({gltf,imageId:0,gltfResource:resource,baseResource:resource});
  const initial=loader._state,promise=loader.load(),loading=loader._state;let error=null,resolved;try{resolved=!!await promise;}catch(e){error=e.message;}
  const result={initial,loading,state:loader._state,resolved:resolved??null,error,image:imageInfo(loader),after:imageInfo(loader),calls:log,destroyed:false};Cache.unload(loader);Cache.unload(seed);result.remaining=Object.keys(Cache.cacheEntries).length;
  images.cases.push({f:'lifecycle',a:[c],r:enc(result)});
 }
 for(const name of ['png','Green4x4.ktx2','Green4x4Mipmap.ktx2','Green4x4_ETC1S.ktx2','Green4x4Mipmap_ETC1S.ktx2'])for(const min of [9729,9987])for(const wrap of [33071,10497])for(const asynchronous of [false,true]){
  const c={...specs.find(s=>s.name===name),mode:'embedded',action:'none',etc1:true,min,wrap,asynchronous};const{resource,gltf,seed}=setup(c);
  let allow=false;const frame={context:{id:'gpu',webgl2:false},jobScheduler:{execute(job){if(!allow)return false;job.execute();return true;}}};
  const loader=Cache.getTextureLoader({gltf,textureInfo:{index:0},gltfResource:resource,baseResource:resource,supportedImageFormats:{},frameState:frame,asynchronous});
  const initial=loader._state;await loader.load();const loaded=loader._state,first=loader.process(frame),deferred=loader._state;allow=true;const ready=loader.process(frame),texture=loader.texture;
  const result={initial,loaded,first,deferred,ready,state:loader._state,dimensions:[texture.width,texture.height],format:texture.pixelFormat,sampler:[texture.sampler.wrapS,texture.sampler.wrapT,texture.sampler.minificationFilter,texture.sampler.magnificationFilter],flipY:texture.flipY,generated:texture.generated,bytes:texture.sizeInBytes,statistics:Cache.statistics.texturesByteLength,calls:log};
  Cache.unload(loader);Cache.unload(seed);Object.assign(result,{destroyed:loader.isDestroyed(),textureDestroyed:texture.isDestroyed(),remaining:Object.keys(Cache.cacheEntries).length,finalStatistics:Cache.statistics.texturesByteLength});
  textures.cases.push({f:'lifecycle',a:[c],r:enc(result)});
 }
 }finally{Cache.clearForSpecs();ImageLoader._loadImageFromTypedArray=old.decode;Resource.prototype.fetchImage=old.image;Resource.prototype.fetchArrayBuffer=old.bytes;KTX2._readyPromise=old.ready;Texture.create=old.create;globalThis.document=old.document;globalThis.require=old.require;globalThis.__dirname=old.dirname;}
 return[images,textures];
}
