//! Approximate height bounds and concrete terrain providers against CesiumJS.
mod common;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::future::{ready, Future};
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

fn complete<F: Future + Unpin>(mut f: F) -> F::Output {
    match Pin::new(&mut f).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("ready fixture request was pending"),
    }
}
fn ellipsoid(name: &Value) -> Ellipsoid {
    match name.as_str() {
        Some("MOON") => Ellipsoid::MOON,
        Some("UNIT_SPHERE") => Ellipsoid::UNIT_SPHERE,
        _ => Ellipsoid::WGS84,
    }
}
fn scheme(o: &Value) -> Option<Box<dyn TilingScheme>> {
    match o["scheme"].as_str() {
        Some("mercator") => Some(Box::new(WebMercatorTilingScheme::new(
            ellipsoid(&o["schemeEllipsoid"]),
            opt(&o["x"]).unwrap_or(1.0) as i32,
            opt(&o["y"]).unwrap_or(1.0) as i32,
            None,
        ))),
        Some(_) => Some(Box::new(GeographicTilingScheme::new(
            ellipsoid(&o["schemeEllipsoid"]),
            Rectangle::MAX_VALUE,
            opt(&o["x"]).unwrap_or(2.0) as i32,
            opt(&o["y"]).unwrap_or(1.0) as i32,
        ))),
        None => None,
    }
}
fn provider_meta<P: TerrainProvider>(p: &P, credit: Value) -> Value {
    json!([
        credit,
        p.has_water_mask(),
        p.has_vertex_normals(),
        p.availability().map(|_| true),
        p.tiling_scheme().ellipsoid().radii.enc(),
        p.tiling_scheme().get_number_of_x_tiles_at_level(0),
        p.tiling_scheme().get_number_of_y_tiles_at_level(0),
        Rc::ptr_eq(&p.error_event(), &p.error_event()),
        ([0, 1, 5, 30, 31, 32, 33, 64].map(|l| p.get_level_maximum_geometric_error(l)).to_vec().enc()),
        p.get_tile_data_available(1, 2, 3),
        p.load_tile_data_availability(1, 2, 3).map(|_| true)
    ])
}
fn data_meta(d: &HeightmapTerrainData) -> Value {
    let buffer = d.buffer().unwrap();
    json!([
        typed_array_name(buffer),
        buffer.to_f64_vec().enc(),
        d.width(),
        d.height(),
        d.child_tile_mask(),
        Value::Null,
        d.water_mask(),
        d.was_created_by_upsampling()
    ])
}

#[test]
fn approximate_terrain_heights() {
    let mut base = ApproximateTerrainHeights::new();
    base.initialize().unwrap();
    run(
        "core",
        "ApproximateTerrainHeights",
        |_| TIGHT,
        |f, a| match f {
            "initialize" => {
                let mut heights = ApproximateTerrainHeights::new();
                let initial = heights.initialized();
                heights.initialize().ok()?;
                // Cached initialization ignores an invalid second table.
                let reused = heights.initialize_from_json("invalid JSON").is_ok();
                ok(json!([initial, reused, heights.initialized()]))
            }
            "query" => {
                let mut heights = if u(&a[2]) == 0 { base.clone() } else { ApproximateTerrainHeights::new() };
                if u(&a[2]) == 1 {
                    heights.initialize_from_json("{}").ok()?;
                }
                if u(&a[2]) == 2 {
                    heights.initialize_from_json("{\"0-0-0\":[-250000,18000],\"6-64-28\":[-300,12345]}").ok()?;
                }
                heights
                    .get_minimum_maximum_heights(&Rectangle::from_degrees(0.1, 10.1, 0.2, 10.2), &Ellipsoid::WGS84)?;
                let r = rect(&a[0]);
                let e = ellipsoid(&a[1]);
                let (min, max) = heights.get_minimum_maximum_heights(&r, &e)?;
                ok(
                    json!([{"minimumTerrainHeight":min,"maximumTerrainHeight":max},heights.get_bounding_sphere(&r,&e)?.enc()]),
                )
            }
            _ => None,
        },
    );
}

#[test]
fn ellipsoid_terrain_provider() {
    run(
        "core",
        "EllipsoidTerrainProvider",
        |_| TIGHT,
        |f, a| match f {
            "provider" => {
                let p =
                    EllipsoidTerrainProvider::new(scheme(&a[0]), Some(ellipsoid(&a[0]["ellipsoid"])), Some(n(&a[1])));
                let first = complete(p.request_tile_geometry(0, 0, 0, None)?).ok()?;
                let second = complete(p.request_tile_geometry(10, 20, 31, None)?).ok()?;
                let shared = match (first.buffer()?, second.buffer()?) {
                    (TypedArray::Uint8(a), TypedArray::Uint8(b)) => std::ptr::eq(a.as_ptr(), b.as_ptr()),
                    _ => false,
                };
                ok(json!([provider_meta(&p, Value::Null), data_meta(&first), data_meta(&second), shared]))
            }
            _ => None,
        },
    );
}

