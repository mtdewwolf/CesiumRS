// The pinned GltfStructuralMetadataLoader and GltfMeshPrimitiveGpmLoader,
// driven through the real ResourceCache. Image decoding, schema fetches and
// GPU texture creation are host boundaries here, stubbed as in
// scene-image-texture-loaders.mjs; images are named by their size.
import Resource from '@cesium/engine/Source/Core/Resource.js';
import Cache from '@cesium/engine/Source/Scene/ResourceCache.js';
import Texture from '@cesium/engine/Source/Renderer/Texture.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
import MetadataSchema from '@cesium/engine/Source/Scene/MetadataSchema.js';
import GltfStructuralMetadataLoader from '@cesium/engine/Source/Scene/GltfStructuralMetadataLoader.js';
import GltfMeshPrimitiveGpmLoader from '@cesium/engine/Source/Scene/Model/Extensions/Gpm/GltfMeshPrimitiveGpmLoader.js';
import { suite, enc } from './lib.mjs';
import { val } from './scene-metadata.mjs';
import { encode } from './scene-metadata-tables.mjs';
import { structuralSchemaJson } from './scene-structural-metadata.mjs';

const ROOT = 'https://metadata.test/models/model.gltf';
const SCHEMA_URL = 'https://metadata.test/models/schema.json';
// Image size by URI, so a texture identifies its image.
export const IMAGE_SIZES = { 'a.png': [3, 5], 'b.png': [4, 7], 'c.png': [2, 2] };

/** Packs `{ id: { buffer, byteOffset } }` views into one buffer and glTF bufferViews indexed by id. */
export function packViews(views) {
  const ids = Object.keys(views).map(Number);
  const count = ids.length ? Math.max(...ids) + 1 : 0;
  const bytes = [];
  const bufferViews = [];
  for (let id = 0; id < count; id++) {
    while (bytes.length % 8) bytes.push(0);
    const v = views[id];
    const data = v ? v.buffer.slice(v.byteOffset) : [0];
    bufferViews.push({ buffer: 0, byteOffset: bytes.length, byteLength: data.length });
    bytes.push(...data);
  }
  return { bytes, bufferViews };
}

/** Property JSON and views for `rows` of `classId`, from buffer view `base`. */
export function columns(classId, rows, base = 0, legacy = false, schemaJson = structuralSchemaJson) {
  const schema = MetadataSchema.fromJson(schemaJson);
  const properties = {};
  const views = {};
  let next = base;
  for (const [id, r] of Object.entries(rows)) {
    const e = encode(schema.classes[classId].properties[id], r, { base: next, legacy });
    properties[id] = e.property;
    Object.assign(views, e.views);
    next += 3;
  }
  return { properties, views };
}

export const gltfFor = (bytes, bufferViews) => ({
  buffers: [{ byteLength: Math.max(bytes.length, 1) }],
  bufferViews,
  images: Object.keys(IMAGE_SIZES).map((uri) => ({ uri })),
  // Clamped and unmipmapped, so images keep their sizes.
  samplers: [{ wrapS: 33071, wrapT: 33071, minFilter: 9729 }],
  textures: [{ source: 0, sampler: 0 }, { source: 1, sampler: 0 }, { source: 2, sampler: 0 }],
});

const legacySchemaJson = {
  classes: {
    tree: { properties: { height: { type: 'FLOAT32' }, name: { type: 'STRING' } } },
    ortho: { properties: { vegetation: { type: 'UINT8', normalized: true } } },
  },
};

const readerInfo = (r) => [r.index ?? null, r.texture ? [r.texture.width, r.texture.height] : null, r.texCoord, r.channels ?? null];
const textureInfo = (t) => [t.id ?? null, t.name ?? null, t.class.id, Object.fromEntries(Object.entries(t.properties).map(([k, p]) => [k, [readerInfo(p.textureReader), val(p.offset), val(p.scale)]]))];
const tableInfo = (t) => {
  const rows = [];
  for (let i = 0; i < t.count; i++) rows.push(Object.fromEntries(t.getPropertyIds(i).map((id) => [id, val(t.getProperty(i, id))])));
  return [t.id ?? null, t.name ?? null, t.count, t.texture ? [t.texture.width, t.texture.height] : null, rows];
};
const metadataInfo = (m) =>
  m ? {
    schema: m.schema.id ?? null,
    classes: Object.keys(m.schema.classes),
    tables: m.propertyTables.map(tableInfo),
    textures: m.propertyTextures.map(textureInfo),
    attributes: m.propertyAttributes.map((a) => [a.id ?? null, a.name ?? null, a.class.id, Object.keys(a.properties)]),
  } : null;

