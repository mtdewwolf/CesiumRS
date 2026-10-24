import {Resource} from '@cesium/engine';
import Key from '@cesium/engine/Source/Scene/ResourceCacheKey.js';
import Util from '@cesium/engine/Source/Scene/GltfLoaderUtil.js';
import Statistics from '@cesium/engine/Source/Scene/ResourceCacheStatistics.js';
import Cache from '@cesium/engine/Source/Scene/ResourceCache.js';
import {suite} from './lib.mjs';
export default function resourceCache(){
 const keys=suite('ResourceCacheKey'),util=suite('GltfLoaderUtil'),stats=suite('ResourceCacheStatistics'),pool=suite('ResourceCache');
 const resource=new Resource('https://cache.test/models/model.gltf?fixture=sample'),base=new Resource('https://cache.test/models/');
 const document={buffers:[{uri:'mesh.bin?x=1'},{byteLength:100}],bufferViews:[{buffer:0,byteOffset:8,byteLength:24},{buffer:1,byteOffset:16,byteLength:32},{buffer:0,byteOffset:0,byteLength:64,extensions:{EXT_meshopt_compression:{buffer:1,byteOffset:6,byteLength:18}}}],accessors:[{bufferView:0,byteOffset:4,componentType:5123,type:'SCALAR',count:6}],images:[{uri:'image.png'},{bufferView:1},{uri:'image.webp'},{uri:'image.ktx2'}],textures:[{source:0},{source:1,sampler:0,extensions:{EXT_texture_webp:{source:2},KHR_texture_basisu:{source:3}}}],samplers:[{wrapS:33071,wrapT:33648,minFilter:9986,magFilter:9728}]};
 const invoke=(method,config)=>{const options={gltf:config.document??document,gltfResource:resource,baseResource:base,resource,parentResource:resource,frameState:{context:{id:config.context??'a'}},supportedImageFormats:{webp:!!config.webp,basis:!!config.basis},...config};delete options.document;if(method==='getSchemaCacheKey'&&config.schema)delete options.resource;return Key[method](options);};
 for(const schema of [{classes:{}},{'2':2,'1':1,z:[1,1e-7,1e21,-0],'01':'x'}])keys.add('getSchemaCacheKey',[{schema}],c=>invoke('getSchemaCacheKey',c));
 for(const method of ['getSchemaCacheKey','getExternalBufferCacheKey','getGltfCacheKey'])keys.add(method,[{}],c=>invoke(method,c));
 for(const bufferId of [0,1,12])keys.add('getEmbeddedBufferCacheKey',[{bufferId}],c=>invoke('getEmbeddedBufferCacheKey',c));
 for(const bufferViewId of [0,1,2])keys.add('getBufferViewCacheKey',[{bufferViewId}],c=>invoke('getBufferViewCacheKey',c));
 for(const bufferView of [0,1,2])keys.add('getDracoCacheKey',[{draco:{bufferView,attributes:{POSITION:1}}}],c=>invoke('getDracoCacheKey',c));
 keys.add('getSpzCacheKey',[{primitive:{}}],c=>invoke('getSpzCacheKey',c));
 for(const context of ['a','b'])for(const loadBuffer of [false,true])for(const loadTypedArray of [false,true]){
  if(!loadBuffer&&!loadTypedArray)continue;
  for(const dequantize of [false,true])for(const source of [{bufferViewId:0},{bufferViewId:1},{bufferViewId:2},{draco:{bufferView:0,attributes:{POSITION:0}},attributeSemantic:'POSITION'},{spz:{},attributeSemantic:'_SCALE'}])keys.add('getVertexBufferCacheKey',[{context,loadBuffer,loadTypedArray,dequantize,...source}],c=>invoke('getVertexBufferCacheKey',c));
  for(const draco of [undefined,{bufferView:0,attributes:{POSITION:0}}])keys.add('getIndexBufferCacheKey',[{context,loadBuffer,loadTypedArray,accessorId:0,...(draco?{draco}:{})}],c=>invoke('getIndexBufferCacheKey',c));
 }
 for(const imageId of [0,1,2])keys.add('getImageCacheKey',[{imageId}],c=>invoke('getImageCacheKey',c));
 for(const context of ['a','b'])for(const webp of [false,true])for(const basis of [false,true])for(const index of [0,1])keys.add('getTextureCacheKey',[{context,webp,basis,textureInfo:{index}}],c=>invoke('getTextureCacheKey',c));
 for(const webp of [false,true])for(const basis of [false,true])for(const textureId of [0,1])util.add('image',[document,textureId,webp,basis],(gltf,textureId,webp,basis)=>Util.getImageIdFromTexture({gltf,textureId,supportedImageFormats:{webp,basis}}));
 for(const minFilter of [9728,9729,9984,9985,9986,9987])for(const compressedTextureNoMipmap of [false,true])util.add('sampler',[minFilter,compressedTextureNoMipmap],(minFilter,compressedTextureNoMipmap)=>{const s=Util.createSampler({gltf:{textures:[{sampler:0}],samplers:[{minFilter}]},textureInfo:{index:0},compressedTextureNoMipmap});return[s.wrapS,s.wrapT,s.minificationFilter,s.magnificationFilter];});
 util.add('sampler-default',[],()=>{const s=Util.createSampler({gltf:{textures:[{}]},textureInfo:{index:0}});return[s.wrapS,s.wrapT,s.minificationFilter,s.magnificationFilter];});
 for(const rotation of [0,.5,-2,Math.PI])for(const transform of [false,true])util.add('reader',[rotation,transform],(rotation,transform)=>{const r=Util.createModelTextureReader({textureInfo:{index:2,texCoord:1,scale:.3,...(transform?{extensions:{KHR_texture_transform:{offset:[.2,-.4],scale:[2,.5],rotation,texCoord:3}}}:{})},channels:'rgb'});return{index:r.index,texCoord:r.texCoord,scale:r.scale,transform:r.transform?Array.from(r.transform):null,channels:r.channels};});
 const operations=[['geometry','a',8,4],['geometry','a',100,100],['texture','a',16],['geometry','b',0,0],['remove','absent'],['remove','a'],['texture','b',4],['clear'],['remove','b']];
 stats.add('sequence',[operations],ops=>{const s=new Statistics();return ops.map(([op,key,a,b])=>{const loader={cacheKey:key,...(a?{buffer:{sizeInBytes:a}}:{}),...(b?{typedArray:{byteLength:b}}:{})};if(op==='geometry')s.addGeometryLoader(loader);if(op==='texture')s.addTextureLoader({cacheKey:key,texture:{sizeInBytes:a}});if(op==='remove')s.removeLoader(loader);if(op==='clear')s.clear();return[s.geometryByteLength,s.texturesByteLength];});});
 const cases=[['add:a','get:a','unload:a','unload:a'],['add:a','add:b','get:a','clear'],['get:absent','add:a','unload:a','add:a','clear'],['add:a','get:a','get:a','unload:a','clear']];
 for(const ops of cases)pool.add('sequence',[ops],ops=>{Cache.clearForSpecs();const loaders=[],handles={};const trace=[];for(const operation of ops){const [op,key]=operation.split(':');if(op==='add'){const loader={cacheKey:key,destroyed:false,destroy(){this.destroyed=true;},isDestroyed(){return this.destroyed;}};loaders.push(loader);handles[key]=Cache.add(loader);}if(op==='get')Cache.get(key);if(op==='unload')Cache.unload(handles[key]);if(op==='clear')Cache.clearForSpecs();trace.push({entries:Object.fromEntries(Object.keys(Cache.cacheEntries).sort().map(k=>[k,Cache.cacheEntries[k].referenceCount])),destroyed:loaders.map(l=>l.destroyed)});}Cache.clearForSpecs();return trace;});
 for(const c of keys.cases)c.a[0].document=document;
 return[keys,util,stats,pool];
}
