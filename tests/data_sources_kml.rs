mod common;
mod entity_common;

use cesium_rs::{
    core::{
        resource::{HttpRequest, HttpResponse, ResourceTransport},
        *,
    },
    data_sources::{kml_data_source::zip, *},
    scene::{Camera, CameraOptions, HeightReference, HorizontalOrigin, VerticalOrigin},
};
use common::{ok, run, TIGHT};
use entity_common::date;
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

// ---------------------------------------------------------------------------
// KML Fixture Documents (matching tools/cesium/fixtures/data-sources-kml.mjs)
// ---------------------------------------------------------------------------

const PLACEMARK_POINT_STYLED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
</kml>"#;

const PLACEMARK_DEFAULT_AND_EMPTY_ICON: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
</kml>"#;

const FOLDER_HIERARCHY_AND_STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
</kml>"#;

const LINES_AND_POLYGONS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
</kml>"#;

const MULTIGEOMETRY_AND_TRACKS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
</kml>"#;

const GROUND_OVERLAYS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
</kml>"#;

const TOURS_AND_UNSUPPORTED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
</kml>"#;

// ---------------------------------------------------------------------------
// Snapshot helpers matching tools/cesium/fixtures/data-sources-kml.mjs
// ---------------------------------------------------------------------------

fn eval_prop(input: Option<PropertyInput>, time: &JulianDate) -> Value {
    let val = match input {
        None => return Value::Null,
        Some(PropertyInput::Value(v)) => v,
        Some(PropertyInput::Component(c)) => match c.get_value(time, None).unwrap() {
            Some(v) => v,
            None => return Value::Null,
        },
    };
    match val {
        DynamicValue::Pin(_) => json!("<pin>"),
        other => other.to_json(),
    }
}

fn encode_material(mat: Option<PropertyInput>, time: &JulianDate) -> Value {
    let Some(PropertyInput::Component(c)) = mat else {
        return Value::Null;
    };
    let kind = c.get_type(time).unwrap().unwrap_or_default();
    let val = c.get_value(time, None).unwrap().unwrap_or(DynamicValue::Null);
    match kind.as_str() {
        "Color" => json!({ "type": kind, "color": val.get("color").map_or(Value::Null, |v| v.to_json()) }),
        "Image" => json!({
            "type": kind,
            "image": val.get("image").map_or(Value::Null, |v| v.to_json()),
            "color": val.get("color").map_or(Value::Null, |v| v.to_json()),
        }),
        _ => json!({ "type": kind }),
    }
}

fn encode_clock(clock: Option<Rc<DataSourceClock>>) -> Value {
    match clock {
        None => Value::Null,
        Some(c) => json!({
            "start": c.start_time().map(|t| t.to_iso8601(None)),
            "stop": c.stop_time().map(|t| t.to_iso8601(None)),
            "current": c.current_time().map(|t| t.to_iso8601(None)),
            "range": c.clock_range().map(|r| r as i32),
            "step": c.clock_step().map(|s| s as i32),
            "multiplier": c.multiplier(),
        }),
    }
}

fn snapshot_entity(e: &EntityRef) -> Value {
    let fixed = date(0.0);
    let t = e.availability().and_then(|a| a.read().start()).unwrap_or(fixed);
    let availability = e.availability().map(|a| {
        let r = a.read();
        json!([r.start().map(|d| d.to_iso8601(None)), r.stop().map(|d| d.to_iso8601(None))])
    });
    let position = e
        .position()
        .and_then(|p| p.get_value(Some(&t), None).unwrap())
        .map(|p| json!(p.to_array()))
        .unwrap_or(Value::Null);
    let kml = KmlDataSource::with_kml_data(e, KmlFeatureData::to_json).unwrap_or(Value::Null);

    let billboard = match e.get_property("billboard") {
        Some(PropertyInput::Component(c)) => {
            let b = c.as_any().downcast_ref::<BillboardGraphics>().unwrap();
            json!({
                "image": eval_prop(b.image(), &t),
                "scale": eval_prop(b.scale(), &t),
                "color": eval_prop(b.color(), &t),
                "width": eval_prop(b.width(), &t),
                "height": eval_prop(b.height(), &t),
                "rotation": eval_prop(b.rotation(), &t),
                "alignedAxis": eval_prop(b.aligned_axis(), &t),
                "pixelOffset": eval_prop(b.pixel_offset(), &t),
                "imageSubRegion": eval_prop(b.image_sub_region(), &t),
                "heightReference": eval_prop(b.height_reference(), &t),
            })
        }
        _ => Value::Null,
    };

    let label = match e.get_property("label") {
        Some(PropertyInput::Component(c)) => {
            let l = c.as_any().downcast_ref::<LabelGraphics>().unwrap();
            json!({
                "text": eval_prop(l.text(), &t),
                "scale": eval_prop(l.scale(), &t),
                "fillColor": eval_prop(l.fill_color(), &t),
                "pixelOffset": eval_prop(l.pixel_offset(), &t),
                "horizontalOrigin": eval_prop(l.horizontal_origin(), &t),
                "heightReference": eval_prop(l.height_reference(), &t),
            })
        }
        _ => Value::Null,
    };

    let polyline = match e.get_property("polyline") {
        Some(PropertyInput::Component(c)) => {
            let pl = c.as_any().downcast_ref::<PolylineGraphics>().unwrap();
            json!({
                "width": eval_prop(pl.width(), &t),
                "material": encode_material(pl.material(), &t),
                "clampToGround": eval_prop(pl.clamp_to_ground(), &t),
                "arcType": eval_prop(pl.arc_type(), &t),
                "zIndex": eval_prop(pl.z_index(), &t),
                "positions": eval_prop(pl.positions(), &t),
            })
        }
        _ => Value::Null,
    };

    let wall = match e.get_property("wall") {
        Some(PropertyInput::Component(c)) => {
            let w = c.as_any().downcast_ref::<WallGraphics>().unwrap();
            json!({
                "positions": eval_prop(w.positions(), &t),
                "fill": eval_prop(w.fill(), &t),
                "material": encode_material(w.material(), &t),
                "outline": eval_prop(w.outline(), &t),
                "outlineColor": eval_prop(w.outline_color(), &t),
                "outlineWidth": eval_prop(w.outline_width(), &t),
            })
        }
        _ => Value::Null,
    };

    let polygon = match e.get_property("polygon") {
        Some(PropertyInput::Component(c)) => {
            let pg = c.as_any().downcast_ref::<PolygonGraphics>().unwrap();
            json!({
                "hierarchy": eval_prop(pg.hierarchy(), &t),
                "height": eval_prop(pg.height(), &t),
                "perPositionHeight": eval_prop(pg.per_position_height(), &t),
                "extrudedHeight": eval_prop(pg.extruded_height(), &t),
                "fill": eval_prop(pg.fill(), &t),
                "material": encode_material(pg.material(), &t),
                "outline": eval_prop(pg.outline(), &t),
                "outlineColor": eval_prop(pg.outline_color(), &t),
                "outlineWidth": eval_prop(pg.outline_width(), &t),
                "zIndex": eval_prop(pg.z_index(), &t),
            })
        }
        _ => Value::Null,
    };

    let rectangle = match e.get_property("rectangle") {
        Some(PropertyInput::Component(c)) => {
            let r = c.as_any().downcast_ref::<RectangleGraphics>().unwrap();
            json!({
                "coordinates": eval_prop(r.coordinates(), &t),
                "height": eval_prop(r.height(), &t),
                "rotation": eval_prop(r.rotation(), &t),
                "stRotation": eval_prop(r.st_rotation(), &t),
                "material": encode_material(r.material(), &t),
                "zIndex": eval_prop(r.z_index(), &t),
            })
        }
        _ => Value::Null,
    };

    let path = match e.get_property("path") {
        Some(PropertyInput::Component(c)) => {
            let pa = c.as_any().downcast_ref::<PathGraphics>().unwrap();
            json!({
                "leadTime": eval_prop(pa.lead_time(), &t),
                "width": eval_prop(pa.width(), &t),
                "material": encode_material(pa.material(), &t),
            })
        }
        _ => Value::Null,
    };

    json!({
        "id": e.id(),
        "name": e.name(),
        "show": e.show(),
        "parent": e.parent().map(|p| p.id().to_owned()),
        "description": eval_prop(e.get_property("description"), &t),
        "availability": availability,
        "position": position,
        "kml": kml,
        "billboard": billboard,
        "label": label,
        "polyline": polyline,
        "wall": wall,
        "polygon": polygon,
        "rectangle": rectangle,
        "path": path,
    })
}

