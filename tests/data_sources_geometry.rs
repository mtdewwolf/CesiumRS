mod common;
mod entity_common;

use cesium_rs::{core::*, data_sources::*, scene::*};
use common::{num, ok, run, Enc, EXACT, TIGHT};
use entity_common::date;
use serde_json::{json, Value};
use std::{cell::RefCell, rc::Rc};

fn time0() -> JulianDate {
    date(0.0)
}

fn time1() -> JulianDate {
    date(10.0)
}

fn make_scene(max_line_width: f64) -> Rc<BasicGeometryScene> {
    let scene = BasicGeometryScene::new();
    scene.set_max_line_width(max_line_width);
    scene.set_height_sampler(Some(Rc::new(|_, _| Some(0.0))));
    scene
}

fn new_entity(id: &str) -> EntityRef {
    Entity::new(EntityOptions { id: Some(id.into()), ..Default::default() }).unwrap()
}

fn set_pos(entity: &EntityRef, pos: Cartesian3) {
    entity.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed)))).unwrap();
}

fn dyn_callback<T: Clone + 'static>(
    f: impl Fn(&JulianDate) -> Option<T> + 'static,
    to_dyn: impl Fn(T) -> DynamicValue + 'static,
) -> PropertyInput {
    let cb: PropertyRef<DynamicValue> =
        Rc::new(CallbackProperty::new(Rc::new(move |time, _| Ok(f(time).map(&to_dyn))), false));
    PropertyInput::Component(PropertyComponent::wrap(cb))
}

fn color_mat(color: Color) -> ComponentRef {
    Rc::new(ColorMaterialProperty::new(Some(PropertyInput::value(color))).unwrap())
}

fn rect_val(west_deg: f64, south_deg: f64, east_deg: f64, north_deg: f64) -> PropertyInput {
    let r = Rectangle::from_degrees(west_deg, south_deg, east_deg, north_deg);
    PropertyInput::value(DynamicValue::Cartesian4(Cartesian4::new(r.west, r.south, r.east, r.north)))
}

fn summarize_updater(u: &dyn GeometryUpdater, t: &JulianDate) -> Value {
    let z = u.z_index().and_then(|slot| match slot {
        PropertyInput::Value(DynamicValue::Number(n)) => Some(n),
        PropertyInput::Component(c) => match c.get_value(t, None).ok().flatten() {
            Some(DynamicValue::Number(n)) => Some(n),
            _ => None,
        },
        _ => None,
    });
    json!({
        "id": u.id(),
        "fillEnabled": u.fill_enabled(),
        "hasConstantFill": u.has_constant_fill(),
        "outlineEnabled": u.outline_enabled(),
        "hasConstantOutline": u.has_constant_outline(),
        "outlineWidth": u.outline_width(),
        "isDynamic": u.is_dynamic(),
        "isClosed": u.is_closed(),
        "onTerrain": u.on_terrain(),
        "clampToGround": u.clamp_to_ground(),
        "isFilled": u.is_filled(t).unwrap(),
        "isOutlineVisible": u.is_outline_visible(t).unwrap(),
        "hasTerrainOffset": u.terrain_offset_property().is_some(),
        "zIndex": z,
    })
}

fn summarize_instance(inst: &GeometryInstance) -> Value {
    let mut show = Value::Null;
    let mut color = Value::Null;
    let mut depth_fail_color = Value::Null;
    let mut ddc = Value::Null;
    let mut offset = Value::Null;
    for (name, attr) in &inst.attributes {
        match name.as_str() {
            "show" => {
                if let TypedArray::Uint8(v) = &attr.value {
                    show = json!(v[0]);
                }
            }
            "color" => {
                if let TypedArray::Uint8(v) = &attr.value {
                    color = json!(v);
                }
            }
            "depthFailColor" => {
                if let TypedArray::Uint8(v) = &attr.value {
                    depth_fail_color = json!(v);
                }
            }
            "distanceDisplayCondition" => {
                if let TypedArray::Float32(v) = &attr.value {
                    ddc = Value::Array(v.iter().map(|&x| num(f64::from(x))).collect());
                }
            }
            "offset" => {
                if let TypedArray::Float32(v) = &attr.value {
                    offset = Value::Array(v.iter().map(|&x| num(f64::from(x))).collect());
                }
            }
            _ => {}
        }
    }
    let (has_geometry, vertex_count, index_count, bs) = match &inst.geometry {
        Some(g) => {
            let vc = g.attributes.position.as_ref().map_or(0, |p| p.values.len() / 3);
            let ic = g.indices.as_ref().map_or(0, |i| i.len());
            let bs = g.bounding_sphere.map(|b| b.enc()).unwrap_or(Value::Null);
            (true, vc, ic, bs)
        }
        None => (false, 0, 0, Value::Null),
    };
    json!({
        "id": inst.id,
        "modelMatrix": inst.model_matrix.enc(),
        "show": show,
        "color": color,
        "depthFailColor": depth_fail_color,
        "ddc": ddc,
        "offset": offset,
        "hasGeometry": has_geometry,
        "vertexCount": vertex_count,
        "indexCount": index_count,
        "boundingSphere": bs,
    })
}

fn c3_deg(lon: f64, lat: f64, h: f64) -> Cartesian3 {
    Cartesian3::from_degrees(lon, lat, h, &Ellipsoid::WGS84)
}

fn c3_deg_array(coords: &[f64]) -> Vec<Cartesian3> {
    Cartesian3::from_degrees_array(coords, &Ellipsoid::WGS84)
}

fn c3_deg_heights_array(coords: &[f64]) -> Vec<Cartesian3> {
    Cartesian3::from_degrees_array_heights(coords, &Ellipsoid::WGS84)
}

fn make_box_entity(id: &str, pos: Cartesian3, dims: Cartesian3, color: Color) -> EntityRef {
    let e = new_entity(id);
    set_pos(&e, pos);
    let b = Rc::new(BoxGraphics::new());
    b.set_dimensions(Some(PropertyInput::value(dims))).unwrap();
    b.set_material(Some(PropertyInput::Component(color_mat(color)))).unwrap();
    e.set_property("box", Some(PropertyInput::Component(b))).unwrap();
    e
}

fn make_rect_entity(id: &str, w: f64, s: f64, east: f64, n: f64, mat: ComponentRef, z: f64) -> EntityRef {
    let e = new_entity(id);
    let r = Rc::new(RectangleGraphics::new());
    r.set_coordinates(Some(rect_val(w, s, east, n))).unwrap();
    r.set_material(Some(PropertyInput::Component(mat))).unwrap();
    r.set_z_index(Some(PropertyInput::value(z))).unwrap();
    e.set_property("rectangle", Some(PropertyInput::Component(r))).unwrap();
    e
}

#[test]
fn geometry_updater_fixture() {
    run(
        "data_sources",
        "GeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let e = new_entity("gu-1");
            set_pos(&e, c3_deg(-105.0, 40.0, 100.0));
            let b = Rc::new(BoxGraphics::new());
            b.set_dimensions(Some(PropertyInput::value(Cartesian3::new(10.0, 20.0, 30.0)))).unwrap();
            b.set_fill(Some(PropertyInput::value(true))).unwrap();
            b.set_outline(Some(PropertyInput::value(true))).unwrap();
            b.set_outline_width(Some(PropertyInput::value(2.0))).unwrap();
            b.set_outline_color(Some(PropertyInput::value(Color::YELLOW))).unwrap();
            b.set_material(Some(PropertyInput::Component(color_mat(Color::RED)))).unwrap();
            e.set_property("box", Some(PropertyInput::Component(b.clone()))).unwrap();

            let mut u = BoxGeometryUpdater::new(Rc::clone(&e), scene);
            let s0 = summarize_updater(&u, &time0());
            let fill0 = summarize_instance(&u.create_fill_geometry_instance(&time0()).unwrap());
            let out0 = summarize_instance(&u.create_outline_geometry_instance(&time0()).unwrap());

            b.set_show(Some(PropertyInput::value(false))).unwrap();
            GeometryUpdater::on_entity_property_changed(&mut u, &e, "box");
            let s_hidden = summarize_updater(&u, &time0());

            let t0 = time0();
            b.set_show(Some(dyn_callback(move |t| Some(*t == t0), DynamicValue::Boolean))).unwrap();
            GeometryUpdater::on_entity_property_changed(&mut u, &e, "box");
            let s_dyn0 = summarize_updater(&u, &time0());
            let s_dyn1 = summarize_updater(&u, &time1());

            b.set_show(Some(PropertyInput::value(true))).unwrap();
            b.set_fill(Some(PropertyInput::value(false))).unwrap();
            b.set_outline(Some(PropertyInput::value(false))).unwrap();
            GeometryUpdater::on_entity_property_changed(&mut u, &e, "box");
            let s_disabled = summarize_updater(&u, &time0());

            e.set_property("box", None).unwrap();
            GeometryUpdater::on_entity_property_changed(&mut u, &e, "box");
            let s_removed = summarize_updater(&u, &time0());

            u.destroy();
            ok(json!({
                "s0": s0,
                "fill0": fill0,
                "out0": out0,
                "sHidden": s_hidden,
                "sDynamicShow0": s_dyn0,
                "sDynamicShow1": s_dyn1,
                "sDisabled": s_disabled,
                "sRemoved": s_removed,
                "destroyed": u.is_destroyed(),
            }))
        },
    );
}