export default async function sceneMetadataLoaders() {
  const structural = suite('GltfStructuralMetadataLoader');
  const gpm = suite('GltfMeshPrimitiveGpmLoader');
  const old = { image: Resource.prototype.fetchImage, json: Resource.prototype.fetchJson, create: Texture.create, size: ContextLimits._maximumTextureSize };
  let failSchema = false;
  Resource.prototype.fetchImage = function () {
    const name = this.url.split('/').pop();
    const [width, height] = IMAGE_SIZES[name];
    return Promise.resolve({ width, height });
  };
  Resource.prototype.fetchJson = function () {
    return failSchema ? Promise.reject(new Error('Request has failed.')) : Promise.resolve(structuredClone(structuralSchemaJson));
  };
  Texture.create = (o) => {
    const source = o.source;
    return { width: o.width ?? source.width, height: o.height ?? source.height, sizeInBytes: 4, destroyed: false, generateMipmap() {}, isDestroyed() { return this.destroyed; }, destroy() { this.destroyed = true; } };
  };
  ContextLimits._maximumTextureSize = 16384;

  // Seeds the glTF's embedded buffer, as a glb's binary chunk would.
  const setup = (bytes) => {
    Cache.clearForSpecs();
    const resource = new Resource(ROOT);
    const seed = Cache.getEmbeddedBufferLoader({ parentResource: resource, bufferId: 0, typedArray: new Uint8Array(bytes.length ? bytes : [0]) });
    return { resource, seed };
  };
  const remaining = () => Object.keys(Cache.cacheEntries).length;
  const firstLine = (e) => String(e.message).split('\n')[0];

  // suite.add is synchronous; these cases await the loaders.
  async function addCase(s, f, args, run) {
    let r;
    try {
      r = enc(await run(...args));
    } catch (e) {
      r = { throws: String(e.message ?? e), kind: e.name ?? 'Error' };
    }
    s.cases.push({ f, a: args.map(enc), r });
  }

  async function lifecycle(make, c, describe) {
    const { resource, seed } = setup(c.bytes);
    let allow = false;
    const frame = { context: { id: 'gpu', webgl2: false }, jobScheduler: { execute(job) { if (!allow) return false; job.execute(); return true; } } };
    failSchema = !!c.failSchema;
    let loader;
    try {
      loader = make(c, resource, frame);
    } catch (e) {
      Cache.unload(seed);
      throw e;
    }
    const initial = loader._state;
    const promise = loader.load();
    const loading = loader._state;
    if (c.action === 'destroy') loader.destroy();
    let resolved = null;
    let error = null;
    try {
      resolved = (await promise) !== undefined;
    } catch (e) {
      error = firstLine(e);
    }
    const loaded = loader._state;
    const result = { initial, loading, resolved, error, loaded };
    if (!error && c.action !== 'destroy') {
      result.first = loader.process(frame);
      result.deferred = loader._state;
      allow = true;
      result.ready = loader.process(frame);
      result.state = loader._state;
      result.again = loader.process(frame);
      Object.assign(result, describe(loader));
      result.processed = remaining();
      loader.unload();
      result.unloaded = describe(loader);
    }
    result.afterUnload = remaining();
    Cache.unload(seed);
    result.empty = remaining();
    return result;
  }

  try {
    // ---- GltfStructuralMetadataLoader ----
    const sensor = columns('sensor', { temp: [1, 2, 3], rgb: [[1, 2, 3], [4, 5, 6], [7, 8, 9]], level: [4, 5, 6] });
    const trees = columns('tree', { age: [7, 8], species: ['A', 'B'] }, 30);
    const full = packViews({ ...sensor.views, ...trees.views });
    const extension = {
      schema: structuralSchemaJson,
      propertyTables: [
        { name: 'sensors', class: 'sensor', count: 3, properties: sensor.properties },
        { class: 'tree', count: 2, properties: trees.properties },
      ],
      propertyTextures: [
        { class: 'sensor', name: 'pt', properties: { temp: { index: 0, channels: [0] }, level: { index: 2, texCoord: 1 } } },
        { class: 'tree', properties: { age: { index: 0, channels: [0, 1] } } },
      ],
      propertyAttributes: [{ class: 'sensor', name: 'pa', properties: { temp: { attribute: '_TEMP' } } }],
      statistics: { s: 1 },
    };
    const legacy = columns('tree', { height: [1.5, 2], name: ['oak', 'elm'] }, 0, true, legacySchemaJson);
    const legacyPacked = packViews(legacy.views);
    const legacyExtension = {
      schema: legacySchemaJson,
      featureTables: { trees: { class: 'tree', count: 2, properties: legacy.properties } },
      featureTextures: { veg: { class: 'ortho', properties: { vegetation: { channels: 'g', texture: { index: 1, texCoord: 0 } } } } },
    };
    const structuralCases = [
      { name: 'embedded', bytes: full.bytes, bufferViews: full.bufferViews, extension },
      { name: 'external', bytes: full.bytes, bufferViews: full.bufferViews, extension: { ...extension, schema: undefined, schemaUri: 'schema.json' } },
      { name: 'legacy', bytes: legacyPacked.bytes, bufferViews: legacyPacked.bufferViews, legacy: legacyExtension },
      { name: 'textureOnly', bytes: [], bufferViews: [], extension: { schema: structuralSchemaJson, propertyTextures: extension.propertyTextures } },
      { name: 'schemaFails', bytes: full.bytes, bufferViews: full.bufferViews, extension: { ...extension, schema: undefined, schemaUri: 'schema.json' }, failSchema: true },
      { name: 'badBufferView', bytes: full.bytes, bufferViews: full.bufferViews, extension: { ...extension, propertyTables: [{ class: 'sensor', count: 3, properties: { temp: { values: 99 } } }] } },
      { name: 'badTexture', bytes: [], bufferViews: [], extension: { schema: structuralSchemaJson, propertyTextures: [{ class: 'sensor', properties: { temp: { index: 7 } } }] } },
      { name: 'destroyed', bytes: full.bytes, bufferViews: full.bufferViews, extension, action: 'destroy' },
    ];
    for (const c of structuralCases) {
      const args = { ...c, extension: c.extension && JSON.parse(JSON.stringify(c.extension)) };
      await addCase(structural, c.name, [args], (spec) => lifecycle(
        (s, resource, frame) => new GltfStructuralMetadataLoader({
          gltf: gltfFor(s.bytes, s.bufferViews), extension: s.extension, extensionLegacy: s.legacy, gltfResource: resource, baseResource: resource,
          supportedImageFormats: {}, frameState: frame, asynchronous: true,
        }),
        spec,
        (l) => ({ metadata: metadataInfo(l.structuralMetadata) }),
      ));
    }

    // ---- GltfMeshPrimitiveGpmLoader ----
    const ppe = (source, index, extra = {}) => ({ ...extra, traits: { source, ...(extra.traits ?? {}) }, index });
    const gpmCases = [
      { name: 'two', extension: { ppeTextures: [ppe('SIGZ', 0, { traits: { min: 0, max: 5 }, texCoord: 1, offset: 0.5, scale: 0.01, noData: 255 }), ppe('VARX', 1)] } },
      { name: 'sameSchema', extension: { ppeTextures: [ppe('SIGZ', 0, { traits: { min: 0, max: 5 }, texCoord: 1, offset: 0.5, scale: 0.01, noData: 7 }), ppe('VARX', 2)] } },
      { name: 'newSchema', extension: { ppeTextures: [ppe('SIGR', 2, { scale: 2 })] } },
      { name: 'sharedTexture', extension: { ppeTextures: [ppe('SIGX', 1, { texCoord: 0 }), ppe('SIGY', 1, { texCoord: 1 })] } },
      { name: 'none', extension: {} },
      { name: 'badTexture', extension: { ppeTextures: [ppe('SIGX', 9)] } },
      { name: 'destroyed', extension: { ppeTextures: [ppe('SIGZ', 0)] }, action: 'destroy' },
    ];
    const local = (m) => m ? m.ppeTextures.map((t) => [t.traits.source, t.traits.min ?? null, t.traits.max ?? null, t.noData ?? null, t.offset ?? null, t.scale ?? null, t.index, t.texCoord ?? null]) : null;
    const classes = (m) => m ? Object.fromEntries(Object.entries(m.schema.classes).map(([id, c]) => [id, [c.name, Object.entries(c.properties).map(([pid, p]) => [pid, p.name, p.normalized, val(p.offset), val(p.scale), val(p.min), val(p.max)])]])) : null;
    for (const c of gpmCases) {
      await addCase(gpm, c.name, [{ ...c, bytes: [], bufferViews: [] }], (spec) => lifecycle(
        (s, resource, frame) => new GltfMeshPrimitiveGpmLoader({
          gltf: gltfFor([], []), extension: s.extension, gltfResource: resource, baseResource: resource, supportedImageFormats: {}, frameState: frame, asynchronous: true,
        }),
        spec,
        (l) => ({ local: local(l.meshPrimitiveGpmLocal), metadata: metadataInfo(l.structuralMetadata), classes: classes(l.structuralMetadata), cached: GltfMeshPrimitiveGpmLoader.ppeTexturesMetadataSchemaCache.size }),
      ));
    }
  } finally {
    Cache.clearForSpecs();
    Resource.prototype.fetchImage = old.image;
    Resource.prototype.fetchJson = old.json;
    Texture.create = old.create;
    ContextLimits._maximumTextureSize = old.size;
  }
  return [structural, gpm];
}
