#!/usr/bin/env node
// Zero-dependency local HTTP server for test-site/ and repo assets/.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, "..");
const siteDir = path.join(rootDir, "test-site");
const assetsDir = path.join(rootDir, "assets");
const testDataDir = path.join(rootDir, "tests", "data");
const port = Number(process.env.PORT || 4173);
const host = process.env.HOST || "127.0.0.1";

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".wasm": "application/wasm",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
  ".txt": "text/plain; charset=utf-8",
};

function safeResolve(base, rel) {
  const cleaned = decodeURIComponent(rel.split("?")[0]).replace(/^\/+/, "");
  const full = path.resolve(base, cleaned);
  if (!full.startsWith(base)) return null;
  return full;
}

const server = http.createServer((req, res) => {
  const urlPath = (req.url || "/").split("?")[0];
  let filePath = null;

  if (urlPath.startsWith("/assets/")) {
    filePath = safeResolve(assetsDir, urlPath.slice("/assets/".length));
  } else if (urlPath.startsWith("/testdata/")) {
    filePath = safeResolve(testDataDir, urlPath.slice("/testdata/".length));
  } else {
    const rel = urlPath === "/" ? "index.html" : urlPath;
    filePath = safeResolve(siteDir, rel);
  }

  if (!filePath) {
    res.writeHead(403, { "Content-Type": "text/plain" });
    res.end("Forbidden");
    return;
  }

  fs.stat(filePath, (err, stat) => {
    if (err || !stat.isFile()) {
      res.writeHead(404, { "Content-Type": "text/plain" });
      res.end(`Not found: ${urlPath}`);
      return;
    }
    const ext = path.extname(filePath).toLowerCase();
    res.writeHead(200, {
      "Content-Type": MIME[ext] || "application/octet-stream",
      "Cache-Control": "no-cache",
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    });
    fs.createReadStream(filePath).pipe(res);
  });
});

server.listen(port, host, () => {
  console.log(`CesiumRS Live Testbed running at http://${host}:${port}/`);
});
