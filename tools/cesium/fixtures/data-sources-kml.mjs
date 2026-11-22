import "./set-dom-parser.mjs";
import { DOMParser, DOMImplementation, XMLSerializer } from "@xmldom/xmldom";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import * as C from "@cesium/engine";
import { enc } from "./lib.mjs";

const FIXED = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");

function setupNodeDom() {
  const globals = ["document", "XMLSerializer", "HTMLCanvasElement"];
  const originals = globals.map((key) => Object.getOwnPropertyDescriptor(globalThis, key));
  const reader = globalThis.FileReader?.prototype;
  const readAsArrayBuffer = reader?.readAsArrayBuffer;
  const impl = new DOMImplementation();
  const sampleDoc = impl.createDocument("http://www.opengis.net/kml/2.2", "kml");
  const elemProto = Object.getPrototypeOf(sampleDoc.documentElement);
  const innerHTML = Object.getOwnPropertyDescriptor(elemProto, "innerHTML");
  if (!innerHTML) {
    Object.defineProperty(elemProto, "innerHTML", {
      get() {
        const ser = new XMLSerializer();
        let out = "";
        for (let i = 0; i < this.childNodes.length; i++) {
          out += ser.serializeToString(this.childNodes[i]);
        }
        return out;
      },
      configurable: true,
    });
  }

  globalThis.XMLSerializer = XMLSerializer;
  if (typeof globalThis.HTMLCanvasElement === "undefined") {
    globalThis.HTMLCanvasElement = class HTMLCanvasElement {};
  }

  const origDoc = globalThis.document ?? {};
  globalThis.document = {
    ...origDoc,
    location: origDoc.location ?? { href: "http://localhost/" },
    implementation: impl,
    createElement(tag) {
      if (tag === "canvas") {
        const canvas = {
          _isFakeCanvas: true,
          width: 0,
          height: 0,
          getContext() {
            return {
              save() {},
              scale() {},
              beginPath() {},
              moveTo() {},
              lineTo() {},
              bezierCurveTo() {},
              closePath() {},
              fill() {},
              stroke() {},
              restore() {},
              fillRect() {},
              drawImage() {},
              fillStyle: "",
              strokeStyle: "",
              lineWidth: 1,
              globalCompositeOperation: "source-over",
            };
          },
        };
        return canvas;
      }
      return {
        _innerHTML: "",
        set innerHTML(value) {
          this._innerHTML = String(value);
        },
        get innerHTML() {
          return this._innerHTML;
        },
        querySelectorAll: () => [],
      };
    },
  };

  if (globalThis.FileReader && !globalThis.FileReader.prototype.readAsArrayBuffer) {
    globalThis.FileReader.prototype.readAsArrayBuffer = function (blob) {
      blob.arrayBuffer().then(
        (buf) => {
          this.result = buf;
          if (this._onload) this._onload();
        },
        (error) => {
          this.error = error;
          if (this._onerror) this._onerror();
        },
      );
    };
  }
  return () => {
    for (const [index, key] of globals.entries()) {
      if (originals[index]) Object.defineProperty(globalThis, key, originals[index]);
      else delete globalThis[key];
    }
    if (!innerHTML) delete elemProto.innerHTML;
    if (reader && !readAsArrayBuffer) delete reader.readAsArrayBuffer;
  };
}

// ---------------------------------------------------------------------------
// KML Documents for KmlDataSource fixtures
// ---------------------------------------------------------------------------

