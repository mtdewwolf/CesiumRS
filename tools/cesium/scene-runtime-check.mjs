// Executable Scene draws and derived shaders against real CesiumJS/WebGL2.
import {chromium} from 'playwright-core';
import {build} from 'esbuild';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve,extname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('../..',import.meta.url)));
const modules={Renderer:['FramebufferManager','Context','ShaderSource','ShaderProgram','VertexArray','Buffer','DrawCommand','Texture','Sampler','PassState','ClearCommand','RenderState'],Scene:['OIT','SceneFramebuffer','GlobeDepth','PickDepth','PickDepthFramebuffer','EdgeFramebuffer','GlobeTranslucencyFramebuffer','PickFramebuffer','DerivedCommand','ResourceCache','ClippingPlane','ClippingPlaneCollection','StencilConstants'],Core:['KTX2Transcoder','loadKTX2','Resource','Color','BoundingRectangle','BoundingSphere','PerspectiveFrustum','Cartesian2','Cartesian3','Matrix4']};
const contents=Object.entries(modules).flatMap(([dir,names])=>names.map(n=>`export {default as ${n}} from '@cesium/engine/Source/${dir}/${n}.js';`)).join('\n')+"\nexport {default as ModelClippingPlanesStageFS} from '@cesium/engine/Source/Shaders/Model/ModelClippingPlanesStageFS.js';\nexport {default as ModelDrawCommand} from '@cesium/engine/Source/Scene/Model/ModelDrawCommand.js';";
const bundle=await build({stdin:{contents,resolveDir:fileURLToPath(new URL('.',import.meta.url))},bundle:true,write:false,format:'esm',platform:'browser'});
const imageFixtures=new Map();
const server=createServer(async(req,res)=>{try{const url=new URL(req.url,'http://localhost');if(url.pathname.startsWith('/image-fixtures/')){if(req.method==='POST'){const chunks=[];for await(const chunk of req)chunks.push(chunk);imageFixtures.set(url.pathname,Buffer.concat(chunks));res.end('ok');return;}if(imageFixtures.has(url.pathname)){res.setHeader('Content-Type','image/'+url.pathname.split('.').at(-1));res.end(imageFixtures.get(url.pathname));return;}}if(url.pathname==='/reference.mjs'){res.setHeader('Content-Type','text/javascript');res.end(bundle.outputFiles[0].contents);return;}if(url.pathname==='/'){res.end('<!doctype html><title>Scene checks</title>');return;}const path=resolve(root,'.'+url.pathname);if(!path.startsWith(root+'/'))throw Error('Invalid path');res.setHeader('Content-Type',({'.js':'text/javascript','.mjs':'text/javascript','.wasm':'application/wasm'})[extname(path)]??'application/octet-stream');res.end(await readFile(path));}catch(e){res.writeHead(404);res.end(e.message);}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));let browser;
try{
 browser=await chromium.launch({executablePath:process.env.CHROMIUM_PATH??'/usr/bin/chromium',headless:true,args:['--no-sandbox','--use-angle=swiftshader','--enable-unsafe-swiftshader']});const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}/`);
 const result=await page.evaluate(async()=>{try{const wasm=await import('/target/renderer-validation/renderer.js');await wasm.default();const ref=await import('/reference.mjs');
  const canvas=()=>{const c=document.createElement('canvas');c.width=c.height=8;return c;};const attributes={antialias:false,alpha:true,stencil:true,preserveDrawingBuffer:true};const passed=await (await import('/tools/cesium/scene-image-texture-cases.mjs')).run(wasm,ref,canvas,attributes);passed.push(...await (await import('/tools/cesium/scene-geometry-buffer-cases.mjs')).run(wasm,ref,canvas,attributes));passed.push(...await(await import('/tools/cesium/scene-depth-cases.mjs')).run(wasm,ref,canvas,attributes));passed.push(...await(await import('/tools/cesium/scene-oit-cases.mjs')).run(wasm,ref,canvas,attributes));passed.push(...await(await import('/tools/cesium/scene-classification-depth-cases.mjs')).run(wasm,ref,canvas,attributes));
  for(const log of [false,true])for(const hdr of [false,true])for(const pass of ['render','pick','depth'])for(const target of [false,true]){
    const config={log,hdr,pick:pass==='pick',depth:pass==='depth',target};window.lastSceneCase=config;const gl=canvas().getContext('webgl2',attributes),context=new ref.Context(canvas(),{webgl:attributes});
    context.uniformState.updateFrustum(new ref.PerspectiveFrustum({fov:Math.PI/3,aspectRatio:1,near:.1,far:1e8}));
    const image=new ref.Texture({context,source:{width:1,height:1,arrayBufferView:new Uint8Array([200,100,50,255])},flipY:false,sampler:ref.Sampler.NEAREST});
    let command=context.createViewportQuadCommand('uniform sampler2D image;uniform vec4 u_pickColor;void main(){out_FragColor=texture(image,vec2(0.5));\n#ifdef HDR\nout_FragColor.rgb*=0.5;\n#endif\n}',{renderState:ref.RenderState.fromCache({}),uniformMap:{image:()=>image,u_pickColor:()=>({x:25/255,y:50/255,z:75/255,w:1})}});
    command.pickId='u_pickColor';const scene={_depthOnlyRenderStateCache:{},picking:{pickRenderStateCache:{}}};
    if(log)command=ref.DerivedCommand.createLogDepthCommand(command,context).command;
    if(hdr&&pass==='render')command=ref.DerivedCommand.createHdrCommand(command,context).command;
    if(pass==='pick')command=ref.DerivedCommand.createPickDerivedCommand(scene,command,context).pickCommand;
    if(pass==='depth')command=ref.DerivedCommand.createDepthOnlyDerivedCommand(scene,command,context).depthOnlyCommand;
    const state=new ref.PassState(context);state.viewport=new ref.BoundingRectangle(0,0,8,8);
    new ref.ClearCommand({color:ref.Color.BLACK,depth:1,stencil:0}).execute(context,state);command.execute(context,state);
    const expected=Array.from(context.readPixels({x:4,y:4,width:1,height:1}));const actual=JSON.parse(wasm.scene_draw_variants(gl,JSON.stringify(config)));
    if(JSON.stringify(actual.pixels)!==JSON.stringify(expected)||actual.executed!==1||gl.getError()!==0)throw Error(JSON.stringify({config,actual,expected,glError:gl.getError()}));passed.push(config);context.destroy();
  }
  for(const webgl1 of [false,true])for(const union of [false,true])for(const log of [false,true]){
    window.lastSceneCase={clip:true,webgl1,union,log};const raw=canvas().getContext(webgl1?'webgl':'webgl2',attributes);
    const gl=webgl1?new Proxy(raw,{get(t,p){if(p==='getExtension')return name=>name==='OES_texture_float'?null:t.getExtension(name);const v=Reflect.get(t,p,t);return typeof v==='function'?v.bind(t):v;}}):raw;
    const context=new ref.Context(canvas(),{webgl:attributes,requestWebgl1:webgl1});if(webgl1){context._textureFloat=undefined;context._gl.getError();}
    const collection=new ref.ClippingPlaneCollection({planes:[new ref.ClippingPlane(ref.Cartesian3.UNIT_X,0),new ref.ClippingPlane(ref.Cartesian3.UNIT_Y,0)],unionClippingRegions:union,edgeColor:ref.Color.RED,edgeWidth:1.25});
    context.uniformState._frameState={mode:3};context.uniformState._mode=3;context.uniformState.updateFrustum(new ref.PerspectiveFrustum({fov:Math.PI/3,aspectRatio:1,near:log?.1:1,far:log?1e8:1000}));
    const image=new ref.Texture({context,source:{width:1,height:1,arrayBufferView:new Uint8Array([200,100,50,255])},flipY:false,sampler:ref.Sampler.NEAREST});const state=new ref.PassState(context);state.viewport=new ref.BoundingRectangle(0,0,8,8);const expected=[];
    for(let phase=0;phase<6;phase++){window.lastSceneCase.phase=phase;
      if(phase===1)collection.get(0).distance=.25;if(phase===2){collection.get(0).distance=-.25;collection.get(1).distance=.25;}if(phase===3)collection.remove(collection.get(1));if(phase===4)collection.removeAll();if(phase===5)collection.add(new ref.ClippingPlane(ref.Cartesian3.UNIT_Y,-.1));collection.update({context});
      const defines=[],sources=['uniform sampler2D image;uniform vec4 u_pickColor;void main(){out_FragColor=texture(image,vec2(0.5));}'];
      if(collection.length){const texture=collection.texture;defines.push(`CLIPPING_PLANES_LENGTH ${collection.length}`,`CLIPPING_PLANES_TEXTURE_WIDTH ${texture.width}`,`CLIPPING_PLANES_TEXTURE_HEIGHT ${texture.height}`);if(!webgl1)defines.push('USE_CLIPPING_PLANES_FLOAT_TEXTURE');if(union)defines.push('UNION_CLIPPING_REGIONS');sources[0]=ref.ShaderSource.replaceMain(sources[0],'original_main');sources.push('uniform sampler2D model_clippingPlanes;uniform vec4 model_clippingPlanesEdgeStyle;uniform mat4 model_clippingPlanesMatrix;\n'+ref.ModelClippingPlanesStageFS+'\nvoid main(){original_main();modelClippingPlanesStage(out_FragColor);}');}
      let command=context.createViewportQuadCommand(new ref.ShaderSource({sources,defines}),{renderState:ref.RenderState.fromCache({}),uniformMap:{image:()=>image,model_clippingPlanes:()=>collection.texture,model_clippingPlanesEdgeStyle:()=>({x:1,y:0,z:0,w:1.25}),model_clippingPlanesMatrix:()=>ref.Matrix4.IDENTITY}});if(log)command=ref.DerivedCommand.createLogDepthCommand(command,context).command;
      new ref.ClearCommand({color:ref.Color.BLACK,depth:1,stencil:0}).execute(context,state);command.execute(context,state);const pixels=Array.from(context.readPixels({width:8,height:8}));expected.push({pixels,texture:collection.texture?[collection.texture.width,collection.texture.height]:null,float:!webgl1});
    }
    const config={clip:true,webgl1,union,log};const actual=JSON.parse(wasm.scene_draw_variants(gl,JSON.stringify(config)));if(JSON.stringify(actual)!==JSON.stringify(expected)){
      const differences=expected.map((e,i)=>({phase:i,texture:[actual[i].texture,e.texture],pixels:e.pixels.flatMap((v,p)=>v===actual[i].pixels[p]?[]:[[p,actual[i].pixels[p],v]])}));throw Error(JSON.stringify({config,differences}));
    }if(raw.getError()!==0)throw Error('Clipping GL error '+JSON.stringify(config));for(let phase=0;phase<6;phase++)passed.push({...config,phase});collection.destroy();context.destroy();
  }
  for(const skip of [false,true])for(const log of [false,true])for(const distance of [750,1000,1300])for(const preserve of [false,true]){
    const config={skip,log,distance,preserve};window.lastSceneCase={mixed:true,...config};const gl=canvas().getContext('webgl2',attributes),context=new ref.Context(canvas(),{webgl:attributes});context.uniformState._frameState={mode:3};context.uniformState.updateFrustum(new ref.PerspectiveFrustum({fov:Math.PI/3,aspectRatio:1,near:700,far:1313}));
    const shader=ref.ShaderProgram.fromCache({context,vertexShaderSource:'in vec2 position;in float distance;void main(){gl_Position=czm_projection*vec4(position*distance*0.5,-distance,1.0);}',fragmentShaderSource:new ref.ShaderSource({sources:['uniform vec4 color;void main(){out_FragColor=color;}'],defines:skip?['POLYGON_OFFSET']:[]}),attributeLocations:{position:0,distance:1}});
    const tileset={hasMixedContent:skip,_backfaceCommands:[]};const wrappers=[];
    for(const id of [0,2]){const size=id===0?1:.5,vertices=[];for(const d of id===0?[800,1200]:[distance])for(const [x,y]of [[-size,-size],[size,-size],[size,size],[-size,size]])vertices.push(x,y,d);
      const buffer=ref.Buffer.createVertexBuffer({context,typedArray:new Float32Array(vertices),usage:35044}),index=ref.Buffer.createIndexBuffer({context,typedArray:new Uint16Array(id===0?[0,1,2,0,2,3,4,6,5,4,7,6]:[0,1,2,0,2,3]),usage:35044,indexDatatype:5123}),va=new ref.VertexArray({context,attributes:[{index:0,vertexBuffer:buffer,componentsPerAttribute:2,strideInBytes:12},{index:1,vertexBuffer:buffer,componentsPerAttribute:1,offsetInBytes:8,strideInBytes:12}],indexBuffer:index});
      const command=new ref.DrawCommand({vertexArray:va,shaderProgram:shader,pass:5,modelMatrix:ref.Matrix4.IDENTITY,boundingVolume:new ref.BoundingSphere(new ref.Cartesian3(6378137,0,0),id===0?300:30),renderState:ref.RenderState.fromCache({depthTest:{enabled:true},stencilTest:ref.StencilConstants.setCesium3DTileBit(),stencilMask:128}),uniformMap:{color:()=>id===0?ref.Color.RED:ref.Color.LIME,u_polygonOffset:()=>ref.Cartesian2.ZERO}});
      wrappers.push(new ref.ModelDrawCommand({command,primitiveRenderResources:{model:{shadows:1,projectTo2D:false,content:{tileset,tile:{_selectionDepth:id===0?0:1,_finalResolution:id!==0}}},runtimePrimitive:{primitive:{material:{doubleSided:false}}},hasSkipLevelOfDetail:skip}}));
    }
    const front=[];if(skip){wrappers[1].pushCommands({mode:3},front);wrappers[0].pushCommands({mode:3},front);}else wrappers[0].pushCommands({mode:3},front);
    const state=new ref.PassState(context);state.viewport=new ref.BoundingRectangle(0,0,8,8);new ref.ClearCommand({color:ref.Color.BLACK,depth:1,stencil:0}).execute(context,state);
    if(preserve)new ref.ClearCommand({stencil:9,renderState:ref.RenderState.fromCache({stencilMask:15})}).execute(context,state);
    if(skip)new ref.ClearCommand({stencil:0,renderState:ref.RenderState.fromCache({stencilMask:112})}).execute(context,state);
    for(let command of [...tileset._backfaceCommands,...front]){if(log)command=ref.DerivedCommand.createLogDepthCommand(command,context).command;command.execute(context,state);}
    if(preserve)context.createViewportQuadCommand('void main(){out_FragColor=vec4(0.0,0.0,1.0,1.0);}',{renderState:ref.RenderState.fromCache({stencilTest:{enabled:true,reference:9,mask:15,frontFunction:514,backFunction:514}})}).execute(context,state);
    const expected=Array.from(context.readPixels({width:8,height:8})),actual=JSON.parse(wasm.scene_mixed_lod(gl,JSON.stringify(config)));
    const expectedCount=(skip?4:1)+(preserve?2:0);if(JSON.stringify(actual.pixels)!==JSON.stringify(expected)||actual.executed!==expectedCount||gl.getError()!==0){const differences=expected.flatMap((v,i)=>v===actual.pixels[i]?[]:[[i,actual.pixels[i],v]]);throw Error(JSON.stringify({config,differences,executed:actual.executed,expectedCount,frustums:actual.frustums,glError:gl.getError()}));}passed.push({mixed:true,...config});context.destroy();
  }
  const queries=[[1,1,1,1,1],[3,1,1,1,1],[1,3,1,1,1],[3,3,1,1,1],[2,2,3,3,5],[2,2,4,6,5],[0,0,3,3,5],[3.75,3.75,3,3,5],[2,2,null,null,1],[2,2,3,3,0],[1.75,2.25,5,3,2]];
  const schedule=()=>new Promise(resolve=>setTimeout(resolve,5));
  for(const webgl1 of [false,true])for(const log of [false,true])for(const hole of [false,true])for(const asynchronous of [false,true]){
    const config={webgl1,log,hole,async:asynchronous,queries};window.lastSceneCase={picking:true,...config};
    const gl=canvas().getContext(webgl1?'webgl':'webgl2',attributes),context=new ref.Context(canvas(),{webgl:attributes,requestWebgl1:webgl1});
    context.uniformState._frameState={mode:3};context.uniformState.updateFrustum(new ref.PerspectiveFrustum({fov:Math.PI/3,aspectRatio:1,near:1,far:5e8}));
    const a={name:'a'},b={name:'b'},c={name:'c'},ids=[a,b,a,c].map(o=>context.createPickId(o));
    let command=context.createViewportQuadCommand('uniform vec4 color0;uniform vec4 color1;uniform vec4 color2;uniform vec4 color3;uniform float hole;void main(){out_FragColor=vec4(0.7,0.2,0.1,0.5);if(hole>0.0&&gl_FragCoord.x>3.0&&gl_FragCoord.x<5.0&&gl_FragCoord.y>3.0&&gl_FragCoord.y<5.0){out_FragColor.a=0.0;}}',{renderState:ref.RenderState.fromCache({blending:{enabled:true}}),uniformMap:{color0:()=>ids[0].color,color1:()=>ids[1].color,color2:()=>ids[2].color,color3:()=>ids[3].color,hole:()=>hole?1:0}});
    command.pickId='(gl_FragCoord.y < 4.0 ? (gl_FragCoord.x < 4.0 ? color0 : color1) : (gl_FragCoord.x < 4.0 ? color2 : color3))';
    const scene={_depthOnlyRenderStateCache:{},picking:{pickRenderStateCache:{}}};if(log)command=ref.DerivedCommand.createLogDepthCommand(command,context).command;
    command=ref.DerivedCommand.createPickDerivedCommand(scene,command,context).pickCommand;
    const framebuffer=new ref.PickFramebuffer(context),expected=[];
    for(const [x,y,w,h,limit]of queries){const width=w??3,height=h??width,rectangle=new ref.BoundingRectangle(x*2-(width-1)*.5,8-y*2-(height-1)*.5,width,height),state=framebuffer.begin(rectangle,new ref.BoundingRectangle(0,0,8,8));
      new ref.ClearCommand({color:ref.Color.TRANSPARENT,depth:1,stencil:0}).execute(context,state);command.execute(context,state);
      let objects;if(asynchronous&&!webgl1){const request=framebuffer.endAsync(rectangle,{afterRender:{push:next=>setTimeout(next,5)}},limit);context.endFrame();objects=await request;}else{objects=framebuffer.end(rectangle,limit);context.endFrame();}
      expected.push(objects.map(o=>o.name));
    }
    const actual=JSON.parse(await wasm.scene_pick(gl,JSON.stringify(config),schedule));
    if(JSON.stringify(actual.objects)!==JSON.stringify(expected)||!actual.restored||actual.frame!==0||gl.getError()!==0)throw Error(JSON.stringify({config,actual,expected,glError:gl.getError()}));
    queries.forEach((q,i)=>passed.push({picking:true,webgl1,log,hole,async:asynchronous,query:i}));framebuffer.destroy();ids.forEach(id=>id.destroy());context.destroy();
  }return passed;
 }catch(e){throw Error(JSON.stringify({case:window.lastSceneCase,message:e.message??String(e),stack:e.stack}));}
 });console.log(`Scene GPU: ${result.length} pixel comparisons passed (derived shaders, float/byte clipping, mixed-LOD depth/stencil, classification bits and sync/async object picking).`);
}finally{if(browser)await browser.close();await new Promise(r=>server.close(r));}
