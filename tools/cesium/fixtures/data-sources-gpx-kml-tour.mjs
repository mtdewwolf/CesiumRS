import * as C from "@cesium/engine";
import { DOMParser } from "@xmldom/xmldom";
import { suite, enc } from "./lib.mjs";

const FIXED = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");

// ---------------------------------------------------------------------------
// GpxDataSource
// ---------------------------------------------------------------------------

const FULL_GPX =
  '<?xml version="1.0" encoding="UTF-8"?>' +
  '<gpx version="1.1" creator="Fixture" xmlns="http://www.topografix.com/GPX/1/1">' +
  "<metadata>" +
  "<name>Test Track</name><desc>A test GPX</desc>" +
  "<author><name>Jane Doe</name>" +
  "<email><id>jane</id><domain>example.com</domain></email>" +
  '<link href="https://example.com/author"><text>Author</text><type>text/html</type></link>' +
  "</author>" +
  '<copyright author="ACME Corp"><year>2020</year><license>MIT</license></copyright>' +
  '<link href="https://example.com/gpx"><text>GPX Link</text><type>application/gpx+xml</type></link>' +
  "<time>2020-01-01T00:00:00Z</time><keywords>hiking, test, gpx</keywords>" +
  "<bounds><minlat>40.0</minlat><maxlat>41.0</maxlat><minlon>-106.0</minlon><maxlon>-105.0</maxlon></bounds>" +
  "</metadata>" +
  '<wpt id="w1" lat="40.5" lon="-105.5"><ele>1600</ele><name>Waypoint One</name>' +
  "<cmt>A waypoint comment</cmt><desc>Waypoint description</desc></wpt>" +
  '<rte id="r1"><name>Route One</name>' +
  '<rtept id="rp1" lat="40.0" lon="-105.0"><ele>1500</ele><name>Route Start</name></rtept>' +
  '<rtept id="rp2" lat="41.0" lon="-106.0"><ele>1700</ele><name>Route End</name></rtept>' +
  "</rte>" +
  '<trk id="t1"><name>Track One</name>' +
  "<trkseg>" +
  '<trkpt lat="40.0" lon="-105.0"><ele>1500</ele><time>2020-01-01T00:00:00Z</time></trkpt>' +
  '<trkpt lat="40.1" lon="-105.1"><ele>1510</ele><time>2020-01-01T00:01:00Z</time></trkpt>' +
  "</trkseg><trkseg>" +
  '<trkpt lat="40.2" lon="-105.2"><ele>1520</ele><time>2020-01-01T00:02:00Z</time></trkpt>' +
  '<trkpt lat="40.3" lon="-105.3"><ele>1530</ele><time>2020-01-01T00:03:00Z</time></trkpt>' +
  "</trkseg></trk></gpx>";

const WAYPOINTS_GPX =
  '<?xml version="1.0" encoding="UTF-8"?>' +
  '<gpx version="1.1" creator="Waypoints" xmlns="http://www.topografix.com/GPX/1/1">' +
  '<wpt id="a" lat="10.0" lon="20.0"><ele>100</ele><name>A</name></wpt>' +
  '<wpt id="b" lat="11.0" lon="21.0"><name>B</name></wpt>' +
  "</gpx>";

const TRACK_GPX =
  '<?xml version="1.0" encoding="UTF-8"?>' +
  '<gpx version="1.1" creator="Track" xmlns="http://www.topografix.com/GPX/1/1">' +
  '<trk id="t"><name>Short Track</name><trkseg>' +
  '<trkpt lat="30.0" lon="-100.0"><ele>500</ele><time>2020-01-01T00:00:00Z</time></trkpt>' +
  '<trkpt lat="30.1" lon="-100.1"><ele>510</ele><time>2020-01-01T00:00:30Z</time></trkpt>' +
  "</trkseg></trk></gpx>";

const INVALID_GPX = "<gpx><broken>";

