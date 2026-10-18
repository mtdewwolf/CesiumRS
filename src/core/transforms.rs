//! CesiumJS `Transforms`: local frames, heading-pitch-roll frames,
//! window-coordinate helpers, and the time-dependent TEME, ICRF, and Moon
//! frames.
//!
//! Cesium keeps the Earth orientation and IAU 2006 XYS data in the globals
//! `Transforms.earthOrientationParameters` and `Transforms.iau2006XysData`;
//! here the functions that need them take them as arguments.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartesian4::Cartesian4;
use super::earth_orientation_parameters::EarthOrientationParameters;
use super::ellipsoid::Ellipsoid;
use super::heading_pitch_roll::HeadingPitchRoll;
use super::iau2006_xys_data::Iau2006XysData;
use super::julian_date::JulianDate;
use super::map_projection::MapProjection;
use super::math::{equals_epsilon, sign, to_radians, EPSILON14, EPSILON6, RADIANS_PER_DEGREE, TWO_PI};
use super::matrix3::Matrix3;
use super::matrix4::Matrix4;
use super::quaternion::Quaternion;
use super::time_constants::{DAYS_PER_JULIAN_CENTURY, SECONDS_PER_DAY};
use super::time_interval::TimeInterval;

/// An axis of a local frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LocalAxis {
    East,
    North,
    Up,
    West,
    South,
    Down,
}

impl LocalAxis {
    /// Direction used at the center of the ellipsoid, where the frame is degenerate.
    fn degenerate(self) -> Cartesian3 {
        match self {
            Self::North => Cartesian3::new(-1.0, 0.0, 0.0),
            Self::East => Cartesian3::new(0.0, 1.0, 0.0),
            Self::Up => Cartesian3::new(0.0, 0.0, 1.0),
            Self::South => Cartesian3::new(1.0, 0.0, 0.0),
            Self::West => Cartesian3::new(0.0, -1.0, 0.0),
            Self::Down => Cartesian3::new(0.0, 0.0, -1.0),
        }
    }

    fn is_east_west(self) -> bool {
        matches!(self, Self::East | Self::West)
    }

    /// `first × second` for a right-handed frame, or `None` if not perpendicular.
    fn third(first: Self, second: Self) -> Option<Self> {
        use LocalAxis::*;
        Some(match (first, second) {
            (Up, South) => East,
            (Up, North) => West,
            (Up, West) => South,
            (Up, East) => North,
            (Down, South) => West,
            (Down, North) => East,
            (Down, West) => North,
            (Down, East) => South,
            (South, Up) => West,
            (South, Down) => East,
            (South, West) => Down,
            (South, East) => Up,
            (North, Up) => East,
            (North, Down) => West,
            (North, West) => Up,
            (North, East) => Down,
            (West, Up) => North,
            (West, Down) => South,
            (West, North) => Down,
            (West, South) => Up,
            (East, Up) => South,
            (East, Down) => North,
            (East, North) => Up,
            (East, South) => Down,
            _ => return None,
        })
    }
}

/// A local frame named by its first two axes, Cesium
/// `localFrameToFixedFrameGenerator(firstAxis, secondAxis)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LocalFrame {
    pub first: LocalAxis,
    pub second: LocalAxis,
    pub third: LocalAxis,
}

impl LocalFrame {
    pub const EAST_NORTH_UP: Self = Self { first: LocalAxis::East, second: LocalAxis::North, third: LocalAxis::Up };
    pub const NORTH_EAST_DOWN: Self = Self { first: LocalAxis::North, second: LocalAxis::East, third: LocalAxis::Down };
    pub const NORTH_UP_EAST: Self = Self { first: LocalAxis::North, second: LocalAxis::Up, third: LocalAxis::East };
    pub const NORTH_WEST_UP: Self = Self { first: LocalAxis::North, second: LocalAxis::West, third: LocalAxis::Up };

    /// `None` unless the axes are perpendicular, where Cesium throws.
    pub fn new(first: LocalAxis, second: LocalAxis) -> Option<Self> {
        Some(Self { first, second, third: LocalAxis::third(first, second)? })
    }

