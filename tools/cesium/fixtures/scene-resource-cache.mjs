// Fixtures for the glTF loader foundations: ResourceCacheKey, ResourceCacheStatistics,
// GltfLoaderUtil and the ModelComponents data classes.
import Resource from '@cesium/engine/Source/Core/Resource.js';
import ResourceCacheKey from '@cesium/engine/Source/Scene/ResourceCacheKey.js';
import ResourceCacheStatistics from '@cesium/engine/Source/Scene/ResourceCacheStatistics.js';
import GltfLoaderUtil from '@cesium/engine/Source/Scene/GltfLoaderUtil.js';
import ModelComponents from '@cesium/engine/Source/Scene/ModelComponents.js';
import {suite, enc} from './lib.mjs';

const GLTF_URL = 'https://cache.test/models/tree/scene.gltf?v=2';
const BASE_URLS = ['https://cache.test/models/tree/', 'https://cache.test/models/tree/scene.gltf?v=2', 'https://cdn.test/assets/base/?token=abc'];
const DATA_URI = 'data:application/octet-stream;base64,AAECAwQFBgc=';
const resource = (url) => new Resource(url);

/** A glTF document with embedded and external buffers, meshopt views, Draco, images and samplers. */
function document() {
  return {
    asset: {version: '2.0'},
    buffers: [{byteLength: 4096}, {byteLength: 512, uri: 'buffers/geometry.bin'}, {byteLength: 8, uri: DATA_URI}, {byteLength: 64, uri: '../shared/fallback.bin'}],
    bufferViews: [
      {buffer: 0, byteOffset: 0, byteLength: 120},
      {buffer: 1, byteOffset: 64, byteLength: 256, byteStride: 12},
      {buffer: 2, byteOffset: 0, byteLength: 8},
      {buffer: 0, byteOffset: 1024, byteLength: 96, extensions: {EXT_meshopt_compression: {buffer: 3, byteOffset: 16, byteLength: 40, byteStride: 12, count: 8, mode: 'ATTRIBUTES'}}},
      {buffer: 1, byteLength: 32, extensions: {EXT_meshopt_compression: {buffer: 1, byteLength: 20, count: 16, byteStride: 2, mode: 'INDICES'}}},
      {buffer: 0, byteLength: 48},
      {buffer: 3, byteOffset: 8, byteLength: 4.5},
    ],
    accessors: [
      {bufferView: 0, byteOffset: 0, componentType: 5126, type: 'VEC3', count: 10},
      {bufferView: 1, byteOffset: 12, componentType: 5123, type: 'SCALAR', count: 36},
      {bufferView: 2, byteOffset: 4, componentType: 5121, type: 'SCALAR', count: 4},
      {bufferView: 5, componentType: 5125, type: 'SCALAR', count: 12},
    ],
    images: [{uri: 'textures/bark.png'}, {bufferView: 2, mimeType: 'image/png'}, {uri: DATA_URI}, {bufferView: 3, mimeType: 'image/webp'}, {bufferView: 1, mimeType: 'image/ktx2'}],
    samplers: [{}, {wrapS: 33071, wrapT: 33648, minFilter: 9987, magFilter: 9728}, {minFilter: 9984}, {minFilter: 9986, wrapS: 10497}, {minFilter: 9985}, {minFilter: 9729, magFilter: 9729}, {wrapT: null, minFilter: 9728}],
    textures: [
      {source: 0},
      {source: 1, sampler: 1},
      {source: 0, sampler: 2, extensions: {EXT_texture_webp: {source: 3}}},
      {source: 2, sampler: 3, extensions: {KHR_texture_basisu: {source: 4}}},
      {source: 0, sampler: 4, extensions: {EXT_texture_webp: {source: 3}, KHR_texture_basisu: {source: 4}}},
      {sampler: 5, extensions: {KHR_texture_basisu: {source: 4}}},
      {source: 1, sampler: 6},
      {source: 2, extensions: {EXT_texture_webp: {}}},
    ],
  };
}

