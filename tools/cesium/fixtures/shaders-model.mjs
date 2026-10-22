// Exact default-export text of every Model shader. Scene imports these
// by file stem, so the stem is the module name.
import { shaderCases } from "./shader-cases.mjs";

export default async function shadersModel() {
  return [{ module: "model", cases: await shaderCases("Model") }];
}