fn encode_tour(tour: &Rc<KmlTour>) -> Value {
    let entries: Vec<Value> = tour
        .playlist()
        .iter()
        .map(|entry| {
            if let Some(w) = entry.as_any().downcast_ref::<KmlTourWait>() {
                json!({ "type": "KmlTourWait", "duration": w.duration })
            } else if let Some(f) = entry.as_any().downcast_ref::<KmlTourFlyTo>() {
                let view = match f.view {
                    KmlTourView::Camera(c) => json!({
                        "kind": "camera",
                        "position": c.position.to_array(),
                        "headingPitchRoll": [c.heading_pitch_roll.heading, c.heading_pitch_roll.pitch, c.heading_pitch_roll.roll],
                    }),
                    KmlTourView::LookAt(l) => json!({
                        "kind": "lookAt",
                        "position": l.position.to_array(),
                        "headingPitchRange": [l.heading_pitch_range.heading, l.heading_pitch_range.pitch, l.heading_pitch_range.range],
                    }),
                };
                json!({
                    "type": "KmlTourFlyTo",
                    "duration": f.duration,
                    "flyToMode": f.fly_to_mode,
                    "view": view,
                })
            } else {
                json!({ "type": "unknown" })
            }
        })
        .collect();
    json!({
        "id": tour.id,
        "name": tour.name,
        "playlist": entries,
    })
}

// ---------------------------------------------------------------------------
// Fixture Replay: KmlDataSource
// ---------------------------------------------------------------------------

#[test]
fn replay_kml_data_source() {
    run(
        "data_sources",
        "KmlDataSource",
        |_| TIGHT,
        |_, args| {
            let kind = args[0].as_str().unwrap();
            let clamp_to_ground = args[1].as_bool().unwrap();
            let (xml, include_clock) = match kind {
                "placemark_point_styled" => (PLACEMARK_POINT_STYLED, true),
                "placemark_default_and_empty_icon" => (PLACEMARK_DEFAULT_AND_EMPTY_ICON, true),
                "folder_hierarchy_and_styles" => (FOLDER_HIERARCHY_AND_STYLES, false),
                "lines_and_polygons" => (LINES_AND_POLYGONS, true),
                "multigeometry_and_tracks" => (MULTIGEOMETRY_AND_TRACKS, true),
                "ground_overlays" => (GROUND_OVERLAYS, true),
                "tours_and_unsupported" => (TOURS_AND_UNSUPPORTED, true),
                other => panic!("unknown kml case {other}"),
            };

            let ds = KmlDataSource::new(KmlOptions::default());
            let changed = Rc::new(Cell::new(0usize));
            let loading = Rc::new(RefCell::new(Vec::<bool>::new()));
            let error_count = Rc::new(Cell::new(0usize));
            let unsupported = Rc::new(RefCell::new(Vec::<String>::new()));

            let c = changed.clone();
            let _s1 = ds.changed_event().add_listener(move |_| c.set(c.get() + 1));
            let l = loading.clone();
            let _s2 = ds.loading_event().add_listener(move |(_, v)| l.borrow_mut().push(*v));
            let e = error_count.clone();
            let _s3 = ds.error_event().add_listener(move |_| e.set(e.get() + 1));
            let u = unsupported.clone();
            let _s4 = ds.unsupported_node_event().add_listener(move |ev| u.borrow_mut().push(ev.node_name.clone()));

            let opts = KmlOptions { clamp_to_ground, ..Default::default() };
            let res = ds.load(xml, opts).now_or_never().unwrap();

            let mid_time = JulianDate::from_iso8601("2020-01-01T00:02:00Z").unwrap();
            let multi_track_mid = ds
                .entities()
                .get_by_id("mtrk1")
                .and_then(|ent| ent.position())
                .and_then(|pos| pos.get_value(Some(&mid_time), None).unwrap())
                .map(|p| json!(p.to_array()))
                .unwrap_or(Value::Null);

            let entities: Vec<Value> = ds.entities().values().iter().map(snapshot_entity).collect();
            let tours: Vec<Value> = ds.kml_tours().iter().map(encode_tour).collect();
            let loading_log = loading.borrow().clone();
            let unsupported_log = unsupported.borrow().clone();

            ok(json!({
                "name": ds.name(),
                "clock": if include_clock { encode_clock(ds.clock()) } else { Value::Null },
                "isLoading": ds.is_loading(),
                "entities": entities,
                "tours": tours,
                "unsupported": unsupported_log,
                "multiTrackMid": multi_track_mid,
                "trace": {
                    "changed": changed.get(),
                    "loading": loading_log,
                    "errorCount": error_count.get(),
                },
                "error": res.is_err(),
            }))
        },
    );
}

