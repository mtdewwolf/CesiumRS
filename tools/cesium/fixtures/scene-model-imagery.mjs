import ImageryCoverage from '@cesium/engine/Source/Scene/Model/ImageryCoverage.js';
import ImageryInput from '@cesium/engine/Source/Scene/Model/ImageryInput.js';
import ImageryPipelineStage from '@cesium/engine/Source/Scene/Model/ImageryPipelineStage.js';
import ModelImageryMapping from '@cesium/engine/Source/Scene/Model/ModelImageryMapping.js';
import ImageryFlags from '@cesium/engine/Source/Scene/Model/ImageryFlags.js';
import GeographicTilingScheme from '@cesium/engine/Source/Core/GeographicTilingScheme.js';
import WebMercatorTilingScheme from '@cesium/engine/Source/Core/WebMercatorTilingScheme.js';
import Rectangle from '@cesium/engine/Source/Core/Rectangle.js';
import Cartographic from '@cesium/engine/Source/Core/Cartographic.js';
import Cartesian2 from '@cesium/engine/Source/Core/Cartesian2.js';
import Cartesian3 from '@cesium/engine/Source/Core/Cartesian3.js';
import Cartesian4 from '@cesium/engine/Source/Core/Cartesian4.js';
import Color from '@cesium/engine/Source/Core/Color.js';
import ShaderBuilder from '@cesium/engine/Source/Renderer/ShaderBuilder.js';
import BoundingRectangle from '@cesium/engine/Source/Core/BoundingRectangle.js';
import {suite, enc} from './lib.mjs';

function snapUniforms(sb) {
  return [...sb._fragmentShaderParts.uniformLines];
}

function snapFunction(sb, id) {
  const fn = sb._functions[id];
  return fn ? fn.generateGlslLines() : [];
}

