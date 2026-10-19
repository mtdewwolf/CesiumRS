//! Core math types against CesiumJS fixtures.

mod common;

use cesium_rs::core::math as cm;
use cesium_rs::core::*;
use common::*;
use serde_json::Value;

#[test]
fn math() {
    run(
        "core",
        "Math",
        |_| TIGHT,
        |f, a| {
            let x = |i: usize| n(&a[i]);
            match f {
                "sign" => ok(cm::sign(x(0))),
                "signNotZero" => ok(cm::sign_not_zero(x(0))),
                "toSNorm" => ok(cm::to_snorm(x(0), x(1))),
                "fromSNorm" => ok(cm::from_snorm(x(0), x(1))),
                "normalize" => ok(cm::normalize(x(0), x(1), x(2))),
                "lerp" => ok(cm::lerp(x(0), x(1), x(2))),
                "toRadians" => ok(cm::to_radians(x(0))),
                "toDegrees" => ok(cm::to_degrees(x(0))),
                "convertLongitudeRange" => ok(cm::convert_longitude_range(x(0))),
                "clampToLatitudeRange" => ok(cm::clamp_to_latitude_range(x(0))),
                "negativePiToPi" => ok(cm::negative_pi_to_pi(x(0))),
                "zeroToTwoPi" => ok(cm::zero_to_two_pi(x(0))),
                "mod" => ok(cm::modulo(x(0), x(1))),
                "equalsEpsilon" => {
                    let rel = x(2);
                    ok(cm::equals_epsilon(x(0), x(1), rel, opt(&a[3]).unwrap_or(rel)))
                }
                "lessThan" => ok(cm::less_than(x(0), x(1), x(2))),
                "lessThanOrEquals" => ok(cm::less_than_or_equals(x(0), x(1), x(2))),
                "greaterThan" => ok(cm::greater_than(x(0), x(1), x(2))),
                "greaterThanOrEquals" => ok(cm::greater_than_or_equals(x(0), x(1), x(2))),
                "factorial" => ok(cm::factorial(u(&a[0]))),
                "incrementWrap" => ok(cm::increment_wrap(x(0), x(1), x(2))),
                "isPowerOfTwo" => ok(cm::is_power_of_two(u(&a[0]))),
                "nextPowerOfTwo" => ok(f64::from(cm::next_power_of_two(u(&a[0])))),
                "previousPowerOfTwo" => ok(f64::from(cm::previous_power_of_two(u(&a[0])))),
                "clamp" => ok(cm::clamp(x(0), x(1), x(2))),
                "acosClamped" => ok(cm::acos_clamped(x(0))),
                "asinClamped" => ok(cm::asin_clamped(x(0))),
                "chordLength" => ok(cm::chord_length(x(0), x(1))),
                "logBase" => ok(cm::log_base(x(0), x(1))),
                "cbrt" => ok(cm::cbrt(x(0))),
                "log2" => ok(cm::log2(x(0))),
                "fog" => ok(cm::fog(x(0), x(1))),
                "fastApproximateAtan" => ok(cm::fast_approximate_atan(x(0))),
                "fastApproximateAtan2" => ok(cm::fast_approximate_atan2(x(0), x(1))),
                "randomSequence" => {
                    cm::set_random_number_seed(u(&a[0]));
                    let all: Vec<f64> = (0..700).map(|_| cm::next_random_number()).collect();
                    ok(all
                        .into_iter()
                        .enumerate()
                        .filter(|(i, _)| *i < 5 || i % 97 == 0 || *i > 620)
                        .map(|(_, v)| v)
                        .collect::<Vec<_>>())
                }
                _ => None,
            }
        },
    );
}