// ---------------------------------------------------------------------------
// Fixture Replay: exportKml
// ---------------------------------------------------------------------------

fn build_export_case(kind: &str) -> Result<ExportKmlResult, RuntimeError> {
    let entities = EntityCollection::new();
    let mut opts = ExportKmlOptions::new(entities.clone());
    opts.time = Some(date(0.0));

    match kind {
        "point_and_billboard" => {
            let pt1 = Entity::new(EntityOptions {
                id: Some("pt1".into()),
                name: Some("Point One".into()),
                position: Some(Rc::new(ConstantPositionProperty::new(
                    Some(Cartesian3::from_degrees(-105.0, 40.0, 100.0, &Ellipsoid::WGS84)),
                    ReferenceFrame::Fixed,
                ))),
                ..Default::default()
            })?;
            pt1.set_property("description", Some(PropertyInput::value("A point")))?;
            let pg = Rc::new(PointGraphics::new());
            pg.set_color(Some(PropertyInput::value(Color::RED)))?;
            pg.set_pixel_size(Some(PropertyInput::value(16.0)))?;
            pg.set_height_reference(Some(PropertyInput::value(HeightReference::RelativeToGround as i32 as f64)))?;
            pt1.set_property("point", Some(PropertyInput::Component(pg)))?;
            let lbl = Rc::new(LabelGraphics::new());
            lbl.set_text(Some(PropertyInput::value("Point Label")))?;
            lbl.set_fill_color(Some(PropertyInput::value(Color::BLUE)))?;
            lbl.set_scale(Some(PropertyInput::value(1.5)))?;
            pt1.set_property("label", Some(PropertyInput::Component(lbl)))?;
            entities.add(pt1)?;

            let bb1 = Entity::new(EntityOptions {
                id: Some("bb1".into()),
                name: Some("Billboard One".into()),
                position: Some(Rc::new(ConstantPositionProperty::new(
                    Some(Cartesian3::from_degrees(-104.0, 39.0, 0.0, &Ellipsoid::WGS84)),
                    ReferenceFrame::Fixed,
                ))),
                ..Default::default()
            })?;
            let bg = Rc::new(BillboardGraphics::new());
            bg.set_image(Some(PropertyInput::value("https://example.com/pin.png")))?;
            bg.set_image_sub_region(Some(PropertyInput::Value(DynamicValue::from_json(json!({
                "x": 0.0, "y": 0.0, "width": 16.0, "height": 16.0
            })))))?;
            bg.set_color(Some(PropertyInput::value(Color::LIME)))?;
            bg.set_scale(Some(PropertyInput::value(2.0)))?;
            bg.set_width(Some(PropertyInput::value(32.0)))?;
            bg.set_height(Some(PropertyInput::value(32.0)))?;
            bg.set_pixel_offset(Some(PropertyInput::value(Cartesian2::new(4.0, -8.0))))?;
            bg.set_horizontal_origin(Some(PropertyInput::value(HorizontalOrigin::Center as i32 as f64)))?;
            bg.set_vertical_origin(Some(PropertyInput::value(VerticalOrigin::Bottom as i32 as f64)))?;
            bg.set_rotation(Some(PropertyInput::value(math::to_radians(-90.0))))?;
            bg.set_aligned_axis(Some(PropertyInput::value(Cartesian3::UNIT_Z)))?;
            bg.set_height_reference(Some(PropertyInput::value(HeightReference::ClampToGround as i32 as f64)))?;
            bb1.set_property("billboard", Some(PropertyInput::Component(bg)))?;
            entities.add(bb1)?;

            let pt2 = Entity::new(EntityOptions {
                id: Some("pt2".into()),
                name: Some("Point Two".into()),
                position: Some(Rc::new(ConstantPositionProperty::new(
                    Some(Cartesian3::from_degrees(-103.0, 38.0, 50.0, &Ellipsoid::WGS84)),
                    ReferenceFrame::Fixed,
                ))),
                ..Default::default()
            })?;
            let pg2 = Rc::new(PointGraphics::new());
            pg2.set_color(Some(PropertyInput::value(Color::RED)))?;
            pg2.set_pixel_size(Some(PropertyInput::value(16.0)))?;
            pg2.set_height_reference(Some(PropertyInput::value(HeightReference::RelativeToGround as i32 as f64)))?;
            pt2.set_property("point", Some(PropertyInput::Component(pg2)))?;
            let lbl2 = Rc::new(LabelGraphics::new());
            lbl2.set_text(Some(PropertyInput::value("Point Label")))?;
            lbl2.set_fill_color(Some(PropertyInput::value(Color::BLUE)))?;
            lbl2.set_scale(Some(PropertyInput::value(1.5)))?;
            pt2.set_property("label", Some(PropertyInput::Component(lbl2)))?;
            entities.add(pt2)?;
        }
        "polyline_and_materials" => {
            let l1 = Entity::new(EntityOptions {
                id: Some("line1".into()),
                name: Some("Clamped Line".into()),
                ..Default::default()
            })?;
            let pl1 = Rc::new(PolylineGraphics::new());
            pl1.set_positions(Some(PropertyInput::Value(DynamicValue::Positions(Rc::new(vec![
                Cartesian3::from_degrees(-105.0, 40.0, 0.0, &Ellipsoid::WGS84),
                Cartesian3::from_degrees(-104.0, 41.0, 0.0, &Ellipsoid::WGS84),
            ])))))?;
            pl1.set_clamp_to_ground(Some(PropertyInput::value(true)))?;
            pl1.set_z_index(Some(PropertyInput::value(4.0)))?;
            pl1.set_width(Some(PropertyInput::value(3.0)))?;
            pl1.set_material(Some(PropertyInput::value(Color::YELLOW)))?;
            l1.set_property("polyline", Some(PropertyInput::Component(pl1)))?;
            entities.add(l1)?;

            let l2 = Entity::new(EntityOptions {
                id: Some("line2".into()),
                name: Some("Outline Line".into()),
                ..Default::default()
            })?;
            let pl2 = Rc::new(PolylineGraphics::new());
            pl2.set_positions(Some(PropertyInput::Value(DynamicValue::Positions(Rc::new(vec![
                Cartesian3::from_degrees(-105.0, 40.0, 200.0, &Ellipsoid::WGS84),
                Cartesian3::from_degrees(-104.0, 41.0, 300.0, &Ellipsoid::WGS84),
            ])))))?;
            pl2.set_width(Some(PropertyInput::value(5.0)))?;
            let mat2 = Rc::new(PolylineOutlineMaterialProperty::new());
            mat2.set_color(Some(PropertyInput::value(Color::CYAN)))?;
            mat2.set_outline_color(Some(PropertyInput::value(Color::BLACK)))?;
            mat2.set_outline_width(Some(PropertyInput::value(2.0)))?;
            pl2.set_material(Some(PropertyInput::Component(mat2)))?;
            l2.set_property("polyline", Some(PropertyInput::Component(pl2)))?;
            entities.add(l2)?;

            let l3 = Entity::new(EntityOptions {
                id: Some("line3".into()),
                name: Some("Stripe Line".into()),
                ..Default::default()
            })?;
            let pl3 = Rc::new(PolylineGraphics::new());
            pl3.set_positions(Some(PropertyInput::Value(DynamicValue::Positions(Rc::new(vec![
                Cartesian3::from_degrees(-106.0, 40.0, 100.0, &Ellipsoid::WGS84),
                Cartesian3::from_degrees(-105.0, 40.0, 100.0, &Ellipsoid::WGS84),
            ])))))?;
            pl3.set_width(Some(PropertyInput::value(2.0)))?;
            let mat3 = Rc::new(StripeMaterialProperty::new());
            mat3.set_even_color(Some(PropertyInput::value(Color::WHITE)))?;
            mat3.set_odd_color(Some(PropertyInput::value(Color::MAGENTA)))?;
            pl3.set_material(Some(PropertyInput::Component(mat3)))?;
            l3.set_property("polyline", Some(PropertyInput::Component(pl3)))?;
            entities.add(l3)?;
        }
        "polygon_and_rectangle" => {
            let p1 = Entity::new(EntityOptions {
                id: Some("poly1".into()),
                name: Some("Extruded Polygon".into()),
                ..Default::default()
            })?;
            let pg = Rc::new(PolygonGraphics::new());
            let hier = PolygonHierarchy::new(
                vec![
                    Cartesian3::from_degrees(-106.0, 39.0, 0.0, &Ellipsoid::WGS84),
                    Cartesian3::from_degrees(-104.0, 39.0, 0.0, &Ellipsoid::WGS84),
                    Cartesian3::from_degrees(-104.0, 41.0, 0.0, &Ellipsoid::WGS84),
                    Cartesian3::from_degrees(-106.0, 41.0, 0.0, &Ellipsoid::WGS84),
                ],
                vec![PolygonHierarchy::new(
                    vec![
                        Cartesian3::from_degrees(-105.5, 39.5, 0.0, &Ellipsoid::WGS84),
                        Cartesian3::from_degrees(-104.5, 39.5, 0.0, &Ellipsoid::WGS84),
                        Cartesian3::from_degrees(-104.5, 40.5, 0.0, &Ellipsoid::WGS84),
                        Cartesian3::from_degrees(-105.5, 40.5, 0.0, &Ellipsoid::WGS84),
                    ],
                    vec![],
                )],
            );
            pg.set_hierarchy(Some(PropertyInput::Value(DynamicValue::PolygonHierarchy(Rc::new(hier)))))?;
            pg.set_extruded_height(Some(PropertyInput::value(500.0)))?;
            pg.set_fill(Some(PropertyInput::value(true)))?;
            pg.set_material(Some(PropertyInput::value(Color::RED.with_alpha(0.5))))?;
            pg.set_outline(Some(PropertyInput::value(true)))?;
            pg.set_outline_color(Some(PropertyInput::value(Color::WHITE)))?;
            pg.set_outline_width(Some(PropertyInput::value(2.0)))?;
            pg.set_height_reference(Some(PropertyInput::value(HeightReference::RelativeToGround as i32 as f64)))?;
            p1.set_property("polygon", Some(PropertyInput::Component(pg)))?;
            entities.add(p1)?;

            let r1 = Entity::new(EntityOptions {
                id: Some("rectPoly".into()),
                name: Some("Color Rectangle".into()),
                ..Default::default()
            })?;
            let rg1 = Rc::new(RectangleGraphics::new());
            let rect1 = Rectangle::from_degrees(-102.0, 38.0, -101.0, 39.0);
            rg1.set_coordinates(Some(PropertyInput::Value(DynamicValue::from_json(json!({
                "west": rect1.west, "south": rect1.south, "east": rect1.east, "north": rect1.north
            })))))?;
            rg1.set_height(Some(PropertyInput::value(150.0)))?;
            rg1.set_fill(Some(PropertyInput::value(true)))?;
            rg1.set_material(Some(PropertyInput::value(Color::BLUE)))?;
            r1.set_property("rectangle", Some(PropertyInput::Component(rg1)))?;
            entities.add(r1)?;

            let r2 = Entity::new(EntityOptions {
                id: Some("rectOverlay".into()),
                name: Some("Image Overlay".into()),
                ..Default::default()
            })?;
            let rg2 = Rc::new(RectangleGraphics::new());
            let rect2 = Rectangle::from_degrees(-100.0, 35.0, -99.0, 36.0);
            rg2.set_coordinates(Some(PropertyInput::Value(DynamicValue::from_json(json!({
                "west": rect2.west, "south": rect2.south, "east": rect2.east, "north": rect2.north
            })))))?;
            rg2.set_height(Some(PropertyInput::value(250.0)))?;
            let img_mat = Rc::new(ImageMaterialProperty::new());
            img_mat.set_image(Some(PropertyInput::value("https://example.com/ground.png")))?;
            img_mat.set_color(Some(PropertyInput::value(Color::WHITE.with_alpha(0.5))))?;
            rg2.set_material(Some(PropertyInput::Component(img_mat)))?;
            r2.set_property("rectangle", Some(PropertyInput::Component(rg2)))?;
            entities.add(r2)?;
        }
        "hierarchy_and_availability" => {
            let parent = Entity::new(EntityOptions {
                id: Some("parent1".into()),
                name: Some("Parent Group".into()),
                show: false,
                position: Some(Rc::new(ConstantPositionProperty::new(
                    Some(Cartesian3::from_degrees(-105.0, 40.0, 0.0, &Ellipsoid::WGS84)),
                    ReferenceFrame::Fixed,
                ))),
                ..Default::default()
            })?;
            parent.set_property("description", Some(PropertyInput::value("Group desc")))?;
            let pg = Rc::new(PointGraphics::new());
            pg.set_color(Some(PropertyInput::value(Color::WHITE)))?;
            pg.set_pixel_size(Some(PropertyInput::value(32.0)))?;
            parent.set_property("point", Some(PropertyInput::Component(pg)))?;
            entities.add(parent.clone())?;

            let avail = Rc::new(PropertyIntervals::new(|a: &Option<()>, b: &Option<()>| a == b));
            avail.add_interval(TimeInterval::new(
                JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap(),
                JulianDate::from_iso8601("2020-01-01T01:00:00Z").unwrap(),
                true,
                true,
                None,
            ));
            let child = Entity::new(EntityOptions {
                id: Some("child1".into()),
                name: Some("Child Point".into()),
                parent: Some(parent),
                availability: Some(avail),
                position: Some(Rc::new(ConstantPositionProperty::new(
                    Some(Cartesian3::from_degrees(-104.0, 40.0, 10.0, &Ellipsoid::WGS84)),
                    ReferenceFrame::Fixed,
                ))),
                ..Default::default()
            })?;
            let pg2 = Rc::new(PointGraphics::new());
            pg2.set_color(Some(PropertyInput::value(Color::GREEN)))?;
            pg2.set_pixel_size(Some(PropertyInput::value(16.0)))?;
            child.set_property("point", Some(PropertyInput::Component(pg2)))?;
            entities.add(child)?;
        }
        "track_and_multitrack" => {
            let t0 = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
            let t1 = JulianDate::from_iso8601("2020-01-01T00:01:00Z").unwrap();
            let t2 = JulianDate::from_iso8601("2020-01-01T00:02:00Z").unwrap();
            let t3 = JulianDate::from_iso8601("2020-01-01T00:03:00Z").unwrap();

            let sampled = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
            sampled.add_sample(t0, &Cartesian3::from_degrees(-105.0, 40.0, 100.0, &Ellipsoid::WGS84), None)?;
            sampled.add_sample(t1, &Cartesian3::from_degrees(-104.0, 41.0, 200.0, &Ellipsoid::WGS84), None)?;
            let avail = Rc::new(PropertyIntervals::new(|a: &Option<()>, b: &Option<()>| a == b));
            avail.add_interval(TimeInterval::new(t0, t1, true, true, None));
            let trk = Entity::new(EntityOptions {
                id: Some("track1".into()),
                name: Some("Sampled Track".into()),
                position: Some(sampled),
                availability: Some(avail),
                ..Default::default()
            })?;
            let bg = Rc::new(BillboardGraphics::new());
            bg.set_image(Some(PropertyInput::value("https://example.com/ship.png")))?;
            trk.set_property("billboard", Some(PropertyInput::Component(bg)))?;
            let path = Rc::new(PathGraphics::new());
            path.set_width(Some(PropertyInput::value(3.0)))?;
            path.set_material(Some(PropertyInput::value(Color::ORANGE)))?;
            trk.set_property("path", Some(PropertyInput::Component(path)))?;
            entities.add(trk)?;

            let s_a = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
            s_a.add_sample(t0, &Cartesian3::from_degrees(-105.0, 40.0, 50.0, &Ellipsoid::WGS84), None)?;
            s_a.add_sample(t1, &Cartesian3::from_degrees(-104.0, 40.0, 50.0, &Ellipsoid::WGS84), None)?;
            let s_b = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
            s_b.add_sample(t2, &Cartesian3::from_degrees(-103.0, 40.0, 50.0, &Ellipsoid::WGS84), None)?;
            s_b.add_sample(t3, &Cartesian3::from_degrees(-102.0, 40.0, 50.0, &Ellipsoid::WGS84), None)?;
            let comp = Rc::new(CompositePositionProperty::new(ReferenceFrame::Fixed));
            comp.intervals().add_interval(TimeInterval::new(t0, t1, true, true, Some(s_a as PositionPropertyRef)));
            comp.intervals().add_interval(TimeInterval::new(t2, t3, true, true, Some(s_b as PositionPropertyRef)));
            let mtrk = Entity::new(EntityOptions {
                id: Some("mtrack1".into()),
                name: Some("Multi Track".into()),
                position: Some(comp),
                ..Default::default()
            })?;
            let path2 = Rc::new(PathGraphics::new());
            path2.set_width(Some(PropertyInput::value(2.0)))?;
            path2.set_material(Some(PropertyInput::value(Color::RED)))?;
            mtrk.set_property("path", Some(PropertyInput::Component(path2)))?;
            entities.add(mtrk)?;
        }
        "model_export" => {
            let e = Entity::new(EntityOptions {
                id: Some("mdl1".into()),
                name: Some("Static Model".into()),
                position: Some(Rc::new(ConstantPositionProperty::new(
                    Some(Cartesian3::from_degrees(-105.0, 40.0, 250.0, &Ellipsoid::WGS84)),
                    ReferenceFrame::Fixed,
                ))),
                ..Default::default()
            })?;
            let m = Rc::new(ModelGraphics::new());
            m.set_uri(Some(PropertyInput::value("models/drone.glb")))?;
            m.set_scale(Some(PropertyInput::value(2.5)))?;
            m.set_height_reference(Some(PropertyInput::value(HeightReference::RelativeToGround as i32 as f64)))?;
            e.set_property("model", Some(PropertyInput::Component(m)))?;
            entities.add(e)?;
            opts.model_callback = Some(Rc::new(|_model, _time, external_files| {
                external_files.insert("models/drone.dae".into(), vec![68, 65, 69]);
                Ok("models/drone.dae".into())
            }));
        }
        "model_missing_callback" => {
            let e = Entity::new(EntityOptions {
                id: Some("mdlErr".into()),
                name: Some("No Callback Model".into()),
                position: Some(Rc::new(ConstantPositionProperty::new(
                    Some(Cartesian3::from_degrees(-105.0, 40.0, 0.0, &Ellipsoid::WGS84)),
                    ReferenceFrame::Fixed,
                ))),
                ..Default::default()
            })?;
            let m = Rc::new(ModelGraphics::new());
            m.set_uri(Some(PropertyInput::value("models/drone.glb")))?;
            e.set_property("model", Some(PropertyInput::Component(m)))?;
            entities.add(e)?;
        }
        "data_uri_texture" => {
            let e = Entity::new(EntityOptions {
                id: Some("tex1".into()),
                name: Some("Inline Texture".into()),
                position: Some(Rc::new(ConstantPositionProperty::new(
                    Some(Cartesian3::from_degrees(-105.0, 40.0, 0.0, &Ellipsoid::WGS84)),
                    ReferenceFrame::Fixed,
                ))),
                ..Default::default()
            })?;
            let bg = Rc::new(BillboardGraphics::new());
            bg.set_image(Some(PropertyInput::value("data:image/png;base64,iVBORw0KGgo=")))?;
            e.set_property("billboard", Some(PropertyInput::Component(bg)))?;
            entities.add(e)?;
        }
        other => panic!("unknown exportKml case {other}"),
    }

    export_kml(opts)
}

