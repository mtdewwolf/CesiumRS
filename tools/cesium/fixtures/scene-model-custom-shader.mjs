// CustomShader, TextureUniform and CustomShaderPipelineStage reference cases.
//
// The stage is driven with the same lightweight render-resource stand-ins the
// other Stage suites use: it only reads the shader builder, the model's custom
// shader/structural metadata, alpha/lighting options and the uniform map.
import CustomShader from '@cesium/engine/Source/Scene/Model/CustomShader.js';
import TextureUniform from '@cesium/engine/Source/Scene/Model/TextureUniform.js';
import CustomShaderPipelineStage from '@cesium/engine/Source/Scene/Model/CustomShaderPipelineStage.js';
import CustomShaderMode from '@cesium/engine/Source/Scene/Model/CustomShaderMode.js';
import CustomShaderTranslucencyMode from '@cesium/engine/Source/Scene/Model/CustomShaderTranslucencyMode.js';
import LightingModel from '@cesium/engine/Source/Scene/Model/LightingModel.js';
import ShaderBuilder from '@cesium/engine/Source/Renderer/ShaderBuilder.js';
import TextureWrap from '@cesium/engine/Source/Renderer/TextureWrap.js';
import {Cartesian2, Cartesian3, Cartesian4, Matrix2, Matrix3, Matrix4} from '@cesium/engine';
import {suite, enc} from './lib.mjs';

function uniformValue(u) {
  switch (u.kind) {
    case 'bool': return u.value;
    case 'number': return u.value;
    case 'vec2': return new Cartesian2(...u.value);
    case 'vec3': return new Cartesian3(...u.value);
    case 'vec4': return new Cartesian4(...u.value);
    case 'mat2': return Matrix2.fromColumnMajorArray(u.value);
    case 'mat3': return Matrix3.fromColumnMajorArray(u.value);
    case 'mat4': return Matrix4.fromColumnMajorArray(u.value);
    case 'sampler': return new TextureUniform({typedArray: new Uint8Array(u.value), width: u.width ?? 1, height: u.height ?? 1});
    default: throw new Error(`unknown uniform kind ${u.kind}`);
  }
}

function encodedUniform(u) {
  if (u.type === 'sampler2D') return 'sampler';
  return enc(uniformValue(u));
}

function buildOptions(o) {
  const options = {};
  if (o.mode !== undefined) options.mode = o.mode;
  if (o.lightingModel !== undefined) options.lightingModel = o.lightingModel;
  if (o.translucencyMode !== undefined) options.translucencyMode = o.translucencyMode;
  if (o.vertexShaderText !== undefined) options.vertexShaderText = o.vertexShaderText;
  if (o.fragmentShaderText !== undefined) options.fragmentShaderText = o.fragmentShaderText;
  if (o.uniforms) {
    options.uniforms = {};
    for (const u of o.uniforms) options.uniforms[u.name] = {type: u.type, value: uniformValue(u)};
  }
  if (o.varyings) {
    options.varyings = {};
    for (const v of o.varyings) options.varyings[v.name] = v.type;
  }
  return options;
}

function usedSets(sets) {
  const out = {};
  for (const key of Object.keys(sets)) out[key] = Object.keys(sets[key]).sort();
  return out;
}

function customShaderSnapshot(cs) {
  const names = Object.keys(cs.uniforms).sort();
  return {
    mode: cs.mode,
    lightingModel: cs.lightingModel ?? null,
    translucencyMode: cs.translucencyMode,
    uniforms: names.map((name) => ({
      name,
      type: cs.uniforms[name].type,
      value: cs.uniforms[name].type === 'sampler2D' ? 'sampler' : enc(cs.uniforms[name].value),
    })),
    uniformMapKeys: Object.keys(cs.uniformMap).sort(),
    uniformMapValues: names.map((name) => {
      if (cs.uniforms[name].type === 'sampler2D') return cs.uniformMap[name]() === undefined ? null : 'texture';
      return enc(cs.uniformMap[name]());
    }),
    varyings: Object.keys(cs.varyings).sort().map((name) => [name, cs.varyings[name]]),
    vertex: usedSets(cs.usedVariablesVertex),
    fragment: usedSets(cs.usedVariablesFragment),
  };
}

