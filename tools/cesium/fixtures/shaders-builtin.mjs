// Exact default-export text of every Builtin shader, and the CzmBuiltins.js
// table. The Cesium build writes each GLSL string into its module; these
// fixtures record those strings.
import czmBuiltins from "@cesium/engine/Source/Shaders/Builtin/CzmBuiltins.js";
import { shaderCases } from "./shader-cases.mjs";

const builtinName = (stem) => `czm_${stem}`;

export default async function shadersBuiltin() {
  return [
    { module: "constants", cases: await shaderCases("Builtin/Constants", builtinName) },
    { module: "functions", cases: await shaderCases("Builtin/Functions", builtinName) },
    { module: "structs", cases: await shaderCases("Builtin/Structs", builtinName) },
    {
      module: "CzmBuiltins",
      cases: [
        {
          f: "czmBuiltins",
          a: [],
          r: Object.entries(czmBuiltins).map(([name, source]) => ({ name, source })),
        },
      ],
    },
  ];
}