/// Normalizes an exported KML XML string into a structured JSON tree so float
/// formatting noise (e.g. `1e-15` in cartographic roundtrip) compares with `TIGHT`.
fn xml_node_to_json(node: roxmltree::Node<'_, '_>) -> Value {
    let mut attrs = serde_json::Map::new();
    for a in node.attributes() {
        let key = a.name().to_owned();
        let val = match a.value().parse::<f64>() {
            Ok(num) => json!(num),
            Err(_) => json!(a.value()),
        };
        attrs.insert(key, val);
    }
    let children: Vec<Value> = node.children().filter(|c| c.is_element()).map(xml_node_to_json).collect();
    let text = if children.is_empty() {
        let raw: String = node.children().filter_map(|c| c.text()).collect();
        // Parse comma/space-separated coordinate tuples or numbers structurally so TIGHT tolerance applies.
        if matches!(node.tag_name().name(), "coordinates" | "coord") && !raw.is_empty() {
            let tuples: Vec<Value> = raw
                .split_whitespace()
                .map(|t| {
                    let nums: Vec<f64> = t.split(',').map(|n| n.parse::<f64>().unwrap()).collect();
                    json!(nums)
                })
                .collect();
            json!(tuples)
        } else if let Ok(num) = raw.parse::<f64>() {
            json!(num)
        } else {
            json!(raw)
        }
    } else {
        Value::Null
    };
    json!({
        "tag": node.tag_name().name(),
        "ns": node.tag_name().namespace(),
        "attrs": attrs,
        "text": text,
        "children": children,
    })
}