export const KML_DOCS = {
  placemark_point_styled: `<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2" xmlns:atom="http://www.w3.org/2005/Atom">
<Document>
  <name>Styled Points</name>
  <Style id="customIcon">
    <IconStyle>
      <color>8000ff00</color>
      <scale>1.5</scale>
      <heading>45</heading>
      <Icon>
        <href>https://example.com/icons/star.png</href>
        <gx:x>32</gx:x>
        <gx:y>64</gx:y>
        <gx:w>32</gx:w>
        <gx:h>32</gx:h>
      </Icon>
      <hotSpot x="10" y="20" xunits="pixels" yunits="pixels"/>
    </IconStyle>
    <LabelStyle>
      <color>ff0000ff</color>
      <scale>1.2</scale>
    </LabelStyle>
    <BalloonStyle>
      <bgColor>ffcccccc</bgColor>
      <textColor>ff112233</textColor>
      <text>Title: $[name] | Desc: $[description] | ID: $[id] | Addr: $[address] | Snip: $[Snippet] | $[city/displayName]=$[city] $[geDirections]</text>
    </BalloonStyle>
  </Style>
  <Placemark id="pm1">
    <name>Boulder</name>
    <description>Mountain town</description>
    <address>123 Pearl St</address>
    <phoneNumber>555-0100</phoneNumber>
    <Snippet>Short note</Snippet>
    <atom:author>
      <atom:name>Alice</atom:name>
      <atom:uri>https://example.com/alice</atom:uri>
      <atom:email>alice@example.com</atom:email>
    </atom:author>
    <atom:link href="https://example.com/boulder" hreflang="en" rel="alternate" type="text/html" title="Boulder" length="42"/>
    <ExtendedData>
      <Data name="city">
        <displayName>City Name</displayName>
        <value>Boulder CO</value>
      </Data>
    </ExtendedData>
    <Camera>
      <longitude>-105.27</longitude>
      <latitude>40.01</latitude>
      <altitude>2000</altitude>
      <heading>90</heading>
      <tilt>45</tilt>
      <roll>10</roll>
    </Camera>
    <LookAt>
      <longitude>-105.27</longitude>
      <latitude>40.01</latitude>
      <altitude>1655</altitude>
      <heading>180</heading>
      <tilt>30</tilt>
      <range>5000</range>
    </LookAt>
    <styleUrl>#customIcon</styleUrl>
    <Point>
      <altitudeMode>relativeToGround</altitudeMode>
      <coordinates>-105.27,40.01,1655</coordinates>
    </Point>
  </Placemark>
</Document>
</kml>`,

  placemark_default_and_empty_icon: `<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2">
<Document>
  <name>Icons And Extrude</name>
  <Placemark id="defaultPin">
    <name>Default Pin</name>
    <ExtendedData>
      <Data name="elevation">
        <displayName>Elevation</displayName>
        <value>1200m</value>
      </Data>
      <Data name="code">
        <value>BDR</value>
      </Data>
    </ExtendedData>
    <Point>
      <coordinates>-105.0,40.0,100</coordinates>
    </Point>
  </Placemark>
  <Placemark id="emptyIcon">
    <name>No Icon</name>
    <Style>
      <IconStyle>
        <scale>0</scale>
        <Icon/>
      </IconStyle>
    </Style>
    <Point>
      <coordinates>-104.0,39.0,0</coordinates>
    </Point>
  </Placemark>
  <Placemark id="paletteIcon">
    <name>Palette</name>
    <Style>
      <IconStyle>
        <Icon>
          <href>root://icons/palette-2.png</href>
          <gx:x>64</gx:x>
          <gx:y>32</gx:y>
        </Icon>
        <hotSpot x="0.5" y="0.25" xunits="fraction" yunits="fraction"/>
      </IconStyle>
      <LineStyle>
        <color>ff00ffff</color>
        <width>3</width>
      </LineStyle>
    </Style>
    <Point>
      <extrude>1</extrude>
      <altitudeMode>absolute</altitudeMode>
      <coordinates>-103.0,38.0,500</coordinates>
    </Point>
  </Placemark>
</Document>
</kml>`,

  folder_hierarchy_and_styles: `<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>Hierarchy</name>
  <Style id="sNormal">
    <IconStyle>
      <scale>2.0</scale>
      <Icon><href>https://example.com/normal.png</href></Icon>
    </IconStyle>
    <LabelStyle>
      <color>ffff0000</color>
    </LabelStyle>
  </Style>
  <StyleMap id="sm1">
    <Pair>
      <key>normal</key>
      <styleUrl>#sNormal</styleUrl>
    </Pair>
  </StyleMap>
  <Folder id="f1">
    <name>Outer Folder</name>
    <TimeSpan>
      <begin>2020-01-01T00:00:00Z</begin>
      <end>2020-01-02T00:00:00Z</end>
    </TimeSpan>
    <Placemark id="child1">
      <name>Child One</name>
      <styleUrl>#sm1</styleUrl>
      <Style>
        <LabelStyle>
          <scale>0.8</scale>
        </LabelStyle>
      </Style>
      <TimeSpan>
        <begin>2020-01-01T06:00:00Z</begin>
        <end>2020-01-03T00:00:00Z</end>
      </TimeSpan>
      <Point><coordinates>10,20,0</coordinates></Point>
    </Placemark>
    <Folder id="fHidden">
      <name>Hidden Folder</name>
      <visibility>0</visibility>
      <Placemark id="hiddenChild">
        <name>Hidden Child</name>
        <visibility>1</visibility>
        <Point><coordinates>11,21,0</coordinates></Point>
      </Placemark>
    </Folder>
  </Folder>
  <Placemark id="tsOnly">
    <name>TimeStamp Feature</name>
    <TimeStamp><when>2020-01-01T12:00:00Z</when></TimeStamp>
    <Point><coordinates>12,22,0</coordinates></Point>
  </Placemark>
</Document>
</kml>`,

  lines_and_polygons: `<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2">
<Document>
  <name>Lines And Polygons</name>
  <Style id="linePolyStyle">
    <LineStyle>
      <color>ff0000ff</color>
      <width>3.5</width>
    </LineStyle>
    <PolyStyle>
      <color>80ff0000</color>
      <fill>1</fill>
      <outline>1</outline>
    </PolyStyle>
  </Style>
  <Placemark id="lineAbs">
    <name>Absolute Line</name>
    <styleUrl>#linePolyStyle</styleUrl>
    <LineString>
      <altitudeMode>absolute</altitudeMode>
      <coordinates>-105,40,100 -104,41,200</coordinates>
    </LineString>
  </Placemark>
  <Placemark id="lineTess">
    <name>Tessellated Line</name>
    <styleUrl>#linePolyStyle</styleUrl>
    <LineString>
      <tessellate>1</tessellate>
      <gx:drawOrder>5</gx:drawOrder>
      <coordinates>-105,40,50 -104,40,50</coordinates>
    </LineString>
  </Placemark>
  <Placemark id="lineWall">
    <name>Extruded Wall</name>
    <styleUrl>#linePolyStyle</styleUrl>
    <LineString>
      <extrude>1</extrude>
      <altitudeMode>absolute</altitudeMode>
      <coordinates>-105,40,300 -104,40,400</coordinates>
    </LineString>
  </Placemark>
  <Placemark id="polyHole">
    <name>Polygon With Hole</name>
    <styleUrl>#linePolyStyle</styleUrl>
    <Polygon>
      <extrude>1</extrude>
      <altitudeMode>absolute</altitudeMode>
      <outerBoundaryIs>
        <LinearRing>
          <coordinates>-106,39,100 -104,39,100 -104,41,100 -106,41,100 -106,39,100</coordinates>
        </LinearRing>
      </outerBoundaryIs>
      <innerBoundaryIs>
        <LinearRing>
          <coordinates>-105.5,39.5,100 -104.5,39.5,100 -104.5,40.5,100 -105.5,40.5,100 -105.5,39.5,100</coordinates>
        </LinearRing>
      </innerBoundaryIs>
    </Polygon>
  </Placemark>
  <Placemark id="polyGround">
    <name>Ground Polygon</name>
    <styleUrl>#linePolyStyle</styleUrl>
    <Polygon>
      <outerBoundaryIs>
        <LinearRing>
          <coordinates>-102,38,0 -101,38,0 -101,39,0 -102,38,0</coordinates>
        </LinearRing>
      </outerBoundaryIs>
    </Polygon>
  </Placemark>
</Document>
</kml>`,

  multigeometry_and_tracks: `<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2">
<Document>
  <name>Multi And Tracks</name>
  <Style id="trackStyle">
    <IconStyle>
      <Icon><href>https://example.com/plane.png</href></Icon>
    </IconStyle>
    <LineStyle>
      <color>ff00ff00</color>
      <width>2</width>
    </LineStyle>
  </Style>
  <Placemark id="mg1">
    <name>Multi Feature</name>
    <styleUrl>#trackStyle</styleUrl>
    <MultiGeometry>
      <Point id="mgPt"><coordinates>-105,40,0</coordinates></Point>
      <LineString id="mgLine"><altitudeMode>absolute</altitudeMode><coordinates>-105,40,10 -104,41,20</coordinates></LineString>
    </MultiGeometry>
  </Placemark>
  <Placemark id="trk1">
    <name>Single Track</name>
    <styleUrl>#trackStyle</styleUrl>
    <gx:Track>
      <altitudeMode>absolute</altitudeMode>
      <when>2020-01-01T00:00:00Z</when>
      <when>2020-01-01T00:01:00Z</when>
      <gx:coord>-105.0 40.0 100</gx:coord>
      <gx:coord>-104.5 40.5 200</gx:coord>
    </gx:Track>
  </Placemark>
  <Placemark id="mtrk1">
    <name>Multi Track</name>
    <styleUrl>#trackStyle</styleUrl>
    <gx:MultiTrack>
      <gx:interpolate>1</gx:interpolate>
      <gx:Track>
        <altitudeMode>absolute</altitudeMode>
        <when>2020-01-01T00:00:00Z</when>
        <when>2020-01-01T00:01:00Z</when>
        <gx:coord>-105.0 40.0 100</gx:coord>
        <gx:coord>-104.0 40.0 100</gx:coord>
      </gx:Track>
      <gx:Track>
        <altitudeMode>absolute</altitudeMode>
        <when>2020-01-01T00:03:00Z</when>
        <when>2020-01-01T00:04:00Z</when>
        <gx:coord>-103.0 40.0 100</gx:coord>
        <gx:coord>-102.0 40.0 100</gx:coord>
      </gx:Track>
    </gx:MultiTrack>
  </Placemark>
</Document>
</kml>`,

  ground_overlays: `<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2">
<Document>
  <name>Overlays</name>
  <GroundOverlay id="goBox">
    <name>Box Overlay</name>
    <color>ccffffff</color>
    <drawOrder>3</drawOrder>
    <Icon>
      <href>https://example.com/overlay.png</href>
    </Icon>
    <LatLonBox>
      <north>41.0</north>
      <south>39.0</south>
      <east>-104.0</east>
      <west>-106.0</west>
      <rotation>15.0</rotation>
    </LatLonBox>
  </GroundOverlay>
  <GroundOverlay id="goAbs">
    <name>Elevated Overlay</name>
    <color>ff00ff00</color>
    <drawOrder>7</drawOrder>
    <altitude>500</altitude>
    <altitudeMode>absolute</altitudeMode>
    <LatLonBox>
      <north>36.0</north>
      <south>35.0</south>
      <east>-100.0</east>
      <west>-101.0</west>
    </LatLonBox>
  </GroundOverlay>
  <GroundOverlay id="goQuad">
    <name>Quad Overlay</name>
    <color>800000ff</color>
    <drawOrder>2</drawOrder>
    <gx:LatLonQuad>
      <coordinates>-105,40 -104,40 -104,41 -105,41</coordinates>
    </gx:LatLonQuad>
  </GroundOverlay>
</Document>
</kml>`,

  tours_and_unsupported: `<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2" xmlns:gx="http://www.google.com/kml/ext/2.2">
<Document>
  <name>Tours</name>
  <gx:Tour id="tour1">
    <name>Scenic Tour</name>
    <gx:Playlist>
      <gx:FlyTo>
        <gx:duration>4.5</gx:duration>
        <gx:flyToMode>smooth</gx:flyToMode>
        <LookAt>
          <longitude>-105.0</longitude>
          <latitude>40.0</latitude>
          <altitude>100</altitude>
          <heading>45</heading>
          <tilt>60</tilt>
          <range>2500</range>
        </LookAt>
      </gx:FlyTo>
      <gx:Wait>
        <gx:duration>2.0</gx:duration>
      </gx:Wait>
      <gx:FlyTo>
        <gx:duration>3.0</gx:duration>
        <gx:flyToMode>bounce</gx:flyToMode>
        <Camera>
          <longitude>-104.0</longitude>
          <latitude>39.0</latitude>
          <altitude>1500</altitude>
          <heading>180</heading>
          <tilt>30</tilt>
          <roll>5</roll>
        </Camera>
      </gx:FlyTo>
    </gx:Playlist>
  </gx:Tour>
  <PhotoOverlay id="photo1">
    <name>Unsupported Photo</name>
  </PhotoOverlay>
</Document>
</kml>`,
};