#[test]
fn custom_heightmap_terrain_provider() {
    run(
        "core",
        "CustomHeightmapTerrainProvider",
        |_| TIGHT,
        |f, a| match f {
            "provider" => {
                let o = &a[0];
                let calls = Rc::new(RefCell::new(Vec::<[u32; 3]>::new()));
                let captured = calls.clone();
                let values = nums(&o["values"]);
                let mode = s(&o["mode"]).to_owned();
                let plain = s(&o["type"]) == "Array";
                let datatype = match s(&o["type"]) {
                    "Int8Array" => ComponentDatatype::Byte,
                    "Uint8Array" => ComponentDatatype::UnsignedByte,
                    "Int16Array" => ComponentDatatype::Short,
                    "Uint16Array" => ComponentDatatype::UnsignedShort,
                    "Int32Array" => ComponentDatatype::Int,
                    "Uint32Array" => ComponentDatatype::UnsignedInt,
                    "Float32Array" => ComponentDatatype::Float,
                    _ => ComponentDatatype::Double,
                };
                let callback = move |x: u32, y: u32, level: u32| {
                    captured.borrow_mut().push([x, y, level]);
                    if mode == "defer" && x == 1 {
                        return None;
                    }
                    Some(ready(if mode == "reject" {
                        Err(RuntimeError::new("terrain callback failed"))
                    } else {
                        let values = values.iter().map(|&v| v + x as f64 - y as f64 + level as f64).collect::<Vec<_>>();
                        Ok(if plain {
                            HeightmapSamples::Numbers(values)
                        } else {
                            HeightmapSamples::Typed(TypedArray::from_values(datatype, &values))
                        })
                    }))
                };
                let mut options = CustomHeightmapTerrainProviderOptions::<String>::new(
                    u(&o["width"]) as usize,
                    u(&o["height"]) as usize,
                );
                options.tiling_scheme = scheme(o);
                options.ellipsoid = Some(ellipsoid(&o["ellipsoid"]));
                options.quality = Some(n(&o["quality"]));
                options.credit = o["credit"].as_str().map(str::to_owned);
                let p = CustomHeightmapTerrainProvider::new(callback, options)?;
                let mut results = Vec::new();
                for [x, y, l] in [[0, 0, 0], [1, 2, 3], [3, 4, 31]] {
                    results.push(match p.request_tile_geometry(x, y, l, None) {
                        None => Value::Null,
                        Some(f) => match complete(f) {
                            Ok(d) => data_meta(&d),
                            Err(e) => json!(["error", e.message]),
                        },
                    });
                }
                let calls = calls.borrow().clone();
                ok(json!([provider_meta(&p, json!(p.credit())), p.width(), p.height(), calls, results]))
            }
            _ => None,
        },
    );
}

#[test]
fn provider_callback_future_can_suspend_and_initialize_is_guarded() {
    struct PendingOnce {
        first: bool,
    }
    impl Future for PendingOnce {
        type Output = Result<HeightmapSamples, RuntimeError>;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.first {
                self.first = false;
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(Ok(vec![1.0, 2.0, 3.0, 4.0].into()))
            }
        }
    }
    let mut options = CustomHeightmapTerrainProviderOptions::<u32>::new(2, 2);
    options.credit = Some(42);
    let p = CustomHeightmapTerrainProvider::new(|_, _, _| Some(PendingOnce { first: true }), options).unwrap();
    let mut future = p.request_tile_geometry(1, 2, 3, None).unwrap();
    let mut context = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut context).is_pending());
    let data = match future.as_mut().poll(&mut context) {
        Poll::Ready(Ok(data)) => data,
        _ => panic!("callback did not complete"),
    };
    assert_eq!(data.buffer().unwrap(), &TypedArray::Float64(vec![1.0, 2.0, 3.0, 4.0]));
    assert_eq!(p.credit(), Some(&42));
    let mut heights = ApproximateTerrainHeights::new();
    assert!(heights.get_minimum_maximum_heights(&Rectangle::MAX_VALUE, &Ellipsoid::WGS84).is_none());
    assert!(heights.initialize_from_json("invalid").is_err());
    assert!(!heights.initialized());
    heights.initialize().unwrap();
    assert!(heights.get_bounding_sphere(&Rectangle::MAX_VALUE, &Ellipsoid::WGS84).is_some());
    assert!(std::ptr::eq(ApproximateTerrainHeights::bundled(), ApproximateTerrainHeights::bundled()));
}