export function sceneModelImagery() {
  const s = suite('ModelImagery');

  // ---- ImageryCoverage: _clampRectangle ----
  const clampCases = [
    {
      input: new Rectangle(-0.5, -0.5, 0.5, 0.5),
      clamp: new Rectangle(-1.0, -1.0, 1.0, 1.0),
    },
    {
      input: new Rectangle(2.0, 2.0, 3.0, 3.0),
      clamp: new Rectangle(-1.0, -1.0, 1.0, 1.0),
    },
    {
      input: new Rectangle(-3.0, -3.0, -2.0, -2.0),
      clamp: new Rectangle(-1.0, -1.0, 1.0, 1.0),
    },
    {
      input: new Rectangle(-2.0, -0.5, 2.0, 0.5),
      clamp: new Rectangle(-1.0, -1.0, 1.0, 1.0),
    },
    {
      input: new Rectangle(-0.2, -2.0, 0.2, 2.0),
      clamp: new Rectangle(-1.0, -1.0, 1.0, 1.0),
    },
  ];
  for (const c of clampCases) {
    s.add('coverage_clamp_rectangle', [c.input, c.clamp], (inRect, clRect) => {
      const res = ImageryCoverage._clampRectangle(inRect, clRect);
      return [res.west, res.south, res.east, res.north];
    });
  }

  // ---- ImageryCoverage: _localizeToCartesianRectangle ----
  const localCases = [
    {
      a: new Rectangle(0.1, 0.2, 0.4, 0.6),
      b: new Rectangle(0.0, 0.0, 1.0, 1.0),
    },
    {
      a: new Rectangle(0.2, 0.3, 0.8, 0.7),
      b: new Rectangle(0.1, 0.2, 0.9, 0.8),
    },
  ];
  for (const c of localCases) {
    s.add('coverage_localize_rectangle', [c.a, c.b], (rA, rB) => {
      const res = ImageryCoverage._localizeToCartesianRectangle(rA, rB);
      return [res.minX, res.minY, res.maxX, res.maxY];
    });
  }

  // ---- ImageryCoverage: _computeOverlappedRectangle ----
  for (const c of clampCases) {
    s.add('coverage_compute_overlapped_rectangle', [c.input, c.clamp], (inRect, bounds) => {
      const res = ImageryCoverage._computeOverlappedRectangle(inRect, bounds);
      return [res.west, res.south, res.east, res.north];
    });
  }

  // ---- ImageryCoverage: _computeImageryRange ----
  const geoScheme = new GeographicTilingScheme();
  const mercScheme = new WebMercatorTilingScheme();
  const rangeCases = [
    {
      input: new Rectangle(-0.5, -0.5, 0.5, 0.5),
      bounds: Rectangle.MAX_VALUE,
      scheme: 'geographic',
      level: 0,
    },
    {
      input: new Rectangle(-0.5, -0.5, 0.5, 0.5),
      bounds: Rectangle.MAX_VALUE,
      scheme: 'geographic',
      level: 1,
    },
    {
      input: new Rectangle(-0.5, -0.5, 0.5, 0.5),
      bounds: Rectangle.MAX_VALUE,
      scheme: 'geographic',
      level: 2,
    },
    {
      input: new Rectangle(-1.0, -0.8, 1.0, 0.8),
      bounds: Rectangle.MAX_VALUE,
      scheme: 'web_mercator',
      level: 1,
    },
    {
      input: new Rectangle(-1.0, -0.8, 1.0, 0.8),
      bounds: Rectangle.MAX_VALUE,
      scheme: 'web_mercator',
      level: 3,
    },
  ];
  for (const c of rangeCases) {
    const ts = c.scheme === 'geographic' ? geoScheme : mercScheme;
    s.add('coverage_compute_imagery_range', [c.input, c.bounds, c.scheme, c.level], (inRect, b, _s, lvl) => {
      const res = ImageryCoverage._computeImageryRange(inRect, b, ts, lvl);
      return [res.minX, res.minY, res.maxX, res.maxY];
    });
  }

  // ---- ModelImageryMapping: computeCartographicBoundingRectangle ----
  const cartoCases = [
    [
      new Cartographic(0.1, 0.2, 10.0),
      new Cartographic(-0.3, 0.5, 20.0),
      new Cartographic(0.4, -0.1, 5.0),
    ],
    [new Cartographic(0.5, 0.5, 0.0)],
  ];
  for (const pts of cartoCases) {
    s.add('mapping_bounding_rectangle', [pts], (positions) => {
      const res = ModelImageryMapping.computeCartographicBoundingRectangle(positions);
      return [res.west, res.south, res.east, res.north];
    });
  }

  // ---- ModelImageryMapping: computeTexCoords ----
  const texCoordCases = [
    {
      positions: [
        new Cartesian3(0.0, 0.0, 0.0),
        new Cartesian3(5.0, 5.0, 0.0),
        new Cartesian3(10.0, 10.0, 0.0),
        new Cartesian3(-2.0, 12.0, 0.0),
      ],
      boundingRectangle: new BoundingRectangle(0.0, 0.0, 10.0, 10.0),
    },
  ];
  for (const c of texCoordCases) {
    s.add('mapping_compute_tex_coords', [c.positions, c.boundingRectangle], (pos, br) => {
      const res = [];
      for (const uv of ModelImageryMapping.computeTexCoords(pos, br)) {
        res.push([uv.x, uv.y]);
      }
      return res;
    });
  }

  // ---- ModelImageryMapping: createTypedArrayFromCartesians2 ----
  const pts2 = [new Cartesian2(0.1, 0.2), new Cartesian2(0.3, 0.4), new Cartesian2(0.5, 0.6)];
  s.add('mapping_create_typed_array', [pts2], (elements) => {
    const arr = ModelImageryMapping.createTypedArrayFromCartesians2(elements.length, elements);
    return Array.from(arr);
  });

  // ---- ImageryPipelineStage: _computeTextureTranslationAndScaleFromNative ----
  const scaleCases = [
    {
      nativeBounding: new Rectangle(100, 200, 300, 400),
      nativeImagery: new Rectangle(50, 150, 450, 550),
    },
    {
      nativeBounding: new Rectangle(-50, -50, 50, 50),
      nativeImagery: new Rectangle(-100, -100, 100, 100),
    },
  ];
  for (const c of scaleCases) {
    s.add('pipeline_translation_and_scale', [c.nativeBounding, c.nativeImagery], (nb, ni) => {
      const res = ImageryPipelineStage._computeTextureTranslationAndScaleFromNative(nb, ni);
      return [res.x, res.y, res.z, res.w];
    });
  }

  // ---- ImageryPipelineStage: _computeImageryFlags & shader defines/functions ----
  const flagConfigs = [
    {}, // all defaults
    { alpha: 0.5 },
    { brightness: 1.5 },
    { contrast: 0.8 },
    { hue: 0.3 },
    { saturation: 1.2 },
    { gamma: 0.9 },
    { colorToAlpha: [0, 0, 0, 1], threshold: 0.1 },
    {
      alpha: 0.7,
      brightness: 1.2,
      contrast: 1.1,
      hue: -0.1,
      saturation: 0.9,
      gamma: 1.3,
      colorToAlpha: [1, 1, 1, 1],
      threshold: 0.2,
    },
  ];

  for (const cfg of flagConfigs) {
    const mockLayer = {
      alpha: cfg.alpha ?? 1.0,
      brightness: cfg.brightness ?? 1.0,
      contrast: cfg.contrast ?? 1.0,
      hue: cfg.hue ?? 0.0,
      saturation: cfg.saturation ?? 1.0,
      gamma: cfg.gamma ?? 1.0,
      colorToAlpha: cfg.colorToAlpha ? Color.unpack(cfg.colorToAlpha) : undefined,
      colorToAlphaThreshold: cfg.threshold ?? 0.0,
    };

    s.add('pipeline_imagery_flags', [cfg], () => {
      const flags = ImageryPipelineStage._computeImageryFlags([mockLayer]);
      return {
        alpha: !!flags.alpha,
        brightness: !!flags.brightness,
        contrast: !!flags.contrast,
        hue: !!flags.hue,
        saturation: !!flags.saturation,
        gamma: !!flags.gamma,
        colorToAlpha: !!flags.colorToAlpha,
      };
    });

    s.add('pipeline_shader_generation', [cfg], () => {
      const flags = ImageryPipelineStage._computeImageryFlags([mockLayer]);
      const sb = new ShaderBuilder();
      sb.addDefine('IMAGERY_TEXTURE_UNITS 2');
      ImageryPipelineStage._defineUniforms(sb, flags);
      ImageryPipelineStage._buildSampleAndBlendFunction(sb, flags);
      const dummyInputs = [
        { imageryTexCoordAttributeSetIndex: 0 },
        { imageryTexCoordAttributeSetIndex: 1 },
      ];
      ImageryPipelineStage._createMainImageryShader(sb, dummyInputs, flags);

      return {
        uniforms: snapUniforms(sb),
        sampleAndBlend: snapFunction(sb, 'sampleAndBlend'),
        blendBaseColorWithImagery: snapFunction(sb, 'blendBaseColorWithImagery'),
      };
    });
  }

  return s;
}
