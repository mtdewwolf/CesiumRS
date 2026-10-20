// Every test uses independent WebGL2 contexts for Rust and the pinned CesiumJS.
import {chromium} from 'playwright-core';
import {build} from 'esbuild';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve,extname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('../..',import.meta.url)));
const names=['Buffer','BufferUsage','VertexArray','Context','ShaderProgram','ShaderSource','Texture','Sampler','RenderState','DrawCommand','ClearCommand','PassState','Framebuffer','Renderbuffer','RenderbufferFormat','ShaderBuilder','ShaderCache','ContextLimits'];
const core=['ComponentDatatype','IndexDatatype','PrimitiveType','Color','WebGLConstants','Geometry','GeometryAttributes','GeometryAttribute','Cartesian2','Cartesian3','Cartesian4','Matrix2','Matrix3','Matrix4'];
const contents=[...names.map(n=>`export {default as ${n}} from '@cesium/engine/Source/Renderer/${n}.js';`),...core.map(n=>`export {default as ${n}} from '@cesium/engine/Source/Core/${n}.js';`)].join('\n');
const bundle=await build({stdin:{contents,resolveDir:fileURLToPath(new URL('.',import.meta.url))},bundle:true,write:false,format:'esm',platform:'browser'});
const server=createServer(async(req,res)=>{try{
 const url=new URL(req.url,'http://localhost');
 if(url.pathname==='/reference.mjs'){res.setHeader('Content-Type','text/javascript');res.end(bundle.outputFiles[0].contents);return;}
 if(url.pathname==='/'){res.setHeader('Content-Type','text/html');res.end('<!doctype html><title>Renderer parity</title>');return;}
 const path=resolve(root,'.'+url.pathname);if(!path.startsWith(root+'/'))throw Error('invalid path');
 res.setHeader('Content-Type',({'.mjs':'text/javascript','.js':'text/javascript','.wasm':'application/wasm'})[extname(path)]??'application/octet-stream');res.end(await readFile(path));
 }catch(e){res.writeHead(404);res.end(e.message);}});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
