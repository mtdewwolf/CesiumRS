// Writes tests/fixtures/<package>/<Module>.json from real CesiumJS.
//
//   npm run fixtures
//
// Run with TZ=UTC (the npm script sets it) so nothing depends on local time.
//
// Each file is `{ "module", "cesium", "cases": [{ "f", "a", "r" }] }`: the
// function name, encoded arguments, and encoded result (or `{ "throws" }`).

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import coreBounds from "./core-bounds.mjs";
import coreGeometry from "./core-geometry.mjs";
import coreMath from "./core-math.mjs";
import coreTime from "./core-time.mjs";
import corePacking from "./core-packing.mjs";
import coreSplines from "./core-splines.mjs";
import coreAlgorithms from "./core-algorithms.mjs";
import coreAstronomy from "./core-astronomy.mjs";
import coreGeometries from "./core-geometries.mjs";
import coreUtilities from "./core-utilities.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, "../../../tests/fixtures");
const version = JSON.parse(readFileSync(join(here, "../node_modules/@cesium/engine/package.json"), "utf8")).version;

const packages = { core: [...coreMath(), ...coreGeometry(), ...coreBounds(), ...coreTime(), ...corePacking(), ...coreSplines(), ...coreAlgorithms(), ...coreGeometries(), ...coreUtilities(), ...coreAstronomy()] };
// coreAstronomy runs last: loading EOP data adds leap seconds to JulianDate.leapSeconds.

// Optional module names avoid rewriting unrelated fixtures during porting.
const selected = new Set(process.argv.slice(2));
const known = new Set(Object.values(packages).flat().map(s => s.module));
for (const name of selected) {
  if (!known.has(name)) throw new Error(`Unknown fixture module: ${name}`);
}

let total = 0;
for (const [pkg, suites] of Object.entries(packages)) {
  mkdirSync(join(out, pkg), { recursive: true });
  for (const s of suites) {
    if (selected.size && !selected.has(s.module)) continue;
    const file = join(out, pkg, `${s.module}.json`);
    writeFileSync(file, JSON.stringify({ module: s.module, cesium: version, cases: s.cases }) + "\n");
    total += s.cases.length;
    console.log(`${pkg}/${s.module}: ${s.cases.length} cases`);
  }
}
console.log(`${total} cases`);
