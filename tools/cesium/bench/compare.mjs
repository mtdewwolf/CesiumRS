// Compares parity benchmark results: CesiumRS time as a ratio of CesiumJS
// time in the same kind of runtime (native vs Node, wasm vs CesiumJS in the
// same browser), plus correctness checks.
//
//   node tools/cesium/bench/compare.mjs [--dir target/bench-results] [--out report.md]
//
// Ratios below 1 mean CesiumRS is faster. A workload is flagged when either
// side failed its fixture check, when CesiumRS and CesiumJS checksums differ,
// or when wasm is more than 1.5x slower than CesiumJS in the same browser.

import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "../../..");
const args = process.argv.slice(2);
const value = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
const dir = value("--dir", join(root, "target/bench-results"));
const out = value("--out", join(dir, "report.md"));
const WASM_SLOWDOWN_LIMIT = Number(value("--wasm-limit", "1.5"));
const CHECKSUM_REL = 1e-9;

/** CesiumRS setup → the CesiumJS setup it is compared with. */
const PAIRS = [
  ["cesiumrs-native", "cesiumjs-node", "native ÷ Node"],
  ["cesiumrs-wasm-shipped", "cesiumjs-chromium", "wasm (shipped) ÷ Chromium"],
  ["cesiumrs-wasm-fast", "cesiumjs-chromium", "wasm (opt 3) ÷ Chromium"],
];

const reports = {};
for (const file of existsSync(dir) ? readdirSync(dir) : []) {
  if (!file.endsWith(".json")) continue;
  const report = JSON.parse(readFileSync(join(dir, file), "utf8"));
  if (report.schema === 1 && report.setup) reports[report.setup] = report;
}
if (Object.keys(reports).length === 0) {
  console.error(`no results in ${dir}; run scripts/bench.sh first`);
  process.exit(1);
}

const byId = (report) => Object.fromEntries((report?.results ?? []).map((r) => [r.id, r]));
const results = Object.fromEntries(Object.entries(reports).map(([k, v]) => [k, byId(v)]));
const ids = [...new Set(Object.values(reports).flatMap((r) => r.results.map((x) => x.id)))];

function ms(x) {
  if (x === undefined || x === null || Number.isNaN(x)) return "–";
  if (x < 0.001) return `${(x * 1e6).toFixed(0)} ns`;
  if (x < 1) return `${(x * 1e3).toFixed(x < 0.01 ? 2 : 1)} µs`;
  if (x < 100) return `${x.toFixed(2)} ms`;
  return `${x.toFixed(0)} ms`;
}
const ratio = (a, b) => (a?.medianMs > 0 && b?.medianMs > 0 ? a.medianMs / b.medianMs : null);
const fmtRatio = (r) => (r === null ? "–" : `${r.toFixed(2)}×`);
// A mismatch is still timed; it is reported under Checks.
const timed = (r) => r && typeof r.medianMs === "number";

const present = PAIRS.filter(([rs, js]) => reports[rs] && reports[js]);
// Every CesiumJS setup gets a column, paired or not.
const jsSetups = ["cesiumjs-node", "cesiumjs-chromium"].filter((js) => reports[js]);
const rsSetups = present.map(([rs]) => rs);
const flags = [];
const notes = [];

const header = ["Workload", "Cases", ...jsSetups, ...rsSetups.flatMap((rs) => [rs, PAIRS.find((p) => p[0] === rs)[2]])];
const rows = [];
for (const id of ids) {
  const any = Object.values(results).map((r) => r[id]).find((r) => r?.cases !== undefined);
  const row = [`${id}`, String(any?.cases ?? "")];
  for (const js of jsSetups) {
    const r = results[js][id];
    row.push(timed(r) ? ms(r.medianMs) : r?.status ?? "–");
  }
  for (const [rs, js] of present) {
    const a = results[rs][id];
    const b = results[js][id];
    row.push(timed(a) ? ms(a.medianMs) : a?.status ?? "–");
    const q = timed(a) && timed(b) ? ratio(a, b) : null;
    row.push(fmtRatio(q));
    if (rs.startsWith("cesiumrs-wasm") && q !== null && q > WASM_SLOWDOWN_LIMIT) {
      flags.push(`${id}: ${rs} is ${q.toFixed(2)}× CesiumJS in Chromium (limit ${WASM_SLOWDOWN_LIMIT}×)`);
    }
  }
  rows.push(row);

  for (const [setup, r] of Object.entries(results)) {
    const e = r[id];
    if (!e || (e.status !== "mismatch" && e.status !== "error")) continue;
    const detail = e.mismatches?.length ? ` (${e.mismatchCount} of ${e.checked} cases; ${e.mismatches[0]})` : e.error ? ` (${e.error.split("\n")[0]})` : "";
    if (e.status === "mismatch" && reports[setup].implementation === "cesiumjs" && setup !== "cesiumjs-node") {
      // The fixtures come from CesiumJS in Node. Another JS engine disagreeing
      // with them is drift in CesiumJS itself, not a port failure.
      notes.push(`${id}: CesiumJS in ${setup} differs from the Node-generated fixtures${detail}`);
    } else {
      flags.push(`${id}: ${setup} ${e.status}${detail}`);
    }
  }
  // Checksums: every CesiumRS setup against CesiumJS in Node (the reference run).
  const reference = results["cesiumjs-node"]?.[id];
  for (const rs of Object.keys(results).filter((s) => s.startsWith("cesiumrs"))) {
    const e = results[rs][id];
    if (!timed(e) || !timed(reference)) continue;
    const a = e.checksum;
    const b = reference.checksum;
    const rel = Math.abs(a - b) / Math.max(Math.abs(a), Math.abs(b), 1e-300);
    if (!(rel <= CHECKSUM_REL) && !(a === 0 && b === 0)) {
      flags.push(`${id}: ${rs} checksum ${a} differs from CesiumJS ${b} (relative ${rel.toExponential(2)})`);
    }
  }
}

const table = (head, body) =>
  [`| ${head.join(" | ")} |`, `| ${head.map((_, i) => (i < 1 ? "---" : "---:")).join(" | ")} |`, ...body.map((r) => `| ${r.join(" | ")} |`)].join("\n");

const firstHead = ["Workload", ...Object.keys(reports)];
const firstRows = ids.map((id) => [id, ...Object.keys(reports).map((s) => (timed(results[s][id]) ? ms(results[s][id].firstMs) : "–"))]);

const env = Object.values(reports)
  .map((r) => `- **${r.setup}**: ${r.runtime}; ${r.host?.os}/${r.host?.arch}, ${r.host?.cpus} CPUs; ${r.settings?.samples} samples of ≥${r.settings?.sampleMs} ms${r.debugBuild ? "; **debug build**" : ""}`)
  .join("\n");

const md = `# CesiumJS vs CesiumRS parity benchmark

Median time per pass over each workload's inputs. Ratios are CesiumRS ÷ CesiumJS in the same kind of runtime; below 1× means CesiumRS is faster.

${table(header, rows)}

## Checks

${flags.length ? flags.map((f) => `- ${f}`).join("\n") : "- All fixture checks passed and every CesiumRS checksum matches CesiumJS."}
${notes.length ? `\n### CesiumJS engine drift\n\n${notes.map((n) => `- ${n}`).join("\n")}\n` : ""}
## First pass (cold, in workload order)

${table(firstHead, firstRows)}

## Setups

${env}
`;

writeFileSync(out, md);
console.log(md);
console.error(`wrote ${out}`);
process.exitCode = flags.some((f) => /mismatch|error|checksum/.test(f)) ? 1 : 0;
