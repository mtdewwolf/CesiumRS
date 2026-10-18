// Exercise the actual wasm artifact shipped by the viewer. Run after build-web.sh.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync(new URL("../web/client/generated/wasm.ts", import.meta.url), "utf8");
const base64 = source.match(/WASM_BASE64\s*=\s*"([^"]+)"/)?.[1];
assert.ok(base64, "generated module must contain the wasm binary");
const { instance: { exports: e } } = await WebAssembly.instantiate(Buffer.from(base64, "base64"), {});
const f64 = () => Array.from(new Float64Array(e.memory.buffer, e.f64_ptr(), e.f64_len()));

for (const [lon, lat, endLon, endLat] of [
  [0, 0, Math.PI, 0],
  [0.3, 0.7, 0.3 + Math.PI, -0.7],
  [0.3, 0.7, 0.3 + Math.PI - 0.001, -0.7],
  [0.3, 0.7, 0.3, 0.7],
]) {
  e.geodesic_info(lon, lat, endLon, endLat);
  const [distance] = f64();
  assert.ok(Number.isFinite(distance) && distance >= 0);
  e.build_geodesic_mesh(lon, lat, endLon, endLat, 32);
  const arc = Array.from(new Float32Array(e.memory.buffer, e.f32_ptr(), e.f32_len()));
  assert.equal(arc.length, 32 * 6);
  assert.ok(arc.every(Number.isFinite));
  e.llh_to_ecef(endLon, endLat, 0);
  const end = f64();
  for (let i = 0; i < 3; i++) assert.ok(Math.abs(arc[arc.length - 3 + i] - end[i]) < 0.5, "arc reaches endpoint");
  for (let i = 0; i < arc.length; i += 6) {
    const chord = Math.hypot(...arc.slice(i, i + 3).map((v, j) => v - arc[i + 3 + j]));
    assert.ok(chord < 1e6, "arc segments remain continuous");
    if (distance < 1) assert.equal(chord, 0, "repeated point stays in place");
  }
  if (distance > 1) {
    const middle = arc.slice(arc.length / 2, arc.length / 2 + 3);
    assert.ok(Math.hypot(...middle.map((v, i) => v - arc[i])) > 1e6, "antipodal route does not collapse");
  }
}
console.log("Viewer wasm: antipodal, near-antipodal and repeated-point geodesics pass");
