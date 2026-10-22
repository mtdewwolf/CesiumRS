// Exact default-export text of every Voxels shader. Scene imports these
// by file stem, so the stem is the module name.
import { shaderCases } from "./shader-cases.mjs";

export default async function shadersVoxels() {
  return [{ module: "voxels", cases: await shaderCases("Voxels") }];
}
