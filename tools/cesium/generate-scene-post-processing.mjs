// Dedicated fixture generator for Cesium Scene post-processing batch.
import fs from 'node:fs';
import PostProcessStage from '@cesium/engine/Source/Scene/PostProcessStage.js';
import PostProcessStageComposite from '@cesium/engine/Source/Scene/PostProcessStageComposite.js';
import PostProcessStageLibrary from '@cesium/engine/Source/Scene/PostProcessStageLibrary.js';
import AutoExposure from '@cesium/engine/Source/Scene/AutoExposure.js';
import PostProcessStageCollection from '@cesium/engine/Source/Scene/PostProcessStageCollection.js';
import Tonemapper from '@cesium/engine/Source/Scene/Tonemapper.js';

const out = {
  source: '@cesium/engine 23.0.0',
  library: {},
  autoExposure: {
    name: 'czm_autoexposure',
    minimumLuminance: 0.1,
    maximumLuminance: 10.0,
    pyramids: [],
  },
  dependencies: [],
};

// 1. Library stages info
const blur = PostProcessStageLibrary.createBlurStage();
out.library.blur = {
  name: blur.name,
  length: blur.length,
  inputPreviousStageTexture: blur.inputPreviousStageTexture,
  stages: [
    { name: blur.get(0).name, sampleMode: blur.get(0).sampleMode },
    { name: blur.get(1).name, sampleMode: blur.get(1).sampleMode },
  ],
};

const dof = PostProcessStageLibrary.createDepthOfFieldStage();
out.library.depthOfField = {
  name: dof.name,
  length: dof.length,
  inputPreviousStageTexture: dof.inputPreviousStageTexture,
  focalDistance: dof.uniforms.focalDistance,
};

const edge = PostProcessStageLibrary.createEdgeDetectionStage();
out.library.edgeDetection = {
  namePrefix: 'czm_edge_detection_',
  length: edge.uniforms.length,
};

const silhouette = PostProcessStageLibrary.createSilhouetteStage();
out.library.silhouette = {
  name: silhouette.name,
  length: silhouette.length,
  inputPreviousStageTexture: silhouette.inputPreviousStageTexture,
};

const bloom = PostProcessStageLibrary.createBloomStage();
out.library.bloom = {
  name: bloom.name,
  length: bloom.length,
  inputPreviousStageTexture: bloom.inputPreviousStageTexture,
  contrast: bloom.uniforms.contrast,
  brightness: bloom.uniforms.brightness,
  glowOnly: bloom.uniforms.glowOnly,
};

const ao = PostProcessStageLibrary.createAmbientOcclusionStage();
out.library.ambientOcclusion = {
  name: ao.name,
  length: ao.length,
  inputPreviousStageTexture: ao.inputPreviousStageTexture,
  intensity: ao.uniforms.intensity,
  bias: ao.uniforms.bias,
  lengthCap: ao.uniforms.lengthCap,
  directionCount: ao.uniforms.directionCount,
  stepCount: ao.uniforms.stepCount,
  ambientOcclusionOnly: ao.uniforms.ambientOcclusionOnly,
};

const fxaa = PostProcessStageLibrary.createFXAAStage();
out.library.fxaa = {
  name: fxaa.name,
  sampleMode: fxaa.sampleMode,
};

const tonemappers = [
  ['aces', PostProcessStageLibrary.createAcesTonemappingStage(false)],
  ['filmic', PostProcessStageLibrary.createFilmicTonemappingStage(false)],
  ['pbrNeutral', PostProcessStageLibrary.createPbrNeutralTonemappingStage(false)],
  ['reinhard', PostProcessStageLibrary.createReinhardTonemappingStage(false)],
  ['modifiedReinhard', PostProcessStageLibrary.createModifiedReinhardTonemappingStage(false)],
];
out.library.tonemappers = {};
for (const [key, stage] of tonemappers) {
  out.library.tonemappers[key] = {
    name: stage.name,
    exposure: stage.uniforms.exposure,
  };
}

out.library.blackAndWhite = {
  name: PostProcessStageLibrary.createBlackAndWhiteStage().name,
  gradations: PostProcessStageLibrary.createBlackAndWhiteStage().uniforms.gradations,
};

out.library.brightness = {
  name: PostProcessStageLibrary.createBrightnessStage().name,
  brightness: PostProcessStageLibrary.createBrightnessStage().uniforms.brightness,
};

out.library.nightVision = {
  name: PostProcessStageLibrary.createNightVisionStage().name,
};

out.library.depthView = {
  name: PostProcessStageLibrary.createDepthViewStage().name,
};

out.library.lensFlare = {
  name: PostProcessStageLibrary.createLensFlareStage().name,
  intensity: PostProcessStageLibrary.createLensFlareStage().uniforms.intensity,
  distortion: PostProcessStageLibrary.createLensFlareStage().uniforms.distortion,
  ghostDispersal: PostProcessStageLibrary.createLensFlareStage().uniforms.ghostDispersal,
  haloWidth: PostProcessStageLibrary.createLensFlareStage().uniforms.haloWidth,
  dirtAmount: PostProcessStageLibrary.createLensFlareStage().uniforms.dirtAmount,
};

// 2. AutoExposure pyramid reduction steps for various sizes
const resolutions = [
  [800, 600],
  [1920, 1080],
  [256, 256],
  [500, 300],
  [3, 3],
  [1, 1],
];

for (const [w, h] of resolutions) {
  let width = w;
  let height = h;
  const length = Math.ceil(Math.log(Math.max(width, height)) / Math.log(3.0));
  const levels = [];
  for (let i = 0; i < length; ++i) {
    width = Math.max(Math.ceil(width / 3.0), 1.0);
    height = Math.max(Math.ceil(height / 3.0), 1.0);
    levels.push([width, height]);
  }
  out.autoExposure.pyramids.push({
    input: [w, h],
    length,
    levels,
  });
}

// 3. Collection defaults
const col = new PostProcessStageCollection();
out.collection = {
  length: col.length,
  tonemapper: col.tonemapper,
  exposure: col.exposure,
  autoExposureEnabled: col._autoExposureEnabled,
};

fs.writeFileSync(
  new URL('../../tests/data/scene-post-processing.json', import.meta.url),
  JSON.stringify(out, null, 2) + '\n'
);
console.log('Post-processing fixture generated successfully');