#[test]
fn ground_geometry_updater_fixture() {
    run(
        "data_sources",
        "GroundGeometryUpdater",
        |_| EXACT,
        |_, _| {
            let mut height_cases = Vec::new();
            for h in [None, Some(0.0), Some(50.0)] {
                for hr in [HeightReference::None, HeightReference::ClampToGround, HeightReference::RelativeToGround] {
                    let eh = match GroundGeometryUpdater::get_geometry_extruded_height(h, hr) {
                        Some(GroundExtrudedHeight::Height(v)) => json!(v),
                        Some(GroundExtrudedHeight::ClampToGround) => json!(GroundGeometryUpdater::CLAMP_TO_GROUND),
                        None => Value::Null,
                    };
                    height_cases.push(json!([
                        "height",
                        h,
                        hr.value(),
                        GroundGeometryUpdater::get_geometry_height(h, hr),
                        eh,
                    ]));
                }
            }
            let mut offset_cases = Vec::new();
            for h in [None, Some(10.0)] {
                for hr in [HeightReference::None, HeightReference::ClampToGround, HeightReference::RelativeToGround] {
                    for eh in [None, Some(30.0)] {
                        for ehr in
                            [HeightReference::None, HeightReference::ClampToGround, HeightReference::RelativeToGround]
                        {
                            let off =
                                GroundGeometryUpdater::compute_geometry_offset_attribute(h, Some(hr), eh, Some(ehr))
                                    .map(|a| a as u8);
                            offset_cases.push(json!([h, hr.value(), eh, ehr.value(), off]));
                        }
                    }
                }
            }
            ok(json!({
                "clampSentinel": GroundGeometryUpdater::CLAMP_TO_GROUND,
                "heightCases": height_cases,
                "offsetCases": offset_cases,
            }))
        },
    );
}

#[test]
fn dynamic_geometry_updater_fixture() {
    run(
        "data_sources",
        "DynamicGeometryUpdater",
        |_| EXACT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let primitives = Rc::new(RefCell::new(PrimitiveCollection::default()));
            let ground_primitives = Rc::new(RefCell::new(OrderedGroundPrimitiveCollection::default()));
            let e = new_entity("dyn-1");
            set_pos(&e, c3_deg(-105.0, 40.0, 100.0));
            let b = Rc::new(BoxGraphics::new());
            let t0 = time0();
            b.set_dimensions(Some(dyn_callback(
                move |t| {
                    Some(if *t == t0 { Cartesian3::new(10.0, 20.0, 30.0) } else { Cartesian3::new(40.0, 50.0, 60.0) })
                },
                DynamicValue::Cartesian3,
            )))
            .unwrap();
            b.set_material(Some(PropertyInput::Component(color_mat(Color::CYAN)))).unwrap();
            b.set_outline(Some(PropertyInput::value(true))).unwrap();
            b.set_outline_color(Some(PropertyInput::value(Color::BLACK))).unwrap();
            b.set_outline_width(Some(PropertyInput::value(3.0))).unwrap();
            e.set_property("box", Some(PropertyInput::Component(b))).unwrap();

            let mut updater = BoxGeometryUpdater::new(Rc::clone(&e), scene);
            let is_dynamic = GeometryUpdater::is_dynamic(&updater);
            let mut dyn_u =
                updater.create_dynamic_updater(Rc::clone(&primitives), Rc::clone(&ground_primitives)).unwrap();
            dyn_u.update(&time0()).unwrap();
            let after0 = json!([primitives.borrow().length(), ground_primitives.borrow().length()]);
            dyn_u.update(&time1()).unwrap();
            let after1 = json!([primitives.borrow().length(), ground_primitives.borrow().length()]);
            e.set_show(false);
            dyn_u.update(&time1()).unwrap();
            let after_hidden = json!([primitives.borrow().length(), ground_primitives.borrow().length()]);
            dyn_u.destroy();
            let after_destroy =
                json!([primitives.borrow().length(), ground_primitives.borrow().length(), dyn_u.is_destroyed()]);
            updater.destroy();
            ok(json!({
                "isDynamic": is_dynamic,
                "after0": after0,
                "after1": after1,
                "afterHidden": after_hidden,
                "afterDestroy": after_destroy,
            }))
        },
    );
}

#[test]
fn dynamic_geometry_batch_fixture() {
    run(
        "data_sources",
        "DynamicGeometryBatch",
        |_| EXACT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let primitives = Rc::new(RefCell::new(PrimitiveCollection::default()));
            let ground_primitives = Rc::new(RefCell::new(OrderedGroundPrimitiveCollection::default()));
            let mut batch = DynamicGeometryBatch::new(Rc::clone(&primitives), Rc::clone(&ground_primitives));

            let e1 = new_entity("db-1");
            set_pos(&e1, c3_deg(-105.0, 40.0, 100.0));
            let b = Rc::new(BoxGraphics::new());
            b.set_dimensions(Some(dyn_callback(|_| Some(Cartesian3::new(10.0, 10.0, 10.0)), DynamicValue::Cartesian3)))
                .unwrap();
            b.set_material(Some(PropertyInput::Component(color_mat(Color::RED)))).unwrap();
            e1.set_property("box", Some(PropertyInput::Component(b))).unwrap();

            let e2 = new_entity("db-2");
            let r = Rc::new(RectangleGraphics::new());
            r.set_coordinates(Some(dyn_callback(
                |_| {
                    let rect = Rectangle::from_degrees(-110.0, 30.0, -100.0, 40.0);
                    Some(Cartesian4::new(rect.west, rect.south, rect.east, rect.north))
                },
                DynamicValue::Cartesian4,
            )))
            .unwrap();
            r.set_material(Some(PropertyInput::Component(color_mat(Color::BLUE)))).unwrap();
            e2.set_property("rectangle", Some(PropertyInput::Component(r))).unwrap();

            let mut u1 = BoxGeometryUpdater::new(e1, scene.clone());
            let mut u2 = RectangleGeometryUpdater::new(e2, scene);

            batch.add(&time0(), &u1).unwrap();
            batch.add(&time0(), &u2).unwrap();
            let updated = batch.update(&time0()).unwrap();
            let counts_after_add = json!([primitives.borrow().length(), ground_primitives.borrow().length()]);

            batch.remove(&u1);
            let counts_after_remove1 = json!([primitives.borrow().length(), ground_primitives.borrow().length()]);

            batch.remove_all_primitives();
            let counts_after_clear = json!([primitives.borrow().length(), ground_primitives.borrow().length()]);

            u1.destroy();
            u2.destroy();
            ok(json!({
                "updated": updated,
                "countsAfterAdd": counts_after_add,
                "countsAfterRemove1": counts_after_remove1,
                "countsAfterClear": counts_after_clear,
            }))
        },
    );
}

#[test]
fn geometry_updater_set_fixture() {
    run(
        "data_sources",
        "GeometryUpdaterSet",
        |_| EXACT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let e =
                make_box_entity("set-1", c3_deg(-105.0, 40.0, 100.0), Cartesian3::new(10.0, 20.0, 30.0), Color::WHITE);
            let r = Rc::new(RectangleGraphics::new());
            r.set_coordinates(Some(rect_val(-106.0, 39.0, -104.0, 41.0))).unwrap();
            e.set_property("rectangle", Some(PropertyInput::Component(r))).unwrap();

            let mut set = GeometryUpdaterSet::new(Rc::clone(&e), scene);
            let changed_ids: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
            let cid = Rc::clone(&changed_ids);
            let _sub = set.geometry_changed().add_listener(move |ev| cid.borrow_mut().push(ev.updater_id.clone()));

            let initial_count = set.updaters().len();
            let active_ids = |s: &GeometryUpdaterSet| -> Vec<String> {
                s.updaters()
                    .iter()
                    .filter(|u| {
                        let u = u.borrow();
                        u.fill_enabled() || u.outline_enabled()
                    })
                    .map(|u| u.borrow().id().to_owned())
                    .collect()
            };
            let active_before = active_ids(&set);

            let w = Rc::new(WallGraphics::new());
            w.set_positions(Some(PropertyInput::value(DynamicValue::Positions(Rc::new(c3_deg_heights_array(&[
                -105.0, 40.0, 100.0, -104.0, 40.0, 100.0,
            ]))))))
            .unwrap();
            e.set_property("wall", Some(PropertyInput::Component(w))).unwrap();
            let active_after_wall = active_ids(&set);

            e.set_property("box", None).unwrap();
            let active_after_remove_box = active_ids(&set);

            set.destroy();
            ok(json!({
                "initialCount": initial_count,
                "activeBefore": active_before,
                "activeAfterWall": active_after_wall,
                "activeAfterRemoveBox": active_after_remove_box,
                "changedIds": *changed_ids.borrow(),
                "destroyed": set.is_destroyed(),
            }))
        },
    );
}

