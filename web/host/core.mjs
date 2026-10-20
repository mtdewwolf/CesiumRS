// Browser capabilities for the Rust Core port. Algorithms, request scheduling,
// texture codecs and geocoder parsing remain in Rust.
export function startRequest(request) {
  if (request.kind === 'jsonp') {
    let script, settled = false, rejectPromise;
    const name = request.functionName;
    const cleanup = () => { delete globalThis[name]; script?.remove(); };
    const promise = new Promise((resolve, reject) => {
      rejectPromise = reject;
      globalThis[name] = value => {
        if (settled) return;
        settled = true; cleanup();
        resolve({status: value === undefined ? 204 : 200, headers: [], body: value === undefined ? new Uint8Array() : new TextEncoder().encode(`${name}(${JSON.stringify(value)});`)});
      };
      script = document.createElement('script');
      script.src = request.url;
      script.onerror = () => { if (!settled) { settled = true; cleanup(); reject({statusCode: undefined}); } };
      document.head.appendChild(script);
    });
    return {promise, abort() { if (!settled) { settled = true; cleanup(); rejectPromise({cancelled: true}); } }};
  }
  if (typeof XMLHttpRequest === 'undefined') {
    const controller = new AbortController();
    const promise = fetch(request.url, {method: request.method, headers: request.headers, body: request.body, credentials: request.withCredentials ? 'include' : 'same-origin', signal: controller.signal}).then(async response => {
      const headers = Array.from(response.headers, ([key,value]) => `${key}: ${value}`).join('\r\n');
      const body = new Uint8Array(await response.arrayBuffer());
      if (!response.ok) throw {statusCode: response.status, response: body, responseHeaders: headers};
      return {status: response.status, body, headers};
    });
    return {promise, abort() { controller.abort(); }};
  }
  const xhr = new XMLHttpRequest();
  const promise = new Promise((resolve, reject) => {
    xhr.open(request.method, request.url, true);
    xhr.responseType = 'arraybuffer';
    xhr.withCredentials = request.withCredentials;
    for (const [key, value] of request.headers) xhr.setRequestHeader(key, value);
    const error = () => ({statusCode: xhr.status || undefined, response: new Uint8Array(xhr.response || new ArrayBuffer(0)), responseHeaders: xhr.getAllResponseHeaders()});
    xhr.onload = () => {
      if ((xhr.status >= 200 && xhr.status < 300) || ((request.url.startsWith('file://') || globalThis.location?.origin === 'file://') && xhr.status === 0)) {
        resolve({status: xhr.status, body: new Uint8Array(xhr.response || new ArrayBuffer(0)), headers: xhr.getAllResponseHeaders()});
      } else reject(error());
    };
    xhr.onerror = () => reject(error());
    xhr.onabort = () => reject({cancelled: true});
    xhr.send(request.body ?? null);
  });
  return {promise, abort() { xhr.abort(); }};
}
export function abortJob(job) { job.abort(); }
const contexts = new Map();
function canvas(width, height) { const c = document.createElement('canvas'); c.width = width; c.height = height; return c; }
export function dimensions(image) { return [image.width, image.height]; }
export function imageFromRgba(bytes, width, height) {
  const c = canvas(width, height);
  c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(bytes.buffer, bytes.byteOffset, bytes.byteLength), width, height), 0, 0);
  return c;
}
export function imagePixels(image, width, height) {
  const key = `${width},${height}`;
  let context = contexts.get(key);
  if (!context) { context = canvas(width, height).getContext('2d', {willReadFrequently: true}); context.globalCompositeOperation = 'copy'; contexts.set(key, context); }
  context.drawImage(image, 0, 0, width, height);
  return new Uint8Array(context.getImageData(0, 0, width, height).data.buffer);
}
export function resizeImage(image, width, height) {
  const c = canvas(width, height);
  c.getContext('2d').drawImage(image, 0, 0, image.width, image.height, 0, 0, width, height);
  return c;
}
let bitmapOptions;
function supportsBitmapOptions() {
  if (!bitmapOptions) bitmapOptions = (async () => {
    if (typeof createImageBitmap !== 'function') return false;
    const bytes = Uint8Array.from(atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAABGdBTUEAAE4g3rEiDgAAACBjSFJNAAB6JgAAgIQAAPoAAACA6AAAdTAAAOpgAAA6mAAAF3CculE8AAAADElEQVQI12Ng6GAAAAEUAIngE3ZiAAAAAElFTkSuQmCC'), c => c.charCodeAt(0));
    let a, b;
    try {
      const blob = new Blob([bytes], {type: 'image/png'});
      [a, b] = await Promise.all([createImageBitmap(blob, {imageOrientation: 'flipY', premultiplyAlpha: 'none', colorSpaceConversion: 'none'}), createImageBitmap(blob)]);
      return imagePixels(a, 1, 1)[1] !== imagePixels(b, 1, 1)[1];
    } catch { return false; }
    finally { a?.close(); b?.close(); }
  })();
  return bitmapOptions;
}
export async function decodeImage(bytes, mimeType, flipY, skipColorSpaceConversion) {
  const blob = new Blob([bytes], {type: mimeType});
  if (await supportsBitmapOptions()) return createImageBitmap(blob, {
    imageOrientation: flipY ? 'flipY' : 'none', premultiplyAlpha: 'none', colorSpaceConversion: skipColorSpaceConversion ? 'none' : 'default'
  });
  const url = URL.createObjectURL(blob);
  try { return await new Promise((resolve, reject) => { const image = new Image(); image.onload = () => resolve(image); image.onerror = reject; image.src = url; }); }
  finally { URL.revokeObjectURL(url); }
}
export function creditElement(html, id) {
  const div = document.createElement('div'); div.className = 'cesium-credit-wrapper'; div._creditId = id; div.style.display = 'inline'; div.innerHTML = html;
  for (const a of div.querySelectorAll('a')) a.setAttribute('target', '_blank');
  return div;
}
export function setPlaybackRate(video, rate) { video.playbackRate = rate; }
export function delay(milliseconds) { return new Promise(resolve => setTimeout(resolve, milliseconds)); }
export function createTaskWorker(path, baseUrl) {
  const absolute = /^[a-z][a-z0-9+.-]*:/i.test(path) && !path.includes('#');
  const resolved = absolute ? path : new URL(`Workers/${path.replace(/\.js$/, '')}.js`, baseUrl || location.href).href;
  let shim;
  const workerPath = new URL(resolved).origin === location.origin ? resolved : (shim = URL.createObjectURL(new Blob([`import ${JSON.stringify(resolved)};`], {type: 'application/javascript'})));
  const worker = new Worker(workerPath, {type: 'module'}), pending = new Map(); let initialization, terminated = false;
  worker.onmessage = ({data}) => {
    const entry = data.id === undefined ? initialization : pending.get(data.id);
    if (!entry) return;
    if (data.id === undefined) initialization = undefined; else pending.delete(data.id);
    if (data.error) entry.reject(data.error); else entry.resolve(data.result);
  };
  worker.onerror = error => {
    for (const entry of pending.values()) entry.reject({message: error.message}); pending.clear();
    initialization?.reject({message: error.message}); initialization = undefined;
  };
  return {
    run(id, parameters) {
      if (terminated) return Promise.reject({message: 'Worker terminated.'});
      return new Promise((resolve, reject) => { pending.set(id, {resolve, reject}); try { worker.postMessage({id, baseUrl, parameters, canTransferArrayBuffer: true}); } catch (error) { pending.delete(id); reject(error); } });
    },
    async initialize(options) {
      if (terminated) throw {message: 'Worker terminated.'};
      let config;
      if (typeof WebAssembly === 'undefined') {
        if (!options.fallbackModulePath) throw {message: `This browser does not support Web Assembly, and no backup module was provided for ${path}`};
        config = {modulePath: new URL(options.fallbackModulePath, baseUrl || location.href).href};
      } else {
        const wasmBinaryFile = new URL(options.wasmBinaryFile, baseUrl || location.href).href;
        const load = startRequest({url: wasmBinaryFile, method: 'GET', headers: [], withCredentials: false});
        const response = await load.promise;
        config = {wasmBinaryFile, wasmBinary: response.body.buffer};
      }
      if (terminated) throw {message: 'Worker terminated.'};
      return new Promise((resolve, reject) => { initialization = {resolve, reject}; try { worker.postMessage({parameters: {webAssemblyConfig: config}, canTransferArrayBuffer: true}, config.wasmBinary ? [config.wasmBinary] : []); } catch (error) { initialization = undefined; reject(error); } });
    },
    cancel(id) { pending.get(id)?.reject({message: 'Task cancelled.'}); pending.delete(id); },
    terminate() { if (terminated) return; terminated = true; worker.terminate(); if (shim) URL.revokeObjectURL(shim); for (const id of pending.keys()) this.cancel(id); initialization?.reject({message: 'Worker terminated.'}); initialization = undefined; }
  };
}

function loadElement(url, credentials) {
  const image = new Image(); let rejectPromise, settled = false;
  if (!/^(data:|blob:)/i.test(url) && new URL(url, location.href).origin !== location.origin) image.crossOrigin = credentials ? 'use-credentials' : '';
  const promise = new Promise((resolve, reject) => {
    rejectPromise = reject;
    image.onload = () => { settled = true; resolve(image); };
    image.onerror = error => { settled = true; reject(error); };
    image.src = url;
  });
  return {promise, abort() { if (!settled) { image.onload = image.onerror = null; image.src = ''; settled = true; rejectPromise({message: 'Image request cancelled.'}); } }};
}
export async function decodeResourceImage(bytes, mimeType, options) {
  const blob = new Blob([bytes], {type: mimeType});
  try {
    let image;
    if (options.preferImageBitmap && await supportsBitmapOptions()) image = await createImageBitmap(blob, {
      imageOrientation: options.flipY ? 'flipY' : 'none', premultiplyAlpha: 'none', colorSpaceConversion: options.skipColorSpaceConversion ? 'none' : 'default'
    });
    else {
      const url = URL.createObjectURL(blob);
      try { image = await loadElement(url, false).promise; } finally { URL.revokeObjectURL(url); }
    }
    if (options.attachBlob) image.blob = blob;
    return image;
  } catch (error) { if (options.attachBlob) error.blob = blob; throw error; }
}
export function startImageRequest(url, credentials, options) {
  let active, cancelled = false;
  const promise = (async () => {
    if (options.preferImageBitmap && await supportsBitmapOptions()) {
      if (cancelled) throw {message: 'Image request cancelled.'};
      active = startRequest({url, method: 'GET', headers: [], withCredentials: credentials});
      const response = await active.promise;
      const mimeType = typeof response.headers === 'string' ? (/^content-type: (.*)$/mi.exec(response.headers)?.[1] ?? '') : '';
      return decodeResourceImage(response.body, mimeType, options);
    }
    if (cancelled) throw {message: 'Image request cancelled.'};
    active = loadElement(url, credentials);
    return active.promise;
  })();
  return {promise, abort() { cancelled = true; active?.abort(); }};
}

// Canvas primitives. Layout and pin geometry are calculated by Rust.
export function createCanvas(width, height) { return canvas(width, height); }
export function canvasContext(image) { return image.getContext('2d', {willReadFrequently:true}); }
export function measureCanvasText(image, text, font) {
  image.style.font = font; image.style.visibility = 'hidden'; document.body.appendChild(image);
  try { const m=canvasContext(image).measureText(text); return [m.width,m.actualBoundingBoxAscent,m.actualBoundingBoxDescent,m.actualBoundingBoxLeft]; }
  finally { image.remove(); image.style.visibility=''; }
}
export function screenListeners(element, callback) {
  element ??= document;
  const alternate=element.disableRootEvents===undefined?document:element;
  const removals=[];let timer,closed=false;
  const fire=(kind,e={})=>{
    if(closed)return;
    const rect=element===document?{left:0,top:0}:element.getBoundingClientRect();
    const result=callback({kind,now:performance.now(),x:e.clientX??0,y:e.clientY??0,left:rect.left,top:rect.top,button:e.button??0,shift:!!e.shiftKey,ctrl:!!e.ctrlKey,alt:!!e.altKey,pointerId:e.pointerId??0,pointerTouch:e.pointerType==='touch',touches:Array.from(e.changedTouches??[],t=>[t.identifier,t.clientX,t.clientY]),deltaY:e.deltaY,deltaMode:e.deltaMode??0,detail:e.detail,wheelDelta:e.wheelDelta});
    if(result.preventDefault)e.preventDefault?.();
    clearTimeout(timer);if(result.holdDeadline!==undefined)timer=setTimeout(()=>fire('hold'),Math.max(0,result.holdDeadline-performance.now()));
  };
  const on=(target,type,kind)=>{const f=e=>{if(kind==='pointerdown')e.target.setPointerCapture(e.pointerId);fire(kind,e);};target.addEventListener(type,f,{capture:false,passive:false});removals.push(()=>target.removeEventListener(type,f,false));};
  if(globalThis.PointerEvent){for(const type of['pointerdown','pointerup','pointermove'])on(element,type,type);on(element,'pointercancel','pointerup');}
  else{on(element,'mousedown','mousedown');on(alternate,'mouseup','mouseup');on(alternate,'mousemove','mousemove');on(element,'touchstart','touchstart');on(alternate,'touchend','touchend');on(alternate,'touchmove','touchmove');on(alternate,'touchcancel','touchend');}
  on(element,'dblclick','dblclick');on(element,'onwheel'in element?'wheel':document.onmousewheel!==undefined?'mousewheel':'DOMMouseScroll','wheel');
  return {destroy(){closed=true;clearTimeout(timer);for(const remove of removals)remove();}};
}
export function removeScreenListeners(job) { job.destroy(); }
