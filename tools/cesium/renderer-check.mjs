// Every test uses independent WebGL2 contexts for Rust and the pinned CesiumJS.
import {chromium} from 'playwright-core';
import {build} from 'esbuild';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve,extname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('../..',import.meta.url)));
const names=['Buffer','BufferUsage','VertexArray','Context','ShaderProgram','ShaderSource','Texture','Sampler','RenderState','DrawCommand','ClearCommand','PassState'];
const core=['ComponentDatatype','IndexDatatype','PrimitiveType','Color','WebGLConstants'];
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
 const checks=await page.evaluate(async()=>{
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

  const drawRef=config=>{
    const indexed=!!config.indexed,instanced=!!config.instanced,textured=!!config.textured;
    const positions=indexed?[-1,-1,1,-1,1,1,-1,1]:[-1,-1,-1,-1,-1,-1,-1,-1,1,-1,1,1,-1,-1,1,1,-1,1];
    const vertex=ref.Buffer.createVertexBuffer({context,usage:ref.BufferUsage.STATIC_DRAW,typedArray:new Float32Array(positions)});
    const indexBuffer=indexed?ref.Buffer.createIndexBuffer({context,usage:ref.BufferUsage.STATIC_DRAW,typedArray:new Uint16Array([0,0,0,0,1,2,0,2,3]),indexDatatype:ref.IndexDatatype.UNSIGNED_SHORT}):undefined;
    const attributes=[{index:0,vertexBuffer:vertex,componentsPerAttribute:2},{index:1,value:[1,.5,.25,1]}];
    if(instanced)attributes.push({index:2,componentsPerAttribute:2,instanceDivisor:1,vertexBuffer:ref.Buffer.createVertexBuffer({context,usage:ref.BufferUsage.STATIC_DRAW,typedArray:new Float32Array([-.5,0,.5,0])})});else attributes.push({index:2,value:[0,0]});
    const va=new ref.VertexArray({context,attributes,indexBuffer});
    const vertexShaderSource=`in vec2 position; in vec4 color; in vec2 offset; out vec4 v_color; out vec2 v_uv; uniform mat4 transform; void main() { gl_Position = transform * vec4(position * vec2(${instanced?'0.5':'1.0'},1.0) + offset,0.,1.); v_color=color; v_uv=position*.5+.5; }`;
    const fragmentShaderSource=`in vec4 v_color; in vec2 v_uv; uniform vec4 tint; uniform int enabled; ${textured?'uniform sampler2D image;':''} void main(){out_FragColor= (enabled==1 ? tint : vec4(0.)) * v_color ${textured?'* texture(image,v_uv)':''};}`;
    const program=ref.ShaderProgram.fromCache({context,vertexShaderSource,fragmentShaderSource,attributeLocations:{position:0,color:1,offset:2}});
    let texture;
    if(textured){const bytes=new Uint8Array(config.pixels??[255,0,0,255,0,255,0,255,0,0,255,255,255,255,255,255]);const datatype=config.textureDatatype??5121;const arrayBufferView=datatype===5126?new Float32Array(bytes.buffer):[5123,32819,32820,33635,36193].includes(datatype)?new Uint16Array(bytes.buffer):datatype===5125?new Uint32Array(bytes.buffer):bytes;texture=new ref.Texture({context,pixelFormat:config.textureFormat??6408,pixelDatatype:datatype,source:{width:config.textureWidth??2,height:config.textureHeight??2,arrayBufferView},flipY:config.flipY??true,sampler:config.linear?new ref.Sampler():ref.Sampler.NEAREST});if(config.update)texture.copyFrom({source:{width:1,height:1,arrayBufferView:new Uint8Array([255,255,0,255])},xOffset:1,yOffset:0});if(config.mipmap)texture.generateMipmap();}
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
  for(const config of cases){
    const expected=drawRef(config),actual=JSON.parse(wasm.renderer_draw(gl,JSON.stringify(config)));
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
 });
 if(errors.length)throw Error(errors.join('\n'));
 for(const check of checks)console.log('PASS '+check);console.log(`${checks.length} renderer checks passed`);
} finally {await browser?.close();await new Promise(resolve=>server.close(resolve));}
