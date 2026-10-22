//! CesiumRS addition: C++ Draco's `Decoder::SetSkipAttributeTransform`.
//!
//! An attribute whose type is skipped keeps the portable values its sequential
//! or KD-tree decoder produced instead of being transformed back to its
//! original format. Quantized and octahedral attributes keep their transform
//! parameters in `attribute_transform_data`, exactly as upstream copies the
//! portable attribute over the decoded one.

use crate::geometry_attribute::GeometryAttributeType;

/// The attribute types whose transforms a decode skips.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SkipAttributeTransforms(u8);

impl SkipAttributeTransforms {
    /// Skips the transform of every attribute of `attribute_type`.
    pub fn insert(&mut self, attribute_type: GeometryAttributeType) {
        if let Some(bit) = Self::bit(attribute_type) {
            self.0 |= bit;
        }
    }

    /// Whether attributes of `attribute_type` keep their portable values.
    pub fn contains(self, attribute_type: GeometryAttributeType) -> bool {
        Self::bit(attribute_type).is_some_and(|bit| self.0 & bit != 0)
    }

    /// Whether no transform is skipped.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn bit(attribute_type: GeometryAttributeType) -> Option<u8> {
        match attribute_type {
            GeometryAttributeType::Invalid => None,
            other => Some(1 << other as i32),
        }
    }
}

#[cfg(any(feature = "point_cloud_decode", feature = "decoder"))]
mod helpers {
    use crate::attribute_octahedron_transform::AttributeOctahedronTransform;
    use crate::attribute_quantization_transform::AttributeQuantizationTransform;
    use crate::attribute_transform::AttributeTransform;
    use crate::attribute_transform_data::AttributeTransformData;
    use crate::draco_types::DataType;
    use crate::geometry_attribute::PointAttribute;
    use crate::status::DracoError;

    /// Upstream's `PreparePortableAttribute`: `Int32` values with the decoded
    /// attribute's component count, identity-mapped, one per decoded point.
    pub(crate) fn integer_portable(attribute: &PointAttribute, num_values: usize) -> Result<PointAttribute, DracoError> {
        let mut portable = PointAttribute::new();
        portable.try_init(attribute.attribute_type(), attribute.num_components(), DataType::Int32, false, num_values)?;
        portable.set_unique_id(attribute.unique_id());
        Ok(portable)
    }

    /// Keeps a sequential decoder's quantized `Int32` values and its
    /// quantization parameters.
    pub(crate) fn keep_quantized(
        dst: &mut PointAttribute,
        portable: &PointAttribute,
        transform: &AttributeQuantizationTransform,
    ) -> Result<(), DracoError> {
        let mut data = AttributeTransformData::new();
        transform.copy_to_attribute_transform_data(&mut data);
        dst.keep_portable(portable, DataType::Int32, Some(data))
    }

    /// Keeps a sequential decoder's two `Int32` octahedral coordinates and the
    /// octahedral quantization bits.
    pub(crate) fn keep_octahedral(
        dst: &mut PointAttribute,
        portable: &PointAttribute,
        transform: &AttributeOctahedronTransform,
    ) -> Result<(), DracoError> {
        let mut data = AttributeTransformData::new();
        transform.copy_to_attribute_transform_data(&mut data);
        dst.keep_portable(portable, DataType::Int32, Some(data))
    }
}
#[cfg(any(feature = "point_cloud_decode", feature = "decoder"))]
pub(crate) use helpers::*;
