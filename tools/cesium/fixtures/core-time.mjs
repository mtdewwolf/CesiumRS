// Fixtures for JulianDate, GregorianDate, TimeInterval,
// TimeIntervalCollection, and Clock.
//
// Dates are encoded as [dayNumber, secondsOfDay] in TAI, intervals as
// [start, stop, isStartIncluded, isStopIncluded, data].

import {
  Clock,
  ClockRange,
  ClockStep,
  GregorianDate,
  JulianDate,
  Simon1994PlanetaryPositions,
  TimeInterval,
  TimeIntervalCollection,
  TimeStandard,
} from "@cesium/engine";
import { generators, makeRng, suite } from "./lib.mjs";

const N = 40;

const J = ([d, s]) => new JulianDate(d, s, TimeStandard.TAI);
const jEnc = (j) => [j.dayNumber, j.secondsOfDay];
const gEnc = (g) => [g.year, g.month, g.day, g.hour, g.minute, g.second, g.millisecond, g.isLeapSecond];
const I = ([start, stop, a, b, data]) => new TimeInterval({ start: J(start), stop: J(stop), isStartIncluded: a, isStopIncluded: b, data: data ?? undefined });
const iEnc = (i) => [jEnc(i.start), jEnc(i.stop), i.isStartIncluded, i.isStopIncluded, i.data ?? null];
const cEnc = (c) => Array.from({ length: c.length }, (_, k) => iEnc(c.get(k)));
const invalid = (fn) => {
  try {
    return fn();
  } catch (e) {
    if (e.name === "DeveloperError") return "invalid";
    throw e;
  }
};

function dates(g) {
  const leap = JulianDate.leapSeconds;
  return () => {
    const r = g.rng();
    if (r < 0.4) return jEnc(new JulianDate(Math.floor(g.uniform(2400000, 2500000)), g.uniform(0, 86400), TimeStandard.TAI));
    if (r < 0.7) {
      const l = g.pick(leap).julianDate;
      return jEnc(JulianDate.addSeconds(l, g.pick([-2, -1, -0.5, 0, 0.25, 1, 2, g.uniform(-90000, 90000)]), new JulianDate()));
    }
    if (r < 0.85) return [Math.floor(g.uniform(2440000, 2460000)), g.pick([0, 43200, 86399.999, 0.0005, 43236])];
    return [g.pick([2451545, 2457754, 2400001]), g.uniform(0, 86400)];
  };
}

