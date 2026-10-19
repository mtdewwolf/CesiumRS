// Real browser checks for the packaged wasm-bindgen adapters. Build first:
// scripts/build-browser-core.sh && node tools/cesium/browser-check.mjs
import {chromium} from 'playwright-core';
import {build} from 'esbuild';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {resolve,extname} from 'node:path';
const root=resolve(fileURLToPath(new URL('../..',import.meta.url)));
const bundle=await build({stdin:{contents:`export {default as Credit} from '@cesium/engine/Source/Core/Credit.js'; export {default as getImageFromTypedArray} from '@cesium/engine/Source/Core/getImageFromTypedArray.js'; export {default as getImagePixels} from '@cesium/engine/Source/Core/getImagePixels.js'; export {default as resizeImageToNextPowerOfTwo} from '@cesium/engine/Source/Core/resizeImageToNextPowerOfTwo.js'; export {default as loadImageFromTypedArray} from '@cesium/engine/Source/Core/loadImageFromTypedArray.js';`,resolveDir:fileURLToPath(new URL('.',import.meta.url))},bundle:true,write:false,format:'esm',platform:'browser'});
const server=createServer(async(req,res)=>{try{
 const url=new URL(req.url,'http://localhost');
 if(url.pathname==='/json'){res.setHeader('Content-Type','application/json');res.end('{"hello":"é","n":2}');return;}
 if(url.pathname==='/empty'){res.writeHead(204);res.end();return;}
 if(url.pathname==='/fail'){res.writeHead(404,{'X-Test':'retained'});res.end('missing');return;}
 if(url.pathname==='/slow'){setTimeout(()=>res.end('{}'),500);return;}
 if(url.pathname==='/jsonp'){res.setHeader('Content-Type','text/javascript');res.end(`${url.searchParams.get('callback')}({"ok":true});`);return;}
 if(url.pathname==='/Workers/echo.js'){res.setHeader('Content-Type','text/javascript');res.end(`onmessage=({data})=>{if(data.parameters.pending)return;if(data.parameters.webAssemblyConfig){postMessage({result:true});return;}if(data.parameters.fail)postMessage({id:data.id,error:{name:'Error',message:'expected failure'}});else postMessage({id:data.id,result:data.parameters});};`);return;}
 if(url.pathname==='/reference.mjs'){res.setHeader('Content-Type','text/javascript');res.end(bundle.outputFiles[0].contents);return;}
 if(url.pathname==='/'){res.setHeader('Content-Type','text/html');res.end('<!doctype html><title>Core validation</title>');return;}
 const path=resolve(root,'.'+url.pathname);if(!path.startsWith(root+'/'))throw Error('invalid path');
 res.setHeader('Content-Type',({'.mjs':'text/javascript','.js':'text/javascript','.wasm':'application/wasm','.ktx2':'application/octet-stream'})[extname(path)]??'application/octet-stream');res.end(await readFile(path));
 }catch(e){res.writeHead(404);res.end(e.message);}});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
