//! CesiumJS `Scene/Model/ModelSkin`: joint matrices for a skinned node.
//!
//! Skins are initialized after every [`ModelRuntimeNode`] exists, because the
//! joints are runtime-node indices.

use super::ModelRuntimeNode;
use crate::{core::Matrix4, renderer::Gl, scene::model_components as mc};

/// An in-memory representation of a skin that affects nodes in the scene graph.
pub struct ModelSkin {
    inverse_bind_matrices: Vec<Matrix4>,
    /// Node indices of the joints, in skin order.
    joints: Vec<usize>,
    /// `jointWorldTransform * inverseBindMatrix` per joint.
    joint_matrices: Vec<Matrix4>,
}

impl ModelSkin {
    /// Builds the runtime skin from the model components. Joints that are not
    /// reachable from the scene (and therefore have no runtime node) are
    /// dropped, as Cesium's undefined runtime-node lookup would fail instead.
    pub fn new<G: Gl, D>(skin: &mc::Skin, runtime_nodes: &[Option<ModelRuntimeNode<G, D>>]) -> Self {
        let mut joints = Vec::with_capacity(skin.joints.len());
        let mut joint_matrices = Vec::with_capacity(skin.joints.len());
        for (i, &joint_index) in skin.joints.iter().enumerate() {
            let Some(joint) = runtime_nodes.get(joint_index).and_then(Option::as_ref) else {
                continue;
            };
            let inverse_bind_matrix = skin.inverse_bind_matrices.get(i).copied().unwrap_or(Matrix4::IDENTITY);
            joints.push(joint_index);
            joint_matrices.push(compute_joint_matrix(joint, &inverse_bind_matrix));
        }
        Self { inverse_bind_matrices: skin.inverse_bind_matrices.clone(), joints, joint_matrices }
    }

    /// The inverse bind matrices of the skin.
    pub fn inverse_bind_matrices(&self) -> &[Matrix4] {
        &self.inverse_bind_matrices
    }

    /// The runtime node indices of the joints.
    pub fn joints(&self) -> &[usize] {
        &self.joints
    }

    /// The joint matrices, where each is `jointWorldTransform * inverseBindMatrix`.
    ///
    /// Each node that references this skin pre-multiplies its own inverse world
    /// transform; see [`ModelRuntimeNode::update_joint_matrices`].
    pub fn joint_matrices(&self) -> &[Matrix4] {
        &self.joint_matrices
    }

    /// Recomputes the joint matrices from the current node transforms.
    pub fn update_joint_matrices<G: Gl, D>(&mut self, runtime_nodes: &[Option<ModelRuntimeNode<G, D>>]) {
        for (i, &joint_index) in self.joints.iter().enumerate() {
            let Some(joint) = runtime_nodes.get(joint_index).and_then(Option::as_ref) else {
                continue;
            };
            let inverse_bind_matrix = self.inverse_bind_matrices.get(i).copied().unwrap_or(Matrix4::IDENTITY);
            self.joint_matrices[i] = compute_joint_matrix(joint, &inverse_bind_matrix);
        }
    }
}

fn compute_joint_matrix<G: Gl, D>(joint: &ModelRuntimeNode<G, D>, inverse_bind_matrix: &Matrix4) -> Matrix4 {
    let joint_world_transform = joint.transform_to_root().multiply_transformation(joint.transform());
    joint_world_transform.multiply_transformation(inverse_bind_matrix)
}

impl std::fmt::Debug for ModelSkin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelSkin")
            .field("joints", &self.joints)
            .field("joint_matrices", &self.joint_matrices.len())
            .finish_non_exhaustive()
    }
}
