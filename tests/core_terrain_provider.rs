//! Terrain grid topology and quadtree availability against CesiumJS.
mod common;
use cesium_rs::core::terrain_provider as tp;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};
use std::sync::Arc;

fn typed(a: &TypedArray) -> Value {
    json!([typed_array_name(a), a.to_f64_vec().enc()])
}
fn topology(a: &TerrainGridIndices) -> Value {
    json!({"indices":typed(&a.indices),"west":a.edges.west_indices_south_to_north,"south":a.edges.south_indices_east_to_west,"east":a.edges.east_indices_north_to_south,"north":a.edges.north_indices_west_to_east,"count":a.index_count_without_skirts})
}
fn digest(a: &TypedArray) -> Value {
    let v = a.to_f64_vec();
    json!([
        typed_array_name(a),
        v.len(),
        v.iter().sum::<f64>(),
        v.iter().copied().fold(0.0, f64::max),
        v[..24.min(v.len())],
        v[v.len().saturating_sub(24)..]
    ])
}
fn edges(v: &Value) -> TerrainEdgeIndices {
    let values = |i| nums(&v[i]).into_iter().map(|n| n as u32).collect();
    TerrainEdgeIndices {
        west_indices_south_to_north: values(0),
        south_indices_east_to_west: values(1),
        east_indices_north_to_south: values(2),
        north_indices_west_to_east: values(3),
    }
}

#[test]
fn terrain_provider() {
    run(
        "core",
        "TerrainProvider",
        |f| if f == "geometricError" { TIGHT } else { EXACT },
        |f, a| match f {
            "grid" => ok(typed(tp::get_regular_grid_indices(u(&a[0]) as usize, u(&a[1]) as usize)?.as_ref())),
            "edges" => ok(topology(
                tp::get_regular_grid_indices_and_edge_indices(u(&a[0]) as usize, u(&a[1]) as usize)?.as_ref(),
            )),
            "skirts" => ok(topology(
                tp::get_regular_grid_and_skirt_indices_and_edge_indices(u(&a[0]) as usize, u(&a[1]) as usize)?.as_ref(),
            )),
            "cache" => {
                let (w, h) = (u(&a[0]) as usize, u(&a[1]) as usize);
                let grid = tp::get_regular_grid_indices(w, h)?;
                let edge = tp::get_regular_grid_indices_and_edge_indices(w, h)?;
                let skirt = tp::get_regular_grid_and_skirt_indices_and_edge_indices(w, h)?;
                ok(json!([
                    Arc::ptr_eq(&grid, &tp::get_regular_grid_indices(w, h)?),
                    Arc::ptr_eq(&edge, &tp::get_regular_grid_indices_and_edge_indices(w, h)?),
                    Arc::ptr_eq(&skirt, &tp::get_regular_grid_and_skirt_indices_and_edge_indices(w, h)?),
                    Arc::ptr_eq(&edge.indices, &grid)
                ]))
            }
            "largeGrid" => {
                let (w, h) = (u(&a[0]) as usize, u(&a[1]) as usize);
                ok(json!([
                    digest(tp::get_regular_grid_indices(w, h)?.as_ref()),
                    digest(&tp::get_regular_grid_and_skirt_indices_and_edge_indices(w, h)?.indices)
                ]))
            }
            "addSkirts" => {
                let edges = edges(&a[0]);
                let caps = a[2].as_bool().unwrap();
                let count = tp::get_skirt_vertex_count(&edges)?;
                let length = if caps {
                    tp::get_skirt_index_count_with_filled_corners(count)?
                } else {
                    tp::get_skirt_index_count(count)?
                };
                let offset = u(&a[4]) as usize;
                let datatype = match s(&a[3]) {
                    "Uint8Array" => ComponentDatatype::UnsignedByte,
                    "Uint16Array" => ComponentDatatype::UnsignedShort,
                    _ => ComponentDatatype::UnsignedInt,
                };
                let mut out = TypedArray::from_values(datatype, &vec![7.0; offset + length + 4]);
                if caps {
                    tp::add_skirt_indices_with_filled_corners(&edges, u(&a[1]), &mut out, offset)?;
                } else {
                    tp::add_skirt_indices(&edges, u(&a[1]), &mut out, offset)?;
                }
                ok(json!([
                    count,
                    tp::get_skirt_index_count(count)?,
                    tp::get_skirt_index_count_with_filled_corners(count)?,
                    typed(&out)
                ]))
            }
            "geometricError" => {
                let e = match s(&a[0]) {
                    "MOON" => Ellipsoid::MOON,
                    "UNIT_SPHERE" => Ellipsoid::UNIT_SPHERE,
                    _ => Ellipsoid::WGS84,
                };
                ok(tp::get_estimated_level_zero_geometric_error_for_a_heightmap(&e, n(&a[1]), n(&a[2]), opt(&a[3])))
            }
            _ => None,
        },
    );
}

