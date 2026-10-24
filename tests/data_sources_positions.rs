//! Reference-frame position and motion properties against pinned CesiumJS.
mod common;
use cesium_rs::core::iau2006_xys_data::Iau2006XysDataOptions;
use cesium_rs::{core::*, data_sources::*};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn date(t: f64) -> JulianDate {
    JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap().add_seconds(t)
}
fn frame(v: &Value) -> ReferenceFrame {
    if n(v) == 0. {
        ReferenceFrame::Fixed
    } else {
        ReferenceFrame::Inertial
    }
}
fn context(mode: &str) -> PositionContext {
    match mode {
        "moon" => PositionContext {
            frame_transform: Rc::new(|t| Ok(Transforms::compute_icrf_to_moon_fixed_matrix(t))),
            ..Default::default()
        },
        "iau" => {
            let mut data = Iau2006XysData::new(Iau2006XysDataOptions {
                interpolation_order: 1,
                sample_zero_julian_ephemeris_date: 2458848.5,
                step_size_days: 1.,
                samples_per_xys_file: 4,
                total_samples: 4,
                ..Default::default()
            });
            data.add_chunk_json(
                0,
                r#"{"samples":[0.001,0.002,0.0001,0.0011,0.0022,0.00012,0.0012,0.0024,0.00014,0.0013,0.0026,0.00016]}"#,
            )
            .unwrap();
            PositionContext::with_earth_data(Rc::new(EarthOrientationParameters::none()), Rc::new(RefCell::new(data)))
        }
        _ => PositionContext::default(),
    }
}
fn count(e: &DefinitionChanged) -> (Rc<Cell<usize>>, Subscription) {
    let c = Rc::new(Cell::new(0));
    let out = c.clone();
    let s = e.add_listener(move |_| out.set(out.get() + 1));
    (c, s)
}
fn optional(v: &Value) -> Option<Cartesian3> {
    (!v.is_null()).then(|| c3(v))
}
fn snapshot(p: &dyn PositionProperty, t: f64) -> Value {
    json!([
        p.get_value(Some(&date(t)), None).unwrap().enc(),
        p.get_value_in_reference_frame(&date(t), ReferenceFrame::Fixed, None).unwrap().enc(),
        p.get_value_in_reference_frame(&date(t), ReferenceFrame::Inertial, None).unwrap().enc()
    ])
}
fn snapshots(p: &dyn PositionProperty, queries: &Value) -> Value {
    list(queries, |t| snapshot(p, n(t))).enc()
}
fn array_snapshots(p: &PositionPropertyArray, queries: &Value) -> Value {
    list(queries, |t| {
        json!([
            p.get_value(Some(&date(n(t))), None).unwrap().enc(),
            p.get_value_in_reference_frame(&date(n(t)), ReferenceFrame::Fixed, None).unwrap().enc(),
            p.get_value_in_reference_frame(&date(n(t)), ReferenceFrame::Inertial, None).unwrap().enc()
        ])
    })
    .enc()
}
fn child(kind: &str, reference: ReferenceFrame, ctx: PositionContext) -> Option<PropertyRef<Cartesian3>> {
    match kind {
        "absent" => None,
        "constant" => Some(Rc::new(ConstantPositionProperty::with_context(
            Some(Cartesian3::new(6378137., 200., 300.)),
            reference,
            ctx,
        ))),
        "sampled" => {
            let p = SampledPositionProperty::with_context(reference, 0, ctx);
            p.add_samples(
                &[date(0.), date(1.), date(2.)],
                &[
                    Cartesian3::new(6378137., 0., 0.),
                    Cartesian3::new(6378138., 3., 1.),
                    Cartesian3::new(6378140., 4., 2.),
                ],
                None,
            )
            .unwrap();
            Some(Rc::new(p))
        }
        _ => {
            let kind = kind.to_owned();
            Some(Rc::new(CallbackPositionProperty::with_context(
                Rc::new(move |time, _| {
                    let x = JulianDate::seconds_difference(time, &date(0.));
                    if kind == "bounded" && !(0. ..=2.).contains(&x)
                        || kind == "missing" && x == 0.
                        || kind == "isolated" && x != 0.
                    {
                        return Ok(None);
                    }
                    let moving = kind != "stationary";
                    Ok(Some(Cartesian3::new(
                        6378137. + if moving { 120. * x } else { 0. },
                        200. + if moving { 30. * x } else { 0. },
                        300. + if moving { 5. * x } else { 0. },
                    )))
                }),
                false,
                reference,
                ctx,
            )))
        }
    }
}
#[test]
fn position_property() {
    run(
        "data_sources",
        "PositionProperty",
        |_| TIGHT,
        |f, a| {
            if f != "convert" {
                return None;
            }
            let mut r = Cartesian3::new(7., 8., 9.);
            let value = context(s(&a[0]))
                .convert(
                    &date(n(&a[3])),
                    optional(&a[4]),
                    FrameConversion { input: frame(&a[1]), output: frame(&a[2]) },
                    Some(&mut r),
                )
                .unwrap();
            ok(json!([value.enc(), r.enc()]))
        },
    );
}
#[test]
fn constant_position_property() {
    run(
        "data_sources",
        "ConstantPositionProperty",
        |_| TIGHT,
        |f, a| {
            if f != "sequence" {
                return None;
            }
            let ctx = context(s(&a[0]));
            let p = ConstantPositionProperty::with_context(None, frame(&a[1]), ctx.clone());
            let (changes, _guard) = count(p.definition_changed());
            let xyz = Some(Cartesian3::new(6378137., 200., 300.));
            let mut out = vec![];
            for (value, next) in [
                (None, None),
                (xyz, None),
                (xyz, None),
                (xyz, Some(ReferenceFrame::Inertial)),
                (Some(Cartesian3::ZERO), Some(ReferenceFrame::Fixed)),
                (None, Some(ReferenceFrame::Fixed)),
                (xyz, Some(ReferenceFrame::Inertial)),
            ] {
                p.set_value(value, next);
                out.push(json!([
                    p.is_constant(),
                    p.reference_frame() as u8,
                    changes.get(),
                    list(&a[2], |t| snapshot(&p, n(&t[0]))),
                    p.equals(&ConstantPositionProperty::with_context(value, p.reference_frame(), ctx.clone())).unwrap()
                ]));
            }
            ok(out)
        },
    );
}
#[test]
fn callback_position_property() {
    run(
        "data_sources",
        "CallbackPositionProperty",
        |_| TIGHT,
        |f, a| {
            if f != "evaluate" {
                return None;
            }
            let ctx = context(s(&a[0]));
            let reference = frame(&a[1]);
            let flag = a[2].as_bool().unwrap();
            let calls = Rc::new(Cell::new(0));
            let out = calls.clone();
            let callback: PropertyCallback<Cartesian3> = Rc::new(move |time, result| {
                out.set(out.get() + 1);
                let x = JulianDate::seconds_difference(time, &date(0.));
                if x < 0. {
                    return Ok(None);
                }
                let v = Cartesian3::new(6378137. + 120. * x, 200. + 30. * x, 300. + 5. * x);
                if let Some(r) = result {
                    *r = v;
                }
                Ok(Some(v))
            });
            let p = CallbackPositionProperty::with_context(callback.clone(), flag, reference, ctx.clone());
            let (changes, _guard) = count(p.definition_changed());
            let mut r = Cartesian3::new(7., 8., 9.);
            let values = a[3]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| {
                    let v = p.get_value(Some(&date(n(t))), Some(&mut r)).unwrap();
                    json!([v.enc(), r.enc()])
                })
                .collect::<Vec<_>>();
            let same = p
                .equals(&CallbackPositionProperty::with_context(callback.clone(), flag, reference, ctx.clone()))
                .unwrap();
            let different = p
                .equals(&CallbackPositionProperty::with_context(
                    callback.clone(),
                    flag,
                    if reference == ReferenceFrame::Fixed { ReferenceFrame::Inertial } else { ReferenceFrame::Fixed },
                    ctx,
                ))
                .unwrap();
            p.set_callback(callback.clone(), flag);
            p.set_callback(callback.clone(), !flag);
            p.set_callback(callback, !flag);
            ok(json!([values, calls.get(), changes.get(), p.is_constant(), p.reference_frame() as u8, same, different]))
        },
    );
}
#[test]
fn sampled_position_property() {
    run(
        "data_sources",
        "SampledPositionProperty",
        |_| TIGHT,
        |f, a| {
            if f != "samples" {
                return None;
            }
            let order = n(&a[2]) as usize;
            let p = SampledPositionProperty::with_context(frame(&a[1]), order, context(s(&a[0])));
            let (changes, _guard) = count(p.definition_changed());
            let ts = [2., 0., 1., 1.];
            let values = ts
                .iter()
                .enumerate()
                .map(|(i, t)| Cartesian3::new(6378137. + t * t + i as f64, 200. + 3. * t, 300. + t))
                .collect::<Vec<_>>();
            let d = ts
                .iter()
                .map(|t| {
                    (0..order)
                        .map(|i| {
                            Cartesian3::new(
                                if i == 0 { 2. * t } else { 2. },
                                if i == 0 { 3. } else { 0. },
                                if i == 0 { 1. } else { 0. },
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let algorithm = match s(&a[3]) {
                "LinearApproximation" => Interpolation::Linear,
                "LagrangePolynomialApproximation" => Interpolation::Lagrange,
                _ => Interpolation::Hermite,
            };
            p.set_interpolation_options(Some(algorithm), Some(3.));
            p.add_samples(&ts.map(date), &values, (order > 0).then_some(d.as_slice())).unwrap();
            let initial = json!([
                p.is_constant(),
                p.reference_frame() as u8,
                p.number_of_derivatives(),
                p.interpolation_degree(),
                p.interpolation_algorithm().algorithm_type(),
                changes.get(),
                snapshots(&p, &a[4])
            ]);
            p.set_forward_extrapolation_type(ExtrapolationType::Hold);
            p.set_backward_extrapolation_type(ExtrapolationType::Extrapolate);
            p.set_backward_extrapolation_duration(0.02);
            p.set_forward_extrapolation_duration(0.02);
            let extrap = snapshots(&p, &a[4]);
            let mut packed = vec![
                PackedSampleValue::Iso8601(date(3.).to_iso8601(None)),
                6378140f64.into(),
                210f64.into(),
                310f64.into(),
            ];
            for _ in 0..order {
                packed.extend([1., 2., 3.].map(PackedSampleValue::Number));
            }
            p.add_samples_packed_array(&packed, None).unwrap();
            let extra = snapshot(&p, 3.);
            let removed = [p.remove_sample(&date(0.25)), p.remove_sample(&date(1.))];
            p.remove_samples(&TimeInterval::<()>::new(date(2.), date(3.), true, false, None));
            ok(json!([initial, extrap, extra, removed, changes.get(), snapshots(&p, &a[4])]))
        },
    );
}
fn intervals_port(f: &str, a: &[Value], composite: bool) -> Value {
    let reference = frame(&a[1]);
    let ctx = context(s(&a[0]));
    let cp = CompositePositionProperty::with_context(reference, ctx.clone());
    let raw = TimeIntervalCollectionPositionProperty::with_context(reference, ctx.clone());
    let p: &dyn PositionProperty = if composite { &cp } else { &raw };
    let (changes, _guard) = count(p.definition_changed());
    let data = [ReferenceFrame::Fixed, ReferenceFrame::Inertial].map(|r| {
        Rc::new(ConstantPositionProperty::with_context(Some(Cartesian3::new(6378137., 200., 300.)), r, ctx.clone()))
    });
    let raw_data = [Rc::new(Cartesian3::new(6378137., 200., 300.)), Rc::new(Cartesian3::new(6378137., 200., 300.))];
    assert_eq!(f, "intervals");
    let capture = || {
        json!([
            p.is_constant(),
            p.reference_frame() as u8,
            changes.get(),
            if composite { cp.intervals().len() } else { raw.intervals().len() },
            snapshots(p, &a[3]),
            if composite {
                Some(data.iter().map(|p| p.definition_changed().number_of_listeners()).collect::<Vec<_>>())
            } else {
                None
            }
        ])
    };
    let mut out = vec![capture()];
    for i in 0..2 {
        if composite {
            cp.intervals().add_interval(TimeInterval::new(
                date(i as f64),
                date(i as f64 + 1.),
                true,
                i == 1,
                Some(data[i].clone() as PositionPropertyRef),
            ));
        } else {
            raw.intervals().add_interval(TimeInterval::new(
                date(i as f64),
                date(i as f64 + 1.),
                true,
                i == 1,
                Some(raw_data[i].clone()),
            ));
        }
        out.push(capture());
    }
    if composite {
        data[0].set_value(Some(Cartesian3::new(1., 2., 3.)), None);
        cp.set_reference_frame(if reference == ReferenceFrame::Fixed {
            ReferenceFrame::Inertial
        } else {
            ReferenceFrame::Fixed
        });
    }
    out.push(capture());
    if composite {
        cp.intervals().remove_interval(&TimeInterval::new(date(0.5), date(1.), true, false, None));
    } else {
        raw.intervals().remove_interval(&TimeInterval::new(date(0.5), date(1.), true, false, None));
    }
    out.push(capture());
    if composite {
        cp.intervals().remove_all();
    } else {
        raw.intervals().remove_all();
    }
    out.push(capture());
    out.enc()
}
fn interval_case(f: &str, a: &[Value], composite: bool) -> Option<Out> {
    match f {
        "intervals" => ok(intervals_port(f, a, composite)),
        "equals" => {
            let reference = frame(&a[0]);
            let equal = a[1].as_bool().unwrap();
            let one = Cartesian3::new(6378137., 200., 300.);
            let two = if equal { one } else { Cartesian3::new(1., 2., 3.) };
            if composite {
                let (a, b) = (CompositePositionProperty::new(reference), CompositePositionProperty::new(reference));
                a.intervals().add_interval(TimeInterval::new(
                    date(0.),
                    date(1.),
                    true,
                    true,
                    Some(Rc::new(ConstantPositionProperty::new(Some(one), reference)) as PositionPropertyRef),
                ));
                b.intervals().add_interval(TimeInterval::new(
                    date(0.),
                    date(1.),
                    true,
                    true,
                    Some(Rc::new(ConstantPositionProperty::new(Some(two), reference)) as PositionPropertyRef),
                ));
                res(a.equals(&b))
            } else {
                let (a, b) = (
                    TimeIntervalCollectionPositionProperty::new(reference),
                    TimeIntervalCollectionPositionProperty::new(reference),
                );
                a.intervals().add_interval(TimeInterval::new(date(0.), date(1.), true, true, Some(Rc::new(one))));
                b.intervals().add_interval(TimeInterval::new(date(0.), date(1.), true, true, Some(Rc::new(two))));
                res(a.equals(&b))
            }
        }
        _ => None,
    }
}
#[test]
fn interval_position_property() {
    run("data_sources", "TimeIntervalCollectionPositionProperty", |_| TIGHT, |f, a| interval_case(f, a, false));
}
#[test]
fn composite_position_property() {
    run("data_sources", "CompositePositionProperty", |_| TIGHT, |f, a| interval_case(f, a, true));
}
#[test]
fn position_property_array() {
    run(
        "data_sources",
        "PositionPropertyArray",
        |_| TIGHT,
        |f, a| {
            if f != "array" {
                return None;
            }
            let ctx = context(s(&a[0]));
            let reference = frame(&a[1]);
            let empty = a[2].as_bool().unwrap();
            let first = Rc::new(ConstantPositionProperty::with_context(None, ReferenceFrame::Fixed, ctx.clone()));
            let second = Rc::new(ConstantPositionProperty::with_context(
                Some(Cartesian3::new(6378137., 200., 300.)),
                ReferenceFrame::Inertial,
                ctx.clone(),
            ));
            let children =
                if empty { vec![] } else { vec![first.clone() as PositionPropertyRef, second.clone(), first.clone()] };
            let p = PositionPropertyArray::with_context(Some(children.clone()), reference, ctx.clone());
            let (changes, _guard) = count(p.definition_changed());
            let listeners = || {
                [first.definition_changed().number_of_listeners(), second.definition_changed().number_of_listeners()]
            };
            let initial = json!([p.is_constant(), p.reference_frame() as u8, array_snapshots(&p, &a[3]), listeners()]);
            first.set_value(Some(Cartesian3::new(1., 2., 3.)), None);
            let after = json!([array_snapshots(&p, &a[3]), changes.get()]);
            let other = PositionPropertyArray::with_context(Some(children), reference, ctx);
            let eq = p.equals(&other).unwrap();
            other.set_value(None);
            p.set_value(None);
            ok(json!([
                initial,
                after,
                eq,
                p.is_constant(),
                p.get_value(Some(&date(0.)), None).unwrap().enc(),
                changes.get(),
                listeners()
            ]))
        },
    );
}
#[test]
fn scaled_position_property() {
    run(
        "data_sources",
        "ScaledPositionProperty",
        |_| TIGHT,
        |f, a| {
            if f != "scaled" {
                return None;
            }
            let mut ctx = context(s(&a[0]));
            let radii = c3(&a[2]);
            ctx.ellipsoid = Rc::new(move || Ellipsoid::new(radii.x, radii.y, radii.z));
            let child = Rc::new(ConstantPositionProperty::with_context(
                Some(Cartesian3::new(6378137., 200., 300.)),
                frame(&a[1]),
                ctx.clone(),
            ));
            let p = ScaledPositionProperty::with_context(Some(child.clone()), ctx.clone());
            let (changes, _guard) = count(p.definition_changed());
            let initial = json!([
                p.is_constant(),
                p.reference_frame() as u8,
                snapshots(&p, &a[3]),
                p.equals(&ScaledPositionProperty::with_context(Some(child.clone()), ctx)).unwrap()
            ]);
            child.set_value(Some(Cartesian3::ZERO), None);
            let mut r = Cartesian3::new(7., 8., 9.);
            let v = p.get_value(Some(&date(0.)), Some(&mut r)).unwrap();
            let zero = json!([v.enc(), r.enc(), changes.get()]);
            p.set_value(Some(child));
            p.set_value(None);
            ok(json!([
                initial,
                zero,
                p.is_constant(),
                p.reference_frame() as u8,
                p.get_value(Some(&date(0.)), None).unwrap().enc(),
                changes.get()
            ]))
        },
    );
}
fn velocity_scratch(orientation: bool) -> Option<Out> {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let out = trace.clone();
    let position = Rc::new(CallbackPositionProperty::new(
        Rc::new(move |time, result| {
            let t = JulianDate::seconds_difference(time, &date(0.));
            out.borrow_mut().push(json!([t, result.is_some()]));
            if t > 0. {
                return Ok(None);
            }
            let Some(result) = result else {
                return Ok(None);
            };
            *result = Cartesian3::new(6378137. + 120. * t, 200. + 30. * t, 300. + 5. * t);
            Ok(Some(*result))
        }),
        false,
        ReferenceFrame::Fixed,
    ));
    let values = if orientation {
        let property = VelocityOrientationProperty::new(Some(position), None);
        [0., -1. / 60., -1.].map(|t| property.get_value(Some(&date(t)), None).unwrap().enc())
    } else {
        let property = VelocityVectorProperty::new(Some(position), false);
        [0., -1. / 60., -1.].map(|t| property.get_value(Some(&date(t)), None).unwrap().enc())
    };
    let trace = trace.borrow().clone();
    ok(json!([values, trace]))
}
fn velocity_case(f: &str, a: &[Value], orientation: bool) -> Option<Out> {
    if f == "scratch" {
        return velocity_scratch(orientation);
    }
    if f != "velocity" {
        return None;
    }
    let ctx = context(s(&a[0]));
    let position = child(s(&a[1]), frame(&a[2]), ctx.clone());
    let normalize = a[3].as_bool().unwrap();
    if orientation {
        let p = VelocityOrientationProperty::with_context(position.clone(), None, ctx.clone());
        let (changes, _guard) = count(p.definition_changed());
        let values = list(&a[4], |t| p.get_value(Some(&date(n(t))), None).unwrap());
        let eq = p
            .equals(&VelocityOrientationProperty::with_context(position, Some(Rc::new(Ellipsoid::WGS84)), ctx))
            .unwrap();
        let initial = json!([p.is_constant(), values.enc(), eq]);
        p.set_ellipsoid(p.ellipsoid());
        p.set_ellipsoid(Rc::new(Ellipsoid::UNIT_SPHERE));
        p.set_ellipsoid(p.ellipsoid());
        let changed = json!([changes.get(), list(&a[4], |t| p.get_value(Some(&date(n(t))), None).unwrap()).enc()]);
        p.set_position(None);
        p.set_position(None);
        ok(json!([initial, changed, changes.get(), p.is_constant(), p.get_value(Some(&date(0.)), None).unwrap().enc()]))
    } else {
        let p = VelocityVectorProperty::with_context(position.clone(), normalize, ctx.clone());
        let (changes, _guard) = count(p.definition_changed());
        let values = list(&a[4], |t| p.get_value(Some(&date(n(t))), None).unwrap());
        let eq = p.equals(&VelocityVectorProperty::with_context(position, !normalize, ctx)).unwrap();
        let initial = json!([p.is_constant(), values.enc(), eq]);
        p.set_normalize(normalize);
        p.set_normalize(!normalize);
        p.set_normalize(!normalize);
        let changed = json!([changes.get(), list(&a[4], |t| p.get_value(Some(&date(n(t))), None).unwrap()).enc()]);
        p.set_position(None);
        p.set_position(None);
        ok(json!([initial, changed, changes.get(), p.is_constant(), p.get_value(Some(&date(0.)), None).unwrap().enc()]))
    }
}
#[test]
fn velocity_vector_property() {
    run("data_sources", "VelocityVectorProperty", |_| TIGHT_VECTOR, |f, a| velocity_case(f, a, false));
}
#[test]
fn velocity_orientation_property() {
    run("data_sources", "VelocityOrientationProperty", |_| TIGHT, |f, a| velocity_case(f, a, true));
}
