// Direct DebugInspector reference cases from the pinned CesiumJS engine. A
// recording context stands in for WebGL: the shader cache records the debug
// program options it is asked for and draw() evaluates the uniform map.
import * as C from '@cesium/engine';
import DebugInspector from '@cesium/engine/Source/Scene/DebugInspector.js';
import DrawCommand from '@cesium/engine/Source/Renderer/DrawCommand.js';
import ShaderSource from '@cesium/engine/Source/Renderer/ShaderSource.js';
import { writeFileSync } from 'node:fs';

const colorValue = (c) => [c.red, c.green, c.blue, c.alpha];

function harness() {
  const requests = [];
  const draws = [];
  const context = {
    shaderCache: {
      getShaderProgram(options) {
        requests.push({
          vertex: { defines: options.vertexShaderSource.defines, sources: options.vertexShaderSource.sources },
          fragment: { defines: options.fragmentShaderSource.defines, sources: options.fragmentShaderSource.sources },
          attributeLocations: options.attributeLocations,
        });
        return { id: `debug${requests.length}` };
      },
    },
    draw(command) {
      const uniforms = {};
      for (const [name, f] of Object.entries(command.uniformMap)) {
        const v = f();
        uniforms[name] = v instanceof C.Color ? colorValue(v) : v;
      }
      draws.push({ program: command.shaderProgram.id, uniforms });
    },
  };
  return { requests, draws, context };
}

const program = (id, fragmentSources, attributes) => ({
  id,
  vertexShaderSource: new ShaderSource({ defines: ['VS'], sources: ['in vec3 position; void main() { gl_Position = vec4(position, 1.0); }'] }),
  fragmentShaderSource: new ShaderSource({ defines: ['FS'], sources: fragmentSources }),
  vertexAttributes: Object.fromEntries(attributes.map(([name, index]) => [name, { index, name }])),
});

const fragments = {
  single: ['void main() { out_FragColor = vec4(1.0); }'],
  multi: ['uniform float a;\nvoid main()\n{\n    out_FragData_1 = vec4(a);\n    out_FragData_0 = vec4(1.0);\n    out_FragData_1.a = 0.5;\n}', 'float helper() { return out_FragData_3.x + out_FragData_0.y; }'],
  noMain: ['vec4 shade() { return vec4(0.0); }', 'void main(void) { out_FragColor = shade(); }'],
};

const cases = [];
for (const [scene, seed] of [[{ debugShowCommands: false, debugShowFrustums: false }, 0], [{ debugShowCommands: true, debugShowFrustums: false }, 11], [{ debugShowCommands: false, debugShowFrustums: true }, 0], [{ debugShowCommands: true, debugShowFrustums: true }, 42]]) {
  C.Math.setRandomNumberSeed(seed);
  const { requests, draws, context } = harness();
  const inspector = new DebugInspector();
  const sceneObject = { ...scene, context };
  const programs = {
    single: program(1, fragments.single, [['position', 0]]),
    multi: program(2, fragments.multi, [['position', 0], ['normal', 3]]),
    noMain: program(3, fragments.noMain, []),
  };
  const commands = [
    { program: 'single', overlap: 1, uniformMap: undefined },
    { program: 'multi', overlap: 6, uniformMap: { a: () => 0.25 } },
    { program: 'single', overlap: 7, uniformMap: { debugShowFrustumsColor: () => 'mine' } },
    { program: 'noMain', overlap: 0, uniformMap: undefined },
  ].map((spec) => {
    const command = new DrawCommand({ shaderProgram: programs[spec.program], uniformMap: spec.uniformMap });
    command.debugOverlappingFrustums = spec.overlap;
    return { spec, command };
  });
  // Every command twice: debug programs and random command colors are cached.
  for (let round = 0; round < 2; round++) {
    for (const { command } of commands) inspector.executeDebugShowFrustumsCommand(sceneObject, command, undefined);
  }
  cases.push({ scene, seed, commands: commands.map(({ spec }) => spec.program + ':' + spec.overlap + ':' + Object.keys(spec.uniformMap ?? {}).join(',')), requests, draws });
}

writeFileSync('../../tests/data/scene-debug-inspector.json', JSON.stringify({ fragments, cases }, null, 1) + '\n');