fn parse_kml_structure(xml: &str) -> Value {
    let doc = roxmltree::Document::parse(xml).expect("valid exported KML XML");
    xml_node_to_json(doc.root_element())
}

#[test]
fn replay_export_kml() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/data_sources/exportKml.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    for (i, c) in cases.iter().enumerate() {
        let kind = c["a"][0].as_str().unwrap();
        let expected_throw = c["r"].get("throws").is_some();
        let res = build_export_case(kind);
        if expected_throw {
            assert!(res.is_err(), "case #{i} ({kind}) expected error, got Ok");
            continue;
        }
        let ExportKmlResult::Kml { kml, external_files } =
            res.unwrap_or_else(|e| panic!("case #{i} ({kind}) err: {e}"))
        else {
            panic!("expected Kml result");
        };
        let expected_kml = c["r"]["kml"].as_str().unwrap();
        let got_tree = parse_kml_structure(&kml);
        let exp_tree = parse_kml_structure(expected_kml);
        assert!(
            common::close(&got_tree, &exp_tree, TIGHT),
            "case #{i} ({kind}) KML XML mismatch!\nexpected XML: {expected_kml}\n     got XML: {kml}"
        );
        let mut got_files = serde_json::Map::new();
        for (k, v) in external_files {
            got_files.insert(k, json!(v));
        }
        assert_eq!(Value::Object(got_files), c["r"]["externalFiles"], "case #{i} ({kind}) externalFiles mismatch");
    }
}

