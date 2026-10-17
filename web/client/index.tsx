import type { ComponentChildren } from "preact";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import {
  formatHeading,
  formatKm,
  formatLat,
  formatLon,
  GEODESICS,
  placeByName,
  PLACES,
  type Place,
} from "../shared/places";
import { loadEngine, type CesiumEngine, type GeodesicInfo } from "./engine";
import { DEFAULT_CAMERA, GlobeView, RAD, type CameraState, type Layers, type Marker } from "./globe";

type Status = { kind: "loading" } | { kind: "online" } | { kind: "error"; message: string };
type LonLat = { lon: number; lat: number };
type Route = { a: LonLat; b: LonLat; label: string; info: GeodesicInfo };

const AMBER: [number, number, number] = [0.894, 0.639, 0.133];
const WHITE: [number, number, number] = [0.92, 0.95, 0.98];
const CYAN: [number, number, number] = [0.38, 0.85, 0.95];

const card = "rounded-lg border border-[#243041] bg-[#07090c]/70";
const chip =
  "rounded-lg border border-[#243041] bg-[#07090c]/70 px-3 py-2.5 text-left text-sm text-[#e7edf4] transition-colors hover:border-[#e4a322]/70 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#e4a322]";
const chipActive = "border-[#e4a322] bg-[#e4a322]/10";

function Section({ title, children }: { title: string; children: ComponentChildren }) {
  return (
    <section className="mt-5">
      <h2 className="mb-2 font-mono text-[11px] uppercase tracking-[0.18em] text-[#8b97a8]">{title}</h2>
      {children}
    </section>
  );
}

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex cursor-pointer items-center justify-between gap-3 py-1.5 text-sm">
      <span>{label}</span>
      <input
        type="checkbox"
        className="h-4 w-4 accent-[#e4a322]"
        checked={checked}
        onChange={(e) => onChange((e.currentTarget as HTMLInputElement).checked)}
      />
    </label>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-baseline justify-between gap-3 py-0.5">
      <span className="text-xs text-[#8b97a8]">{label}</span>
      <span className="font-mono text-sm tabular-nums">{value}</span>
    </div>
  );
}

function formatPoint(p: LonLat): string {
  return `${formatLat(p.lat / RAD)}  ${formatLon(p.lon / RAD)}`;
}

function arcMidpoint(engine: CesiumEngine, arc: Float32Array): LonLat | null {
  const segments = arc.length / 6;
  const i = Math.floor(segments / 2) * 6;
  const hit = engine.toCartographic(arc[i], arc[i + 1], arc[i + 2]);
  return hit ? { lon: hit.longitude, lat: hit.latitude } : null;
}

