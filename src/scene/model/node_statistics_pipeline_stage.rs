//! CesiumJS `Scene/Model/NodeStatisticsPipelineStage`: updates `ModelStatistics`
//! for a node's instancing attributes and generated 2D/transform buffers.

use super::ModelStatistics;
use crate::renderer::{Buffer, Gl};
use crate::scene::model_components as mc;

/// Runtime GPU buffers generated for a node during instancing / 2D projection.
#[derive(Clone, Default)]
pub struct NodeRuntimeBuffers<G: Gl> {
    pub instancing_transforms_buffer: Option<Buffer<G>>,
    pub instancing_transforms_buffer_2d: Option<Buffer<G>>,
    pub instancing_translation_buffer_2d: Option<Buffer<G>>,
}

pub fn process<G: Gl>(statistics: &mut ModelStatistics, node: &mc::Node<G>, runtime_node: &NodeRuntimeBuffers<G>) {
    count_instancing_attributes(statistics, node.instances.as_ref());
    count_generated_buffers(statistics, runtime_node);
}

pub fn count_instancing_attributes<G: Gl>(statistics: &mut ModelStatistics, instances: Option<&mc::Instances<G>>) {
    let Some(instances) = instances else {
        return;
    };
    for attribute in &instances.attributes {
        if let Some(buffer) = &attribute.buffer {
            // Optimal usage for instancing attributes is to load them on GPU only.
            // Those with CPU copies will not have buffers created here; an
            // overarching buffer will be created in InstancingPipelineStage.
            let has_cpu_copy = false;
            statistics.add_buffer(buffer, has_cpu_copy);
        }
    }
}

pub fn count_generated_buffers<G: Gl>(statistics: &mut ModelStatistics, runtime_node: &NodeRuntimeBuffers<G>) {
    let has_cpu_copy = false;
    if let Some(buffer) = &runtime_node.instancing_transforms_buffer {
        statistics.add_buffer(buffer, has_cpu_copy);
    }
    if let Some(buffer) = &runtime_node.instancing_transforms_buffer_2d {
        statistics.add_buffer(buffer, has_cpu_copy);
    }
    if let Some(buffer) = &runtime_node.instancing_translation_buffer_2d {
        statistics.add_buffer(buffer, has_cpu_copy);
    }
}
