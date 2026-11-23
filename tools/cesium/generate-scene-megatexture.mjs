import Megatexture from '@cesium/engine/Source/Scene/Megatexture.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import ContextLimits from '@cesium/engine/Source/Renderer/ContextLimits.js';
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