function encodeLink(l) {
  if (!l) return null;
  return { href: enc(l.href), text: enc(l.text), mimeType: enc(l.mimeType) };
}
function encodeMetadata(m) {
  if (!m) return null;
  return {
    name: enc(m.name),
    desc: enc(m.desc),
    author: m.author
      ? { name: enc(m.author.name), email: enc(m.author.email), link: encodeLink(m.author.link) }
      : null,
    copyright: m.copyright
      ? { author: enc(m.copyright.author), year: enc(m.copyright.year), license: enc(m.copyright.license) }
      : null,
    link: encodeLink(m.link),
    time: enc(m.time),
    keywords: enc(m.keywords),
    bounds: m.bounds
      ? {
          minLat: enc(m.bounds.minLat),
          maxLat: enc(m.bounds.maxLat),
          minLon: enc(m.bounds.minLon),
          maxLon: enc(m.bounds.maxLon),
        }
      : null,
  };
}
function encodeClock(clock) {
  if (!clock) return null;
  return {
    start: C.JulianDate.toIso8601(clock.startTime),
    stop: C.JulianDate.toIso8601(clock.stopTime),
    current: C.JulianDate.toIso8601(clock.currentTime),
    range: clock.clockRange,
    step: clock.clockStep,
    multiplier: clock.multiplier,
  };
}
function prop(property, time) {
  return property ? enc(property.getValue(time)) : null;
}
function snapshot(e) {
  const t = e.availability ? e.availability.start : FIXED;
  const p = (x) => prop(x, t);
  const material =
    e.polyline && e.polyline.material
      ? {
          color: p(e.polyline.material.color),
          outlineColor: p(e.polyline.material.outlineColor),
          outlineWidth: p(e.polyline.material.outlineWidth),
        }
      : null;
  return {
    id: e.id,
    name: enc(e.name),
    description: p(e.description),
    availability: e.availability
      ? [C.JulianDate.toIso8601(e.availability.start), C.JulianDate.toIso8601(e.availability.stop)]
      : null,
    position: p(e.position),
    billboard: e.billboard
      ? {
          image: p(e.billboard.image),
          width: p(e.billboard.width),
          height: p(e.billboard.height),
          scaleByDistance: p(e.billboard.scaleByDistance),
          pixelOffsetScaleByDistance: p(e.billboard.pixelOffsetScaleByDistance),
          verticalOrigin: p(e.billboard.verticalOrigin),
          heightReference: p(e.billboard.heightReference),
        }
      : null,
    label: e.label
      ? {
          text: p(e.label.text),
          font: p(e.label.font),
          style: p(e.label.style),
          pixelOffset: p(e.label.pixelOffset),
          horizontalOrigin: p(e.label.horizontalOrigin),
          translucencyByDistance: p(e.label.translucencyByDistance),
          heightReference: p(e.label.heightReference),
        }
      : null,
    polyline: e.polyline
      ? {
          width: p(e.polyline.width),
          material,
          clampToGround: p(e.polyline.clampToGround),
          positions: p(e.polyline.positions),
        }
      : null,
  };
}
async function gpxCase(input, options) {
  const s = new C.GpxDataSource();
  const trace = { changed: 0, loading: [], errorCount: 0 };
  s.changedEvent.addEventListener(() => trace.changed++);
  s.loadingEvent.addEventListener((_, v) => trace.loading.push(v));
  s.errorEvent.addEventListener(() => trace.errorCount++);
  let error = false;
  try {
    await s.load(input, options);
  } catch (e) {
    error = true;
  }
  return {
    name: enc(s.name),
    creator: enc(s.creator),
    version: enc(s.version),
    metadata: encodeMetadata(s.metadata),
    clock: encodeClock(s.clock),
    isLoading: s.isLoading,
    entities: s.entities.values.map(snapshot),
    trace,
    error,
  };
}

async function gpxCases() {
  const doc = (xml) => new DOMParser().parseFromString(xml, "application/xml");
  const cases = [];
  cases.push({
    f: "load",
    a: ["full"],
    r: await gpxCase(doc(FULL_GPX), { waypointImage: "marker.png" }),
  });
  cases.push({
    f: "load",
    a: ["styled"],
    r: await gpxCase(doc(FULL_GPX), {
      clampToGround: true,
      waypointImage: "waypoint.png",
      trackColor: C.Color.fromCssColorString("#00ff00"),
      routeColor: C.Color.fromCssColorString("#0000ff"),
    }),
  });
  cases.push({
    f: "load",
    a: ["waypoints"],
    r: await gpxCase(doc(WAYPOINTS_GPX), { waypointImage: "marker.png" }),
  });
  cases.push({
    f: "load",
    a: ["track"],
    r: await gpxCase(doc(TRACK_GPX), { waypointImage: "marker.png" }),
  });
  cases.push({ f: "load", a: ["invalid"], r: await gpxCase(new Blob([INVALID_GPX]), undefined) });
  return cases;
}

// ---------------------------------------------------------------------------
// KmlCamera / KmlLookAt
// ---------------------------------------------------------------------------