#[test]
fn static_geometry_color_batch_fixture() {
    run(
        "data_sources",
        "StaticGeometryColorBatch",
        |_| EXACT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let primitives = Rc::new(RefCell::new(PrimitiveCollection::default()));
            let mut batch = StaticGeometryColorBatch::new(
                Rc::clone(&primitives),
                GeometryAppearanceKind::PerInstanceColor,
                None,
                true,
                ShadowMode::Disabled,
            );
            let e1 =
                make_box_entity("cb-s1", c3_deg(-105.0, 40.0, 100.0), Cartesian3::new(10.0, 10.0, 10.0), Color::RED);
            let e2 =
                make_box_entity("cb-s2", c3_deg(-104.0, 40.0, 100.0), Cartesian3::new(10.0, 10.0, 10.0), Color::GREEN);
            let e3 = make_box_entity(
                "cb-t1",
                c3_deg(-103.0, 40.0, 100.0),
                Cartesian3::new(10.0, 10.0, 10.0),
                Color::BLUE.with_alpha(0.5),
            );
            let u1: GeometryUpdaterRef = Rc::new(RefCell::new(BoxGeometryUpdater::new(e1, scene.clone())));
            let u2: GeometryUpdaterRef = Rc::new(RefCell::new(BoxGeometryUpdater::new(e2, scene.clone())));
            let u3: GeometryUpdaterRef = Rc::new(RefCell::new(BoxGeometryUpdater::new(e3, scene)));

            batch.add(&time0(), Rc::clone(&u1)).unwrap();
            batch.add(&time0(), Rc::clone(&u2)).unwrap();
            batch.add(&time0(), Rc::clone(&u3)).unwrap();
            let first_update = batch.update(&time0()).unwrap();
            let prims_after_add = primitives.borrow().length();
            let solid_buckets = batch.solid_batch_count();
            let trans_buckets = batch.translucent_batch_count();

            batch.remove(&*u3.borrow());
            let after_remove_trans = batch.update(&time0()).unwrap();
            let prims_after_remove_trans = primitives.borrow().length();

            batch.remove_all_primitives();
            let prims_after_clear = primitives.borrow().length();

            u1.borrow_mut().destroy();
            u2.borrow_mut().destroy();
            u3.borrow_mut().destroy();
            ok(json!({
                "firstUpdate": first_update,
                "primsAfterAdd": prims_after_add,
                "solidBuckets": solid_buckets,
                "transBuckets": trans_buckets,
                "afterRemoveTrans": after_remove_trans,
                "primsAfterRemoveTrans": prims_after_remove_trans,
                "primsAfterClear": prims_after_clear,
            }))
        },
    );
}

#[test]
fn static_geometry_per_material_batch_fixture() {
    run(
        "data_sources",
        "StaticGeometryPerMaterialBatch",
        |_| EXACT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let primitives = Rc::new(RefCell::new(PrimitiveCollection::default()));
            let mut batch = StaticGeometryPerMaterialBatch::new(
                Rc::clone(&primitives),
                GeometryAppearanceKind::Material,
                None,
                true,
                ShadowMode::Disabled,
            );
            let grid1 = Rc::new(GridMaterialProperty::new());
            grid1.set_color(Some(PropertyInput::value(Color::RED))).unwrap();
            let grid2 = Rc::new(GridMaterialProperty::new());
            grid2.set_color(Some(PropertyInput::value(Color::RED))).unwrap();
            let stripe = Rc::new(StripeMaterialProperty::new());
            stripe.set_even_color(Some(PropertyInput::value(Color::BLUE))).unwrap();
            stripe.set_odd_color(Some(PropertyInput::value(Color::WHITE))).unwrap();

            let make = |id: &str, lon: f64, mat: ComponentRef| -> GeometryUpdaterRef {
                let e = new_entity(id);
                set_pos(&e, c3_deg(lon, 40.0, 100.0));
                let b = Rc::new(BoxGraphics::new());
                b.set_dimensions(Some(PropertyInput::value(Cartesian3::new(10.0, 10.0, 10.0)))).unwrap();
                b.set_material(Some(PropertyInput::Component(mat))).unwrap();
                e.set_property("box", Some(PropertyInput::Component(b))).unwrap();
                Rc::new(RefCell::new(BoxGeometryUpdater::new(e, scene.clone())))
            };
            let u1 = make("mb-1", -105.0, grid1);
            let u2 = make("mb-2", -104.0, grid2);
            let u3 = make("mb-3", -103.0, stripe);

            batch.add(&time0(), Rc::clone(&u1)).unwrap();
            batch.add(&time0(), Rc::clone(&u2)).unwrap();
            batch.add(&time0(), Rc::clone(&u3)).unwrap();
            let first_update = batch.update(&time0()).unwrap();
            let item_count = batch.batch_count();
            let prims_after_add = primitives.borrow().length();

            batch.remove(&*u3.borrow());
            batch.update(&time0()).unwrap();
            let item_count_after_remove = batch.batch_count();
            let prims_after_remove = primitives.borrow().length();

            batch.remove_all_primitives();
            let prims_after_clear = primitives.borrow().length();

            u1.borrow_mut().destroy();
            u2.borrow_mut().destroy();
            u3.borrow_mut().destroy();
            ok(json!({
                "firstUpdate": first_update,
                "itemCount": item_count,
                "primsAfterAdd": prims_after_add,
                "itemCountAfterRemove": item_count_after_remove,
                "primsAfterRemove": prims_after_remove,
                "primsAfterClear": prims_after_clear,
            }))
        },
    );
}

#[test]
fn static_ground_geometry_color_batch_fixture() {
    run(
        "data_sources",
        "StaticGroundGeometryColorBatch",
        |_| EXACT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let gp = Rc::new(RefCell::new(OrderedGroundPrimitiveCollection::default()));
            let mut batch = StaticGroundGeometryColorBatch::new(Rc::clone(&gp), ClassificationType::Both);

            let entities = [
                make_rect_entity("gcb-1", -110.0, 30.0, -105.0, 35.0, color_mat(Color::RED), 0.0),
                make_rect_entity("gcb-2", -95.0, 30.0, -90.0, 35.0, color_mat(Color::GREEN), 0.0),
                make_rect_entity("gcb-3", -108.0, 32.0, -102.0, 37.0, color_mat(Color::BLUE), 0.0),
                make_rect_entity("gcb-4", -80.0, 30.0, -75.0, 35.0, color_mat(Color::YELLOW), 5.0),
            ];
            let updaters: Vec<GeometryUpdaterRef> = entities
                .into_iter()
                .map(|e| Rc::new(RefCell::new(RectangleGeometryUpdater::new(e, scene.clone()))) as GeometryUpdaterRef)
                .collect();
            for u in &updaters {
                batch.add(&time0(), Rc::clone(u)).unwrap();
            }
            let first_update = batch.update(&time0()).unwrap();
            let batch_count = batch.batch_count();
            let ground_count = gp.borrow().length();

            batch.remove(&*updaters[3].borrow());
            batch.update(&time0()).unwrap();
            let batch_count_after_remove = batch.batch_count();
            let ground_count_after_remove = gp.borrow().length();

            batch.remove_all_primitives();
            let ground_count_after_clear = gp.borrow().length();
            for u in updaters {
                u.borrow_mut().destroy();
            }
            ok(json!({
                "firstUpdate": first_update,
                "batchCount": batch_count,
                "groundCount": ground_count,
                "batchCountAfterRemove": batch_count_after_remove,
                "groundCountAfterRemove": ground_count_after_remove,
                "groundCountAfterClear": ground_count_after_clear,
            }))
        },
    );
}

#[test]
fn static_ground_geometry_per_material_batch_fixture() {
    run(
        "data_sources",
        "StaticGroundGeometryPerMaterialBatch",
        |_| EXACT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let gp = Rc::new(RefCell::new(OrderedGroundPrimitiveCollection::default()));
            let mut batch = StaticGroundGeometryPerMaterialBatch::new(
                Rc::clone(&gp),
                ClassificationType::Both,
                GeometryAppearanceKind::Material,
            );
            let grid_a = Rc::new(GridMaterialProperty::new());
            grid_a.set_color(Some(PropertyInput::value(Color::RED))).unwrap();
            let grid_b = Rc::new(GridMaterialProperty::new());
            grid_b.set_color(Some(PropertyInput::value(Color::RED))).unwrap();
            let stripe = Rc::new(StripeMaterialProperty::new());
            stripe.set_even_color(Some(PropertyInput::value(Color::GREEN))).unwrap();
            stripe.set_odd_color(Some(PropertyInput::value(Color::BLACK))).unwrap();

            let entities = [
                make_rect_entity("gmb-1", -110.0, 30.0, -108.0, 32.0, grid_a, 0.0),
                make_rect_entity("gmb-2", -95.0, 30.0, -93.0, 32.0, grid_b.clone(), 0.0),
                make_rect_entity("gmb-3", -109.0, 31.0, -107.0, 33.0, grid_b, 0.0),
                make_rect_entity("gmb-4", -85.0, 30.0, -83.0, 32.0, stripe, 0.0),
            ];
            let updaters: Vec<GeometryUpdaterRef> = entities
                .into_iter()
                .map(|e| Rc::new(RefCell::new(RectangleGeometryUpdater::new(e, scene.clone()))) as GeometryUpdaterRef)
                .collect();
            for u in &updaters {
                batch.add(&time0(), Rc::clone(u)).unwrap();
            }
            let first_update = batch.update(&time0()).unwrap();
            let batch_count = batch.batch_count();
            let ground_count = gp.borrow().length();

            batch.remove(&*updaters[3].borrow());
            batch.update(&time0()).unwrap();
            let batch_count_after_remove = batch.batch_count();

            batch.remove_all_primitives();
            let ground_count_after_clear = gp.borrow().length();
            for u in updaters {
                u.borrow_mut().destroy();
            }
            ok(json!({
                "firstUpdate": first_update,
                "batchCount": batch_count,
                "groundCount": ground_count,
                "batchCountAfterRemove": batch_count_after_remove,
                "groundCountAfterClear": ground_count_after_clear,
            }))
        },
    );
}