    /// Matrix from this local frame at `origin` to the ellipsoid's fixed frame.
    pub fn to_fixed_frame(&self, origin: Cartesian3, ellipsoid: &Ellipsoid) -> Matrix4 {
        let (a, b, c) = if origin.equals_epsilon(Cartesian3::ZERO, EPSILON14) {
            (self.first.degenerate(), self.second.degenerate(), self.third.degenerate())
        } else if equals_epsilon(origin.x, 0.0, EPSILON14, EPSILON14)
            && equals_epsilon(origin.y, 0.0, EPSILON14, EPSILON14)
        {
            let s = sign(origin.z);
            let pick = |axis: LocalAxis| {
                let v = axis.degenerate();
                if axis.is_east_west() {
                    v
                } else {
                    v.multiply_by_scalar(s)
                }
            };
            (pick(self.first), pick(self.second), pick(self.third))
        } else {
            let up = ellipsoid.geodetic_surface_normal(origin).expect("origin is away from the center");
            let east = Cartesian3::new(-origin.y, origin.x, 0.0).normalize();
            let north = up.cross(east);
            let axis = |a: LocalAxis| match a {
                LocalAxis::East => east,
                LocalAxis::North => north,
                LocalAxis::Up => up,
                LocalAxis::West => east.multiply_by_scalar(-1.0),
                LocalAxis::South => north.multiply_by_scalar(-1.0),
                LocalAxis::Down => up.multiply_by_scalar(-1.0),
            };
            (axis(self.first), axis(self.second), axis(self.third))
        };
        Matrix4([a.x, a.y, a.z, 0.0, b.x, b.y, b.z, 0.0, c.x, c.y, c.z, 0.0, origin.x, origin.y, origin.z, 1.0])
    }
}

/// Namespace for Cesium's `Transforms` functions.
pub struct Transforms;

impl Transforms {
    /// Rows `[0,0,1,0; 1,0,0,0; 0,1,0,0; 0,0,0,1]`: swaps x, y, z for 2D.
    pub const SWIZZLE_3D_TO_2D_MATRIX: Matrix4 =
        Matrix4::new(0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0);

    pub fn east_north_up_to_fixed_frame(origin: Cartesian3, ellipsoid: &Ellipsoid) -> Matrix4 {
        LocalFrame::EAST_NORTH_UP.to_fixed_frame(origin, ellipsoid)
    }

    pub fn north_east_down_to_fixed_frame(origin: Cartesian3, ellipsoid: &Ellipsoid) -> Matrix4 {
        LocalFrame::NORTH_EAST_DOWN.to_fixed_frame(origin, ellipsoid)
    }

    pub fn north_up_east_to_fixed_frame(origin: Cartesian3, ellipsoid: &Ellipsoid) -> Matrix4 {
        LocalFrame::NORTH_UP_EAST.to_fixed_frame(origin, ellipsoid)
    }

    pub fn north_west_up_to_fixed_frame(origin: Cartesian3, ellipsoid: &Ellipsoid) -> Matrix4 {
        LocalFrame::NORTH_WEST_UP.to_fixed_frame(origin, ellipsoid)
    }

    /// Local frame at `origin` rotated by `hpr`. Cesium's default frame is east-north-up.
    pub fn heading_pitch_roll_to_fixed_frame(
        origin: Cartesian3,
        hpr: &HeadingPitchRoll,
        ellipsoid: &Ellipsoid,
        frame: LocalFrame,
    ) -> Matrix4 {
        let q = Quaternion::from_heading_pitch_roll(hpr);
        let hpr_matrix = Matrix4::from_translation_quaternion_rotation_scale(Cartesian3::ZERO, &q, Cartesian3::ONE);
        frame.to_fixed_frame(origin, ellipsoid).multiply(&hpr_matrix)
    }

    pub fn heading_pitch_roll_quaternion(
        origin: Cartesian3,
        hpr: &HeadingPitchRoll,
        ellipsoid: &Ellipsoid,
        frame: LocalFrame,
    ) -> Quaternion {
        let transform = Self::heading_pitch_roll_to_fixed_frame(origin, hpr, ellipsoid, frame);
        Quaternion::from_rotation_matrix(&transform.get_matrix3())
    }

