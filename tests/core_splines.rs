mod common;

use cesium_rs::core::spline::{find_time_interval, SplinePoint};
use cesium_rs::core::*;
use common::*;
use serde_json::Value;

fn sample<S: Spline>(spline: &S, points: Value, args: &[Value]) -> Value
where
    S::Value: Enc,
{
    let queries = nums(&args[2]);
    let outside = nums(&args[3]);
    Value::Array(vec![
        spline.times().to_vec().enc(),
        points,
        queries.iter().map(|&t| spline.evaluate(t)).collect::<Vec<_>>().enc(),
        outside.iter().map(|&t| spline.wrap_time(t)).collect::<Vec<_>>().enc(),
        outside.iter().map(|&t| spline.clamp_time(t)).collect::<Vec<_>>().enc(),
        queries.iter().map(|&t| spline.find_time_interval(t, None).map(|i| i as u32)).collect::<Vec<_>>().enc(),
    ])
}

fn constant_sample<T: SplinePoint + Enc>(value: T, queries: &Value) -> Value {
    let spline = ConstantSpline::new(value);
    let queries = nums(queries);
    Value::Array(vec![
        spline.value().enc(),
        queries.iter().map(|&t| spline.evaluate(t)).collect::<Vec<_>>().enc(),
        queries.iter().map(|&t| spline.wrap_time(t)).collect::<Vec<_>>().enc(),
        queries.iter().map(|&t| spline.clamp_time(t)).collect::<Vec<_>>().enc(),
    ])
}

#[test]
fn spline_time_operations() {
    run(
        "core",
        "Spline",
        |_| TIGHT,
        |f, a| {
            let times = nums(&a[0]);
            let spline = LinearSpline::new(times.clone(), times.clone()).unwrap();
            match f {
                "findTimeInterval" => {
                    ok(spline.find_time_interval(n(&a[1]), Some(u(&a[2]) as usize)).map(|i| i as u32))
                }
                "wrapTime" => ok(spline.wrap_time(n(&a[1]))),
                "clampTime" => ok(spline.clamp_time(n(&a[1]))),
                _ => None,
            }
        },
    );
}

#[test]
fn constant_spline() {
    run(
        "core",
        "ConstantSpline",
        |_| TIGHT,
        |f, a| match f {
            "scalar" => ok(constant_sample(n(&a[0]), &a[1])),
            "cartesian" => ok(constant_sample(c3(&a[0]), &a[1])),
            "quaternion" => ok(constant_sample(q(&a[0]), &a[1])),
            _ => None,
        },
    );
}

#[test]
fn linear_spline() {
    run(
        "core",
        "LinearSpline",
        |_| TIGHT,
        |f, a| match f {
            "scalar" => {
                let spline = LinearSpline::new(nums(&a[0]), nums(&a[1])).unwrap();
                ok(sample(&spline, spline.points().to_vec().enc(), a))
            }
            "cartesian" => {
                let spline = LinearSpline::new(nums(&a[0]), list(&a[1], c3)).unwrap();
                ok(sample(&spline, spline.points().to_vec().enc(), a))
            }
            _ => None,
        },
    );
}

#[test]
fn stepped_spline() {
    run(
        "core",
        "SteppedSpline",
        |_| TIGHT,
        |f, a| match f {
            "scalar" => {
                let spline = SteppedSpline::new(nums(&a[0]), nums(&a[1])).unwrap();
                ok(sample(&spline, spline.points().to_vec().enc(), a))
            }
            "cartesian" => {
                let spline = SteppedSpline::new(nums(&a[0]), list(&a[1], c3)).unwrap();
                ok(sample(&spline, spline.points().to_vec().enc(), a))
            }
            "quaternion" => {
                let spline = SteppedSpline::new(nums(&a[0]), list(&a[1], q)).unwrap();
                ok(sample(&spline, spline.points().to_vec().enc(), a))
            }
            _ => None,
        },
    );
}