#[test]
fn static_ground_polyline_per_material_batch_fixture() {
    run(
        "data_sources",
        "StaticGroundPolylinePerMaterialBatch",
        |_| EXACT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let gp = Rc::new(RefCell::new(OrderedGroundPrimitiveCollection::default()));
            let mut batch =
                StaticGroundPolylinePerMaterialBatch::new(Rc::clone(&gp), ClassificationType::Both, Some(true));

            let make = |id: &str, coords: &[f64], mat: ComponentRef, z: f64| -> GeometryUpdaterRef {
                let e = new_entity(id);
                let pl = Rc::new(PolylineGraphics::new());
                pl.set_positions(Some(PropertyInput::value(DynamicValue::Positions(Rc::new(c3_deg_array(coords))))))
                    .unwrap();
                pl.set_clamp_to_ground(Some(PropertyInput::value(true))).unwrap();
                pl.set_material(Some(PropertyInput::Component(mat))).unwrap();
                pl.set_z_index(Some(PropertyInput::value(z))).unwrap();
                e.set_property("polyline", Some(PropertyInput::Component(pl))).unwrap();
                Rc::new(RefCell::new(PolylineGeometryUpdater::new(e, scene.clone())))
            };
            let dash = Rc::new(PolylineDashMaterialProperty::new());
            dash.set_color(Some(PropertyInput::value(Color::YELLOW))).unwrap();

            let updaters = [
                make("gpb-1", &[-110.0, 35.0, -109.0, 36.0], color_mat(Color::RED), 0.0),
                make("gpb-2", &[-105.0, 35.0, -104.0, 36.0], color_mat(Color::BLUE), 0.0),
                make("gpb-3", &[-100.0, 35.0, -99.0, 36.0], color_mat(Color::GREEN), 2.0),
                make("gpb-4", &[-100.0, 35.0, -99.0, 36.0], dash, 0.0),
            ];
            for u in &updaters {
                batch.add(&time0(), Rc::clone(u)).unwrap();
            }
            let first_update = batch.update(&time0()).unwrap();
            let batch_count = batch.batch_count();
            let ground_count = gp.borrow().length();

            batch.remove(&*updaters[3].borrow());
            batch.update(&time0()).unwrap();
            let batch_count_after_remove = batch.batch_count();

            batch.remove_all_primitives();
            let ground_count_after_clear = gp.borrow().length();
            for u in updaters {
                u.borrow_mut().destroy();
            }
            ok(json!({
                "firstUpdate": first_update,
                "batchCount": batch_count,
                "groundCount": ground_count,
                "batchCountAfterRemove": batch_count_after_remove,
                "groundCountAfterClear": ground_count_after_clear,
            }))
        },
    );
}

#[test]
fn static_outline_geometry_batch_fixture() {
    run(
        "data_sources",
        "StaticOutlineGeometryBatch",
        |_| EXACT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(4.0);
            let primitives = Rc::new(RefCell::new(PrimitiveCollection::default()));
            let mut batch =
                StaticOutlineGeometryBatch::new(Rc::clone(&primitives), scene.clone(), ShadowMode::Disabled);

            let make = |id: &str, lon: f64, width: f64, color: Color| -> GeometryUpdaterRef {
                let e = new_entity(id);
                set_pos(&e, c3_deg(lon, 40.0, 100.0));
                let b = Rc::new(BoxGraphics::new());
                b.set_dimensions(Some(PropertyInput::value(Cartesian3::new(10.0, 10.0, 10.0)))).unwrap();
                b.set_outline(Some(PropertyInput::value(true))).unwrap();
                b.set_outline_width(Some(PropertyInput::value(width))).unwrap();
                b.set_outline_color(Some(PropertyInput::value(color))).unwrap();
                e.set_property("box", Some(PropertyInput::Component(b))).unwrap();
                Rc::new(RefCell::new(BoxGeometryUpdater::new(e, scene.clone())))
            };
            let u1 = make("ob-1", -105.0, 2.0, Color::WHITE);
            let u2 = make("ob-2", -104.0, 8.0, Color::YELLOW);
            let u3 = make("ob-3", -103.0, 2.0, Color::CYAN.with_alpha(0.4));

            batch.add(&time0(), Rc::clone(&u1)).unwrap();
            batch.add(&time0(), Rc::clone(&u2)).unwrap();
            batch.add(&time0(), Rc::clone(&u3)).unwrap();
            let first_update = batch.update(&time0()).unwrap();
            let solid_widths = batch.solid_batch_count();
            let trans_widths = batch.translucent_batch_count();
            let prim_count = primitives.borrow().length();

            batch.remove(&*u2.borrow());
            batch.update(&time0()).unwrap();
            let prim_count_after_remove = primitives.borrow().length();

            batch.remove_all_primitives();
            let prim_count_after_clear = primitives.borrow().length();
            u1.borrow_mut().destroy();
            u2.borrow_mut().destroy();
            u3.borrow_mut().destroy();
            ok(json!({
                "firstUpdate": first_update,
                "solidWidths": solid_widths,
                "transWidths": trans_widths,
                "primCount": prim_count,
                "primCountAfterRemove": prim_count_after_remove,
                "primCountAfterClear": prim_count_after_clear,
            }))
        },
    );
}

#[test]
fn box_geometry_updater_fixture() {
    run(
        "data_sources",
        "BoxGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let pos = c3_deg(-105.0, 40.0, 500.0);

            let e_static = make_box_entity("box-static", pos, Cartesian3::new(100.0, 200.0, 300.0), Color::RED);
            if let Some(PropertyInput::Component(c)) = e_static.get_property("box") {
                let b = c.as_any().downcast_ref::<BoxGraphics>().unwrap();
                b.set_outline(Some(PropertyInput::value(true))).unwrap();
                b.set_outline_color(Some(PropertyInput::value(Color::BLACK))).unwrap();
                b.set_outline_width(Some(PropertyInput::value(2.0))).unwrap();
            }
            let mut u_static = BoxGeometryUpdater::new(e_static, scene.clone());
            let static_summary = summarize_updater(&u_static, &time0());
            let static_fill = summarize_instance(&u_static.create_fill_geometry_instance(&time0()).unwrap());
            let static_outline = summarize_instance(&u_static.create_outline_geometry_instance(&time0()).unwrap());

            let e_clamped = make_box_entity("box-clamp", pos, Cartesian3::new(100.0, 200.0, 300.0), Color::GREEN);
            if let Some(PropertyInput::Component(c)) = e_clamped.get_property("box") {
                let b = c.as_any().downcast_ref::<BoxGraphics>().unwrap();
                b.set_height_reference(Some(PropertyInput::value(HeightReference::ClampToGround.value() as f64)))
                    .unwrap();
            }
            let mut u_clamped = BoxGeometryUpdater::new(e_clamped, scene);
            let clamped_summary = summarize_updater(&u_clamped, &time0());
            let clamped_fill = summarize_instance(&u_clamped.create_fill_geometry_instance(&time0()).unwrap());

            u_static.destroy();
            u_clamped.destroy();
            ok(json!({
                "staticSummary": static_summary,
                "staticFill": static_fill,
                "staticOutline": static_outline,
                "clampedSummary": clamped_summary,
                "clampedFill": clamped_fill,
            }))
        },
    );
}

#[test]
fn corridor_geometry_updater_fixture() {
    run(
        "data_sources",
        "CorridorGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let positions = Rc::new(c3_deg_array(&[-105.0, 40.0, -104.0, 40.0, -104.0, 41.0]));

            let e_ground = new_entity("cor-ground");
            let c1 = Rc::new(CorridorGraphics::new());
            c1.set_positions(Some(PropertyInput::value(DynamicValue::Positions(Rc::clone(&positions))))).unwrap();
            c1.set_width(Some(PropertyInput::value(1000.0))).unwrap();
            c1.set_material(Some(PropertyInput::Component(color_mat(Color::ORANGE)))).unwrap();
            c1.set_z_index(Some(PropertyInput::value(3.0))).unwrap();
            e_ground.set_property("corridor", Some(PropertyInput::Component(c1))).unwrap();
            let mut u_ground = CorridorGeometryUpdater::new(e_ground, scene.clone());
            let ground_summary = summarize_updater(&u_ground, &time0());
            let ground_fill = summarize_instance(&u_ground.create_fill_geometry_instance(&time0()).unwrap());

            let e_ext = new_entity("cor-extruded");
            let c2 = Rc::new(CorridorGraphics::new());
            c2.set_positions(Some(PropertyInput::value(DynamicValue::Positions(positions)))).unwrap();
            c2.set_width(Some(PropertyInput::value(1000.0))).unwrap();
            c2.set_height(Some(PropertyInput::value(100.0))).unwrap();
            c2.set_extruded_height(Some(PropertyInput::value(500.0))).unwrap();
            c2.set_corner_type(Some(PropertyInput::value(CornerType::Mitered as u32 as f64))).unwrap();
            c2.set_outline(Some(PropertyInput::value(true))).unwrap();
            c2.set_outline_color(Some(PropertyInput::value(Color::WHITE))).unwrap();
            c2.set_material(Some(PropertyInput::Component(color_mat(Color::BLUE)))).unwrap();
            e_ext.set_property("corridor", Some(PropertyInput::Component(c2))).unwrap();
            let mut u_ext = CorridorGeometryUpdater::new(e_ext, scene);
            let extruded_summary = summarize_updater(&u_ext, &time0());
            let extruded_fill = summarize_instance(&u_ext.create_fill_geometry_instance(&time0()).unwrap());
            let extruded_outline = summarize_instance(&u_ext.create_outline_geometry_instance(&time0()).unwrap());

            u_ground.destroy();
            u_ext.destroy();
            ok(json!({
                "groundSummary": ground_summary,
                "groundFill": ground_fill,
                "extrudedSummary": extruded_summary,
                "extrudedFill": extruded_fill,
                "extrudedOutline": extruded_outline,
            }))
        },
    );
}

