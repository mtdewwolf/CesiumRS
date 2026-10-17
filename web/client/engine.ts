import { WASM_BASE64 } from "./generated/wasm";

export function decodeBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

type Exports = {
  memory: WebAssembly.Memory;
  f64_ptr: () => number;
  f64_len: () => number;
  f32_ptr: () => number;
  f32_len: () => number;
  u32_ptr: () => number;
  u32_len: () => number;
  camera_look_at: (
    lon: number,
    lat: number,
    height: number,
    heading: number,
    pitch: number,
    range: number,
    fovY: number,
    aspect: number,
    near: number,
    far: number,
  ) => void;
  pick_ndc: (x: number, y: number) => void;
  llh_to_ecef: (lon: number, lat: number, height: number) => void;
  ecef_to_llh: (x: number, y: number, z: number) => void;
  geodesic_info: (lon1: number, lat1: number, lon2: number, lat2: number) => void;
  sun_direction: (jd: number) => void;
  suggested_level: (range: number) => number;
  build_globe_mesh: (latSegments: number, lonSegments: number) => void;
  build_graticule_mesh: (stepDeg: number) => void;
  build_geodesic_mesh: (
    lon1: number,
    lat1: number,
    lon2: number,
    lat2: number,
    segments: number,
  ) => void;
  build_tile_mesh: (
    level: number,
    scheme: number,
    focusLon: number,
    focusLat: number,
    window: number,
  ) => void;
  build_star_mesh: (count: number, seed: number) => void;
  radians: (degrees: number) => number;
};

export type CameraFrame = {
  view: Float32Array;
  projection: Float32Array;
  viewProjection: Float32Array;
  eye: [number, number, number];
  direction: [number, number, number];
  up: [number, number, number];
  right: [number, number, number];
  focus: [number, number, number];
};

export type PickHit = {
  t: number;
  longitude: number;
  latitude: number;
  height: number;
};

export type GeodesicInfo = {
  distance: number;
  startHeading: number;
  endHeading: number;
};

export type CesiumEngine = {
  lookAt: (args: {
    lon: number;
    lat: number;
    height: number;
    heading: number;
    pitch: number;
    range: number;
    fovY: number;
    aspect: number;
    near: number;
    far: number;
  }) => CameraFrame;
  pick: (ndcX: number, ndcY: number) => PickHit | null;
  toCartesian: (lon: number, lat: number, height: number) => [number, number, number];
  toCartographic: (x: number, y: number, z: number) => { longitude: number; latitude: number } | null;
  geodesic: (lon1: number, lat1: number, lon2: number, lat2: number) => GeodesicInfo;
  sun: (jd: number) => { direction: [number, number, number]; longitude: number; latitude: number };
  level: (range: number) => number;
  globe: (latSegments: number, lonSegments: number) => { vertices: Float32Array; indices: Uint32Array };
  graticule: (stepDeg: number) => Float32Array;
  arc: (lon1: number, lat1: number, lon2: number, lat2: number, segments: number) => Float32Array;
  tiles: (level: number, scheme: number, lon: number, lat: number, window: number) => Float32Array;
  stars: (count: number, seed: number) => Float32Array;
};

function takeF64(exp: Exports): Float64Array {
  const len = exp.f64_len();
  if (len <= 0) return new Float64Array();
  return new Float64Array(exp.memory.buffer, exp.f64_ptr(), len).slice();
}

function takeF32(exp: Exports): Float32Array {
  const len = exp.f32_len();
  if (len <= 0) return new Float32Array();
  return new Float32Array(exp.memory.buffer, exp.f32_ptr(), len).slice();
}

function takeU32(exp: Exports): Uint32Array {
  const len = exp.u32_len();
  if (len <= 0) return new Uint32Array();
  return new Uint32Array(exp.memory.buffer, exp.u32_ptr(), len).slice();
}

function vec3(src: Float64Array, offset: number): [number, number, number] {
  return [src[offset], src[offset + 1], src[offset + 2]];
}

function mat16(src: Float64Array, offset: number): Float32Array {
  const out = new Float32Array(16);
  for (let i = 0; i < 16; i++) out[i] = src[offset + i];
  return out;
}

export async function loadEngine(): Promise<CesiumEngine> {
  const { instance } = await WebAssembly.instantiate(decodeBase64(WASM_BASE64), {});
  const exp = instance.exports as unknown as Exports;
  if (typeof exp.camera_look_at !== "function" || !exp.memory) {
    throw new Error("cesium-rs wasm is missing the camera export");
  }

  return {
    lookAt(args) {
      exp.camera_look_at(
        args.lon,
        args.lat,
        args.height,
        args.heading,
        args.pitch,
        args.range,
        args.fovY,
        args.aspect,
        args.near,
        args.far,
      );
      const raw = takeF64(exp);
      if (raw.length < 63) throw new Error(`camera frame is short (${raw.length})`);
      return {
        view: mat16(raw, 0),
        projection: mat16(raw, 16),
        viewProjection: mat16(raw, 32),
        eye: vec3(raw, 48),
        direction: vec3(raw, 51),
        up: vec3(raw, 54),
        right: vec3(raw, 57),
        focus: vec3(raw, 60),
      };
    },
    pick(ndcX, ndcY) {
      exp.pick_ndc(ndcX, ndcY);
      const raw = takeF64(exp);
      if (raw.length < 8 || raw[0] === 0) return null;
      return { t: raw[1], longitude: raw[2], latitude: raw[3], height: raw[4] };
    },
    toCartesian(lon, lat, height) {
      exp.llh_to_ecef(lon, lat, height);
      return vec3(takeF64(exp), 0);
    },
    toCartographic(x, y, z) {
      exp.ecef_to_llh(x, y, z);
      const raw = takeF64(exp);
      if (raw.length < 3 || raw[0] === 0) return null;
      return { longitude: raw[1], latitude: raw[2] };
    },
    geodesic(lon1, lat1, lon2, lat2) {
      exp.geodesic_info(lon1, lat1, lon2, lat2);
      const raw = takeF64(exp);
      return { distance: raw[0] ?? 0, startHeading: raw[1] ?? 0, endHeading: raw[2] ?? 0 };
    },
    sun(jd) {
      exp.sun_direction(jd);
      const raw = takeF64(exp);
      return {
        direction: [raw[0] ?? 1, raw[1] ?? 0, raw[2] ?? 0],
        longitude: raw[3] ?? 0,
        latitude: raw[4] ?? 0,
      };
    },
    level(range) {
      return exp.suggested_level(range);
    },
    globe(latSegments, lonSegments) {
      exp.build_globe_mesh(latSegments, lonSegments);
      return { vertices: takeF32(exp), indices: takeU32(exp) };
    },
    graticule(stepDeg) {
      exp.build_graticule_mesh(stepDeg);
      return takeF32(exp);
    },
    arc(lon1, lat1, lon2, lat2, segments) {
      exp.build_geodesic_mesh(lon1, lat1, lon2, lat2, segments);
      return takeF32(exp);
    },
    tiles(level, scheme, lon, lat, window) {
      exp.build_tile_mesh(level, scheme, lon, lat, window);
      return takeF32(exp);
    },
    stars(count, seed) {
      exp.build_star_mesh(count, seed);
      return takeF32(exp);
    },
  };
}
