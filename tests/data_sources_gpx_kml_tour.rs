mod common;
mod entity_common;

use cesium_rs::{core::*, data_sources::*, scene::HeightReference};
use common::{ok, run, EXACT, TIGHT};
use entity_common::date;
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::Rc,
};

const FULL_GPX: &str = r#"<?xml version="1.0" encoding="UTF-8"?><gpx version="1.1" creator="Fixture" xmlns="http://www.topografix.com/GPX/1/1"><metadata><name>Test Track</name><desc>A test GPX</desc><author><name>Jane Doe</name><email><id>jane</id><domain>example.com</domain></email><link href="https://example.com/author"><text>Author</text><type>text/html</type></link></author><copyright author="ACME Corp"><year>2020</year><license>MIT</license></copyright><link href="https://example.com/gpx"><text>GPX Link</text><type>application/gpx+xml</type></link><time>2020-01-01T00:00:00Z</time><keywords>hiking, test, gpx</keywords><bounds><minlat>40.0</minlat><maxlat>41.0</maxlat><minlon>-106.0</minlon><maxlon>-105.0</maxlon></bounds></metadata><wpt id="w1" lat="40.5" lon="-105.5"><ele>1600</ele><name>Waypoint One</name><cmt>A waypoint comment</cmt><desc>Waypoint description</desc></wpt><rte id="r1"><name>Route One</name><rtept id="rp1" lat="40.0" lon="-105.0"><ele>1500</ele><name>Route Start</name></rtept><rtept id="rp2" lat="41.0" lon="-106.0"><ele>1700</ele><name>Route End</name></rtept></rte><trk id="t1"><name>Track One</name><trkseg><trkpt lat="40.0" lon="-105.0"><ele>1500</ele><time>2020-01-01T00:00:00Z</time></trkpt><trkpt lat="40.1" lon="-105.1"><ele>1510</ele><time>2020-01-01T00:01:00Z</time></trkpt></trkseg><trkseg><trkpt lat="40.2" lon="-105.2"><ele>1520</ele><time>2020-01-01T00:02:00Z</time></trkpt><trkpt lat="40.3" lon="-105.3"><ele>1530</ele><time>2020-01-01T00:03:00Z</time></trkpt></trkseg></trk></gpx>"#;

const WAYPOINTS_GPX: &str = r#"<?xml version="1.0" encoding="UTF-8"?><gpx version="1.1" creator="Waypoints" xmlns="http://www.topografix.com/GPX/1/1"><wpt id="a" lat="10.0" lon="20.0"><ele>100</ele><name>A</name></wpt><wpt id="b" lat="11.0" lon="21.0"><name>B</name></wpt></gpx>"#;

const TRACK_GPX: &str = r#"<?xml version="1.0" encoding="UTF-8"?><gpx version="1.1" creator="Track" xmlns="http://www.topografix.com/GPX/1/1"><trk id="t"><name>Short Track</name><trkseg><trkpt lat="30.0" lon="-100.0"><ele>500</ele><time>2020-01-01T00:00:00Z</time></trkpt><trkpt lat="30.1" lon="-100.1"><ele>510</ele><time>2020-01-01T00:00:30Z</time></trkpt></trkseg></trk></gpx>"#;

const INVALID_GPX: &str = "<gpx><broken>";

fn eval_input(input: Option<PropertyInput>, time: &JulianDate) -> Value {
    match input {
        None => Value::Null,
        Some(PropertyInput::Value(v)) => v.to_json(),
        Some(PropertyInput::Component(c)) => {
            c.get_value(time, None).unwrap().map_or(Value::Null, |v| v.to_json())
        }
    }
}

fn encode_link(l: Option<&GpxLink>) -> Value {
    match l {
        None => Value::Null,
        Some(l) => json!({ "href": l.href, "text": l.text, "mimeType": l.mime_type }),
    }
}