#[test]
fn cylinder_geometry_updater_fixture() {
    run(
        "data_sources",
        "CylinderGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let e = new_entity("cyl-1");
            set_pos(&e, c3_deg(-105.0, 40.0, 200.0));
            let c = Rc::new(CylinderGraphics::new());
            c.set_length(Some(PropertyInput::value(400.0))).unwrap();
            c.set_top_radius(Some(PropertyInput::value(50.0))).unwrap();
            c.set_bottom_radius(Some(PropertyInput::value(100.0))).unwrap();
            c.set_slices(Some(PropertyInput::value(16.0))).unwrap();
            c.set_material(Some(PropertyInput::Component(color_mat(Color::MAGENTA)))).unwrap();
            c.set_outline(Some(PropertyInput::value(true))).unwrap();
            c.set_outline_color(Some(PropertyInput::value(Color::BLACK))).unwrap();
            c.set_number_of_vertical_lines(Some(PropertyInput::value(8.0))).unwrap();
            e.set_property("cylinder", Some(PropertyInput::Component(c))).unwrap();

            let mut u = CylinderGeometryUpdater::new(e, scene);
            let summary = summarize_updater(&u, &time0());
            let fill = summarize_instance(&u.create_fill_geometry_instance(&time0()).unwrap());
            let outline = summarize_instance(&u.create_outline_geometry_instance(&time0()).unwrap());
            u.destroy();
            ok(json!({ "summary": summary, "fill": fill, "outline": outline }))
        },
    );
}

#[test]
fn ellipse_geometry_updater_fixture() {
    run(
        "data_sources",
        "EllipseGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let pos = c3_deg(-105.0, 40.0, 0.0);

            let e_g = new_entity("ell-ground");
            set_pos(&e_g, pos);
            let eg = Rc::new(EllipseGraphics::new());
            eg.set_semi_major_axis(Some(PropertyInput::value(5000.0))).unwrap();
            eg.set_semi_minor_axis(Some(PropertyInput::value(3000.0))).unwrap();
            eg.set_rotation(Some(PropertyInput::value(0.5))).unwrap();
            eg.set_material(Some(PropertyInput::Component(color_mat(Color::LIME)))).unwrap();
            eg.set_z_index(Some(PropertyInput::value(2.0))).unwrap();
            e_g.set_property("ellipse", Some(PropertyInput::Component(eg))).unwrap();
            let mut u_g = EllipseGeometryUpdater::new(e_g, scene.clone());
            let ground_summary = summarize_updater(&u_g, &time0());
            let ground_fill = summarize_instance(&u_g.create_fill_geometry_instance(&time0()).unwrap());

            let e_h = new_entity("ell-height");
            set_pos(&e_h, pos);
            let eh = Rc::new(EllipseGraphics::new());
            eh.set_semi_major_axis(Some(PropertyInput::value(5000.0))).unwrap();
            eh.set_semi_minor_axis(Some(PropertyInput::value(3000.0))).unwrap();
            eh.set_height(Some(PropertyInput::value(100.0))).unwrap();
            eh.set_extruded_height(Some(PropertyInput::value(600.0))).unwrap();
            eh.set_outline(Some(PropertyInput::value(true))).unwrap();
            eh.set_outline_color(Some(PropertyInput::value(Color::BLACK))).unwrap();
            eh.set_number_of_vertical_lines(Some(PropertyInput::value(12.0))).unwrap();
            e_h.set_property("ellipse", Some(PropertyInput::Component(eh))).unwrap();
            let mut u_h = EllipseGeometryUpdater::new(e_h, scene);
            let height_summary = summarize_updater(&u_h, &time0());
            let height_fill = summarize_instance(&u_h.create_fill_geometry_instance(&time0()).unwrap());
            let height_outline = summarize_instance(&u_h.create_outline_geometry_instance(&time0()).unwrap());

            u_g.destroy();
            u_h.destroy();
            ok(json!({
                "groundSummary": ground_summary,
                "groundFill": ground_fill,
                "heightSummary": height_summary,
                "heightFill": height_fill,
                "heightOutline": height_outline,
            }))
        },
    );
}

#[test]
fn ellipsoid_geometry_updater_fixture() {
    run(
        "data_sources",
        "EllipsoidGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let pos = c3_deg(-105.0, 40.0, 500.0);

            let e = new_entity("elpd-1");
            set_pos(&e, pos);
            let el = Rc::new(EllipsoidGraphics::new());
            el.set_radii(Some(PropertyInput::value(Cartesian3::new(200.0, 300.0, 400.0)))).unwrap();
            el.set_inner_radii(Some(PropertyInput::value(Cartesian3::new(50.0, 60.0, 70.0)))).unwrap();
            el.set_stack_partitions(Some(PropertyInput::value(16.0))).unwrap();
            el.set_slice_partitions(Some(PropertyInput::value(16.0))).unwrap();
            el.set_material(Some(PropertyInput::Component(color_mat(Color::CYAN)))).unwrap();
            el.set_outline(Some(PropertyInput::value(true))).unwrap();
            el.set_outline_color(Some(PropertyInput::value(Color::WHITE))).unwrap();
            el.set_subdivisions(Some(PropertyInput::value(32.0))).unwrap();
            e.set_property("ellipsoid", Some(PropertyInput::Component(el))).unwrap();
            let mut u = EllipsoidGeometryUpdater::new(e, scene.clone());
            let summary = summarize_updater(&u, &time0());
            let fill = summarize_instance(&u.create_fill_geometry_instance(&time0()).unwrap());
            let outline = summarize_instance(&u.create_outline_geometry_instance(&time0()).unwrap());

            let e_dyn = new_entity("elpd-dyn");
            set_pos(&e_dyn, pos);
            let ed = Rc::new(EllipsoidGraphics::new());
            let t0 = time0();
            ed.set_radii(Some(dyn_callback(
                move |t| {
                    Some(if *t == t0 {
                        Cartesian3::new(100.0, 200.0, 300.0)
                    } else {
                        Cartesian3::new(400.0, 500.0, 600.0)
                    })
                },
                DynamicValue::Cartesian3,
            )))
            .unwrap();
            ed.set_material(Some(PropertyInput::Component(color_mat(Color::YELLOW)))).unwrap();
            e_dyn.set_property("ellipsoid", Some(PropertyInput::Component(ed))).unwrap();
            let mut u_dyn = EllipsoidGeometryUpdater::new(e_dyn, scene);
            let prims = Rc::new(RefCell::new(PrimitiveCollection::default()));
            let mut dyn_u = u_dyn.create_dynamic(Rc::clone(&prims)).unwrap();
            dyn_u.update(&time0()).unwrap();
            let m0 = dyn_u.model_matrix().unwrap().enc();
            dyn_u.update(&time1()).unwrap();
            let m1 = dyn_u.model_matrix().unwrap().enc();
            let dyn_prim_count = prims.borrow().length();
            dyn_u.destroy();
            let dyn_after_destroy = prims.borrow().length();

            u.destroy();
            u_dyn.destroy();
            ok(json!({
                "summary": summary,
                "fill": fill,
                "outline": outline,
                "m0": m0,
                "m1": m1,
                "dynPrimCount": dyn_prim_count,
                "dynAfterDestroy": dyn_after_destroy,
            }))
        },
    );
}

#[test]
fn plane_geometry_updater_fixture() {
    run(
        "data_sources",
        "PlaneGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let e = new_entity("plane-1");
            set_pos(&e, c3_deg(-105.0, 40.0, 200.0));
            let p = Rc::new(PlaneGraphics::new());
            p.set_plane(Some(PropertyInput::value(DynamicValue::from_json(json!({
                "normal": [0.0, 0.0, 1.0],
                "distance": 25.0,
            })))))
            .unwrap();
            p.set_dimensions(Some(PropertyInput::value(Cartesian2::new(100.0, 200.0)))).unwrap();
            p.set_material(Some(PropertyInput::Component(color_mat(Color::AQUA)))).unwrap();
            p.set_outline(Some(PropertyInput::value(true))).unwrap();
            p.set_outline_color(Some(PropertyInput::value(Color::BLACK))).unwrap();
            e.set_property("plane", Some(PropertyInput::Component(p))).unwrap();

            let mut u = PlaneGeometryUpdater::new(e, scene);
            let summary = summarize_updater(&u, &time0());
            let fill = summarize_instance(&u.create_fill_geometry_instance(&time0()).unwrap());
            let outline = summarize_instance(&u.create_outline_geometry_instance(&time0()).unwrap());
            u.destroy();
            ok(json!({ "summary": summary, "fill": fill, "outline": outline }))
        },
    );
}

