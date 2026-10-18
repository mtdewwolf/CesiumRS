// Fixtures for the time-dependent Transforms frames, EarthOrientationParameters,
// Iau2006XysData, Iau2000Orientation, and IauOrientationAxes.
//
// The XYS chunks are loaded from assets/IAU2006_XYS (Cesium's own files) the
// way Iau2006XysData's downloader would store them. The EOP table is
// synthetic, with the 2015 and 2017 leap seconds inside it.

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
  EarthOrientationParameters,
  Iau2000Orientation,
  Iau2006XysData,
  IauOrientationAxes,
  JulianDate,
  TimeStandard,
  Transforms,
} from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const N = 40;

const J = ([d, s]) => new JulianDate(d, s, TimeStandard.TAI);

export function loadXys() {
  const xys = new Iau2006XysData();
  for (let chunk = 0; chunk < 28; chunk++) {
    const file = join(here, `../../../assets/IAU2006_XYS/IAU2006_XYS_${chunk}.json`);
    const samples = JSON.parse(readFileSync(file, "utf8")).samples;
    const start = chunk * 1000 * 3;
    for (let i = 0; i < samples.length; i++) xys._samples[start + i] = samples[i];
  }
  return xys;
}

/** A daily table from MJD 57000 to 57900 with smooth, made-up values. */
export function eopData() {
  const columnNames = [
    "dateIso8601",
    "modifiedJulianDateUtc",
    "xPoleWanderRadians",
    "yPoleWanderRadians",
    "ut1MinusUtcSeconds",
    "lengthOfDayCorrectionSeconds",
    "xCelestialPoleOffsetRadians",
    "yCelestialPoleOffsetRadians",
    "taiMinusUtcSeconds",
  ];
  const samples = [];
  for (let mjd = 57000; mjd <= 57900; mjd++) {
    const k = mjd - 57000;
    const tai = mjd < 57204 ? 35 : mjd < 57754 ? 36 : 37;
    // UT1 − UTC drifts down and jumps up by one second at each leap second.
    const ut1 = -0.2 - 0.0016 * k + (tai - 35);
    const iso = new Date(Date.UTC(1858, 10, 17) + mjd * 86400000).toISOString().slice(0, 10);
    samples.push(
      iso,
      mjd,
      7.5e-7 * Math.sin(k / 60),
      1.6e-6 + 5e-7 * Math.cos(k / 45),
      ut1,
      0.001 * Math.sin(k / 30),
      1e-9 * Math.cos(k / 20),
      -2e-9 * Math.sin(k / 25),
      tai,
    );
  }
  return { columnNames, samples };
}

function transformsSuite(xys) {
  const g = generators(makeRng(500));
  const s = suite("TransformsTime");
  const anyDate = () => [Math.floor(g.uniform(2415021, 2488070)), g.uniform(0, 86400)];
  // Inside the XYS data (1975 to 2050), away from its ends.
  const xysDate = () => [Math.floor(g.uniform(2442500, 2469700)), g.uniform(0, 86400)];
  const leapDate = () => {
    const l = g.pick(JulianDate.leapSeconds).julianDate;
    const d = JulianDate.addSeconds(l, g.pick([-1, -0.5, 0, 0.5, 1, 2]), new JulianDate());
    return [d.dayNumber, d.secondsOfDay];
  };
  const date = () => (g.rng() < 0.2 ? leapDate() : anyDate());

  s.repeat("computeTemeToPseudoFixedMatrix", N * 2, () => [date()], (d) => Transforms.computeTemeToPseudoFixedMatrix(J(d)));
  s.repeat("computeMoonFixedToIcrfMatrix", N, () => [date()], (d) => Transforms.computeMoonFixedToIcrfMatrix(J(d)));
  s.repeat("computeIcrfToMoonFixedMatrix", N, () => [date()], (d) => Transforms.computeIcrfToMoonFixedMatrix(J(d)));

  Transforms.iau2006XysData = xys;
  Transforms.earthOrientationParameters = EarthOrientationParameters.NONE;
  s.repeat("computeFixedToIcrfMatrix", N * 2, () => [g.rng() < 0.15 ? leapDate() : xysDate()], (d) => Transforms.computeFixedToIcrfMatrix(J(d)));
  s.repeat("computeIcrfToFixedMatrix", N, () => [xysDate()], (d) => Transforms.computeIcrfToFixedMatrix(J(d)));
  // Before the XYS epoch there is no data: undefined, and the central-body
  // matrix falls back to TEME.
  s.repeat("computeIcrfToFixedMatrix", 5, () => [[Math.floor(g.uniform(2420000, 2442390)), g.uniform(0, 86400)]], (d) => Transforms.computeIcrfToFixedMatrix(J(d)));
  s.repeat("computeIcrfToCentralBodyFixedMatrix", N, () => [g.rng() < 0.3 ? [Math.floor(g.uniform(2420000, 2442390)), 100] : xysDate()], (d) =>
    Transforms.computeIcrfToCentralBodyFixedMatrix(J(d)),
  );

  // With Earth orientation data.
  const eop = new EarthOrientationParameters({ data: eopData(), addNewLeapSeconds: false });
  Transforms.earthOrientationParameters = eop;
  const eopDate = () => [Math.floor(g.uniform(2457000, 2457902)), g.uniform(0, 86400)];
  s.repeat("computeFixedToIcrfMatrixEop", N, () => [eopDate()], (d) => Transforms.computeFixedToIcrfMatrix(J(d)));
  Transforms.earthOrientationParameters = EarthOrientationParameters.NONE;
  return s;
}

