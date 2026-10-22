// Exact default-export text of every Appearances shader. Scene imports these
// by file stem, so the stem is the module name.
import { shaderCases } from "./shader-cases.mjs";

export default async function shadersAppearances() {
  return [{ module: "appearances", cases: await shaderCases("Appearances", (stem) => stem) }];
}
