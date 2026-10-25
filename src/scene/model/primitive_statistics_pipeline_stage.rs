//! CesiumJS `Scene/Model/PrimitiveStatisticsPipelineStage`: updates `ModelStatistics`
//! for a primitive's geometry, 2D positions, morph targets, material textures,
//! feature ID textures, and binary metadata.

use super::model_statistics::BatchTextureStatistics;
use super::{ModelStatistics, PrimitiveRenderResources};
use crate::core::PrimitiveType;
use crate::renderer::Gl;
use crate::scene::{model_components as mc, StructuralMetadata};

pub fn process<G: Gl>(
    statistics: &mut ModelStatistics,
    resources: &PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    structural_metadata: Option<&StructuralMetadata<G>>,
    feature_tables: &[Option<BatchTextureStatistics>],
) {
    count_geometry(statistics, primitive);
    count_2d_positions(statistics, resources);
    count_morph_target_attributes(statistics, primitive);
    count_material_textures(statistics, primitive.material.as_ref());
    count_feature_id_textures(statistics, &primitive.feature_ids);
    count_binary_metadata(statistics, structural_metadata, feature_tables);
}

pub fn count_geometry<G: Gl>(statistics: &mut ModelStatistics, primitive: &mc::Primitive<G>) {
    let count = primitive
        .indices
        .as_ref()
        .and_then(|i| i.count)
        .or_else(|| {
            primitive
                .attributes
                .iter()
                .find(|a| {
                    a.semantic == Some(mc::AttributeSemantic::Vertex(crate::scene::VertexAttributeSemantic::Position))
                })
                .and_then(|a| a.count)
        })
        .unwrap_or(0);
    match primitive.primitive_type {
        Some(PrimitiveType::Points) => {
            statistics.points_length += count;
        }
        Some(PrimitiveType::Triangles) => {
            statistics.triangles_length += count / 3;
        }
        Some(PrimitiveType::TriangleStrip | PrimitiveType::TriangleFan) => {
            statistics.triangles_length += count.saturating_sub(2);
        }
        _ => {}
    }

    for attribute in &primitive.attributes {
        if let Some(buffer) = &attribute.buffer {
            let has_cpu_copy = attribute.typed_array.is_some();
            statistics.add_buffer(buffer, has_cpu_copy);
        }
    }

    if let Some(outline_coordinates) = &primitive.outline_coordinates {
        if let Some(buffer) = &outline_coordinates.buffer {
            statistics.add_buffer(buffer, false);
        }
    }

    if let Some(indices) = &primitive.indices {
        if let Some(buffer) = &indices.buffer {
            let has_cpu_copy = indices.typed_array.is_some();
            statistics.add_buffer(buffer, has_cpu_copy);
        }
    }
}

pub fn count_2d_positions<G: Gl>(statistics: &mut ModelStatistics, resources: &PrimitiveRenderResources<G>) {
    if let Some(buffer) = &resources.position_buffer_2d {
        let has_cpu_copy = true;
        statistics.add_buffer(buffer, has_cpu_copy);
    }
}

pub fn count_morph_target_attributes<G: Gl>(statistics: &mut ModelStatistics, primitive: &mc::Primitive<G>) {
    let has_cpu_copy = false;
    for morph_target in &primitive.morph_targets {
        for attribute in &morph_target.attributes {
            if let Some(buffer) = &attribute.buffer {
                statistics.add_buffer(buffer, has_cpu_copy);
            }
        }
    }
}

pub fn count_material_textures<G: Gl>(statistics: &mut ModelStatistics, material: Option<&mc::Material<G>>) {
    let Some(material) = material else {
        return;
    };
    let metallic_roughness = &material.metallic_roughness;
    let specular_glossiness = material.specular_glossiness.as_ref();

    let mut texture_readers = vec![
        material.emissive_texture.as_ref(),
        material.normal_texture.as_ref(),
        material.occlusion_texture.as_ref(),
        metallic_roughness.base_color_texture.as_ref(),
        metallic_roughness.metallic_roughness_texture.as_ref(),
    ];

    if let Some(sg) = specular_glossiness {
        texture_readers.push(sg.diffuse_texture.as_ref());
        texture_readers.push(sg.specular_glossiness_texture.as_ref());
    }

    for reader in texture_readers.into_iter().flatten() {
        if let Some(texture) = &reader.texture {
            statistics.add_texture(texture);
        }
    }
}

pub fn count_feature_id_textures<G: Gl>(statistics: &mut ModelStatistics, feature_ids: &[mc::FeatureId<G>]) {
    for feature_id in feature_ids {
        if let mc::FeatureId::Texture(tex) = feature_id {
            if let Some(reader) = &tex.texture_reader {
                if let Some(texture) = &reader.texture {
                    statistics.add_texture(texture);
                }
            }
        }
    }
}

pub fn count_binary_metadata<G: Gl>(
    statistics: &mut ModelStatistics,
    structural_metadata: Option<&StructuralMetadata<G>>,
    feature_tables: &[Option<BatchTextureStatistics>],
) {
    if let Some(structural_metadata) = structural_metadata {
        for property_texture in structural_metadata.property_textures() {
            for (_, property) in property_texture.properties().iter() {
                if let Some(texture) = &property.texture_reader().texture {
                    statistics.add_texture(texture);
                }
            }
        }
        statistics.property_tables_byte_length = structural_metadata.property_tables_byte_length();
    }

    for batch_texture in feature_tables.iter().flatten() {
        statistics.add_batch_texture(batch_texture);
    }
}