// ---------------------------------------------------------------------------
// Native tests: KMZ roundtrip, NetworkLink refresh, ScreenOverlay, Tours
// ---------------------------------------------------------------------------

#[test]
fn kmz_zip_and_kml_roundtrip() {
    let png_bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4];
    let kml = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>KMZ Package</name>
  <Style id="s1">
    <IconStyle>
      <Icon><href>icons/pin.png</href></Icon>
    </IconStyle>
  </Style>
  <Placemark id="p1">
    <name>Embedded</name>
    <description><![CDATA[<img src="icons/pin.png"/>]]></description>
    <styleUrl>#s1</styleUrl>
    <Point><coordinates>-105.0,40.0,100.0</coordinates></Point>
  </Placemark>
</Document>
</kml>"#;

    let archive = zip::write_zip(&[("doc.kml", kml.as_bytes()), ("icons/pin.png", &png_bytes)]).unwrap();
    assert!(zip::is_zip_bytes(&archive));

    let ds = KmlDataSource::load_new(KmlInput::Kmz(archive), KmlOptions::default()).now_or_never().unwrap().unwrap();
    assert_eq!(ds.name().as_deref(), Some("KMZ Package"));
    let p1 = ds.entities().get_by_id("p1").unwrap();
    let t = date(0.0);
    let bb_comp = p1.get_property("billboard").unwrap().component().unwrap();
    let bb = bb_comp.as_any().downcast_ref::<BillboardGraphics>().unwrap();
    let img_url = eval_prop(bb.image(), &t);
    assert!(
        img_url.as_str().unwrap().starts_with("data:image/png;base64,"),
        "expected embedded data URI, got {img_url}"
    );
    let desc = eval_prop(p1.get_property("description"), &t);
    assert!(desc.as_str().unwrap().contains("data:image/png;base64,"));
}