let browser;
try{
 browser=await chromium.launch({executablePath:process.env.CHROMIUM_PATH??'/usr/bin/chromium',headless:true,args:['--no-sandbox']});
 const page=await browser.newPage();const runtimeErrors=[];page.on('pageerror',e=>runtimeErrors.push(e.message));
 await page.goto(`http://127.0.0.1:${server.address().port}/`);
 const checks=await page.evaluate(async()=>{
  const {default:init,BrowserCore}=await import('/target/browser-core/cesium_rs.js');await init();const core=new BrowserCore();
  const host=await import('/web/host/core.mjs'),ref=await import('/reference.mjs'),passed=[];
  const eq=(a,b,label)=>{if(JSON.stringify(a)!==JSON.stringify(b))throw Error(label+': '+JSON.stringify(a)+' != '+JSON.stringify(b));passed.push(label);};
  eq(await core.fetchJson('/json'),{hello:'é',n:2},'HTTP JSON through Rust scheduler');
  eq(await core.fetchJson('data:application/json,%7B%22n%22%3A3%7D'),{n:3},'data URI through Rust');
  eq(await core.fetchJson('/empty'),undefined,'HTTP 204 optional result');
  let failed=false;try{await core.fetchJson('/fail');}catch(e){failed=String(e).includes('404');}eq(failed,true,'HTTP error propagation');
  const pixels=Uint8Array.from([255,0,0,255,0,255,0,255,0,0,255,255,10,20,30,255,20,30,40,255,30,40,50,255]);
  const canvas=core.rgbaCanvas(pixels,3,2),reference=ref.getImageFromTypedArray(pixels,3,2);
  eq(Array.from(host.imagePixels(canvas,3,2)),Array.from(ref.getImagePixels(reference)),'RGBA canvas and pixels');
  const resized=host.resizeImage(canvas,4,2),refResized=ref.resizeImageToNextPowerOfTwo(reference);
  eq(Array.from(host.imagePixels(resized,4,2)),Array.from(ref.getImagePixels(refResized)),'browser canvas resampling');
  const png=new Uint8Array(await (await fetch(canvas.toDataURL())).arrayBuffer());
  for(const flipY of [false,true]){const image=await host.decodeImage(png,'image/png',flipY,true),expected=await ref.loadImageFromTypedArray({uint8Array:png,format:'image/png',flipY,skipColorSpaceConversion:true});eq(Array.from(host.imagePixels(image,3,2)),Array.from(ref.getImagePixels(expected)),'PNG decode flipY='+flipY);image.close?.();expected.close?.();}
  const image=await core.fetchImage(canvas.toDataURL(),false,true);eq(Array.from(host.imagePixels(image,3,2)),Array.from(pixels),'Resource image loading');
  for(const html of ['<a href="https://example.test">link</a><script>alert(1)</script>','<img src=x onerror="alert(1)"><a href="javascript:alert(1)">bad</a>']){const actual=core.creditElement(html,false),expected=new ref.Credit(html,false).element;eq(actual.outerHTML,expected.outerHTML,'credit sanitization '+html.slice(0,20));}
  const req={url:'/jsonp?callback=coreCheckJsonp',kind:'jsonp',functionName:'coreCheckJsonp'};
  const response=await host.startRequest(req).promise;eq(new TextDecoder().decode(response.body),'coreCheckJsonp({"ok":true});','JSONP callback');eq('coreCheckJsonp' in globalThis,false,'JSONP callback cleanup');
  const job=host.startRequest({url:'/slow',method:'GET',headers:[],withCredentials:false});job.abort();eq(await job.promise.then(()=>false,e=>e.cancelled===true),true,'XHR cancellation');
  const worker=host.createTaskWorker('echo',location.href);eq(await worker.run(1,{value:7}),{value:7},'worker dispatch');eq(await worker.run(2,{fail:true}).then(()=>false,e=>e.message==='expected failure'),true,'worker error');worker.terminate();eq(await worker.run(3,{}).then(()=>false,()=>true),true,'worker termination');
  const bytes=new Uint8Array(await(await fetch('/tests/data/ktx2/Green4x4Mipmap_ETC1S.ktx2')).arrayBuffer());const texture=await core.transcodeKtx2(bytes,{etc:true});eq(texture.length,3,'WASM Basis mipmaps');
  const fixture=await(await fetch('/tests/fixtures/core/KTX2Transcoder.json')).json();
  for(const format of ['astc','bc7']){
   const bytes=new Uint8Array(await(await fetch('/tests/data/ktx2/Cesium_Logo_ETC1S.ktx2')).arrayBuffer());
   const actual=await core.transcodeKtx2(bytes,{[format]:true});
   const expected=fixture.cases.find(c=>c.a[0]==='Cesium_Logo_ETC1S.ktx2'&&Object.keys(c.a[1]).length===1&&c.a[1][format]).r[0][0];
   eq([actual.internalFormat,actual.width,actual.height,Array.from(actual.bufferView)],[expected[1],expected[3],expected[4],expected[5]],'WASM legacy ETC1S '+format+' parity');
  }
  const lifecycle=await import('/target/browser-validation/validation.js');await lifecycle.default();
  await lifecycle.validate_workers(location.href);passed.push('Rust browser worker initialization, capacity, errors and destroy');
  const video=()=>{const v=document.createElement('video');Object.defineProperties(v,{readyState:{value:2},duration:{value:20},currentTime:{value:0,writable:true}});return v;};
  await lifecycle.validate_dropped_io(location.href);passed.push('unpolled HTTP/image futures abort without unhandled rejections');
  lifecycle.validate_video(video(),video());passed.push('Rust video clock, seek events, replacement and subscription cleanup');
  core.free();return passed;
 });
 await page.waitForTimeout(50);if(runtimeErrors.length)throw Error('Unhandled browser errors: '+runtimeErrors.join('; '));
 console.log(`Chromium: ${checks.length} checks passed`);for(const c of checks)console.log('  '+c);
}finally{await browser?.close();await new Promise(resolve=>server.close(resolve));}
