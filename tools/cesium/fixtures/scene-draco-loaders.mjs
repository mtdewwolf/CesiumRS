// DracoLoader and GltfDracoLoader through the real decodeDraco worker. Only the
// TaskProcessor transport is replaced: initWebAssemblyModule resolves with a
// chosen result and scheduleTask posts the task to the worker module in-process.
import {createRequire} from 'node:module';
import TaskProcessor from '@cesium/engine/Source/Core/TaskProcessor.js';
import Resource from '@cesium/engine/Source/Core/Resource.js';
import DracoLoader from '@cesium/engine/Source/Scene/DracoLoader.js';
import GltfDracoLoader from '@cesium/engine/Source/Scene/GltfDracoLoader.js';
import GltfBufferViewLoader from '@cesium/engine/Source/Scene/GltfBufferViewLoader.js';
import BufferLoader from '@cesium/engine/Source/Scene/BufferLoader.js';
import {enc} from './lib.mjs';
import {invoke, loadWorker} from './workers-terrain.mjs';
import {present} from './workers-decode-draco.mjs';

const require = createRequire(new URL('../package.json', import.meta.url));
const draco3d = require('draco3d');
const URL_ = 'https://draco.test/model.gltf';

/** Lets pending promise callbacks run, as a frame boundary does. */
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function encode(encoderModule) {
  const builder = new encoderModule.MeshBuilder();
  const mesh = new encoderModule.Mesh();
  const n = 4;
  const positions = new Float32Array(n * n * 3);
  const normals = new Float32Array(n * n * 3);
  const uvs = new Float32Array(n * n * 2);
  const colors = new Float32Array(n * n * 3);
  for (let i = 0; i < n * n; i++) {
    const x = i % n;
    const y = Math.floor(i / n);
    positions.set([x * 1.5, y * 0.5, Math.sin(i) * 2], i * 3);
    normals.set([0, Math.sin(i * 0.3), Math.cos(i * 0.3)], i * 3);
    uvs.set([x / (n - 1), y / (n - 1)], i * 2);
    colors.set([x / n, y / n, 0.5], i * 3);
  }
  const faces = [];
  for (let y = 0; y < n - 1; y++) for (let x = 0; x < n - 1; x++) {
    const a = y * n + x;
    faces.push(a, a + 1, a + n, a + 1, a + n + 1, a + n);
  }
  const ids = {
    POSITION: builder.AddFloatAttributeToMesh(mesh, encoderModule.POSITION, n * n, 3, positions),
    NORMAL: builder.AddFloatAttributeToMesh(mesh, encoderModule.NORMAL, n * n, 3, normals),
    TEXCOORD_0: builder.AddFloatAttributeToMesh(mesh, encoderModule.TEX_COORD, n * n, 2, uvs),
    COLOR_0: builder.AddFloatAttributeToMesh(mesh, encoderModule.COLOR, n * n, 3, colors),
  };
  builder.AddFacesToMesh(mesh, faces.length / 3, new Uint32Array(faces));
  const encoder = new encoderModule.Encoder();
  encoder.SetAttributeQuantization(encoderModule.POSITION, 14);
  encoder.SetAttributeQuantization(encoderModule.NORMAL, 10);
  encoder.SetAttributeQuantization(encoderModule.TEX_COORD, 12);
  encoder.SetAttributeQuantization(encoderModule.COLOR, 8);
  encoder.SetSpeedOptions(5, 5);
  const out = new encoderModule.DracoInt8Array();
  const length = encoder.EncodeMeshToDracoBuffer(mesh, out);
  const raw = new Uint8Array(length);
  for (let i = 0; i < length; i++) raw[i] = out.GetValue(i) & 0xff;
  for (const object of [out, encoder, mesh, builder]) encoderModule.destroy(object);
  return {raw: Array.from(raw), ids};
}

function resetDraco() {
  DracoLoader._decoderTaskProcessor = undefined;
  DracoLoader._taskProcessorReady = false;
  DracoLoader._error = undefined;
}

