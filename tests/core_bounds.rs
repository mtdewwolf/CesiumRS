//! Polynomials, intersections, bounding boxes, frustums, culling, tangent
//! planes, occluders, rhumb lines, and geodesics against CesiumJS fixtures.

mod common;

use cesium_rs::core::intersection_tests as it;
use cesium_rs::core::*;
use cesium_rs::core::{cubic_real_polynomial as cubic, quadratic_real_polynomial as quad, quartic_real_polynomial as quartic};
use common::*;
use serde_json::{json, Value};

const E: Ellipsoid = Ellipsoid::WGS84;

/// Fixture rays come from Cesium's constructor, so the direction is already normalized.
fn ray(v: &Value) -> Ray {
    Ray { origin: c3(&v["origin"]), direction: c3(&v["direction"]) }
}

fn obb(v: &Value) -> OrientedBoundingBox {
    let a = v.as_array().unwrap();
    OrientedBoundingBox::new(c3(&a[0]), m3(&a[1]))
}

fn obb_enc(b: &OrientedBoundingBox) -> Value {
    json!([b.center.enc(), b.half_axes.enc()])
}

fn aabb_enc(b: &AxisAlignedBoundingBox) -> Value {
    json!([b.minimum.enc(), b.maximum.enc(), b.center.enc()])
}

fn opt_num(v: &Value) -> Option<f64> {
    opt(v)
}

#[test]
fn polynomials() {
    run("core", "Polynomials", |_| TIGHT, |f, a| {
        let x = |i: usize| n(&a[i]);
        match f {
            "quadraticRoots" => ok(quad::compute_real_roots(x(0), x(1), x(2))),
            "quadraticDiscriminant" => ok(quad::compute_discriminant(x(0), x(1), x(2))),
            "cubicRoots" => ok(cubic::compute_real_roots(x(0), x(1), x(2), x(3))),
            "cubicDiscriminant" => ok(cubic::compute_discriminant(x(0), x(1), x(2), x(3))),
            "quarticRoots" => ok(quartic::compute_real_roots(x(0), x(1), x(2), x(3), x(4))),
            "quarticDiscriminant" => ok(quartic::compute_discriminant(x(0), x(1), x(2), x(3), x(4))),
            _ => None,
        }
    });
}

#[test]
fn intersection_tests() {
    // The grazing point's small components come from cancellation in large ones.
    let tol = |f: &str| if f == "grazingAltitudeLocation" { TIGHT_VECTOR } else { TIGHT };
    run("core", "IntersectionTests", tol, |f, a| match f {
        "Ray" => {
            let r = Ray::new(c3(&a[0]), c3(&a[1]));
            ok(vec![r.origin, r.direction])
        }
        "Ray.getPoint" => ok(ray(&a[0]).get_point(n(&a[1]))),
        "rayPlane" => ok(it::ray_plane(&ray(&a[0]), &plane(&a[1]))),
        "rayTriangleParametric" => ok(it::ray_triangle_parametric(
            &ray(&a[0]),
            c3(&a[1]),
            c3(&a[2]),
            c3(&a[3]),
            a[4].as_bool().unwrap(),
        )),
        "rayTriangle" => ok(it::ray_triangle(&ray(&a[0]), c3(&a[1]), c3(&a[2]), c3(&a[3]), a[4].as_bool().unwrap())),
        "lineSegmentTriangle" => ok(it::line_segment_triangle(
            c3(&a[0]),
            c3(&a[1]),
            c3(&a[2]),
            c3(&a[3]),
            c3(&a[4]),
            a[5].as_bool().unwrap(),
        )),
        "raySphere" => ok(it::ray_sphere(&ray(&a[0]), &bsphere(&a[1]))),
        "lineSegmentSphere" => ok(it::line_segment_sphere(c3(&a[0]), c3(&a[1]), &bsphere(&a[2]))),
        "rayEllipsoid" => ok(it::ray_ellipsoid(&ray(&a[0]), &E)),
        "rayAxisAlignedBoundingBox" => {
            let b = &a[1];
            let box_ = AxisAlignedBoundingBox::new(c3(&b["minimum"]), c3(&b["maximum"]), Some(c3(&b["center"])));
            ok(it::ray_axis_aligned_bounding_box(&ray(&a[0]), &box_))
        }
        "grazingAltitudeLocation" => ok(it::grazing_altitude_location(&ray(&a[0]), &E)),
        "lineSegmentPlane" => ok(it::line_segment_plane(c3(&a[0]), c3(&a[1]), &plane(&a[2]))),
        "trianglePlaneIntersection" => {
            let r = it::triangle_plane_intersection(c3(&a[0]), c3(&a[1]), c3(&a[2]), &plane(&a[3]));
            ok(r.map(|t| json!({ "positions": t.positions.enc(), "indices": t.indices.to_vec() })))
        }
        "quadraticVectorExpression" => {
            ok(it::quadratic_vector_expression(&m3(&a[0]), c3(&a[1]), n(&a[2]), n(&a[3]), n(&a[4])))
        }
        _ => None,
    });
}