#[test]
fn polygon_geometry_updater_fixture() {
    run(
        "data_sources",
        "PolygonGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let ring = PolygonHierarchy::new(
                c3_deg_array(&[-105.0, 40.0, -104.0, 40.0, -104.0, 41.0, -105.0, 41.0]),
                Vec::new(),
            );

            let e_g = new_entity("poly-ground");
            let pg = Rc::new(PolygonGraphics::new());
            pg.set_hierarchy(Some(PropertyInput::value(DynamicValue::PolygonHierarchy(Rc::new(ring.clone())))))
                .unwrap();
            pg.set_material(Some(PropertyInput::Component(color_mat(Color::RED)))).unwrap();
            pg.set_z_index(Some(PropertyInput::value(1.0))).unwrap();
            e_g.set_property("polygon", Some(PropertyInput::Component(pg))).unwrap();
            let mut u_g = PolygonGeometryUpdater::new(e_g, scene.clone());
            let ground_summary = summarize_updater(&u_g, &time0());
            let ground_fill = summarize_instance(&u_g.create_fill_geometry_instance(&time0()).unwrap());
            let center = u_g.compute_center(&time0()).unwrap().enc();

            let e_ext = new_entity("poly-ext");
            let pe = Rc::new(PolygonGraphics::new());
            pe.set_hierarchy(Some(PropertyInput::value(DynamicValue::PolygonHierarchy(Rc::new(ring))))).unwrap();
            pe.set_height(Some(PropertyInput::value(50.0))).unwrap();
            pe.set_extruded_height(Some(PropertyInput::value(300.0))).unwrap();
            pe.set_close_top(Some(PropertyInput::value(true))).unwrap();
            pe.set_close_bottom(Some(PropertyInput::value(false))).unwrap();
            pe.set_outline(Some(PropertyInput::value(true))).unwrap();
            pe.set_outline_color(Some(PropertyInput::value(Color::WHITE))).unwrap();
            e_ext.set_property("polygon", Some(PropertyInput::Component(pe))).unwrap();
            let mut u_ext = PolygonGeometryUpdater::new(e_ext, scene.clone());
            let extruded_summary = summarize_updater(&u_ext, &time0());
            let extruded_fill = summarize_instance(&u_ext.create_fill_geometry_instance(&time0()).unwrap());
            let extruded_outline = summarize_instance(&u_ext.create_outline_geometry_instance(&time0()).unwrap());

            let e_cop = new_entity("poly-coplanar");
            let pc = Rc::new(PolygonGraphics::new());
            pc.set_hierarchy(Some(PropertyInput::value(DynamicValue::PolygonHierarchy(Rc::new(
                PolygonHierarchy::new(
                    c3_deg_heights_array(&[-105.0, 40.0, 100.0, -104.0, 40.0, 200.0, -104.0, 41.0, 150.0]),
                    Vec::new(),
                ),
            )))))
            .unwrap();
            pc.set_per_position_height(Some(PropertyInput::value(true))).unwrap();
            pc.set_outline(Some(PropertyInput::value(true))).unwrap();
            pc.set_material(Some(PropertyInput::Component(color_mat(Color::YELLOW)))).unwrap();
            e_cop.set_property("polygon", Some(PropertyInput::Component(pc))).unwrap();
            let mut u_cop = PolygonGeometryUpdater::new(e_cop, scene);
            let coplanar_summary = summarize_updater(&u_cop, &time0());
            let coplanar_fill = summarize_instance(&u_cop.create_fill_geometry_instance(&time0()).unwrap());
            let coplanar_outline = summarize_instance(&u_cop.create_outline_geometry_instance(&time0()).unwrap());

            u_g.destroy();
            u_ext.destroy();
            u_cop.destroy();
            ok(json!({
                "groundSummary": ground_summary,
                "groundFill": ground_fill,
                "extrudedSummary": extruded_summary,
                "extrudedFill": extruded_fill,
                "extrudedOutline": extruded_outline,
                "coplanarSummary": coplanar_summary,
                "coplanarFill": coplanar_fill,
                "coplanarOutline": coplanar_outline,
                "center": center,
            }))
        },
    );
}

#[test]
fn polyline_geometry_updater_fixture() {
    run(
        "data_sources",
        "PolylineGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let positions = Rc::new(c3_deg_heights_array(&[-105.0, 40.0, 100.0, -104.0, 40.5, 200.0]));

            let e_s = new_entity("pl-static");
            let ps = Rc::new(PolylineGraphics::new());
            ps.set_positions(Some(PropertyInput::value(DynamicValue::Positions(Rc::clone(&positions))))).unwrap();
            ps.set_width(Some(PropertyInput::value(4.0))).unwrap();
            ps.set_material(Some(PropertyInput::Component(color_mat(Color::RED)))).unwrap();
            ps.set_depth_fail_material(Some(PropertyInput::Component(color_mat(Color::BLUE)))).unwrap();
            ps.set_arc_type(Some(PropertyInput::value(ArcType::Geodesic as u8 as f64))).unwrap();
            e_s.set_property("polyline", Some(PropertyInput::Component(ps))).unwrap();
            let mut u_s = PolylineGeometryUpdater::new(e_s, scene.clone());
            let static_summary = summarize_updater(&u_s, &time0());
            let static_fill = summarize_instance(&u_s.create_fill_geometry_instance(&time0()).unwrap());

            let e_c = new_entity("pl-clamp");
            let pc = Rc::new(PolylineGraphics::new());
            pc.set_positions(Some(PropertyInput::value(DynamicValue::Positions(Rc::clone(&positions))))).unwrap();
            pc.set_width(Some(PropertyInput::value(3.0))).unwrap();
            pc.set_clamp_to_ground(Some(PropertyInput::value(true))).unwrap();
            pc.set_material(Some(PropertyInput::Component(color_mat(Color::GREEN)))).unwrap();
            pc.set_z_index(Some(PropertyInput::value(4.0))).unwrap();
            e_c.set_property("polyline", Some(PropertyInput::Component(pc))).unwrap();
            let mut u_c = PolylineGeometryUpdater::new(e_c, scene.clone());
            let clamp_summary = summarize_updater(&u_c, &time0());
            let clamp_fill = summarize_instance(&u_c.create_fill_geometry_instance(&time0()).unwrap());

            let e_short = new_entity("pl-short");
            let pshort = Rc::new(PolylineGraphics::new());
            pshort
                .set_positions(Some(PropertyInput::value(DynamicValue::Positions(Rc::new(vec![positions[0]])))))
                .unwrap();
            e_short.set_property("polyline", Some(PropertyInput::Component(pshort))).unwrap();
            let mut u_short = PolylineGeometryUpdater::new(e_short, scene.clone());
            let short_summary = summarize_updater(&u_short, &time0());

            let e_dyn = new_entity("pl-dyn");
            let pd = Rc::new(PolylineGraphics::new());
            let pos_cb = Rc::clone(&positions);
            pd.set_positions(Some(dyn_callback(move |_| Some(Rc::clone(&pos_cb)), DynamicValue::Positions))).unwrap();
            pd.set_width(Some(PropertyInput::value(5.0))).unwrap();
            pd.set_material(Some(PropertyInput::Component(color_mat(Color::YELLOW)))).unwrap();
            pd.set_arc_type(Some(PropertyInput::value(ArcType::None as u8 as f64))).unwrap();
            e_dyn.set_property("polyline", Some(PropertyInput::Component(pd))).unwrap();
            let mut u_dyn = PolylineGeometryUpdater::new(e_dyn, scene);
            let prims = Rc::new(RefCell::new(PrimitiveCollection::default()));
            let gprims = Rc::new(RefCell::new(OrderedGroundPrimitiveCollection::default()));
            let mut dyn_u = u_dyn.create_dynamic_updater(Rc::clone(&prims), Rc::clone(&gprims)).unwrap();
            dyn_u.update(&time0()).unwrap();
            let dyn_prims = json!([prims.borrow().length(), gprims.borrow().length()]);
            let mut bs = BoundingSphere::default();
            let bs_state = dyn_u.get_bounding_sphere(&mut bs) as u8;
            dyn_u.destroy();
            let dyn_after_destroy = json!([prims.borrow().length(), gprims.borrow().length()]);

            u_s.destroy();
            u_c.destroy();
            u_short.destroy();
            u_dyn.destroy();
            ok(json!({
                "staticSummary": static_summary,
                "staticFill": static_fill,
                "clampSummary": clamp_summary,
                "clampFill": clamp_fill,
                "shortSummary": short_summary,
                "dynPrims": dyn_prims,
                "bsState": bs_state,
                "bs": bs.enc(),
                "dynAfterDestroy": dyn_after_destroy,
            }))
        },
    );
}