function kmlCamera() {
  const s = suite("KmlCamera");
  s.add(
    "capture",
    [[1, 2, 3], [0.1, 0.2, 0.3]],
    (position, hpr) => {
      const c = new C.KmlCamera(
        new C.Cartesian3(position[0], position[1], position[2]),
        new C.HeadingPitchRoll(hpr[0], hpr[1], hpr[2]),
      );
      return {
        position: enc(c.position),
        headingPitchRoll: enc(c.headingPitchRoll),
      };
    },
  );
  return [s];
}
function kmlLookAt() {
  const s = suite("KmlLookAt");
  s.add(
    "capture",
    [[4, 5, 6], [0.4, 0.5, 100]],
    (position, hpr) => {
      const c = new C.KmlLookAt(
        new C.Cartesian3(position[0], position[1], position[2]),
        new C.HeadingPitchRange(hpr[0], hpr[1], hpr[2]),
      );
      return {
        position: enc(c.position),
        headingPitchRange: [c.headingPitchRange.heading, c.headingPitchRange.pitch, c.headingPitchRange.range],
      };
    },
  );
  return [s];
}

// ---------------------------------------------------------------------------
// KmlTourFlyTo / KmlTourWait / KmlTour
// ---------------------------------------------------------------------------

function encodeOptions(o) {
  return {
    duration: o.duration,
    hasComplete: !!o.complete,
    hasEasingFunction: !!o.easingFunction,
    destination: o.destination ? [o.destination.x, o.destination.y, o.destination.z] : null,
    orientation: o.orientation
      ? [o.orientation.heading, o.orientation.pitch, o.orientation.roll]
      : null,
    offset: o.offset ? [o.offset.heading, o.offset.pitch, o.offset.range] : null,
  };
}

function kmlTourFlyTo() {
  const s = suite("KmlTourFlyTo");
  const views = {
    camera: new C.KmlCamera(new C.Cartesian3(1, 2, 3), new C.HeadingPitchRoll(0.1, 0.2, 0.3)),
    lookAt: new C.KmlLookAt(new C.Cartesian3(4, 5, 6), new C.HeadingPitchRange(0.4, 0.5, 100)),
  };
  for (const viewKind of ["camera", "lookAt"]) {
    for (const mode of ["bounce", "smooth"]) {
      s.add("options", [viewKind, mode], (vk, m) => {
        const f = new C.KmlTourFlyTo(3.5, m, views[vk]);
        return {
          type: f.type,
          blocking: f.blocking,
          duration: f.duration,
          flyToMode: f.flyToMode,
          activeCamera: !!f.activeCamera,
          activeCallback: !!f.activeCallback,
          options: encodeOptions(f.getCameraOptions(undefined)),
        };
      });
    }
  }
  // cameraOptions merge overrides duration.
  s.add("merge", [], () => {
    const f = new C.KmlTourFlyTo(5, "bounce", views.camera);
    return encodeOptions(f.getCameraOptions({ duration: 99 }));
  });
  // play -> stop lifecycle with a mock camera.
  s.add("playStop", [], () => {
    const f = new C.KmlTourFlyTo(2, "smooth", views.camera);
    const trace = [];
    const camera = {
      flyTo(opts) {
        trace.push(["flyTo", encodeOptions(opts)]);
      },
      flyToBoundingSphere(target, opts) {
        trace.push(["flyToBoundingSphere", [target.center.x, target.center.y, target.center.z], encodeOptions(opts)]);
      },
      cancelFlight() {
        trace.push(["cancelFlight"]);
      },
    };
    f.play((terminated) => trace.push(["done", terminated]), camera, undefined);
    const afterPlay = { activeCamera: !!f.activeCamera, activeCallback: !!f.activeCallback };
    f.stop();
    const afterStop = { activeCamera: !!f.activeCamera, activeCallback: !!f.activeCallback };
    return { afterPlay, afterStop, trace };
  });
  return [s];
}

function kmlTourWait() {
  const s = suite("KmlTourWait");
  s.add("capture", [2.5], (duration) => {
    const w = new C.KmlTourWait(duration);
    const state = () => ({
      type: w.type,
      blocking: w.blocking,
      duration: w.duration,
      hasCallback: !!w.activeCallback,
      hasTimeout: !!w.timeout,
    });
    const before = state();
    const calls = [];
    w.play((terminated) => calls.push(terminated));
    const afterPlay = state();
    w.stop();
    const afterStop = state();
    return [before, afterPlay, calls, afterStop];
  });
  return [s];
}

