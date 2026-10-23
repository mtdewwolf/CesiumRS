// Timing loop for the CesiumJS side of the parity benchmark. Mirrors
// examples/parity_bench/harness.rs step for step so the two sides' numbers are
// comparable: first pass (cold), fixture check, calibration, warm-up, samples.

export const FULL = { samples: 15, sampleMs: 25, warmupMs: 300 };
export const QUICK = { samples: 5, sampleMs: 5, warmupMs: 50 };

/** Nearest-rank percentile of sorted samples (same rule as harness.rs). */
function percentile(sorted, p) {
  if (sorted.length === 0) return NaN;
  const rank = Math.ceil(p * sorted.length);
  return sorted[Math.min(Math.max(rank, 1), sorted.length) - 1];
}

/**
 * `prepared` = { cases, run, verify, async? }. `run()` is one pass over every
 * input and returns a checksum (or a promise of one when `async`).
 */
export async function measure(prepared, settings, now = () => performance.now()) {
  const { run } = prepared;
  const pass = prepared.async ? () => run() : null;

  let start = now();
  const checksum = prepared.async ? await pass() : run();
  const firstMs = now() - start;

  const verify = await prepared.verify();

  let sink = 0;
  let passes = 1;
  for (;;) {
    start = now();
    if (prepared.async) for (let i = 0; i < passes; i++) sink += await pass();
    else for (let i = 0; i < passes; i++) sink += run();
    const elapsed = now() - start;
    if (elapsed >= settings.sampleMs || passes >= 1 << 24) break;
    passes *= 2;
  }

  const warmStart = now();
  while (now() - warmStart < settings.warmupMs) sink += prepared.async ? await pass() : run();

  const samples = [];
  for (let s = 0; s < settings.samples; s++) {
    start = now();
    if (prepared.async) for (let i = 0; i < passes; i++) sink += await pass();
    else for (let i = 0; i < passes; i++) sink += run();
    samples.push((now() - start) / passes);
  }
  globalThis.__benchSink = sink;

  const sorted = [...samples].sort((a, b) => a - b);
  const status = verify.mismatches.length ? "mismatch" : verify.checked > 0 ? "verified" : "checksum-only";
  return {
    status,
    cases: prepared.cases,
    checked: verify.checked,
    mismatchCount: verify.mismatches.length,
    mismatches: verify.mismatches.slice(0, 5),
    firstMs,
    passesPerSample: passes,
    samplesMs: samples,
    medianMs: percentile(sorted, 0.5),
    p95Ms: percentile(sorted, 0.95),
    minMs: sorted[0],
    checksum,
  };
}

/** Runs every workload whose id contains `filter`; one result entry each. */
export async function runAll(workloads, source, env, settings, { filter = "", log = () => {} } = {}) {
  const results = [];
  for (const w of workloads) {
    if (!w.id.includes(filter)) continue;
    const entry = { id: w.id, group: w.group, title: w.title, input: w.input };
    try {
      const prepared = await w.prepare(source, env);
      if (prepared.skip) {
        Object.assign(entry, { status: "skipped", reason: prepared.skip });
        log(`${w.id.padEnd(34)} skipped: ${prepared.skip}`);
      } else {
        Object.assign(entry, await measure(prepared, settings));
        log(`${w.id.padEnd(34)} ${entry.medianMs.toFixed(4).padStart(12)} ms/pass  ${String(prepared.cases).padStart(4)} cases  ${entry.status}`);
      }
    } catch (e) {
      Object.assign(entry, { status: "error", error: String(e?.stack ?? e) });
      log(`${w.id.padEnd(34)} error: ${e?.message ?? e}`);
    }
    results.push(entry);
  }
  return results;
}
