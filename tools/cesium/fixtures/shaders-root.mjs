// Exact default-export text of every root `Source/Shaders/*.js` shader.
// Scene imports these by file stem, so the stem is the module name.
import { shaderCases } from "./shader-cases.mjs";

export default async function shadersRoot() {
  return [{ module: "root", cases: await shaderCases("") }];
}
