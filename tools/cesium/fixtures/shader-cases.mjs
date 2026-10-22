// Shared by the shaders-<directory>.mjs fixture files. The filename does not
// start with `shaders-`, so generate.mjs does not load it as a directory.
//
// Every module in a Source/Shaders group is discovered from the installed
// package, so a module the port has not registered fails its replay instead of
// being skipped.
import { readdirSync } from "node:fs";
import { fileURLToPath, pathToFileURL } from "node:url";

const shadersDir = fileURLToPath(new URL("../node_modules/@cesium/engine/Source/Shaders/", import.meta.url));

// `{ f: "shader", a: [id], r: { name, source } }` for each `<group>/*.js`, in
// filename order. `name` maps a file stem to the module's name.
export async function shaderCases(group, name) {
  const cases = [];
  for (const file of readdirSync(shadersDir + group).filter((entry) => entry.endsWith(".js")).sort()) {
    const stem = file.slice(0, -".js".length);
    const source = (await import(pathToFileURL(`${shadersDir}${group}/${file}`))).default;
    if (typeof source !== "string") throw new Error(`${group}/${file} does not export a GLSL string`);
    cases.push({ f: "shader", a: [`Shaders/${group}/${stem}`], r: { name: name(stem), source } });
  }
  return cases;
}