    /// Heading, pitch, and roll of `transform` relative to the local frame at its translation.
    pub fn fixed_frame_to_heading_pitch_roll(
        transform: &Matrix4,
        ellipsoid: &Ellipsoid,
        frame: LocalFrame,
    ) -> HeadingPitchRoll {
        let center = transform.get_translation();
        if center == Cartesian3::ZERO {
            return HeadingPitchRoll::default();
        }
        let to_fixed_frame = frame.to_fixed_frame(center, ellipsoid).inverse_transformation();
        let copy = transform.set_scale(Cartesian3::ONE).set_translation(Cartesian3::ZERO);
        let local = to_fixed_frame.multiply(&copy);
        let q = Quaternion::from_rotation_matrix(&local.get_matrix3()).normalize();
        HeadingPitchRoll::from_quaternion(&q)
    }

    /// Window coordinates with the origin at the top left.
    pub fn point_to_window_coordinates(
        model_view_projection: &Matrix4,
        viewport_transformation: &Matrix4,
        point: Cartesian3,
    ) -> Cartesian2 {
        let mut r = Self::point_to_gl_window_coordinates(model_view_projection, viewport_transformation, point);
        r.y = 2.0 * viewport_transformation[5] - r.y;
        r
    }

    /// Window coordinates with the origin at the bottom left, as in WebGL.
    pub fn point_to_gl_window_coordinates(
        model_view_projection: &Matrix4,
        viewport_transformation: &Matrix4,
        point: Cartesian3,
    ) -> Cartesian2 {
        let clip = model_view_projection.multiply_by_vector(Cartesian4::new(point.x, point.y, point.z, 1.0));
        let ndc = clip.multiply_by_scalar(1.0 / clip.w);
        Cartesian2::from_cartesian4(viewport_transformation.multiply_by_vector(ndc))
    }

    /// Rotation whose columns are velocity, right, and up for a body moving
    /// with `velocity` at `position`.
    pub fn rotation_matrix_from_position_velocity(
        position: Cartesian3,
        velocity: Cartesian3,
        ellipsoid: &Ellipsoid,
    ) -> Matrix3 {
        let normal =
            ellipsoid.geodetic_surface_normal(position).unwrap_or(Cartesian3::new(f64::NAN, f64::NAN, f64::NAN));
        let mut right = velocity.cross(normal);
        if right.equals_epsilon(Cartesian3::ZERO, EPSILON6) {
            right = Cartesian3::UNIT_X;
        }
        let up = right.cross(velocity).normalize();
        let right = velocity.cross(up).negate().normalize();
        Matrix3([velocity.x, velocity.y, velocity.z, right.x, right.y, right.z, up.x, up.y, up.z])
    }

    /// Re-expresses a model matrix in 2D (projected) coordinates.
    pub fn basis_to_2d(projection: &dyn MapProjection, matrix: &Matrix4) -> Matrix4 {
        let rtc_center = matrix.get_translation();
        let ellipsoid = projection.ellipsoid();
        let projected = if rtc_center == Cartesian3::ZERO {
            Cartesian3::ZERO
        } else {
            let c = ellipsoid.cartesian_to_cartographic(rtc_center).expect("translation is away from the center");
            let p = projection.project(&c);
            Cartesian3::new(p.z, p.x, p.y)
        };
        let from_enu = Self::east_north_up_to_fixed_frame(rtc_center, ellipsoid);
        let to_enu = from_enu.inverse_transformation();
        let local = to_enu.multiply_by_matrix3(&matrix.get_matrix3());
        Self::SWIZZLE_3D_TO_2D_MATRIX.multiply(&local).set_translation(projected)
    }

    /// Model matrix that places an ellipsoid-centered model at `center` in 2D.
    pub fn ellipsoid_to_2d_model_matrix(projection: &dyn MapProjection, center: Cartesian3) -> Matrix4 {
        let ellipsoid = projection.ellipsoid();
        let from_enu = Self::east_north_up_to_fixed_frame(center, ellipsoid);
        let to_enu = from_enu.inverse_transformation();
        let c = ellipsoid.cartesian_to_cartographic(center).expect("center is away from the ellipsoid's center");
        let p = projection.project(&c);
        let translation = Matrix4::from_translation(Cartesian3::new(p.z, p.x, p.y));
        translation.multiply(&Self::SWIZZLE_3D_TO_2D_MATRIX.multiply(&to_enu))
    }

