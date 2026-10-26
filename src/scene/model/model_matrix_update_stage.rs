//! CesiumJS `Scene/Model/ModelMatrixUpdateStage`: propagates a dirty node's
//! transform to its children and to their draw commands.
//!
//! Cesium recurses through the dirty subtree. The port uses an explicit stack,
//! which produces the same transforms and the same set of updated draw
//! commands without risking recursion depth on deep hierarchies. It also skips
//! the stage entirely when the model is projected to 2D in a non-3D mode, as
//! Cesium does.
//!
//! Transform propagation and draw-command updates are separate calls so the
//! runtime can be exercised without a renderer: only the second call needs the
//! render lane's [`RuntimeDrawCommand`].

use super::{model_utility, ModelSceneGraph, RuntimeDrawCommand};
use crate::{
    core::Matrix4,
    renderer::Gl,
    scene::{FrameState, SceneMode},
};

/// Updates the model matrices and cull faces of draw commands under dirty
/// nodes.
pub struct ModelMatrixUpdateStage;

impl ModelMatrixUpdateStage {
    /// The model matrix the draw commands of this frame use.
    fn frame_model_matrix<G: Gl, D>(scene_graph: &ModelSceneGraph<G, D>, frame_state: &FrameState) -> Option<Matrix4> {
        let use_2d = frame_state.mode != SceneMode::Scene3D;
        if use_2d && scene_graph.project_to_2d() {
            // The stage does not run when the model is projected to 2D.
            return None;
        }
        Some(if use_2d { *scene_graph.computed_model_matrix_2d() } else { *scene_graph.computed_model_matrix() })
    }

    /// Propagates transforms through every dirty subtree and returns the
    /// indices of the nodes that were updated.
    ///
    /// Nothing is returned when the model is projected to 2D in a non-3D mode.
    pub fn update_transforms<G: Gl, D>(
        scene_graph: &mut ModelSceneGraph<G, D>,
        frame_state: &FrameState,
    ) -> Vec<usize> {
        if Self::frame_model_matrix(scene_graph, frame_state).is_none() {
            return Vec::new();
        }

        let mut updated = Vec::new();
        let mut stack = Vec::new();
        for &root in scene_graph.root_nodes() {
            let Some(node) = scene_graph.runtime_node(root) else {
                continue;
            };
            if node.is_transform_dirty() {
                stack.push(root);
            }
        }

        while let Some(node_index) = stack.pop() {
            let Some(node) = scene_graph.runtime_node_mut(node_index) else {
                continue;
            };
            if !node.is_transform_dirty() {
                continue;
            }
            node.update_computed_transform();
            let computed = *node.computed_transform();
            updated.push(node_index);

            let children = node.children().to_vec();
            node.mark_transform_clean();
            for child_index in children {
                let Some(child) = scene_graph.runtime_node_mut(child_index) else {
                    continue;
                };
                child.set_transform_to_root(computed);
                // Children inherit the dirty state from their parent, so their
                // draw commands are rebuilt even if they were clean before.
                child.mark_transform_dirty();
                stack.push(child_index);
            }
        }
        updated
    }

    /// Updates the model matrix and cull face of every draw command under the
    /// nodes returned by [`Self::update_transforms`].
    pub fn update_draw_commands<G: Gl, D: RuntimeDrawCommand>(
        scene_graph: &mut ModelSceneGraph<G, D>,
        frame_state: &FrameState,
        updated_nodes: &[usize],
    ) {
        let Some(model_matrix) = Self::frame_model_matrix(scene_graph, frame_state) else {
            return;
        };
        for &node_index in updated_nodes {
            let Some(node) = scene_graph.runtime_node_mut(node_index) else {
                continue;
            };
            let computed_transform = *node.computed_transform();
            for primitive in &mut node.runtime_primitives {
                update_draw_command(primitive.draw_command.as_mut(), &model_matrix, &computed_transform);
            }
        }
    }

    /// Runs the whole stage, as `ModelSceneGraph#update` does.
    pub fn update<G: Gl, D: RuntimeDrawCommand>(scene_graph: &mut ModelSceneGraph<G, D>, frame_state: &FrameState) {
        let updated_nodes = Self::update_transforms(scene_graph, frame_state);
        Self::update_draw_commands(scene_graph, frame_state, &updated_nodes);
    }
}

fn update_draw_command<D: RuntimeDrawCommand>(
    draw_command: Option<&mut D>,
    model_matrix: &Matrix4,
    transform_to_root: &Matrix4,
) {
    let Some(draw_command) = draw_command else {
        return;
    };
    let model_matrix = model_matrix.multiply_transformation(transform_to_root);
    draw_command.set_model_matrix(model_matrix);
    if let Some(primitive_type) = draw_command.primitive_type() {
        draw_command.set_cull_face(model_utility::get_cull_face(&model_matrix, primitive_type));
    }
}