function kmlTour() {
  const s = suite("KmlTour");
  const mk = (id) => ({
    id,
    blocking: true,
    play(done) {
      this.done = done;
    },
    stop() {
      if (this.done) {
        this.done(true);
        this.done = undefined;
      }
    },
  });
  s.add("play", [], () => {
    const t = new C.KmlTour("My Tour", "tour-1");
    const trace = [];
    t.tourStart.addEventListener(() => trace.push(["tourStart"]));
    t.tourEnd.addEventListener((terminated) => trace.push(["tourEnd", terminated]));
    t.entryStart.addEventListener((entry) => trace.push(["entryStart", entry.id]));
    t.entryEnd.addEventListener((entry, terminated) => trace.push(["entryEnd", entry.id, terminated]));
    const entries = ["a", "b", "c"].map(mk);
    for (const e of entries) t.addPlaylistEntry(e);
    const before = {
      id: t.id,
      name: t.name,
      playlistIndex: t.playlistIndex,
      playlist: t.playlist.map((e) => e.id),
      active: t._activeEntries.map((e) => e.id),
    };
    // Make entries complete synchronously.
    for (const e of entries) e.play = function (done) { trace.push(["play", this.id]); done(false); };
    t.play({ scene: { camera: {} } });
    const afterPlay = {
      playlistIndex: t.playlistIndex,
      active: t._activeEntries.map((e) => e.id),
    };
    return { before, afterPlay, trace };
  });
  s.add("stop", [], () => {
    const t = new C.KmlTour("Tour", "t2");
    const trace = [];
    t.tourStart.addEventListener(() => trace.push(["tourStart"]));
    t.tourEnd.addEventListener((terminated) => trace.push(["tourEnd", terminated]));
    t.entryStart.addEventListener((entry) => trace.push(["entryStart", entry.id]));
    t.entryEnd.addEventListener((entry, terminated) => trace.push(["entryEnd", entry.id, terminated]));
    const a = mk("a");
    a.play = function (done) { trace.push(["play", this.id]); this.done = done; };
    const b = mk("b");
    b.play = function (done) { trace.push(["play", this.id]); this.done = done; };
    t.addPlaylistEntry(a);
    t.addPlaylistEntry(b);
    t.play({ scene: { camera: {} } });
    t.stop();
    return { trace };
  });
  return [s];
}

// ---------------------------------------------------------------------------
// heightReferenceOnEntityPropertyChanged
// ---------------------------------------------------------------------------

function heightReference() {
  const s = suite("heightReferenceOnEntityPropertyChanged");
  // The helper first invokes the base GeometryUpdater._onEntityPropertyChanged,
  // which is not ported. Mock it so the terrain-offset logic is isolated.
  const saved = C.GeometryUpdater.prototype._onEntityPropertyChanged;
  const run = (opts) => {
    const entity = new C.Entity({ id: "e" });
    if (opts.geometry) {
      entity.billboard = new C.BillboardGraphics();
      if (opts.heightReference) entity.billboard.heightReference = C.HeightReference.CLAMP_TO_GROUND;
    }
    const trace = [];
    const host = {
      _observedPropertyNames: ["billboard"],
      _entity: entity,
      _geometryPropertyName: "billboard",
      _scene: {},
      _dynamic: true,
      _terrainOffsetProperty: opts.preExisting ? { destroy: () => trace.push(["destroy"]) } : undefined,
      _computeCenter: () => new C.Cartesian3(1, 2, 3),
    };
    C.GeometryUpdater.prototype._onEntityPropertyChanged = function (entity, propertyName) {
      trace.push(["base", propertyName]);
    };
    C.heightReferenceOnEntityPropertyChanged.call(host, entity, opts.propertyName, null, null);
    return { trace, terrainOffset: !!host._terrainOffsetProperty };
  };
  try {
    s.add("run", ["withHeight"], () =>
      run({ propertyName: "billboard", geometry: true, heightReference: true }),
    );
    s.add("run", ["noHeight"], () =>
      run({ propertyName: "billboard", geometry: true, heightReference: false }),
    );
    s.add("run", ["notObserved"], () =>
      run({ propertyName: "position", geometry: true, heightReference: true }),
    );
    s.add("run", ["noGeometry"], () =>
      run({ propertyName: "billboard", geometry: false, heightReference: false }),
    );
    s.add("run", ["preExisting"], () =>
      run({ propertyName: "billboard", geometry: true, heightReference: true, preExisting: true }),
    );
  } finally {
    C.GeometryUpdater.prototype._onEntityPropertyChanged = saved;
  }
  return [s];
}

export default async function dataSourcesGpxKmlTour() {
  return [
    { module: "GpxDataSource", cases: await gpxCases() },
    ...kmlCamera(),
    ...kmlLookAt(),
    ...kmlTourFlyTo(),
    ...kmlTourWait(),
    ...kmlTour(),
    ...heightReference(),
  ];
}
