//! Direct replay of the pinned CesiumJS DataSources property scenarios.
mod common;
use cesium_rs::data_sources::property;
use cesium_rs::{core::*, data_sources::*};
use common::*;
use serde_json::{json, Value};
use std::{cell::Cell, rc::Rc};
fn date(t: f64) -> JulianDate {
    JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap().add_seconds(t)
}
fn sec(t: Option<JulianDate>) -> Value {
    t.map(|t| JulianDate::seconds_difference(&t, &date(0.))).enc()
}
fn count(event: &DefinitionChanged) -> (Rc<Cell<usize>>, Subscription) {
    let counter = Rc::new(Cell::new(0));
    let c = counter.clone();
    let subscription = event.add_listener(move |_| c.set(c.get() + 1));
    (counter, subscription)
}
fn interval<D>(a: &[Value], data: Option<D>) -> TimeInterval<D> {
    TimeInterval::new(date(n(&a[0])), date(n(&a[1])), a[2].as_bool().unwrap(), a[3].as_bool().unwrap(), data)
}
fn nullable<T>(v: &Value, decode: impl Fn(&Value) -> T) -> Option<Option<T>> {
    if v.as_str() == Some("undefined") {
        None
    } else {
        Some((!v.is_null()).then(|| decode(v)))
    }
}
fn constant_sequence<T: PropertyValue + Enc>(values: &Value, decode: impl Fn(&Value) -> T) -> Value {
    let p = ConstantProperty::<Option<T>>::new(None);
    let (changes, _guard) = count(p.definition_changed());
    list(values, |v| {
        p.set_value(nullable(v, &decode));
        let other = ConstantProperty::new(nullable(v, &decode));
        json!([
            p.is_constant(),
            p.get_value(Some(&date(5.)), None).unwrap().enc(),
            p.to_string(),
            changes.get(),
            p.equals(&other).unwrap(),
            p.equals(&p).unwrap(),
            p.value_of().enc()
        ])
    })
    .enc()
}
#[test]
fn constant_property() {
    run(
        "data_sources",
        "ConstantProperty",
        |_| TIGHT,
        |f, a| match f {
            "sequence" => ok(match s(&a[0]) {
                "number" => constant_sequence(&a[1], n),
                "Cartesian2" => constant_sequence(&a[1], c2),
                "Cartesian3" => constant_sequence(&a[1], c3),
                "Cartesian4" => constant_sequence(&a[1], c4),
                "Color" => constant_sequence(&a[1], color),
                "Quaternion" => constant_sequence(&a[1], q),
                "Matrix2" => constant_sequence(&a[1], m2),
                "Matrix3" => constant_sequence(&a[1], m3),
                "Matrix4" => constant_sequence(&a[1], m4),
                _ => unreachable!(),
            }),
            "reference" => {
                let object = JsonReference::object(json!({"a":1}).as_object().unwrap().clone());
                let p = ConstantProperty::new(Some(object.clone()));
                let (changes, _guard) = count(p.definition_changed());
                p.set_value(Some(object.clone()));
                let same = p.equals(&ConstantProperty::new(Some(object.clone()))).unwrap();
                object.value_mut()["a"] = json!(2);
                let value = p.get_value(None, None).unwrap().unwrap().value().clone();
                let other = || JsonReference::object(json!({"a":2}).as_object().unwrap().clone());
                p.set_value(Some(other()));
                ok(json!([
                    same,
                    value,
                    changes.get(),
                    p.equals(&ConstantProperty::new(Some(other()))).unwrap(),
                    p.to_string()
                ]))
            }
            _ => None,
        },
    );
}
#[test]
fn callback_property() {
    run(
        "data_sources",
        "CallbackProperty",
        |_| TIGHT,
        |f, a| {
            if f != "evaluate" {
                return None;
            }
            let flag = a[0].as_bool().unwrap();
            let calls = Rc::new(Cell::new(0));
            let c = calls.clone();
            let cb: PropertyCallback<Cartesian3> = Rc::new(move |time, result| {
                c.set(c.get() + 1);
                let result = result.unwrap();
                let old = result.x;
                let t = JulianDate::seconds_difference(time, &date(0.));
                *result = Cartesian3::new(t, t * 2., old);
                Ok(Some(*result))
            });
            let p = CallbackProperty::new(cb.clone(), flag);
            let (changes, _guard) = count(p.definition_changed());
            let mut result = Cartesian3::new(7., 8., 9.);
            let values = a[1]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| p.get_value(Some(&date(n(t))), Some(&mut result)).unwrap())
                .collect::<Vec<_>>();
            let eq = p.equals(&CallbackProperty::new(cb.clone(), flag)).unwrap();
            p.set_callback(cb.clone(), flag);
            p.set_callback(cb.clone(), !flag);
            p.set_callback(cb, !flag);
            let different = p.equals(&CallbackProperty::new(Rc::new(|_, _| Ok(None)), !flag)).unwrap();
            ok(json!([values.enc(), result.enc(), calls.get(), changes.get(), p.is_constant(), eq, different]))
        },
    );
}
#[test]
fn property_helpers() {
    run(
        "data_sources",
        "Property",
        |_| TIGHT,
        |f, a| match f {
            "helpers" => {
                let p: Option<PropertyRef<Option<f64>>> =
                    if a[0] == "absent" { None } else { Some(Rc::new(ConstantProperty::new(nullable(&a[0], n)))) };
                let reference = p.as_deref();
                let array = p.iter().cloned().collect::<Vec<_>>();
                ok(json!([
                    property::is_constant(reference),
                    property::get_value_or_undefined(reference, Some(&date(0.)), None).unwrap().enc(),
                    property::get_value_or_default(reference, Some(&date(0.)), Some(9.), None).unwrap().enc(),
                    property::equals(p.as_ref(), p.as_ref()).unwrap(),
                    property::equals(p.as_ref(), None).unwrap(),
                    property::array_equals(Some(&array), Some(&array)).unwrap(),
                    property::array_equals::<Option<f64>>(None, Some(&[])).unwrap()
                ]))
            }
            "eventEdits" => {
                use std::cell::RefCell;
                let p = ConstantProperty::new(Some(0.));
                let e = p.definition_changed();
                let trace = Rc::new(RefCell::new(Vec::new()));
                let edited = Rc::new(Cell::new(false));
                let remove_b = Rc::new(RefCell::new(None::<Subscription>));
                let add_c = Rc::new(RefCell::new(None::<Subscription>));
                let t = trace.clone();
                let c: PropertyListener = Rc::new(move |_| t.borrow_mut().push("c"));
                let t = trace.clone();
                let b = remove_b.clone();
                let cc = add_c.clone();
                let _a = e.add_listener(move |e| {
                    t.borrow_mut().push("a");
                    if !edited.replace(true) {
                        b.borrow_mut().as_mut().unwrap().remove();
                        *cc.borrow_mut() = Some(e.subscribe(c.clone()));
                    }
                });
                let t = trace.clone();
                *remove_b.borrow_mut() = Some(e.add_listener(move |_| t.borrow_mut().push("b")));
                p.set_value(Some(1.));
                p.set_value(Some(2.));
                let listener: PropertyListener = Rc::new(|_| {});
                let mut first = e.subscribe(listener.clone());
                let mut second = e.subscribe(listener);
                let before = e.number_of_listeners();
                first.remove();
                second.remove();
                let trace = trace.borrow().clone();
                ok(json!([trace, before, e.number_of_listeners()]))
            }
            "eventReentry" => {
                use std::cell::RefCell;
                let p = Rc::new(ConstantProperty::new(Some(0.)));
                let trace = Rc::new(RefCell::new(Vec::new()));
                let nested = Rc::new(Cell::new(false));
                let extra = Rc::new(RefCell::new(None::<Subscription>));
                let weak = Rc::downgrade(&p);
                let t = trace.clone();
                let c: PropertyListener = Rc::new(move |_| t.borrow_mut().push("c"));
                let t = trace.clone();
                let _a = p.definition_changed().add_listener(move |e| {
                    t.borrow_mut().push("a");
                    if !nested.replace(true) {
                        *extra.borrow_mut() = Some(e.subscribe(c.clone()));
                        weak.upgrade().unwrap().set_value(Some(2.));
                    }
                });
                let t = trace.clone();
                let _b = p.definition_changed().add_listener(move |_| t.borrow_mut().push("b"));
                p.set_value(Some(1.));
                let trace = trace.borrow().clone();
                ok(json!([trace, p.definition_changed().number_of_listeners()]))
            }
            "clonedDefault" => {
                let p = CallbackProperty::new(
                    Rc::new(|_, r: Option<&mut Cartesian3>| {
                        r.unwrap().x = 77.;
                        Ok(None)
                    }),
                    false,
                );
                let mut result = Cartesian3::new(1., 2., 3.);
                let fallback = Cartesian3::new(4., 5., 6.);
                let mut value =
                    property::get_value_or_cloned_default(Some(&p), Some(&date(0.)), &fallback, Some(&mut result))
                        .unwrap();
                value.x = 99.;
                ok(json!([value.enc(), result.enc(), fallback.enc(), false, false]))
            }
            _ => None,
        },
    );
}
#[test]
fn property_array() {
    run(
        "data_sources",
        "PropertyArray",
        |_| TIGHT,
        |f, a| {
            if f != "sequence" {
                return None;
            }
            let variable = a[1].as_bool().unwrap();
            let children: Vec<PropertyRef<Option<f64>>> = list(&a[0], |v| {
                let value = nullable(v, n);
                if variable {
                    Rc::new(CallbackProperty::new(Rc::new(move |_, _| Ok(value)), false)) as PropertyRef<_>
                } else {
                    Rc::new(ConstantProperty::new(value)) as PropertyRef<_>
                }
            });
            let mut values = children.clone();
            if let Some(first) = children.first() {
                values.push(first.clone());
            }
            let p = PropertyArray::new(Some(values.clone()));
            let (changes, _guard) = count(p.definition_changed());
            let listeners =
                || children.iter().map(|p| p.definition_changed().number_of_listeners()).collect::<Vec<_>>();
            let initial = json!([
                p.is_constant(),
                p.get_value(Some(&date(0.)), None).unwrap().enc(),
                listeners(),
                p.equals(&PropertyArray::new(Some(values))).unwrap()
            ]);
            if let Some(first) = children.first() {
                if variable {
                    first
                        .as_any()
                        .downcast_ref::<CallbackProperty<Option<f64>>>()
                        .unwrap()
                        .set_callback(Rc::new(|_, _| Ok(Some(Some(5.)))), true);
                } else {
                    first.as_any().downcast_ref::<ConstantProperty<Option<f64>>>().unwrap().set_value(Some(Some(5.)));
                }
            }
            let changed = json!([p.is_constant(), p.get_value(Some(&date(1.)), None).unwrap().enc(), changes.get()]);
            p.set_value(None);
            let cleared =
                json!([p.is_constant(), p.get_value(Some(&date(1.)), None).unwrap().enc(), changes.get(), listeners()]);
            p.set_value(Some(vec![]));
            ok(json!([initial, changed, cleared, p.get_value(Some(&date(1.)), None).unwrap().enc(), changes.get()]))
        },
    );
}
fn intervals_port<T: PropertyValue + Enc>(
    f: &str,
    a: &[Value],
    composite: bool,
    decode: impl Fn(&Value) -> T,
) -> Result<Value, RuntimeError> {
    let raw = TimeIntervalCollectionProperty::<T>::new();
    let cp = CompositeProperty::<T>::new();
    let property: &dyn Property<T> = if composite { &cp } else { &raw };
    if f == "equals" {
        let equal = a[1].as_bool().unwrap();
        let v = if a[0] == "number" { json!(1) } else { json!([1, 2, 3]) };
        let w = if equal {
            v.clone()
        } else if a[0] == "number" {
            json!(2)
        } else {
            json!([4, 5, 6])
        };
        let data = decode(&v);
        let other_data = decode(&w);
        let span = list(&json!([0, 10, true, false]), Clone::clone);
        if composite {
            cp.intervals()
                .add_interval(interval(&span, Some(Rc::new(ConstantProperty::new(Some(data))) as PropertyRef<T>)));
            let other = CompositeProperty::<T>::new();
            other.intervals().add_interval(interval(
                &span,
                Some(Rc::new(ConstantProperty::new(Some(other_data))) as PropertyRef<T>),
            ));
            return Ok(json!(cp.equals(&other)?));
        }
        raw.intervals().add_interval(interval(&span, Some(Rc::new(data))));
        let other = TimeIntervalCollectionProperty::<T>::new();
        other.intervals().add_interval(interval(&span, Some(Rc::new(other_data))));
        return Ok(json!(raw.equals(&other)?));
    }
    let raw_data = list(&a[1], |v| Rc::new(decode(v)));
    let data = list(&a[1], |v| Rc::new(ConstantProperty::new(Some(decode(v)))));
    let (changes, _guard) = count(property.definition_changed());
    let mut out = vec![];
    for op in a[2].as_array().unwrap() {
        let op = op.as_array().unwrap();
        let mut removed = None;
        match s(&op[0]) {
            "add" => {
                let id = op[5].as_u64().unwrap() as usize;
                if composite {
                    cp.intervals().add_interval(interval(&op[1..], Some(data[id].clone() as PropertyRef<T>)));
                } else {
                    raw.intervals().add_interval(interval(&op[1..], Some(raw_data[id].clone())));
                }
            }
            "remove" => {
                removed = Some(if composite {
                    cp.intervals().remove_interval(&interval(&op[1..], None))
                } else {
                    raw.intervals().remove_interval(&interval(&op[1..], None))
                });
            }
            "clear" => {
                if composite {
                    cp.intervals().remove_all();
                } else {
                    raw.intervals().remove_all();
                }
            }
            "set" => {
                if composite {
                    data[op[1].as_u64().unwrap() as usize].set_value(Some(decode(&op[2])));
                }
            }
            _ => unreachable!(),
        }
        let values = list(&a[3], |t| property.get_value(Some(&date(n(t))), None).unwrap());
        let listeners = if composite {
            Some(data.iter().map(|p| p.definition_changed().number_of_listeners()).collect::<Vec<_>>())
        } else {
            None
        };
        out.push(json!([
            changes.get(),
            property.is_constant(),
            if composite { cp.intervals().len() } else { raw.intervals().len() },
            values.enc(),
            removed,
            listeners
        ]));
    }
    Ok(out.enc())
}
fn intervals_case(f: &str, a: &[Value], composite: bool) -> Option<Out> {
    match s(&a[0]) {
        "number" => res(intervals_port(f, a, composite, n)),
        "Cartesian3" => res(intervals_port(f, a, composite, c3)),
        _ => None,
    }
}
#[test]
fn time_interval_collection_property() {
    run("data_sources", "TimeIntervalCollectionProperty", |_| TIGHT, |f, a| intervals_case(f, a, false));
}
#[test]
fn composite_property() {
    run("data_sources", "CompositeProperty", |_| TIGHT, |f, a| intervals_case(f, a, true));
}
fn evaluate<T: SampledValue + Enc>(p: &SampledProperty<T>, queries: &Value) -> Value {
    list(queries, |t| p.get_value(Some(&date(n(t))), None).unwrap()).enc()
}
fn sampled_port<T: SampledValue + Enc>(a: &[Value], decode: impl Fn(&Value) -> T) -> Value {
    let order = n(&a[3]) as usize;
    let p = SampledProperty::<T>::new((order > 0).then(|| vec![SampleType::of::<T>(); order]));
    let (changes, _guard) = count(p.definition_changed());
    let algorithm = match s(&a[1]) {
        "LinearApproximation" => Interpolation::Linear,
        "LagrangePolynomialApproximation" => Interpolation::Lagrange,
        _ => Interpolation::Hermite,
    };
    p.set_interpolation_options(Some(algorithm), Some(n(&a[2])));
    let derivatives = list(&a[6], |row| list(row, nums));
    p.add_samples(&list(&a[4], |t| date(n(t))), &list(&a[5], decode), (order > 0).then_some(derivatives.as_slice()))
        .unwrap();
    let initial =
        json!([evaluate(&p, &a[7]), changes.get(), p.is_constant(), ([-7, -1, 0, 4, 5].map(|i| sec(p.get_sample(i))))]);
    p.set_forward_extrapolation_type(ExtrapolationType::Hold);
    p.set_backward_extrapolation_type(ExtrapolationType::Hold);
    let hold = evaluate(&p, &a[7]);
    p.set_forward_extrapolation_type(ExtrapolationType::Extrapolate);
    p.set_backward_extrapolation_type(ExtrapolationType::Extrapolate);
    p.set_forward_extrapolation_duration(3.);
    p.set_backward_extrapolation_duration(3.);
    let extrap = evaluate(&p, &a[7]);
    let removed = [p.remove_sample(&date(7.)), p.remove_sample(&date(10.))];
    p.remove_samples(&TimeInterval::<()>::new(date(4.), date(15.), false, true, None));
    let final_state =
        json!([evaluate(&p, &a[7]), changes.get(), p.is_constant(), ([-1, 0, 1, 2].map(|i| sec(p.get_sample(i))))]);
    json!([initial, hold, extrap, removed, final_state])
}
#[test]
fn sampled_property() {
    run(
        "data_sources",
        "SampledProperty",
        |_| TIGHT,
        |f, a| match f {
            "samples" => ok(match s(&a[0]) {
                "number" => sampled_port(a, n),
                "Cartesian2" => sampled_port(a, c2),
                "Cartesian3" => sampled_port(a, c3),
                "Color" => sampled_port(a, color),
                "Quaternion" => sampled_port(a, q),
                _ => unreachable!(),
            }),
            "emptyDerivatives" => {
                let p = SampledProperty::<f64>::new(Some(vec![]));
                let other = SampledProperty::<f64>::new(None);
                let (changes, _guard) = count(p.definition_changed());
                p.add_sample(date(0.), &1., None).unwrap();
                p.add_samples(&[date(10.)], &[3.], None).unwrap();
                other.add_samples(&[date(0.), date(10.)], &[1., 3.], None).unwrap();
                ok(json!([
                    p.get_value(Some(&date(5.)), None).unwrap().enc(),
                    p.equals(&other).unwrap(),
                    changes.get(),
                    p.derivative_types().unwrap().len()
                ]))
            }
            "packed" => {
                let p = SampledProperty::<f64>::new(None);
                let (changes, _guard) = count(p.definition_changed());
                let times = nums(&a[1]);
                let values = nums(&a[2]);
                let kind = n(&a[0]) as usize;
                let data = times
                    .iter()
                    .zip(&values)
                    .flat_map(|(t, v)| {
                        [
                            match kind {
                                0 => PackedSampleValue::Number(*t),
                                1 => PackedSampleValue::Date(date(*t)),
                                _ => PackedSampleValue::Iso8601(date(*t).to_iso8601(None)),
                            },
                            PackedSampleValue::Number(*v),
                        ]
                    })
                    .collect::<Vec<_>>();
                p.add_samples_packed_array(&data, (kind == 0).then(|| date(0.))).unwrap();
                let other = SampledProperty::<f64>::new(None);
                let data = times
                    .iter()
                    .zip(&values)
                    .flat_map(|(t, v)| [PackedSampleValue::Number(*t), PackedSampleValue::Number(*v)])
                    .collect::<Vec<_>>();
                other.add_samples_packed_array(&data, Some(date(0.))).unwrap();
                other.set_forward_extrapolation_type(ExtrapolationType::Hold);
                let eq = p.equals(&other).unwrap();
                p.remove_samples(&TimeInterval::<()>::new(date(100.), date(200.), true, true, None));
                ok(json!([evaluate(&p, &a[3]), changes.get(), p.is_constant(), eq]))
            }
            _ => None,
        },
    );
}