#[test]
fn boxes() {
    run("core", "Boxes", |_| TIGHT, |f, a| match f {
        "AABB.fromPoints" => ok(aabb_enc(&AxisAlignedBoundingBox::from_points(&list(&a[0], c3)))),
        "AABB.intersectPlane" => ok(AxisAlignedBoundingBox::from_points(&list(&a[0], c3)).intersect_plane(&plane(&a[1]))),
        "AABB.intersectAxisAlignedBoundingBox" => ok(AxisAlignedBoundingBox::from_points(&list(&a[0], c3))
            .intersect_axis_aligned_bounding_box(&AxisAlignedBoundingBox::from_points(&list(&a[1], c3)))),
        "OBB.fromPoints" => ok(obb_enc(&OrientedBoundingBox::from_points(&list(&a[0], c3)))),
        "OBB.fromRectangle" => ok(OrientedBoundingBox::from_rectangle(&rect(&a[0]), n(&a[1]), n(&a[2]), &E).map(|b| obb_enc(&b))),
        "OBB.fromTransformation" => ok(obb_enc(&OrientedBoundingBox::from_transformation(&m4(&a[0])))),
        "OBB.intersectPlane" => ok(obb(&a[0]).intersect_plane(&plane(&a[1]))),
        "OBB.distanceSquaredTo" => ok(obb(&a[0]).distance_squared_to(c3(&a[1]))),
        "OBB.computePlaneDistances" => ok(obb(&a[0]).compute_plane_distances(c3(&a[1]), c3(&a[2]))),
        "OBB.computeCorners" => ok(obb(&a[0]).compute_corners()),
        "OBB.computeTransformation" => ok(obb(&a[0]).compute_transformation()),
        "BoundingSphere.fromOrientedBoundingBox" => ok(BoundingSphere::from_oriented_bounding_box(&obb(&a[0]))),
        _ => None,
    });
}

fn persp(v: &Value) -> PerspectiveFrustum {
    let p = nums(v);
    PerspectiveFrustum { fov: p[0], aspect_ratio: p[1], near: p[2], far: p[3], x_offset: p[4], y_offset: p[5] }
}

fn off6(v: &Value) -> [f64; 6] {
    nums(v).try_into().unwrap()
}