let browser;
try {
 browser=await chromium.launch({executablePath:process.env.CHROMIUM_PATH??'/usr/bin/chromium',headless:true,args:['--no-sandbox','--use-angle=swiftshader','--enable-unsafe-swiftshader']});
 const page=await browser.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.goto(`http://127.0.0.1:${server.address().port}/`);
 const checks=await page.evaluate(async()=>{try{
  const wasm=await import('/target/renderer-validation/renderer.js');await wasm.default();const ref=await import('/reference.mjs'),passed=[];
  const canvas=()=>{const c=document.createElement('canvas');c.width=8;c.height=8;return c;};
  const gl=canvas().getContext('webgl2',{antialias:false,alpha:true,stencil:true});if(!gl)throw Error('WebGL2 unavailable');
  const context=new ref.Context(canvas(),{webgl:{antialias:false,alpha:true,stencil:true}});
  const canonical=v=>Array.isArray(v)?v.map(canonical):v&&typeof v==='object'?Object.fromEntries(Object.keys(v).sort().map(k=>[k,canonical(v[k])])):v;
  const eq=(a,b,label)=>{if(JSON.stringify(canonical(a))!==JSON.stringify(canonical(b)))throw Error(label+': '+JSON.stringify(a)+' != '+JSON.stringify(b));passed.push(label);};
  const b=ref.Buffer.createVertexBuffer({context,usage:ref.BufferUsage.DYNAMIC_DRAW,typedArray:new Uint8Array([0,1,2,3,4,5,6,7,8,9,10,11])});
  b.copyFromArrayView(new Uint8Array([21,22,23]),3);
  const target=ref.Buffer.createVertexBuffer({context,usage:ref.BufferUsage.STATIC_DRAW,sizeInBytes:12});target.copyFromBuffer(b,2,4,6);
  const indices=ref.Buffer.createIndexBuffer({context,usage:ref.BufferUsage.STATIC_DRAW,typedArray:new Uint16Array([0,1,2]),indexDatatype:ref.IndexDatatype.UNSIGNED_SHORT});
  const va=new ref.VertexArray({context,attributes:[{index:0,vertexBuffer:b,componentsPerAttribute:3,componentDatatype:ref.ComponentDatatype.UNSIGNED_BYTE,strideInBytes:4},{index:1,value:[1,0,0,1]}],indexBuffer:indices});
  const read=(buffer,offset,length)=>{const bytes=new Uint8Array(length);buffer.getBufferData(bytes,offset);return Array.from(bytes);};
  const kinds=[];
  for(const datatype of [null,5121,5123,5125,'pixel']){
    const opts={context,usage:ref.BufferUsage.DYNAMIC_READ,typedArray:new Uint8Array([1,2,3,4,5,6,7,8,9,10,11,12])};
    const tmp=datatype===null?ref.Buffer.createVertexBuffer(opts):datatype==='pixel'?ref.Buffer.createPixelBuffer(opts):ref.Buffer.createIndexBuffer({...opts,indexDatatype:datatype});
    tmp.copyFromBuffer(tmp,0,8,4);kinds.push([tmp.sizeInBytes,tmp.numberOfIndices??null,read(tmp,0,12)]);tmp.destroy();
  }
  const expected={kinds,source:read(b,0,12),target:read(target,0,12),slice:read(b,3,4),vertices:va.numberOfVertices,indices:indices.numberOfIndices,bytes:b.sizeInBytes,usage:b.usage};
  eq(JSON.parse(wasm.renderer_buffers(gl)),expected,'buffer allocation/write/copy/readback and vertex/index metadata');
  eq(gl.getError(),gl.NO_ERROR,'Rust buffers leave no GL errors');
  eq(context._gl.getError(),gl.NO_ERROR,'Cesium buffers leave no GL errors');
  va.destroy();target.destroy();

  const drawRef=(config,image)=>{
    const indexed=!!config.indexed,instanced=!!config.instanced,textured=!!config.textured;
    const positions=indexed?[-1,-1,1,-1,1,1,-1,1]:[-1,-1,-1,-1,-1,-1,-1,-1,1,-1,1,1,-1,-1,1,1,-1,1];
    const vertex=ref.Buffer.createVertexBuffer({context,usage:ref.BufferUsage.STATIC_DRAW,typedArray:new Float32Array(positions)});
    const indexBuffer=indexed?ref.Buffer.createIndexBuffer({context,usage:ref.BufferUsage.STATIC_DRAW,typedArray:new Uint16Array([0,0,0,0,1,2,0,2,3]),indexDatatype:ref.IndexDatatype.UNSIGNED_SHORT}):undefined;
    const attributes=[{index:0,vertexBuffer:vertex,componentsPerAttribute:2},{index:1,value:[1,.5,.25,1]}];
    if(instanced)attributes.push({index:2,componentsPerAttribute:2,instanceDivisor:1,vertexBuffer:ref.Buffer.createVertexBuffer({context,usage:ref.BufferUsage.STATIC_DRAW,typedArray:new Float32Array([-.5,0,.5,0])})});else attributes.push({index:2,value:[0,0]});
    const va=new ref.VertexArray({context,attributes,indexBuffer});
    const vertexShaderSource=`in vec2 position; in vec4 color; in vec2 offset; out vec4 v_color; out vec2 v_uv; uniform mat4 transform; void main() { gl_Position = transform * vec4(position * vec2(${instanced?'0.5':'1.0'},1.0) + offset,0.,1.); v_color=color; v_uv=position*.5+.5; }`;
    const fragmentShaderSource=`in vec4 v_color; in vec2 v_uv; uniform vec4 tint; uniform int enabled; ${textured?'uniform sampler2D image;':''} void main(){out_FragColor= (enabled==1 ? tint : vec4(0.)) * v_color ${textured?(config.lod===undefined?'* texture(image,v_uv)':`* textureLod(image,v_uv,${config.lod}.0)`):''};}`;
    const program=ref.ShaderProgram.fromCache({context,vertexShaderSource,fragmentShaderSource,attributeLocations:{position:0,color:1,offset:2}});
    let texture;
    if(textured){const bytes=new Uint8Array(config.pixels??[255,0,0,255,0,255,0,255,0,0,255,255,255,255,255,255]);const datatype=config.textureDatatype??5121;const arrayBufferView=datatype===5126?new Float32Array(bytes.buffer):[5123,32819,32820,33635,36193].includes(datatype)?new Uint16Array(bytes.buffer):datatype===5125?new Uint32Array(bytes.buffer):bytes;const sampler=config.linear?new ref.Sampler():config.samplerMips?new ref.Sampler({minificationFilter:9984,magnificationFilter:9728}):ref.Sampler.NEAREST;
      const opts={context,width:config.textureWidth??2,height:config.textureHeight??2,pixelFormat:config.textureFormat??6408,pixelDatatype:datatype,source:{width:config.textureWidth??2,height:config.textureHeight??2,arrayBufferView,mipLevels:config.mipLevels?.map(a=>new Uint8Array(a))},flipY:config.flipY??true,sampler,preMultiplyAlpha:config.preMultiplyAlpha??false,skipColorSpaceConversion:config.skipColorSpaceConversion??false};
      if(config.sourceKind==='image')opts.source=image;if(config.sourceKind==='imageSub')delete opts.source;texture=new ref.Texture(opts);if(config.sourceKind==='imageSub')texture.copyFrom({source:image,skipColorSpaceConversion:config.skipColorSpaceConversion??false});if(config.update)texture.copyFrom({source:{width:1,height:1,arrayBufferView:new Uint8Array([255,255,0,255])},xOffset:1,yOffset:0});if(config.mipmap)texture.generateMipmap();}
    const pass=new ref.PassState(context);Object.assign(pass,config.pass??{});
    new ref.ClearCommand({color:new ref.Color(.1,.2,.3,1),depth:1,stencil:0}).execute(context);
    const uniformMap={tint:()=>new ref.Color(1,1,1,.5),enabled:()=>1,transform:()=>[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]};
    if(texture)uniformMap.image=()=>texture;
    const command=new ref.DrawCommand({vertexArray:va,shaderProgram:program,primitiveType:ref.PrimitiveType.TRIANGLES,offset:config.offset??0,count:config.count,instanceCount:instanced?2:0,renderState:ref.RenderState.fromCache(config.state??{}),uniformMap});
    command.execute(context,pass);
    if(config.switchConstant){const other=new ref.VertexArray({context,attributes:[{index:0,value:[0,0]},{index:1,value:[0,1,0,1]},{index:2,value:[0,0]}]});new ref.DrawCommand({vertexArray:other,shaderProgram:program,uniformMap,primitiveType:ref.PrimitiveType.TRIANGLES,renderState:command.renderState}).execute(context,pass);command.execute(context,pass);other.destroy();}
    const result={pixels:Array.from(context.readPixels({width:8,height:8})),textureBytes:texture?.sizeInBytes??null};
    va.destroy();program.destroy();texture?.destroy();return result;
  };
  const cases=[{}, {indexed:true},{count:0},{indexed:true,count:0},{offset:3,count:6},{indexed:true,offset:3,count:6},{count:100},{indexed:true,count:100},{instanced:true},{indexed:true,instanced:true},{switchConstant:true},{indexed:true,switchConstant:true},
    {state:{cull:{enabled:true,face:1028}}},{state:{depthTest:{enabled:true,func:512}}},{state:{depthTest:{enabled:true,func:515}}},
    {state:{blending:{enabled:true,functionSourceRgb:770,functionDestinationRgb:771}}},{state:{colorMask:{red:false,alpha:false}}},
    {state:{viewport:{x:2,y:1,width:4,height:5}}},{pass:{viewport:{x:1,y:2,width:6,height:4}}},
    {state:{scissorTest:{enabled:true,rectangle:{x:1,y:2,width:3,height:4}}}},
    {state:{scissorTest:{enabled:false}},pass:{scissorTest:{enabled:true,rectangle:{x:3,y:0,width:2,height:8}}}},
    {state:{blending:{enabled:true}},pass:{blendingEnabled:false}},
    ...[false,true].flatMap(flipY=>[{textured:true,flipY},{textured:true,flipY,update:true},{textured:true,flipY,mipmap:true}])];

  const rawBytes=a=>Array.from(new Uint8Array(a.buffer));
  for(const flipY of [false,true]) {
    cases.push({textured:true,flipY,linear:true});
    cases.push({textured:true,flipY,textureDatatype:5126,pixels:rawBytes(new Float32Array([1,0,0,1,0,1,0,1,0,0,1,1,1,1,1,1]))});
    cases.push({textured:true,flipY,textureDatatype:36193,pixels:rawBytes(new Uint16Array([15360,0,0,15360,0,15360,0,15360,0,0,15360,15360,15360,15360,15360,15360]))});
    cases.push({textured:true,flipY,textureFormat:6407,textureDatatype:33635,pixels:rawBytes(new Uint16Array([63488,2016,31,65535]))});
    cases.push({textured:true,flipY,textureDatatype:32819,pixels:rawBytes(new Uint16Array([61455,3855,255,65535]))});
    cases.push({textured:true,flipY,textureDatatype:32820,pixels:rawBytes(new Uint16Array([63489,1985,63,65535]))});
    // Odd RGB row lengths exercise pixel alignment 1 and 2, not RGBA's 4.
    cases.push({textured:true,flipY,textureFormat:6407,textureWidth:3,textureHeight:2,pixels:[255,0,0,0,255,0,0,0,255,255,255,255,0,0,0,255,255,0]});
    cases.push({textured:true,flipY,textureFormat:6403,pixels:[255,64,128,0]});
    cases.push({textured:true,flipY,textureFormat:33319,pixels:[255,0,64,255,128,32,0,128]});
  }

  for(const sourceKind of ['image','imageSub'])for(const flipY of [false,true])for(const preMultiplyAlpha of [false,true])for(const skipColorSpaceConversion of [false,true])cases.push({textured:true,sourceKind,flipY,preMultiplyAlpha,skipColorSpaceConversion});
  for(const lod of [0,1,2])cases.push({textured:true,sourceKind:'mipmaps',samplerMips:true,lod,textureWidth:4,textureHeight:4,pixels:Array(16).fill([255,0,0,255]).flat(),mipLevels:[Array(4).fill([0,255,0,255]).flat(),[0,0,255,255]]});
  for(const lod of [0,1,2])cases.push({textured:true,sourceKind:'compressed',samplerMips:true,lod,textureFormat:33776,textureWidth:4,textureHeight:4,pixels:[0,248,0,248,0,0,0,0],mipLevels:[[224,7,224,7,0,0,0,0],[31,0,31,0,0,0,0,0]]});
  const image=canvas();image.width=2;image.height=2;image.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray([255,0,0,128,0,255,0,255,0,0,255,64,255,255,255,255]),2,2),0,0);
  for(const config of cases){
    const expected=drawRef(config,image),actual=JSON.parse(wasm.renderer_draw(gl,JSON.stringify(config),image));
    eq(actual,expected,'draw pixels '+JSON.stringify(config));eq(gl.getError(),gl.NO_ERROR,'Rust draw GL status '+JSON.stringify(config));eq(context._gl.getError(),gl.NO_ERROR,'Cesium draw GL status '+JSON.stringify(config));
  }
  const rejected=(vertexShaderSource,fragmentShaderSource)=>{const p=ref.ShaderProgram.fromCache({context,vertexShaderSource,fragmentShaderSource});try{p._bind();return false;}catch(e){return e.name==='RuntimeError'&&e.message.length>0;}finally{p.destroy();}};
  const shaderErrors=[rejected('BROKEN SHADER','void main(){out_FragColor=vec4(1.);}'),rejected('out vec2 varyingValue;void main(){gl_Position=vec4(0.);varyingValue=vec2(1.);}','in vec3 varyingValue;void main(){out_FragColor=vec4(varyingValue,1.);}')];
  eq(JSON.parse(wasm.renderer_shader_errors(gl)),shaderErrors,'shader compiler/linker diagnostics, inactive uniforms and destruction');
  eq(gl.getError(),gl.NO_ERROR,'shader failure cleanup GL status');

  for(const state of [{},{colorMask:{red:false,alpha:false}},{scissorTest:{enabled:true,rectangle:{x:2,y:1,width:3,height:5}}}])for(const pass of [{},{scissorTest:{enabled:true,rectangle:{x:0,y:0,width:1,height:1}}}]){
    new ref.ClearCommand({color:new ref.Color(.1,.2,.3,1)}).execute(context);
    const p=new ref.PassState(context);Object.assign(p,pass);
    new ref.ClearCommand({color:new ref.Color(1,.5,0,.25),depth:.5,stencil:3,renderState:ref.RenderState.fromCache(state)}).execute(context,p);
    eq(JSON.parse(wasm.renderer_clear(gl,JSON.stringify(state),JSON.stringify(pass))),Array.from(context.readPixels({width:8,height:8})),'masked/scissored clear '+JSON.stringify({state,pass}));
    eq(gl.getError(),gl.NO_ERROR,'Rust clear GL status');eq(context._gl.getError(),gl.NO_ERROR,'Cesium clear GL status');
  }

  for(const mode of [0,1,2]){
    const make=()=>new ref.Framebuffer({context,colorTextures:[new ref.Texture({context,width:4,height:4,sampler:ref.Sampler.NEAREST})],destroyAttachments:false});
    const a=make(),b=make(),pass=new ref.PassState(context);pass.framebuffer=a;
    new ref.ClearCommand({color:new ref.Color(1,0,0,1)}).execute(context,pass);
    new ref.ClearCommand({color:new ref.Color(0,1,0,1),framebuffer:b}).execute(context,pass);
    const pixels=f=>Array.from(context.readPixels({framebuffer:f,width:4,height:4}));
    const result={status:a.status,count:a.numberOfColorAttachments,depth:a.hasDepthAttachment,a:pixels(a),b:pixels(b)};
    const pbo=context.readPixelsToPBO({framebuffer:b,width:4,height:4});const bytes=new Uint8Array(64);pbo.getBufferData(bytes);result.pbo=Array.from(bytes);pbo.destroy();
    const copied=ref.Texture.fromFramebuffer({context,pixelFormat:6408,width:4,height:4,framebuffer:a}),cf=new ref.Framebuffer({context,colorTextures:[copied]});result.copy=pixels(cf);cf.destroy();
    if(mode>0){const color=new ref.Renderbuffer({context,format:ref.RenderbufferFormat.RGBA8,width:4,height:4,numSamples:mode===2?4:1}),depth=new ref.Renderbuffer({context,format:ref.RenderbufferFormat.DEPTH_COMPONENT16,width:4,height:4,numSamples:mode===2?4:1});const f=new ref.Framebuffer({context,colorRenderbuffers:[color],depthRenderbuffer:depth});new ref.ClearCommand({color:new ref.Color(.2,.4,.6,1),framebuffer:f}).execute(context);f.bindRead();b.bindDraw();context._gl.blitFramebuffer(0,0,4,4,0,0,4,4,context._gl.COLOR_BUFFER_BIT,context._gl.NEAREST);context._gl.bindFramebuffer(context._gl.FRAMEBUFFER,null);context._currentFramebuffer=undefined;result.resolved=pixels(b);result.depth=f.hasDepthAttachment;result.status=f.status;result.renderbuffer=[color.format,color.width,color.height];f.destroy();}
    eq(JSON.parse(wasm.renderer_framebuffers(gl,mode)),result,'framebuffer routing, PBO, copy and resolve mode '+mode);eq(gl.getError(),gl.NO_ERROR,'Rust framebuffer GL status');eq(context._gl.getError(),gl.NO_ERROR,'Cesium framebuffer GL status');
    a.getColorTexture(0).destroy();b.getColorTexture(0).destroy();a.destroy();b.destroy();context._currentFramebuffer=undefined;
  }

  // Cesium's typed readPixels allocates using the attachment format but asks
  // GL for RGBA, which underallocates non-RGBA outputs. Compare the corrected
  // Rust layout to explicit, guaranteed WebGL read formats on Cesium targets.
  gl.getExtension('EXT_color_buffer_float');
  const readbackCases=[
    [gl.RGB,gl.UNSIGNED_BYTE,new Uint8Array(Array(6).fill([32,64,128]).flat())],
    [gl.RED,gl.UNSIGNED_BYTE,new Uint8Array(Array(6).fill(32))],
    [gl.RG,gl.UNSIGNED_BYTE,new Uint8Array(Array(6).fill([32,64]).flat())],
    [gl.RGBA,gl.UNSIGNED_BYTE,new Uint8Array(Array(6).fill([32,64,128,255]).flat())],
    [gl.RED_INTEGER,gl.UNSIGNED_INT,new Uint32Array(Array(6).fill(70000))],
    [gl.RG_INTEGER,gl.UNSIGNED_INT,new Uint32Array(Array(6).fill([70000,80000]).flat())],
    [gl.RGBA_INTEGER,gl.UNSIGNED_INT,new Uint32Array(Array(6).fill([70000,80000,90000,100000]).flat())],
    [gl.RED,gl.FLOAT,new Float32Array(Array(6).fill(.5))],
    [gl.RGBA,5131,new Uint16Array(Array(6).fill([0x3800,0x3400,0x3c00,0x3c00]).flat())],
    [gl.RGB,gl.UNSIGNED_SHORT_5_6_5,new Uint16Array(Array(6).fill(0xf800))],
  ];
  for(const [format,type,values] of readbackCases){
    const datatype=type===5131?36193:type,integer=[gl.RED_INTEGER,gl.RG_INTEGER,gl.RGBA_INTEGER].includes(format),floating=[gl.FLOAT,5131].includes(type);
    const texture=new ref.Texture({context,width:3,height:2,pixelFormat:format,pixelDatatype:datatype,source:{arrayBufferView:values},flipY:false,sampler:ref.Sampler.NEAREST});
    // Cesium's debug isColorFormat also omits RG and integer attachments;
    // attach its texture directly for these WebGL2 regression cases.
    const framebuffer=context._gl.createFramebuffer();context._gl.bindFramebuffer(gl.FRAMEBUFFER,framebuffer);
    context._gl.framebufferTexture2D(gl.FRAMEBUFFER,gl.COLOR_ATTACHMENT0,gl.TEXTURE_2D,texture._texture,0);
    const output=integer?new Uint32Array(24):floating?new Float32Array(24):new Uint8Array(24),outputFormat=integer?gl.RGBA_INTEGER:gl.RGBA,outputType=integer?gl.UNSIGNED_INT:floating?gl.FLOAT:gl.UNSIGNED_BYTE;
    context._gl.bindBuffer(gl.PIXEL_PACK_BUFFER,null);context._gl.pixelStorei(gl.PACK_ALIGNMENT,1);
    context._gl.readPixels(0,0,3,2,outputFormat,outputType,output);
    const expected={bytes:Array.from(new Uint8Array(output.buffer)),format:outputFormat,datatype:outputType,pbo:Array.from(new Uint8Array(output.buffer)),status:context._gl.checkFramebufferStatus(gl.FRAMEBUFFER)};
    eq(expected.status,gl.FRAMEBUFFER_COMPLETE,'reference readback target complete');
    const actual=JSON.parse(wasm.renderer_readback(gl,JSON.stringify({format,datatype,bytes:Array.from(new Uint8Array(values.buffer))})));
    eq(actual,expected,'converted typed/PBO readback '+format+'/'+type);
    eq(gl.getError(),gl.NO_ERROR,'Rust readback GL status');eq(context._gl.getError(),gl.NO_ERROR,'reference readback GL status');context._gl.bindFramebuffer(gl.FRAMEBUFFER,null);context._gl.deleteFramebuffer(framebuffer);texture.destroy();context._currentFramebuffer=undefined;
  }
  const anisotropy=context._gl.getExtension('EXT_texture_filter_anisotropic');
  if(!anisotropy||!gl.getExtension('EXT_texture_filter_anisotropic'))throw Error('Anisotropy regression requires the extension');
  for(const maximumAnisotropy of [1,1.5,2]){
    const texture=new ref.Texture({context,width:1,height:1,sampler:new ref.Sampler({maximumAnisotropy})});context._gl.bindTexture(gl.TEXTURE_2D,texture._texture);
    const applied=context._gl.getTexParameter(gl.TEXTURE_2D,anisotropy.TEXTURE_MAX_ANISOTROPY_EXT);
    eq(JSON.parse(wasm.renderer_anisotropy(gl,maximumAnisotropy)),[texture.sampler.maximumAnisotropy,applied],'Cesium anisotropy metadata and GL conversion '+maximumAnisotropy);
    eq(gl.getError(),gl.NO_ERROR,'Rust anisotropy GL status');eq(context._gl.getError(),gl.NO_ERROR,'Cesium anisotropy GL status');texture.destroy();
  }

  for(const interleave of [false,true]){
    const position=new ref.GeometryAttribute({componentDatatype:5130,componentsPerAttribute:2,values:new Float64Array([-1,-1,1,-1,1,1,-1,1])}),color=new ref.GeometryAttribute({componentDatatype:5121,componentsPerAttribute:4,normalize:true,values:new Uint8Array([255,128,64,255,255,128,64,255,255,128,64,255,255,128,64,255])}),weight=new ref.GeometryAttribute({componentDatatype:5123,componentsPerAttribute:2,values:new Uint16Array([1,2,3,4,5,6,7,8])});
    const va=ref.VertexArray.fromGeometry({context,geometry:new ref.Geometry({attributes:{position,color,weight},indices:new Uint16Array([0,1,2,0,2,3]),primitiveType:4}),attributeLocations:{position:0,color:1,weight:2},interleave});
    const layouts=Array.from({length:va.numberOfAttributes},(_,i)=>{const a=va.getAttribute(i);return[a.index,a.componentsPerAttribute,a.componentDatatype,a.normalize,a.offsetInBytes,a.strideInBytes,read(a.vertexBuffer,0,a.vertexBuffer.sizeInBytes)];});
    const geometryCache=new ref.ShaderCache(context);const program=geometryCache.getShaderProgram({vertexShaderSource:'in vec2 position;in vec4 color;out vec4 v_color;void main(){gl_Position=czm_modelViewProjection*vec4(position,0.,1.);v_color=color;}',fragmentShaderSource:'in vec4 v_color;void main(){out_FragColor=v_color;}',attributeLocations:{position:0,color:1}});
    ref.ClearCommand.ALL.execute(context);new ref.DrawCommand({vertexArray:va,shaderProgram:program}).execute(context);
    const attributes=Object.values(program.vertexAttributes).map(v=>({name:v.name,datatype:v.type,size:1})).sort((a,b)=>a.name.localeCompare(b.name));
    const expected={layouts,indices:read(va.indexBuffer,0,12),pixels:Array.from(context.readPixels({width:8,height:8})),attributes,programs:geometryCache.numberOfShaders};
    eq(JSON.parse(wasm.renderer_geometry(gl,interleave)),expected,'geometry upload, builtin automatic matrix, reflection and shader cache interleave='+interleave);eq(gl.getError(),gl.NO_ERROR,'Rust geometry GL status');eq(context._gl.getError(),gl.NO_ERROR,'Cesium geometry GL status');va.destroy();program.destroy();geometryCache.destroy();context._currentFramebuffer=undefined;
  }
  const specs=[],declarations=[],terms=[];
  for(const kind of ['float','int','bool'])for(let components=1;components<=4;components++){
    const name=(kind==='float'?'f':kind==='int'?'i':'b')+components,datatype=components===1?kind:(kind==='float'?'vec':kind==='int'?'ivec':'bvec')+components,values=Array.from({length:components*2},(_,i)=>kind==='bool'?i%2:i+1);specs.push({name,kind,components,values});declarations.push(`uniform ${datatype} ${name}[2];`);terms.push(components===1?(kind==='bool'?`float(${name}[0])+float(${name}[1])`:`float(${name}[0]+${name}[1])`):`dot(vec${components}(${name}[0])+vec${components}(${name}[1]),vec${components}(1))`);
  }
  for(const components of [2,3,4]){const name='m'+components,values=Array.from({length:components*components*2},(_,i)=>i+1);specs.push({name,kind:'matrix',components,values});declarations.push(`uniform mat${components} ${name}[2];`);terms.push(`${name}[0][0][0]+${name}[1][0][0]`);}
  const vertex='#version 300 es\nvoid main(){gl_Position=vec4(0.);}',fragment='#version 300 es\nprecision highp float;\n'+declarations.join('\n')+`\nvoid main(){out_FragColor=vec4((${terms.join('+')})*.001);}`;
  const program=ref.ShaderProgram.fromCache({context,vertexShaderSource:vertex,fragmentShaderSource:fragment}),uniformMap={};
  for(const spec of specs){const {kind,components:n,values}=spec,width=kind==='matrix'?n*n:n;const array=[values.slice(0,width),values.slice(width)];uniformMap[spec.name]=()=>n===1?values:array.map(a=>kind==='matrix'?ref['Matrix'+n].fromArray(a):new ref['Cartesian'+n](...a));}
  program._bind();program._setUniforms(uniformMap,context.uniformState);const uniformExpected={};for(const spec of specs)uniformExpected[spec.name]=[0,1].map(i=>{const v=context._gl.getUniform(program._program,context._gl.getUniformLocation(program._program,`${spec.name}[${i}]`));return ArrayBuffer.isView(v)?Array.from(v):v;});
  eq(JSON.parse(wasm.renderer_uniform_arrays(gl,vertex,fragment.replaceAll('out_FragColor','result').replace('void main()','out vec4 result;\nvoid main()'),JSON.stringify(specs))),uniformExpected,'float/int/bool/vector/matrix uniform arrays');eq(gl.getError(),gl.NO_ERROR,'Rust uniform arrays GL status');eq(context._gl.getError(),gl.NO_ERROR,'Cesium uniform arrays GL status');program.destroy();
  const limitNames=['maximumCombinedTextureImageUnits','maximumCubeMapSize','maximumFragmentUniformVectors','maximumTextureImageUnits','maximumRenderbufferSize','maximumTextureSize','maximum3DTextureSize','maximumVaryingVectors','maximumVertexAttributes','maximumVertexTextureImageUnits','maximumVertexUniformVectors','minimumAliasedLineWidth','maximumAliasedLineWidth','minimumAliasedPointSize','maximumAliasedPointSize','maximumViewportWidth','maximumViewportHeight','maximumTextureFilterAnisotropy','maximumDrawBuffers','maximumColorAttachments','maximumSamples','highpFloatSupported','highpIntSupported'];
  const actualLimits=JSON.parse(wasm.renderer_limits(gl));if('maximum3dTextureSize'in actualLimits){actualLimits.maximum3DTextureSize=actualLimits.maximum3dTextureSize;delete actualLimits.maximum3dTextureSize;}
  eq(actualLimits,Object.fromEntries(limitNames.map(n=>[n,ref.ContextLimits[n]])),'all context limits');
  const parameters=['FRONT_FACE','CULL_FACE','CULL_FACE_MODE','LINE_WIDTH','POLYGON_OFFSET_FILL','POLYGON_OFFSET_FACTOR','POLYGON_OFFSET_UNITS','DEPTH_RANGE','DEPTH_TEST','DEPTH_FUNC','COLOR_WRITEMASK','DEPTH_WRITEMASK','STENCIL_WRITEMASK','BLEND','BLEND_COLOR','BLEND_EQUATION_RGB','BLEND_EQUATION_ALPHA','BLEND_SRC_RGB','BLEND_SRC_ALPHA','BLEND_DST_RGB','BLEND_DST_ALPHA','STENCIL_TEST','STENCIL_FUNC','STENCIL_BACK_FUNC','STENCIL_REF','STENCIL_VALUE_MASK','STENCIL_FAIL','STENCIL_PASS_DEPTH_FAIL','STENCIL_PASS_DEPTH_PASS','STENCIL_BACK_FAIL','STENCIL_BACK_PASS_DEPTH_FAIL','STENCIL_BACK_PASS_DEPTH_PASS','SAMPLE_COVERAGE','SAMPLE_COVERAGE_VALUE','SAMPLE_COVERAGE_INVERT','SCISSOR_TEST','SCISSOR_BOX','VIEWPORT'];
  const snapshot=g=>Object.fromEntries(parameters.map(n=>{const v=g.getParameter(g[n]);return [n,ArrayBuffer.isView(v)?Array.from(v):v];}));
  // Compare active state after alternating nondefault/default/pass settings.
  const states=[{frontFace:2304,cull:{enabled:true,face:1028},polygonOffset:{enabled:true,factor:1.5,units:2},depthRange:{near:.1,far:.9},depthTest:{enabled:true,func:515},colorMask:{red:false,blue:false},depthMask:false,stencilMask:15,blending:{enabled:true,color:{red:.2,blue:.7},equationRgb:32779,functionSourceRgb:770,functionDestinationRgb:771},stencilTest:{enabled:true,frontFunction:514,backFunction:516,reference:3,mask:127,frontOperation:{zPass:7681},backOperation:{fail:7682,zFail:7683,zPass:5386}},sampleCoverage:{enabled:true,value:.25,invert:true},scissorTest:{enabled:true,rectangle:{x:1,y:2,width:3,height:4}},viewport:{x:1,y:1,width:6,height:6}},{}];
  for(const state of states)for(const pass of [{},{viewport:{x:2,y:2,width:4,height:4},blendingEnabled:false,scissorTest:{enabled:true,rectangle:{x:0,y:0,width:2,height:2}}}]){
    wasm.renderer_state(gl,JSON.stringify(state),JSON.stringify(pass));
    const p=new ref.PassState(context);Object.assign(p,pass);ref.RenderState.apply(context._gl,ref.RenderState.fromCache(state),p);
    eq(snapshot(gl),snapshot(context._gl),'complete GL render state '+JSON.stringify({state,pass}));
    eq(gl.getError(),gl.NO_ERROR,'Rust render-state GL status');eq(context._gl.getError(),gl.NO_ERROR,'Cesium render-state GL status');
  }
  context.destroy();
  return passed;
 }catch(e){throw new Error(String(e.message??e)+"\n"+String(e.stack??""));}});
 if(errors.length)throw Error(errors.join('\n'));
 for(const check of checks)console.log('PASS '+check);console.log(`${checks.length} renderer checks passed`);
} finally {await browser?.close();await new Promise(resolve=>server.close(resolve));}
