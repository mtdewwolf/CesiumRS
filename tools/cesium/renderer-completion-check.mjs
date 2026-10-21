import {chromium} from 'playwright-core';
import {build} from 'esbuild';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve,extname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('../..',import.meta.url)));
const renderer=['Context','CubeMap','Texture3D','Texture','Sampler','ShaderProgram','DrawCommand','ClearCommand','Framebuffer','PassState','TextureAtlas','VertexArrayFacade','UniformState','SharedContext','createUniform','createUniformArray','freezeRenderState','loadCubeMap','AutomaticUniforms','Buffer','RenderState'];
const core=['Color','Cartesian2','Cartesian3','Cartesian4','Matrix2','Matrix3','Matrix4','BoundingRectangle','Geometry','GeometryAttribute','PrimitiveType','ComponentDatatype','PixelFormat','PixelDatatype','Ellipsoid','GeographicProjection','WebMercatorProjection','Cartographic','PerspectiveFrustum','PerspectiveOffCenterFrustum','OrthographicFrustum','OrthographicOffCenterFrustum','JulianDate'];
const contents=[...renderer.map(n=>`export {default as ${n}} from '@cesium/engine/Source/Renderer/${n}.js';`),...core.filter(n=>n!=='PixelDatatype').map(n=>`export {default as ${n}} from '@cesium/engine/Source/Core/${n}.js';`),"export {default as PixelDatatype} from '@cesium/engine/Source/Renderer/PixelDatatype.js';","export {default as ViewportQuadVS} from '@cesium/engine/Source/Shaders/ViewportQuadVS.js';"].join('\n');
const bundle=await build({stdin:{contents,resolveDir:fileURLToPath(new URL('.',import.meta.url))},bundle:true,write:false,format:'esm',platform:'browser'});
const server=createServer(async(req,res)=>{try{const url=new URL(req.url,'http://localhost');if(url.pathname==='/reference.mjs'){res.setHeader('Content-Type','text/javascript');res.end(bundle.outputFiles[0].contents);return;}if(url.pathname==='/'){res.end('<!doctype html><title>Renderer completion checks</title>');return;}const path=resolve(root,'.'+url.pathname);if(!path.startsWith(root+'/'))throw Error('Invalid path');res.setHeader('Content-Type',({'.js':'text/javascript','.mjs':'text/javascript','.wasm':'application/wasm'})[extname(path)]??'application/octet-stream');res.end(await readFile(path));}catch(e){res.writeHead(404);res.end(e.message);}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));let browser;
try{
 browser=await chromium.launch({executablePath:process.env.CHROMIUM_PATH??'/usr/bin/chromium',headless:true,args:['--no-sandbox','--use-angle=swiftshader','--enable-unsafe-swiftshader']});const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}/`);
 const checks=await page.evaluate(async()=>{try{
  const wasm=await import('/target/renderer-completion/completion.js');await wasm.default();const ref=await import('/reference.mjs'),passed=[];
  const canvas=()=>{const c=document.createElement('canvas');c.width=8;c.height=8;return c;};
  const options={webgl:{antialias:false,alpha:true,stencil:true}};const gl=canvas().getContext('webgl2',options.webgl),context=new ref.Context(canvas(),options);
  const canonical=v=>Array.isArray(v)?v.map(canonical):v&&typeof v==='object'?Object.fromEntries(Object.keys(v).sort().map(k=>[k,canonical(v[k])])):v;
  const eq=(a,b,label)=>{if(JSON.stringify(canonical(a))!==JSON.stringify(canonical(b)))throw Error(label+': '+JSON.stringify(a)+' != '+JSON.stringify(b));passed.push(label);window.lastCheck=label;};
  const image=canvas();image.width=2;image.height=2;image.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray([255,0,0,255,0,255,0,255,0,0,255,255,255,255,255,255]),2,2),0,0);
  const faces=Array.from(ref.CubeMap.faceNames()),directions=[[1,0,0],[-1,0,0],[0,1,0],[0,-1,0],[0,0,1],[0,0,-1]];
  // Cesium's immutable typed volume allocation omits generated mip levels and
  // uses an unsized packed format. Use its mutable path / valid raw allocation
  // for reference pixels in those two corrected allocation cases.
  const cases=[{kind:'cube',blank:true,patch:true},{kind:'cube',fbCopy:true},{kind:'cube',mip:true,fbMip:true,lod:1},{kind:'volume',generate:true,lod:1},{kind:'volume',packed:true},{kind:'cube'},{kind:'cube',flip:false},{kind:'cube',patch:true},{kind:'cube',copy:true},{kind:'cube',dom:true},{kind:'cube',dom:true,flip:false},{kind:'cube',generate:true},...[0,1,2].map(lod=>({kind:'cube',mip:true,lod})),{kind:'volume'},{kind:'volume',flip:false},{kind:'volume',patch:true},...[0,1,2].map(lod=>({kind:'volume',mip:true,lod}))];
  for(const config of cases){const cube=config.kind==='cube',width=config.mip?4:2,flipY=config.flip??true;let texture;
   if(cube){const source=Object.fromEntries(faces.map((face,i)=>[face,config.dom?image:{width,height:width,arrayBufferView:new Uint8Array(Array.from({length:width*width},(_,p)=>[16+i*32,255-i*16-Math.floor(p/width)*8,i*20+p%width*8,255]).flat())}]));texture=new ref.CubeMap({context,source:config.blank?undefined:source,width:config.blank?width:undefined,height:config.blank?width:undefined,flipY,sampler:ref.Sampler.NEAREST});
    if(config.patch)texture.positiveX.copyFrom({source:{width:1,height:1,arrayBufferView:new Uint8Array([255,255,255,255])},xOffset:1,yOffset:1});
    if(config.copy){const t=new ref.Texture({context,source:{width,height:width,arrayBufferView:new Uint8Array(Array(width*width).fill([0,255,255,255]).flat())}});texture.copyFace({context},t,'negativeZ');t.destroy();}
    if(config.mip)texture.loadMipmaps([[2,[0,255,0,255]],[1,[0,0,255,255]]].map(([size,color])=>Object.fromEntries(faces.map(f=>[f,{width:size,height:size,arrayBufferView:new Uint8Array(Array(size*size).fill(color).flat())}]))));if(config.generate)texture.generateMipmap();
   }else{const data=config.mip?Array(64).fill([255,0,0,255]).flat():Array.from({length:8},(_,i)=>[20+i*20,40+i*10,200-i*10,255]).flat(),mipLevels=config.mip?[new Uint8Array(Array(8).fill([0,255,0,255]).flat()),new Uint8Array([0,0,255,255])]:undefined;if(config.generate||config.packed){const datatype=config.packed?32819:5121,bytes=config.packed?new Uint16Array([0xf00f,0x0f0f,0x00ff,0xffff,0xff0f,0xf0ff,0x0fff,0x888f]):new Uint8Array(data);texture=new ref.Texture3D({context,width,height:width,depth:width,pixelDatatype:config.packed?5121:datatype,flipY,sampler:ref.Sampler.NEAREST});if(config.packed){const driver=context._gl;driver.bindTexture(driver.TEXTURE_3D,texture._texture);driver.texImage3D(driver.TEXTURE_3D,0,driver.RGBA4,width,width,width,0,driver.RGBA,driver.UNSIGNED_SHORT_4_4_4_4,bytes);driver.bindTexture(driver.TEXTURE_3D,null);texture._pixelDatatype=datatype;texture._sizeInBytes=width*width*width*2;}else texture.copyFrom({source:{width,height:width,depth:width,arrayBufferView:bytes}});if(config.generate)texture.generateMipmap();}else texture=new ref.Texture3D({context,source:{width,height:width,depth:width,arrayBufferView:new Uint8Array(data),mipLevels},flipY,sampler:ref.Sampler.NEAREST});if(config.patch)texture.copyFrom({source:{width:1,height:1,depth:1,arrayBufferView:new Uint8Array([255,255,255,255])},xOffset:1,yOffset:1,zOffset:1});}
   if(config.fbCopy||config.fbMip){new ref.ClearCommand({color:new ref.Color(0,1,1,1)}).execute(context);if(config.fbCopy)texture.positiveX.copyFromFramebuffer(0,0,0,0,width,width);else for(const face of faces)texture[face].copyMipmapFromFramebuffer(0,0,2,2,1);}
   const result={dimensions:cube?[width,width]:[width,width,width],bytes:texture.sizeInBytes,flip:texture.flipY,premultiply:texture.preMultiplyAlpha,faces:[],pixels:[]};
   if(cube)for(const face of faces){const f=new ref.Framebuffer({context,colorTextures:[texture[face]],destroyAttachments:false});result.faces.push(Array.from(context.readPixels({framebuffer:f,width,height:width})));f.destroy();context._currentFramebuffer=undefined;}
   const shader=ref.ShaderProgram.fromCache({context,vertexShaderSource:ref.ViewportQuadVS,fragmentShaderSource:`uniform ${cube?'samplerCube':'sampler3D'} image;uniform vec3 coordinate;uniform float lod;void main(){out_FragColor=textureLod(image,coordinate,lod);}`,attributeLocations:{position:0,textureCoordinates:1}}),coords=cube?directions:Array.from({length:8},(_,i)=>[i&1?.75:.25,i&2?.75:.25,i&4?.75:.25]);
   for(const coordinate of coords){const command=new ref.DrawCommand({vertexArray:context.getViewportQuadVertexArray(),shaderProgram:shader,uniformMap:{image:()=>texture,coordinate:()=>new ref.Cartesian3(...coordinate),lod:()=>config.lod??0}});command.execute(context);result.pixels.push(Array.from(context.readPixels({width:1,height:1})));}
   eq(JSON.parse(wasm.textures(gl,JSON.stringify(config),image)),result,'cube/volume textures '+JSON.stringify(config));eq(gl.getError(),gl.NO_ERROR,'Rust texture GL status '+JSON.stringify(config));eq(context._gl.getError(),gl.NO_ERROR,'Cesium texture GL status '+JSON.stringify(config));shader.destroy();texture.destroy();context._currentFramebuffer=undefined;
  }
  const numeric=v=>{
    if(typeof v==='number')return [Math.fround(v)];
    if(v instanceof ref.Color)return [v.red,v.green,v.blue,v.alpha].map(Math.fround);
    if(v instanceof ref.Cartesian2)return [v.x,v.y].map(Math.fround);
    if(v instanceof ref.Cartesian3)return [v.x,v.y,v.z].map(Math.fround);
    if(v instanceof ref.Cartesian4)return [v.x,v.y,v.z,v.w].map(Math.fround);
    if(Array.isArray(v))return v.flatMap(numeric);
    return Array.from({length:v.length??(v instanceof ref.Matrix3?9:16)},(_,i)=>Math.fround(v[i]));
  };
  const near=(a,b,label)=>{if(a.length!==b.length||a.some((v,i)=>Math.abs(v-b[i])>2e-6*Math.max(1,Math.abs(b[i]))))throw Error(label+': '+JSON.stringify(a)+' != '+JSON.stringify(b));passed.push(label);window.lastCheck=label;};
  const frameCases=[{moveCamera:true},{mode:1,moveCamera:true},{}, {light:true},{mode:2,frustum:'orthoOffcenter'}, {mode:1,mercator:true}, {mode:0}, {frustum:'ortho'}, {frustum:'offcenter'}, {frustum:'orthoOffcenter'}, {sphere:true,noCartographic:true}, {noCartographic:true}, {infiniteDistance:true,subfrustum:true}, {date:'2000-01-01T12:00:00Z'}];
  for(const config of frameCases){
   const ellipsoid=config.sphere?ref.Ellipsoid.UNIT_SPHERE:ref.Ellipsoid.WGS84,mode=config.mode??3,cartographic=new ref.Cartographic(.4,.2,1000),position=mode===3?ellipsoid.cartographicToCartesian(cartographic):new ref.Cartesian3(100000,200000,300000),direction=mode===3?ref.Cartesian3.normalize(ref.Cartesian3.negate(position,new ref.Cartesian3()),new ref.Cartesian3()):new ref.Cartesian3(-1,0,0),right=ref.Cartesian3.normalize(ref.Cartesian3.cross(direction,ref.Cartesian3.UNIT_Z,new ref.Cartesian3()),new ref.Cartesian3()),up=ref.Cartesian3.cross(right,direction,new ref.Cartesian3()),view=ref.Matrix4.computeView(position,direction,up,right,new ref.Matrix4());
   const frusta={perspective:new ref.PerspectiveFrustum({fov:1.1,aspectRatio:4/3,near:2,far:12345,xOffset:.2,yOffset:-.1}),ortho:new ref.OrthographicFrustum({width:100,aspectRatio:4/3,near:2,far:12345}),offcenter:new ref.PerspectiveOffCenterFrustum({left:-1,right:2,top:3,bottom:-2,near:2,far:12345}),orthoOffcenter:new ref.OrthographicOffCenterFrustum({left:-50,right:50,top:40,bottom:-40,near:2,far:12345})};
   const frame={mode,mapProjection:new (config.mercator?ref.WebMercatorProjection:ref.GeographicProjection)(ellipsoid),pixelRatio:2,camera:{positionWC:position,directionWC:direction,rightWC:right,upWC:up,positionCartographic:config.noCartographic?undefined:cartographic,viewMatrix:view,inverseViewMatrix:ref.Matrix4.inverseTransformation(view,new ref.Matrix4()),frustum:frusta[config.frustum??'perspective']},time:ref.JulianDate.fromIso8601(config.date??'2025-01-02T03:04:05Z'),context:{defaultCubeMap:{},drawingBufferWidth:800},frameNumber:37,morphTime:.75,fog:{density:.001,visualDensityScalar:2,minimumBrightness:.1},atmosphere:{hueShift:.1,saturationShift:.2,brightnessShift:.3,lightIntensity:4,rayleighCoefficient:new ref.Cartesian3(.001,.002,.003),rayleighScaleHeight:10000,mieCoefficient:new ref.Cartesian3(.004,.005,.006),mieScaleHeight:3000,mieAnisotropy:.8,dynamicLighting:2},invertClassificationColor:new ref.Color(.4,.3,.2,.1),backgroundColor:new ref.Color(.1,.2,.3,.4),splitPosition:.3,maximumScreenSpaceError:8,minimumDisableDepthTestDistance:config.infiniteDistance?Infinity:100,specularEnvironmentMapsMaximumLOD:5,sphericalHarmonicCoefficients:Array.from({length:9},(_,i)=>new ref.Cartesian3(i*.1,i*.2,i*.3))};
   if(config.light)frame.light={direction:new ref.Cartesian3(1,2,3),color:new ref.Color(.3,.6,.9,1),intensity:4};
   const state=new ref.UniformState();state.viewport=new ref.BoundingRectangle(7,9,640,480);state.model=ref.Matrix4.fromArray([2,0,0,0,0,3,0,0,0,0,4,0,12,34,56,1]);state.gamma=2.2;state.update(frame);state.updatePass(8);
   if(config.subfrustum)state.updateFrustum(new ref.PerspectiveOffCenterFrustum({left:-1,right:1,top:1,bottom:-1,near:3,far:100}));
   if(config.moveCamera){const camera=frame.camera;camera.positionWC=ref.Cartesian3.add(camera.positionWC,new ref.Cartesian3(10,20,30),new ref.Cartesian3());camera.positionCartographic=undefined;camera.viewMatrix=ref.Matrix4.computeView(camera.positionWC,camera.directionWC,camera.upWC,camera.rightWC,new ref.Matrix4());camera.inverseViewMatrix=ref.Matrix4.inverseTransformation(camera.viewMatrix,new ref.Matrix4());state.updateCamera(camera);}

   const result=JSON.parse(wasm.uniform_frame(JSON.stringify(config)));
   for(const [name,uniform]of Object.entries(ref.AutomaticUniforms)){if([35678,35680].includes(uniform._datatype))continue;near(result[name],numeric(uniform.getValue(state)),'UniformState '+name+' '+JSON.stringify(config));}
  }
  {
   const atlas=new ref.TextureAtlas({initialSize:new ref.Cartesian2(4,4)}),image=(width,height,color)=>({width,height,arrayBufferView:new Uint8Array(Array(width*height).fill(color).flat())});
   const settle=async()=>{for(let i=0;i<5;i++)await Promise.resolve();};
   const a=atlas.addImage('a',image(2,2,[255,0,0,255])),b=atlas.addImage('b',image(1,3,[0,255,0,255]));await settle();atlas.update(context);const indices=[await a,await b];
   const c=atlas.addImage('c',image(5,3,[0,0,255,255])),sub=atlas.addImageSubRegion('a',new ref.BoundingRectangle(1,0,1,2));await settle();atlas.update(context);indices.push(await c,await sub);
   const texture=atlas.texture,f=new ref.Framebuffer({context,colorTextures:[texture],destroyAttachments:false});
   const result={indices,coords:Array.from({length:atlas.numberOfImages},(_,i)=>{const r=atlas.computeTextureCoordinates(i);return [r.x,r.y,r.width,r.height];}),size:[texture.width,texture.height],bytes:atlas.sizeInBytes,pixels:Array.from(context.readPixels({framebuffer:f,width:texture.width,height:texture.height}))};
   eq(JSON.parse(await wasm.atlas(gl)),result,'TextureAtlas packing, repacking, subregions and preserved GPU pixels');f.destroy();atlas.destroy();context._currentFramebuffer=undefined;
  }
  for(const config of [{},{subcommit:true},{resize:true},{indexed:true},{indexed:true,size:65532},{indexed:true,size:65535}]){
   const size=config.size??3,facade=new ref.VertexArrayFacade(context,[{index:0,componentsPerAttribute:2},{index:1,componentsPerAttribute:4,componentDatatype:5121,normalize:true},{index:2,componentsPerAttribute:2,componentDatatype:5123},{index:3,componentsPerAttribute:1,usage:35048}],size);
   const write=()=>{for(let i=0;i<size;i++){facade.writers[0](i,i===1?3:-1,i===2?3:-1);facade.writers[1](i,255,128,64,255);facade.writers[2](i,i,1000+i);facade.writers[3](i,i+.5);}};write();
   const indices=config.indexed?ref.Buffer.createIndexBuffer({context,typedArray:new Uint16Array([0,1,2]),usage:35044,indexDatatype:5123}):undefined;facade.commit(indices);
   if(config.subcommit){facade.writers[1](1,0,255,255,255);facade.subCommit(1,1);facade.endSubCommits();facade.commit(indices);}
   if(config.resize){facade.resize(size*2);facade.writers[0](size,0,0);facade.commit(indices);}
   const arrays=facade.va.map(({va,indicesCount})=>({count:indicesCount,attributes:Array.from({length:va.numberOfAttributes},(_,i)=>{const a=va.getAttribute(i);return [a.index,a.componentsPerAttribute,a.componentDatatype,a.normalize,a.offsetInBytes,a.strideInBytes,a.instanceDivisor];})})),va=facade.va[0].va,buffers=[];
   for(let i=0;i<va.numberOfAttributes;i++){const a=va.getAttribute(i);if(a.offsetInBytes===0 && size<100){const bytes=new Uint8Array(a.vertexBuffer.sizeInBytes);a.vertexBuffer.getBufferData(bytes);buffers.push(Array.from(bytes));}}
   let pixels=[];
   if(size<100){const shader=ref.ShaderProgram.fromCache({context,vertexShaderSource:'in vec2 position;in vec4 color;out vec4 v;void main(){gl_Position=vec4(position,0.,1.);v=color;}',fragmentShaderSource:'in vec4 v;void main(){out_FragColor=v;}',attributeLocations:{position:0,color:1}});new ref.DrawCommand({vertexArray:va,shaderProgram:shader,count:3}).execute(context);pixels=Array.from(context.readPixels({width:1,height:1}));shader.destroy();}
   eq(JSON.parse(wasm.facade(gl,JSON.stringify(config))),{arrays,buffers,pixels},'VertexArrayFacade '+JSON.stringify(config));facade.destroy();if(indices && !indices.isDestroyed())indices.destroy();
  }
  for(const webgl1 of [false,true]) {
   const rc=canvas(),reference=new ref.Context(rc,{...options,requestWebgl1:webgl1}),raw=canvas().getContext(webgl1?'webgl':'webgl2',options.webgl);
   // Pinned Context queries MAX_3D_TEXTURE_SIZE unconditionally in WebGL1.
   if(webgl1){if(reference._gl.getError()!==1280 || reference._gl.getError()!==0)throw Error('Unexpected WebGL1 constructor errors');}
   const colors=[reference.defaultTexture,reference.defaultEmissiveTexture,reference.defaultNormalTexture].map(texture=>{const command=reference.createViewportQuadCommand('uniform sampler2D image;void main(){out_FragColor=texture(image,vec2(0.5));}',{uniformMap:{image:()=>texture}});command.execute(reference);return Array.from(reference.readPixels({width:1,height:1}));});
   const a=reference.createPickId(17),b=reference.createPickId(42),picks=[Array.from(a.color.toBytes()),Array.from(b.color.toBytes())];a.destroy();const found=reference.getObjectByPickColor(b.key);
   reference.validateShaderProgram=true;reference.validateFramebuffer=true;reference.throwOnWebGLError=true;
   reference.uniformState._backgroundColor=new ref.Color(.25,.5,.75,1);const command=reference.createViewportQuadCommand('void main(){out_FragColor=czm_backgroundColor;}');command.execute(reference);const pixels=Array.from(reference.readPixels({width:1,height:1})),sameQuad=command.vertexArray===reference.getViewportQuadVertexArray();reference.endFrame();
   const debugTarget=new ref.Texture({context:reference,width:2,height:2}),debugFramebuffer=new ref.Framebuffer({context:reference,colorTextures:[debugTarget]});reference.createViewportQuadCommand('void main(){out_FragColor=czm_backgroundColor;}',{framebuffer:debugFramebuffer,renderState:ref.RenderState.fromCache({viewport:new ref.BoundingRectangle(0,0,2,2)})}).execute(reference);const debugPixels=Array.from(reference.readPixels({framebuffer:debugFramebuffer,width:1,height:1}));if(JSON.stringify(debugPixels)!==JSON.stringify(pixels))throw Error('Debug target validation changed pixels');debugFramebuffer.destroy();reference._currentFramebuffer=undefined;
   const result={colors,picks,found,pixels,sameQuad,webgl2:reference.webgl2,max3D:webgl1?0:raw.getParameter(raw.MAX_3D_TEXTURE_SIZE),stencilBits:reference.stencilBits,antialias:reference.antialias};
   eq(JSON.parse(wasm.context_api(raw,webgl1)),result,'Context defaults, picking, viewport commands and WebGL'+(webgl1?'1':'2'));
   eq(raw.getError(),0,'Context GL status');
   if(webgl1){const fallback=canvas().getContext('webgl',options.webgl),proxy=new Proxy(fallback,{get(t,p){if(p==='getExtension')return name=>['OES_vertex_array_object','ANGLE_instanced_arrays','WEBGL_draw_buffers'].includes(name)?null:t.getExtension(name);const v=Reflect.get(t,p,t);return typeof v==='function'?v.bind(t):v;}});eq(JSON.parse(wasm.context_api(proxy,true)),result,'WebGL1 software VAOs without optional extensions');eq(fallback.getError(),0,'WebGL1 fallback GL status');}
   reference.destroy();
  }
  for(const autoDestroy of [true,false]){
   const surfaces=()=>[[6,5],[4,3]].map(([w,h])=>{const c=canvas();c.width=w;c.height=h;return c;}),actual=surfaces(),expected=surfaces(),raw=canvas().getContext('webgl2',options.webgl),shared=new ref.SharedContext({contextOptions:options,autoDestroy}),a=shared.createSceneContext(expected[0]),b=shared.createSceneContext(expected[1]);
   let duplicate=false;try{shared.createSceneContext(expected[0]);}catch{duplicate=true;}
   a.beginFrame();new ref.ClearCommand({color:ref.Color.RED}).execute(a);a.endFrame();b.beginFrame();new ref.ClearCommand({color:new ref.Color(0,1,0,1)}).execute(b);
   new ref.ClearCommand({color:ref.Color.BLUE,renderState:ref.RenderState.fromCache({scissorTest:{enabled:true,rectangle:new ref.BoundingRectangle(0,0,4,2)}})}).execute(b);b.endFrame();a.destroy();const afterFirst=shared.isDestroyed();b.destroy();const result={duplicate,afterFirst,afterLast:shared.isDestroyed(),active:0};
   eq(JSON.parse(wasm.shared(raw,actual,autoDestroy)),result,'SharedContext lifecycle '+autoDestroy);
   for(let i=0;i<2;i++)eq(Array.from(actual[i].getContext('2d').getImageData(0,0,actual[i].width,actual[i].height).data),Array.from(expected[i].getContext('2d').getImageData(0,0,expected[i].width,expected[i].height).data),'SharedContext canvas crop '+autoDestroy+'/'+i);
   if(!autoDestroy)shared.destroy();
  }
  const factoryGl=canvas().getContext('webgl2',options.webgl);
  const factoryTypes=[['float',5126,1],['vec2',35664,2],['vec3',35665,3],['vec4',35666,4],['int',5124,1],['ivec2',35667,2],['ivec3',35668,3],['ivec4',35669,4],['bool',35670,1],['bvec2',35671,2],['bvec3',35672,3],['bvec4',35673,4],['mat2',35674,2],['mat3',35675,3],['mat4',35676,4]];
  for(const [type,datatype,components]of factoryTypes)for(const array of [false,true]){
   const matrix=type.startsWith('mat'),signed=['i','b'].includes(type[0]),width=matrix?components*components:components,values=Array.from({length:width*(array?2:1)},(_,i)=>signed?i%2:(i+1)/40),v=array?'(value[0] + value[1])':'value',expr=matrix?`${v}[0]`:v,cast=signed?(components===1?`float(${expr})`:`vec${components}(${expr})`):expr;
   // Bool arrays use logical OR because GLSL does not add booleans.
   const selected=type.startsWith('b')&&array?'value[1]':expr,converted=signed?(components===1?`float(${selected})`:`vec${components}(${selected})`):expr;
   const output=components===1?`vec4(${converted},0.,0.,1.)`:components===2?`vec4(${converted},0.,1.)`:components===3?`vec4(${converted},1.)`:converted;
   const shader=`uniform ${type} value${array?'[2]':''};void main(){out_FragColor=${output};}`,config={shader,datatype,components,array,matrix,signed,values};
   const rg=factoryGl,program=rg.createProgram(),compile=(type,source)=>{const s=rg.createShader(type);rg.shaderSource(s,source);rg.compileShader(s);if(!rg.getShaderParameter(s,rg.COMPILE_STATUS))throw Error(rg.getShaderInfoLog(s));rg.attachShader(program,s);};
   compile(rg.VERTEX_SHADER,'#version 300 es\nvoid main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);gl_Position=vec4(p*2.-1.,0.,1.);}');compile(rg.FRAGMENT_SHADER,'#version 300 es\nprecision highp float;out vec4 out_FragColor;'+shader);rg.linkProgram(program);if(!rg.getProgramParameter(program,rg.LINK_STATUS))throw Error(rg.getProgramInfoLog(program));rg.useProgram(program);
   const locations=Array.from({length:array?2:1},(_,i)=>rg.getUniformLocation(program,'value'+(array?`[${i}]`:''))),binding=(array?ref.createUniformArray:ref.createUniform)(rg,{type:datatype,size:array?2:1},'value',array?locations:locations[0]);
   const convert=values=>components===1?values[0]:matrix?ref['Matrix'+components].fromArray(values):new ref['Cartesian'+components](...values);
   binding.value=array?[convert(values.slice(0,width)),convert(values.slice(width))]:convert(values);binding.set();binding.set();rg.drawArrays(rg.TRIANGLES,0,3);const pixel=new Uint8Array(4);rg.readPixels(0,0,1,1,rg.RGBA,rg.UNSIGNED_BYTE,pixel);
   eq(JSON.parse(wasm.factory(gl,JSON.stringify(config))),Array.from(pixel),'Uniform factory '+type+(array?' array':''));eq(gl.getError(),0,'Uniform factory GL status '+type+'/'+array);
  }
  for(const defaultEnvironment of [false,true]){
   const source='uniform sampler2D image;void main(){vec2 p=vec2(0.5);vec3 d=vec3(1.,0.,0.);out_FragColor=(texture(image,p)+texture(czm_globeDepthTexture,p)+texture(czm_edgeIdTexture,p)+texture(czm_edgeColorTexture,p)+texture(czm_edgeDepthTexture,p)+texture(czm_brdfLut,p)+texture(czm_environmentMap,d)+texture(czm_specularEnvironmentMaps,d))/8.;}',state=context.uniformState,leases=[];
   for(const [name,color]of [['globeDepthTexture',[255,0,0,255]],['edgeIdTexture',[0,255,0,255]],['edgeColorTexture',[0,0,255,255]],['edgeDepthTexture',[255,255,0,255]],['_brdfLut',[0,255,255,255]]]){const texture=new ref.Texture({context,source:{width:1,height:1,arrayBufferView:new Uint8Array(color)}});leases.push(texture);state[name]=texture;}
   for(const [name,color]of [['_environmentMap',[255,0,255,255]],['_specularEnvironmentMaps',[128,128,128,255]]]){const texture=new ref.CubeMap({context,source:Object.fromEntries(faces.map(f=>[f,{width:1,height:1,arrayBufferView:new Uint8Array(color)}]))});leases.push(texture);state[name]=defaultEnvironment&&name==='_environmentMap'?context.defaultCubeMap:texture;}
   const command=context.createViewportQuadCommand(source,{uniformMap:{image:()=>context.defaultTexture}});command.execute(context);const pixels=Array.from(context.readPixels({width:1,height:1}));
   eq(JSON.parse(wasm.automatic_textures(gl,defaultEnvironment)),pixels,'All seven automatic samplers '+defaultEnvironment);eq(gl.getError(),0,'Automatic sampler GL status');leases.forEach(t=>t.destroy());command.shaderProgram.destroy();
  }

  for(const skip of [false,true]){
   const urls=Object.fromEntries(faces.map(f=>[f,image.toDataURL()])),cube=await ref.loadCubeMap(context,urls,skip),pixels=faces.map(face=>{const f=new ref.Framebuffer({context,colorTextures:[cube[face]],destroyAttachments:false}),bytes=Array.from(context.readPixels({framebuffer:f,width:2,height:2}));f.destroy();context._currentFramebuffer=undefined;return bytes;});
   eq(JSON.parse(await wasm.cube_loader(gl,image.toDataURL(),skip)),pixels,'loadCubeMap Resource decode, face order and ImageBitmap flip '+skip);cube.destroy();
  }
  let rejected=false;try{await wasm.cube_loader(gl,location.origin+'/missing-cube.png',false);}catch{rejected=true;}eq(rejected,true,'loadCubeMap image errors reject');
  return passed;
 }catch(e){throw Error(String(e?.message??e)+'\n'+e?.stack+'\nLast check: '+window.lastCheck);}});
 console.log(checks.map(s=>'PASS '+s).join('\n'));console.log(`${checks.length} Renderer completion checks passed`);
}finally{await browser?.close();await new Promise(r=>server.close(r));}