#[test]
fn export_kmz_and_reload_into_kml_data_source() {
    let entities = EntityCollection::new();
    let e = Entity::new(EntityOptions {
        id: Some("rt1".into()),
        name: Some("Roundtrip Point".into()),
        position: Some(Rc::new(ConstantPositionProperty::new(
            Some(Cartesian3::from_degrees(-105.25, 40.02, 1600.0, &Ellipsoid::WGS84)),
            ReferenceFrame::Fixed,
        ))),
        ..Default::default()
    })
    .unwrap();
    let bg = Rc::new(BillboardGraphics::new());
    bg.set_image(Some(PropertyInput::value("data:image/png;base64,iVBORw0KGgo="))).unwrap();
    e.set_property("billboard", Some(PropertyInput::Component(bg))).unwrap();
    entities.add(e).unwrap();

    let mut opts = ExportKmlOptions::new(entities);
    opts.time = Some(date(0.0));
    opts.kmz = true;
    let ExportKmlResult::Kmz(kmz_bytes) = export_kml(opts).unwrap() else {
        panic!("expected Kmz");
    };
    let entries = zip::read_zip(&kmz_bytes).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].filename, "doc.kml");
    assert_eq!(entries[1].filename, "texture_1.png");

    let ds = KmlDataSource::load_new(kmz_bytes, KmlOptions::default()).now_or_never().unwrap().unwrap();
    let loaded = ds.entities().get_by_id("rt1").unwrap();
    assert_eq!(loaded.name().as_deref(), Some("Roundtrip Point"));
    let pos = loaded.position().unwrap().get_value(Some(&date(0.0)), None).unwrap().unwrap();
    let carto = Ellipsoid::WGS84.cartesian_to_cartographic(pos).unwrap();
    assert!((math::to_degrees(carto.longitude) + 105.25).abs() < 1e-6);
    assert!((math::to_degrees(carto.latitude) - 40.02).abs() < 1e-6);
    assert!((carto.height - 1600.0).abs() < 1e-3);
}

struct MockTransport {
    responses: RefCell<HashMap<String, Vec<String>>>,
    requests: RefCell<Vec<String>>,
    gate: Rc<Cell<bool>>,
}

impl MockTransport {
    fn new() -> Self {
        Self {
            responses: RefCell::new(HashMap::new()),
            requests: RefCell::new(Vec::new()),
            gate: Rc::new(Cell::new(true)),
        }
    }
    fn push(&self, url: &str, body: &str) {
        self.responses.borrow_mut().entry(url.to_owned()).or_default().push(body.to_owned());
    }
}

impl ResourceTransport for MockTransport {
    fn execute(&self, request: HttpRequest) -> RequestFuture<HttpResponse> {
        self.requests.borrow_mut().push(request.url.clone());
        let mut map = self.responses.borrow_mut();
        let body = map.get_mut(&request.url).and_then(|list| {
            if list.len() > 1 {
                Some(list.remove(0))
            } else {
                list.first().cloned()
            }
        });
        let gate = self.gate.clone();
        let mut response = Some(match body {
            Some(text) => Ok(HttpResponse {
                status: 200,
                body: text.into_bytes(),
                headers: vec![("Content-Type".into(), "application/vnd.google-earth.kml+xml".into())],
            }),
            None => {
                Err(RequestErrorEvent::new(Some(404), Some(format!("Not found: {}", request.url).into_bytes()), None))
            }
        });
        Box::pin(std::future::poll_fn(move |_| {
            if gate.get() {
                std::task::Poll::Ready(response.take().unwrap())
            } else {
                std::task::Poll::Pending
            }
        }))
    }
}

#[test]
fn network_link_initial_load_and_interval_refresh() {
    let transport = Rc::new(MockTransport::new());
    let root_kml = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>Root Link</name>
  <NetworkLink id="nl1">
    <name>Dynamic Feed</name>
    <Link>
      <href>feed.kml</href>
      <refreshMode>onInterval</refreshMode>
      <refreshInterval>10</refreshInterval>
    </Link>
  </NetworkLink>
</Document>
</kml>"#;

    let feed_v1 = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<NetworkLinkControl>
  <minRefreshPeriod>15</minRefreshPeriod>
  <cookie>session=abc</cookie>
</NetworkLinkControl>
<Document>
  <Placemark id="item"><name>Version 1</name><Point><coordinates>-105,40,0</coordinates></Point></Placemark>
</Document>
</kml>"#;

    let feed_v2 = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <Placemark id="item"><name>Version 2</name><Point><coordinates>-104,41,0</coordinates></Point></Placemark>
</Document>
</kml>"#;

    transport.push("https://example.com/root.kml", root_kml);
    transport.push("https://example.com/feed.kml", feed_v1);
    transport.push("https://example.com/feed.kml?session=abc", feed_v2);

    let client = ResourceClient::new(transport.clone());
    let current_time = Rc::new(Cell::new(date(0.0)));
    let ds = KmlDataSource::new(KmlOptions { resource_client: Some(client.clone()), ..Default::default() });
    let clock_cell = current_time.clone();
    ds.set_now_provider(Some(Rc::new(move || clock_cell.get())));

    let refreshes = Rc::new(RefCell::new(Vec::<String>::new()));
    let r = refreshes.clone();
    let _sub = ds.refresh_event().add_listener(move |(_, url)| r.borrow_mut().push(url.clone()));

    ds.load(
        Resource::new("https://example.com/root.kml").unwrap(),
        KmlOptions { resource_client: Some(client), ..Default::default() },
    )
    .now_or_never()
    .unwrap()
    .unwrap();

    let item_v1 = ds.entities().get_by_id("nl1item").expect("linked entity present");
    assert_eq!(item_v1.name().as_deref(), Some("Version 1"));

    // 12 seconds elapsed: minRefreshPeriod=15 prevents refresh even though refreshInterval=10.
    current_time.set(date(12.0));
    ds.update(&date(12.0)).unwrap();
    assert!(refreshes.borrow().is_empty());

    // 20 seconds elapsed: triggers refresh with cookie query parameter `session=abc`.
    current_time.set(date(20.0));
    transport.gate.set(false);
    ds.update(&date(20.0)).unwrap();
    ds.update(&date(20.0)).unwrap();
    assert_eq!(transport.requests.borrow().len(), 3);
    assert!(refreshes.borrow().is_empty());
    assert_eq!(ds.entities().get_by_id("nl1item").unwrap().name().as_deref(), Some("Version 1"));

    transport.gate.set(true);
    ds.update(&date(20.0)).unwrap();
    assert_eq!(transport.requests.borrow().len(), 3);
    assert_eq!(refreshes.borrow().len(), 1);
    assert_eq!(refreshes.borrow()[0], "https://example.com/feed.kml?session=abc");
    let item_v2 = ds.entities().get_by_id("nl1item").expect("refreshed entity present");
    assert_eq!(item_v2.name().as_deref(), Some("Version 2"));
}

