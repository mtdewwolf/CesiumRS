//! CesiumJS `Scene/Model/ModelArticulation`: an `AGI_articulations`
//! articulation that drives the transforms of a set of nodes.
//!
//! Stages compose in the order they appear in the model. Applying an
//! articulation overwrites the transforms of its nodes with
//! `originalTransform * articulationMatrix`.

use super::{ModelArticulationStage, ModelRuntimeNode};
use crate::{
    core::{Matrix4, RuntimeError},
    renderer::Gl,
    scene::model_components as mc,
};
use std::collections::BTreeMap;

/// An in-memory representation of an articulation.
pub struct ModelArticulation {
    name: Option<String>,
    runtime_stages: Vec<ModelArticulationStage>,
    runtime_stages_by_name: BTreeMap<String, usize>,
    /// Indices of the runtime nodes affected by this articulation.
    runtime_nodes: Vec<usize>,
    /// `true` until the first application, as in Cesium.
    dirty: bool,
}

impl ModelArticulation {
    pub fn new(articulation: &mc::Articulation) -> Result<Self, RuntimeError> {
        let mut runtime_stages = Vec::with_capacity(articulation.stages.len());
        let mut runtime_stages_by_name = BTreeMap::new();
        for stage in &articulation.stages {
            let runtime_stage = ModelArticulationStage::new(stage)?;
            if let Some(name) = runtime_stage.name() {
                runtime_stages_by_name.insert(name.to_string(), runtime_stages.len());
            }
            runtime_stages.push(runtime_stage);
        }
        Ok(Self {
            name: articulation.name.clone(),
            runtime_stages,
            runtime_stages_by_name,
            runtime_nodes: Vec::new(),
            dirty: true,
        })
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The runtime stages, in the order they appeared in the model.
    pub fn runtime_stages(&self) -> &[ModelArticulationStage] {
        &self.runtime_stages
    }

    /// The indices of the runtime nodes affected by this articulation.
    pub fn runtime_nodes(&self) -> &[usize] {
        &self.runtime_nodes
    }

    /// Whether applying this articulation would change node transforms.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn add_runtime_node(&mut self, node_index: usize) {
        self.runtime_nodes.push(node_index);
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Sets the current value of a stage by name. Unknown names are ignored, as
    /// Cesium only checks that the stage exists.
    pub fn set_articulation_stage(&mut self, stage_name: &str, value: f64) {
        let Some(&index) = self.runtime_stages_by_name.get(stage_name) else {
            return;
        };
        if self.runtime_stages[index].set_current_value(value) {
            self.dirty = true;
        }
    }

    /// Applies the chain of stages to each participating node's transform.
    ///
    /// Returns `false` when nothing was dirty, in which case the nodes were not
    /// touched.
    pub fn apply<G: Gl, D>(&mut self, runtime_nodes: &mut [Option<ModelRuntimeNode<G, D>>]) -> bool {
        if !self.dirty {
            return false;
        }
        self.dirty = false;

        let mut articulation_matrix = Matrix4::IDENTITY;
        for stage in &self.runtime_stages {
            articulation_matrix = stage.apply_stage_to_matrix(&articulation_matrix);
        }

        for &node_index in &self.runtime_nodes {
            if let Some(node) = runtime_nodes.get_mut(node_index).and_then(Option::as_mut) {
                let transform = node.original_transform().multiply_transformation(&articulation_matrix);
                node.set_transform(transform);
            }
        }
        true
    }
}

impl std::fmt::Debug for ModelArticulation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelArticulation")
            .field("name", &self.name)
            .field("runtime_stages", &self.runtime_stages.len())
            .field("runtime_nodes", &self.runtime_nodes)
            .field("dirty", &self.dirty)
            .finish()
    }
}
