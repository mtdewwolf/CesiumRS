mod common;
mod entity_common;

use cesium_rs::{core::*, data_sources::*};
use common::{ok, run, Enc, EXACT, TIGHT};
use entity_common::{date, encode};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

type Trace = Rc<RefCell<Vec<Value>>>;
fn time() -> JulianDate {
    date(0.)
}

#[test]
fn sphere_states() {
    run(
        "data_sources",
        "BoundingSphereState",
        |_| EXACT,
        |_, _| {
            use BoundingSphereState::*;
            ok(json!([Done as u8, Pending as u8, Failed as u8]))
        },
    );
}

#[test]
fn custom_data_source() {
    run(
        "data_sources",
        "CustomDataSource",
        |_| EXACT,
        |_, _| {
            let s = CustomDataSource::new(None);
            let trace = Trace::default();
            let t = trace.clone();
            let _changed = s
                .changed_event()
                .add_listener(move |s| t.borrow_mut().push(json!(["change", s.name(), s.clock().is_some()])));
            let t = trace.clone();
            let _loading = s
                .loading_event()
                .add_listener(move |(s, v)| t.borrow_mut().push(json!(["loading", v, s.is_loading()])));
            let t = trace.clone();
            let _entities = s.entities().collection_changed().add_listener(move |e| {
                let s = e.collection.upgrade().unwrap().owner().unwrap().downcast::<CustomDataSource>().unwrap();
                t.borrow_mut().push(json!([
                    "entities",
                    e.added.iter().map(|e| e.id()).collect::<Vec<_>>(),
                    s.is_loading()
                ]));
            });
            let owner = s.entities().owner().unwrap().downcast::<CustomDataSource>().unwrap();
            let initial = json!([s.name(), s.clock().is_some(), s.show(), s.is_loading(), Rc::ptr_eq(&s, &owner)]);
            s.set_name(Some("one".into()));
            s.set_name(Some("one".into()));
            let clock = Rc::new(DataSourceClock::new());
            s.set_clock(Some(clock.clone()));
            s.set_clock(Some(clock));
            s.set_clock(None);
            s.set_loading(true).unwrap();
            s.set_loading(true).unwrap();
            s.entities().get_or_create_entity("A");
            s.entities().get_or_create_entity("B");
            s.set_loading(false).unwrap();
            s.set_loading(false).unwrap();
            s.set_show(false).unwrap();
            ok(json!([initial, trace.take(), s.entities().show(), s.update(&time()).unwrap()]))
        },
    );
}

struct TestSource {
    state: DataSourceState,
    tag: usize,
    trace: Trace,
}
impl TestSource {
    fn make(name: &str, tag: usize, trace: Trace) -> DataSourceRef {
        Rc::new_cyclic(|owner| Self { state: DataSourceState::new(owner.clone(), Some(name.into())), tag, trace })
    }
}
impl DataSource for TestSource {
    fn state(&self) -> &DataSourceState {
        &self.state
    }
    fn destroy(&self) {
        self.trace.borrow_mut().push(json!(["destroy", self.tag]));
    }
}

#[test]
fn collection_order_and_destruction() {
    run(
        "data_sources",
        "DataSourceCollection",
        |_| EXACT,
        |_, _| {
            let c = DataSourceCollection::new();
            let trace = Trace::default();
            let sources = ["A", "B", "A"]
                .into_iter()
                .enumerate()
                .map(|(i, n)| TestSource::make(n, i, trace.clone()))
                .collect::<Vec<_>>();
            let tag = {
                let sources = sources.clone();
                move |s: &DataSourceRef| sources.iter().position(|p| Rc::ptr_eq(p, s)).unwrap()
            };
            let mut subscriptions = vec![];
            for (event, label) in [(c.data_source_added(), "add"), (c.data_source_removed(), "remove")] {
                let t = trace.clone();
                let tag = tag.clone();
                subscriptions.push(
                    event.add_listener(move |e| {
                        t.borrow_mut().push(json!([label, tag(&e.source), e.collection.length()]))
                    }),
                );
            }
            let t = trace.clone();
            let moved_tag = tag.clone();
            subscriptions.push(c.data_source_moved().add_listener(move |e| {
                t.borrow_mut().push(json!(["move", moved_tag(&e.source), e.new_index, e.old_index]))
            }));
            for s in &sources {
                c.add(s.clone()).unwrap();
            }
            let (a, b, d) = (&sources[0], &sources[1], &sources[2]);
            c.lower(a).unwrap();
            c.raise(a).unwrap();
            c.raise_to_top(b).unwrap();
            c.lower_to_bottom(d).unwrap();
            c.lower(b).unwrap();
            c.raise(a).unwrap();
            let order: Vec<_> = (0..c.length()).map(|i| tag(&c.get(i).unwrap())).collect();
            let names: Vec<_> = c.get_by_name("A").iter().map(tag).collect();
            let removed = c.remove(a, true);
            let absent = c.remove(a, true);
            assert!(!c.contains(a));
            assert!(c.raise(a).is_err());
            c.destroy();
            assert!(c.add(a.clone()).is_err());
            ok(json!([trace.take(), order, names, removed, absent, c.is_destroyed()]))
        },
    );
}