fn availability_queries(availability: &TileAvailability, points: &Value, rectangles: &Value, tiles: &Value) -> Value {
    let ps: Vec<_> =
        points.as_array().unwrap().iter().map(|p| availability.compute_maximum_level_at_position(&carto(p))).collect();
    let rs: Vec<_> = rectangles
        .as_array()
        .unwrap()
        .iter()
        .map(|r| availability.compute_best_available_level_over_rectangle(&rect(r)))
        .collect();
    let ts: Vec<_> = tiles
        .as_array()
        .unwrap()
        .iter()
        .map(|t| {
            json!([
                availability.is_tile_available(u(&t[0]), u(&t[1]), u(&t[2])),
                availability.compute_child_mask_for_tile(u(&t[0]), u(&t[1]), u(&t[2]))
            ])
        })
        .collect();
    json!([ps, rs, ts])
}

#[test]
fn tile_availability() {
    run(
        "core",
        "TileAvailability",
        |_| EXACT,
        |f, a| match f {
            "availability" => {
                let o = &a[0];
                let scheme: Box<dyn TilingScheme> = if s(&o["kind"]) == "mercator" {
                    Box::new(WebMercatorTilingScheme::new(
                        Ellipsoid::WGS84,
                        opt(&o["x"]).unwrap_or(1.0) as i32,
                        opt(&o["y"]).unwrap_or(1.0) as i32,
                        None,
                    ))
                } else {
                    Box::new(GeographicTilingScheme::new(
                        Ellipsoid::WGS84,
                        if o["rectangle"].is_null() { Rectangle::MAX_VALUE } else { rect(&o["rectangle"]) },
                        opt(&o["x"]).unwrap_or(2.0) as i32,
                        opt(&o["y"]).unwrap_or(1.0) as i32,
                    ))
                };
                let mut availability = TileAvailability::from_boxed(scheme, u(&a[1]));
                let mut result = vec![availability_queries(&availability, &a[3], &a[4], &a[5])];
                for r in a[2].as_array().unwrap() {
                    availability.add_available_tile_range(u(&r[0]), u(&r[1]), u(&r[2]), u(&r[3]), u(&r[4]))?;
                    result.push(availability_queries(&availability, &a[3], &a[4], &a[5]));
                }
                ok(result)
            }
            _ => None,
        },
    );
}

#[test]
fn skirt_failures_are_atomic_and_cached_data_is_immutable() {
    let a = tp::get_regular_grid_indices(3, 3).unwrap();
    let mut copy = a.clone();
    Arc::make_mut(&mut copy).set(0, 999.0);
    assert_ne!(a.get(0), copy.get(0));
    assert!(Arc::ptr_eq(&a, &tp::get_regular_grid_indices(3, 3).unwrap()));
    assert!(tp::get_regular_grid_indices(0, 4).is_none());
    assert!(tp::get_regular_grid_indices(65536, 65536).is_none());
    assert!(tp::get_regular_grid_indices(usize::MAX, 2).is_none());
    let edges = tp::get_regular_grid_indices_and_edge_indices(3, 3).unwrap().edges.clone();
    let mut out = TypedArray::Uint16(vec![7; 3]);
    let before = out.clone();
    assert!(tp::add_skirt_indices(&edges, 9, &mut out, 0).is_none());
    assert_eq!(out, before);
    assert!(tp::add_skirt_indices(&edges, u32::MAX, &mut out, 0).is_none());
    assert_eq!(out, before);
    let empty = TerrainEdgeIndices::default();
    assert!(tp::add_skirt_indices_with_filled_corners(&empty, 9, &mut out, 0).is_none());
    assert_eq!(out, before);
}

