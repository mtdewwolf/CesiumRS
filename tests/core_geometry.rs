//! Ellipsoid, cartographic, rectangle, projection, bounding-volume, plane,
//! local-frame, and color ports against CesiumJS fixtures.

mod common;

use cesium_rs::core::color::ColorRandomOptions;
use cesium_rs::core::math as cm;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};

const E: Ellipsoid = Ellipsoid::WGS84;

#[test]
fn ellipsoid() {
    run("core", "Ellipsoid", |_| TIGHT, |f, a| {
        let p = |i: usize| c3(&a[i]);
        match f {
            "constants" => {
                let e = match s(&a[0]) {
                    "WGS84" => Ellipsoid::WGS84,
                    "MOON" => Ellipsoid::MOON,
                    "MARS" => Ellipsoid::MARS,
                    "UNIT_SPHERE" => Ellipsoid::UNIT_SPHERE,
                    _ => return None,
                };
                ok(vec![
                    e.radii.enc(),
                    e.radii_squared.enc(),
                    e.radii_to_the_fourth.enc(),
                    e.one_over_radii.enc(),
                    e.one_over_radii_squared.enc(),
                    e.minimum_radius.enc(),
                    e.maximum_radius.enc(),
                ])
            }
            "geodeticSurfaceNormalCartographic" => ok(E.geodetic_surface_normal_cartographic(&carto(&a[0]))),
            "geodeticSurfaceNormal" => ok(E.geodetic_surface_normal(p(0))),
            "cartographicToCartesian" => ok(E.cartographic_to_cartesian(&carto(&a[0]))),
            "cartesianToCartographic" => ok(E.cartesian_to_cartographic(p(0))),
            "cartesianToCartographicMoon" => ok(Ellipsoid::MOON.cartesian_to_cartographic(p(0))),
            "scaleToGeodeticSurface" => ok(E.scale_to_geodetic_surface(p(0))),
            "scaleToGeocentricSurface" => ok(E.scale_to_geocentric_surface(p(0))),
            "transformPositionToScaledSpace" => ok(E.transform_position_to_scaled_space(p(0))),
            "transformPositionFromScaledSpace" => ok(E.transform_position_from_scaled_space(p(0))),
            "getSurfaceNormalIntersectionWithZAxis" => ok(E.get_surface_normal_intersection_with_z_axis(p(0), n(&a[1]))),
            "getLocalCurvature" => ok(E.get_local_curvature(p(0))),
            "surfaceArea" => ok(E.surface_area(&rect(&a[0]))),
            _ => None,
        }
    });
}

#[test]
fn cartographic() {
    run("core", "Cartographic", |_| TIGHT, |f, a| match f {
        "fromDegrees" => ok(Cartographic::from_degrees(n(&a[0]), n(&a[1]), n(&a[2]))),
        "fromCartesian" => ok(Cartographic::from_cartesian(c3(&a[0]), &E)),
        "toCartesian" => ok(carto(&a[0]).to_cartesian(&E)),
        _ => None,
    });
}

#[test]
fn rectangle() {
    run("core", "Rectangle", |_| TIGHT, |f, a| {
        let r = |i: usize| rect(&a[i]);
        match f {
            "computeWidth" => ok(r(0).compute_width()),
            "computeHeight" => ok(r(0).compute_height()),
            "fromDegrees" => ok(Rectangle::from_degrees(n(&a[0]), n(&a[1]), n(&a[2]), n(&a[3]))),
            "fromCartographicArray" => ok(Rectangle::from_cartographic_array(&list(&a[0], carto))),
            "fromCartesianArray" => ok(Rectangle::from_cartesian_array(&list(&a[0], c3), &E)),
            "fromBoundingSphere" => ok(Rectangle::from_bounding_sphere(&bsphere(&a[0]), &E)),
            "southwest" => ok(r(0).southwest()),
            "center" => ok(r(0).center()),
            "intersection" => ok(r(0).intersection(&r(1))),
            "simpleIntersection" => ok(r(0).simple_intersection(&r(1))),
            "union" => ok(r(0).union(&r(1))),
            "expand" => ok(r(0).expand(&carto(&a[1]))),
            "contains" => ok(r(0).contains(&carto(&a[1]))),
            "subsample" => ok(r(0).subsample(&E, n(&a[1]))),
            "subsection" => ok(r(0).subsection(n(&a[1]), n(&a[2]), n(&a[3]), n(&a[4]))),
            _ => None,
        }
    });
}