function cacheKeys() {
  const s = suite('ResourceCacheKey');
  const gltf = document();
  const draco = (bufferView, attributes) => ({bufferView, attributes});
  for (const url of [GLTF_URL, 'https://cache.test/a%20b/model.glb', 'models/relative.gltf', DATA_URI]) {
    s.add('externalBuffer', [url], (u) => ResourceCacheKey.getExternalBufferCacheKey({resource: resource(u)}));
    s.add('gltf', [url], (u) => ResourceCacheKey.getGltfCacheKey({gltfResource: resource(u)}));
    for (const bufferId of [0, 3]) {
      s.add('embeddedBuffer', [url, bufferId], (u, id) => ResourceCacheKey.getEmbeddedBufferCacheKey({parentResource: resource(u), bufferId: id}));
    }
    s.add('schema', [null, url], (schema, u) => ResourceCacheKey.getSchemaCacheKey({resource: resource(u)}));
  }
  for (const schema of [
    {id: 'tree', classes: {leaf: {properties: {area: {type: 'SCALAR', componentType: 'FLOAT32'}}}}},
    {classes: {b: {}, 10: {}, 2: {}, a: {}}, version: '1.0', name: 'quote "and" \\ slash\n', scale: [0.1, 1e21, -0, 1e-7, 123456789012]},
    {enums: {}, unicode: 'é ☃ \u0001'},
  ]) {
    s.add('schema', [schema, null], (sc) => ResourceCacheKey.getSchemaCacheKey({schema: sc}));
  }
  for (const baseUrl of BASE_URLS) {
    for (let bufferViewId = 0; bufferViewId < gltf.bufferViews.length; bufferViewId++) {
      s.add('bufferView', [gltf, bufferViewId, GLTF_URL, baseUrl], (g, id, gu, bu) =>
        ResourceCacheKey.getBufferViewCacheKey({gltf: g, bufferViewId: id, gltfResource: resource(gu), baseResource: resource(bu)}));
    }
    for (let imageId = 0; imageId < gltf.images.length; imageId++) {
      s.add('image', [gltf, imageId, GLTF_URL, baseUrl], (g, id, gu, bu) =>
        ResourceCacheKey.getImageCacheKey({gltf: g, imageId: id, gltfResource: resource(gu), baseResource: resource(bu)}));
    }
    for (const bufferView of [0, 1, 2, 3]) {
      s.add('draco', [gltf, draco(bufferView, {POSITION: 0}), GLTF_URL, baseUrl], (g, d, gu, bu) =>
        ResourceCacheKey.getDracoCacheKey({gltf: g, draco: d, gltfResource: resource(gu), baseResource: resource(bu)}));
    }
    s.add('spz', [gltf, GLTF_URL, baseUrl], (g, gu, bu) =>
      ResourceCacheKey.getSpzCacheKey({gltf: g, primitive: {}, gltfResource: resource(gu), baseResource: resource(bu)}));
  }
  const vertexOptions = [
    {bufferViewId: 0, loadBuffer: true},
    {bufferViewId: 1, loadTypedArray: true},
    {bufferViewId: 3, dequantize: true, loadBuffer: true, loadTypedArray: true},
    {bufferViewId: 2, attributeSemantic: 'POSITION', loadTypedArray: true},
    {draco: draco(1, {POSITION: 0, NORMAL: 1}), attributeSemantic: 'NORMAL', loadBuffer: true},
    {draco: draco(0, {POSITION: 0}), attributeSemantic: 'POSITION', dequantize: true, loadTypedArray: true},
    {spz: {}, attributeSemantic: 'POSITION', loadTypedArray: true},
    {spz: {}, attributeSemantic: '_SCALE', loadBuffer: true, loadTypedArray: true},
    // Release builds compute these too; the debug checks below record DeveloperErrors instead.
    {bufferViewId: 0},
    {bufferViewId: 0, draco: draco(0, {POSITION: 0}), attributeSemantic: 'POSITION', loadBuffer: true},
    {draco: draco(0, {POSITION: 0}), loadBuffer: true},
    {draco: draco(0, {POSITION: 0}), attributeSemantic: 'COLOR_0', loadBuffer: true},
  ];
  for (const baseUrl of [BASE_URLS[0], BASE_URLS[2]]) {
    for (const options of vertexOptions) {
      s.add('vertexBuffer', [gltf, GLTF_URL, baseUrl, 'context-7', options], (g, gu, bu, contextId, o) =>
        ResourceCacheKey.getVertexBufferCacheKey({...o, gltf: g, gltfResource: resource(gu), baseResource: resource(bu), frameState: {context: {id: contextId}}}));
    }
    for (const [accessorId, dracoExtension, loadBuffer, loadTypedArray] of [
      [0, null, true, false], [1, null, false, true], [2, null, true, true], [3, null, true, false],
      [0, draco(1, {POSITION: 0}), true, false], [1, draco(3, {}), false, true], [1, null, false, false],
    ]) {
      s.add('indexBuffer', [gltf, accessorId, GLTF_URL, baseUrl, 'context-7', dracoExtension, loadBuffer, loadTypedArray], (g, id, gu, bu, contextId, d, lb, lt) =>
        ResourceCacheKey.getIndexBufferCacheKey({gltf: g, accessorId: id, gltfResource: resource(gu), baseResource: resource(bu), frameState: {context: {id: contextId}}, draco: d ?? undefined, loadBuffer: lb, loadTypedArray: lt}));
    }
    for (let index = 0; index < gltf.textures.length; index++) {
      for (const formats of [{webp: false, basis: false}, {webp: true, basis: false}, {webp: false, basis: true}, {webp: true, basis: true}]) {
        s.add('texture', [gltf, {index, texCoord: 1}, GLTF_URL, baseUrl, formats, 'ctx'], (g, info, gu, bu, f, contextId) =>
          ResourceCacheKey.getTextureCacheKey({gltf: g, textureInfo: info, gltfResource: resource(gu), baseResource: resource(bu), supportedImageFormats: f, frameState: {context: {id: contextId}}}));
      }
    }
  }
  // Views as the loader sees them before addDefaults: a missing byteOffset prints as "undefined".
  const sparse = {buffers: [{byteLength: 8}], bufferViews: [{buffer: 0, byteLength: 8}], accessors: [{bufferView: 0, componentType: 5126, type: 'SCALAR', count: 2}]};
  s.add('bufferView', [sparse, 0, GLTF_URL, BASE_URLS[0]], (g, id, gu, bu) =>
    ResourceCacheKey.getBufferViewCacheKey({gltf: g, bufferViewId: id, gltfResource: resource(gu), baseResource: resource(bu)}));
  s.add('indexBuffer', [sparse, 0, GLTF_URL, BASE_URLS[0], 'c', null, false, true], (g, id, gu, bu, contextId, d, lb, lt) =>
    ResourceCacheKey.getIndexBufferCacheKey({gltf: g, accessorId: id, gltfResource: resource(gu), baseResource: resource(bu), frameState: {context: {id: contextId}}, loadBuffer: lb, loadTypedArray: lt}));
  return s;
}