/// Functions every Cartesian type shares, mapped by name.
macro_rules! cartesian_port {
    ($f:expr, $a:expr, $dec:ident) => {{
        let a = $a;
        let v = |i: usize| $dec(&a[i]);
        match $f {
            "magnitude" => ok(v(0).magnitude()),
            "magnitudeSquared" => ok(v(0).magnitude_squared()),
            "distance" => ok(v(0).distance(v(1))),
            "distanceSquared" => ok(v(0).distance_squared(v(1))),
            "normalize" => ok(v(0).normalize()),
            "dot" => ok(v(0).dot(v(1))),
            "multiplyComponents" => ok(v(0).multiply_components(v(1))),
            "divideComponents" => ok(v(0).divide_components(v(1))),
            "add" => ok(v(0).add(v(1))),
            "subtract" => ok(v(0).subtract(v(1))),
            "multiplyByScalar" => ok(v(0).multiply_by_scalar(n(&a[1]))),
            "divideByScalar" => ok(v(0).divide_by_scalar(n(&a[1]))),
            "negate" => ok(v(0).negate()),
            "abs" => ok(v(0).abs()),
            "lerp" => ok(v(0).lerp(v(1), n(&a[2]))),
            "maximumComponent" => ok(v(0).maximum_component()),
            "minimumComponent" => ok(v(0).minimum_component()),
            "minimumByComponent" => ok(v(0).minimum_by_component(v(1))),
            "maximumByComponent" => ok(v(0).maximum_by_component(v(1))),
            "clamp" => ok(v(0).clamp(v(1), v(2))),
            "mostOrthogonalAxis" => ok(v(0).most_orthogonal_axis()),
            "equalsEpsilon" => {
                let rel = n(&a[2]);
                ok(v(0).equals_epsilon_with(v(1), rel, opt(&a[3]).unwrap_or(rel)))
            }
            _ => None,
        }
    }};
}

#[test]
fn cartesian2() {
    run(
        "core",
        "Cartesian2",
        |_| TIGHT,
        |f, a| {
            cartesian_port!(f, a, c2).or_else(|| match f {
                "cross" => ok(c2(&a[0]).cross(c2(&a[1]))),
                "angleBetween" => ok(c2(&a[0]).angle_between(c2(&a[1]))),
                _ => None,
            })
        },
    );
}

#[test]
fn cartesian3() {
    let e = Ellipsoid::WGS84;
    run(
        "core",
        "Cartesian3",
        |_| TIGHT,
        |f, a| {
            cartesian_port!(f, a, c3).or_else(|| match f {
                "cross" => ok(c3(&a[0]).cross(c3(&a[1]))),
                "midpoint" => ok(c3(&a[0]).midpoint(c3(&a[1]))),
                "projectVector" => ok(c3(&a[0]).project_vector(c3(&a[1]))),
                "angleBetween" => ok(c3(&a[0]).angle_between(c3(&a[1]))),
                "fromSpherical" => ok(Cartesian3::from_spherical(&sph(&a[0]))),
                "fromDegrees" => ok(Cartesian3::from_degrees(n(&a[0]), n(&a[1]), n(&a[2]), &e)),
                "fromRadians" => ok(Cartesian3::from_radians(n(&a[0]), n(&a[1]), n(&a[2]), &e)),
                "fromRadiansMoon" => ok(Cartesian3::from_radians(n(&a[0]), n(&a[1]), n(&a[2]), &Ellipsoid::MOON)),
                "fromDegreesArrayHeights" => ok(Cartesian3::from_degrees_array_heights(&nums(&a[0]), &e)),
                "fromDegreesArray" => ok(Cartesian3::from_degrees_array(&nums(&a[0]), &e)),
                _ => None,
            })
        },
    );
}

#[test]
fn cartesian4() {
    run(
        "core",
        "Cartesian4",
        |_| TIGHT,
        |f, a| {
            cartesian_port!(f, a, c4).or_else(|| match f {
                "packFloat" => ok(Cartesian4::pack_float(n(&a[0]))),
                "unpackFloat" => ok(Cartesian4::unpack_float(c4(&a[0]))),
                _ => None,
            })
        },
    );
}