#[test]
fn frustums() {
    run("core", "Frustums", |_| TIGHT, |f, a| {
        let cam = |i: usize| (c3(&a[i]), c3(&a[i + 1]), c3(&a[i + 2]));
        match f {
            "PerspectiveFrustum" => {
                let fr = persp(&a[0]);
                let o = fr.off_center_frustum();
                ok(json!([
                    fr.fovy().enc(),
                    fr.sse_denominator().enc(),
                    ([o.left, o.right, o.top, o.bottom, o.near, o.far]).enc(),
                    fr.projection_matrix().enc(),
                    fr.infinite_projection_matrix().enc()
                ]))
            }
            "PerspectiveFrustum.computeCullingVolume" => {
                let (p, d, u) = cam(1);
                ok(persp(&a[0]).compute_culling_volume(p, d, u).planes)
            }
            "PerspectiveFrustum.getPixelDimensions" => {
                ok(persp(&a[0]).get_pixel_dimensions(n(&a[1]), n(&a[2]), n(&a[3]), n(&a[4])))
            }
            "PerspectiveOffCenterFrustum" => {
                let [l, r, t, b, ne, fa] = off6(&a[0]);
                let fr = PerspectiveOffCenterFrustum::new(l, r, t, b, ne, fa);
                let (p, d, u) = cam(1);
                ok(json!([
                    fr.projection_matrix().enc(),
                    fr.infinite_projection_matrix().enc(),
                    fr.compute_culling_volume(p, d, u).planes.enc(),
                    fr.get_pixel_dimensions(800.0, 600.0, 50.0, 1.0).enc()
                ]))
            }
            "OrthographicOffCenterFrustum" => {
                let [l, r, t, b, ne, fa] = off6(&a[0]);
                let fr = OrthographicOffCenterFrustum::new(l, r, t, b, ne, fa);
                let (p, d, u) = cam(1);
                ok(json!([
                    fr.projection_matrix().enc(),
                    fr.compute_culling_volume(p, d, u).planes.enc(),
                    fr.get_pixel_dimensions(800.0, 600.0, 50.0, 2.0).enc()
                ]))
            }
            "OrthographicFrustum" => {
                let v = nums(&a[0]);
                let fr = OrthographicFrustum { width: v[0], aspect_ratio: v[1], near: v[2], far: v[3] };
                let o = fr.off_center_frustum();
                let (p, d, u) = cam(1);
                ok(json!([
                    ([o.left, o.right, o.top, o.bottom, o.near, o.far]).enc(),
                    fr.projection_matrix().enc(),
                    fr.compute_culling_volume(p, d, u).planes.enc(),
                    fr.get_pixel_dimensions(640.0, 480.0, 10.0, 1.0).enc()
                ]))
            }
            "CullingVolume.fromBoundingSphere" => ok(CullingVolume::from_bounding_sphere(&bsphere(&a[0])).planes),
            "CullingVolume.computeVisibility" => {
                let (p, d, u) = cam(1);
                let cv = persp(&a[0]).compute_culling_volume(p, d, u);
                let b = bsphere(&a[4]);
                let r = Cartesian3::new(b.radius, b.radius, b.radius);
                let box_ = AxisAlignedBoundingBox::from_points(&[b.center.subtract(r), b.center.add(r)]);
                let masks: Vec<Value> = [CullingVolume::MASK_INDETERMINATE, 0b101010, 0b111111, CullingVolume::MASK_INSIDE]
                    .iter()
                    .map(|m| json!(cv.compute_visibility_with_plane_mask(&b, *m)))
                    .collect();
                let mut out = vec![cv.compute_visibility(&b).enc(), cv.compute_visibility(&box_).enc()];
                out.extend(masks);
                ok(out)
            }
            _ => None,
        }
    });
}

#[test]
fn tangent_plane() {
    run("core", "EllipsoidTangentPlane", |_| TIGHT, |f, a| {
        let tp = |v: &Value| EllipsoidTangentPlane::new(c3(v), &E).expect("tangent plane");
        let enc = |t: &EllipsoidTangentPlane| json!([t.origin.enc(), t.x_axis.enc(), t.y_axis.enc(), t.z_axis().enc(), t.plane.enc()]);
        match f {
            "new" => ok(enc(&tp(&a[0]))),
            "fromPoints" => ok(enc(&EllipsoidTangentPlane::from_points(&list(&a[0], c3), &E).unwrap())),
            "projectPointOntoPlane" => ok(tp(&a[0]).project_point_onto_plane(c3(&a[1]))),
            "projectPointsOntoPlane" => ok(tp(&a[0]).project_points_onto_plane(&list(&a[1], c3))),
            "projectPointToNearestOnPlane" => ok(tp(&a[0]).project_point_to_nearest_on_plane(c3(&a[1]))),
            "projectPointOntoEllipsoid" => ok(tp(&a[0]).project_point_onto_ellipsoid(c2(&a[1]))),
            _ => None,
        }
    });
}

