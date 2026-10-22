// Exact default-export text of one Builtin shader. The Cesium build writes
// the GLSL string into the module; this fixture records that string.
import czmPi from "@cesium/engine/Source/Shaders/Builtin/Constants/pi.js";

export default function shadersBuiltin() {
  return [
    {
      module: "pi",
      cases: [
        {
          f: "shader",
          a: ["Shaders/Builtin/Constants/pi"],
          r: { name: "czm_pi", source: czmPi },
        },
      ],
    },
  ];
}