function xysSuite(xys) {
  const g = generators(makeRng(501));
  const s = suite("Iau2006XysData");
  const enc = (r) => (r ? [r.x, r.y, r.s] : r);
  s.repeat("computeXysRadians", N * 2, () => [Math.floor(g.uniform(2442396, 2469830)), g.uniform(0, 90000)], (d, sec) => enc(xys.computeXysRadians(d, sec)));
  // Both ends of the data and just outside them.
  for (const [d, sec] of [[2442396, 43200], [2442396, 43199], [2442397, 0], [2469822, 43200], [2469821, 43200], [2469830, 0], [2442400, 100000]]) {
    s.add("computeXysRadians", [d, sec], (d, sec) => enc(xys.computeXysRadians(d, sec)));
  }
  return s;
}

function orientationSuite() {
  const g = generators(makeRng(502));
  const s = suite("IauOrientation");
  const date = () => [Math.floor(g.uniform(2415021, 2488070)), g.uniform(0, 86400)];
  s.repeat("computeMoon", N, () => [date()], (d) => {
    const p = Iau2000Orientation.ComputeMoon(J(d));
    return [p.rightAscension, p.declination, p.rotation, p.rotationRate];
  });
  const axes = new IauOrientationAxes();
  s.repeat("evaluate", N, () => [date()], (d) => axes.evaluate(J(d)));
  return s;
}

/** Runs last: loading with addNewLeapSeconds changes JulianDate.leapSeconds. */
function eopSuite() {
  const g = generators(makeRng(503));
  const s = suite("EarthOrientationParameters");
  // The table itself, so the Rust tests load the same data.
  s.add("data", [], eopData);
  const enc = (r) => [r.xPoleWander, r.yPoleWander, r.xPoleOffset, r.yPoleOffset, r.ut1MinusUtc];
  const sample = () => {
    const r = g.rng();
    if (r < 0.5) return [Math.floor(g.uniform(2457000, 2457902)), g.uniform(0, 86400)];
    if (r < 0.7) {
      // Exactly on a row: MJD + 2400000.5 + TAI − UTC seconds.
      const mjd = Math.floor(g.uniform(57000, 57901));
      const tai = mjd < 57204 ? 35 : mjd < 57754 ? 36 : 37;
      return [mjd + 2400000, 43200 + tai];
    }
    if (r < 0.85) return [g.pick([2457204, 2457754]), 43200 + g.pick([34.5, 35, 35.5, 36, 36.5, 37, 37.5])];
    return [g.pick([2456000, 2457000, 2457950, 2458500]), g.uniform(0, 86400)];
  };
  // Each case evaluates several dates in order on one fresh object, so the
  // cached-index fast path runs too.
  s.repeat("compute", N * 2, () => [Array.from({ length: 6 }, sample)], (dates) => {
    const eop = new EarthOrientationParameters({ data: eopData(), addNewLeapSeconds: false });
    return dates.map((d) => enc(eop.compute(J(d))));
  });
  s.add("computeNone", [[2457100, 1000]], (d) => enc(EarthOrientationParameters.NONE.compute(J(d))));
  s.add("computeEmpty", [[2457100, 1000]], (d) => enc(new EarthOrientationParameters().compute(J(d))));

  // Rows past each leap second become leap-second entries if they are new.
  s.add("leapSecondsAfterLoad", [], () => {
    const before = JulianDate.leapSeconds.length;
    new EarthOrientationParameters({ data: eopData(), addNewLeapSeconds: true });
    return [JulianDate.leapSeconds.length - before, JulianDate.leapSeconds.map((l) => [l.julianDate.dayNumber, l.julianDate.secondsOfDay, l.offset])];
  });
  return s;
}

export default function coreAstronomy() {
  const xys = loadXys();
  return [transformsSuite(xys), xysSuite(xys), orientationSuite(), eopSuite()];
}
