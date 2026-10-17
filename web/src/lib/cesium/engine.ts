import { previewPathPrefix } from "@/lib/preview-path-prefix";

export function assetUrl(path: string): string {
  const normalized = path.startsWith("/") ? path : `/${path}`;
  if (typeof window === "undefined") return normalized;
  const prefix = previewPathPrefix(window.location.hostname, window.location.pathname) ?? "";
  return `${prefix}${normalized}`;
}

export async function loadEngine() {
  const response = await fetch(assetUrl("/cesium_rs.wasm"));
  if (!response.ok) throw new Error(`cesium-rs failed to load (${response.status})`);
  const { instance } = await WebAssembly.instantiate(await response.arrayBuffer(), {});
  return instance.exports;
}