/// Functions every matrix type shares, mapped by name.
macro_rules! matrix_port {
    ($f:expr, $a:expr, $dec:ident, $T:ident, $vec:ident) => {{
        let a = $a;
        let m = |i: usize| $dec(&a[i]);
        let idx = |i: usize| n(&a[i]) as usize;
        match $f {
            "multiply" => ok(m(0).multiply(&m(1))),
            "add" => ok(m(0).add(&m(1))),
            "subtract" => ok(m(0).subtract(&m(1))),
            "multiplyByScalar" => ok(m(0).multiply_by_scalar(n(&a[1]))),
            "negate" => ok(m(0).negate()),
            "abs" => ok(m(0).abs()),
            "transpose" => ok(m(0).transpose()),
            "fromRowMajorArray" => ok($T::from_row_major_array(m(0).0)),
            "getColumn" => ok(m(0).get_column(idx(1))),
            "getRow" => ok(m(0).get_row(idx(1))),
            "setColumn" => ok(m(0).set_column(idx(1), $vec(&a[2]))),
            "setRow" => ok(m(0).set_row(idx(1), $vec(&a[2]))),
            "equalsEpsilon" => ok(m(0).equals_epsilon(&m(1), n(&a[2]))),
            _ => None,
        }
    }};
}

/// Scale and rotation helpers shared by Matrix2 and Matrix3.
macro_rules! scale_port {
    ($f:expr, $a:expr, $dec:ident, $T:ident, $vec:ident) => {{
        let a = $a;
        let m = |i: usize| $dec(&a[i]);
        match $f {
            "fromScale" => ok($T::from_scale($vec(&a[0]))),
            "fromUniformScale" => ok($T::from_uniform_scale(n(&a[0]))),
            "getScale" => ok(m(0).get_scale()),
            "getMaximumScale" => ok(m(0).get_maximum_scale()),
            "setScale" => ok(m(0).set_scale($vec(&a[1]))),
            "setUniformScale" => ok(m(0).set_uniform_scale(n(&a[1]))),
            "setRotation" => ok(m(0).set_rotation(&m(1))),
            "getRotation" => ok(m(0).get_rotation()),
            "multiplyByVector" => ok(m(0).multiply_by_vector($vec(&a[1]))),
            "multiplyByScale" => ok(m(0).multiply_by_scale($vec(&a[1]))),
            "multiplyByUniformScale" => ok(m(0).multiply_by_uniform_scale(n(&a[1]))),
            _ => None,
        }
    }};
}

#[test]
fn matrix2() {
    run(
        "core",
        "Matrix2",
        |_| TIGHT,
        |f, a| {
            matrix_port!(f, a, m2, Matrix2, c2).or_else(|| scale_port!(f, a, m2, Matrix2, c2)).or_else(|| match f {
                "fromRotation" => ok(Matrix2::from_rotation(n(&a[0]))),
                _ => None,
            })
        },
    );
}

#[test]
fn matrix3() {
    run(
        "core",
        "Matrix3",
        |_| TIGHT,
        |f, a| {
            matrix_port!(f, a, m3, Matrix3, c3).or_else(|| scale_port!(f, a, m3, Matrix3, c3)).or_else(|| match f {
                "fromQuaternion" => ok(Matrix3::from_quaternion(&q(&a[0]))),
                "fromHeadingPitchRoll" => ok(Matrix3::from_heading_pitch_roll(&hpr(&a[0]))),
                "fromCrossProduct" => ok(Matrix3::from_cross_product(c3(&a[0]))),
                "fromRotationX" => ok(Matrix3::from_rotation_x(n(&a[0]))),
                "fromRotationY" => ok(Matrix3::from_rotation_y(n(&a[0]))),
                "fromRotationZ" => ok(Matrix3::from_rotation_z(n(&a[0]))),
                "determinant" => ok(m3(&a[0]).determinant()),
                "inverse" => ok(m3(&a[0]).inverse()),
                "inverseTranspose" => ok(m3(&a[0]).inverse_transpose()),
                "computeEigenDecomposition" => {
                    let d = m3(&a[0]).compute_eigen_decomposition();
                    ok([d.unitary, d.diagonal])
                }
                _ => None,
            })
        },
    );
}

