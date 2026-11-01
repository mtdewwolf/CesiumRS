// Exhaustive ShadowMapShader receive cases from the pinned CesiumJS engine.
// Every combination of the ten keyword inputs runs against four fragment
// layouts (with or without position and normal varyings). Each case gets
// distinct point, terrain and primitive biases so bias selection is checked.
// Sources are compared through a 64-bit FNV-1a hash of JSON.stringify.
import S from '@cesium/engine/Source/Scene/ShadowMapShader.js';
import ShaderSource from '@cesium/engine/Source/Renderer/ShaderSource.js';
import { writeFileSync } from 'node:fs';

const fnv = (text) => {
  let hash = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(text)) hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
  return hash.toString(16);
};

const fragment = (layout) =>
  new ShaderSource({
    defines: ['EXISTING'],
    sources: [`${layout & 1 ? 'in vec3 v_positionEC;' : ''}${layout & 2 ? 'in vec3 v_normalEC;' : ''}void main() { out_FragColor = vec4(1.0); }`],
  });

// Three bits per bias: normalShading, normalShadingSmooth > 0, normalOffset.
const bias = (bits) => ({ normalShading: !!(bits & 1), normalShadingSmooth: bits & 2 ? 0.1 : 0, normalOffset: !!(bits & 4) });

const receive = [];
for (let flags = 0; flags < 1024; flags++) {
  const f = Array.from({ length: 10 }, (_, b) => !!(flags & (1 << b)));
  const [usesDepthTexture, polygonOffsetSupported, isPointLight, isSpotLight, hasCascades, debugCascadeColors, softShadows, castShadows, isTerrain, hasTerrainNormal] = f;
  for (let layout = 0; layout < 4; layout++) {
    const mix = (Math.imul(flags * 4 + layout, 2654435761) >>> 0) >>> 7;
    const biases = { point: mix & 7, terrain: (mix >> 3) & 7, primitive: (mix >> 6) & 7 };
    const shadowMap = {
      _usesDepthTexture: usesDepthTexture,
      _polygonOffsetSupported: polygonOffsetSupported,
      _isPointLight: isPointLight,
      _isSpotLight: isSpotLight,
      _numberOfCascades: hasCascades ? 4 : 1,
      debugCascadeColors,
      softShadows,
      _pointBias: bias(biases.point),
      _terrainBias: bias(biases.terrain),
      _primitiveBias: bias(biases.primitive),
    };
    const result = S.createShadowReceiveFragmentShader(fragment(layout), shadowMap, castShadows, isTerrain, hasTerrainNormal);
    receive.push({
      flags,
      layout,
      biases,
      keyword: layout === 0 ? S.getShadowReceiveShaderKeyword(shadowMap, castShadows, isTerrain, hasTerrainNormal) : undefined,
      defines: result.defines,
      hash: fnv(JSON.stringify(result.sources)),
    });
  }
}

const receiveVertex = [];
for (let mask = 0; mask < 4; mask++) {
  const vs = new ShaderSource({ defines: ['EXISTING'], sources: ['void main() {}', 'float extra;'] });
  const result = S.createShadowReceiveVertexShader(vs, !!(mask & 1), !!(mask & 2));
  receiveVertex.push({ mask, defines: result.defines, sources: result.sources });
}

writeFileSync('../../tests/data/scene-shadow-map-shader.json', JSON.stringify({ receive, receiveVertex }) + '\n');
