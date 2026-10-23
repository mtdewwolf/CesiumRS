// Executable Scene draws and derived shaders against real CesiumJS/WebGL2.
import {chromium} from 'playwright-core';
import {build} from 'esbuild';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve,extname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('../..',import.meta.url)));
const modules={Renderer:['Context','ShaderSource','Texture','Sampler','PassState','ClearCommand','RenderState'],Scene:['DerivedCommand'],Core:['Color','BoundingRectangle','PerspectiveFrustum']};
const contents=Object.entries(modules).flatMap(([dir,names])=>names.map(n=>`export {default as ${n}} from '@cesium/engine/Source/${dir}/${n}.js';`)).join('\n');
const bundle=await build({stdin:{contents,resolveDir:fileURLToPath(new URL('.',import.meta.url))},bundle:true,write:false,format:'esm',platform:'browser'});
const server=createServer(async(req,res)=>{try{const url=new URL(req.url,'http://localhost');if(url.pathname==='/reference.mjs'){res.setHeader('Content-Type','text/javascript');res.end(bundle.outputFiles[0].contents);return;}if(url.pathname==='/'){res.end('<!doctype html><title>Scene checks</title>');return;}const path=resolve(root,'.'+url.pathname);if(!path.startsWith(root+'/'))throw Error('Invalid path');res.setHeader('Content-Type',({'.js':'text/javascript','.mjs':'text/javascript','.wasm':'application/wasm'})[extname(path)]??'application/octet-stream');res.end(await readFile(path));}catch(e){res.writeHead(404);res.end(e.message);}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));let browser;
try{
 browser=await chromium.launch({executablePath:process.env.CHROMIUM_PATH??'/usr/bin/chromium',headless:true,args:['--no-sandbox','--use-angle=swiftshader','--enable-unsafe-swiftshader']});const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}/`);
 const result=await page.evaluate(async()=>{const wasm=await import('/target/renderer-validation/renderer.js');await wasm.default();const ref=await import('/reference.mjs');
  const canvas=()=>{const c=document.createElement('canvas');c.width=c.height=8;return c;};const attributes={antialias:false,alpha:true,stencil:true,preserveDrawingBuffer:true};const passed=[];
  for(const log of [false,true])for(const hdr of [false,true])for(const pass of ['render','pick','depth'])for(const target of [false,true]){
    const config={log,hdr,pick:pass==='pick',depth:pass==='depth',target};const gl=canvas().getContext('webgl2',attributes),context=new ref.Context(canvas(),{webgl:attributes});
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
  }return passed;
 });console.log(`Scene WebGL2: ${result.length} pixel comparisons passed (log depth, HDR, pick/depth passes, framebuffers, retained uniforms/textures).`);
}finally{if(browser)await browser.close();await new Promise(r=>server.close(r));}