#[test]
fn matrix4() {
    run(
        "core",
        "Matrix4",
        |_| TIGHT,
        |f, a| {
            let m = |i: usize| m4(&a[i]);
            matrix_port!(f, a, m4, Matrix4, c4).or_else(|| match f {
                "fromRotationTranslation" => ok(Matrix4::from_rotation_translation(&m3(&a[0]), c3(&a[1]))),
                "fromTranslationQuaternionRotationScale" => {
                    ok(Matrix4::from_translation_quaternion_rotation_scale(c3(&a[0]), &q(&a[1]), c3(&a[2])))
                }
                "fromTranslation" => ok(Matrix4::from_translation(c3(&a[0]))),
                "fromScale" => ok(Matrix4::from_scale(c3(&a[0]))),
                "fromUniformScale" => ok(Matrix4::from_uniform_scale(n(&a[0]))),
                "fromRotation" => ok(Matrix4::from_rotation(&m3(&a[0]))),
                "fromCamera" => ok(Matrix4::from_camera(c3(&a[0]), c3(&a[1]), c3(&a[2]))),
                "computePerspectiveFieldOfView" => {
                    ok(Matrix4::compute_perspective_field_of_view(n(&a[0]), n(&a[1]), n(&a[2]), n(&a[3])))
                }
                "computeOrthographicOffCenter" => ok(Matrix4::compute_orthographic_off_center(
                    n(&a[0]),
                    n(&a[1]),
                    n(&a[2]),
                    n(&a[3]),
                    n(&a[4]),
                    n(&a[5]),
                )),
                "computePerspectiveOffCenter" => ok(Matrix4::compute_perspective_off_center(
                    n(&a[0]),
                    n(&a[1]),
                    n(&a[2]),
                    n(&a[3]),
                    n(&a[4]),
                    n(&a[5]),
                )),
                "computeInfinitePerspectiveOffCenter" => ok(Matrix4::compute_infinite_perspective_off_center(
                    n(&a[0]),
                    n(&a[1]),
                    n(&a[2]),
                    n(&a[3]),
                    n(&a[4]),
                )),
                "computeViewportTransformation" => {
                    ok(Matrix4::compute_viewport_transformation(&brect(&a[0]), n(&a[1]), n(&a[2])))
                }
                "computeView" => ok(Matrix4::compute_view(c3(&a[0]), c3(&a[1]), c3(&a[2]), c3(&a[3]))),
                "setTranslation" => ok(m(0).set_translation(c3(&a[1]))),
                "getScale" => ok(m(0).get_scale()),
                "getMaximumScale" => ok(m(0).get_maximum_scale()),
                "setScale" => ok(m(0).set_scale(c3(&a[1]))),
                "setUniformScale" => ok(m(0).set_uniform_scale(n(&a[1]))),
                "setRotation" => ok(m(0).set_rotation(&m3(&a[1]))),
                "getRotation" => ok(m(0).get_rotation()),
                "multiplyTransformation" => ok(m(0).multiply_transformation(&m(1))),
                "multiplyByMatrix3" => ok(m(0).multiply_by_matrix3(&m3(&a[1]))),
                "multiplyByTranslation" => ok(m(0).multiply_by_translation(c3(&a[1]))),
                "multiplyByScale" => ok(m(0).multiply_by_scale(c3(&a[1]))),
                "multiplyByUniformScale" => ok(m(0).multiply_by_uniform_scale(n(&a[1]))),
                "multiplyByVector" => ok(m(0).multiply_by_vector(c4(&a[1]))),
                "multiplyByPointAsVector" => ok(m(0).multiply_by_point_as_vector(c3(&a[1]))),
                "multiplyByPoint" => ok(m(0).multiply_by_point(c3(&a[1]))),
                "getTranslation" => ok(m(0).get_translation()),
                "getMatrix3" => ok(m(0).get_matrix3()),
                "inverse" => res(m(0).inverse()),
                "inverseTransformation" => ok(m(0).inverse_transformation()),
                "inverseTranspose" => res(m(0).inverse_transpose()),
                _ => None,
            })
        },
    );
}