#[test]
fn polyline_volume_geometry_updater_fixture() {
    run(
        "data_sources",
        "PolylineVolumeGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let e = new_entity("pv-1");
            let pv = Rc::new(PolylineVolumeGraphics::new());
            pv.set_positions(Some(PropertyInput::value(DynamicValue::Positions(Rc::new(c3_deg_array(&[
                -105.0, 40.0, -104.0, 40.0, -104.0, 41.0,
            ]))))))
            .unwrap();
            pv.set_shape(Some(PropertyInput::value(DynamicValue::from_json(json!([
                [-50.0, -50.0],
                [50.0, -50.0],
                [50.0, 50.0],
                [-50.0, 50.0],
            ])))))
            .unwrap();
            pv.set_corner_type(Some(PropertyInput::value(CornerType::Beveled as u32 as f64))).unwrap();
            pv.set_material(Some(PropertyInput::Component(color_mat(Color::ORANGE)))).unwrap();
            pv.set_outline(Some(PropertyInput::value(true))).unwrap();
            pv.set_outline_color(Some(PropertyInput::value(Color::BLACK))).unwrap();
            e.set_property("polylineVolume", Some(PropertyInput::Component(pv))).unwrap();

            let mut u = PolylineVolumeGeometryUpdater::new(e, scene);
            let summary = summarize_updater(&u, &time0());
            let fill = summarize_instance(&u.create_fill_geometry_instance(&time0()).unwrap());
            let outline = summarize_instance(&u.create_outline_geometry_instance(&time0()).unwrap());
            u.destroy();
            ok(json!({ "summary": summary, "fill": fill, "outline": outline }))
        },
    );
}

#[test]
fn rectangle_geometry_updater_fixture() {
    run(
        "data_sources",
        "RectangleGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let e_g = make_rect_entity("rect-ground", -106.0, 39.0, -104.0, 41.0, color_mat(Color::RED), 2.0);
            let mut u_g = RectangleGeometryUpdater::new(e_g, scene.clone());
            let ground_summary = summarize_updater(&u_g, &time0());
            let ground_fill = summarize_instance(&u_g.create_fill_geometry_instance(&time0()).unwrap());
            let center = u_g.compute_center(&time0()).unwrap().enc();

            let e_ext = new_entity("rect-ext");
            let re = Rc::new(RectangleGraphics::new());
            re.set_coordinates(Some(rect_val(-106.0, 39.0, -104.0, 41.0))).unwrap();
            re.set_height(Some(PropertyInput::value(100.0))).unwrap();
            re.set_extruded_height(Some(PropertyInput::value(500.0))).unwrap();
            re.set_rotation(Some(PropertyInput::value(0.25))).unwrap();
            re.set_outline(Some(PropertyInput::value(true))).unwrap();
            re.set_outline_color(Some(PropertyInput::value(Color::WHITE))).unwrap();
            re.set_material(Some(PropertyInput::Component(color_mat(Color::BLUE)))).unwrap();
            e_ext.set_property("rectangle", Some(PropertyInput::Component(re))).unwrap();
            let mut u_ext = RectangleGeometryUpdater::new(e_ext, scene);
            let extruded_summary = summarize_updater(&u_ext, &time0());
            let extruded_fill = summarize_instance(&u_ext.create_fill_geometry_instance(&time0()).unwrap());
            let extruded_outline = summarize_instance(&u_ext.create_outline_geometry_instance(&time0()).unwrap());

            u_g.destroy();
            u_ext.destroy();
            ok(json!({
                "groundSummary": ground_summary,
                "groundFill": ground_fill,
                "center": center,
                "extrudedSummary": extruded_summary,
                "extrudedFill": extruded_fill,
                "extrudedOutline": extruded_outline,
            }))
        },
    );
}

#[test]
fn wall_geometry_updater_fixture() {
    run(
        "data_sources",
        "WallGeometryUpdater",
        |_| TIGHT,
        |_, _| {
            let scene: Rc<dyn GeometryScene> = make_scene(5.0);
            let e = new_entity("wall-1");
            let w = Rc::new(WallGraphics::new());
            w.set_positions(Some(PropertyInput::value(DynamicValue::Positions(Rc::new(c3_deg_array(&[
                -105.0, 40.0, -104.0, 40.0, -104.0, 41.0,
            ]))))))
            .unwrap();
            w.set_minimum_heights(Some(PropertyInput::value(DynamicValue::from_json(json!([0.0, 0.0, 0.0]))))).unwrap();
            w.set_maximum_heights(Some(PropertyInput::value(DynamicValue::from_json(json!([1000.0, 1500.0, 1000.0])))))
                .unwrap();
            w.set_material(Some(PropertyInput::Component(color_mat(Color::GREEN)))).unwrap();
            w.set_outline(Some(PropertyInput::value(true))).unwrap();
            w.set_outline_color(Some(PropertyInput::value(Color::BLACK))).unwrap();
            e.set_property("wall", Some(PropertyInput::Component(w))).unwrap();

            let mut u = WallGeometryUpdater::new(e, scene);
            let summary = summarize_updater(&u, &time0());
            let fill = summarize_instance(&u.create_fill_geometry_instance(&time0()).unwrap());
            let outline = summarize_instance(&u.create_outline_geometry_instance(&time0()).unwrap());
            u.destroy();
            ok(json!({ "summary": summary, "fill": fill, "outline": outline }))
        },
    );
}

#[test]
fn geometry_visualizer_fixture() {
    run(
        "data_sources",
        "GeometryVisualizer",
        |_| EXACT,
        |_, _| {
            let scene_concrete = make_scene(5.0);
            let scene: Rc<dyn GeometryScene> = scene_concrete.clone();
            let entities = EntityCollection::new();

            let e_box =
                make_box_entity("gv-box", c3_deg(-105.0, 40.0, 100.0), Cartesian3::new(10.0, 20.0, 30.0), Color::RED);
            if let Some(PropertyInput::Component(c)) = e_box.get_property("box") {
                c.as_any()
                    .downcast_ref::<BoxGraphics>()
                    .unwrap()
                    .set_outline(Some(PropertyInput::value(true)))
                    .unwrap();
            }
            entities.add(e_box).unwrap();

            entities
                .add(make_rect_entity("gv-rect-ground", -110.0, 30.0, -105.0, 35.0, color_mat(Color::BLUE), 0.0))
                .unwrap();

            let grid = Rc::new(GridMaterialProperty::new());
            grid.set_color(Some(PropertyInput::value(Color::GREEN))).unwrap();
            entities.add(make_rect_entity("gv-rect-mat", -95.0, 30.0, -90.0, 35.0, grid, 0.0)).unwrap();

            let e_wall = new_entity("gv-wall-dyn");
            let w = Rc::new(WallGraphics::new());
            let wall_pts = Rc::new(c3_deg_heights_array(&[-105.0, 40.0, 500.0, -104.0, 40.0, 500.0]));
            w.set_positions(Some(dyn_callback(move |_| Some(Rc::clone(&wall_pts)), DynamicValue::Positions))).unwrap();
            w.set_material(Some(PropertyInput::Component(color_mat(Color::YELLOW)))).unwrap();
            e_wall.set_property("wall", Some(PropertyInput::Component(w))).unwrap();
            entities.add(e_wall).unwrap();

            let mut viz = GeometryVisualizer::new(scene, &entities, None, None);
            let total_batches = viz.total_batch_count();
            let first_update = viz.update(&time0()).unwrap();
            let after_first = json!([
                scene_concrete.primitives.borrow().length(),
                scene_concrete.ground_primitives.borrow().length()
            ]);

            entities.remove_by_id("gv-rect-ground");
            viz.update(&time0()).unwrap();
            let after_remove = json!([
                scene_concrete.primitives.borrow().length(),
                scene_concrete.ground_primitives.borrow().length()
            ]);

            viz.destroy();
            let after_destroy = json!([
                scene_concrete.primitives.borrow().length(),
                scene_concrete.ground_primitives.borrow().length(),
                viz.is_destroyed(),
            ]);
            ok(json!({
                "totalBatches": total_batches,
                "firstUpdate": first_update,
                "afterFirst": after_first,
                "afterRemove": after_remove,
                "afterDestroy": after_destroy,
            }))
        },
    );
}