#[test]
fn occluders() {
    run("core", "Occluders", |_| TIGHT, |f, a| match f {
        "Occluder.isPointVisible" => ok(Occluder::new(&bsphere(&a[0]), c3(&a[1])).is_point_visible(c3(&a[2]))),
        "Occluder.isBoundingSphereVisible" => {
            ok(Occluder::new(&bsphere(&a[0]), c3(&a[1])).is_bounding_sphere_visible(&bsphere(&a[2])))
        }
        "Occluder.computeVisibility" => {
            ok(json!(Occluder::new(&bsphere(&a[0]), c3(&a[1])).compute_visibility(&bsphere(&a[2])) as i8))
        }
        "Occluder.computeOccludeePoint" => ok(Occluder::compute_occludee_point(&bsphere(&a[0]), c3(&a[1]), &list(&a[2], c3))),
        "Occluder.computeOccludeePointFromRectangle" => ok(Occluder::compute_occludee_point_from_rectangle(&rect(&a[0]), &E)),
        "EllipsoidalOccluder.isPointVisible" => ok(EllipsoidalOccluder::new(&E, Some(c3(&a[0]))).is_point_visible(c3(&a[1]))),
        "EllipsoidalOccluder.isScaledSpacePointVisiblePossiblyUnderEllipsoid" => ok(EllipsoidalOccluder::new(&E, Some(c3(&a[0])))
            .is_scaled_space_point_visible_possibly_under_ellipsoid(c3(&a[1]), opt_num(&a[2]))),
        "EllipsoidalOccluder.computeHorizonCullingPoint" => {
            ok(EllipsoidalOccluder::new(&E, None).compute_horizon_culling_point(c3(&a[0]), &list(&a[1], c3)))
        }
        "EllipsoidalOccluder.computeHorizonCullingPointPossiblyUnderEllipsoid" => ok(EllipsoidalOccluder::new(&E, None)
            .compute_horizon_culling_point_possibly_under_ellipsoid(c3(&a[0]), &list(&a[1], c3), opt_num(&a[2]))),
        "EllipsoidalOccluder.computeHorizonCullingPointFromVertices" => ok(EllipsoidalOccluder::new(&E, None)
            .compute_horizon_culling_point_from_vertices(c3(&a[0]), &nums(&a[1]), 3, c3(&a[2]))),
        "EllipsoidalOccluder.computeHorizonCullingPointFromRectangle" => {
            ok(EllipsoidalOccluder::new(&E, None).compute_horizon_culling_point_from_rectangle(&rect(&a[0]), &E))
        }
        _ => None,
    });
}

#[test]
fn curves() {
    let fractions = [0.0, 0.25, 0.5, 0.9, 1.0];
    // Rhumb-line intersections scale a sigma difference by tan(heading),
    // which magnifies last-bit libm differences near east-west headings.
    let tol = |f: &str| {
        if f.starts_with("EllipsoidRhumbLine.findIntersection") {
            Tol { rel: 1e-9, abs: 1e-9, vector: false }
        } else {
            TIGHT
        }
    };
    run("core", "Curves", tol, |f, a| match f {
        "EllipsoidGeodesic" => Some(match EllipsoidGeodesic::new(&carto(&a[0]), &carto(&a[1]), &E) {
            Some(g) => {
                let mut out = vec![g.surface_distance.enc(), g.start_heading.enc(), g.end_heading.enc()];
                out.extend(fractions.iter().map(|t| g.interpolate_using_fraction(*t).enc()));
                Out::Ok(json!(out))
            }
            None => Out::Err("near antipodal".into()),
        }),
        "EllipsoidGeodesicMoon" => Some(match EllipsoidGeodesic::new(&carto(&a[0]), &carto(&a[1]), &Ellipsoid::MOON) {
            Some(g) => Out::Ok(json!([
                g.surface_distance.enc(),
                g.start_heading.enc(),
                g.end_heading.enc(),
                g.interpolate_using_surface_distance(1000.0).enc()
            ])),
            None => Out::Err("near antipodal".into()),
        }),
        "EllipsoidRhumbLine" => {
            let l = EllipsoidRhumbLine::new(&carto(&a[0]), &carto(&a[1]), &E);
            let mut out = vec![l.surface_distance.enc(), l.heading.enc(), l.start.enc(), l.end.enc()];
            out.extend(fractions.iter().map(|t| l.interpolate_using_fraction(*t).enc()));
            ok(json!(out))
        }
        "EllipsoidRhumbLine.fromStartHeadingDistance" => {
            let l = EllipsoidRhumbLine::from_start_heading_distance(&carto(&a[0]), n(&a[1]), n(&a[2]), &E);
            ok(json!([l.surface_distance.enc(), l.heading.enc(), l.end.enc()]))
        }
        "EllipsoidRhumbLine.findIntersectionWithLongitude" => {
            ok(EllipsoidRhumbLine::new(&carto(&a[0]), &carto(&a[1]), &E).find_intersection_with_longitude(n(&a[2])))
        }
        "EllipsoidRhumbLine.findIntersectionWithLatitude" => {
            ok(EllipsoidRhumbLine::new(&carto(&a[0]), &carto(&a[1]), &E).find_intersection_with_latitude(n(&a[2])))
        }
        "EllipsoidRhumbLineSphere" => {
            let l = EllipsoidRhumbLine::new(&carto(&a[0]), &carto(&a[1]), &Ellipsoid::UNIT_SPHERE);
            ok(json!([l.surface_distance.enc(), l.heading.enc(), l.interpolate_using_fraction(0.3).enc()]))
        }
        _ => None,
    });
}
