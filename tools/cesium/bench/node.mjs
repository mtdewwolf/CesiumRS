// CesiumJS (Node) side of the parity benchmark.
//
//   node tools/cesium/bench/node.mjs [--quick] [--filter <id>] [--out <file>]
//
// Writes the same JSON shape as examples/parity_bench for compare.mjs.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { availableParallelism } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { FULL, QUICK, runAll } from "./harness.mjs";
import { workloads } from "./workloads.mjs";
import { invoke, loadWorker } from "../fixtures/workers-terrain.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const value = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);

const settings = flag("--quick") ? QUICK : FULL;
const out = value("--out", null);
const source = { text: async (path) => readFileSync(join(root, path), "utf8") };

// decodeDraco runs through its real worker, as the fixture generator does.
const worker = await loadWorker("decodeDraco");
await invoke(worker, { webAssemblyConfig: {} });
const env = { decodeDraco: async (parameters) => (await invoke(worker, parameters)).result };

const results = await runAll(workloads, source, env, settings, { filter: value("--filter", ""), log: (l) => console.error(l) });

const require = createRequire(import.meta.url);
const report = {
  schema: 1,
  setup: value("--setup", "cesiumjs-node"),
  implementation: "cesiumjs",
  cesium: require("@cesium/engine/package.json").version,
  runtime: `Node ${process.version}`,
  host: { os: process.platform, arch: process.arch, cpus: availableParallelism() },
  settings,
  unixMs: Date.now(),
  results,
};
const text = JSON.stringify(report, null, 2);
if (out) {
  mkdirSync(dirname(resolve(out)), { recursive: true });
  writeFileSync(out, text);
  console.error(`wrote ${out}`);
} else {
  console.log(text);
}