    /// The rotation from TEME to the pseudo-fixed frame at a TAI date, using
    /// UTC in place of UT1 for Greenwich mean sidereal time.
    pub fn compute_teme_to_pseudo_fixed_matrix(date: &JulianDate) -> Matrix3 {
        // Not convert_tai_to_utc: this must not fail inside a leap second.
        let date_in_utc = date.add_seconds(-date.compute_tai_minus_utc());
        let utc_day_number = date_in_utc.day_number;
        let utc_seconds_into_day = date_in_utc.seconds_of_day;

        let diff_days = utc_day_number - 2451545.0;
        let t = if utc_seconds_into_day >= 43200.0 {
            (diff_days + 0.5) / DAYS_PER_JULIAN_CENTURY
        } else {
            (diff_days - 0.5) / DAYS_PER_JULIAN_CENTURY
        };

        let gmst0 = GMST_CONSTANT0 + t * (GMST_CONSTANT1 + t * (GMST_CONSTANT2 + t * GMST_CONSTANT3));
        let angle = (gmst0 * TWO_PI_OVER_SECONDS_IN_DAY) % TWO_PI;
        let ratio = WGS84_WR_PRECESSING + RATE_COEF * (utc_day_number - 2451545.5);
        let seconds_since_midnight = (utc_seconds_into_day + SECONDS_PER_DAY * 0.5) % SECONDS_PER_DAY;
        let gha = angle + ratio * seconds_since_midnight;
        let (sin_gha, cos_gha) = gha.sin_cos();
        Matrix3::new(cos_gha, sin_gha, 0.0, -sin_gha, cos_gha, 0.0, 0.0, 0.0, 1.0)
    }

    /// The XYS chunks needed for ICRF conversions over `time_interval` (TAI),
    /// recorded as requested on `xys`.
    pub fn preload_icrf_fixed<T>(time_interval: &TimeInterval<T>, xys: &Iau2006XysData) -> Vec<usize> {
        let start_day_tt = time_interval.start.day_number;
        let start_second_tt = time_interval.start.seconds_of_day + TT_MINUS_TAI;
        let stop_day_tt = time_interval.stop.day_number;
        let stop_second_tt = time_interval.stop.seconds_of_day + TT_MINUS_TAI;
        xys.preload(start_day_tt, start_second_tt, stop_day_tt, stop_second_tt)
    }

    /// ICRF to Earth-fixed at a TAI date; `None` while XYS data is missing.
    pub fn compute_icrf_to_fixed_matrix(
        date: &JulianDate,
        eop: &EarthOrientationParameters,
        xys: &Iau2006XysData,
    ) -> Option<Matrix3> {
        Self::compute_fixed_to_icrf_matrix(date, eop, xys).map(|m| m.transpose())
    }

    /// ICRF to Earth-fixed if the data is available, otherwise TEME to pseudo-fixed.
    pub fn compute_icrf_to_central_body_fixed_matrix(
        date: &JulianDate,
        eop: &EarthOrientationParameters,
        xys: &Iau2006XysData,
    ) -> Matrix3 {
        Self::compute_icrf_to_fixed_matrix(date, eop, xys)
            .unwrap_or_else(|| Self::compute_teme_to_pseudo_fixed_matrix(date))
    }

    /// The Moon's fixed frame to ICRF at a TAI date, from the approximate
    /// rotation of Eckhardt (1981).
    pub fn compute_moon_fixed_to_icrf_matrix(date: &JulianDate) -> Matrix3 {
        let seconds_tt = date.add_seconds(TT_MINUS_TAI);
        let d = seconds_tt.total_days() - J2000_DAYS;

        let e1 = to_radians(12.112) - to_radians(0.052992) * d;
        let e2 = to_radians(24.224) - to_radians(0.105984) * d;
        let e3 = to_radians(227.645) + to_radians(13.012) * d;
        let e4 = to_radians(261.105) + to_radians(13.340716) * d;
        let e5 = to_radians(358.0) + to_radians(0.9856) * d;

        let pitch = to_radians(270.0 - 90.0) - to_radians(3.878) * e1.sin() - to_radians(0.12) * e2.sin()
            + to_radians(0.07) * e3.sin()
            - to_radians(0.017) * e4.sin();
        let roll = to_radians(66.53 - 90.0) + to_radians(1.543) * e1.cos() + to_radians(0.24) * e2.cos()
            - to_radians(0.028) * e3.cos()
            + to_radians(0.007) * e4.cos();
        let heading = to_radians(244.375 - 90.0)
            + to_radians(13.17635831) * d
            + to_radians(3.558) * e1.sin()
            + to_radians(0.121) * e2.sin()
            - to_radians(0.064) * e3.sin()
            + to_radians(0.016) * e4.sin()
            + to_radians(0.025) * e5.sin();
        Matrix3::from_heading_pitch_roll(&HeadingPitchRoll::new(heading, pitch, roll))
    }

