export type Place = { name: string; lat: number; lon: number };

/** Degrees, WGS84. */
export const PLACES: Place[] = [
  { name: "Fort Collins", lat: 40.5853, lon: -105.0844 },
  { name: "New York", lat: 40.7128, lon: -74.006 },
  { name: "London", lat: 51.5074, lon: -0.1278 },
  { name: "Cairo", lat: 30.0444, lon: 31.2357 },
  { name: "Tokyo", lat: 35.6762, lon: 139.6503 },
  { name: "Singapore", lat: 1.3521, lon: 103.8198 },
  { name: "Sydney", lat: -33.8688, lon: 151.2093 },
  { name: "Quito", lat: -0.1807, lon: -78.4678 },
  { name: "Cape Town", lat: -33.9249, lon: 18.4241 },
  { name: "Reykjavík", lat: 64.1466, lon: -21.9426 },
];

export const GEODESICS: [string, string][] = [
  ["New York", "London"],
  ["Tokyo", "Sydney"],
  ["Fort Collins", "Tokyo"],
  ["Cape Town", "Reykjavík"],
];

export function placeByName(name: string): Place {
  const place = PLACES.find((p) => p.name === name);
  if (!place) throw new Error(`unknown place ${name}`);
  return place;
}

export function formatLat(deg: number): string {
  return `${Math.abs(deg).toFixed(2)}° ${deg >= 0 ? "N" : "S"}`;
}

export function formatLon(deg: number): string {
  return `${Math.abs(deg).toFixed(2)}° ${deg >= 0 ? "E" : "W"}`;
}

export function formatKm(meters: number): string {
  return `${Math.round(meters / 1000).toLocaleString("en-US")} km`;
}

/** Compass heading in degrees, 0 = north, clockwise. */
export function formatHeading(rad: number): string {
  const deg = ((rad * 180) / Math.PI + 360) % 360;
  const names = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
  return `${deg.toFixed(1)}° ${names[Math.round(deg / 45) % 8]}`;
}