// ---------------------------------------------------------------------------
// Snapshot encoding for KmlDataSource
// ---------------------------------------------------------------------------

function encPropVal(v) {
  if (v === undefined || v === null) return null;
  if (v._isFakeCanvas) return "<pin>";
  if (v instanceof C.Resource) return v.url;
  if (v instanceof C.PolygonHierarchy) {
    return {
      positions: v.positions.map(enc),
      holes: v.holes.map(encPropVal),
    };
  }
  if (v instanceof C.NearFarScalar) {
    return { near: v.near, nearValue: v.nearValue, far: v.far, farValue: v.farValue };
  }
  if (v instanceof C.BoundingRectangle) {
    return { x: v.x, y: v.y, width: v.width, height: v.height };
  }
  if (v instanceof C.Rectangle) {
    return { west: v.west, south: v.south, east: v.east, north: v.north };
  }
  return enc(v);
}

function prop(p, t) {
  return p ? encPropVal(p.getValue(t)) : null;
}

function encodeMaterial(mat, t) {
  if (!mat) return null;
  const type = mat.getType(t);
  const val = mat.getValue(t);
  if (type === "Color") {
    return { type, color: enc(val.color) };
  }
  if (type === "Image") {
    return {
      type,
      image: val.image instanceof C.Resource ? val.image.url : enc(val.image),
      color: enc(val.color),
    };
  }
  return { type };
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

function encodeKmlData(k) {
  if (!k) return null;
  let ext = null;
  if (k.extendedData) {
    ext = {};
    for (const [key, val] of Object.entries(k.extendedData)) {
      ext[key] = { displayName: enc(val.displayName), value: enc(val.value) };
    }
  }
  return {
    author: { name: enc(k.author?.name), uri: enc(k.author?.uri), email: enc(k.author?.email) },
    link: {
      href: enc(k.link?.href),
      hreflang: enc(k.link?.hreflang),
      rel: enc(k.link?.rel),
      type: enc(k.link?.type),
      title: enc(k.link?.title),
      length: enc(k.link?.length),
    },
    address: enc(k.address),
    phoneNumber: enc(k.phoneNumber),
    snippet: enc(k.snippet),
    extendedData: ext,
    camera: k.camera
      ? { position: enc(k.camera.position), headingPitchRoll: enc(k.camera.headingPitchRoll) }
      : null,
    lookAt: k.lookAt
      ? {
          position: enc(k.lookAt.position),
          headingPitchRange: [
            k.lookAt.headingPitchRange.heading,
            k.lookAt.headingPitchRange.pitch,
            k.lookAt.headingPitchRange.range,
          ],
        }
      : null,
  };
}

function snapshotEntity(e) {
  const t = e.availability && e.availability.start ? e.availability.start : FIXED;
  const p = (x) => prop(x, t);
  const avail = e.availability
    ? [
        e.availability.start ? C.JulianDate.toIso8601(e.availability.start) : null,
        e.availability.stop ? C.JulianDate.toIso8601(e.availability.stop) : null,
      ]
    : null;
  return {
    id: e.id,
    name: enc(e.name),
    show: e.show,
    parent: e.parent ? e.parent.id : null,
    description: p(e.description),
    availability: avail,
    position: p(e.position),
    kml: encodeKmlData(e.kml),
    billboard: e.billboard
      ? {
          image: p(e.billboard.image),
          scale: p(e.billboard.scale),
          color: p(e.billboard.color),
          width: p(e.billboard.width),
          height: p(e.billboard.height),
          rotation: p(e.billboard.rotation),
          alignedAxis: p(e.billboard.alignedAxis),
          pixelOffset: p(e.billboard.pixelOffset),
          imageSubRegion: p(e.billboard.imageSubRegion),
          heightReference: p(e.billboard.heightReference),
        }
      : null,
    label: e.label
      ? {
          text: p(e.label.text),
          scale: p(e.label.scale),
          fillColor: p(e.label.fillColor),
          pixelOffset: p(e.label.pixelOffset),
          horizontalOrigin: p(e.label.horizontalOrigin),
          heightReference: p(e.label.heightReference),
        }
      : null,
    polyline: e.polyline
      ? {
          width: p(e.polyline.width),
          material: encodeMaterial(e.polyline.material, t),
          clampToGround: p(e.polyline.clampToGround),
          arcType: p(e.polyline.arcType),
          zIndex: p(e.polyline.zIndex),
          positions: p(e.polyline.positions),
        }
      : null,
    wall: e.wall
      ? {
          positions: p(e.wall.positions),
          fill: p(e.wall.fill),
          material: encodeMaterial(e.wall.material, t),
          outline: p(e.wall.outline),
          outlineColor: p(e.wall.outlineColor),
          outlineWidth: p(e.wall.outlineWidth),
        }
      : null,
    polygon: e.polygon
      ? {
          hierarchy: p(e.polygon.hierarchy),
          height: p(e.polygon.height),
          perPositionHeight: p(e.polygon.perPositionHeight),
          extrudedHeight: p(e.polygon.extrudedHeight),
          fill: p(e.polygon.fill),
          material: encodeMaterial(e.polygon.material, t),
          outline: p(e.polygon.outline),
          outlineColor: p(e.polygon.outlineColor),
          outlineWidth: p(e.polygon.outlineWidth),
          zIndex: p(e.polygon.zIndex),
        }
      : null,
    rectangle: e.rectangle
      ? {
          coordinates: p(e.rectangle.coordinates),
          height: p(e.rectangle.height),
          rotation: p(e.rectangle.rotation),
          stRotation: p(e.rectangle.stRotation),
          material: encodeMaterial(e.rectangle.material, t),
          zIndex: p(e.rectangle.zIndex),
        }
      : null,
    path: e.path
      ? {
          leadTime: p(e.path.leadTime),
          width: p(e.path.width),
          material: encodeMaterial(e.path.material, t),
        }
      : null,
  };
}

function encodeTour(tour) {
  return {
    id: enc(tour.id),
    name: enc(tour.name),
    playlist: tour.playlist.map((entry) => {
      if (entry.type === "KmlTourWait") {
        return { type: "KmlTourWait", duration: entry.duration };
      }
      if (entry.type === "KmlTourFlyTo") {
        const view =
          entry.view instanceof C.KmlCamera
            ? {
                kind: "camera",
                position: enc(entry.view.position),
                headingPitchRoll: enc(entry.view.headingPitchRoll),
              }
            : {
                kind: "lookAt",
                position: enc(entry.view.position),
                headingPitchRange: [
                  entry.view.headingPitchRange.heading,
                  entry.view.headingPitchRange.pitch,
                  entry.view.headingPitchRange.range,
                ],
              };
        return {
          type: "KmlTourFlyTo",
          duration: entry.duration,
          flyToMode: enc(entry.flyToMode),
          view,
        };
      }
      return { type: entry.type };
    }),
  };
}

async function kmlCase(xml, options, includeClock = true) {
  const ds = new C.KmlDataSource();
  const unsupported = [];
  const trace = { changed: 0, loading: [], errorCount: 0 };
  ds.changedEvent.addEventListener(() => trace.changed++);
  ds.loadingEvent.addEventListener((_, v) => trace.loading.push(v));
  ds.errorEvent.addEventListener(() => trace.errorCount++);
  ds.unsupportedNodeEvent.addEventListener((_, __, node) => unsupported.push(node.localName));
  let error = false;
  try {
    const doc = new DOMParser().parseFromString(xml, "application/xml");
    await ds.load(doc, options);
  } catch (e) {
    error = true;
  }
  // Check a midpoint position sample on MultiTrack if present.
  const midTime = C.JulianDate.fromIso8601("2020-01-01T00:02:00Z");
  const multiTrack = ds.entities.getById("mtrk1");
  const multiTrackMid = multiTrack && multiTrack.position ? enc(multiTrack.position.getValue(midTime)) : null;

  return {
    name: enc(ds.name),
    clock: includeClock ? encodeClock(ds.clock) : null,
    isLoading: ds.isLoading,
    entities: ds.entities.values.map(snapshotEntity),
    tours: ds.kmlTours.map(encodeTour),
    unsupported,
    multiTrackMid,
    trace,
    error,
  };
}

async function kmlDataSourceCases() {
  const cases = [];
  cases.push({
    f: "load",
    a: ["placemark_point_styled", false],
    r: await kmlCase(KML_DOCS.placemark_point_styled, { clampToGround: false }),
  });
  cases.push({
    f: "load",
    a: ["placemark_point_styled", true],
    r: await kmlCase(KML_DOCS.placemark_point_styled, { clampToGround: true }),
  });
  cases.push({
    f: "load",
    a: ["placemark_default_and_empty_icon", false],
    r: await kmlCase(KML_DOCS.placemark_default_and_empty_icon, { clampToGround: false }),
  });
  cases.push({
    f: "load",
    a: ["folder_hierarchy_and_styles", false],
    // `tsOnly` has an open-ended TimeStamp -> stop defaults to local midnight tonight,
    // so skip wall-clock stop comparison for this specific case (covered with explicit clock in tracks).
    r: await kmlCase(KML_DOCS.folder_hierarchy_and_styles, { clampToGround: false }, false),
  });
  cases.push({
    f: "load",
    a: ["lines_and_polygons", false],
    r: await kmlCase(KML_DOCS.lines_and_polygons, { clampToGround: false }),
  });
  cases.push({
    f: "load",
    a: ["lines_and_polygons", true],
    r: await kmlCase(KML_DOCS.lines_and_polygons, { clampToGround: true }),
  });
  cases.push({
    f: "load",
    a: ["multigeometry_and_tracks", false],
    r: await kmlCase(KML_DOCS.multigeometry_and_tracks, { clampToGround: false }),
  });
  cases.push({
    f: "load",
    a: ["ground_overlays", false],
    r: await kmlCase(KML_DOCS.ground_overlays, { clampToGround: false }),
  });
  cases.push({
    f: "load",
    a: ["tours_and_unsupported", false],
    r: await kmlCase(KML_DOCS.tours_and_unsupported, { clampToGround: false }),
  });
  return cases;
}

// ---------------------------------------------------------------------------
// exportKml fixture cases
// ---------------------------------------------------------------------------

async function exportCase(buildEntities, extraOptions = {}) {
  const entities = new C.EntityCollection();
  buildEntities(entities);
  try {
    const res = await C.exportKml({
      entities,
      time: FIXED,
      ...extraOptions,
    });
    const externalFiles = {};
    for (const [k, blob] of Object.entries(res.externalFiles ?? {})) {
      const buf = new Uint8Array(await blob.arrayBuffer());
      externalFiles[k] = Array.from(buf);
    }
    return { kml: res.kml, externalFiles };
  } catch (e) {
    return { throws: String(e.message ?? e), kind: e.name ?? "Error" };
  }
}

async function exportKmlCases() {
  const cases = [];

  cases.push({
    f: "export",
    a: ["point_and_billboard"],
    r: await exportCase((entities) => {
      entities.add({
        id: "pt1",
        name: "Point One",
        description: "A point",
        position: C.Cartesian3.fromDegrees(-105.0, 40.0, 100.0),
        point: {
          color: C.Color.RED,
          pixelSize: 16,
          heightReference: C.HeightReference.RELATIVE_TO_GROUND,
        },
        label: {
          text: "Point Label",
          fillColor: C.Color.BLUE,
          scale: 1.5,
        },
      });
      entities.add({
        id: "bb1",
        name: "Billboard One",
        position: C.Cartesian3.fromDegrees(-104.0, 39.0, 0.0),
        billboard: {
          image: "https://example.com/pin.png",
          imageSubRegion: new C.BoundingRectangle(0, 0, 16, 16),
          color: C.Color.LIME,
          scale: 2.0,
          width: 32,
          height: 32,
          pixelOffset: new C.Cartesian2(4, -8),
          horizontalOrigin: C.HorizontalOrigin.CENTER,
          verticalOrigin: C.VerticalOrigin.BOTTOM,
          rotation: C.Math.toRadians(-90),
          alignedAxis: C.Cartesian3.UNIT_Z,
          heightReference: C.HeightReference.CLAMP_TO_GROUND,
        },
      });
      // Identical style to pt1 (without label) to verify style caching/dedup.
      entities.add({
        id: "pt2",
        name: "Point Two",
        position: C.Cartesian3.fromDegrees(-103.0, 38.0, 50.0),
        point: {
          color: C.Color.RED,
          pixelSize: 16,
          heightReference: C.HeightReference.RELATIVE_TO_GROUND,
        },
        label: {
          text: "Point Label",
          fillColor: C.Color.BLUE,
          scale: 1.5,
        },
      });
    }),
  });

  cases.push({
    f: "export",
    a: ["polyline_and_materials"],
    r: await exportCase((entities) => {
      entities.add({
        id: "line1",
        name: "Clamped Line",
        polyline: {
          positions: [
            C.Cartesian3.fromDegrees(-105.0, 40.0, 0.0),
            C.Cartesian3.fromDegrees(-104.0, 41.0, 0.0),
          ],
          clampToGround: true,
          zIndex: 4,
          width: 3,
          material: C.Color.YELLOW,
        },
      });
      entities.add({
        id: "line2",
        name: "Outline Line",
        polyline: {
          positions: [
            C.Cartesian3.fromDegrees(-105.0, 40.0, 200.0),
            C.Cartesian3.fromDegrees(-104.0, 41.0, 300.0),
          ],
          width: 5,
          material: new C.PolylineOutlineMaterialProperty({
            color: C.Color.CYAN,
            outlineColor: C.Color.BLACK,
            outlineWidth: 2,
          }),
        },
      });
      entities.add({
        id: "line3",
        name: "Stripe Line",
        polyline: {
          positions: [
            C.Cartesian3.fromDegrees(-106.0, 40.0, 100.0),
            C.Cartesian3.fromDegrees(-105.0, 40.0, 100.0),
          ],
          width: 2,
          material: new C.StripeMaterialProperty({
            evenColor: C.Color.WHITE,
            oddColor: C.Color.MAGENTA,
          }),
        },
      });
    }),
  });

  cases.push({
    f: "export",
    a: ["polygon_and_rectangle"],
    r: await exportCase((entities) => {
      entities.add({
        id: "poly1",
        name: "Extruded Polygon",
        polygon: {
          hierarchy: new C.PolygonHierarchy(
            [
              C.Cartesian3.fromDegrees(-106.0, 39.0, 0.0),
              C.Cartesian3.fromDegrees(-104.0, 39.0, 0.0),
              C.Cartesian3.fromDegrees(-104.0, 41.0, 0.0),
              C.Cartesian3.fromDegrees(-106.0, 41.0, 0.0),
            ],
            [
              new C.PolygonHierarchy([
                C.Cartesian3.fromDegrees(-105.5, 39.5, 0.0),
                C.Cartesian3.fromDegrees(-104.5, 39.5, 0.0),
                C.Cartesian3.fromDegrees(-104.5, 40.5, 0.0),
                C.Cartesian3.fromDegrees(-105.5, 40.5, 0.0),
              ]),
            ],
          ),
          extrudedHeight: 500,
          fill: true,
          material: C.Color.RED.withAlpha(0.5),
          outline: true,
          outlineColor: C.Color.WHITE,
          outlineWidth: 2,
          heightReference: C.HeightReference.RELATIVE_TO_GROUND,
        },
      });
      entities.add({
        id: "rectPoly",
        name: "Color Rectangle",
        rectangle: {
          coordinates: C.Rectangle.fromDegrees(-102.0, 38.0, -101.0, 39.0),
          height: 150,
          fill: true,
          material: C.Color.BLUE,
        },
      });
      entities.add({
        id: "rectOverlay",
        name: "Image Overlay",
        rectangle: {
          coordinates: C.Rectangle.fromDegrees(-100.0, 35.0, -99.0, 36.0),
          height: 250,
          material: new C.ImageMaterialProperty({
            image: "https://example.com/ground.png",
            color: C.Color.WHITE.withAlpha(0.5),
          }),
        },
      });
    }),
  });

  cases.push({
    f: "export",
    a: ["hierarchy_and_availability"],
    r: await exportCase((entities) => {
      const parent = entities.add({
        id: "parent1",
        name: "Parent Group",
        show: false,
        description: "Group desc",
        position: C.Cartesian3.fromDegrees(-105.0, 40.0, 0.0),
        point: { color: C.Color.WHITE, pixelSize: 32 },
      });
      const avail = new C.TimeIntervalCollection([
        new C.TimeInterval({
          start: C.JulianDate.fromIso8601("2020-01-01T00:00:00Z"),
          stop: C.JulianDate.fromIso8601("2020-01-01T01:00:00Z"),
        }),
      ]);
      entities.add({
        id: "child1",
        name: "Child Point",
        parent,
        availability: avail,
        position: C.Cartesian3.fromDegrees(-104.0, 40.0, 10.0),
        point: { color: C.Color.GREEN, pixelSize: 16 },
      });
    }),
  });

  cases.push({
    f: "export",
    a: ["track_and_multitrack"],
    r: await exportCase((entities) => {
      const t0 = C.JulianDate.fromIso8601("2020-01-01T00:00:00Z");
      const t1 = C.JulianDate.fromIso8601("2020-01-01T00:01:00Z");
      const t2 = C.JulianDate.fromIso8601("2020-01-01T00:02:00Z");
      const t3 = C.JulianDate.fromIso8601("2020-01-01T00:03:00Z");

      const sampled = new C.SampledPositionProperty();
      sampled.addSample(t0, C.Cartesian3.fromDegrees(-105.0, 40.0, 100.0));
      sampled.addSample(t1, C.Cartesian3.fromDegrees(-104.0, 41.0, 200.0));
      entities.add({
        id: "track1",
        name: "Sampled Track",
        position: sampled,
        availability: new C.TimeIntervalCollection([new C.TimeInterval({ start: t0, stop: t1 })]),
        billboard: { image: "https://example.com/ship.png" },
        path: { width: 3, material: C.Color.ORANGE },
      });

      const sA = new C.SampledPositionProperty();
      sA.addSample(t0, C.Cartesian3.fromDegrees(-105.0, 40.0, 50.0));
      sA.addSample(t1, C.Cartesian3.fromDegrees(-104.0, 40.0, 50.0));
      const sB = new C.SampledPositionProperty();
      sB.addSample(t2, C.Cartesian3.fromDegrees(-103.0, 40.0, 50.0));
      sB.addSample(t3, C.Cartesian3.fromDegrees(-102.0, 40.0, 50.0));
      const comp = new C.CompositePositionProperty();
      comp.intervals.addInterval(new C.TimeInterval({ start: t0, stop: t1, data: sA }));
      comp.intervals.addInterval(new C.TimeInterval({ start: t2, stop: t3, data: sB }));
      entities.add({
        id: "mtrack1",
        name: "Multi Track",
        position: comp,
        path: { width: 2, material: C.Color.RED },
      });
    }),
  });

  cases.push({
    f: "export",
    a: ["model_export"],
    r: await exportCase(
      (entities) => {
        entities.add({
          id: "mdl1",
          name: "Static Model",
          position: C.Cartesian3.fromDegrees(-105.0, 40.0, 250.0),
          model: {
            uri: "models/drone.glb",
            scale: 2.5,
            heightReference: C.HeightReference.RELATIVE_TO_GROUND,
          },
        });
      },
      {
        modelCallback(model, _time, externalFiles) {
          externalFiles["models/drone.dae"] = new Blob([new Uint8Array([68, 65, 69])]);
          return "models/drone.dae";
        },
      },
    ),
  });

  cases.push({
    f: "export",
    a: ["model_missing_callback"],
    r: await exportCase((entities) => {
      entities.add({
        id: "mdlErr",
        name: "No Callback Model",
        position: C.Cartesian3.fromDegrees(-105.0, 40.0, 0.0),
        model: { uri: "models/drone.glb" },
      });
    }),
  });

  cases.push({
    f: "export",
    a: ["data_uri_texture"],
    r: await exportCase((entities) => {
      entities.add({
        id: "tex1",
        name: "Inline Texture",
        position: C.Cartesian3.fromDegrees(-105.0, 40.0, 0.0),
        billboard: {
          image: "data:image/png;base64,iVBORw0KGgo=",
        },
      });
    }),
  });

  return cases;
}

export default async function dataSourcesKml() {
  const restoreDom = setupNodeDom();
  try {
    return [
      { module: "KmlDataSource", cases: await kmlDataSourceCases() },
      { module: "exportKml", cases: await exportKmlCases() },
    ];
  } finally {
    restoreDom();
  }
}

// Allow direct invocation: `TZ=UTC node tools/cesium/fixtures/data-sources-kml.mjs`
const selfPath = fileURLToPath(import.meta.url);
if (process.argv[1] && fileURLToPath(`file://${process.argv[1]}`) === selfPath) {
  const here = dirname(selfPath);
  const outDir = join(here, "../../../tests/fixtures/data_sources");
  const version = JSON.parse(readFileSync(join(here, "../node_modules/@cesium/engine/package.json"), "utf8")).version;
  const suites = await dataSourcesKml();
  mkdirSync(outDir, { recursive: true });
  for (const s of suites) {
    const file = join(outDir, `${s.module}.json`);
    writeFileSync(file, JSON.stringify({ module: s.module, cesium: version, cases: s.cases }) + "\n");
    console.log(`data_sources/${s.module}: ${s.cases.length} cases`);
  }
}
