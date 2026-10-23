// Browser entry for the CesiumJS side of the parity benchmark; browser.mjs
// bundles it with esbuild and calls `run` in Chromium. Inputs are fetched from
// the repository root that browser.mjs serves.

import { runAll } from "./harness.mjs";
import { workloads } from "./workloads.mjs";

const source = {
  async text(path) {
    const response = await fetch(`/${path}`);
    if (!response.ok) throw new Error(`${path}: HTTP ${response.status}`);
    return response.text();
  },
};

export async function run(settings, filter = "") {
  const logs = [];
  const results = await runAll(workloads, source, {}, settings, { filter, log: (line) => logs.push(line) });
  return { results, logs };
}
