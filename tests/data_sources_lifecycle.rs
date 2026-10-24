//! Ownership and event/callback integration that JSON fixtures cannot express.
use cesium_rs::{core::*, data_sources::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn date(t: f64) -> JulianDate {
    JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap().add_seconds(t)
}
fn span<D>(start: f64, stop: f64, data: D) -> TimeInterval<D> {
    TimeInterval::new(date(start), date(stop), true, true, Some(data))
}
#[test]
fn observers_can_read_and_change_committed_values() {
    let p = Rc::new(ConstantProperty::new(Some(1.)));
    let weak = Rc::downgrade(&p);
    let seen = Rc::new(RefCell::new(vec![]));
    let out = seen.clone();
    let _guard = p.definition_changed().add_listener(move |_| {
        let p = weak.upgrade().unwrap();
        let v = p.get_value(None, None).unwrap().unwrap();
        out.borrow_mut().push(v);
        if v == 2. {
            p.set_value(Some(3.));
        }
    });
    p.set_value(Some(2.));
    assert_eq!(*seen.borrow(), vec![2., 3.]);
}
#[test]
fn child_subscriptions_are_unique_and_clean_up_with_owners() {
    let child = Rc::new(ConstantProperty::new(Some(1.)));
    let parent = Rc::new(PropertyArray::new(Some(vec![child.clone(), child.clone()])));
    let weak = Rc::downgrade(&parent);
    assert_eq!(child.definition_changed().number_of_listeners(), 1);
    let seen = Rc::new(Cell::new(0));
    let out = seen.clone();
    let _guard = parent.definition_changed().add_listener(move |_| out.set(out.get() + 1));
    child.set_value(Some(2.));
    assert_eq!(seen.get(), 1);
    drop(parent);
    assert!(weak.upgrade().is_none());
    assert_eq!(child.definition_changed().number_of_listeners(), 0);
}
#[test]
fn composite_rewires_before_observers_run_and_releases_old_children() {
    let a = Rc::new(ConstantProperty::new(Some(1.)));
    let b = Rc::new(ConstantProperty::new(Some(2.)));
    let parent = Rc::new(CompositeProperty::new());
    let weak = Rc::downgrade(&parent);
    parent.intervals().add_interval(span(0., 10., a.clone() as PropertyRef<f64>));
    let seen = Rc::new(RefCell::new(vec![]));
    let out = seen.clone();
    let _guard = parent.definition_changed().add_listener(move |_| {
        out.borrow_mut().push(weak.upgrade().unwrap().get_value(Some(&date(5.)), None).unwrap())
    });
    parent.intervals().add_interval(span(0., 10., b.clone() as PropertyRef<f64>));
    assert_eq!(a.definition_changed().number_of_listeners(), 0);
    assert_eq!(b.definition_changed().number_of_listeners(), 1);
    a.set_value(Some(4.));
    b.set_value(Some(7.));
    assert_eq!(*seen.borrow(), vec![Some(2.), Some(7.)]);
    drop(parent);
    assert_eq!(b.definition_changed().number_of_listeners(), 0);
}
#[test]
fn interval_observers_can_query_and_mutate_the_collection() {
    let p = Rc::new(TimeIntervalCollectionProperty::new());
    let weak = Rc::downgrade(&p);
    let _guard = p.definition_changed().add_listener(move |_| {
        let p = weak.upgrade().unwrap();
        if p.intervals().len() == 1 {
            assert_eq!(p.get_value(Some(&date(1.)), None).unwrap(), Some(2.));
            p.intervals().add_interval(span(10., 20., Rc::new(3.)));
        }
    });
    p.intervals().add_interval(span(0., 5., Rc::new(2.)));
    assert_eq!(p.intervals().len(), 2);
}
#[test]
fn default_time_is_sampled_once_and_shared_across_the_array() {
    let clocks = Rc::new(Cell::new(0));
    let c = clocks.clone();
    let clock: PropertyClock = Rc::new(move || {
        c.set(c.get() + 1);
        date(c.get() as f64)
    });
    let cb: PropertyCallback<f64> = Rc::new(|time, _| Ok(Some(JulianDate::seconds_difference(time, &date(0.)))));
    let child = Rc::new(CallbackProperty::with_clock(cb, false, Rc::new(|| panic!("child clock must not be read"))));
    let p = PropertyArray::with_clock(Some(vec![child.clone(), child]), clock);
    assert_eq!(p.get_value(None, None).unwrap(), Some(vec![1., 1.]));
    assert_eq!(clocks.get(), 1);
    assert_eq!(p.get_value(Some(&date(7.)), None).unwrap(), Some(vec![7., 7.]));
    assert_eq!(clocks.get(), 1);
}
#[test]
fn callback_can_replace_itself_while_evaluating() {
    let link = Rc::new(RefCell::new(None::<std::rc::Weak<CallbackProperty<f64>>>));
    let shared = link.clone();
    let p = Rc::new(CallbackProperty::new(
        Rc::new(move |_, _| {
            shared.borrow().as_ref().unwrap().upgrade().unwrap().set_callback(Rc::new(|_, _| Ok(Some(2.))), true);
            Ok(Some(1.))
        }),
        false,
    ));
    *link.borrow_mut() = Some(Rc::downgrade(&p));
    assert_eq!(p.get_value(Some(&date(0.)), None).unwrap(), Some(1.));
    assert_eq!(p.get_value(Some(&date(0.)), None).unwrap(), Some(2.));
    assert!(p.is_constant());
}
#[test]
fn cloned_values_and_results_do_not_alias_storage() {
    let p = ConstantProperty::new(Some(Cartesian3::new(1., 2., 3.)));
    let mut result = Cartesian3::ZERO;
    assert_eq!(p.get_value(None, Some(&mut result)).unwrap(), Some(Cartesian3::new(1., 2., 3.)));
    result.x = 99.;
    let mut extracted = p.value_of().unwrap();
    extracted.y = 99.;
    assert_eq!(p.get_value(None, None).unwrap(), Some(Cartesian3::new(1., 2., 3.)));
    let raw = TimeIntervalCollectionProperty::new();
    raw.intervals().add_interval(span(0., 10., Rc::new(Cartesian3::new(4., 5., 6.))));
    raw.get_value(Some(&date(3.)), Some(&mut result)).unwrap();
    result.z = 99.;
    assert_eq!(result.z, 99.);
    assert_eq!(raw.get_value(Some(&date(3.)), None).unwrap(), Some(Cartesian3::new(4., 5., 6.)));
    assert_eq!(extracted.y, 99.);
}
#[test]
fn sampled_change_observers_see_new_samples() {
    let p = Rc::new(SampledProperty::<f64>::new(None));
    let weak = Rc::downgrade(&p);
    let seen = Rc::new(RefCell::new(vec![]));
    let out = seen.clone();
    let _guard = p.definition_changed().add_listener(move |_| {
        out.borrow_mut().push(weak.upgrade().unwrap().get_value(Some(&date(5.)), None).unwrap())
    });
    p.add_samples(&[date(0.), date(10.)], &[0., 10.], None).unwrap();
    p.add_sample(date(10.), &20., None).unwrap();
    assert_eq!(*seen.borrow(), vec![Some(5.), Some(10.)]);
}
#[test]
fn custom_interpolation_points_cache_tracks_sample_and_option_changes() {
    use cesium_rs::core::interpolation_algorithm::{InterpolationAlgorithm, InterpolationSamples, LinearApproximation};
    struct Algorithm(Rc<Cell<usize>>);
    impl InterpolationAlgorithm for Algorithm {
        fn algorithm_type(&self) -> &'static str {
            "Test"
        }
        fn get_required_data_points(&self, _: f64, _: f64) -> f64 {
            self.0.set(self.0.get() + 1);
            2.
        }
        fn interpolate_order_zero(&self, s: &InterpolationSamples<'_>) -> Option<Vec<f64>> {
            LinearApproximation.interpolate_order_zero(s)
        }
    }
    let calls = Rc::new(Cell::new(0));
    let algorithm = Interpolation::Custom(Rc::new(Algorithm(calls.clone())));
    let p = SampledProperty::<f64>::new(None);
    p.set_interpolation_options(Some(algorithm), None);
    p.add_samples(&[date(0.), date(10.)], &[0., 10.], None).unwrap();
    for i in 1..10 {
        assert_eq!(p.get_value(Some(&date(i as f64)), None).unwrap(), Some(i as f64));
    }
    assert_eq!(calls.get(), 1);
    p.set_forward_extrapolation_type(ExtrapolationType::Hold);
    p.get_value(Some(&date(2.)), None).unwrap();
    assert_eq!(calls.get(), 1);
    p.add_sample(date(10.), &20., None).unwrap();
    p.get_value(Some(&date(2.)), None).unwrap();
    assert_eq!(calls.get(), 2);
    p.set_interpolation_options(None, Some(2.));
    p.get_value(Some(&date(2.)), None).unwrap();
    assert_eq!(calls.get(), 3);
}
#[test]
fn event_dispatch_defers_edits_and_duplicate_listeners_share_identity() {
    let event = DefinitionChanged::new();
    let trace = Rc::new(RefCell::new(vec![]));
    let edited = Rc::new(Cell::new(false));
    let b = Rc::new(RefCell::new(None::<Subscription>));
    let c = Rc::new(RefCell::new(None::<Subscription>));
    let t = trace.clone();
    let c_listener: PropertyListener = Rc::new(move |_| t.borrow_mut().push("c"));
    let t = trace.clone();
    let changed = edited.clone();
    let b_remove = b.clone();
    let c_add = c.clone();
    let _a = event.add_listener(move |e| {
        t.borrow_mut().push("a");
        if !changed.replace(true) {
            b_remove.borrow_mut().as_mut().unwrap().remove();
            *c_add.borrow_mut() = Some(e.subscribe(c_listener.clone()));
        }
    });
    let t = trace.clone();
    *b.borrow_mut() = Some(event.add_listener(move |_| t.borrow_mut().push("b")));
    event.raise();
    event.raise();
    assert_eq!(*trace.borrow(), vec!["a", "b", "a", "c"]);
    assert_eq!(event.number_of_listeners(), 2);
    let listener: PropertyListener = Rc::new(|_| {});
    let mut first = event.subscribe(listener.clone());
    let second = event.subscribe(listener);
    assert_eq!(event.number_of_listeners(), 3);
    assert!(first.remove());
    drop(second);
    assert_eq!(event.number_of_listeners(), 2);
}