#[test]
fn polyline_visualizer_fixture() {
    run(
        "data_sources",
        "PolylineVisualizer",
        |_| EXACT,
        |_, _| {
            let scene_concrete = make_scene(5.0);
            let scene: Rc<dyn GeometryScene> = scene_concrete.clone();
            let entities = EntityCollection::new();
            let pts = Rc::new(c3_deg_heights_array(&[-105.0, 40.0, 100.0, -104.0, 40.5, 100.0]));

            let add_pl = |id: &str, mat: ComponentRef, df: Option<ComponentRef>, clamp: bool, dynamic: bool| {
                let e = new_entity(id);
                let pl = Rc::new(PolylineGraphics::new());
                if dynamic {
                    let p = Rc::clone(&pts);
                    pl.set_positions(Some(dyn_callback(move |_| Some(Rc::clone(&p)), DynamicValue::Positions)))
                        .unwrap();
                    pl.set_width(Some(PropertyInput::value(4.0))).unwrap();
                } else {
                    pl.set_positions(Some(PropertyInput::value(DynamicValue::Positions(Rc::clone(&pts))))).unwrap();
                    pl.set_width(Some(PropertyInput::value(3.0))).unwrap();
                }
                pl.set_material(Some(PropertyInput::Component(mat))).unwrap();
                if let Some(df) = df {
                    pl.set_depth_fail_material(Some(PropertyInput::Component(df))).unwrap();
                }
                if clamp {
                    pl.set_clamp_to_ground(Some(PropertyInput::value(true))).unwrap();
                }
                e.set_property("polyline", Some(PropertyInput::Component(pl))).unwrap();
                entities.add(e).unwrap();
            };

            add_pl("pv-color", color_mat(Color::RED), None, false, false);
            add_pl("pv-df", color_mat(Color::RED), Some(color_mat(Color::BLUE)), false, false);
            let dash = Rc::new(PolylineDashMaterialProperty::new());
            dash.set_color(Some(PropertyInput::value(Color::YELLOW))).unwrap();
            add_pl("pv-mat", dash, None, false, false);
            add_pl("pv-ground", color_mat(Color::GREEN), None, true, false);
            add_pl("pv-dyn", color_mat(Color::CYAN), None, false, true);

            let mut viz = PolylineVisualizer::new(scene, &entities, None, None);
            let total_batches = viz.total_batch_count();
            let first_update = viz.update(&time0()).unwrap();
            let after_first = json!([
                scene_concrete.primitives.borrow().length(),
                scene_concrete.ground_primitives.borrow().length()
            ]);

            entities.remove_by_id("pv-ground");
            viz.update(&time0()).unwrap();
            let after_remove = json!([
                scene_concrete.primitives.borrow().length(),
                scene_concrete.ground_primitives.borrow().length()
            ]);

            viz.destroy();
            let after_destroy = json!([
                scene_concrete.primitives.borrow().length(),
                scene_concrete.ground_primitives.borrow().length(),
                viz.is_destroyed(),
            ]);
            ok(json!({
                "totalBatches": total_batches,
                "firstUpdate": first_update,
                "afterFirst": after_first,
                "afterRemove": after_remove,
                "afterDestroy": after_destroy,
            }))
        },
    );
}

// ---------------------------------------------------------------------------
// Native lifecycle, attribute-update, translucency migration & bounding tests
// ---------------------------------------------------------------------------

#[test]
fn static_color_batch_solid_to_translucent_migration_and_bounding_sphere() {
    let scene: Rc<dyn GeometryScene> = make_scene(5.0);
    let primitives = Rc::new(RefCell::new(PrimitiveCollection::default()));
    let mut batch = StaticGeometryColorBatch::new(
        Rc::clone(&primitives),
        GeometryAppearanceKind::PerInstanceColor,
        None,
        true,
        ShadowMode::Disabled,
    );

    let e = new_entity("migrate-box");
    set_pos(&e, c3_deg(-105.0, 40.0, 100.0));
    let t0 = time0();
    let mat = Rc::new(
        ColorMaterialProperty::new(Some(dyn_callback(
            move |t| Some(if *t == t0 { Color::RED } else { Color::RED.with_alpha(0.5) }),
            DynamicValue::Color,
        )))
        .unwrap(),
    );
    let b = Rc::new(BoxGraphics::new());
    b.set_dimensions(Some(PropertyInput::value(Cartesian3::new(10.0, 20.0, 30.0)))).unwrap();
    b.set_material(Some(PropertyInput::Component(mat))).unwrap();
    e.set_property("box", Some(PropertyInput::Component(b))).unwrap();

    let u: GeometryUpdaterRef = Rc::new(RefCell::new(BoxGeometryUpdater::new(Rc::clone(&e), scene)));
    batch.add(&time0(), Rc::clone(&u)).unwrap();
    assert_eq!(batch.solid_batch_count(), 1);
    assert_eq!(batch.translucent_batch_count(), 0);

    // First update creates async primitive -> returns false and bounding sphere is Pending.
    assert!(!batch.update(&time0()).unwrap());
    let mut bs = BoundingSphere::default();
    assert_eq!(batch.get_bounding_sphere(&*u.borrow(), &mut bs), BoundingSphereState::Pending);

    // Mark ready -> update succeeds and bounding sphere is Done.
    batch.mark_all_ready();
    assert!(batch.update(&time0()).unwrap());
    assert_eq!(batch.get_bounding_sphere(&*u.borrow(), &mut bs), BoundingSphereState::Done);
    assert!(bs.radius > 0.0);

    // Advance to time1 -> alpha becomes 0.5 -> migrates from solid to translucent bucket.
    assert!(!batch.update(&time1()).unwrap());
    assert_eq!(batch.translucent_batch_count(), 1);
    batch.mark_all_ready();
    assert!(batch.update(&time1()).unwrap());
    assert_eq!(batch.get_bounding_sphere(&*u.borrow(), &mut bs), BoundingSphereState::Done);

    // Hide entity -> bounding sphere returns Failed.
    e.set_show(false);
    assert!(batch.update(&time1()).unwrap());
    assert_eq!(batch.get_bounding_sphere(&*u.borrow(), &mut bs), BoundingSphereState::Failed);

    batch.remove_all_primitives();
    u.borrow_mut().destroy();
}

#[test]
fn geometry_visualizer_static_to_dynamic_and_registry() {
    let scene_concrete = make_scene(5.0);
    let scene: Rc<dyn GeometryScene> = scene_concrete.clone();
    let entities = EntityCollection::new();

    let e = make_box_entity("vis-dyn", c3_deg(-105.0, 40.0, 100.0), Cartesian3::new(20.0, 20.0, 20.0), Color::RED);
    entities.add(Rc::clone(&e)).unwrap();

    let mut viz = GeometryVisualizer::new(scene, &entities, None, None);
    assert!(!viz.update(&time0()).unwrap());
    viz.mark_all_primitives_ready();
    assert!(viz.update(&time0()).unwrap());

    let mut bs = BoundingSphere::default();
    assert_eq!(viz.get_bounding_sphere(&e, &mut bs), BoundingSphereState::Done);
    assert!(bs.radius > 0.0);

    // Transition static -> dynamic via CallbackProperty dimensions.
    if let Some(PropertyInput::Component(c)) = e.get_property("box") {
        c.as_any()
            .downcast_ref::<BoxGraphics>()
            .unwrap()
            .set_dimensions(Some(dyn_callback(|_| Some(Cartesian3::new(40.0, 40.0, 40.0)), DynamicValue::Cartesian3)))
            .unwrap();
    }
    assert!(viz.update(&time1()).unwrap());
    let mut bs2 = BoundingSphere::default();
    assert_eq!(viz.get_bounding_sphere(&e, &mut bs2), BoundingSphereState::Done);
    assert!(bs2.radius > bs.radius);

    // Register & unregister custom updater name cleanly.
    let before = GeometryUpdaterSet::registered_updater_names();
    GeometryVisualizer::register_updater(GeometryUpdaterRegistration::new(
        "CustomTestUpdater",
        Rc::new(|ent, sc| Rc::new(RefCell::new(BoxGeometryUpdater::new(ent, sc)))),
    ));
    assert_eq!(GeometryUpdaterSet::registered_updater_names().len(), before.len() + 1);
    GeometryVisualizer::unregister_updater("CustomTestUpdater");
    assert_eq!(GeometryUpdaterSet::registered_updater_names(), before);

    viz.destroy();
    assert!(viz.is_destroyed());
    assert_eq!(scene_concrete.primitives.borrow().length(), 0);
}

#[test]
fn polyline_visualizer_bounding_spheres_and_dynamic_clamp_switch() {
    let scene_concrete = make_scene(5.0);
    let scene: Rc<dyn GeometryScene> = scene_concrete.clone();
    let entities = EntityCollection::new();
    let pts = Rc::new(c3_deg_heights_array(&[-105.0, 40.0, 100.0, -104.0, 40.5, 100.0]));

    let e = new_entity("pl-switch");
    let pl = Rc::new(PolylineGraphics::new());
    let p_cb = Rc::clone(&pts);
    pl.set_positions(Some(dyn_callback(move |_| Some(Rc::clone(&p_cb)), DynamicValue::Positions))).unwrap();
    let t0 = time0();
    pl.set_clamp_to_ground(Some(dyn_callback(move |t| Some(*t != t0), DynamicValue::Boolean))).unwrap();
    pl.set_material(Some(PropertyInput::Component(color_mat(Color::LIME)))).unwrap();
    e.set_property("polyline", Some(PropertyInput::Component(pl))).unwrap();
    entities.add(Rc::clone(&e)).unwrap();

    let mut viz = PolylineVisualizer::new(scene, &entities, None, None);
    // At time0: clampToGround = false -> uses shared DynamicPolylineCollection in primitives.
    assert!(viz.update(&time0()).unwrap());
    assert_eq!(scene_concrete.primitives.borrow().length(), 1);
    assert_eq!(scene_concrete.ground_primitives.borrow().length(), 0);

    let mut bs = BoundingSphere::default();
    assert_eq!(viz.get_bounding_sphere(&e, &mut bs), BoundingSphereState::Done);
    assert!(bs.radius > 0.0);

    // At time1: clampToGround = true -> adds GroundGeometryPrimitive to ground_primitives.
    assert!(viz.update(&time1()).unwrap());
    assert_eq!(scene_concrete.ground_primitives.borrow().length(), 1);
    assert_eq!(viz.get_bounding_sphere(&e, &mut bs), BoundingSphereState::Done);

    viz.destroy();
    assert!(viz.is_destroyed());
    assert_eq!(scene_concrete.primitives.borrow().length(), 0);
    assert_eq!(scene_concrete.ground_primitives.borrow().length(), 0);
}
