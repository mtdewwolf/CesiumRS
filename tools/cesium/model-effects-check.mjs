import {chromium} from 'playwright-core';
import {build} from 'esbuild';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {resolve,extname} from 'node:path';
import {fileURLToPath} from 'node:url';
const root=resolve(fileURLToPath(new URL('../..',import.meta.url)));
const names=['Renderer/Context','Scene/BrdfLutGenerator','Scene/AutoExposure','Renderer/Framebuffer','Renderer/Texture','Renderer/PassState','Scene/PostProcessStageCollection','Scene/PostProcessStageLibrary','Scene/PostProcessStageComposite'];
const bundle=await build({stdin:{contents:names.map(n=>`export {default as ${n.split('/').at(-1)}} from './node_modules/@cesium/engine/Source/${n}.js';`).join('\n'),resolveDir:resolve(root,'tools/cesium')},bundle:true,format:'esm',write:false});
const server=createServer(async(req,res)=>{try{if(req.url==='/ref.mjs'){res.setHeader('Content-Type','text/javascript');res.end(bundle.outputFiles[0].contents);return;}if(req.url==='/'){res.end('<!doctype html>');return;}const path=resolve(root,'.'+req.url);if(!path.startsWith(root+'/'))throw Error('path');res.setHeader('Content-Type',extname(path)==='.wasm'?'application/wasm':'text/javascript');res.end(await readFile(path));}catch(e){res.writeHead(404);res.end(String(e));}});
await new Promise(r=>server.listen(0,'127.0.0.1',r));let browser;
try {
 browser=await chromium.launch({executablePath:process.env.CHROMIUM_PATH??'/opt/helium/chrome',headless:true,args:['--no-sandbox','--use-angle=swiftshader','--enable-unsafe-swiftshader']});
 const page=await browser.newPage();await page.goto(`http://127.0.0.1:${server.address().port}`);
 const result=await page.evaluate(async()=>{const wasm=await import('/target/model-effects-gpu/effects.js');await wasm.default();const ref=await import('/ref.mjs');const attrs={antialias:false,alpha:true,stencil:true,preserveDrawingBuffer:true};const canvas=size=>{const c=document.createElement('canvas');c.width=c.height=size;return c;};const results=[];
 const compare=(actual,expected,name)=>{if(actual.length!==expected.length)throw Error(name+' length');let max=0;for(let i=0;i<actual.length;i++)max=Math.max(max,Math.abs(actual[i]-expected[i]));if(max>1)throw Error(name+' max channel difference '+max);results.push({name,bytes:actual.length,maxDifference:max});};
 const context=new ref.Context(canvas(256),{webgl:attrs});const generator=new ref.BrdfLutGenerator();generator.update({context});const fb=new ref.Framebuffer({context,colorTextures:[generator.colorTexture],destroyAttachments:false});compare(wasm.brdf(canvas(256).getContext('webgl2',attrs)),context.readPixels({framebuffer:fb,width:256,height:256}),'BRDF');fb.destroy();generator.destroy();context.destroy();
 for(const kind of ['single','nested']) {const context=new ref.Context(canvas(8),{webgl:attrs});const bytes=new Uint8Array(Array.from({length:64},(_,i)=>[i*3,255-i*2,i*4,255]).flat());const color=new ref.Texture({context,source:{width:8,height:8,arrayBufferView:bytes}});const collection=new ref.PostProcessStageCollection();collection.fxaa.enabled=false;let stage=ref.PostProcessStageLibrary.createBlackAndWhiteStage();if(kind==='nested')for(let i=0;i<4;i++)stage=new ref.PostProcessStageComposite({stages:[stage]});collection.add(stage);collection.update(context,false,false);collection.clear(context);collection.execute(context,color);collection.copy(context);compare(wasm.post_process(canvas(8).getContext('webgl2',attrs),kind),context.readPixels({width:8,height:8}),'post-'+kind);collection.destroy();color.destroy();context.destroy();}
 const exposureContext=new ref.Context(canvas(8),{webgl:attrs});
 const exposureColor=new ref.Texture({context:exposureContext,source:{width:8,height:8,arrayBufferView:new Uint8Array(Array(64).fill([128,64,32,255]).flat())}});
 const exposure=new ref.AutoExposure();const expected=[];
 for(let i=0;i<3;i++){exposure.update(exposureContext);exposure.clear(exposureContext);exposure.execute(exposureContext,exposureColor);const fb=new ref.Framebuffer({context:exposureContext,colorTextures:[exposure.outputTexture],destroyAttachments:false});const pixels=exposureContext.readPixels({framebuffer:fb,width:1,height:1});
 // Cesium readPixels returns HALF_FLOAT attachment bits in Uint16Array.
 const half=b=>{const sign=b&0x8000?-1:1,e=(b>>10)&31,m=b&1023;return sign*(e===0?2**-14*(m/1024):e===31?(m?NaN:Infinity):2**(e-15)*(1+m/1024));};
 expected.push(...Array.from(pixels,v=>pixels instanceof Uint16Array?half(v):v));fb.destroy();}
 const actual=Array.from(wasm.exposure(canvas(8).getContext('webgl2',attrs)));let max=0;for(let i=0;i<actual.length;i++)max=Math.max(max,Math.abs(actual[i]-expected[i]));if(actual.length!==expected.length||max>0.00001)throw Error('exposure mismatch '+JSON.stringify({actual,expected,max}));results.push({name:'auto-exposure-three-frames',values:actual.length,maxDifference:max});exposure.destroy();exposureColor.destroy();exposureContext.destroy();
 return results;});console.log(JSON.stringify(result,null,2));
} finally {await browser?.close();server.close();}
