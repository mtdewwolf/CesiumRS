import Megatexture from '@cesium/engine/Source/Scene/Megatexture.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
import MetadataComponentType from '@cesium/engine/Source/Scene/MetadataComponentType.js';
import { writeFileSync } from 'node:fs';

const cases = [];
for (const max of [8, 16, 64]) {
  ContextLimits._maximum3DTextureSize = max;
  for (const dimensions of [[1,1,1], [2,3,4], [4,4,2], [8,2,1], [17,1,1]]) {
    for (const bytes of [1, 4]) {
      for (const memory of [32, 1024, 65536]) {
        for (const count of [null, 1, 7, 100]) {
          let result, error;
          try {
            const d = Megatexture.get3DTextureDimension(new Cartesian3(...dimensions), bytes, memory, count ?? undefined);
            result = [d.x, d.y, d.z];
          } catch (e) { error = e.message; }
          cases.push({max, dimensions, bytes, memory, count, result, error});
        }
      }
    }
  }
}
writeFileSync(new URL('../../tests/data/scene-megatexture.json', import.meta.url), JSON.stringify(cases, null, 2) + '\n');

// Record the 3D texture boundary while Cesium manages the atlas and its slots.
const uploads = [];
const context = {
  webgl2: true,
  floatingPointTexture: true,
  _gl: {
    createTexture: () => ({}),
    deleteTexture() {},
    activeTexture() {},
    bindTexture() {},
    pixelStorei() {},
    texImage3D() {},
    texParameteri() {},
    texSubImage3D(target, level, x, y, z, width, height, depth, format, type, source) {
      uploads.push({ origin: [x, y, z], size: [width, height, depth], format, type, data: [...source] });
    },
  },
};
ContextLimits._maximum3DTextureSize = 16;
const texture = new Megatexture(context, new Cartesian3(2, 3, 2), 4, MetadataComponentType.UINT8, 1024, 6);
const sampler = () => [texture.texture.sampler.minificationFilter, texture.texture.sampler.magnificationFilter];
const samplers = [sampler()];
texture.nearestSampling = true;
samplers.push(sampler());
texture.nearestSampling = false;
samplers.push(sampler());
const slots = [];
const snapshot = (slot) => slots.push({ slot, occupied: texture.occupiedCount, full: texture.isFull() });
const data = (seed) => Uint8Array.from({length: 48}, (_, i) => (seed * 31 + i) & 255);
for (let seed = 1; seed <= 4; seed++) snapshot(texture.add(data(seed)));
for (const slot of [1, 3]) {
  texture.remove(slot);
  snapshot(slot);
}
for (let seed = 5; seed <= 8; seed++) snapshot(texture.add(data(seed)));
let fullError;
try {
  texture.add(data(9));
} catch (error) {
  fullError = error.message;
}
const {width, height, depth} = texture.texture;
const memory = texture.textureMemoryByteLength;
texture.destroy();
const atlas = { dimensions: [width, height, depth], memory, samplers, slots, uploads, fullError, destroyed: texture.isDestroyed() };
writeFileSync(new URL('../../tests/data/scene-megatexture-atlas.json', import.meta.url), JSON.stringify(atlas) + '\n');