export function App() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const viewRef = useRef<GlobeView | null>(null);
  const engineRef = useRef<CesiumEngine | null>(null);
  const [status, setStatus] = useState<Status>({ kind: "loading" });
  const [camera, setCamera] = useState<CameraState>(DEFAULT_CAMERA);
  const [layers, setLayers] = useState<Layers>({ graticule: true, tiles: false, tileScheme: 0, sunLighting: false });
  const [tileLevel, setTileLevel] = useState(0);
  const [place, setPlace] = useState<string | null>(null);
  const [route, setRoute] = useState<Route | null>(null);
  const [picked, setPicked] = useState<LonLat | null>(null);
  const [measuring, setMeasuring] = useState(false);
  const [measureStart, setMeasureStart] = useState<LonLat | null>(null);
  const [panelOpen, setPanelOpen] = useState(() => typeof window === "undefined" || window.innerWidth >= 640);
  const coarse = useMemo(() => typeof window !== "undefined" && window.matchMedia("(pointer: coarse)").matches, []);

  const measuringRef = useRef(measuring);
  measuringRef.current = measuring;
  const measureStartRef = useRef(measureStart);
  measureStartRef.current = measureStart;

  useEffect(() => {
    let disposed = false;
    loadEngine()
      .then((engine) => {
        if (disposed || !canvasRef.current) return;
        engineRef.current = engine;
        const view = new GlobeView(canvasRef.current, engine, DEFAULT_CAMERA);
        view.onChange = setCamera;
        view.onTileLevel = setTileLevel;
        view.onTap = (hit) => handleTap(hit ? { lon: hit.lon, lat: hit.lat } : null);
        viewRef.current = view;
        setStatus({ kind: "online" });
      })
      .catch((err: unknown) => {
        setStatus({ kind: "error", message: err instanceof Error ? err.message : String(err) });
      });
    return () => {
      disposed = true;
      viewRef.current?.dispose();
      viewRef.current = null;
    };
  }, []);

  function showRoute(a: LonLat, b: LonLat, label: string, fly: boolean) {
    const engine = engineRef.current;
    const view = viewRef.current;
    if (!engine || !view) return;
    const info = engine.geodesic(a.lon, a.lat, b.lon, b.lat);
    const arc = engine.arc(a.lon, a.lat, b.lon, b.lat, 192);
    view.setArc(arc);
    setRoute({ a, b, label, info });
    if (fly) {
      const mid = arcMidpoint(engine, arc);
      if (mid) view.flyTo({ ...mid, heading: 0, pitch: -90 * RAD, range: Math.max(info.distance * 1.7, 3000e3) });
    }
  }

  function handleTap(hit: LonLat | null) {
    if (!hit) return;
    if (measuringRef.current) {
      const start = measureStartRef.current;
      if (!start) {
        setMeasureStart(hit);
        setRoute(null);
        viewRef.current?.setArc(null);
      } else {
        showRoute(start, hit, "Measured", false);
        setMeasureStart(null);
      }
      setPicked(null);
      return;
    }
    setPicked(hit);
  }

  function collapseOnPhone() {
    if (window.innerWidth < 640) setPanelOpen(false);
  }

  function goToPlace(p: Place) {
    collapseOnPhone();
    setPlace(p.name);
    viewRef.current?.flyTo({ lon: p.lon * RAD, lat: p.lat * RAD, heading: 0, pitch: -90 * RAD, range: 2500e3 });
  }

  function goToGeodesic(from: string, to: string) {
    const a = placeByName(from);
    const b = placeByName(to);
    collapseOnPhone();
    setPlace(null);
    setMeasuring(false);
    setMeasureStart(null);
    showRoute({ lon: a.lon * RAD, lat: a.lat * RAD }, { lon: b.lon * RAD, lat: b.lat * RAD }, `${from} → ${to}`, true);
  }

  function updateLayers(patch: Partial<Layers>) {
    const next = { ...layers, ...patch };
    setLayers(next);
    viewRef.current?.setLayers(next);
  }

  // Markers: every place, then the active route, measurement, and tap.
  useEffect(() => {
    const markers: Marker[] = PLACES.map((p) => ({
      lon: p.lon * RAD,
      lat: p.lat * RAD,
      color: p.name === place ? AMBER : WHITE,
      size: p.name === place ? 11 : 6,
    }));
    if (route) {
      markers.push({ ...route.a, color: AMBER, size: 10 }, { ...route.b, color: AMBER, size: 10 });
    }
    if (measureStart) markers.push({ ...measureStart, color: AMBER, size: 11 });
    if (picked) markers.push({ ...picked, color: CYAN, size: 11 });
    viewRef.current?.setMarkers(markers);
  }, [place, route, measureStart, picked, status]);

  const hint = coarse
    ? "Drag to orbit. Pinch to zoom. Two fingers twist and tilt. Tap to pick."
    : "Drag orbits. Right-drag or Shift-drag tilts and turns. Scroll zooms. Click picks.";

  const pill =
    status.kind === "online"
      ? { text: "Rust core online", dot: "bg-[#4ade80]" }
      : status.kind === "loading"
        ? { text: "Loading Rust core…", dot: "bg-[#e4a322]" }
        : { text: "Core failed", dot: "bg-[#f87171]" };

  return (
    <main className="fixed inset-0 overflow-hidden bg-[#07090c] font-sans text-[#e7edf4]">
      <canvas ref={canvasRef} className="absolute inset-0 h-full w-full touch-none select-none" aria-label="Interactive WGS84 globe" />

      <header className="pointer-events-none absolute left-4 top-4 [text-shadow:0_1px_4px_rgba(0,0,0,0.9)] sm:left-6 sm:top-5">
        <p className="font-mono text-[11px] uppercase tracking-[0.32em] text-[#e4a322]">Cesium-RS</p>
        <h1 className="mt-1 text-xl font-semibold tracking-tight sm:text-2xl">WGS84 globe</h1>
      </header>

      <div className="absolute right-4 top-4 flex items-center gap-2 sm:right-6 sm:top-5">
        <span className="flex items-center gap-2 rounded-full border border-[#243041] bg-[#10141a]/80 px-3 py-1.5 font-mono text-xs text-[#8b97a8] backdrop-blur">
          <span className={`h-1.5 w-1.5 rounded-full ${pill.dot}`} />
          {pill.text}
        </span>
      </div>

      {status.kind === "error" ? (
        <div role="alert" className="absolute inset-x-4 top-24 mx-auto max-w-md rounded-lg border border-[#f87171]/50 bg-[#10141a] p-4 text-sm">
          <p className="font-medium">The Rust core could not start.</p>
          <p className="mt-1 font-mono text-xs text-[#8b97a8]">{status.message}</p>
        </div>
      ) : null}

      <button
        type="button"
        className="absolute bottom-4 left-4 rounded-full border border-[#243041] bg-[#10141a]/90 px-4 py-2 text-sm backdrop-blur sm:hidden"
        style={{ display: panelOpen ? "none" : undefined }}
        onClick={() => setPanelOpen(true)}
      >
        Controls
      </button>

      <aside
        className="absolute inset-x-3 bottom-3 max-h-[58vh] overflow-y-auto rounded-xl border border-[#243041] bg-[#10141a]/90 p-4 shadow-2xl backdrop-blur sm:inset-x-auto sm:bottom-6 sm:left-6 sm:top-24 sm:max-h-none sm:w-80"
        style={{ display: panelOpen ? undefined : "none" }}
      >
        <div className="flex items-start justify-between gap-3">
          <p className="font-mono text-sm tabular-nums text-[#c9d3df]">
            {formatLat(camera.lat / RAD)} {formatLon(camera.lon / RAD)}
            <span className="whitespace-nowrap text-[#8b97a8]"> · {formatKm(camera.range)} range</span>
          </p>
          <button
            type="button"
            className="-mr-1 -mt-1 shrink-0 rounded px-2 py-1 text-xs text-[#8b97a8] hover:text-white sm:hidden"
            onClick={() => setPanelOpen(false)}
          >
            Hide
          </button>
        </div>
        <div className="mt-1 flex items-center justify-between gap-3 font-mono text-xs text-[#8b97a8]">
          <span>
            heading {formatHeading(camera.heading)} · pitch {(camera.pitch / RAD).toFixed(0)}°
          </span>
          <button
            type="button"
            className="shrink-0 whitespace-nowrap rounded border border-[#243041] px-2 py-0.5 text-[11px] text-[#c9d3df] hover:border-[#e4a322]/70"
            onClick={() => viewRef.current?.resetNorth()}
          >
            North up
          </button>
        </div>

        <p className="mt-3 text-sm leading-snug text-[#8b97a8]">{hint}</p>

        {picked ? (
          <div className={`${card} mt-3 flex items-center justify-between gap-2 px-3 py-2`}>
            <span className="font-mono text-xs">
              <span className="text-[#5fd8f2]">●</span> {formatPoint(picked)}
            </span>
            <button type="button" className="text-xs text-[#8b97a8] hover:text-white" onClick={() => setPicked(null)}>
              Clear
            </button>
          </div>
        ) : null}

        <Section title="Places">
          <div className="grid grid-cols-2 gap-2">
            {PLACES.map((p) => (
              <button key={p.name} type="button" className={`${chip} ${place === p.name ? chipActive : ""}`} onClick={() => goToPlace(p)}>
                {p.name}
              </button>
            ))}
          </div>
        </Section>

        <Section title="Geodesics (Vincenty)">
          <div className="grid gap-2">
            {GEODESICS.map(([a, b]) => {
              const label = `${a} → ${b}`;
              return (
                <button key={label} type="button" className={`${chip} ${route?.label === label ? chipActive : ""}`} onClick={() => goToGeodesic(a, b)}>
                  {label}
                </button>
              );
            })}
          </div>
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              className={`${chip} flex-1 ${measuring ? chipActive : ""}`}
              onClick={() => {
                setMeasuring(!measuring);
                setMeasureStart(null);
                setPicked(null);
              }}
            >
              {measuring ? (measureStart ? "Tap the end point" : "Tap the start point") : "Measure on the globe"}
            </button>
            {route ? (
              <button
                type="button"
                className={chip}
                onClick={() => {
                  setRoute(null);
                  viewRef.current?.setArc(null);
                }}
              >
                Clear
              </button>
            ) : null}
          </div>
          {route ? (
            <div className={`${card} mt-2 px-3 py-2`}>
              <p className="mb-1 text-sm font-medium">{route.label}</p>
              <Stat label="Distance" value={formatKm(route.info.distance)} />
              <Stat label="Initial heading" value={formatHeading(route.info.startHeading)} />
              <Stat label="Final heading" value={formatHeading(route.info.endHeading)} />
              {route.label === "Measured" ? (
                <p className="mt-1 font-mono text-[11px] text-[#8b97a8]">
                  {formatPoint(route.a)} → {formatPoint(route.b)}
                </p>
              ) : null}
            </div>
          ) : null}
        </Section>

        <Section title="Layers">
          <div className={`${card} px-3 py-1`}>
            <Toggle label="Graticule (15°)" checked={layers.graticule} onChange={(v) => updateLayers({ graticule: v })} />
            <Toggle label="Sun lighting (now)" checked={layers.sunLighting} onChange={(v) => updateLayers({ sunLighting: v })} />
            <Toggle label={`Tile grid · level ${tileLevel}`} checked={layers.tiles} onChange={(v) => updateLayers({ tiles: v })} />
            {layers.tiles ? (
              <div className="flex gap-2 pb-2 pt-1">
                {(["Geographic", "Web Mercator"] as const).map((name, i) => (
                  <button
                    key={name}
                    type="button"
                    className={`${chip} flex-1 py-1.5 text-center text-xs ${layers.tileScheme === i ? chipActive : ""}`}
                    onClick={() => updateLayers({ tileScheme: i as 0 | 1 })}
                  >
                    {name}
                  </button>
                ))}
              </div>
            ) : null}
          </div>
        </Section>

        <p className="mt-5 text-[11px] leading-relaxed text-[#8b97a8]">
          Camera, picking, geodesics, tiling and meshes run in Rust (wasm), ported from CesiumJS 1.139 (Apache-2.0). Imagery:
          Natural Earth II.
        </p>
      </aside>
    </main>
  );
}