function julianDateSuite() {
  const g = generators(makeRng(400));
  const date = dates(g);
  const s = suite("JulianDate");
  s.repeat("new", N, () => [g.uniform(2400000, 2500000), g.pick([0, g.uniform(-1e5, 1e5), 43200]), g.pick([0, 1])], (d, sod, std) =>
    jEnc(new JulianDate(d, sod, std === 0 ? TimeStandard.UTC : TimeStandard.TAI)),
  );
  // Around leap seconds, in UTC.
  for (const l of JulianDate.leapSeconds) {
    for (const sod of [43199, 43200, 43200 + l.offset, 86399.5]) {
      s.add("new", [l.julianDate.dayNumber, sod, 0], (d, sod, std) => jEnc(new JulianDate(d, sod, std === 0 ? TimeStandard.UTC : TimeStandard.TAI)));
    }
  }
  const greg = () => {
    const y = Math.floor(g.uniform(1850, 2150));
    const m = 1 + Math.floor(g.rng() * 12);
    const d = 1 + Math.floor(g.rng() * 28);
    return [y, m, d, Math.floor(g.rng() * 24), Math.floor(g.rng() * 60), Math.floor(g.rng() * 60), g.pick([0, g.uniform(0, 999.999), 500]), false];
  };
  const fromGreg = (y, mo, d, h, mi, sec, ms, leapSecond) => jEnc(JulianDate.fromGregorianDate(new GregorianDate(y, mo, d, h, mi, sec, ms, leapSecond)));
  s.repeat("fromGregorianDate", N, greg, fromGreg);
  s.add("fromGregorianDate", [2016, 12, 31, 23, 59, 60, 0, true], fromGreg);
  s.add("fromGregorianDate", [2016, 12, 31, 23, 59, 60, 500, true], fromGreg);
  s.add("fromGregorianDate", [1972, 6, 30, 23, 59, 60, 0, true], fromGreg);
  s.add("fromGregorianDate", [2000, 1, 1, 12, 0, 0, 0, false], fromGreg);
  s.repeat("fromDate", N, () => [Math.round(g.uniform(-3e12, 5e12)) + g.pick([0, 0.5])], (ms) => jEnc(JulianDate.fromDate(new Date(ms))));

  const isoStrings = [
    "2012-03-05T10:15:30Z",
    "2012-03-05T10:15:30.123456Z",
    "2012-03-05T10:15:30,5Z",
    "2012-03-05T10:15Z",
    "2012-03-05T10Z",
    "2012-03-05T10:15:30",
    "2012-03-05T10:15:30+05:30",
    "2012-03-05T10:15:30-0800",
    "2012-03-05T10:15:30+05",
    "2012-03-05T23:30:00-02:00",
    "2012-03-05T00:30:00+02:00",
    "20120305T101530Z",
    "20120305T1015Z",
    "2012-03-05",
    "20120305",
    "2012-03",
    "2012",
    "2012-065",
    "2012065",
    "2012-W10",
    "2012-W10-1",
    "2012W107",
    "2009-W53-7",
    "2010-W01-1",
    "2016-12-31T23:59:60Z",
    "2016-12-31T23:59:60.5Z",
    "2016-12-31T23:59:60+01:00",
    "2015-06-30T23:59:60Z",
    "2012-03-05T24:00:00Z",
    "2012-02-29T12:00:00Z",
    "2013-02-29T12:00:00Z",
    "2012-13-01",
    "2012-00-01",
    "2012-04-31",
    "2012-03-05T10:61:00Z",
    "2012-03-05T25:00:00Z",
    "2012-03-05T10:15:30Q",
    "garbage",
    "",
    "2012-366",
    "2013-366",
    "1990-12-31T23:59:60Z",
    "1858-11-17T00:00:00Z",
    "2100-01-01T00:00:00.000001Z",
    "0099-01-01T00:00:00Z",
    "2012-03-05T10:15:30.9999999Z",
    "2012-03-05T10:15:30.Z",
    "2012-03-05T23:59:60Z",
    "2012-06-30T23:59:60Z",
    "2012-03-05T10:15:30+14:00",
    "2012-03-05t10:15:30z",
  ];
  for (const str of isoStrings) s.add("fromIso8601", [str], (x) => invalid(() => jEnc(JulianDate.fromIso8601(x))));

  s.repeat("toGregorianDate", N * 2, () => [date()], (d) => gEnc(JulianDate.toGregorianDate(J(d))));
  s.repeat("toDate", N, () => [date()], (d) => JulianDate.toDate(J(d)).getTime());
  s.repeat("toIso8601", N * 2, () => [date(), g.pick([null, null, 0, 1, 3, 6, 9])], (d, p) => JulianDate.toIso8601(J(d), p ?? undefined));
  s.add("toIso8601", [[2451545, 32.0000001], 3], (d, p) => JulianDate.toIso8601(J(d), p));
  s.add("toIso8601", [[2451545, 32.9996], 3], (d, p) => JulianDate.toIso8601(J(d), p));
  s.add("toIso8601", [[2451545, 32.9996], null], (d) => JulianDate.toIso8601(J(d)));
  s.repeat("roundTripIso8601", N, () => [date()], (d) => jEnc(JulianDate.fromIso8601(JulianDate.toIso8601(J(d)))));
  const pair = () => {
    const a = date();
    return [a, g.pick([a, date(), [a[0], a[1] + g.pick([1e-9, -1e-9, 0.5])], [a[0] + 1, a[1] - 86400]])];
  };
  s.repeat("compare", N, pair, (a, b) => JulianDate.compare(J(a), J(b)));
  s.repeat("equals", N, pair, (a, b) => JulianDate.equals(J(a), J(b)));
  s.repeat("equalsEpsilon", N, () => [...pair(), g.pick([0, 1e-6, 1])], (a, b, e) => JulianDate.equalsEpsilon(J(a), J(b), e));
  s.repeat("lessThan", N, pair, (a, b) => JulianDate.lessThan(J(a), J(b)));
  s.repeat("lessThanOrEquals", N, pair, (a, b) => JulianDate.lessThanOrEquals(J(a), J(b)));
  s.repeat("greaterThan", N, pair, (a, b) => JulianDate.greaterThan(J(a), J(b)));
  s.repeat("greaterThanOrEquals", N, pair, (a, b) => JulianDate.greaterThanOrEquals(J(a), J(b)));
  s.repeat("totalDays", N, () => [date()], (d) => JulianDate.totalDays(J(d)));
  s.repeat("secondsDifference", N, pair, (a, b) => JulianDate.secondsDifference(J(a), J(b)));
  s.repeat("daysDifference", N, pair, (a, b) => JulianDate.daysDifference(J(a), J(b)));
  s.repeat("computeTaiMinusUtc", N * 2, () => [date()], (d) => JulianDate.computeTaiMinusUtc(J(d)));
  const amount = () => g.pick([0, 1, -1, 0.5, g.uniform(-1e6, 1e6), g.uniform(-10, 10)]);
  for (const f of ["addSeconds", "addMinutes", "addHours", "addDays"]) {
    s.repeat(f, N, () => [date(), amount()], (d, x) => jEnc(JulianDate[f](J(d), x, new JulianDate())));
  }
  return s;
}