#[test]
fn subscriptions_renewed_during_dispatch_keep_their_new_identity() {
    for renewals in [1, 2] {
        for keep in [false, true] {
            let event = DefinitionChanged::new();
            let registration = Rc::new(RefCell::new(None::<Subscription>));
            let callback = Rc::new(RefCell::new(None::<std::rc::Weak<dyn Fn(&DefinitionChanged)>>));
            let calls = Rc::new(Cell::new(0));
            let (current, weak_callback, seen) = (registration.clone(), callback.clone(), calls.clone());
            let listener: PropertyListener = Rc::new(move |event| {
                seen.set(seen.get() + 1);
                for _ in 0..renewals {
                    let old = current.borrow_mut().take();
                    drop(old);
                    let next = event.subscribe(weak_callback.borrow().as_ref().unwrap().upgrade().unwrap());
                    *current.borrow_mut() = Some(next);
                }
                if !keep {
                    let last = current.borrow_mut().take();
                    drop(last);
                }
            });
            *callback.borrow_mut() = Some(Rc::downgrade(&listener));
            *registration.borrow_mut() = Some(event.subscribe(listener.clone()));
            let stale = event.subscribe(listener);
            event.raise();
            assert_eq!(calls.get(), 1);
            assert_eq!(event.number_of_listeners(), usize::from(keep));
            drop(stale);
            for _ in 0..2 {
                let previous = calls.get();
                event.raise();
                assert_eq!(calls.get(), previous + usize::from(keep));
                assert_eq!(event.number_of_listeners(), usize::from(keep));
            }
            drop(registration.borrow_mut().take());
            assert_eq!(event.number_of_listeners(), 0);
            assert!(callback.borrow().as_ref().unwrap().upgrade().is_none());
        }
    }
}

