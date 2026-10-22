// Exact default-export text of every PostProcessStages shader. Scene imports these
// by file stem, so the stem is the module name.
import { shaderCases } from "./shader-cases.mjs";

export default async function shadersPostProcessStages() {
  return [{ module: "post_process_stages", cases: await shaderCases("PostProcessStages") }];
}