function timeIntervalSuite() {
  const g = generators(makeRng(401));
  const s = suite("TimeInterval");
  const point = () => [2451545 + Math.floor(g.rng() * 2), Math.floor(g.rng() * 8) * 10800];
  const data = () => g.pick([1, 2, 3, null]);
  const interval = () => [point(), point(), g.pick([true, false]), g.pick([true, false]), data()];
  const isos = [
    "2012-03-05T10:15:30Z/2012-03-06T10:15:30Z",
    "2012-03-05/2012-03-05",
    "2012-03-06/2012-03-05",
    "2016-12-31T23:59:59Z/2017-01-01T00:00:01Z",
    "2012-03-05T10:15:30Z",
    "2012-03-05T10:15:30Z/garbage",
  ];
  for (const str of isos) {
    for (const [a, b] of [[true, true], [false, true], [true, false]]) {
      s.add("fromIso8601", [str, a, b], (x, a, b) => invalid(() => iEnc(TimeInterval.fromIso8601({ iso8601: x, isStartIncluded: a, isStopIncluded: b }))));
    }
  }
  s.repeat("toIso8601", N, () => [interval(), g.pick([null, 0, 2])], (i, p) => TimeInterval.toIso8601(I(i), p ?? undefined));
  s.repeat("isEmpty", N, () => [interval()], (i) => I(i).isEmpty);
  s.repeat("contains", N * 2, () => [interval(), point()], (i, d) => TimeInterval.contains(I(i), J(d)));
  s.repeat("equals", N, () => {
    const a = interval();
    return [a, g.pick([a, interval(), [a[0], a[1], !a[2], a[3], a[4]], [a[0], a[1], a[2], a[3], data()]])];
  }, (a, b) => TimeInterval.equals(I(a), I(b)));
  s.repeat("equalsEpsilon", N, () => {
    const a = interval();
    return [a, [[a[0][0], a[0][1] + g.pick([0, 0.5, 2])], a[1], a[2], a[3], a[4]], g.pick([0, 1])];
  }, (a, b, e) => TimeInterval.equalsEpsilon(I(a), I(b), e));
  s.repeat("intersect", N * 2, () => [interval(), interval(), g.pick([true, false])], (a, b, merge) =>
    iEnc(TimeInterval.intersect(I(a), I(b), new TimeInterval(), merge ? (x, y) => (x ?? 0) * 10 + (y ?? 0) : undefined)),
  );
  return s;
}

const opts = (o) => ({ isStartIncluded: o[0], isStopIncluded: o[1], leadingInterval: o[2], trailingInterval: o[3] });

