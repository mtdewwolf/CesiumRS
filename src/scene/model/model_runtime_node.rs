//! CesiumJS `Scene/Model/ModelRuntimeNode`: one node of a loaded model's
//! runtime scene graph.
//!
//! Cesium keeps back-references from a runtime node to its `ModelSceneGraph`.
//! The Rust port stores the node's index instead, so the scene graph can hand
//! out `&mut` access without reference cycles. Methods that need the graph
//! (`get_child`, articulation registration) take it as a parameter.

use super::{
    instancing_pipeline_stage::RuntimeNodeInstancingState, model_runtime_primitive::ModelRuntimePrimitive,
    node_statistics_pipeline_stage::NodeRuntimeBuffers, ModelSkin,
};
use crate::{
    core::{Cartesian3, Matrix4, Quaternion, RuntimeError},
    renderer::Gl,
    scene::model_components as mc,
};

/// `ModelComponents.Node`'s translation, rotation and scale, kept so animations
/// and `ModelNode` can drive a node that was not defined with a matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransformParameters {
    pub translation: Cartesian3,
    pub rotation: Quaternion,
    pub scale: Cartesian3,
}

/// Node-level pipeline stages, in the order `configure_pipeline` pushes them.
///
/// The stages themselves are executed by the render lane; a runtime node only
/// records the sequence Cesium would use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodePipelineStage {
    Instancing,
    NodeStatistics,
}

/// Node-level update stages. Cesium currently pushes only
/// `ModelMatrixUpdateStage`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeUpdateStage {
    ModelMatrix,
}

/// An in-memory representation of a node as part of a [`ModelSceneGraph`].
///
/// `D` is the render lane's draw command type. It defaults to `()`, so the
/// runtime can be built and validated without a renderer.
///
/// [`ModelSceneGraph`]: super::ModelSceneGraph
pub struct ModelRuntimeNode<G: Gl, D = ()> {
    node_index: usize,
    name: Option<String>,
    id: Option<usize>,
    children: Vec<usize>,
    original_transform: Matrix4,
    transform: Matrix4,
    transform_to_root: Matrix4,
    computed_transform: Matrix4,
    pub(crate) transform_dirty: bool,
    transform_parameters: Option<TransformParameters>,
    morph_weights: Vec<f64>,
    runtime_skin: Option<usize>,
    computed_joint_matrices: Vec<Matrix4>,

    /// Whether to show this node and its children.
    pub show: bool,
    /// Whether the user supplies this node's transform, which suppresses the
    /// asset's animations for the node.
    pub user_animated: bool,
    /// Pipeline stages to apply across all the mesh primitives of this node.
    pub pipeline_stages: Vec<NodePipelineStage>,
    /// The mesh primitives that belong to this node.
    pub runtime_primitives: Vec<ModelRuntimePrimitive<G, D>>,
    /// Update stages to apply to this node.
    pub update_stages: Vec<NodeUpdateStage>,

    /// Instancing state written by `InstancingPipelineStage`.
    pub instancing: RuntimeNodeInstancingState<G>,
    /// Generated buffers counted by `NodeStatisticsPipelineStage`.
    pub statistics_buffers: NodeRuntimeBuffers<G>,
}

impl<G: Gl, D> ModelRuntimeNode<G, D> {
    /// Creates the runtime node and its initial computed transform.
    ///
    /// `transform` excludes ancestor transforms, `transform_to_root` is the
    /// product of all ancestors' transforms, and `children` are indices into
    /// the scene graph's runtime node array.
    pub fn new(node: &mc::Node<G>, transform: Matrix4, transform_to_root: Matrix4, children: Vec<usize>) -> Self {
        let mut runtime_node = Self {
            node_index: node.index.unwrap_or(0),
            name: node.name.clone(),
            id: node.index,
            children,
            original_transform: transform,
            transform,
            transform_to_root,
            computed_transform: Matrix4::IDENTITY,
            transform_dirty: false,
            transform_parameters: None,
            morph_weights: Vec::new(),
            runtime_skin: None,
            computed_joint_matrices: Vec::new(),
            show: true,
            user_animated: false,
            pipeline_stages: Vec::new(),
            runtime_primitives: Vec::new(),
            update_stages: Vec::new(),
            instancing: RuntimeNodeInstancingState::default(),
            statistics_buffers: NodeRuntimeBuffers {
                instancing_transforms_buffer: None,
                instancing_transforms_buffer_2d: None,
                instancing_translation_buffer_2d: None,
            },
        };
        runtime_node.initialize(node);
        runtime_node
    }