struct TestVisualizer {
    name: String,
    i: usize,
    ready: Rc<Cell<bool>>,
    pending: Rc<Cell<bool>>,
    trace: Trace,
    destroyed: bool,
}
impl Visualizer for TestVisualizer {
    fn update(&mut self, _: &JulianDate) -> Result<bool, RuntimeError> {
        self.trace.borrow_mut().push(json!(["update", self.name, self.i]));
        Ok(self.ready.get())
    }
    fn get_bounding_sphere(&self, _: &EntityRef, r: &mut BoundingSphere) -> BoundingSphereState {
        if self.i == 2 {
            return if self.pending.get() { BoundingSphereState::Pending } else { BoundingSphereState::Failed };
        }
        *r = BoundingSphere::new(Cartesian3::new((self.i * 6) as f64, 0., 0.), 1. + self.i as f64);
        BoundingSphereState::Done
    }
    fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    fn destroy(&mut self) {
        assert!(!self.is_destroyed());
        self.destroyed = true;
        self.trace.borrow_mut().push(json!(["destroy", self.name, self.i]));
    }
}
#[test]
fn display_updates_bounds_and_teardown() {
    run(
        "data_sources",
        "DataSourceDisplay",
        |_| TIGHT,
        |_, _| {
            let c = DataSourceCollection::new();
            let trace = Trace::default();
            let a = c.add(CustomDataSource::new(Some("A".into()))).unwrap();
            let ready = Rc::new(Cell::new(true));
            let pending = Rc::new(Cell::new(true));
            let d = DataSourceDisplay::new(
                c.clone(),
                Rc::new({
                    let ready = ready.clone();
                    let pending = pending.clone();
                    let t = trace.clone();
                    move |s| {
                        let name = s.name().unwrap_or_else(|| "default".into());
                        t.borrow_mut().push(json!(["create", name]));
                        (0..3)
                            .map(|i| {
                                Box::new(TestVisualizer {
                                    name: name.clone(),
                                    i,
                                    ready: ready.clone(),
                                    pending: pending.clone(),
                                    trace: t.clone(),
                                    destroyed: false,
                                }) as Box<dyn Visualizer>
                            })
                            .collect()
                    }
                }),
            );
            let b = c.add(CustomDataSource::new(Some("B".into()))).unwrap();
            c.raise(&a).unwrap();
            let e = b.entities().get_or_create_entity("B");
            let def = d.default_data_source().entities().get_or_create_entity("default");
            let outsider = Entity::with_id("B");
            let bounds = |e, partial| {
                let mut r = BoundingSphere::new(Cartesian3::new(2., 3., 4.), 5.);
                json!([d.get_bounding_sphere(e, partial, &mut r) as u8, r.enc()])
            };
            let mut frames = vec![
                json!(d.ready()),
                bounds(&e, false),
                json!(d.update(&time()).unwrap()),
                json!(d.ready()),
                bounds(&e, false),
                bounds(&e, true),
                bounds(&outsider, true),
            ];
            ready.set(false);
            pending.set(false);
            frames.extend([json!(d.update(&time()).unwrap()), json!(d.ready()), bounds(&def, false)]);
            c.remove(&b, false);
            frames.push(bounds(&e, false));
            d.destroy();
            assert_eq!(c.data_source_moved().number_of_listeners(), 0);
            assert_eq!(c.data_source_added().number_of_listeners(), 0);
            assert_eq!(c.data_source_removed().number_of_listeners(), 0);
            ok(json!([frames, trace.take(), d.is_destroyed(), c.length()]))
        },
    );
}

