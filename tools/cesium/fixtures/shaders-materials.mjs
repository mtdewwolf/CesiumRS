// Exact default-export text of every Materials shader. Scene imports these
// by file stem, so the stem is the module name.
import { shaderCases } from "./shader-cases.mjs";

export default async function shadersMaterials() {
  return [{ module: "materials", cases: await shaderCases("Materials", (stem) => stem) }];
}
