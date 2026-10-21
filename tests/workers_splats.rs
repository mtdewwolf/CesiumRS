//! Gaussian splat workers against their real CesiumJS message handlers and
//! the `@cesium/wasm-splats` module they load.
mod common;
use cesium_rs::workers::*;
use common::*;
use serde_json::{json, Value};

fn f32s(v: &Value) -> Vec<f32> {
    nums(v).into_iter().map(|x| x as f32).collect()
}

#[test]
fn gaussian_splat_sorter_reference() {
    run(
        "workers",
        "gaussianSplatSorter",
        |_| EXACT,
        |f, a| {
            let result = gaussian_splat_sorter(&f32s(&a[0]), &f32s(&a[1]), u(&a[2]) as usize, f);
            Some(match result {
                Ok(Some(indices)) => Out::Ok(json!({"indices": indices, "transferCount": 0})),
                Ok(None) => Out::Ok(Value::Null),
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
}

#[test]
fn gaussian_splat_texture_generator_reference() {
    run(
        "workers",
        "gaussianSplatTextureGenerator",
        |_| EXACT,
        |_, a| {
            let colors = nums(&a[3]).into_iter().map(|x| x as u8).collect::<Vec<_>>();
            res(gaussian_splat_texture_generator(&f32s(&a[0]), &f32s(&a[1]), &f32s(&a[2]), &colors, u(&a[4]) as usize)
                .map(|t| {
                    let end = t.data.iter().rposition(|&v| v != 0).map_or(0, |i| i + 1);
                    json!({
                        "data": {"length": t.data.len(), "values": t.data[..end]},
                        "width": t.width,
                        "height": t.height,
                        "transferCount": 0,
                    })
                }))
        },
    );
}

#[test]
fn sort_is_stable_for_equal_depths_and_wraps_large_ranges() {
    let identity = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.];
    let order = radix_sort_gaussians_indexes(&[0., 0., 1., 0., 0., -1., 0., 0., 1., 0., 0., 0.], &identity, 4).unwrap();
    assert_eq!(order, [1, 3, 0, 2]);
    // Depths past ±2^31/4096 saturate and the offset then wraps, so the far splat
    // sorts first, exactly as @cesium/wasm-splats returns.
    let order = radix_sort_gaussians_indexes(&[0., 0., 1e9, 0., 0., -1e9, 0., 0., 0.], &identity, 3).unwrap();
    assert_eq!(order, [2, 0, 1]);
    assert!(generate_splat_texture(&[0.; 3], &[1.; 3], &[0.; 4], &[0; 3], 1).is_err());
}