function statistics() {
  const s = suite('ResourceCacheStatistics');
  const sequences = [
    [['geometry', 'a', 100, null], ['geometry', 'b', null, 24], ['geometry', 'c', 40, 12], ['texture', 't', 64], ['remove', 'b'], ['remove', 't'], ['remove', 'missing']],
    [['geometry', 'a', 100, 4], ['geometry', 'a', 999, 999], ['texture', 'a', 8], ['remove', 'a'], ['geometry', 'a', null, null], ['clear'], ['texture', 'x', 3]],
    [['texture', 't1', 16], ['texture', 't2', 32], ['texture', 't1', 1000], ['clear'], ['remove', 't2'], ['geometry', 'g', 0, 0]],
  ];
  for (const ops of sequences) {
    s.add('sequence', [ops], (list) => {
      const stats = new ResourceCacheStatistics();
      const out = [];
      for (const [op, key, a, b] of list) {
        if (op === 'geometry') stats.addGeometryLoader({cacheKey: key, buffer: a === null ? undefined : {sizeInBytes: a}, typedArray: b === null ? undefined : {byteLength: b}});
        else if (op === 'texture') stats.addTextureLoader({cacheKey: key, texture: {sizeInBytes: a}});
        else if (op === 'remove') stats.removeLoader({cacheKey: key});
        else stats.clear();
        out.push([stats.geometryByteLength, stats.texturesByteLength]);
      }
      return out;
    });
  }
  return s;
}

