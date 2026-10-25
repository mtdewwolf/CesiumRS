//! CesiumJS `ClassificationPipelineStage`: splits a primitive into batches by
//! feature ID so a `ClassificationModelDrawCommand` can render each batch with
//! the classification of the asset it overlaps.
use super::{model_utility, PrimitiveRenderResources};
use crate::{
    core::RuntimeError,
    renderer::{Gl, ShaderDestination},
    scene::{model_components as mc, VertexAttributeSemantic},
};

/// Add the classification define and compute batch lengths and offsets.
pub fn process<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
) -> Result<(), RuntimeError> {
    resources.shader_builder.add_define("HAS_CLASSIFICATION", None, ShaderDestination::BOTH);
    if resources.batch_lengths.is_none() {
        create_classification_batches(resources, primitive)?;
    }
    Ok(())
}

fn create_classification_batches<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
) -> Result<(), RuntimeError> {
    let position_attribute = model_utility::get_attribute_by_semantic(
        &primitive.attributes,
        mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position),
        None,
    )
    .ok_or_else(|| RuntimeError::new("Primitives must have a position attribute to be used for classification."))?;

    let indices = primitive.indices.as_ref();
    let count = indices
        .and_then(|i| i.count)
        .or(position_attribute.count)
        .ok_or_else(|| RuntimeError::new("Missing primitive count"))?;
    let indices_array = indices
        .map(|i| i.typed_array.as_ref().ok_or_else(|| RuntimeError::new("Missing index typed array")))
        .transpose()?;

    let feature_id_attribute = model_utility::get_attribute_by_semantic(
        &primitive.attributes,
        mc::AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId),
        Some(0),
    );

    // If there are no feature IDs, render the primitive in a single batch.
    let Some(feature_id_attribute) = feature_id_attribute else {
        resources.batch_lengths = Some(vec![count]);
        resources.batch_offsets = Some(vec![0]);
        return Ok(());
    };

    let feature_ids =
        feature_id_attribute.typed_array.as_ref().ok_or_else(|| RuntimeError::new("Missing feature ID typed array"))?;

    let mut batch_lengths = Vec::new();
    let mut batch_offsets = vec![0];
    let first_index = match indices_array {
        Some(indices) => indices.get(0).ok_or_else(|| RuntimeError::new("Missing first index"))?,
        None => 0.,
    };
    let first_index = first_index as usize;
    let mut current_batch_id =
        feature_ids.get(first_index).ok_or_else(|| RuntimeError::new("Feature ID index out of range"))?;
    let mut current_offset = 0;

    for i in 1..count {
        let index = match indices_array {
            Some(indices) => indices.get(i).ok_or_else(|| RuntimeError::new("Index out of range"))? as usize,
            None => i,
        };
        let batch_id = feature_ids.get(index).ok_or_else(|| RuntimeError::new("Feature ID index out of range"))?;

        if batch_id != current_batch_id {
            // Store the length of this batch and begin counting the next one.
            batch_lengths.push(i - current_offset);
            batch_offsets.push(i);
            current_offset = i;
            current_batch_id = batch_id;
        }
    }

    batch_lengths.push(count - current_offset);
    resources.batch_lengths = Some(batch_lengths);
    resources.batch_offsets = Some(batch_offsets);
    Ok(())
}
