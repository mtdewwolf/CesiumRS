mod common;

use cesium_rs::{core::*, data_sources::*};
use common::{num, ok, run, TIGHT};
use futures_util::FutureExt;
use serde_json::{json, Map, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

fn encode_val(v: &DynamicValue) -> Value {
    match v {
        DynamicValue::Record(entries) => Value::Object(
            entries.iter().filter_map(|(k, val)| val.as_ref().map(|val| (k.clone(), encode_val(val)))).collect(),
        ),
        DynamicValue::Number(x) => num(*x),
        other => other.to_json(),
    }
}

fn is_guid(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 36
        && bytes[8] == b'-'
        && bytes[13] == b'-'
        && bytes[18] == b'-'
        && bytes[23] == b'-'
        && bytes.iter().all(|b| b.is_ascii_hexdigit() || *b == b'-')
}

fn eval_input(input: Option<PropertyInput>, time: &JulianDate) -> Value {
    match input {
        None => Value::Null,
        Some(PropertyInput::Value(v)) => encode_val(&v),
        Some(PropertyInput::Component(c)) => c.get_value(time, None).unwrap().as_ref().map_or(Value::Null, encode_val),
    }
}

fn eval_material(input: Option<PropertyInput>, time: &JulianDate) -> Value {
    match input {
        Some(PropertyInput::Component(c)) => {
            let kind = c.get_type(time).unwrap();
            let value = c.get_value(time, None).unwrap().as_ref().map_or(Value::Null, encode_val);
            json!({ "type": kind, "value": value })
        }
        _ => Value::Null,
    }
}

fn eval_graphics<G: Graphics + 'static>(
    entity: &EntityRef,
    slot_name: &str,
    keys: &[&str],
    material_keys: &[&str],
    time: &JulianDate,
) -> Value {
    let Some(PropertyInput::Component(c)) = entity.get_property(slot_name) else {
        return Value::Null;
    };
    let g = c.as_any().downcast_ref::<G>().unwrap();
    let mut map = Map::new();
    for &k in keys {
        let v = if material_keys.contains(&k) { eval_material(g.get(k), time) } else { eval_input(g.get(k), time) };
        map.insert(k.to_owned(), v);
    }
    Value::Object(map)
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

fn snapshot_entity(e: &EntityRef, time: &JulianDate, norm_id: &mut impl FnMut(&str) -> String) -> Value {
    let availability = e.availability().map(|a| {
        let r = a.read();
        Value::Array(
            r.intervals().iter().map(|iv| json!([iv.start.to_iso8601(None), iv.stop.to_iso8601(None)])).collect(),
        )
    });

    let position = e
        .position()
        .and_then(|p| p.get_value(Some(time), None).unwrap())
        .map(|p| json!(p.to_array()))
        .unwrap_or(Value::Null);

    let position_inertial = e
        .position()
        .and_then(|p| p.get_value_in_reference_frame(time, ReferenceFrame::Inertial, None).unwrap())
        .map(|p| json!(p.to_array()))
        .unwrap_or(Value::Null);

    let model = match e.get_property("model") {
        Some(PropertyInput::Component(c)) => {
            let m = c.as_any().downcast_ref::<ModelGraphics>().unwrap();
            let mut map = Map::new();
            for k in [
                "show",
                "uri",
                "scale",
                "minimumPixelSize",
                "maximumScale",
                "incrementallyLoadTextures",
                "runAnimations",
                "clampAnimations",
                "shadows",
                "heightReference",
                "silhouetteColor",
                "silhouetteSize",
                "color",
                "colorBlendMode",
                "colorBlendAmount",
                "distanceDisplayCondition",
                "nodeTransformations",
                "articulations",
            ] {
                map.insert(k.to_owned(), eval_input(m.get(k), time));
            }
            Value::Object(map)
        }
        _ => Value::Null,
    };

    json!({
        "id": norm_id(e.id()),
        "name": e.name(),
        "parent": e.parent().map(|p| norm_id(p.id())),
        "description": eval_input(e.get_property("description"), time),
        "availability": availability,
        "position": position,
        "positionInertial": position_inertial,
        "orientation": eval_input(e.get_property("orientation"), time),
        "viewFrom": eval_input(e.get_property("viewFrom"), time),
        "properties": eval_input(e.get_property("properties"), time),
        "billboard": eval_graphics::<BillboardGraphics>(e, "billboard", &[
            "show", "image", "scale", "pixelOffset", "eyeOffset", "horizontalOrigin", "verticalOrigin",
            "heightReference", "color", "rotation", "alignedAxis", "sizeInMeters", "width", "height",
            "scaleByDistance", "translucencyByDistance", "pixelOffsetScaleByDistance", "imageSubRegion",
            "distanceDisplayCondition", "disableDepthTestDistance",
        ], &[], time),
        "box": eval_graphics::<BoxGraphics>(e, "box", &[
            "show", "dimensions", "heightReference", "fill", "material", "outline", "outlineColor",
            "outlineWidth", "shadows", "distanceDisplayCondition",
        ], &["material"], time),
        "corridor": eval_graphics::<CorridorGraphics>(e, "corridor", &[
            "show", "positions", "width", "height", "heightReference", "extrudedHeight",
            "extrudedHeightReference", "cornerType", "granularity", "fill", "material", "outline",
            "outlineColor", "outlineWidth", "shadows", "distanceDisplayCondition", "classificationType", "zIndex",
        ], &["material"], time),
        "cylinder": eval_graphics::<CylinderGraphics>(e, "cylinder", &[
            "show", "length", "topRadius", "bottomRadius", "heightReference", "fill", "material",
            "outline", "outlineColor", "outlineWidth", "numberOfVerticalLines", "slices", "shadows",
            "distanceDisplayCondition",
        ], &["material"], time),
        "ellipse": eval_graphics::<EllipseGraphics>(e, "ellipse", &[
            "show", "semiMajorAxis", "semiMinorAxis", "height", "heightReference", "extrudedHeight",
            "extrudedHeightReference", "rotation", "stRotation", "granularity", "fill", "material",
            "outline", "outlineColor", "outlineWidth", "numberOfVerticalLines", "shadows",
            "distanceDisplayCondition", "classificationType", "zIndex",
        ], &["material"], time),
        "ellipsoid": eval_graphics::<EllipsoidGraphics>(e, "ellipsoid", &[
            "show", "radii", "innerRadii", "minimumClock", "maximumClock", "minimumCone", "maximumCone",
            "heightReference", "fill", "material", "outline", "outlineColor", "outlineWidth",
            "stackPartitions", "slicePartitions", "subdivisions", "shadows", "distanceDisplayCondition",
        ], &["material"], time),
        "label": eval_graphics::<LabelGraphics>(e, "label", &[
            "show", "text", "font", "style", "scale", "showBackground", "backgroundColor",
            "backgroundPadding", "pixelOffset", "eyeOffset", "horizontalOrigin", "verticalOrigin",
            "heightReference", "fillColor", "outlineColor", "outlineWidth", "translucencyByDistance",
            "pixelOffsetScaleByDistance", "scaleByDistance", "distanceDisplayCondition",
            "disableDepthTestDistance",
        ], &[], time),
        "model": model,
        "path": eval_graphics::<PathGraphics>(e, "path", &[
            "show", "leadTime", "trailTime", "width", "resolution", "material", "distanceDisplayCondition",
        ], &["material"], time),
        "point": eval_graphics::<PointGraphics>(e, "point", &[
            "show", "pixelSize", "heightReference", "color", "outlineColor", "outlineWidth",
            "scaleByDistance", "translucencyByDistance", "distanceDisplayCondition", "disableDepthTestDistance",
        ], &[], time),
        "polygon": eval_graphics::<PolygonGraphics>(e, "polygon", &[
            "show", "hierarchy", "height", "heightReference", "extrudedHeight", "extrudedHeightReference",
            "stRotation", "granularity", "fill", "material", "outline", "outlineColor", "outlineWidth",
            "perPositionHeight", "closeTop", "closeBottom", "arcType", "shadows", "distanceDisplayCondition",
            "classificationType", "zIndex",
        ], &["material"], time),
        "polyline": eval_graphics::<PolylineGraphics>(e, "polyline", &[
            "show", "positions", "width", "granularity", "material", "depthFailMaterial", "arcType",
            "clampToGround", "shadows", "distanceDisplayCondition", "classificationType", "zIndex",
        ], &["material", "depthFailMaterial"], time),
        "polylineVolume": eval_graphics::<PolylineVolumeGraphics>(e, "polylineVolume", &[
            "positions", "shape", "show", "cornerType", "fill", "material", "outline", "outlineColor",
            "outlineWidth", "granularity", "shadows", "distanceDisplayCondition",
        ], &["material"], time),
        "rectangle": eval_graphics::<RectangleGraphics>(e, "rectangle", &[
            "show", "coordinates", "height", "heightReference", "extrudedHeight", "extrudedHeightReference",
            "rotation", "stRotation", "granularity", "fill", "material", "outline", "outlineColor",
            "outlineWidth", "shadows", "distanceDisplayCondition", "classificationType", "zIndex",
        ], &["material"], time),
        "tileset": eval_graphics::<Cesium3DTilesetGraphics>(e, "tileset", &[
            "show", "uri", "maximumScreenSpaceError",
        ], &[], time),
        "wall": eval_graphics::<WallGraphics>(e, "wall", &[
            "show", "positions", "minimumHeights", "maximumHeights", "granularity", "fill", "material",
            "outline", "outlineColor", "outlineWidth", "shadows", "distanceDisplayCondition",
        ], &["material"], time),
    })
}

#[test]
fn replay_czml_data_source_fixtures() {
    run(
        "data_sources",
        "CzmlDataSource",
        |_| TIGHT,
        |_, args| {
            let initial_name = args[0].as_str().map(str::to_owned);
            let ops = args[1].as_array().unwrap();
            let sample_times = args[2].as_array().unwrap();

            let ds = CzmlDataSource::new(initial_name);
            let changed = Rc::new(Cell::new(0usize));
            let loading = Rc::new(RefCell::new(Vec::<bool>::new()));
            let errors = Rc::new(RefCell::new(Vec::<String>::new()));

            let c = changed.clone();
            let _s1 = ds.changed_event().add_listener(move |_| c.set(c.get() + 1));
            let l = loading.clone();
            let _s2 = ds.loading_event().add_listener(move |(_, v)| l.borrow_mut().push(*v));
            let er = errors.clone();
            let _s3 = ds.error_event().add_listener(move |(_, e)| er.borrow_mut().push(e.message.clone()));

            let mut auto_ids: HashMap<String, String> = HashMap::new();
            let mut norm_id = |id: &str| -> String {
                if !is_guid(id) {
                    return id.to_owned();
                }
                let next = auto_ids.len();
                auto_ids.entry(id.to_owned()).or_insert_with(|| format!("@{next}")).clone()
            };

            let mut steps = Vec::with_capacity(ops.len());
            for op in ops {
                let method = op["method"].as_str().unwrap();
                let czml = op["czml"].clone();
                let mut options = CzmlLoadOptions::default();
                if let Some(opts) = op.get("options").and_then(Value::as_object) {
                    if let Some(uri) = opts.get("sourceUri").and_then(Value::as_str) {
                        options.source_uri = Some(uri.to_owned());
                    }
                    if let Some(credit) = opts.get("credit").and_then(Value::as_str) {
                        options.credit = Some(Credit::new(credit, false));
                    }
                }
                let res = match method {
                    "load" => ds.load(czml, &options).now_or_never().unwrap(),
                    "process" => ds.process(czml, &options).now_or_never().unwrap(),
                    other => panic!("unknown czml method {other}"),
                };
                let error = res.err().map(|e| e.message);

                let mut frames = Map::new();
                for iso_val in sample_times {
                    let iso = iso_val.as_str().unwrap();
                    let jd = JulianDate::from_iso8601(iso).unwrap();
                    let entities: Vec<Value> =
                        ds.entities().values().iter().map(|e| snapshot_entity(e, &jd, &mut norm_id)).collect();
                    frames.insert(iso.to_owned(), Value::Array(entities));
                }

                steps.push(json!({
                    "name": ds.name(),
                    "clock": encode_clock(ds.clock()),
                    "credit": ds.credit().map(|c| c.html().to_owned()),
                    "isLoading": ds.is_loading(),
                    "error": error,
                    "frames": Value::Object(frames),
                }));
            }

            ok(json!({
                "steps": steps,
                "trace": {
                    "changed": changed.get(),
                    "loading": loading.borrow().clone(),
                    "errors": errors.borrow().clone(),
                },
            }))
        },
    );
}

#[test]
fn custom_updater_and_static_helpers() {
    let ds = CzmlDataSource::new(Some("Custom".into()));
    let updater = CzmlUpdater::new(|entity, packet, entities, source_uri| {
        if let Some(custom) = packet.get("customBadge") {
            let bag = Rc::new(PropertyBag::new());
            CzmlDataSource::process_packet_data(
                CzmlValueType::String,
                &*bag,
                "badge",
                custom.get("label"),
                None,
                source_uri,
                entities,
            )?;
            CzmlDataSource::process_packet_data(
                CzmlValueType::Uri,
                &*bag,
                "icon",
                custom.get("icon"),
                None,
                source_uri,
                entities,
            )?;
            entity.set_property("properties", Some(PropertyInput::Component(bag)))?;
        }
        Ok(())
    });

    assert_eq!(ds.updaters().len(), 23);
    ds.register_updater(updater.clone());
    // Duplicate registration is ignored.
    ds.register_updater(updater.clone());
    assert_eq!(ds.updaters().len(), 24);

    let czml = json!([
        { "id": "document", "version": "1.0" },
        {
            "id": "e1",
            "customBadge": { "label": "VIP", "icon": "badges/vip.png" }
        }
    ]);
    let opts = CzmlLoadOptions { source_uri: Some("https://example.test/app/data.czml".into()), ..Default::default() };
    ds.load(czml.clone(), &opts).now_or_never().unwrap().unwrap();

    let e1 = ds.entities().get_by_id("e1").unwrap();
    let jd = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
    let props = eval_input(e1.get_property("properties"), &jd);
    assert_eq!(props["badge"], json!("VIP"));
    assert_eq!(props["icon"], json!("https://example.test/app/badges/vip.png"));

    // Unregistering stops the custom updater from running on subsequent loads.
    assert!(ds.unregister_updater(&updater));
    assert!(!ds.unregister_updater(&updater));
    ds.load(czml, &opts).now_or_never().unwrap().unwrap();
    let e1_after = ds.entities().get_by_id("e1").unwrap();
    assert!(e1_after.get_property("properties").is_none());
}

#[test]
fn load_new_text_show_update_and_resource_transport() {
    struct StaticTransport(Vec<u8>);
    impl ResourceTransport for StaticTransport {
        fn execute(&self, _req: HttpRequest) -> RequestFuture<HttpResponse> {
            let body = self.0.clone();
            Box::pin(async move { Ok(HttpResponse { status: 200, body, headers: vec![] }) })
        }
    }

    let text = r#"[{"id":"document","version":"1.0","name":"FromText"},{"id":"a","name":"Alpha"}]"#;
    let ds = CzmlDataSource::load_new(text, &CzmlLoadOptions::default()).now_or_never().unwrap().unwrap();
    assert_eq!(ds.name().as_deref(), Some("FromText"));
    assert_eq!(ds.version().as_deref(), Some("1.0"));
    assert!(ds.show());
    ds.set_show(false).unwrap();
    assert!(!ds.show());
    assert!(ds.update(&JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap()).unwrap());

    // Fetch via Resource + ResourceClient with resource credits.
    let client = ResourceClient::new(Rc::new(StaticTransport(text.as_bytes().to_vec())));
    let res = Resource::from_options(cesium_rs::core::resource::ResourceOptions {
        url: "https://example.test/remote/scene.czml".into(),
        credits: Some(vec![Credit::new("Remote Attribution", false)]),
        ..Default::default()
    })
    .unwrap();
    let ds2 = CzmlDataSource::load_new(res, &CzmlLoadOptions { resource_client: Some(client), ..Default::default() })
        .now_or_never()
        .unwrap()
        .unwrap();
    assert_eq!(ds2.name().as_deref(), Some("FromText"));
    assert_eq!(ds2.source_uri().unwrap().url(), "https://example.test/remote/scene.czml");
    assert_eq!(ds2.resource_credits().len(), 1);
    assert_eq!(ds2.resource_credits()[0].html(), "Remote Attribution");
}
