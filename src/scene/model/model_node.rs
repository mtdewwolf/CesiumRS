//! CesiumJS `Scene/Model/ModelNode`: the user-facing handle on a runtime node.
//!
//! Assigning `matrix` drives a node from outside the asset's animations. The
//! Rust handle stores the runtime node index and takes the scene graph as a
//! parameter, so a `ModelNode` never outlives or aliases the graph.

use super::ModelSceneGraph;
use crate::{core::Matrix4, renderer::Gl};

/// A model node with a modifiable transform for user-defined animation.
///
/// Obtain one with [`ModelSceneGraph::get_node`]; do not construct it directly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelNode {
    runtime_node: usize,
    name: Option<String>,
    id: Option<usize>,
}

impl ModelNode {
    pub(crate) fn new(runtime_node: usize, name: Option<&str>, id: Option<usize>) -> Self {
        Self { runtime_node, name: name.map(str::to_string), id }
    }

    /// The index of the node in the scene graph's runtime node array.
    pub fn runtime_node(&self) -> usize {
        self.runtime_node
    }

    /// The value of the `name` property of this node.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The index of the node in the glTF.
    pub fn id(&self) -> Option<usize> {
        self.id
    }

    /// Determines if this node and its children will be shown.
    pub fn show<G: Gl, D>(&self, scene_graph: &ModelSceneGraph<G, D>) -> bool {
        scene_graph.runtime_node(self.runtime_node).is_some_and(|node| node.show)
    }

    pub fn set_show<G: Gl, D>(&self, scene_graph: &mut ModelSceneGraph<G, D>, show: bool) {
        if let Some(node) = scene_graph.runtime_node_mut(self.runtime_node) {
            node.show = show;
        }
    }

    /// The node's local transform. Changes only take effect through
    /// [`Self::set_matrix`].
    pub fn matrix<'a, G: Gl, D>(&self, scene_graph: &'a ModelSceneGraph<G, D>) -> Option<&'a Matrix4> {
        scene_graph.runtime_node(self.runtime_node).map(|node| node.transform())
    }

    /// Assigns a user transform, or restores the original one with `None`.
    ///
    /// Assigning `Some` sets `userAnimated` and marks the model's user
    /// animation dirty so the next update rebuilds transforms. `None` lets the
    /// model's own animations drive the node again.
    pub fn set_matrix<G: Gl, D>(&self, scene_graph: &mut ModelSceneGraph<G, D>, value: Option<&Matrix4>) {
        match value {
            Some(value) => {
                if let Some(node) = scene_graph.runtime_node_mut(self.runtime_node) {
                    node.set_transform(*value);
                    node.user_animated = true;
                }
                scene_graph.user_animation_dirty = true;
            }
            None => {
                let original = scene_graph.runtime_node(self.runtime_node).map(|node| *node.original_transform());
                if let Some(original) = original {
                    if let Some(node) = scene_graph.runtime_node_mut(self.runtime_node) {
                        node.set_transform(original);
                        node.user_animated = false;
                    }
                }
            }
        }
    }

    /// The node's original local transform, without ancestors or articulations.
    pub fn original_matrix<'a, G: Gl, D>(&self, scene_graph: &'a ModelSceneGraph<G, D>) -> Option<&'a Matrix4> {
        scene_graph.runtime_node(self.runtime_node).map(|node| node.original_transform())
    }
}