#[test]
fn network_link_waits_for_camera_to_stop_and_substitutes_the_current_view() {
    let transport = Rc::new(MockTransport::new());
    let feed = r#"<kml xmlns="http://www.opengis.net/kml/2.2"><Placemark id="item"/></kml>"#;
    transport.push("https://example.com/feed.kml?width=800", feed);
    transport.push("https://example.com/feed.kml?width=640", feed);
    let options = KmlOptions {
        source_uri: Some("https://example.com/root.kml".into()),
        resource_client: Some(ResourceClient::new(transport.clone())),
        ..Default::default()
    };
    let source = KmlDataSource::new(options.clone());
    let now = Rc::new(Cell::new(date(0.0)));
    let time = now.clone();
    source.set_now_provider(Some(Rc::new(move || time.get())));
    let mut camera = Camera::new(CameraOptions::new(800.0, 600.0));
    source.update_camera(&mut camera).unwrap();
    source
        .load(
            r#"<kml xmlns="http://www.opengis.net/kml/2.2">
            <NetworkLink id="link"><Link>
                <href>feed.kml</href>
                <viewRefreshMode>onStop</viewRefreshMode>
                <viewRefreshTime>2</viewRefreshTime>
                <viewFormat>width=[horizPixels]</viewFormat>
            </Link></NetworkLink>
        </kml>"#,
            options,
        )
        .now_or_never()
        .unwrap()
        .unwrap();
    assert_eq!(&*transport.requests.borrow(), &["https://example.com/feed.kml?width=800"]);

    now.set(date(10.0));
    camera.position += Cartesian3::new(10.0, 0.0, 0.0);
    camera.client_width = 640.0;
    source.update_camera(&mut camera).unwrap();
    now.set(date(11.0));
    source.update(&date(11.0)).unwrap();
    assert_eq!(transport.requests.borrow().len(), 1);
    now.set(date(12.0));
    source.update(&date(12.0)).unwrap();
    source.update(&date(12.0)).unwrap();
    assert_eq!(
        &*transport.requests.borrow(),
        &["https://example.com/feed.kml?width=800", "https://example.com/feed.kml?width=640",]
    );
}

#[test]
fn screen_overlay_and_destroy_and_invalid_xml() {
    let kml = r#"<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>Overlay Doc</name>
  <ScreenOverlay>
    <Icon><href>https://example.com/logo.png</href></Icon>
    <overlayXY x="0" y="1" xunits="fraction" yunits="fraction"/>
    <screenXY x="10" y="10" xunits="pixels" yunits="insetPixels"/>
    <size x="100" y="50" xunits="pixels" yunits="pixels"/>
  </ScreenOverlay>
</Document>
</kml>"#;
    let container: KmlScreenOverlayContainer = Rc::new(RefCell::new(Vec::new()));
    let ds = KmlDataSource::load_new(
        kml,
        KmlOptions {
            screen_overlay_container: Some(container.clone()),
            credit: Some(Credit::new("KML Credit", false)),
            ..Default::default()
        },
    )
    .now_or_never()
    .unwrap()
    .unwrap();

    assert_eq!(ds.credit().unwrap().html(), "KML Credit");
    assert_eq!(container.borrow().len(), 1);
    assert_eq!(container.borrow()[0].icon_url, "https://example.com/logo.png");
    assert_eq!(container.borrow()[0].size, Some((100.0, 50.0, Some("pixels".into()), Some("pixels".into()))));
    ds.destroy();
    assert!(container.borrow().is_empty());

    // Invalid XML raises errorEvent twice (matching CesiumJS) and returns Err.
    let err_count = Rc::new(Cell::new(0usize));
    let ec = err_count.clone();
    let _sub = ds.error_event().add_listener(move |_| ec.set(ec.get() + 1));
    let res = ds.load("<kml><broken>", KmlOptions::default()).now_or_never().unwrap();
    assert!(res.is_err());
    assert_eq!(err_count.get(), 2);
    assert!(!ds.is_loading());
}

#[test]
fn kml_external_file_map_roundtrip_smoke() {
    let entities = EntityCollection::new();
    let e = Entity::new(EntityOptions {
        id: Some("e1".into()),
        name: Some("Model".into()),
        position: Some(Rc::new(ConstantPositionProperty::new(
            Some(Cartesian3::from_degrees(0.0, 0.0, 0.0, &Ellipsoid::WGS84)),
            ReferenceFrame::Fixed,
        ))),
        ..Default::default()
    })
    .unwrap();
    let m = Rc::new(ModelGraphics::new());
    m.set_uri(Some(PropertyInput::value("a.glb"))).unwrap();
    e.set_property("model", Some(PropertyInput::Component(m))).unwrap();
    entities.add(e).unwrap();

    let mut opts = ExportKmlOptions::new(entities);
    opts.time = Some(date(0.0));
    opts.model_callback = Some(Rc::new(|_, _, files: &mut BTreeMap<String, Vec<u8>>| {
        files.insert("a.dae".into(), b"<COLLADA/>".to_vec());
        Ok("a.dae".into())
    }));
    let ExportKmlResult::Kml { kml, external_files } = export_kml(opts).unwrap() else {
        panic!("expected Kml");
    };
    assert!(kml.contains("<href>a.dae</href>"));
    assert_eq!(external_files.get("a.dae").map(Vec::as_slice), Some(b"<COLLADA/>".as_slice()));
}
