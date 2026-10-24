// Real PNG/JPEG/WebP decoding and glTF GPU uploads. KTX2's worker boundary
// returns goldens from the actual pinned transcodeKTX2 worker fixture suite.
export async function run(wasm,ref,canvas,attributes){
 const passed=[],goldens=(await(await fetch('/tests/fixtures/core/KTX2Transcoder.json')).json()).cases;
 const raster=[];for(const format of ['png','jpeg','webp']){
  const c=document.createElement('canvas');c.width=3;c.height=5;const ctx=c.getContext('2d'),pixels=ctx.createImageData(3,5);
  for(let y=0;y<5;y++)for(let x=0;x<3;x++)pixels.data.set([30+x*80,20+y*40,210-y*23,(x+y)%2?255:160],4*(y*3+x));ctx.putImageData(pixels,0,0);
  const blob=await new Promise(r=>c.toBlob(r,`image/${format}`,0.9));if(blob.type!==`image/${format}`)throw Error('Missing browser codec '+format);
  const raw=Array.from(new Uint8Array(await blob.arrayBuffer()));const uri=`/image-fixtures/test.${format}`;
  const response=await fetch(uri,{method:'POST',body:new Uint8Array(raw)});if(!response.ok)throw Error('Image fixture upload failed');raster.push({name:format,raw,uri});
 }
 const ktx=[];for(const name of ['Green4x4.ktx2','Green4x4Mipmap.ktx2','Green4x4_ETC1S.ktx2','Green4x4Mipmap_ETC1S.ktx2']){
  const golden=goldens.find(c=>c.a[0]===name&&JSON.stringify(c.a[1])===JSON.stringify(name.includes('ETC1S')?{s3tc:true}:{}));
  if(!golden||!Array.isArray(golden.r))throw Error('Missing KTX2 worker fixture '+name);
  const uri='/tests/data/ktx2/'+name,raw=Array.from(new Uint8Array(await(await fetch(uri)).arrayBuffer()));ktx.push({name,raw,uri,golden:golden.r,s3tc:name.includes('ETC1S')});
 }
 const pattern=goldens.find(c=>c.a[0]===null&&Array.isArray(c.r)&&c.r.length===3&&c.r.every(l=>l.length===1&&l[0][1]===6408&&l[0][2]===5121));
 if(!pattern)throw Error('Missing multi-level RGBA KTX2 worker fixture');ktx.push({name:'pattern.ktx2',raw:pattern.a[2],golden:pattern.r,uri:'/image-fixtures/pattern.ktx2',scale:4});
 await fetch('/image-fixtures/pattern.ktx2',{method:'POST',body:new Uint8Array(pattern.a[2])});
 const oldReady=ref.KTX2Transcoder._readyPromise;
 try{
 for(const spec of [...raster,...ktx])for(const webgl1 of [false,true])for(const embedded of [false,true])for(const min of [9729,9987])for(const wrap of [33071,10497]){
  ref.ResourceCache.clearForSpecs();window.lastSceneCase={texture:true,name:spec.name,webgl1,embedded,min,wrap};
  const gl=canvas().getContext(webgl1?'webgl':'webgl2',attributes),context=new ref.Context(canvas(),{webgl:attributes,requestWebgl1:webgl1});
  const initError=context._gl.getError();if(initError!==0&&!(webgl1&&initError===1280))throw Error('Unexpected context initialization error '+initError);
  if(spec.s3tc&&!context.s3tc)throw Error('S3TC is required for the model texture validation');
  if(spec.golden){
   ref.loadKTX2.setKTX2SupportedFormats(!!spec.s3tc,false,false,false,false,false);
   ref.KTX2Transcoder._readyPromise=Promise.resolve({scheduleTask(parameters){
    if(JSON.stringify(Array.from(parameters.ktx2Buffer))!==JSON.stringify(spec.raw))throw Error('KTX2 loader sent different bytes');
    return Promise.resolve(spec.golden.map(level=>Object.fromEntries(level.map(([face,internalFormat,datatype,width,height,bytes])=>[face,{internalFormat,datatype,width,height,levelBuffer:Uint8Array.from(bytes)}]))));
   }});
  }
  const resource=new ref.Resource(location.origin+'/models/model.gltf');
  const gltf={buffers:[{byteLength:spec.raw.length}],bufferViews:[{buffer:0,byteOffset:0,byteLength:spec.raw.length}],images:[embedded?{bufferView:0}:{uri:spec.uri+'?variant=fixture'}],textures:[{source:0,sampler:0}],samplers:[{wrapS:wrap,wrapT:wrap,minFilter:min}]};
  const seed=ref.ResourceCache.getEmbeddedBufferLoader({parentResource:resource,bufferId:0,typedArray:Uint8Array.from(spec.raw)});
  let allow=false;const frame={context,jobScheduler:{execute(job){if(!allow)return false;job.execute();return true;}}};
  const options={gltf,textureInfo:{index:0},gltfResource:resource,baseResource:resource,supportedImageFormats:{},frameState:frame,asynchronous:true};
  const loader=ref.ResourceCache.getTextureLoader(options),duplicate=ref.ResourceCache.getTextureLoader(options);await loader.load();
  const loadedEntries=Object.keys(ref.ResourceCache.cacheEntries).length,first=loader.process(frame);allow=true;const ready=loader.process(frame),texture=loader.texture;
  const processedEntries=Object.keys(ref.ResourceCache.cacheEntries).length,sampler=texture.sampler,dimensions=[texture.width,texture.height],format=texture.pixelFormat,flipY=texture.flipY,bytes=texture.sizeInBytes,statistics=ref.ResourceCache.statistics.texturesByteLength;
  const scale=spec.scale??2;
  const command=context.createViewportQuadCommand('in vec2 v_textureCoordinates;uniform sampler2D image;uniform float scale;void main(){out_FragColor=texture(image,v_textureCoordinates*scale);}',{uniformMap:{image:()=>texture,scale:()=>scale},renderState:ref.RenderState.fromCache({})});
  const pass=new ref.PassState(context);new ref.ClearCommand({color:ref.Color.BLACK,depth:1,stencil:0}).execute(context,pass);command.execute(context,pass);const pixels=Array.from(context.readPixels({width:8,height:8}));
  ref.ResourceCache.unload(loader);const aliveWithLease=!texture.isDestroyed();ref.ResourceCache.unload(duplicate);ref.ResourceCache.unload(seed);
  const expected={shared:loader===duplicate,loadedEntries,processedEntries,first,ready,dimensions,format,flipY,preMultiplyAlpha:texture.preMultiplyAlpha,sampler:[sampler.wrapS,sampler.wrapT,sampler.minificationFilter,sampler.magnificationFilter],bytes,statistics,pixels,aliveWithLease,destroyed:texture.isDestroyed(),remaining:Object.keys(ref.ResourceCache.cacheEntries).length,finalStatistics:ref.ResourceCache.statistics.texturesByteLength};
  const config={resource:resource.url,gltf,raw:spec.raw,webgl1,asynchronous:true,s3tc:!!spec.s3tc,scale};
  const actual=JSON.parse(await wasm.scene_image_texture(gl,JSON.stringify(config)));
  for(const key of Object.keys(expected))if(JSON.stringify(actual[key])!==JSON.stringify(expected[key]))throw Error(JSON.stringify({case:window.lastSceneCase,key,actual:actual[key],expected:expected[key]}));
  const actualError=gl.getError(),expectedError=context._gl.getError();
  // The pinned RGB mip path retains base-level alignment, so the tightly
  // packed 2x2 RGB level in this upstream asset produces INVALID_OPERATION.
  const rgbMipAlignment=spec.name==='Green4x4Mipmap.ktx2'&&expectedError===1282;
  if(actualError!==expectedError||expectedError!==0&&!rgbMipAlignment)throw Error(JSON.stringify({case:window.lastSceneCase,actualError,expectedError}));
  passed.push({...window.lastSceneCase,glError:expectedError});const referenceGl=context._gl;context.destroy();gl.getExtension('WEBGL_lose_context')?.loseContext();referenceGl.getExtension('WEBGL_lose_context')?.loseContext();
 }
 }finally{ref.ResourceCache.clearForSpecs();ref.KTX2Transcoder._readyPromise=oldReady;}
 return passed;
}