function loaderUtil() {
  const s = suite('GltfLoaderUtil');
  const gltf = document();
  for (let textureId = 0; textureId < gltf.textures.length; textureId++) {
    for (const formats of [{webp: false, basis: false}, {webp: true, basis: false}, {webp: false, basis: true}, {webp: true, basis: true}]) {
      s.add('getImageIdFromTexture', [gltf, textureId, formats], (g, id, f) => GltfLoaderUtil.getImageIdFromTexture({gltf: g, textureId: id, supportedImageFormats: f}));
    }
    for (const noMipmap of [false, true]) {
      s.add('createSampler', [gltf, {index: textureId}, noMipmap], (g, info, n) => {
        const sampler = GltfLoaderUtil.createSampler({gltf: g, textureInfo: info, compressedTextureNoMipmap: n});
        return [sampler.wrapS, sampler.wrapT, sampler.wrapR, sampler.minificationFilter, sampler.magnificationFilter, sampler.maximumAnisotropy];
      });
    }
  }
  // An invalid enum value is a debug-build DeveloperError in the Sampler constructor.
  const invalid = {textures: [{sampler: 0}], samplers: [{wrapS: 1234}]};
  s.add('createSampler', [invalid, {index: 0}, false], (g, info, n) => GltfLoaderUtil.createSampler({gltf: g, textureInfo: info, compressedTextureNoMipmap: n}).wrapS);
  const texture = {sentinel: true};
  const infos = [
    {index: 0},
    {index: 2, texCoord: 1, scale: 0.5},
    {index: 1, extensions: {KHR_texture_transform: {}}},
    {index: 1, texCoord: 1, extensions: {KHR_texture_transform: {offset: [0.25, -0.5], rotation: 1.5707963267948966, scale: [2, 3], texCoord: 2}}},
    {index: 3, extensions: {KHR_texture_transform: {rotation: -0.3, scale: [0.5, 0.25]}}},
    {index: 4, strength: 0.7, extensions: {KHR_texture_transform: {offset: [1, 1], texCoord: 0}}},
    {index: 5, texCoord: 3, extensions: {KHR_texture_transform: {rotation: 2.5}}},
  ];
  for (const info of infos) {
    for (const channels of [null, 'r', 'gba']) {
      s.add('createModelTextureReader', [info, channels], (i, c) => {
        const reader = GltfLoaderUtil.createModelTextureReader({textureInfo: i, channels: c ?? undefined, texture});
        return {index: reader.index, texCoord: reader.texCoord, scale: reader.scale, transform: reader.transform, channels: reader.channels, texture: reader.texture === texture};
      });
    }
  }
  return s;
}

function components() {
  const s = suite('ModelComponents');
  const classes = ['Quantization', 'Attribute', 'Indices', 'FeatureIdAttribute', 'FeatureIdImplicitRange', 'FeatureIdTexture', 'MorphTarget', 'Primitive', 'Instances', 'Skin', 'Node', 'Scene',
    'AnimationSampler', 'AnimationTarget', 'AnimationChannel', 'Animation', 'ArticulationStage', 'Articulation', 'Asset', 'Components', 'TextureReader',
    'MetallicRoughness', 'SpecularGlossiness', 'Specular', 'Anisotropy', 'Clearcoat', 'Material'];
  for (const name of classes) s.add('defaults', [name], (n) => new ModelComponents[n]());
  s.add('animatedPropertyType', [], () => ({...ModelComponents.AnimatedPropertyType}));
  s.add('constants', [], () => ({
    metallicRoughness: [ModelComponents.MetallicRoughness.DEFAULT_BASE_COLOR_FACTOR, ModelComponents.MetallicRoughness.DEFAULT_METALLIC_FACTOR, ModelComponents.MetallicRoughness.DEFAULT_ROUGHNESS_FACTOR],
    specularGlossiness: [ModelComponents.SpecularGlossiness.DEFAULT_DIFFUSE_FACTOR, ModelComponents.SpecularGlossiness.DEFAULT_SPECULAR_FACTOR, ModelComponents.SpecularGlossiness.DEFAULT_GLOSSINESS_FACTOR],
    specular: [ModelComponents.Specular.DEFAULT_SPECULAR_FACTOR, ModelComponents.Specular.DEFAULT_SPECULAR_COLOR_FACTOR],
    anisotropy: [ModelComponents.Anisotropy.DEFAULT_ANISOTROPY_STRENGTH, ModelComponents.Anisotropy.DEFAULT_ANISOTROPY_ROTATION],
    clearcoat: [ModelComponents.Clearcoat.DEFAULT_CLEARCOAT_FACTOR, ModelComponents.Clearcoat.DEFAULT_CLEARCOAT_ROUGHNESS_FACTOR],
    material: [ModelComponents.Material.DEFAULT_EMISSIVE_FACTOR],
  }));
  return s;
}

export default function resourceCache() {
  return [cacheKeys(), statistics(), loaderUtil(), components()];
}
