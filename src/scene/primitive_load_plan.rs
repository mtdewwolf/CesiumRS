//! CesiumJS `PrimitiveLoadPlan`: whether each attribute and the indices of a
//! primitive end up as GPU buffers, typed arrays or both, and the geometry
//! post-processing (`CESIUM_primitive_outline`, Gaussian splats) that runs
//! once they are loaded.
//!
//! Plans name the attribute they describe instead of holding a reference to
//! it; [`PrimitiveLoadPlan::post_process`] takes the primitive.
use super::model::primitive_outline_generator::{index_datatype_of, PrimitiveOutlineGenerator};
use super::model_components::{Attribute, Primitive};
use super::AttributeType;
use crate::core::{ComponentDatatype, RuntimeError, TypedArray};
use crate::renderer::{Buffer, BufferData, BufferKind, BufferUsage, Context, Gl};

/// The attribute an [`AttributeLoadPlan`] describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlannedAttribute {
    /// `primitive.attributes[i]`.
    Attribute(usize),
    /// `primitive.morphTargets[target].attributes[attribute]`.
    MorphTarget { target: usize, attribute: usize },
    /// `primitive.outlineCoordinates`.
    OutlineCoordinates,
}

/// `PrimitiveLoadPlan.AttributeLoadPlan`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttributeLoadPlan {
    pub attribute: PlannedAttribute,
    pub load_buffer: bool,
    pub load_typed_array: bool,
}

impl AttributeLoadPlan {
    pub fn new(attribute: PlannedAttribute) -> Self {
        Self { attribute, load_buffer: false, load_typed_array: false }
    }
}

/// `PrimitiveLoadPlan.IndicesLoadPlan`, for `primitive.indices`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IndicesLoadPlan {
    pub load_buffer: bool,
    pub load_typed_array: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrimitiveLoadPlan {
    pub attribute_plans: Vec<AttributeLoadPlan>,
    pub indices_plan: Option<IndicesLoadPlan>,
    pub needs_outlines: bool,
    /// The `CESIUM_primitive_outline` edge indices.
    pub outline_indices: Option<Vec<f64>>,
    pub needs_gaussian_splats: bool,
}

fn missing(property: &str) -> RuntimeError {
    RuntimeError::new(format!("Cannot read properties of undefined (reading '{property}')"))
}

fn attribute_mut<G: Gl>(
    primitive: &mut Primitive<G>,
    planned: PlannedAttribute,
) -> Result<&mut Attribute<G>, RuntimeError> {
    match planned {
        PlannedAttribute::Attribute(i) => primitive.attributes.get_mut(i),
        PlannedAttribute::MorphTarget { target, attribute } => {
            primitive.morph_targets.get_mut(target).and_then(|t| t.attributes.get_mut(attribute))
        }
        PlannedAttribute::OutlineCoordinates => primitive.outline_coordinates.as_mut(),
    }
    .ok_or_else(|| RuntimeError::new("Load plan names a missing attribute"))
}

impl PrimitiveLoadPlan {
    /// Outlines modify the indices and attributes, add the outline coordinate
    /// attribute and create the buffers the plan asks for; Gaussian splats
    /// keep every attribute as a typed array.
    pub fn post_process<G: Gl>(
        &mut self,
        primitive: &mut Primitive<G>,
        context: &Context<G>,
    ) -> Result<(), RuntimeError> {
        if self.needs_outlines {
            self.generate_outlines(primitive)?;
            self.generate_buffers(primitive, context)?;
        }
        if self.needs_gaussian_splats {
            for plan in &mut self.attribute_plans {
                plan.load_buffer = false;
                plan.load_typed_array = true;
            }
        }
        Ok(())
    }

    fn generate_outlines<G: Gl>(&mut self, primitive: &mut Primitive<G>) -> Result<(), RuntimeError> {
        let vertex_count = primitive.attributes.first().ok_or_else(|| missing("count"))?.count;
        let indices = primitive.indices.as_mut().ok_or_else(|| missing("typedArray"))?;
        let triangles = indices.typed_array.clone().ok_or_else(|| missing("length"))?;
        let outline = self.outline_indices.as_deref().unwrap_or_default();
        let generator = PrimitiveOutlineGenerator::new(triangles, outline, vertex_count.unwrap_or(0));
        // Copies can widen the indices to 16 or 32 bits.
        let updated = generator.updated_triangle_indices().clone();
        indices.index_datatype = index_datatype_of(&updated);
        indices.typed_array = Some(updated);

        let coordinates = generator.outline_coordinates();
        primitive.outline_coordinates = Some(Attribute {
            name: Some("_OUTLINE_COORDINATES".into()),
            typed_array: Some(TypedArray::Float32(coordinates.to_vec())),
            component_datatype: Some(ComponentDatatype::Float),
            attribute_type: Some(AttributeType::Vec3),
            normalized: false,
            count: Some(coordinates.len() / 3),
            ..Attribute::default()
        });
        self.attribute_plans.push(AttributeLoadPlan {
            attribute: PlannedAttribute::OutlineCoordinates,
            load_buffer: true,
            load_typed_array: false,
        });
        // Every attribute gains the copied vertices, the new one included.
        for plan in &self.attribute_plans {
            let attribute = attribute_mut(primitive, plan.attribute)?;
            let values = attribute.typed_array.as_ref().ok_or_else(|| missing("length"))?;
            attribute.typed_array = Some(generator.update_attribute(values)?);
        }
        Ok(())
    }

    fn generate_buffers<G: Gl>(&self, primitive: &mut Primitive<G>, context: &Context<G>) -> Result<(), RuntimeError> {
        for plan in &self.attribute_plans {
            let attribute = attribute_mut(primitive, plan.attribute)?;
            if plan.load_buffer {
                let bytes = attribute.typed_array.as_ref().map(TypedArray::to_le_bytes);
                let bytes = bytes.ok_or_else(|| RuntimeError::new("Must specify either sizeInBytes or typedArray."))?;
                attribute.buffer =
                    Some(Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?);
            }
            if !plan.load_typed_array {
                attribute.typed_array = None;
            }
        }
        if let (Some(plan), Some(indices)) = (self.indices_plan, primitive.indices.as_mut()) {
            if plan.load_buffer {
                let datatype = indices.index_datatype.ok_or_else(|| RuntimeError::new("Invalid index datatype"))?;
                let bytes = indices.typed_array.as_ref().map(TypedArray::to_le_bytes).unwrap_or_default();
                indices.buffer = Some(Buffer::new(
                    context,
                    BufferKind::Index(datatype),
                    BufferUsage::StaticDraw,
                    BufferData::Bytes(&bytes),
                )?);
            }
            if !plan.load_typed_array {
                indices.typed_array = None;
            }
        }
        Ok(())
    }
}
