import FrustumCommands from '@cesium/engine/Source/Scene/FrustumCommands.js';
import Pass from '@cesium/engine/Source/Renderer/Pass.js';
import {suite} from './lib.mjs';

export default function sceneFrustumCommands() {
  const s = suite('FrustumCommands');
  const encode = (f) => ({
    near: f.near,
    far: f.far,
    passes: f.commands.length,
    indices: f.indices,
    lengths: f.commands.map((commands) => commands.length),
  });

  for (const [near, far] of [[undefined, undefined], [1.5, 9], [0, 0], [-3, 5], [1e-6, 1e9]])
    s.add('construct', [near ?? null, far ?? null], (near, far) => {
      const f = new FrustumCommands(near ?? undefined, far ?? undefined);
      return {...encode(f), numberOfPasses: Pass.NUMBER_OF_PASSES};
    });

  // Simulate insertIntoBin's slot bookkeeping without a renderer: the
  // constructor must expose one independent command array and index per pass.
  s.add('slots', [], () => {
    const f = new FrustumCommands(2, 8);
    f.indices[Pass.OPAQUE] = 2;
    f.indices[Pass.TRANSLUCENT] = 1;
    f.commands[Pass.OPAQUE].push({}, {});
    f.commands[Pass.TRANSLUCENT].push({});
    return {...encode(f), opaqueIndex: f.indices[Pass.OPAQUE], translucentIndex: f.indices[Pass.TRANSLUCENT]};
  });

  return [s];
}