#[test]
fn duplicate_subscriptions_during_dispatch_count_one_registration() {
    let event = DefinitionChanged::new();
    let duplicates = Rc::new(RefCell::new(Vec::new()));
    let callback = Rc::new(RefCell::new(None::<std::rc::Weak<dyn Fn(&DefinitionChanged)>>));
    let (handles, weak_callback) = (duplicates.clone(), callback.clone());
    let listener: PropertyListener = Rc::new(move |event| {
        handles.borrow_mut().push(event.subscribe(weak_callback.borrow().as_ref().unwrap().upgrade().unwrap()));
        assert_eq!(event.number_of_listeners(), 1);
    });
    *callback.borrow_mut() = Some(Rc::downgrade(&listener));
    let original = event.subscribe(listener);
    event.raise();
    event.raise();
    assert_eq!(event.number_of_listeners(), 1);
    drop(original);
    duplicates.borrow_mut().clear();
    assert_eq!(event.number_of_listeners(), 0);
}

#[test]
fn property_array_keeps_forwarding_when_observers_reset_the_same_children() {
    let child = Rc::new(ConstantProperty::new(Some(1.)));
    let parent = Rc::new(PropertyArray::new(Some(vec![child.clone(), child.clone()])));
    let weak = Rc::downgrade(&parent);
    let (changing, calls) = (Rc::new(Cell::new(false)), Rc::new(Cell::new(0)));
    let child_to_keep = child.clone();
    let seen = calls.clone();
    let _guard = parent.definition_changed().add_listener(move |_| {
        if !changing.replace(true) {
            seen.set(seen.get() + 1);
            weak.upgrade().unwrap().set_value(Some(vec![child_to_keep.clone(), child_to_keep.clone()]));
            changing.set(false);
        }
    });
    for value in [2., 3., 4.] {
        child.set_value(Some(value));
        assert_eq!(calls.get(), (value - 1.) as usize);
        assert_eq!(child.definition_changed().number_of_listeners(), 1);
        assert_eq!(parent.get_value(Some(&date(0.)), None).unwrap(), Some(vec![value, value]));
    }
    drop(parent);
    assert_eq!(child.definition_changed().number_of_listeners(), 0);
}

