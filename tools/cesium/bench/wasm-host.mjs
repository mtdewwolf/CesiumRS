// Browser host for examples/parity_bench_wasm.rs (a raw C-ABI module, no
// wasm-bindgen). browser.mjs imports this page module and calls `runWasm`.

const decoder = new TextDecoder();
const encoder = new TextEncoder();

export async function runWasm(url, settings, filter = "") {
  const bytes = await (await fetch(url)).arrayBuffer();
  const { instance } = await WebAssembly.instantiate(bytes, { env: { bench_now: () => performance.now() } });
  const x = instance.exports;
  // Memory can grow during a call, so take a fresh view every time.
  // Pointers and lengths come back as signed i32; `>>> 0` reads them unsigned.
  const read = (ptr) => JSON.parse(decoder.decode(new Uint8Array(x.memory.buffer, ptr >>> 0, x.bench_output_len() >>> 0)));

  const paths = read(x.bench_inputs());
  const files = {};
  for (const path of paths) {
    const response = await fetch(`/${path}`);
    if (response.ok) files[path] = await response.text();
  }
  const request = encoder.encode(JSON.stringify({ settings, filter, files }));
  const ptr = x.bench_alloc(request.length) >>> 0;
  new Uint8Array(x.memory.buffer, ptr, request.length).set(request);
  const output = read(x.bench_run(ptr, request.length));
  if (output.error) throw new Error(output.error);
  return { results: output.results, wasmBytes: bytes.byteLength };
}