function structLines(sb, ids) {
  const lines = [];
  for (const id of ids) lines.push(...sb._structs[id].generateGlslLines());
  return lines;
}

function functionLines(sb, ids) {
  const lines = [];
  for (const id of ids) lines.push(...sb._functions[id].generateGlslLines());
  return lines;
}

/** Reproduces ShaderBuilder.buildShaderProgram's source assembly without a GL context. */
function shaderSnapshot(sb) {
  const v = sb._vertexShaderParts;
  const f = sb._fragmentShaderParts;
  return {
    vertexDefines: v.defineLines,
    fragmentDefines: f.defineLines,
    vertexSource: [
      ...(sb._positionAttributeLine === undefined ? [] : [sb._positionAttributeLine]),
      ...sb._attributeLines,
      ...v.uniformLines,
      ...v.varyingLines,
      ...structLines(sb, v.structIds),
      ...functionLines(sb, v.functionIds),
      ...v.shaderLines,
    ].join('\n'),
    fragmentSource: [
      ...f.uniformLines,
      ...f.varyingLines,
      ...structLines(sb, f.structIds),
      ...functionLines(sb, f.functionIds),
      ...f.shaderLines,
    ].join('\n'),
  };
}

function stageResources(customShader) {
  return {
    shaderBuilder: new ShaderBuilder(),
    model: {customShader, structuralMetadata: undefined},
    alphaOptions: {pass: undefined},
    lightingOptions: {lightingModel: LightingModel.UNLIT},
    uniformMap: {},
  };
}

function stageAttribute(a) {
  const attribute = {
    semantic: a.semantic,
    type: a.type,
    componentDatatype: a.componentDatatype ?? 5126,
    normalized: false,
  };
  if (a.setIndex !== undefined) attribute.setIndex = a.setIndex;
  if (a.name !== undefined) attribute.name = a.name;
  return attribute;
}