#[test]
fn quaternion() {
    run(
        "core",
        "Quaternion",
        |_| TIGHT,
        |f, a| {
            let x = |i: usize| q(&a[i]);
            match f {
                "fromAxisAngle" => ok(Quaternion::from_axis_angle(c3(&a[0]), n(&a[1]))),
                "fromRotationMatrix" => ok(Quaternion::from_rotation_matrix(&m3(&a[0]))),
                "fromHeadingPitchRoll" => ok(Quaternion::from_heading_pitch_roll(&hpr(&a[0]))),
                "conjugate" => ok(x(0).conjugate()),
                "magnitude" => ok(x(0).magnitude()),
                "normalize" => ok(x(0).normalize()),
                "inverse" => ok(x(0).inverse()),
                "add" => ok(x(0).add(&x(1))),
                "subtract" => ok(x(0).subtract(&x(1))),
                "negate" => ok(x(0).negate()),
                "dot" => ok(x(0).dot(&x(1))),
                "multiply" => ok(x(0).multiply(&x(1))),
                "multiplyByScalar" => ok(x(0).multiply_by_scalar(n(&a[1]))),
                "divideByScalar" => ok(x(0).divide_by_scalar(n(&a[1]))),
                "computeAxis" => ok(x(0).compute_axis()),
                "computeAngle" => ok(x(0).compute_angle()),
                "lerp" => ok(x(0).lerp(&x(1), n(&a[2]))),
                "slerp" => ok(x(0).slerp(&x(1), n(&a[2]))),
                "log" => ok(x(0).log()),
                "exp" => ok(Quaternion::exp(c3(&a[0]))),
                "computeInnerQuadrangle" => ok(Quaternion::compute_inner_quadrangle(&x(0), &x(1), &x(2))),
                "squad" => ok(Quaternion::squad(&x(0), &x(1), &x(2), &x(3), n(&a[4]))),
                "fastSlerp" => ok(x(0).fast_slerp(&x(1), n(&a[2]))),
                "fastSquad" => ok(Quaternion::fast_squad(&x(0), &x(1), &x(2), &x(3), n(&a[4]))),
                "convertPackedArrayForInterpolation" => {
                    let qs = list(&a[0], q);
                    let mut packed = vec![0.0; qs.len() * 4];
                    for (i, v) in qs.iter().enumerate() {
                        v.pack(&mut packed, i * 4);
                    }
                    ok(<Quaternion as PackableForInterpolation>::convert_packed_array_for_interpolation(
                        &packed,
                        0,
                        qs.len() - 1,
                    ))
                }
                "unpackInterpolationResult" => {
                    let v = c3(&a[0]);
                    let mut source = [0.0; 4];
                    x(1).pack(&mut source, 0);
                    ok(<Quaternion as PackableForInterpolation>::unpack_interpolation_result(
                        &[v.x, v.y, v.z],
                        &source,
                        0,
                        0,
                    ))
                }
                _ => None,
            }
        },
    );
}

#[test]
fn heading_pitch_roll() {
    run(
        "core",
        "HeadingPitchRoll",
        |_| TIGHT,
        |f, a| match f {
            "fromQuaternion" => ok(HeadingPitchRoll::from_quaternion(&q(&a[0]))),
            "fromDegrees" => ok(HeadingPitchRoll::from_degrees(n(&a[0]), n(&a[1]), n(&a[2]))),
            "roundTrip" => ok(HeadingPitchRoll::from_quaternion(&Quaternion::from_heading_pitch_roll(&hpr(&a[0])))),
            _ => None,
        },
    );
}

#[test]
fn spherical() {
    run(
        "core",
        "Spherical",
        |_| TIGHT,
        |f, a: &[Value]| match f {
            "fromCartesian3" => ok(Spherical::from_cartesian3(c3(&a[0]))),
            "normalize" => ok(sph(&a[0]).normalize()),
            _ => None,
        },
    );
}