    fn initialize(&mut self, node: &mc::Node<G>) {
        self.computed_transform = self.transform_to_root.multiply_transformation(&self.transform);
        if node.matrix.is_none() {
            self.transform_parameters = Some(TransformParameters {
                translation: node.translation.unwrap_or(Cartesian3::ZERO),
                rotation: node.rotation.unwrap_or(Quaternion::IDENTITY),
                scale: node.scale.unwrap_or(Cartesian3::ONE),
            });
        }
        if !node.morph_weights.is_empty() {
            self.morph_weights = node.morph_weights.clone();
        }
    }

    /// The index of the corresponding node in [`ModelComponents::nodes`].
    ///
    /// [`ModelComponents::nodes`]: crate::scene::model_components::Components
    pub fn node_index(&self) -> usize {
        self.node_index
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The index of the node in the glTF.
    pub fn id(&self) -> Option<usize> {
        self.id
    }

    /// The indices of the children of this node in the scene graph.
    pub fn children(&self) -> &[usize] {
        &self.children
    }

    /// The node's local space transform.
    pub fn transform(&self) -> &Matrix4 {
        &self.transform
    }

    /// Replaces the node's local space transform and marks the subtree dirty.
    pub fn set_transform(&mut self, value: Matrix4) {
        self.transform_dirty = true;
        self.transform = value;
    }

    /// The transforms of all the node's ancestors, not including this node's
    /// transform.
    pub fn transform_to_root(&self) -> &Matrix4 {
        &self.transform_to_root
    }

    /// `transformToRoot * transform`: local space to model scene-graph space.
    pub fn computed_transform(&self) -> &Matrix4 {
        &self.computed_transform
    }

    /// The node's original transform, without ancestors or articulations.
    pub fn original_transform(&self) -> &Matrix4 {
        &self.original_transform
    }

    /// Whether an articulation or a user assignment changed the transform.
    pub fn is_transform_dirty(&self) -> bool {
        self.transform_dirty
    }

    /// Marks this node's subtree as needing a transform update, as assigning
    /// `ModelNode#matrix` and `updateModelMatrix` do.
    pub fn mark_transform_dirty(&mut self) {
        self.transform_dirty = true;
    }

    /// Called by `ModelMatrixUpdateStage` once the node's draw commands were
    /// updated.
    pub fn mark_transform_clean(&mut self) {
        self.transform_dirty = false;
    }

    /// Stores the transform product of this node's ancestors, including the
    /// node's own transform, while the update stage descends.
    pub fn set_transform_to_root(&mut self, transform_to_root: Matrix4) {
        self.transform_to_root = transform_to_root;
    }

    /// The node's local translation, or `None` when the node was defined with a
    /// matrix.
    pub fn translation(&self) -> Option<Cartesian3> {
        self.transform_parameters.map(|p| p.translation)
    }

    /// Cesium throws `DeveloperError` when the node was defined with a matrix.
    pub fn set_translation(&mut self, value: Cartesian3) -> Result<(), RuntimeError> {
        let Some(mut parameters) = self.transform_parameters else {
            return Err(RuntimeError::new(
                "The translation of a node cannot be set if it was defined using a matrix in the model.",
            ));
        };
        if parameters.translation == value {
            return Ok(());
        }
        parameters.translation = value;
        self.transform_parameters = Some(parameters);
        self.update_transform_from_parameters(parameters);
        Ok(())
    }

    /// The node's local rotation, or `None` when the node was defined with a
    /// matrix.
    pub fn rotation(&self) -> Option<Quaternion> {
        self.transform_parameters.map(|p| p.rotation)
    }

    /// Cesium throws `DeveloperError` when the node was defined with a matrix.
    pub fn set_rotation(&mut self, value: Quaternion) -> Result<(), RuntimeError> {
        let Some(mut parameters) = self.transform_parameters else {
            return Err(RuntimeError::new(
                "The rotation of a node cannot be set if it was defined using a matrix in the model.",
            ));
        };
        if parameters.rotation.equals(&value) {
            return Ok(());
        }
        parameters.rotation = value;
        self.transform_parameters = Some(parameters);
        self.update_transform_from_parameters(parameters);
        Ok(())
    }

    /// The node's local scale, or `None` when the node was defined with a
    /// matrix.
    pub fn scale(&self) -> Option<Cartesian3> {
        self.transform_parameters.map(|p| p.scale)
    }

    /// Cesium throws `DeveloperError` when the node was defined with a matrix.
    pub fn set_scale(&mut self, value: Cartesian3) -> Result<(), RuntimeError> {
        let Some(mut parameters) = self.transform_parameters else {
            return Err(RuntimeError::new(
                "The scale of a node cannot be set if it was defined using a matrix in the model.",
            ));
        };
        if parameters.scale == value {
            return Ok(());
        }
        parameters.scale = value;
        self.transform_parameters = Some(parameters);
        self.update_transform_from_parameters(parameters);
        Ok(())
    }

    /// The node's morph weights, in glTF target order.
    pub fn morph_weights(&self) -> &[f64] {
        &self.morph_weights
    }

    /// Cesium requires the same length as the original weights array.
    pub fn set_morph_weights(&mut self, value: &[f64]) -> Result<(), RuntimeError> {
        if self.morph_weights.len() != value.len() {
            return Err(RuntimeError::new("value must have the same length as the original weights array."));
        }
        self.morph_weights.copy_from_slice(value);
        Ok(())
    }

    fn update_transform_from_parameters(&mut self, parameters: TransformParameters) {
        self.transform_dirty = true;
        self.transform = Matrix4::from_translation_quaternion_rotation_scale(
            parameters.translation,
            &parameters.rotation,
            parameters.scale,
        );
    }

    /// The skin applied to this node, as an index into the scene graph's skins.
    pub fn runtime_skin(&self) -> Option<usize> {
        self.runtime_skin
    }

    pub(crate) fn set_runtime_skin(&mut self, runtime_skin: Option<usize>) {
        self.runtime_skin = runtime_skin;
    }

    /// The computed joint matrices of this node, derived from its skin.
    pub fn computed_joint_matrices(&self) -> &[Matrix4] {
        &self.computed_joint_matrices
    }

    /// Configures this node's pipeline stages. Call it again if the pipeline
    /// needs to be re-run.
    pub fn configure_pipeline(&mut self, node: &mc::Node<G>) {
        self.pipeline_stages.clear();
        self.update_stages.clear();
        if node.instances.is_some() {
            self.pipeline_stages.push(NodePipelineStage::Instancing);
        }
        self.pipeline_stages.push(NodePipelineStage::NodeStatistics);
        self.update_stages.push(NodeUpdateStage::ModelMatrix);
    }

    /// Recomputes `computedTransform` from `transformToRoot * transform`.
    pub fn update_computed_transform(&mut self) {
        self.computed_transform = self.transform_to_root.multiply_transformation(&self.transform);
    }

    /// Updates this node's joint matrices as
    /// `inverse(nodeWorld) * (jointWorld * inverseBindMatrix)`.
    ///
    /// `skin` must already have been updated for the current frame (see
    /// [`ModelSkin::update_joint_matrices`]).
    pub fn update_joint_matrices(&mut self, skin: &ModelSkin) {
        if self.runtime_skin.is_none() {
            return;
        }
        let skin_joint_matrices = skin.joint_matrices();
        self.computed_joint_matrices.clear();
        self.computed_joint_matrices.resize(skin_joint_matrices.len(), Matrix4::IDENTITY);

        let node_world_transform = self.transform_to_root.multiply_transformation(&self.transform);
        let inverse_node_world_transform = node_world_transform.inverse_transformation();
        for (i, joint_matrix) in skin_joint_matrices.iter().enumerate() {
            self.computed_joint_matrices[i] = inverse_node_world_transform.multiply_transformation(joint_matrix);
        }
    }

    /// The child runtime node with the given index.
    pub fn get_child<'a>(
        &self,
        scene_graph: &'a super::ModelSceneGraph<G, D>,
        index: usize,
    ) -> Result<&'a ModelRuntimeNode<G, D>, RuntimeError> {
        let child = *self.children.get(index).ok_or_else(|| {
            RuntimeError::new("index must be greater than or equal to 0 and less than the number of children.")
        })?;
        scene_graph.runtime_node(child).ok_or_else(|| RuntimeError::new("Missing child runtime node"))
    }
}

impl<G: Gl, D> std::fmt::Debug for ModelRuntimeNode<G, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelRuntimeNode")
            .field("node_index", &self.node_index)
            .field("name", &self.name)
            .field("children", &self.children)
            .field("show", &self.show)
            .field("user_animated", &self.user_animated)
            .finish_non_exhaustive()
    }
}