#[test]
fn provider_contract_supports_throttling_futures_and_shared_error_events() {
    use std::cell::RefCell;
    use std::future::{ready, Future, Ready};
    use std::pin::Pin;
    use std::rc::Rc;
    use std::task::{Context, Poll, Waker};
    struct Provider {
        errors: Rc<RefCell<Event<&'static str>>>,
        scheme: GeographicTilingScheme,
        availability: TileAvailability,
        credit: &'static str,
    }
    impl TerrainProvider for Provider {
        type Error = &'static str;
        type Credit = &'static str;
        type TerrainData = [u32; 3];
        type Request = bool;
        type GeometryFuture<'a>
            = Ready<Result<Self::TerrainData, Self::Error>>
        where
            Self: 'a;
        type AvailabilityFuture<'a>
            = Ready<Result<(), Self::Error>>
        where
            Self: 'a;
        fn error_event(&self) -> Rc<RefCell<Event<Self::Error>>> {
            self.errors.clone()
        }
        fn credit(&self) -> Option<&Self::Credit> {
            Some(&self.credit)
        }
        fn tiling_scheme(&self) -> &dyn TilingScheme {
            &self.scheme
        }
        fn has_water_mask(&self) -> bool {
            false
        }
        fn has_vertex_normals(&self) -> bool {
            true
        }
        fn availability(&self) -> Option<&TileAvailability> {
            Some(&self.availability)
        }
        fn request_tile_geometry(
            &self,
            x: u32,
            y: u32,
            level: u32,
            request: Option<bool>,
        ) -> Option<Self::GeometryFuture<'_>> {
            if request == Some(true) {
                None
            } else {
                Some(ready(if x == u32::MAX { Err("missing tile") } else { Ok([x, y, level]) }))
            }
        }
        fn get_level_maximum_geometric_error(&self, level: u32) -> f64 {
            1000.0 / 2f64.powi(level as i32)
        }
        fn get_tile_data_available(&self, x: u32, y: u32, level: u32) -> Option<bool> {
            (level <= self.availability.maximum_level()).then(|| self.availability.is_tile_available(level, x, y))
        }
        fn load_tile_data_availability(&self, x: u32, _y: u32, _level: u32) -> Option<Self::AvailabilityFuture<'_>> {
            (x == 0).then(|| ready(Ok(())))
        }
    }
    fn complete<F: Future + Unpin>(mut future: F) -> F::Output {
        match Pin::new(&mut future).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("ready test future was pending"),
        }
    }
    let mut availability = TileAvailability::new(GeographicTilingScheme::default(), 4);
    availability.add_available_tile_range(0, 0, 0, 1, 0).unwrap();
    let provider = Provider {
        errors: Rc::new(RefCell::new(Event::new())),
        scheme: GeographicTilingScheme::default(),
        availability,
        credit: "test credit",
    };
    assert!(provider.request_tile_geometry(0, 0, 0, Some(true)).is_none());
    assert_eq!(complete(provider.request_tile_geometry(1, 2, 3, None).unwrap()), Ok([1, 2, 3]));
    assert_eq!(complete(provider.request_tile_geometry(u32::MAX, 0, 0, None).unwrap()), Err("missing tile"));
    assert_eq!(complete(provider.load_tile_data_availability(0, 0, 0).unwrap()), Ok(()));
    assert!(provider.load_tile_data_availability(1, 0, 0).is_none());
    assert_eq!(provider.get_tile_data_available(0, 0, 0), Some(true));
    assert_eq!(provider.get_tile_data_available(0, 0, 8), None);
    assert_eq!(provider.get_level_maximum_geometric_error(2), 250.0);
    assert_eq!(provider.credit(), Some(&"test credit"));
    assert!(!provider.has_water_mask() && provider.has_vertex_normals());
    assert_eq!(provider.tiling_scheme().get_number_of_x_tiles_at_level(0), 2);
    assert!(provider.availability().is_some());
    let seen = Rc::new(RefCell::new(Vec::new()));
    let captured = seen.clone();
    provider.error_event().borrow_mut().add_event_listener(move |error| captured.borrow_mut().push(*error));
    provider.errors.borrow_mut().raise_event(&"provider error");
    assert_eq!(*seen.borrow(), vec!["provider error"]);
}
