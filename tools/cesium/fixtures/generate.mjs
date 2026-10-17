// Writes tests/fixtures/<package>/<Module>.json from real CesiumJS.
//
//   npm run fixtures
//
// Each file is `{ "module", "cesium", "cases": [{ "f", "a", "r" }] }`: the
// function name, encoded arguments, and encoded result (or `{ "throws" }`).

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import coreBounds from "./core-bounds.mjs";
import coreGeometry from "./core-geometry.mjs";
import coreMath from "./core-math.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, "../../../tests/fixtures");
const version = JSON.parse(readFileSync(join(here, "../node_modules/@cesium/engine/package.json"), "utf8")).version;

const packages = { core: [...coreMath(), ...coreGeometry(), ...coreBounds()] };

let total = 0;
for (const [pkg, suites] of Object.entries(packages)) {
  mkdirSync(join(out, pkg), { recursive: true });
  for (const s of suites) {
    const file = join(out, pkg, `${s.module}.json`);
    writeFileSync(file, JSON.stringify({ module: s.module, cesium: version, cases: s.cases }) + "\n");
    total += s.cases.length;
    console.log(`${pkg}/${s.module}: ${s.cases.length} cases`);
  }
}
console.log(`${total} cases`);