#[test]
fn array_evaluation_uses_one_definition_when_a_child_replaces_it() {
    for replacement in
        [None, Some(vec![]), Some(vec![99.]), Some(vec![99., 100., 101.]), Some(vec![99., 100., 101., 102.])]
    {
        let link = Rc::new(RefCell::new(None::<std::rc::Weak<PropertyArray<f64>>>));
        let weak = link.clone();
        let next = replacement.clone();
        let child = Rc::new(CallbackProperty::new(
            Rc::new(move |_, _| {
                let value = next
                    .as_ref()
                    .map(|v| v.iter().map(|v| Rc::new(ConstantProperty::new(Some(*v))) as PropertyRef<f64>).collect());
                weak.borrow().as_ref().unwrap().upgrade().unwrap().set_value(value);
                Ok(Some(1.))
            }),
            false,
        ));
        let parent = Rc::new(PropertyArray::new(Some(vec![
            child.clone(),
            Rc::new(ConstantProperty::new(Some(2.))),
            Rc::new(ConstantProperty::new(Some(3.))),
        ])));
        *link.borrow_mut() = Some(Rc::downgrade(&parent));
        let mut result = vec![7., 8., 9., 10.];
        assert_eq!(parent.get_value(Some(&date(0.)), Some(&mut result)).unwrap(), Some(vec![1., 2., 3.]));
        assert_eq!(result, vec![1., 2., 3.]);
        assert_eq!(parent.get_value(Some(&date(0.)), None).unwrap(), replacement);
        assert_eq!(child.definition_changed().number_of_listeners(), 0);
        assert!(parent.is_constant());
    }
}