fn encode_metadata(m: Option<&GpxMetadata>) -> Value {
    match m {
        None => Value::Null,
        Some(m) => json!({
            "name": m.name,
            "desc": m.desc,
            "author": m.author.as_ref().map(|a| json!({
                "name": a.name,
                "email": a.email,
                "link": encode_link(a.link.as_ref()),
            })),
            "copyright": m.copyright.as_ref().map(|c| json!({
                "author": c.author,
                "year": c.year,
                "license": c.license,
            })),
            "link": encode_link(m.link.as_ref()),
            "time": m.time,
            "keywords": m.keywords,
            "bounds": m.bounds.as_ref().map(|b| json!({
                "minLat": b.min_lat,
                "maxLat": b.max_lat,
                "minLon": b.min_lon,
                "maxLon": b.max_lon,
            })),
        }),
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
    let billboard = match e.get_property("billboard") {
        Some(PropertyInput::Component(c)) => {
            let b = c.as_any().downcast_ref::<BillboardGraphics>().unwrap();
            json!({
                "image": eval_input(b.image(), &t),
                "width": eval_input(b.width(), &t),
                "height": eval_input(b.height(), &t),
                "scaleByDistance": eval_input(b.scale_by_distance(), &t),
                "pixelOffsetScaleByDistance": eval_input(b.pixel_offset_scale_by_distance(), &t),
                "verticalOrigin": eval_input(b.vertical_origin(), &t),
                "heightReference": eval_input(b.height_reference(), &t),
            })
        }
        _ => Value::Null,
    };
    let label = match e.get_property("label") {
        Some(PropertyInput::Component(c)) => {
            let l = c.as_any().downcast_ref::<LabelGraphics>().unwrap();
            json!({
                "text": eval_input(l.text(), &t),
                "font": eval_input(l.font(), &t),
                "style": eval_input(l.style(), &t),
                "pixelOffset": eval_input(l.pixel_offset(), &t),
                "horizontalOrigin": eval_input(l.horizontal_origin(), &t),
                "translucencyByDistance": eval_input(l.translucency_by_distance(), &t),
                "heightReference": eval_input(l.height_reference(), &t),
            })
        }
        _ => Value::Null,
    };
    let polyline = match e.get_property("polyline") {
        Some(PropertyInput::Component(c)) => {
            let p = c.as_any().downcast_ref::<PolylineGraphics>().unwrap();
            let material = match p.material() {
                Some(PropertyInput::Component(mc)) => {
                    let m = mc.as_any().downcast_ref::<PolylineOutlineMaterialProperty>().unwrap();
                    json!({
                        "color": eval_input(m.color(), &t),
                        "outlineColor": eval_input(m.outline_color(), &t),
                        "outlineWidth": eval_input(m.outline_width(), &t),
                    })
                }
                _ => Value::Null,
            };
            json!({
                "width": eval_input(p.width(), &t),
                "material": material,
                "clampToGround": eval_input(p.clamp_to_ground(), &t),
                "positions": eval_input(p.positions(), &t),
            })
        }
        _ => Value::Null,
    };
    json!({
        "id": e.id(),
        "name": e.name(),
        "description": eval_input(e.get_property("description"), &t),
        "availability": availability,
        "position": position,
        "billboard": billboard,
        "label": label,
        "polyline": polyline,
    })
}

#[test]
fn replay_gpx_data_source() {
    run(
        "data_sources",
        "GpxDataSource",
        |_| TIGHT,
        |_, args| {
            let kind = args[0].as_str().unwrap();
            let (xml, opts) = match kind {
                "full" => (
                    FULL_GPX,
                    GpxOptions { waypoint_image: Some("marker.png".into()), ..Default::default() },
                ),
                "styled" => (
                    FULL_GPX,
                    GpxOptions {
                        clamp_to_ground: true,
                        waypoint_image: Some("waypoint.png".into()),
                        track_color: Color::from_css_color_string("#00ff00"),
                        route_color: Color::from_css_color_string("#0000ff"),
                        ..Default::default()
                    },
                ),
                "waypoints" => (
                    WAYPOINTS_GPX,
                    GpxOptions { waypoint_image: Some("marker.png".into()), ..Default::default() },
                ),
                "track" => (
                    TRACK_GPX,
                    GpxOptions { waypoint_image: Some("marker.png".into()), ..Default::default() },
                ),
                "invalid" => (INVALID_GPX, GpxOptions::default()),
                other => panic!("unknown gpx case {other}"),
            };

            let ds = GpxDataSource::new();
            let changed = Rc::new(Cell::new(0usize));
            let loading = Rc::new(RefCell::new(Vec::<bool>::new()));
            let error_count = Rc::new(Cell::new(0usize));
            let c = changed.clone();
            let _s1 = ds.changed_event().add_listener(move |_| c.set(c.get() + 1));
            let l = loading.clone();
            let _s2 = ds.loading_event().add_listener(move |(_, v)| l.borrow_mut().push(*v));
            let e = error_count.clone();
            let _s3 = ds.error_event().add_listener(move |_| e.set(e.get() + 1));

            let res = ds.load(xml, opts).now_or_never().unwrap();
            let entities: Vec<Value> = ds.entities().values().iter().map(snapshot_entity).collect();
            let loading_log = loading.borrow().clone();
            ok(json!({
                "name": ds.name(),
                "creator": ds.creator(),
                "version": ds.version(),
                "metadata": encode_metadata(ds.metadata().as_ref()),
                "clock": encode_clock(ds.clock()),
                "isLoading": ds.is_loading(),
                "entities": entities,
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

#[test]
fn replay_kml_camera_and_look_at() {
    run(
        "data_sources",
        "KmlCamera",
        |_| EXACT,
        |_, args| {
            let p = args[0].as_array().unwrap();
            let h = args[1].as_array().unwrap();
            let c = KmlCamera::new(
                Cartesian3::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()),
                HeadingPitchRoll::new(h[0].as_f64().unwrap(), h[1].as_f64().unwrap(), h[2].as_f64().unwrap()),
            );
            ok(json!({
                "position": c.position.to_array(),
                "headingPitchRoll": [c.heading_pitch_roll.heading, c.heading_pitch_roll.pitch, c.heading_pitch_roll.roll],
            }))
        },
    );
    run(
        "data_sources",
        "KmlLookAt",
        |_| EXACT,
        |_, args| {
            let p = args[0].as_array().unwrap();
            let h = args[1].as_array().unwrap();
            let c = KmlLookAt::new(
                Cartesian3::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap()),
                HeadingPitchRange::new(h[0].as_f64().unwrap(), h[1].as_f64().unwrap(), h[2].as_f64().unwrap()),
            );
            ok(json!({
                "position": c.position.to_array(),
                "headingPitchRange": [c.heading_pitch_range.heading, c.heading_pitch_range.pitch, c.heading_pitch_range.range],
            }))
        },
    );
}

fn encode_cam_options(o: &KmlTourCameraOptions) -> Value {
    json!({
        "duration": o.duration,
        "hasComplete": o.complete.is_some(),
        "hasEasingFunction": o.easing_function.is_some(),
        "destination": o.destination.map(|d| d.to_array()),
        "orientation": o.orientation.map(|h| [h.heading, h.pitch, h.roll]),
        "offset": o.offset.map(|h| [h.heading, h.pitch, h.range]),
    })
}

struct MockCamera(Rc<RefCell<Vec<Value>>>);
impl KmlTourCamera for MockCamera {
    fn fly_to(&self, options: KmlTourCameraOptions) {
        self.0.borrow_mut().push(json!(["flyTo", encode_cam_options(&options)]));
    }
    fn fly_to_bounding_sphere(&self, target: BoundingSphere, options: KmlTourCameraOptions) {
        self.0.borrow_mut().push(json!(["flyToBoundingSphere", target.center.to_array(), encode_cam_options(&options)]));
    }
    fn cancel_flight(&self) {
        self.0.borrow_mut().push(json!(["cancelFlight"]));
    }
}

#[test]
fn replay_kml_tour_fly_to() {
    run(
        "data_sources",
        "KmlTourFlyTo",
        |_| EXACT,
        |f, args| {
            let cam = KmlCamera::new(Cartesian3::new(1., 2., 3.), HeadingPitchRoll::new(0.1, 0.2, 0.3));
            let look = KmlLookAt::new(Cartesian3::new(4., 5., 6.), HeadingPitchRange::new(0.4, 0.5, 100.));
            match f {
                "options" => {
                    let view = match args[0].as_str().unwrap() {
                        "camera" => KmlTourView::Camera(cam),
                        _ => KmlTourView::LookAt(look),
                    };
                    let mode = args[1].as_str().unwrap();
                    let fly = KmlTourFlyTo::new(3.5, mode, view);
                    ok(json!({
                        "type": fly.entry_type(),
                        "blocking": fly.blocking(),
                        "duration": fly.duration,
                        "flyToMode": fly.fly_to_mode,
                        "activeCamera": fly.has_active_camera(),
                        "activeCallback": fly.has_active_callback(),
                        "options": encode_cam_options(&fly.get_camera_options(None)),
                    }))
                }
                "merge" => {
                    let fly = KmlTourFlyTo::new(5., "bounce", cam);
                    let extra = KmlTourCameraOptions { duration: Some(99.), ..Default::default() };
                    ok(encode_cam_options(&fly.get_camera_options(Some(&extra))))
                }
                "playStop" => {
                    let fly = KmlTourFlyTo::new(2., "smooth", cam);
                    let trace = Rc::new(RefCell::new(Vec::<Value>::new()));
                    let camera = Rc::new(MockCamera(trace.clone()));
                    let t = trace.clone();
                    fly.play(Some(Rc::new(move |terminated| t.borrow_mut().push(json!(["done", terminated])))), camera, None);
                    let after_play = json!({
                        "activeCamera": fly.has_active_camera(),
                        "activeCallback": fly.has_active_callback(),
                    });
                    fly.stop();
                    let after_stop = json!({
                        "activeCamera": fly.has_active_camera(),
                        "activeCallback": fly.has_active_callback(),
                    });
                    let log = trace.borrow().clone();
                    ok(json!({
                        "afterPlay": after_play,
                        "afterStop": after_stop,
                        "trace": log,
                    }))
                }
                _ => None,
            }
        },
    );
}

#[test]
fn replay_kml_tour_wait() {
    run(
        "data_sources",
        "KmlTourWait",
        |_| EXACT,
        |_, args| {
            let duration = args[0].as_f64().unwrap();
            let w = KmlTourWait::new(duration);
            let state = |w: &KmlTourWait| {
                json!({
                    "type": w.entry_type(),
                    "blocking": w.blocking(),
                    "duration": w.duration,
                    "hasCallback": w.has_active_callback(),
                    "hasTimeout": w.has_timeout(),
                })
            };
            let before = state(&w);
            let calls = Rc::new(RefCell::new(Vec::<bool>::new()));
            let c = calls.clone();
            w.play(Rc::new(move |terminated| c.borrow_mut().push(terminated)));
            let after_play = state(&w);
            w.stop();
            let after_stop = state(&w);
            let calls_log = calls.borrow().clone();
            ok(json!([before, after_play, calls_log, after_stop]))
        },
    );
}

type DoneCallback = Rc<dyn Fn(bool)>;

struct MockEntry {
    id: String,
    sync_done: bool,
    trace: Rc<RefCell<Vec<Value>>>,
    done: RefCell<Option<DoneCallback>>,
}
impl MockEntry {
    fn new(id: &str, sync_done: bool, trace: Rc<RefCell<Vec<Value>>>) -> Rc<Self> {
        Rc::new(Self { id: id.to_owned(), sync_done, trace, done: RefCell::new(None) })
    }
}
impl KmlTourPlaylistEntry for MockEntry {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn blocking(&self) -> bool {
        true
    }
    fn play(&self, done: Rc<dyn Fn(bool)>, _: Option<Rc<dyn KmlTourCamera>>, _: Option<&KmlTourCameraOptions>) {
        self.trace.borrow_mut().push(json!(["play", self.id]));
        if self.sync_done {
            done(false);
        } else {
            *self.done.borrow_mut() = Some(done);
        }
    }
    fn stop(&self) {
        if let Some(done) = self.done.borrow_mut().take() {
            done(true);
        }
    }
}

fn entry_id(e: &KmlTourEntryRef) -> String {
    e.as_any().downcast_ref::<MockEntry>().unwrap().id.clone()
}

#[test]
fn replay_kml_tour() {
    run(
        "data_sources",
        "KmlTour",
        |_| EXACT,
        |f, _| match f {
            "play" => {
                let t = KmlTour::new("My Tour", "tour-1");
                let trace = Rc::new(RefCell::new(Vec::<Value>::new()));
                let tr = trace.clone();
                let _s1 = t.tour_start().add_listener(move |_| tr.borrow_mut().push(json!(["tourStart"])));
                let tr = trace.clone();
                let _s2 = t.tour_end().add_listener(move |term| tr.borrow_mut().push(json!(["tourEnd", term])));
                let tr = trace.clone();
                let _s3 = t.entry_start().add_listener(move |e| tr.borrow_mut().push(json!(["entryStart", entry_id(e)])));
                let tr = trace.clone();
                let _s4 = t
                    .entry_end()
                    .add_listener(move |(e, term)| tr.borrow_mut().push(json!(["entryEnd", entry_id(e), term])));
                for id in ["a", "b", "c"] {
                    t.add_playlist_entry(MockEntry::new(id, true, trace.clone()));
                }
                let before = json!({
                    "id": t.id,
                    "name": t.name,
                    "playlistIndex": t.playlist_index(),
                    "playlist": t.playlist().iter().map(entry_id).collect::<Vec<_>>(),
                    "active": t.active_entries().iter().map(entry_id).collect::<Vec<_>>(),
                });
                t.play(None, None);
                let after_play = json!({
                    "playlistIndex": t.playlist_index(),
                    "active": t.active_entries().iter().map(entry_id).collect::<Vec<_>>(),
                });
                let log = trace.borrow().clone();
                ok(json!({ "before": before, "afterPlay": after_play, "trace": log }))
            }
            "stop" => {
                let t = KmlTour::new("Tour", "t2");
                let trace = Rc::new(RefCell::new(Vec::<Value>::new()));
                let tr = trace.clone();
                let _s1 = t.tour_start().add_listener(move |_| tr.borrow_mut().push(json!(["tourStart"])));
                let tr = trace.clone();
                let _s2 = t.tour_end().add_listener(move |term| tr.borrow_mut().push(json!(["tourEnd", term])));
                let tr = trace.clone();
                let _s3 = t.entry_start().add_listener(move |e| tr.borrow_mut().push(json!(["entryStart", entry_id(e)])));
                let tr = trace.clone();
                let _s4 = t
                    .entry_end()
                    .add_listener(move |(e, term)| tr.borrow_mut().push(json!(["entryEnd", entry_id(e), term])));
                t.add_playlist_entry(MockEntry::new("a", false, trace.clone()));
                t.add_playlist_entry(MockEntry::new("b", false, trace.clone()));
                t.play(None, None);
                t.stop();
                let log = trace.borrow().clone();
                ok(json!({ "trace": log }))
            }
            _ => None,
        },
    );
}

struct MockHost {
    entity: EntityRef,
    trace: Vec<Value>,
    terrain_offset: Option<()>,
}

impl HeightReferenceUpdaterHost for MockHost {
    type TerrainOffset = ();
    fn on_entity_property_changed(&mut self, _: &Entity, property_name: &str) {
        self.trace.push(json!(["base", property_name]));
    }
    fn observed_property_names(&self) -> &[&str] {
        &["billboard"]
    }
    fn entity(&self) -> &Entity {
        &self.entity
    }
    fn geometry_property_name(&self) -> &str {
        "billboard"
    }
    fn is_dynamic(&self) -> bool {
        true
    }
    fn take_terrain_offset(&mut self) -> Option<()> {
        self.terrain_offset.take()
    }
    fn destroy_terrain_offset(&mut self, _: ()) {
        self.trace.push(json!(["destroy"]));
    }
    fn compute_center(&self) -> Rc<dyn Fn(Option<&JulianDate>) -> Option<Cartesian3>> {
        Rc::new(|_| Some(Cartesian3::new(1., 2., 3.)))
    }
    fn create_terrain_offset(&mut self, _: PropertyRef<Cartesian3>, _: PropertyInput) {}
    fn set_terrain_offset(&mut self, offset: Option<()>) {
        self.terrain_offset = offset;
    }
}

#[test]
fn replay_height_reference_on_entity_property_changed() {
    run(
        "data_sources",
        "heightReferenceOnEntityPropertyChanged",
        |_| EXACT,
        |_, args| {
            let kind = args[0].as_str().unwrap();
            let (prop_name, geometry, height_ref, pre_existing) = match kind {
                "withHeight" => ("billboard", true, true, false),
                "noHeight" => ("billboard", true, false, false),
                "notObserved" => ("position", true, true, false),
                "noGeometry" => ("billboard", false, false, false),
                "preExisting" => ("billboard", true, true, true),
                other => panic!("unknown case {other}"),
            };
            let entity = Entity::new(EntityOptions { id: Some("e".into()), ..Default::default() }).unwrap();
            if geometry {
                let bb = Rc::new(BillboardGraphics::new());
                if height_ref {
                    bb.set_height_reference(Some(PropertyInput::value(HeightReference::ClampToGround as i32 as f64)))
                        .unwrap();
                }
                entity.set_property("billboard", Some(PropertyInput::Component(bb))).unwrap();
            }
            let mut host = MockHost {
                entity: entity.clone(),
                trace: Vec::new(),
                terrain_offset: pre_existing.then_some(()),
            };
            height_reference_on_entity_property_changed(&mut host, &entity, prop_name);
            ok(json!({
                "trace": host.trace,
                "terrainOffset": host.terrain_offset.is_some(),
            }))
        },
    );
}
