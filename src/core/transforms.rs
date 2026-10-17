//! CesiumJS `Transforms`: local frames, heading-pitch-roll frames, and
//! window-coordinate helpers. The time-dependent ICRF, TEME, and Moon frames
//! need `JulianDate` and are added with the time modules.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartesian4::Cartesian4;
use super::ellipsoid::Ellipsoid;
use super::heading_pitch_roll::HeadingPitchRoll;
use super::map_projection::MapProjection;
use super::math::{equals_epsilon, sign, EPSILON14, EPSILON6};
use super::matrix3::Matrix3;
use super::matrix4::Matrix4;
use super::quaternion::Quaternion;

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
}