    pub fn compute_icrf_to_moon_fixed_matrix(date: &JulianDate) -> Matrix3 {
        Self::compute_moon_fixed_to_icrf_matrix(date).transpose()
    }

    /// Earth-fixed to ICRF at a TAI date (IAU 2006 CIO-based); `None` while
    /// XYS data is missing.
    pub fn compute_fixed_to_icrf_matrix(
        date: &JulianDate,
        eop: &EarthOrientationParameters,
        xys: &Iau2006XysData,
    ) -> Option<Matrix3> {
        let eop = eop.compute(date);

        // TT from TAI by offset. secondTT may pass 86400, which Cesium accepts.
        let day_tt = date.day_number;
        let second_tt = date.seconds_of_day + TT_MINUS_TAI;
        let xys = xys.compute_xys_radians(day_tt, second_tt)?;

        let x = xys.x + eop.x_pole_offset;
        let y = xys.y + eop.y_pole_offset;

        // XYS rotation.
        let a = 1.0 / (1.0 + (1.0 - x * x - y * y).sqrt());
        let rotation1 = Matrix3::from_column_major_array([
            1.0 - a * x * x,
            -a * x * y,
            -x,
            -a * x * y,
            1.0 - a * y * y,
            -y,
            x,
            y,
            1.0 - a * (x * x + y * y),
        ]);
        let rotation2 = Matrix3::from_rotation_z(-xys.s);
        let matrix_q = rotation1.multiply(&rotation2);

        let date_ut1_day = date.day_number;
        let date_ut1_sec = date.seconds_of_day - date.compute_tai_minus_utc() + eop.ut1_minus_utc;

        // Earth rotation angle, dropping whole revolutions for precision:
        // era = a + fractionOfDay + b (JulianDayNumber - 2451545 + fractionOfDay).
        let days_since_j2000 = date_ut1_day - 2451545.0;
        let fraction_of_day = date_ut1_sec / SECONDS_PER_DAY;
        let mut era = 0.779057273264 + fraction_of_day + 0.00273781191135448 * (days_since_j2000 + fraction_of_day);
        era = (era % 1.0) * TWO_PI;

        let earth_rotation = Matrix3::from_rotation_z(era);
        let pf_to_icrf = matrix_q.multiply(&earth_rotation);

        // Polar motion.
        let (sinxp, cosxp) = eop.x_pole_wander.sin_cos();
        let (sinyp, cosyp) = eop.y_pole_wander.sin_cos();

        let mut ttt = day_tt - J2000_TT_DAYS + second_tt / SECONDS_PER_DAY;
        ttt /= 36525.0;

        // Approximate s' in radians.
        let sp = (-47.0e-6 * ttt * RADIANS_PER_DEGREE) / 3600.0;
        let (sinsp, cossp) = sp.sin_cos();

        let f_to_pf_mtx = Matrix3::from_column_major_array([
            cosxp * cossp,
            cosxp * sinsp,
            sinxp,
            -cosyp * sinsp + sinyp * sinxp * cossp,
            cosyp * cossp + sinyp * sinxp * sinsp,
            -sinyp * cosxp,
            -sinyp * sinsp - cosyp * sinxp * cossp,
            sinyp * cossp - cosyp * sinxp * sinsp,
            cosyp * cosxp,
        ]);
        Some(pf_to_icrf.multiply(&f_to_pf_mtx))
    }
}

const GMST_CONSTANT0: f64 = 6.0 * 3600.0 + 41.0 * 60.0 + 50.54841;
const GMST_CONSTANT1: f64 = 8640184.812866;
const GMST_CONSTANT2: f64 = 0.093104;
const GMST_CONSTANT3: f64 = -6.2e-6;
const RATE_COEF: f64 = 1.1772758384668e-19;
const WGS84_WR_PRECESSING: f64 = 7.2921158553e-5;
const TWO_PI_OVER_SECONDS_IN_DAY: f64 = TWO_PI / 86400.0;
const TT_MINUS_TAI: f64 = 32.184;
const J2000_TT_DAYS: f64 = 2451545.0;
const J2000_DAYS: f64 = 2451545.0;
