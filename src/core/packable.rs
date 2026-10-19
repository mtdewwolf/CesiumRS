//! Cesium's packing interfaces as Rust traits over the existing numeric codecs.

pub trait Packable: Sized {
    const PACKED_LENGTH: usize;
    fn pack(&self, array: &mut [f64], starting_index: usize);
    fn unpack(array: &[f64], starting_index: usize) -> Self;
}

macro_rules! numeric_packable {
    ($($ty:ty),* $(,)?) => { $(
        impl Packable for $ty {
            const PACKED_LENGTH: usize = <$ty>::PACKED_LENGTH;
            fn pack(&self, array: &mut [f64], starting_index: usize) { <$ty>::pack(self, array, starting_index); }
            fn unpack(array: &[f64], starting_index: usize) -> Self { <$ty>::unpack(array, starting_index) }
        }
    )* };
}

numeric_packable!(
    super::Cartesian2,
    super::Cartesian3,
    super::Cartesian4,
    super::Matrix2,
    super::Matrix3,
    super::Matrix4,
    super::Quaternion,
    super::Color,
    super::BoundingRectangle,
    super::BoundingSphere,
    super::Rectangle,
    super::Ellipsoid,
    super::NearFarScalar,
    super::DistanceDisplayCondition,
    super::VertexFormat
);

pub trait PackableForInterpolation: Packable {
    const PACKED_INTERPOLATION_LENGTH: usize;
    fn convert_packed_array_for_interpolation(packed: &[f64], starting_index: usize, last_index: usize) -> Vec<f64>;
    fn unpack_interpolation_result(array: &[f64], source: &[f64], starting_index: usize, last_index: usize) -> Self;
}

impl PackableForInterpolation for super::Quaternion {
    const PACKED_INTERPOLATION_LENGTH: usize = Self::PACKED_INTERPOLATION_LENGTH;
    fn convert_packed_array_for_interpolation(packed: &[f64], starting_index: usize, last_index: usize) -> Vec<f64> {
        Self::convert_packed_array_for_interpolation(packed, starting_index, last_index)
    }
    fn unpack_interpolation_result(array: &[f64], source: &[f64], starting_index: usize, last_index: usize) -> Self {
        Self::unpack_interpolation_result(array, source, starting_index, last_index)
    }
}
