// Browser setups of the parity benchmark, in headless Chromium:
//   cesiumjs-chromium       CesiumJS bundled with esbuild
//   cesiumrs-wasm-shipped   target/bench-wasm/cesiumrs-wasm-shipped.wasm (release profile)
//   cesiumrs-wasm-fast      target/bench-wasm/cesiumrs-wasm-fast.wasm (bench profile, opt-level 3)
// A wasm setup runs only when its file exists; scripts/bench.sh builds them.
//
//   node tools/cesium/bench/browser.mjs [--quick] [--filter <id>] [--dir target/bench-results]
//
// Chromium: $CHROMIUM_PATH, else /usr/bin/chromium (as the other browser checks).
// Pages are cross-origin isolated so performance.now() has fine resolution.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { availableParallelism } from "node:os";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { build } from "esbuild";
import { chromium } from "playwright-core";
import { FULL, QUICK } from "./harness.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");
const args = process.argv.slice(2);
const value = (name, fallback) => (args.includes(name) ? args[args.indexOf(name) + 1] : fallback);
const settings = args.includes("--quick") ? QUICK : FULL;
const filter = value("--filter", "");
const outDir = value("--dir", join(root, "target/bench-results"));

const bundle = await build({
  entryPoints: [join(here, "browser-entry.mjs")],
  bundle: true,
  write: false,
  format: "esm",
  platform: "browser",
  logLevel: "error",
});

const TYPES = { ".mjs": "text/javascript", ".js": "text/javascript", ".json": "application/json", ".wasm": "application/wasm" };
const server = createServer((req, res) => {
  res.setHeader("Cross-Origin-Opener-Policy", "same-origin");
  res.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
  try {
    const url = new URL(req.url, "http://localhost");
    if (url.pathname === "/") {
      res.setHeader("Content-Type", "text/html");
      return res.end("<!doctype html><meta charset=utf-8><title>Parity benchmark</title>");
    }
    if (url.pathname === "/cesiumjs-bench.mjs") {
      res.setHeader("Content-Type", "text/javascript");
      return res.end(bundle.outputFiles[0].contents);
    }
    if (url.pathname === "/wasm-host.mjs") {
      res.setHeader("Content-Type", "text/javascript");
      return res.end(readFileSync(join(here, "wasm-host.mjs")));
    }
    const path = resolve(root, `.${decodeURIComponent(url.pathname)}`);
    if (!path.startsWith(`${root}/`)) throw new Error("outside the repository");
    res.setHeader("Content-Type", TYPES[extname(path)] ?? "application/octet-stream");
    res.end(readFileSync(path));
  } catch (e) {
    res.writeHead(404);
    res.end(String(e.message));
  }
});
await new Promise((ok) => server.listen(0, "127.0.0.1", ok));
const origin = `http://127.0.0.1:${server.address().port}`;

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM_PATH ?? "/usr/bin/chromium",
  headless: true,
  args: ["--no-sandbox"],
});
const runtime = `Chromium ${browser.version()}`;
const host = { os: process.platform, arch: process.arch, cpus: availableParallelism() };
const require = createRequire(import.meta.url);

async function inPage(fn, arg) {
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto(`${origin}/`);
  try {
    return await page.evaluate(fn, arg);
  } catch (e) {
    throw new Error(`${e.message}${errors.length ? `\npage errors: ${errors.join("; ")}` : ""}`);
  } finally {
    await page.close();
  }
}

function write(setup, report) {
  mkdirSync(outDir, { recursive: true });
  const file = join(outDir, `${setup}.json`);
  writeFileSync(file, JSON.stringify({ schema: 1, setup, host, settings, unixMs: Date.now(), ...report }, null, 2));
  console.error(`wrote ${file}`);
}

try {
  console.error(`cesiumjs-chromium (${runtime})`);
  const js = await inPage(
    async ({ settings, filter }) => (await import("/cesiumjs-bench.mjs")).run(settings, filter),
    { settings, filter },
  );
  js.logs.forEach((line) => console.error(`  ${line}`));
  write("cesiumjs-chromium", {
    implementation: "cesiumjs",
    cesium: require("@cesium/engine/package.json").version,
    runtime,
    crossOriginIsolated: true,
    results: js.results,
  });

  for (const [setup, profile] of [
    ["cesiumrs-wasm-shipped", "release profile (opt-level s, LTO)"],
    ["cesiumrs-wasm-fast", "bench profile (opt-level 3)"],
  ]) {
    const file = `target/bench-wasm/${setup}.wasm`;
    if (!existsSync(join(root, file))) {
      console.error(`${setup}: ${file} not found, skipped (scripts/bench.sh builds it)`);
      continue;
    }
    console.error(`${setup} (${runtime})`);
    const rs = await inPage(
      async ({ file, settings, filter }) => (await import("/wasm-host.mjs")).runWasm(`/${file}`, settings, filter),
      { file, settings, filter },
    );
    for (const r of rs.results) {
      console.error(`  ${r.id.padEnd(34)} ${r.medianMs === undefined ? r.status : `${r.medianMs.toFixed(4).padStart(12)} ms/pass  ${r.status}`}`);
    }
    write(setup, { implementation: "cesiumrs", runtime: `${runtime}, wasm32 ${profile}`, wasmBytes: rs.wasmBytes, results: rs.results });
  }
} finally {
  await browser.close();
  server.close();
}
