//! Picking reference cases executed from the pinned CesiumJS private helpers.
mod common;
use cesium_rs::{core::*, scene::*};
use common::*;
use serde_json::json;

fn reference_camera() -> Camera {
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    camera.frustum =
        CameraFrustum::Orthographic(OrthographicFrustum { width: 1., aspect_ratio: 1., near: 1., far: 1e9 });
    camera
}

#[test]
fn offscreen_camera_from_ray_matches_pinned_source() {
    run(
        "scene",
        "PickingRay",
        |_| TIGHT,
        |_, a| {
            let ray = Ray::new(c3(&a[0]), c3(&a[1]));
            let width = if a[2].is_null() { None } else { Some(n(&a[2])) };
            let mut camera = reference_camera();
            let volume = Picking::update_offscreen_camera_from_ray(&ray, width, &mut camera).unwrap();
            let CameraFrustum::Orthographic(frustum) = camera.frustum else { panic!("expected orthographic camera") };
            Some(Out::Ok(json!({
                "position": [num(camera.position.x), num(camera.position.y), num(camera.position.z)],
                "direction": [num(camera.direction.x), num(camera.direction.y), num(camera.direction.z)],
                "up": [num(camera.up.x), num(camera.up.y), num(camera.up.z)],
                "right": [num(camera.right.x), num(camera.right.y), num(camera.right.z)],
                "width": num(frustum.width),
                "planes": volume.planes.iter().map(|p| json!([
                    num(p.x), num(p.y), num(p.z), num(p.w)
                ])).collect::<Vec<_>>(),
            })))
        },
    );
}

#[test]
fn sample_height_ray_matches_pinned_source() {
    run(
        "scene",
        "PickHeightRay",
        |_| TIGHT,
        |_, a| {
            let ray = Picking::ray_for_sample_height(&Ellipsoid::WGS84, &carto(&a[0]));
            Some(Out::Ok(json!({
                "origin": [num(ray.origin.x), num(ray.origin.y), num(ray.origin.z)],
                "direction": [num(ray.direction.x), num(ray.direction.y), num(ray.direction.z)],
            })))
        },
    );
}

#[test]
fn offscreen_camera_rejects_invalid_rays_and_widths() {
    let mut camera = reference_camera();
    assert!(Picking::update_offscreen_camera_from_ray(
        &Ray::new(Cartesian3::ZERO, Cartesian3::ZERO),
        None,
        &mut camera
    )
    .is_err());
    for width in [0., -1., f64::NAN] {
        assert!(Picking::update_offscreen_camera_from_ray(
            &Ray::new(Cartesian3::ZERO, Cartesian3::UNIT_Z),
            Some(width),
            &mut camera
        )
        .is_err());
    }
}