fn project(offset: f64) -> CrsFunction {
    Rc::new(move |p| Ok(Cartesian3::new(p[0] + offset, p[1], p.get(2).copied().unwrap_or(0.))))
}
fn link(offset: f64) -> CrsLinkResolver {
    Rc::new(move |p| Box::pin(async move { Ok(project(p["offset"].as_f64().unwrap() + offset)) }))
}
fn options(v: &Value) -> GeoJsonOptions {
    let mut registry = CrsRegistry::default();
    registry.names.insert("fixture".into(), project(1.));
    registry.link_hrefs.insert("fixture".into(), link(1.));
    registry.link_types.insert("fixture-type".into(), link(2.));
    let mut o = GeoJsonOptions {
        crs: Rc::new(registry),
        source_uri: v["sourceUri"].as_str().map(str::to_owned),
        clamp_to_ground: v["clampToGround"].as_bool().unwrap_or(false),
        ..Default::default()
    };
    for (key, dest) in [("stroke", &mut o.stroke), ("fill", &mut o.fill), ("markerColor", &mut o.marker_color)] {
        if let Some(css) = v[key].as_str() {
            *dest = Color::from_css_color_string(css).unwrap();
        }
    }
    o.stroke_width = v["strokeWidth"].as_f64().unwrap_or(o.stroke_width);
    o.marker_size = v["markerSize"].as_f64().unwrap_or(o.marker_size);
    o.marker_symbol = v["markerSymbol"].as_str().map(str::to_owned);
    o
}
fn evaluated(p: Option<PropertyInput>) -> Value {
    match p {
        Some(PropertyInput::Component(c)) => {
            c.get_value(&time(), None).unwrap().as_ref().map(encode).unwrap_or_default()
        }
        Some(PropertyInput::Value(v)) => encode(&v),
        None => Value::Null,
    }
}
fn graphics(e: &Entity, name: &str, fields: &[&str]) -> Value {
    let Some(c) = e.get_property(name).and_then(|p| p.component()) else {
        return Value::Null;
    };
    Value::Object(fields.iter().map(|&k| (k.into(), evaluated(c.get_property(k)))).collect())
}
#[derive(Default)]
struct Ids(RefCell<HashMap<String, String>>);
impl Ids {
    fn id(&self, e: &Entity) -> String {
        let id = e.id();
        if id.len() == 36 && id.as_bytes()[8] == b'-' {
            let mut ids = self.0.borrow_mut();
            let n = ids.len();
            ids.entry(id.into()).or_insert_with(|| format!("@{n}")).clone()
        } else {
            id.into()
        }
    }
    fn snapshot(&self, e: &Entity) -> Value {
        json!({"id":self.id(e),"name":e.name(),"description":evaluated(e.get_property("description")),
          "properties":evaluated(e.get_property("properties")),"position":e.position_at(&time()).unwrap().enc(),
          "billboard":graphics(e,"billboard",&["image","verticalOrigin","heightReference"]),
          "polyline":graphics(e,"polyline",&["positions","material","arcType","width","clampToGround"]),
          "polygon":graphics(e,"polygon",&["hierarchy","material","arcType","outline","outlineColor","outlineWidth","perPositionHeight","height"])})
    }
}
#[test]
fn geo_json_and_topology() {
    run(
        "data_sources",
        "GeoJsonDataSource",
        |_| TIGHT,
        |_, a| {
            let s = GeoJsonDataSource::new(Some("initial".into()));
            let ids = Rc::new(Ids::default());
            let trace = Trace::default();
            let t = trace.clone();
            let _name = s.changed_event().add_listener(move |s| t.borrow_mut().push(json!(["name", s.name()])));
            let t = trace.clone();
            let _error = s.error_event().add_listener(move |(_, e)| t.borrow_mut().push(json!(["error", e.message])));
            let t = trace.clone();
            let _loading = s.loading_event().add_listener(move |(_, v)| t.borrow_mut().push(json!(["loading", v])));
            let t = trace.clone();
            let labels = ids.clone();
            let _entities = s.entities().collection_changed().add_listener(move |e| {
                let names = |v: &[EntityRef]| v.iter().map(|e| labels.id(e)).collect::<Vec<_>>();
                t.borrow_mut().push(json!(["entities", names(&e.added), names(&e.removed), names(&e.changed)]));
            });
            let mut results = Vec::new();
            for op in a[0].as_array().unwrap() {
                let o = options(&op["options"]);
                let data = op["data"].clone();
                let result = if op["method"] == "process" {
                    s.process(data, &o).now_or_never()
                } else {
                    s.load(data, &o).now_or_never()
                }
                .unwrap();
                results.push(json!({"name":s.name(),"entities":s.entities().values().iter().map(|e|ids.snapshot(e)).collect::<Vec<_>>(),
                "trace":trace.take(),"error":result.err().map(|e|e.message)}));
            }
            ok(json!(results))
        },
    );
}

