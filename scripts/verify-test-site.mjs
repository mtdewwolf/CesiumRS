#!/usr/bin/env node
// Headless Chromium end-to-end verification for test-site/.
import { createRequire } from "node:module";
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

function findChromium() {
  if (process.env.CHROMIUM_PATH && fs.existsSync(process.env.CHROMIUM_PATH)) return process.env.CHROMIUM_PATH;
  for (const p of ["/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/google-chrome"]) {
    if (fs.existsSync(p)) return p;
  }
  const cacheDir = path.join(os.homedir(), ".cache", "ms-playwright");
  if (fs.existsSync(cacheDir)) {
    const dirs = fs.readdirSync(cacheDir).filter((d) => d.startsWith("chromium")).sort().reverse();
    for (const d of dirs) {
      for (const rel of ["chrome-headless-shell-linux64/chrome-headless-shell", "chrome-linux/chrome", "chrome-linux64/chrome"]) {
        const cand = path.join(cacheDir, d, rel);
        if (fs.existsSync(cand)) return cand;
      }
    }
  }
  return undefined;
}

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
const require = createRequire(path.join(root, "tools/cesium/package.json"));
const { chromium } = require("playwright-core");

const PORT = 4179;
const server = spawn(process.execPath, [path.join(root, "scripts/serve-test-site.mjs")], {
  env: { ...process.env, PORT: String(PORT), HOST: "127.0.0.1" },
  stdio: ["ignore", "pipe", "pipe"],
});

await new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error("Server startup timeout")), 5000);
  server.stdout.on("data", (d) => {
    if (String(d).includes("running at")) {
      clearTimeout(timer);
      resolve();
    }
  });
  server.stderr.on("data", (d) => process.stderr.write(d));
  server.on("exit", (c) => reject(new Error(`Server exited early: ${c}`)));
});

let browser;
try {
  const launchOpts = {
    headless: true,
    args: ["--no-sandbox", "--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"],
  };
  const exe = findChromium();
  if (exe) launchOpts.executablePath = exe;
  browser = await chromium.launch(launchOpts);
  const page = await browser.newPage({ viewport: { width: 1280, height: 820 } });
  const errors = [];
  page.on("pageerror", (e) => errors.push("PAGEERROR: " + e.message));
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push("CONSOLE_ERR: " + msg.text());
  });

  await page.goto(`http://127.0.0.1:${PORT}/`, { waitUntil: "networkidle" });
  await page.waitForFunction(() => document.getElementById("wasm-label")?.textContent?.includes("Online"), { timeout: 25000 });
  console.log("WASM status:", await page.textContent("#wasm-label"));

  // Exercise Globe controls (body switch, geometry, post-process, scene mode)
  await page.click('#body-buttons button[data-body="mars"]');
  await page.selectOption("#geom-select", "polygon");
  await page.selectOption("#pp-effect", "night_vision");
  await page.waitForTimeout(200);
  await page.selectOption("#pp-effect", "none");
  await page.click('#body-buttons button[data-body="wgs84"]');
  await page.waitForTimeout(150);

  // Visit every tab
  for (const tab of ["model", "datasources", "workers", "renderer", "browserhost", "shaders", "selftest"]) {
    await page.click(`#main-tabs button[data-tab="${tab}"]`);
    await page.waitForTimeout(300);
  }

  // Run full self-test suite
  await page.click("#btn-run-all-tests");
  await page.waitForFunction(
    () => {
      const t = document.getElementById("selftest-summary")?.textContent || "";
      return t.includes("PASSED");
    },
    { timeout: 30000 },
  );

  const summary = await page.textContent("#selftest-summary");
  const rows = await page.$$eval("#selftest-list .test-item", (els) =>
    els.map((el) => ({
      name: el.querySelector("strong")?.textContent,
      detail: el.querySelector(".meta")?.textContent,
      badge: el.querySelector(".badge")?.textContent,
    })),
  );
  console.log("\n=== Live Browser Self-Test Summary:", summary, "===");
  for (const r of rows) {
    console.log(`  [${r.badge}] ${r.name} — ${r.detail}`);
  }
  if (errors.length) {
    console.error("\nBrowser errors detected:\n" + errors.join("\n"));
    process.exitCode = 1;
  }
  const failed = rows.filter((r) => !r.badge?.startsWith("PASS"));
  if (failed.length) {
    console.error("\nFailed test cases:", failed);
    process.exitCode = 1;
  }
} finally {
  await browser?.close();
  server.kill();
}
