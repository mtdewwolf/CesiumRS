use cesium_rs::core::iau2006_xys_data::Iau2006XysDataOptions;
use cesium_rs::{core::*, data_sources::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn date(t: f64) -> JulianDate {
    JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap().add_seconds(t)
}
fn span<D>(data: D) -> TimeInterval<D> {
    TimeInterval::new(date(0.), date(10.), true, true, Some(data))
}
#[test]
fn frame_conversion_shortcuts_and_host_errors_preserve_the_result() {
    let calls = Rc::new(Cell::new(0));
    let n = calls.clone();
    let ctx = PositionContext {
        frame_transform: Rc::new(move |_| {
            n.set(n.get() + 1);
            Err(RuntimeError::new("host transform unavailable"))
        }),
        ..Default::default()
    };
    let mut result = Cartesian3::new(7., 8., 9.);
    assert_eq!(
        ctx.convert(
            &date(0.),
            None,
            FrameConversion { input: ReferenceFrame::Inertial, output: ReferenceFrame::Fixed },
            Some(&mut result)
        )
        .unwrap(),
        None
    );
    assert_eq!(result, Cartesian3::new(7., 8., 9.));
    assert_eq!(calls.get(), 0);
    assert_eq!(
        ctx.convert(&date(0.), Some(Cartesian3::UNIT_X), FrameConversion::default(), Some(&mut result)).unwrap(),
        Some(Cartesian3::UNIT_X)
    );
    assert_eq!(calls.get(), 0);
    assert!(ctx
        .convert(
            &date(0.),
            Some(Cartesian3::UNIT_Y),
            FrameConversion { input: ReferenceFrame::Inertial, output: ReferenceFrame::Fixed },
            Some(&mut result)
        )
        .is_err());
    assert_eq!(result, Cartesian3::UNIT_X);
    assert_eq!(calls.get(), 1);
}
#[test]
fn constant_position_notifies_after_committing_both_value_and_frame() {
    let ctx = PositionContext { frame_transform: Rc::new(|_| Ok(Matrix3::IDENTITY)), ..Default::default() };
    let p = Rc::new(ConstantPositionProperty::with_context(Some(Cartesian3::UNIT_X), ReferenceFrame::Fixed, ctx));
    let weak = Rc::downgrade(&p);
    let seen = Rc::new(RefCell::new(vec![]));
    let out = seen.clone();
    let _guard = p.definition_changed().add_listener(move |_| {
        let p = weak.upgrade().unwrap();
        out.borrow_mut().push((p.get_value(Some(&date(0.)), None).unwrap(), p.reference_frame()));
    });
    p.set_value(Some(Cartesian3::UNIT_Y), Some(ReferenceFrame::Inertial));
    p.set_value(Some(Cartesian3::UNIT_Y), Some(ReferenceFrame::Fixed));
    p.set_value(Some(Cartesian3::UNIT_Y), None);
    assert_eq!(
        *seen.borrow(),
        vec![(Some(Cartesian3::UNIT_Y), ReferenceFrame::Inertial), (Some(Cartesian3::UNIT_Y), ReferenceFrame::Fixed)]
    );
}
#[test]
fn position_callback_can_replace_itself_without_retaining_a_borrow() {
    let link = Rc::new(RefCell::new(None::<std::rc::Weak<CallbackPositionProperty>>));
    let shared = link.clone();
    let p = Rc::new(CallbackPositionProperty::new(
        Rc::new(move |_, _| {
            shared
                .borrow()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap()
                .set_callback(Rc::new(|_, _| Ok(Some(Cartesian3::UNIT_Y))), true);
            Ok(Some(Cartesian3::UNIT_X))
        }),
        false,
        ReferenceFrame::Fixed,
    ));
    *link.borrow_mut() = Some(Rc::downgrade(&p));
    assert_eq!(p.get_value(Some(&date(0.)), None).unwrap(), Some(Cartesian3::UNIT_X));
    assert_eq!(p.get_value(Some(&date(0.)), None).unwrap(), Some(Cartesian3::UNIT_Y));
    assert!(p.is_constant());
}
#[test]
fn position_array_keeps_one_snapshot_and_one_time_when_a_child_clears_it() {
    let link = Rc::new(RefCell::new(None::<std::rc::Weak<PositionPropertyArray>>));
    let shared = link.clone();
    let clocks = Rc::new(Cell::new(0));
    let c = clocks.clone();
    let ctx = PositionContext {
        clock: Rc::new(move || {
            c.set(c.get() + 1);
            date(c.get() as f64)
        }),
        ..Default::default()
    };
    let seen = Rc::new(RefCell::new(vec![]));
    let out = seen.clone();
    let cb = Rc::new(CallbackPositionProperty::new(
        Rc::new(move |time, _| {
            out.borrow_mut().push(*time);
            shared.borrow().as_ref().unwrap().upgrade().unwrap().set_value(None);
            Ok(Some(Cartesian3::UNIT_X))
        }),
        false,
        ReferenceFrame::Fixed,
    ));
    let out = seen.clone();
    let next = Rc::new(CallbackPositionProperty::new(
        Rc::new(move |time, _| {
            out.borrow_mut().push(*time);
            Ok(Some(Cartesian3::UNIT_Y))
        }),
        false,
        ReferenceFrame::Fixed,
    ));
    let p = Rc::new(PositionPropertyArray::with_context(
        Some(vec![cb.clone(), next.clone()]),
        ReferenceFrame::Inertial,
        ctx,
    ));
    *link.borrow_mut() = Some(Rc::downgrade(&p));
    let mut result = vec![Cartesian3::ZERO; 3];
    assert_eq!(p.get_value(None, Some(&mut result)).unwrap(), Some(vec![Cartesian3::UNIT_X, Cartesian3::UNIT_Y]));
    assert_eq!(result, vec![Cartesian3::UNIT_X, Cartesian3::UNIT_Y]);
    assert_eq!(*seen.borrow(), vec![date(1.), date(1.)]);
    assert_eq!(clocks.get(), 1);
    assert_eq!(cb.definition_changed().number_of_listeners(), 0);
    assert_eq!(next.definition_changed().number_of_listeners(), 0);
    assert_eq!(p.get_value(None, None).unwrap(), None);
    assert_eq!(clocks.get(), 2);
}
#[test]
fn position_composite_rewires_safely_inside_a_forwarded_change() {
    let child = Rc::new(ConstantPositionProperty::new(Some(Cartesian3::UNIT_X), ReferenceFrame::Fixed));
    let p = Rc::new(CompositePositionProperty::new(ReferenceFrame::Fixed));
    p.intervals().add_interval(span(child.clone() as PositionPropertyRef));
    let weak = Rc::downgrade(&p);
    let resetting = Rc::new(Cell::new(false));
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    let c = child.clone();
    let _guard = p.definition_changed().add_listener(move |_| {
        if !resetting.replace(true) {
            seen.set(seen.get() + 1);
            let p = weak.upgrade().unwrap();
            p.intervals().remove_all();
            p.intervals().add_interval(span(c.clone() as PositionPropertyRef));
            resetting.set(false);
        }
    });
    for (i, value) in [Cartesian3::UNIT_Y, Cartesian3::UNIT_Z, Cartesian3::UNIT_X].into_iter().enumerate() {
        child.set_value(Some(value), None);
        assert_eq!(calls.get(), i + 1);
        assert_eq!(child.definition_changed().number_of_listeners(), 1);
        assert_eq!(p.get_value(Some(&date(1.)), None).unwrap(), Some(value));
    }
    drop(p);
    assert_eq!(child.definition_changed().number_of_listeners(), 0);
}
#[test]
fn position_wrappers_forward_child_changes_and_drop_their_subscriptions() {
    let child =
        Rc::new(ConstantPositionProperty::new(Some(Cartesian3::new(6378137., 10., 20.)), ReferenceFrame::Fixed));
    let scaled = ScaledPositionProperty::new(Some(child.clone()));
    let velocity = VelocityVectorProperty::new(Some(child.clone()), true);
    let orientation = VelocityOrientationProperty::new(Some(child.clone()), None);
    assert_eq!(child.definition_changed().number_of_listeners(), 3);
    let seen = Rc::new(Cell::new(0));
    let n = seen.clone();
    let _s = scaled.definition_changed().add_listener(move |_| n.set(n.get() + 1));
    let n = seen.clone();
    let _v = velocity.definition_changed().add_listener(move |_| n.set(n.get() + 1));
    let n = seen.clone();
    let _o = orientation.definition_changed().add_listener(move |_| n.set(n.get() + 1));
    child.set_value(Some(Cartesian3::new(6378138., 11., 21.)), None);
    assert_eq!(seen.get(), 3);
    drop(scaled);
    assert_eq!(child.definition_changed().number_of_listeners(), 2);
    velocity.set_position(None);
    assert_eq!(child.definition_changed().number_of_listeners(), 1);
    drop(orientation);
    assert_eq!(child.definition_changed().number_of_listeners(), 0);
}
#[test]
fn motion_properties_have_independent_scratch_during_nested_evaluation() {
    let other = Rc::new(VelocityVectorProperty::new(
        Some(Rc::new(CallbackProperty::new(
            Rc::new(|time, _| {
                let t = JulianDate::seconds_difference(time, &date(0.));
                Ok(Some(Cartesian3::new(0., 0., t * 5.)))
            }),
            false,
        ))),
        false,
    ));
    let nested = other.clone();
    let child = Rc::new(CallbackProperty::new(
        Rc::new(move |time, _| {
            assert!(nested.get_value(Some(time), None)?.unwrap().equals_epsilon(Cartesian3::new(0., 0., 5.), 1e-8));
            let t = JulianDate::seconds_difference(time, &date(0.));
            Ok(Some(Cartesian3::new(6378137. + t * 120., t * 30., 0.)))
        }),
        false,
    ));
    let velocity = VelocityVectorProperty::new(Some(child.clone()), false);
    let orientation = VelocityOrientationProperty::new(Some(child), None);
    let v = velocity.get_value(Some(&date(1.)), None).unwrap().unwrap();
    assert!(v.equals_epsilon(Cartesian3::new(120., 30., 0.), 1e-6));
    let q = orientation.get_value(Some(&date(1.)), None).unwrap().unwrap();
    let expected = Quaternion::from_rotation_matrix(&Transforms::rotation_matrix_from_position_velocity(
        Cartesian3::new(6378257., 30., 0.),
        v.normalize(),
        &Ellipsoid::WGS84,
    ));
    assert!(q.equals_epsilon(&expected, 1e-10));
}
#[test]
fn positions_see_shared_earth_data_loaded_after_construction() {
    let data = Rc::new(RefCell::new(Iau2006XysData::new(Iau2006XysDataOptions {
        interpolation_order: 1,
        sample_zero_julian_ephemeris_date: 2458848.5,
        step_size_days: 1.,
        samples_per_xys_file: 4,
        total_samples: 4,
        ..Default::default()
    })));
    let eop = Rc::new(EarthOrientationParameters::none());
    let p = ConstantPositionProperty::with_context(
        Some(Cartesian3::new(6378137., 200., 300.)),
        ReferenceFrame::Inertial,
        PositionContext::with_earth_data(eop.clone(), data.clone()),
    );
    let before = p.get_value(Some(&date(0.)), None).unwrap().unwrap();
    data.borrow_mut()
        .add_chunk_json(
            0,
            r#"{"samples":[0.001,0.002,0.0001,0.0011,0.0022,0.00012,0.0012,0.0024,0.00014,0.0013,0.0026,0.00016]}"#,
        )
        .unwrap();
    let after = p.get_value(Some(&date(0.)), None).unwrap().unwrap();
    let expected = Transforms::compute_icrf_to_fixed_matrix(&date(0.), &eop, &data.borrow())
        .unwrap()
        .multiply_by_vector(Cartesian3::new(6378137., 200., 300.));
    assert_eq!(after, expected);
    assert!(!after.equals_epsilon(before, 1e-6));
}