#[test]
fn projections() {
    let geo = GeographicProjection::default();
    let merc = WebMercatorProjection::default();
    run("core", "Projections", |_| TIGHT, |f, a| match f {
        "GeographicProjection.project" => ok(geo.project(&carto(&a[0]))),
        "GeographicProjection.unproject" => ok(geo.unproject(c3(&a[0]))),
        "WebMercatorProjection.project" => ok(merc.project(&carto(&a[0]))),
        "WebMercatorProjection.unproject" => ok(merc.unproject(c3(&a[0]))),
        "WebMercatorProjection.geodeticLatitudeToMercatorAngle" => {
            ok(WebMercatorProjection::geodetic_latitude_to_mercator_angle(n(&a[0])))
        }
        "WebMercatorProjection.mercatorAngleToGeodeticLatitude" => {
            ok(WebMercatorProjection::mercator_angle_to_geodetic_latitude(n(&a[0])))
        }
        "WebMercatorProjection.MaximumLatitude" => ok(WebMercatorProjection::maximum_latitude()),
        _ => None,
    });
}

#[test]
fn bounding_rectangle() {
    let merc = WebMercatorProjection::default();
    run("core", "BoundingRectangle", |_| TIGHT, |f, a| match f {
        "fromPoints" => ok(BoundingRectangle::from_points(&list(&a[0], c2))),
        "fromRectangle" => ok(BoundingRectangle::from_rectangle(&rect(&a[0]), None)),
        "fromRectangleMercator" => ok(BoundingRectangle::from_rectangle(&rect(&a[0]), Some(&merc))),
        "union" => ok(brect(&a[0]).union(&brect(&a[1]))),
        "expand" => ok(brect(&a[0]).expand(c2(&a[1]))),
        "intersect" => ok(brect(&a[0]).intersect(&brect(&a[1]))),
        _ => None,
    });
}

#[test]
fn bounding_sphere() {
    run("core", "BoundingSphere", |_| TIGHT, |f, a| {
        let b = |i: usize| bsphere(&a[i]);
        match f {
            "fromPoints" => ok(BoundingSphere::from_points(&list(&a[0], c3))),
            "fromRectangle2D" => ok(BoundingSphere::from_rectangle_2d(&rect(&a[0]), None)),
            "fromRectangleWithHeights2D" => {
                ok(BoundingSphere::from_rectangle_with_heights_2d(&rect(&a[0]), None, n(&a[1]), n(&a[2])))
            }
            "fromRectangle3D" => ok(BoundingSphere::from_rectangle_3d(&rect(&a[0]), &E, n(&a[1]))),
            "fromVertices" => ok(BoundingSphere::from_vertices(&nums(&a[0]), c3(&a[1]), 3)),
            "fromEncodedCartesianVertices" => {
                ok(BoundingSphere::from_encoded_cartesian_vertices(&nums(&a[0]), &nums(&a[1])))
            }
            "fromCornerPoints" => ok(BoundingSphere::from_corner_points(c3(&a[0]), c3(&a[1]))),
            "fromBoundingSpheres" => ok(BoundingSphere::from_bounding_spheres(&list(&a[0], bsphere))),
            "fromTransformation" => ok(BoundingSphere::from_transformation(&m4(&a[0]))),
            "union" => ok(b(0).union(&b(1))),
            "expand" => ok(b(0).expand(c3(&a[1]))),
            "intersectPlane" => ok(b(0).intersect_plane(&plane(&a[1]))),
            "transform" => ok(b(0).transform(&m4(&a[1]))),
            "distanceSquaredTo" => ok(b(0).distance_squared_to(c3(&a[1]))),
            "transformWithoutScale" => ok(b(0).transform_without_scale(&m4(&a[1]))),
            "computePlaneDistances" => ok(b(0).compute_plane_distances(c3(&a[1]), c3(&a[2]))),
            "projectTo2D" => ok(b(0).project_to_2d(None)),
            "volume" => ok(b(0).volume()),
            _ => None,
        }
    });
}

#[test]
fn plane_() {
    run("core", "Plane", |_| TIGHT, |f, a| match f {
        "fromPointNormal" => ok(Plane::from_point_normal(c3(&a[0]), c3(&a[1]))),
        "fromCartesian4" => ok(Plane::from_cartesian4(c4(&a[0]))),
        "getPointDistance" => ok(plane(&a[0]).get_point_distance(c3(&a[1]))),
        "projectPointOntoPlane" => ok(plane(&a[0]).project_point_onto_plane(c3(&a[1]))),
        "transform" => res(plane(&a[0]).transform(&m4(&a[1]))),
        _ => None,
    });
}

fn axis(v: &Value) -> LocalAxis {
    match s(v) {
        "east" => LocalAxis::East,
        "north" => LocalAxis::North,
        "up" => LocalAxis::Up,
        "west" => LocalAxis::West,
        "south" => LocalAxis::South,
        "down" => LocalAxis::Down,
        other => panic!("axis {other}"),
    }
}

