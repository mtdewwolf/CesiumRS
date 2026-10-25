// Tolerant comparison of encoded values: a port of `close` in
// tests/common/mod.rs, so both sides check outputs the same way.

export const EXACT = { rel: 0, abs: 0, vector: false };
export const TIGHT = { rel: 1e-12, abs: 1e-12, vector: false };
export const GEOMETRY = { rel: 1e-7, abs: 1e-9, vector: false };

const SPECIAL = new Set(["NaN", "Infinity", "-Infinity", "-0"]);

function num(v) {
  if (typeof v === "number") return v;
  if (v === null || v === "NaN") return NaN;
  if (v === "Infinity") return Infinity;
  if (v === "-Infinity") return -Infinity;
  if (v === "-0") return -0;
  throw new Error(`not a number: ${v}`);
}

function flatScale(x, y) {
  let scale = 0;
  for (const v of [...x, ...y]) {
    if (typeof v === "number") scale = Math.max(scale, Math.abs(v));
    else if (typeof v !== "string" && v !== null) return null;
  }
  return scale;
}

export function close(a, b, tol) {
  if (Array.isArray(a) && Array.isArray(b)) {
    const scale = flatScale(a, b);
    const t = scale !== null && tol.vector ? { ...tol, abs: Math.max(tol.abs, tol.rel * scale) } : tol;
    return a.length === b.length && a.every((p, i) => close(p, b[i], t));
  }
  const isNumLike = (v) => typeof v === "number" || typeof v === "string";
  if (isNumLike(a) && isNumLike(b)) {
    if (typeof a === "string" && typeof b === "string" && !(SPECIAL.has(a) && SPECIAL.has(b))) return a === b;
    if ((typeof a === "string" && !SPECIAL.has(a)) || (typeof b === "string" && !SPECIAL.has(b))) return false;
    const p = num(a);
    const q = num(b);
    if (Number.isNaN(p) || Number.isNaN(q)) return Number.isNaN(p) && Number.isNaN(q);
    if (!Number.isFinite(p) || !Number.isFinite(q)) return p === q;
    const diff = Math.abs(p - q);
    return diff <= tol.abs || diff <= tol.rel * Math.max(Math.abs(p), Math.abs(q));
  }
  if (a && b && typeof a === "object" && typeof b === "object" && !Array.isArray(a) && !Array.isArray(b)) {
    const ka = Object.keys(a);
    return ka.length === Object.keys(b).length && ka.every((k) => k in b && close(a[k], b[k], tol));
  }
  if (a === null && b === "NaN") return true;
  if (a === "NaN" && b === null) return true;
  return a === b;
}

/** Fixture arguments back to numbers: the encoder writes non-finite numbers and -0 as strings. */
export function dec(v) {
  if (typeof v === "string" && SPECIAL.has(v)) return num(v);
  if (Array.isArray(v)) return v.map(dec);
  if (v && typeof v === "object") return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, dec(x)]));
  return v;
}