#[test]
fn resource_and_text_inputs() {
    use cesium_rs::core::resource::{HttpRequest, HttpResponse, ResourceTransport};
    struct Transport;
    impl ResourceTransport for Transport {
        fn execute(&self, req: HttpRequest) -> RequestFuture<HttpResponse> {
            assert_eq!(req.url, "https://fixture.test/one.geojson?a=1");
            Box::pin(async {
                Ok(HttpResponse {
                    status: 200,
                    body: br#"{"type":"Feature","geometry":null,"id":"remote"}"#.to_vec(),
                    headers: vec![],
                })
            })
        }
    }
    let s = GeoJsonDataSource::new(None);
    let options =
        GeoJsonOptions { resource_client: Some(ResourceClient::new(Rc::new(Transport))), ..Default::default() };
    let url = "https://fixture.test/one.geojson?a=1";
    s.load(Resource::new(url).unwrap(), &options).now_or_never().unwrap().unwrap();
    assert_eq!(s.name().as_deref(), Some("one.geojson"));
    assert!(s.entities().get_by_id("remote").is_some());
    s.load(r#"{"type":"Feature","id":"local","geometry":null}"#, &options).now_or_never().unwrap().unwrap();
    assert!(s.entities().get_by_id("remote").is_none());
    assert!(s.entities().get_by_id("local").is_some());
    s.load(url, &options).now_or_never().unwrap().unwrap();
    assert!(s.entities().get_by_id("remote").is_some());
    assert!(s.entities().get_by_id("local").is_none());
}

#[test]
fn dropping_a_pending_crs_load_resumes_entity_events() {
    let mut registry = CrsRegistry::default();
    registry.link_hrefs.insert("pending".into(), Rc::new(|_| Box::pin(std::future::pending())));
    let opts = GeoJsonOptions { crs: Rc::new(registry), ..Default::default() };
    let s = GeoJsonDataSource::new(None);
    let doc = json!({"type":"Feature","geometry":null,"crs":{"type":"link","properties":{"href":"pending"}}});
    let mut load = Box::pin(s.load(doc, &opts));
    assert!(load.as_mut().now_or_never().is_none());
    assert!(s.is_loading());
    let seen = Rc::new(Cell::new(0));
    let events = seen.clone();
    let _sub = s.entities().collection_changed().add_listener(move |c| events.set(events.get() + c.added.len()));
    s.process(json!({"type":"Feature","geometry":null,"id":"nested"}), &opts).now_or_never().unwrap().unwrap();
    assert!(s.is_loading());
    assert_eq!(seen.get(), 0);
    drop(load);
    assert!(!s.is_loading());
    assert_eq!(seen.get(), 1);
}

#[test]
fn clear_cancels_pending_add_and_returns_the_resolved_source() {
    let collection = DataSourceCollection::new();
    let source: DataSourceRef = CustomDataSource::new(None);
    let add = collection.add_async(std::future::ready(Ok(source.clone())));
    collection.remove_all(false);
    let resolved = add.now_or_never().unwrap().unwrap();
    assert!(Rc::ptr_eq(&resolved, &source));
    assert_eq!(collection.length(), 0);
    collection.add_async(std::future::ready(Ok(source.clone()))).now_or_never().unwrap().unwrap();
    assert!(collection.contains(&source));
}

#[test]
fn removed_during_factory_creation_destroys_visualizers_once() {
    let collection = DataSourceCollection::new();
    let weak = Rc::downgrade(&collection);
    let trace = Trace::default();
    let events = trace.clone();
    let display = DataSourceDisplay::new(
        collection.clone(),
        Rc::new(move |source| {
            if source.name().is_none() {
                return vec![];
            }
            weak.upgrade().unwrap().remove(source, false);
            vec![Box::new(TestVisualizer {
                name: "removed".into(),
                i: 0,
                ready: Rc::new(Cell::new(true)),
                pending: Rc::new(Cell::new(false)),
                trace: events.clone(),
                destroyed: false,
            })]
        }),
    );
    collection.add(CustomDataSource::new(Some("temporary".into()))).unwrap();
    assert!(display.update(&time()).unwrap());
    display.destroy();
    assert_eq!(json!(trace.take()), json!([["destroy", "removed", 0]]));
}