#[test]
fn quaternion_spline() {
    run(
        "core",
        "QuaternionSpline",
        |_| TIGHT,
        |f, a| match f {
            "quaternion" => {
                let spline = QuaternionSpline::new(nums(&a[0]), list(&a[1], q)).unwrap();
                ok(sample(&spline, spline.points().to_vec().enc(), a))
            }
            _ => None,
        },
    );
}

#[test]
fn morph_weight_spline() {
    run(
        "core",
        "MorphWeightSpline",
        |_| TIGHT,
        |f, a| match f {
            "weights" => {
                let spline = MorphWeightSpline::new(nums(&a[0]), nums(&a[1])).unwrap();
                ok(sample(&spline, spline.weights().to_vec().enc(), a))
            }
            _ => None,
        },
    );
}

#[test]
fn invalid_domains_and_times_are_rejected() {
    assert!(LinearSpline::<f64>::new(vec![], vec![]).is_none());
    assert!(LinearSpline::new(vec![0.0], vec![0.0]).is_none());
    assert!(LinearSpline::new(vec![0.0, 1.0], vec![0.0]).is_none());
    assert!(SteppedSpline::new(vec![1.0, 0.0], vec![0.0, 1.0]).is_none());
    assert!(SteppedSpline::new(vec![0.0, 0.0], vec![0.0, 1.0]).is_none());
    assert!(QuaternionSpline::new(vec![0.0, f64::NAN], vec![Quaternion::IDENTITY; 2]).is_none());
    assert!(QuaternionSpline::new(vec![0.0, f64::INFINITY], vec![Quaternion::IDENTITY; 2]).is_none());
    assert!(MorphWeightSpline::new(vec![], vec![]).is_none());
    assert!(MorphWeightSpline::new(vec![0.0, 1.0], vec![0.0, 1.0, 2.0]).is_none());
    assert!(MorphWeightSpline::new(vec![0.0, 1.0], vec![0.0, 1.0]).is_none());

    let spline = LinearSpline::new(vec![0.0, 1.0], vec![0.0, 1.0]).unwrap();
    for time in [-1.0, 2.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(spline.evaluate(time).is_none());
    }
    assert_eq!(spline.find_time_interval(0.5, Some(usize::MAX)), None);
    assert_eq!(find_time_interval(&[], 0.0, None), None);
    assert_eq!(find_time_interval(&[0.0], 0.0, None), None);
}

#[test]
fn cesium_endpoint_behavior_and_owned_values() {
    let mut times = vec![1.0, 3.0, 5.0];
    let mut points = vec![10.0, 20.0, 30.0];
    let stepped = SteppedSpline::new(times.clone(), points.clone()).unwrap();
    times[0] = 100.0;
    points[0] = 100.0;
    assert_eq!(stepped.times(), &[1.0, 3.0, 5.0]);
    assert_eq!(stepped.evaluate(1.0), Some(10.0));
    assert_eq!(stepped.evaluate(3.0), Some(20.0));
    assert_eq!(stepped.evaluate(5.0), Some(20.0));
    assert_eq!(stepped.wrap_time(-3.0), 5.0);
    assert_eq!(stepped.wrap_time(9.0), 1.0);
    assert_eq!(stepped.clamp_time(9.0), 5.0);

    let constant = ConstantSpline::new(Cartesian3::ONE);
    let mut result = constant.evaluate(0.0).unwrap();
    result.x = 12.0;
    assert_eq!(constant.value(), Cartesian3::ONE);
    assert_eq!(constant.find_time_interval(0.0, None), None);
    assert_eq!(result.x, 12.0);
    let morph = MorphWeightSpline::new(vec![1.0, 2.0], vec![0.0, 1.0, 1.0, 0.0]).unwrap();
    assert_eq!(morph.target_count(), 2);
    assert_eq!(morph.evaluate(1.5), Some(vec![0.5, 0.5]));
}