#[test]
fn transforms() {
    let geo = GeographicProjection::default();
    run("core", "Transforms", |_| TIGHT, |f, a| match f {
        "localFrameToFixedFrame" => Some(match LocalFrame::new(axis(&a[0]), axis(&a[1])) {
            Some(frame) => Out::Ok(frame.to_fixed_frame(c3(&a[2]), &E).enc()),
            None => Out::Err("invalid axes".into()),
        }),
        "eastNorthUpToFixedFrame" => ok(Transforms::east_north_up_to_fixed_frame(c3(&a[0]), &E)),
        "headingPitchRollToFixedFrame" => {
            ok(Transforms::heading_pitch_roll_to_fixed_frame(c3(&a[0]), &hpr(&a[1]), &E, LocalFrame::EAST_NORTH_UP))
        }
        "headingPitchRollQuaternion" => {
            ok(Transforms::heading_pitch_roll_quaternion(c3(&a[0]), &hpr(&a[1]), &E, LocalFrame::EAST_NORTH_UP))
        }
        "fixedFrameToHeadingPitchRoll" => {
            ok(Transforms::fixed_frame_to_heading_pitch_roll(&m4(&a[0]), &E, LocalFrame::EAST_NORTH_UP))
        }
        "pointToWindowCoordinates" => {
            let (mvp, vp, p) = (m4(&a[0]), m4(&a[1]), c3(&a[2]));
            ok((
                Transforms::point_to_window_coordinates(&mvp, &vp, p),
                Transforms::point_to_gl_window_coordinates(&mvp, &vp, p),
            ))
        }
        "rotationMatrixFromPositionVelocity" => {
            ok(Transforms::rotation_matrix_from_position_velocity(c3(&a[0]), c3(&a[1]), &E))
        }
        "basisTo2D" => ok(Transforms::basis_to_2d(&geo, &m4(&a[0]))),
        "ellipsoidTo2DModelMatrix" => ok(Transforms::ellipsoid_to_2d_model_matrix(&geo, c3(&a[0]))),
        _ => None,
    });
}

#[test]
fn color() {
    run("core", "Color", |_| TIGHT, |f, a| {
        let c = |i: usize| common::color(&a[i]);
        match f {
            "fromCssColorString" => ok(Color::from_css_color_string(s(&a[0]))),
            "fromBytes" => ok(Color::from_bytes(n(&a[0]), n(&a[1]), n(&a[2]), n(&a[3]))),
            "fromRgba" => ok(Color::from_rgba(u(&a[0]))),
            "fromHsl" => ok(Color::from_hsl(n(&a[0]), n(&a[1]), n(&a[2]), n(&a[3]))),
            "floatToByte" => ok(Color::float_to_byte(n(&a[0]))),
            "toCssColorString" => ok(c(0).to_css_color_string()),
            "toCssHexString" => ok(c(0).to_css_hex_string()),
            "toBytes" => ok(c(0).to_bytes()),
            "toRgba" => ok(c(0).to_rgba()),
            "bytesToRgba" => ok(Color::bytes_to_rgba(
                n(&a[0]) as i32,
                n(&a[1]) as i32,
                n(&a[2]) as i32,
                n(&a[3]) as i32,
            )),
            "brighten" => ok(c(0).brighten(n(&a[1]))),
            "darken" => ok(c(0).darken(n(&a[1]))),
            "add" => ok(c(0).add(&c(1))),
            "subtract" => ok(c(0).subtract(&c(1))),
            "multiply" => ok(c(0).multiply(&c(1))),
            "divide" => ok(c(0).divide(&c(1))),
            "mod" => ok(c(0).modulo(&c(1))),
            "lerp" => ok(c(0).lerp(&c(1), n(&a[2]))),
            "multiplyByScalar" => ok(c(0).multiply_by_scalar(n(&a[1]))),
            "divideByScalar" => ok(c(0).divide_by_scalar(n(&a[1]))),
            "fromRandomSeeded" => {
                cm::set_random_number_seed(u(&a[0]));
                ok(vec![
                    Color::from_random(&ColorRandomOptions::default()),
                    Color::from_random(&ColorRandomOptions {
                        minimum_red: Some(0.5),
                        maximum_red: Some(0.6),
                        alpha: Some(1.0),
                        ..Default::default()
                    }),
                    Color::from_random(&ColorRandomOptions {
                        red: Some(0.1),
                        green: Some(0.2),
                        blue: Some(0.3),
                        minimum_alpha: Some(0.9),
                        ..Default::default()
                    }),
                ])
            }
            "named" => {
                let names = list(&a[0], |v| s(v).to_string());
                let got: Vec<Value> = names
                    .iter()
                    .map(|name| Color::named(name).map_or(json!(format!("missing {name}")), |c| c.enc()))
                    .collect();
                ok(got)
            }
            _ => None,
        }
    });
}