function collectionSuite() {
  const g = generators(makeRng(402));
  const s = suite("TimeIntervalCollection");
  const point = () => [2451545 + Math.floor(g.rng() * 2), Math.floor(g.rng() * 8) * 10800];
  const interval = () => [point(), point(), g.pick([true, false]), g.pick([true, false]), g.pick([1, 2, null])];
  const grid = [];
  for (let d = 0; d < 2; d++) for (let k = 0; k <= 8; k++) grid.push([2451545 + d, k * 10800]);
  const build = (ops) => {
    const c = new TimeIntervalCollection();
    let changed = 0;
    c.changedEvent.addEventListener(() => changed++);
    const removed = [];
    for (const [op, i] of ops) {
      if (op === "add") c.addInterval(I(i));
      else removed.push(c.removeInterval(I(i)));
    }
    return { c, changed, removed };
  };
  const ops = () => Array.from({ length: 1 + Math.floor(g.rng() * 8) }, () => [g.rng() < 0.7 ? "add" : "remove", interval()]);
  s.repeat("ops", N * 4, () => [ops()], (o) => {
    const { c, changed, removed } = build(o);
    return [
      cEnc(c),
      removed,
      changed,
      grid.map((d) => c.indexOf(J(d))),
      grid.map((d) => c.findDataForIntervalContainingDate(J(d)) ?? null),
      c.isEmpty ? null : [jEnc(c.start), c.isStartIncluded, jEnc(c.stop), c.isStopIncluded],
    ];
  });
  s.repeat("intersect", N * 2, () => [ops(), ops(), g.pick([true, false])], (a, b, merge) =>
    cEnc(build(a).c.intersect(build(b).c, undefined, merge ? (x, y) => (x ?? 0) * 10 + (y ?? 0) : undefined)),
  );
  const flags = () => [g.pick([true, false]), g.pick([true, false]), g.pick([true, false]), g.pick([true, false])];
  s.repeat("fromJulianDateArray", N, () => {
    const n = 1 + Math.floor(g.rng() * 5);
    const ds = Array.from({ length: n }, point).sort((a, b) => a[0] - b[0] || a[1] - b[1]);
    return [ds, flags()];
  }, (ds, o) => cEnc(TimeIntervalCollection.fromJulianDateArray({ julianDates: ds.map(J), ...opts(o), dataCallback: (_, i) => i })));
  const isoIntervals = [
    "2012-03-05T00:00:00Z/2012-03-06T00:00:00Z",
    "2012-03-05T00:00:00Z/2012-03-06T00:00:00Z/PT6H",
    "2012-03-05T00:00:00Z/2012-03-06T00:00:00Z/PT7H",
    "2012-01-31T00:00:00Z/2012-06-01T00:00:00Z/P1M",
    "2011-01-31T00:00:00Z/2013-06-01T00:00:00Z/P1Y2M",
    "2012-02-28T00:00:00Z/2012-03-02T00:00:00Z/P1D",
    "2012-03-05T00:00:00Z/2012-03-05T01:00:00Z/PT1.5M",
    "2012-03-05T00:00:00Z/2012-03-05T00:00:10Z/PT2.25S",
    "2012-03-05T00:00:00Z/2012-03-20T00:00:00Z/P1W",
    "2012-03-05T00:00:00Z/2012-03-05T12:00:00Z/03:00:00",
    "2016-12-31T22:00:00Z/2017-01-01T02:00:00Z/PT1H",
    "2012-03-05T00:00:00Z/2012-03-06T00:00:00Z/P0D",
    "2012-03-05T00:00:00Z/2012-03-05T00:01:00Z/PT0,5M",
  ];
  for (const str of isoIntervals) {
    for (const o of [[true, true, false, false], [false, true, true, true], [true, false, false, true]]) {
      s.add("fromIso8601", [str, o], (x, o) => invalid(() => cEnc(TimeIntervalCollection.fromIso8601({ iso8601: x, ...opts(o), dataCallback: (_, i) => i }))));
    }
  }
  const dateArrays = [
    ["2012-03-05T00:00:00Z", "2012-03-06T00:00:00Z", "2012-03-07T12:00:00Z"],
    ["2012-03-05T00:00:00Z"],
    ["2016-12-31T23:59:59Z", "2016-12-31T23:59:60Z", "2017-01-01T00:00:00Z"],
  ];
  for (const arr of dateArrays) {
    for (const o of [[true, true, false, false], [false, false, true, true]]) {
      s.add("fromIso8601DateArray", [arr, o], (x, o) => cEnc(TimeIntervalCollection.fromIso8601DateArray({ iso8601Dates: x, ...opts(o), dataCallback: (_, i) => i })));
    }
  }
  const durationArrays = [
    ["P0D", "PT1H", "PT2H", "P1D"],
    ["PT0S", "PT0S", "PT30M", "P1M"],
    ["P1D", "P1D", "P1D"],
    ["PT1H30M", "PT0.5S", "P1Y"],
    ["01:00:00", "P1DT12H"],
  ];
  for (const arr of durationArrays) {
    for (const rel of [false, true]) {
      for (const o of [[true, true, false, false], [false, true, true, false]]) {
        s.add("fromIso8601DurationArray", [[2455990, 43200], arr, rel, o], (e, x, rel, o) =>
          invalid(() => cEnc(TimeIntervalCollection.fromIso8601DurationArray({ epoch: J(e), iso8601Durations: x, relativeToPrevious: rel, ...opts(o), dataCallback: (_, i) => i }))),
        );
      }
    }
  }
  return s;
}