export default async function dracoLoaders() {
  const worker = await loadWorker('decodeDraco');
  await invoke(worker, {webAssemblyConfig: {}});
  const encoderModule = await draco3d.createEncoderModule();
  const encoded = encode(encoderModule);
  const original = {init: TaskProcessor.prototype.initWebAssemblyModule, schedule: TaskProcessor.prototype.scheduleTask, fetch: BufferLoader._fetchArrayBuffer};
  let init = 'true';
  let scheduled = [];
  TaskProcessor.prototype.initWebAssemblyModule = async function (options) {
    scheduled.push({init: {name: this._workerPath, wasmBinaryFile: options.wasmBinaryFile}});
    if (init === 'reject') throw new Error('Worker failed to start');
    return init === 'true';
  };
  TaskProcessor.prototype.scheduleTask = function (parameters, transfer) {
    scheduled.push({
      task: Object.keys(parameters).sort(),
      transfers: transfer.length,
      bytes: (parameters.array ?? parameters.buffer).length,
      bufferView: parameters.bufferView ?? null,
      compressedAttributes: parameters.compressedAttributes ?? null,
      dequantizeInShader: parameters.dequantizeInShader ?? null,
      attributesToSkipTransform: parameters.attributesToSkipTransform ?? null,
    });
    return invoke(worker, parameters).then(({result}) => result);
  };

  const draco = {cases: [], module: 'DracoLoader'};
  const gltfDraco = {cases: [], module: 'GltfDracoLoader'};
  try {
    for (const result of ['true', 'false', 'reject']) {
      for (const kind of ['bufferView', 'pointCloud']) {
        resetDraco();
        init = result;
        scheduled = [];
        const calls = [];
        const parameters = kind === 'bufferView'
          ? {array: new Uint8Array(encoded.raw), bufferView: {byteLength: encoded.raw.length}, compressedAttributes: {POSITION: encoded.ids.POSITION}}
          : {buffer: new Uint8Array([1, 2, 3]), properties: {}};
        for (let i = 0; i < 3; i++) {
          let call;
          try {
            const task = kind === 'bufferView' ? DracoLoader.decodeBufferView({...parameters}) : DracoLoader.decodePointCloud({...parameters});
            call = task === undefined ? 'undefined' : 'task';
            if (task) await task.catch(() => {});
          } catch (e) {
            call = {error: e.message};
          }
          calls.push(call);
          await flush();
        }
        draco.cases.push({f: 'schedule', a: [result, kind, enc(parameters)], r: enc({calls, scheduled})});
      }
    }

    const gltf = (attributes) => ({
      asset: {version: '2.0'},
      buffers: [{byteLength: encoded.raw.length + 8}, {byteLength: 64, uri: 'missing.bin'}],
      bufferViews: [{buffer: 0, byteOffset: 4, byteLength: encoded.raw.length}, {buffer: 1, byteLength: 16}],
      accessors: attributes.map(([, componentType]) => ({componentType, type: 'VEC3', count: 16})),
    });
    const scenarios = [
      {name: 'decode', attributes: [['POSITION', 5126], ['NORMAL', 5126], ['TEXCOORD_0', 5123], ['COLOR_0', 5126]]},
      {name: 'decode', attributes: [['POSITION', 5123], ['TEXCOORD_0', 5126], ['TEXCOORD_1', 5126], ['_FEATURE_ID_0', 5126], ['NORMAL', 5121]]},
      {name: 'decode', attributes: [['COLOR_0', 5126], ['POSITION', 5126]], init: 'false'},
      {name: 'corrupt', attributes: [['POSITION', 5126]]},
      {name: 'viewFailure', attributes: [['POSITION', 5126]]},
      {name: 'destroyDuringLoad', attributes: [['POSITION', 5126]]},
      {name: 'unloadAfterReady', attributes: [['POSITION', 5126], ['NORMAL', 5126]]},
    ];
    for (const scenario of scenarios) {
      resetDraco();
      init = scenario.init ?? 'true';
      scheduled = [];
      const document = gltf(scenario.attributes);
      const primitive = {attributes: Object.fromEntries(scenario.attributes.map(([name], i) => [name, i]))};
      const extension = {bufferView: scenario.name === 'viewFailure' ? 1 : 0, attributes: {POSITION: encoded.ids.POSITION, NORMAL: encoded.ids.NORMAL, TEXCOORD_0: encoded.ids.TEXCOORD_0, COLOR_0: encoded.ids.COLOR_0}};
      const bytes = new Uint8Array(encoded.raw.length + 8);
      bytes.set(encoded.raw, 4);
      if (scenario.name === 'corrupt') bytes.fill(7, 4 + 40, 4 + encoded.raw.length);
      const unloaded = [];
      BufferLoader._fetchArrayBuffer = () => Promise.reject(new Error('Request has failed. Status Code: 503'));
      const embedded = Object.assign(function () {}, {
        getEmbeddedBufferLoader: () => new BufferLoader({typedArray: bytes}),
        getExternalBufferLoader: ({resource}) => new BufferLoader({resource}),
        unload: (loader) => loader.destroy(),
      });
      const cache = Object.assign(function () {}, {
        getBufferViewLoader: ({gltf: g, bufferViewId, gltfResource, baseResource}) =>
          new GltfBufferViewLoader({resourceCache: embedded, gltf: g, bufferViewId, gltfResource, baseResource, cacheKey: `view-${bufferViewId}`}),
        unload: (loader) => {
          unloaded.push(loader.cacheKey);
          loader.destroy();
        },
      });
      const resource = new Resource(URL_);
      const loader = new GltfDracoLoader({resourceCache: cache, gltf: document, primitive, draco: extension, gltfResource: resource, baseResource: resource, cacheKey: 'draco'});
      const frameState = {};
      const steps = [];
      const snapshot = (step, extra = {}) => steps.push({step, state: loader._state, decoded: loader.decodedData !== undefined, ...extra});
      snapshot('initial');
      const promise = loader.load();
      snapshot('loading');
      if (scenario.name === 'destroyDuringLoad') loader.destroy();
      try {
        const value = await promise;
        snapshot('loaded', {resolved: value !== undefined});
      } catch (e) {
        snapshot('loaded', {error: e.message});
      }
      for (let i = 0; i < 4 && !loader.isDestroyed(); i++) {
        try {
          snapshot(`process${i}`, {ready: !!loader.process(frameState)});
        } catch (e) {
          snapshot(`process${i}`, {error: e.message.split('\n')[0]});
        }
        await flush();
      }
      const decoded = loader.decodedData;
      const result = {steps, scheduled, unloaded: [...unloaded]};
      if (decoded) {
        const plain = present({indexArray: decoded.indices, attributeData: decoded.vertexAttributes});
        result.decoded = {indices: plain.indexArray, vertexAttributes: plain.attributeData};
      }
      if (scenario.name === 'unloadAfterReady') {
        loader.unload();
        result.afterUnload = {decoded: loader.decodedData !== undefined, state: loader._state};
      }
      gltfDraco.cases.push({f: scenario.name, a: [document, primitive, extension, encoded.raw, scenario.init ?? 'true', scenario.name === 'corrupt'], r: enc(result)});
    }
  } finally {
    TaskProcessor.prototype.initWebAssemblyModule = original.init;
    TaskProcessor.prototype.scheduleTask = original.schedule;
    BufferLoader._fetchArrayBuffer = original.fetch;
    resetDraco();
  }
  return [draco, gltfDraco];
}