export function sceneModelCustomShader() {
  const s = suite('ModelCustomShader');

  s.add('construct', [{}], (o) => customShaderSnapshot(new CustomShader(buildOptions(o))));

  s.add(
    'constructAll',
    [
      {
        mode: CustomShaderMode.REPLACE_MATERIAL,
        lightingModel: LightingModel.PBR,
        translucencyMode: CustomShaderTranslucencyMode.TRANSLUCENT,
        uniforms: [
          {name: 'u_float', kind: 'number', type: 'float', value: 0.25},
          {name: 'u_int', kind: 'number', type: 'int', value: 7},
          {name: 'u_bool', kind: 'bool', type: 'bool', value: true},
          {name: 'u_vec2', kind: 'vec2', type: 'vec2', value: [1, 2]},
          {name: 'u_vec3', kind: 'vec3', type: 'vec3', value: [3, 4, 5]},
          {name: 'u_vec4', kind: 'vec4', type: 'vec4', value: [6, 7, 8, 9]},
          {name: 'u_mat2', kind: 'mat2', type: 'mat2', value: [1, 2, 3, 4]},
          {name: 'u_mat3', kind: 'mat3', type: 'mat3', value: [1, 2, 3, 4, 5, 6, 7, 8, 9]},
          {name: 'u_mat4', kind: 'mat4', type: 'mat4', value: Array.from({length: 16}, (_, i) => i)},
          {name: 'u_sampler', kind: 'sampler', type: 'sampler2D', value: [9, 8, 7, 255]},
        ],
        varyings: [
          {name: 'v_selectedColor', type: 'vec3'},
          {name: 'v_uv', type: 'vec2'},
          {name: 'v_factor', type: 'float'},
        ],
        vertexShaderText:
          'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) {\n  v_selectedColor = vsInput.attributes.color_0.rgb;\n  vsOutput.positionMC += 0.1 * vsInput.attributes.normalMC;\n}',
        fragmentShaderText:
          'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n  material.diffuse = texture(u_sampler, fsInput.attributes.texCoord_0).rgb;\n  material.alpha = u_float * fsInput.attributes.color_0.a;\n}',
      },
    ],
    (o) => customShaderSnapshot(new CustomShader(buildOptions(o))),
  );

  s.add(
    'vertexOnly',
    [
      {
        uniforms: [{name: 'u_position', kind: 'vec3', type: 'vec3', value: [1, 0, 0]}],
        vertexShaderText:
          'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) {\n  vsOutput.positionMC += u_position;\n}',
      },
    ],
    (o) => customShaderSnapshot(new CustomShader(buildOptions(o))),
  );

  s.add(
    'fragmentOnly',
    [
      {
        fragmentShaderText:
          'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n  material.diffuse = fsInput.attributes.normalEC;\n  material.roughness = fsInput.attributes.positionEC.z;\n}',
      },
    ],
    (o) => customShaderSnapshot(new CustomShader(buildOptions(o))),
  );

  s.add(
    'usedVariables',
    [
      {
        vertexShaderText: [
          'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) {',
          '  vec3 a = vsInput.attributes.positionMC + vsInput.attributes.normalMC + vsInput.attributes.featureId_0;',
          '  vec3 b = vsInput.attributes.positionMC + vsInput.attributes._CUSTOM_NAME;',
          '  float m = vsInput.metadata.height + vsInput.metadataClass.height + vsInput.metadataStatistics.height;',
          '}',
        ].join('\n'),
        fragmentShaderText: [
          'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {',
          '  material.diffuse = fsInput.attributes.positionWC + fsInput.attributes.normalEC;',
          '  material.emissive = vec3(fsInput.metadata.height + fsInput.metadataStatistics.height);',
          '  material.metallic = fsInput.material.metallic;',
          '}',
        ].join('\n'),
      },
    ],
    (o) => customShaderSnapshot(new CustomShader(buildOptions(o))),
  );

  // The setUniform contract: declared uniforms update in place, sampler2D
  // queues an upload, undeclared names throw.
  s.add(
    'setUniform',
    [
      {
        uniforms: [
          {name: 'u_float', kind: 'number', type: 'float', value: 1},
          {name: 'u_vec3', kind: 'vec3', type: 'vec3', value: [1, 2, 3]},
          {name: 'u_sampler', kind: 'sampler', type: 'sampler2D', value: [1, 2, 3, 4]},
        ],
        ops: [
          {name: 'u_float', kind: 'number', type: 'float', value: 5},
          {name: 'u_vec3', kind: 'vec3', type: 'vec3', value: [4, 5, 6]},
          {name: 'u_sampler', kind: 'sampler', type: 'sampler2D', value: [5, 6, 7, 8, 9, 10, 11, 12], width: 2, height: 1},
          {name: 'u_missing', kind: 'number', type: 'float', value: 1},
        ],
      },
    ],
    (o) => {
      const shader = new CustomShader(buildOptions(o));
      const steps = [];
      for (const op of o.ops) {
        try {
          shader.setUniform(op.name, uniformValue(op));
          steps.push({value: customShaderSnapshot(shader)});
        } catch (e) {
          steps.push({throws: true});
        }
      }
      return steps;
    },
  );

  // Invalid built-in variables. Upstream raises DeveloperError from the
  // constructor; the native test compares every message exactly.
  const invalid = [
    [{vertexShaderText: 'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) { vec3 p = vsInput.attributes.position; }'}, 'position'],
    [{vertexShaderText: 'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) { vec3 p = vsInput.attributes.normal; }'}, 'normal'],
    [{vertexShaderText: 'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) { vec3 p = vsInput.attributes.tangent; }'}, 'tangent'],
    [{vertexShaderText: 'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) { vec3 p = vsInput.attributes.bitangent; }'}, 'bitangent'],
    [{vertexShaderText: 'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) { vec3 p = vsInput.attributes.positionWC; }'}, 'positionWC'],
    [{vertexShaderText: 'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) { vec3 p = vsInput.attributes.positionEC; }'}, 'positionEC'],
    [{vertexShaderText: 'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) { vec3 p = vsInput.attributes.normalEC; }'}, 'normalEC'],
    [{fragmentShaderText: 'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) { vec3 p = fsInput.attributes.position; }'}, 'position'],
    [{fragmentShaderText: 'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) { vec3 p = fsInput.attributes.normal; }'}, 'normal'],
    [{fragmentShaderText: 'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) { vec3 p = fsInput.attributes.normalMC; }'}, 'normalMC'],
    [{fragmentShaderText: 'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) { vec3 p = fsInput.attributes.tangentMC; }'}, 'tangentMC'],
    [{fragmentShaderText: 'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) { vec3 p = fsInput.attributes.bitangentMC; }'}, 'bitangentMC'],
  ];
  for (const [options, variable] of invalid) {
    s.add('invalidVariable', [options], (o) => new CustomShader(buildOptions(o)));
    s.add('invalidVariableName', [{options, variable}], (o) => {
      new CustomShader(buildOptions(o.options));
      return o.variable;
    });
  }

  const textureUniform = (o) => {
    const options = {};
    if (o.typedArray !== undefined) options.typedArray = new Uint8Array(o.typedArray);
    if (o.width !== undefined) options.width = o.width;
    if (o.height !== undefined) options.height = o.height;
    if (o.url !== undefined) options.url = o.url;
    if (o.repeat !== undefined) options.repeat = o.repeat;
    if (o.pixelFormat !== undefined) options.pixelFormat = o.pixelFormat;
    if (o.pixelDatatype !== undefined) options.pixelDatatype = o.pixelDatatype;
    if (o.minificationFilter !== undefined) options.minificationFilter = o.minificationFilter;
    if (o.magnificationFilter !== undefined) options.magnificationFilter = o.magnificationFilter;
    if (o.maximumAnisotropy !== undefined) options.maximumAnisotropy = o.maximumAnisotropy;
    const t = new TextureUniform(options);
    return {
      typedArray: t.typedArray === undefined ? null : Array.from(t.typedArray),
      width: t.width ?? null,
      height: t.height ?? null,
      pixelFormat: t.pixelFormat,
      pixelDatatype: t.pixelDatatype,
      wrap: [t.sampler.wrapS === TextureWrap.REPEAT, t.sampler.wrapT === TextureWrap.REPEAT],
      min: t.sampler.minificationFilter,
      mag: t.sampler.magnificationFilter,
      anisotropy: t.sampler.maximumAnisotropy,
      hasResource: t.resource !== undefined,
    };
  };

  s.add('textureUniform', [{typedArray: [1, 2, 3, 4], width: 1, height: 1}], textureUniform);
  s.add('textureUniform', [{typedArray: [1], width: 2, height: 2, repeat: false}], textureUniform);
  s.add('textureUniform', [{url: 'https://example.com/a.png'}], textureUniform);
  s.add('textureUniform', [{url: 'https://example.com/a.png', repeat: false, maximumAnisotropy: 4}], textureUniform);
  s.add('textureUniform', [{typedArray: [1, 2], width: 1, height: 1, pixelFormat: 6407, pixelDatatype: 5121}], textureUniform);
  s.add('textureUniform', [{typedArray: [1, 2], width: 1, height: 1, minificationFilter: 9987, magnificationFilter: 9728}], textureUniform);

  // Note: the samplerCube rejection is a debug-only precondition upstream
  // (release builds fall through to the scalar uniform path), so it is a
  // native contract test rather than a fixture case.

  // ---- pipeline stage -------------------------------------------------

  const stageAttributeSets = {
    full: [
      {semantic: 'POSITION', type: 'VEC3'},
      {semantic: 'NORMAL', type: 'VEC3'},
      {semantic: 'TANGENT', type: 'VEC4'},
      {semantic: 'TEXCOORD', setIndex: 0, type: 'VEC2'},
      {semantic: 'COLOR', setIndex: 0, type: 'VEC4'},
      {name: '_CUSTOM', type: 'VEC3'},
    ],
    position: [{semantic: 'POSITION', type: 'VEC3'}],
    empty: [],
  };

  const stageCases = [
    {
      name: 'vertexAndFragment',
      options: {
        uniforms: [{name: 'u_color', kind: 'vec4', type: 'vec4', value: [1, 0, 0, 1]}],
        varyings: [{name: 'v_color', type: 'vec4'}],
        vertexShaderText:
          'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) {\n  v_color = vsInput.attributes.color_0;\n}',
        fragmentShaderText:
          'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n  material.diffuse = v_color.rgb * u_color.rgb;\n}',
      },
      attributes: stageAttributeSets.full,
    },
    {
      name: 'defaults',
      options: {
        fragmentShaderText:
          'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n  material.diffuse = fsInput.attributes.color_1.rgb + fsInput.attributes.texCoord_0.x * fsInput.attributes.joints_0.x;\n}',
      },
      attributes: stageAttributeSets.position,
    },
    {
      name: 'positionBuiltins',
      options: {
        fragmentShaderText:
          'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n  material.diffuse = fsInput.attributes.positionWC + fsInput.attributes.positionEC;\n}',
      },
      attributes: stageAttributeSets.position,
    },
    {
      name: 'incompatibleVertex',
      options: {vertexShaderText: 'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) { vsOutput.positionMC += vsInput.attributes.unknownThing; }'},
      attributes: stageAttributeSets.position,
    },
    {
      name: 'translucent',
      options: {
        translucencyMode: CustomShaderTranslucencyMode.TRANSLUCENT,
        lightingModel: LightingModel.PBR,
        vertexShaderText: 'void vertexMain(VertexInput vsInput, inout czm_modelVertexOutput vsOutput) {}',
      },
      attributes: stageAttributeSets.empty,
    },
    {
      name: 'opaque',
      options: {translucencyMode: CustomShaderTranslucencyMode.OPAQUE},
      attributes: stageAttributeSets.empty,
      initialPass: 9,
    },
    {
      name: 'inherit',
      options: {},
      attributes: stageAttributeSets.empty,
      initialPass: 9,
    },
    {
      name: 'noText',
      options: {lightingModel: LightingModel.PBR, varyings: [{name: 'v_unused', type: 'vec2'}]},
      attributes: stageAttributeSets.empty,
    },
    {
      name: 'normalCoords',
      options: {
        fragmentShaderText:
          'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n  material.normal = fsInput.attributes.normalEC + fsInput.attributes.tangentEC;\n}',
      },
      attributes: stageAttributeSets.full,
    },
  ];

  for (const c of stageCases) {
    s.add('stage', [{...c}], (c) => {
      const shader = c.options === null ? undefined : new CustomShader(buildOptions(c.options));
      const r = stageResources(shader);
      if (c.initialPass !== undefined) r.alphaOptions.pass = c.initialPass;
      const primitive = {
        attributes: c.attributes.map(stageAttribute),
        propertyTextureIds: [],
        propertyAttributeIds: [],
        featureIds: [],
      };
      CustomShaderPipelineStage.process(r, primitive, {});
      const out = shaderSnapshot(r.shaderBuilder);
      out.pass = r.alphaOptions.pass ?? null;
      out.lightingModel = r.lightingOptions.lightingModel;
      const samplerNames = new Set(
        Object.keys((shader && shader.uniforms) || {}).filter((name) => shader.uniforms[name].type === 'sampler2D'),
      );
      out.uniformKeys = Object.keys(r.uniformMap).sort();
      out.uniformValues = out.uniformKeys.map((name) => (samplerNames.has(name) ? 'sampler' : enc(r.uniformMap[name]())));
      return out;
    });
  }

  // Metadata compatibility: a shader reading a metadata property disables
  // itself on primitives that do not reference it.
  s.add(
    'stageMetadataIncompatible',
    [
      {
        options: {
          fragmentShaderText:
            'void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material) {\n  material.diffuse = vec3(fsInput.metadata.height);\n}',
        },
        propertyTextureIds: [],
        propertyAttributeIds: [],
        featureIds: [],
      },
    ],
    (c) => {
      const shader = new CustomShader(buildOptions(c.options));
      const r = stageResources(shader);
      const primitive = {attributes: [stageAttribute({semantic: 'POSITION', type: 'VEC3'})], ...c};
      CustomShaderPipelineStage.process(r, primitive, {});
      return {pass: r.alphaOptions.pass ?? null, ...shaderSnapshot(r.shaderBuilder)};
    },
  );

  return s;
}