function clockSuite() {
  const g = generators(makeRng(403));
  const s = suite("Clock");
  // Clock reads performance.now(); drive it by hand.
  let now = 1000;
  globalThis.performance.now = () => now;
  const state = (c) => [jEnc(c.currentTime), c.multiplier, c.clockStep, c.shouldAnimate];
  const options = () => {
    const start = [2451545, g.uniform(0, 1000)];
    return [
      start,
      [2451545, start[1] + g.pick([0, 10, 100, 3600])],
      [2451545, start[1] + g.uniform(-50, 200)],
      g.pick([1, 2, -3, 0.5, 60]),
      g.pick([ClockStep.TICK_DEPENDENT, ClockStep.SYSTEM_CLOCK_MULTIPLIER]),
      g.pick([ClockRange.UNBOUNDED, ClockRange.CLAMPED, ClockRange.LOOP_STOP]),
      g.pick([true, true, false]),
      g.pick([true, true, false]),
    ];
  };
  const make = ([start, stop, current, multiplier, clockStep, clockRange, canAnimate, shouldAnimate]) =>
    new Clock({ startTime: J(start), stopTime: J(stop), currentTime: J(current), multiplier, clockStep, clockRange, canAnimate, shouldAnimate });
  s.repeat("tick", N * 2, () => [options(), Array.from({ length: 6 }, () => g.pick([16, 33, 1000, 2500, 0]))], (o, steps) => {
    now = 1000;
    const c = make(o);
    let stops = 0;
    let ticks = 0;
    c.onStop.addEventListener(() => stops++);
    c.onTick.addEventListener(() => ticks++);
    const out = steps.map((dt) => {
      now += dt;
      return jEnc(c.tick());
    });
    return [out, stops, ticks, state(c)];
  });
  // Property setters interact: SYSTEM_CLOCK forces multiplier 1 and animation.
  const setters = () =>
    Array.from({ length: 5 }, () =>
      g.pick([
        ["clockStep", g.pick([0, 1, 2])],
        ["multiplier", g.pick([1, 2, -1])],
        ["shouldAnimate", g.pick([true, false])],
        ["currentTime", [2451545, g.uniform(0, 100)]],
      ]),
    );
  s.repeat("setters", N, () => [options(), setters()], (o, ops) => {
    const c = make(o);
    // Choosing SYSTEM_CLOCK sets the time to now, which is not reproducible.
    let fromSystem = false;
    return ops.map(([k, v]) => {
      c[k] = k === "currentTime" ? J(v) : v;
      if (k === "clockStep" && v === ClockStep.SYSTEM_CLOCK) fromSystem = true;
      if (k === "currentTime") fromSystem = false;
      const st = state(c);
      if (fromSystem) st[0] = "now";
      return st;
    });
  });
  return s;
}

export default function coreTime() {
  return [julianDateSuite(), timeIntervalSuite(), collectionSuite(), clockSuite(), simonSuite()];
}

export function simonSuite() {
  const g = generators(makeRng(404));
  const s = suite("Simon1994PlanetaryPositions");
  const date = () => [Math.floor(g.uniform(2400000, 2500000)), g.uniform(0, 86400)];
  s.repeat("sun", 60, date, (d, sod) => Simon1994PlanetaryPositions.computeSunPositionInEarthInertialFrame(J([d, sod])));
  s.repeat("moon", 60, date, (d, sod) => Simon1994PlanetaryPositions.computeMoonPositionInEarthInertialFrame(J([d, sod])));
  s.add("sun", [2451545, 0], (d, sod) => Simon1994PlanetaryPositions.computeSunPositionInEarthInertialFrame(J([d, sod])));
  s.add("moon", [2451545, 0], (d, sod) => Simon1994PlanetaryPositions.computeMoonPositionInEarthInertialFrame(J([d, sod])));
  return s;
}
