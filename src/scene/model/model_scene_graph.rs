//! CesiumJS `Scene/Model/ModelSceneGraph`: the runtime node, skin,
//! articulation and primitive hierarchy of a loaded model.
//!
//! The scene graph owns the model components and addresses runtime nodes and
//! primitives by index, so no reference cycles are needed between nodes, skins
//! and their parent graph. GPU work stays in the render lane: this module
//! computes transforms, bounds, joint matrices and articulations and publishes
//! the traversal order those stages use.

use super::{
    model_runtime_node::{ModelRuntimeNode, NodeUpdateStage},
    model_runtime_primitive::ModelRuntimePrimitive,
    model_skin::ModelSkin,
    ModelArticulation, ModelNode, ModelRenderResources, ModelType,
};
use crate::{
    core::{BoundingSphere, Cartesian3, MapProjection, Matrix4, RuntimeError},
    renderer::Gl,
    scene::{model::model_utility, model_components as mc, Axis, FrameState, SceneMode, SplitDirection},
};
use std::collections::BTreeMap;

/// Model-level pipeline stages, in the order `configure_pipeline` pushes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelPipelineStage {
    Color,
    ImageBasedLighting,
    ClippingPlanes,
    ClippingPolygons,
    Silhouette,
    Splitter,
    Tileset,
    Atmosphere,
}

/// The `customShader` properties `configurePipeline` inspects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CustomShaderPipelineProperties {
    /// `defined(customShader.fragmentShaderText)`.
    pub has_fragment_shader: bool,
    /// `customShader.mode === CustomShaderMode.REPLACE_MATERIAL`.
    pub replace_material: bool,
}

/// The `pointCloudShading` properties `configurePipeline` inspects.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointCloudShadingProperties {
    pub attenuation: bool,
    pub back_face_culling: bool,
}

/// The model properties the runtime's `configurePipeline` methods read.
///
/// This is the typed boundary between the runtime lane and the render lane's
/// `Model`: fill it once from the model's public properties before configuring
/// pipelines and mutate it when those properties change.
#[derive(Clone, Debug)]
pub struct ModelPipelineProperties {
    /// `frameState.context.webgl2`.
    pub use_webgl2: bool,
    /// Whether the model may be projected to 2D in non-3D scene modes.
    pub project_to_2d: bool,
    /// `model.hasVerticalExaggeration`.
    pub has_vertical_exaggeration: bool,
    /// `defined(model.imageryLayers)`.
    pub has_imagery_layers: bool,
    pub custom_shader: Option<CustomShaderPipelineProperties>,
    /// `defined(model.style)`.
    pub has_style: bool,
    /// `model.debugWireframe`.
    pub debug_wireframe: bool,
    /// `model._enableDebugWireframe`.
    pub enable_debug_wireframe: bool,
    pub point_cloud_shading: Option<PointCloudShadingProperties>,
    /// `model._enableShowOutline`.
    pub enable_show_outline: bool,
    /// `defined(model.classificationType)`.
    pub classification_type: Option<crate::scene::ClassificationType>,
    /// `model.allowPicking`.
    pub allow_picking: bool,
    /// `model.featureIdLabel`.
    pub feature_id_label: String,
    /// `model.instanceFeatureIdLabel`.
    pub instance_feature_id_label: String,
    /// `defined(model.color)`.
    pub has_color: bool,
    /// `model.imageBasedLighting.enabled`.
    pub image_based_lighting_enabled: bool,
    /// `model.isClippingEnabled()`.
    pub clipping_enabled: bool,
    /// `model.isClippingPolygonsEnabled()`.
    pub clipping_polygons_enabled: bool,
    /// `model.hasSilhouette(frameState)`.
    pub has_silhouette: bool,
    /// `model.splitDirection`.
    pub split_direction: SplitDirection,
    /// `model.type`.
    pub model_type: ModelType,
}

impl Default for ModelPipelineProperties {
    fn default() -> Self {
        Self {
            use_webgl2: true,
            project_to_2d: false,
            has_vertical_exaggeration: false,
            has_imagery_layers: false,
            custom_shader: None,
            has_style: false,
            debug_wireframe: false,
            enable_debug_wireframe: false,
            point_cloud_shading: None,
            enable_show_outline: true,
            classification_type: None,
            allow_picking: true,
            feature_id_label: "featureId_0".into(),
            instance_feature_id_label: "instanceFeatureId_0".into(),
            has_color: false,
            image_based_lighting_enabled: true,
            clipping_enabled: false,
            clipping_polygons_enabled: false,
            has_silhouette: false,
            split_direction: SplitDirection::None,
            model_type: ModelType::Gltf,
        }
    }
}

/// Bounding volumes computed by [`ModelSceneGraph::compute_bounding_volumes`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelBoundingVolumes {
    /// All primitives in model space, after axis correction and the model's
    /// own transform.
    pub scene_graph_bounding_sphere: BoundingSphere,
    /// The scene-graph sphere in world space.
    pub model_bounding_sphere: BoundingSphere,
    /// `model_bounding_sphere.radius` before scaling.
    pub initial_radius: f64,
}

/// Options for [`ModelSceneGraph::new`].
#[derive(Clone, Debug)]
pub struct ModelSceneGraphOptions {
    /// `model.modelMatrix`.
    pub model_matrix: Matrix4,
    /// `model.computedScale`.
    pub computed_scale: f64,
    /// `model._clampedScale`, applied to the model bounding sphere's radius.
    pub clamped_scale: f64,
    pub pipeline: ModelPipelineProperties,
}

impl Default for ModelSceneGraphOptions {
    fn default() -> Self {
        Self {
            model_matrix: Matrix4::IDENTITY,
            computed_scale: 1.0,
            clamped_scale: 1.0,
            pipeline: ModelPipelineProperties::default(),
        }
    }
}

/// An in-memory representation of the scene graph for a model.
pub struct ModelSceneGraph<G: Gl, D = ()> {
    components: mc::Components<G>,
    root_nodes: Vec<usize>,
    skinned_nodes: Vec<usize>,
    /// Index-aligned with `components.nodes`; unreachable nodes stay `None`.
    runtime_nodes: Vec<Option<ModelRuntimeNode<G, D>>>,
    runtime_skins: Vec<ModelSkin>,
    /// Articulations in model order. Cesium keys them by name; the port keeps
    /// the list so unnamed articulations are still applied.
    runtime_articulations: Vec<ModelArticulation>,
    runtime_articulations_by_name: BTreeMap<String, usize>,
    nodes_by_name: BTreeMap<String, ModelNode>,
    model_pipeline_stages: Vec<ModelPipelineStage>,
    computed_model_matrix: Matrix4,
    computed_model_matrix_2d: Matrix4,
    axis_correction_matrix: Matrix4,
    computed_scale: f64,
    bounding_sphere: Option<BoundingSphere>,
    bounding_sphere_2d: Option<BoundingSphere>,
    pipeline: ModelPipelineProperties,
    project_to_2d: bool,
    /// `model._userAnimationDirty`: set when a `ModelNode` matrix is assigned.
    pub user_animation_dirty: bool,
}

impl<G: Gl, D> ModelSceneGraph<G, D> {
    /// Builds the runtime scene graph, including skins and articulations.
    pub fn new(components: mc::Components<G>, options: ModelSceneGraphOptions) -> Result<Self, RuntimeError> {
        let axis_correction_matrix = model_utility::get_axis_correction_matrix(
            components.up_axis.unwrap_or(Axis::Y),
            components.forward_axis.unwrap_or(Axis::Z),
        );
        let project_to_2d = options.pipeline.project_to_2d;

        let mut graph = Self {
            root_nodes: Vec::new(),
            skinned_nodes: Vec::new(),
            runtime_nodes: (0..components.nodes.len()).map(|_| None).collect(),
            runtime_skins: Vec::new(),
            runtime_articulations: Vec::new(),
            runtime_articulations_by_name: BTreeMap::new(),
            nodes_by_name: BTreeMap::new(),
            model_pipeline_stages: Vec::new(),
            computed_model_matrix: Matrix4::IDENTITY,
            computed_model_matrix_2d: Matrix4::IDENTITY,
            axis_correction_matrix,
            computed_scale: options.computed_scale,
            bounding_sphere: None,
            bounding_sphere_2d: None,
            pipeline: options.pipeline,
            project_to_2d,
            user_animation_dirty: false,
            components,
        };

        graph.compute_model_matrix(&options.model_matrix, options.computed_scale);

        for (index, articulation) in graph.components.articulations.iter().enumerate() {
            let runtime_articulation = ModelArticulation::new(articulation)?;
            if let Some(name) = runtime_articulation.name() {
                graph.runtime_articulations_by_name.insert(name.to_string(), index);
            }
            graph.runtime_articulations.push(runtime_articulation);
        }

        let root_nodes = graph.components.scene.clone().unwrap_or_default().nodes;
        let mut visiting = vec![false; graph.components.nodes.len()];
        for root_node in root_nodes {
            let index = graph.traverse_and_create_scene_graph(root_node, &Matrix4::IDENTITY, &mut visiting)?;
            graph.root_nodes.push(index);
        }

        // Skins are created after every runtime node exists, because joints are
        // node indices.
        for skin in &graph.components.skins {
            graph.runtime_skins.push(ModelSkin::new(skin, &graph.runtime_nodes));
        }
        for i in 0..graph.skinned_nodes.len() {
            let node_index = graph.skinned_nodes[i];
            let Some(skin_index) = graph.components.nodes[node_index].skin else {
                continue;
            };
            if graph.runtime_skins.get(skin_index).is_none() {
                continue;
            }
            if let Some(node) = graph.runtime_nodes[node_index].as_mut() {
                node.set_runtime_skin(Some(skin_index));
            }
            graph.runtime_skins[skin_index].update_joint_matrices(&graph.runtime_nodes);
            let skin = &graph.runtime_skins[skin_index];
            if let Some(node) = graph.runtime_nodes[node_index].as_mut() {
                node.update_joint_matrices(skin);
            }
        }

        // Ensure articulations are applied with their initial values.
        graph.apply_articulations();
        Ok(graph)
    }

    fn traverse_and_create_scene_graph(
        &mut self,
        node_index: usize,
        transform_to_root: &Matrix4,
        visiting: &mut [bool],
    ) -> Result<usize, RuntimeError> {
        if node_index >= self.components.nodes.len() {
            return Err(RuntimeError::new("Node index is out of range"));
        }
        if visiting[node_index] {
            return Err(RuntimeError::new("Cycle in the model node hierarchy"));
        }
        visiting[node_index] = true;

        let node = self.components.nodes[node_index].clone();
        let transform = model_utility::get_node_transform(&node);

        let mut children_indices = Vec::with_capacity(node.children.len());
        for &child_index in &node.children {
            let child_transform_to_root = transform_to_root.multiply_transformation(&transform);
            let child = self.traverse_and_create_scene_graph(child_index, &child_transform_to_root, visiting)?;
            children_indices.push(child);
        }

        let mut runtime_node = ModelRuntimeNode::new(&node, transform, *transform_to_root, children_indices);
        for primitive_index in 0..node.primitives.len() {
            runtime_node.runtime_primitives.push(ModelRuntimePrimitive::new(node_index, primitive_index));
        }

        if let Some(name) = runtime_node.name().map(str::to_string) {
            self.nodes_by_name.insert(name, ModelNode::new(node_index, runtime_node.name(), runtime_node.id()));
        }
        if node.skin.is_some() {
            self.skinned_nodes.push(node_index);
        }
        if let Some(articulation_name) = &node.articulation_name {
            if let Some(&articulation_index) = self.runtime_articulations_by_name.get(articulation_name) {
                self.runtime_articulations[articulation_index].add_runtime_node(node_index);
            }
        }

        self.runtime_nodes[node_index] = Some(runtime_node);
        visiting[node_index] = false;
        Ok(node_index)
    }

    /// The model components this scene graph represents.
    pub fn components(&self) -> &mc::Components<G> {
        &self.components
    }

    pub fn components_mut(&mut self) -> &mut mc::Components<G> {
        &mut self.components
    }

    /// The node component at `node_index`, if it exists.
    pub fn node(&self, node_index: usize) -> Option<&mc::Node<G>> {
        self.components.nodes.get(node_index)
    }

    /// The primitive component for a runtime primitive.
    pub fn primitive(&self, node_index: usize, primitive_index: usize) -> Option<&mc::Primitive<G>> {
        self.components.nodes.get(node_index)?.primitives.get(primitive_index)
    }

    /// `model.modelMatrix * components.transform * axisCorrection * scale`.
    pub fn computed_model_matrix(&self) -> &Matrix4 {
        &self.computed_model_matrix
    }

    /// The axis-corrected model matrix used when projecting to 2D.
    pub fn computed_model_matrix_2d(&self) -> &Matrix4 {
        &self.computed_model_matrix_2d
    }

    /// A matrix to correct from y-up in some model formats to z-up.
    pub fn axis_correction_matrix(&self) -> &Matrix4 {
        &self.axis_correction_matrix
    }

    /// The scene graph's bounding sphere in model space, once computed.
    pub fn bounding_sphere(&self) -> Option<&BoundingSphere> {
        self.bounding_sphere.as_ref()
    }

    /// The scene graph's bounding sphere in 2D world space, once computed.
    pub fn bounding_sphere_2d(&self) -> Option<&BoundingSphere> {
        self.bounding_sphere_2d.as_ref()
    }

    pub fn project_to_2d(&self) -> bool {
        self.project_to_2d
    }

    pub fn set_project_to_2d(&mut self, project_to_2d: bool) {
        self.project_to_2d = project_to_2d;
    }

    pub fn model_pipeline_stages(&self) -> &[ModelPipelineStage] {
        &self.model_pipeline_stages
    }

    /// The indices of the root nodes in the runtime node array.
    pub fn root_nodes(&self) -> &[usize] {
        &self.root_nodes
    }

    /// The runtime node at `index`, or `None` for an unreachable node.
    pub fn runtime_node(&self, index: usize) -> Option<&ModelRuntimeNode<G, D>> {
        self.runtime_nodes.get(index).and_then(Option::as_ref)
    }

    pub fn runtime_node_mut(&mut self, index: usize) -> Option<&mut ModelRuntimeNode<G, D>> {
        self.runtime_nodes.get_mut(index).and_then(Option::as_mut)
    }

    /// The runtime nodes, index-aligned with the model components.
    pub fn runtime_nodes(&self) -> &[Option<ModelRuntimeNode<G, D>>] {
        &self.runtime_nodes
    }

    pub fn runtime_nodes_mut(&mut self) -> &mut [Option<ModelRuntimeNode<G, D>>] {
        &mut self.runtime_nodes
    }

    /// The runtime primitive of a node, if both indices exist.
    pub fn runtime_primitive(&self, node_index: usize, primitive_index: usize) -> Option<&ModelRuntimePrimitive<G, D>> {
        self.runtime_node(node_index)?.runtime_primitives.get(primitive_index)
    }

    pub fn runtime_primitive_mut(
        &mut self,
        node_index: usize,
        primitive_index: usize,
    ) -> Option<&mut ModelRuntimePrimitive<G, D>> {
        self.runtime_node_mut(node_index)?.runtime_primitives.get_mut(primitive_index)
    }

    pub fn runtime_skins(&self) -> &[ModelSkin] {
        &self.runtime_skins
    }

    pub fn runtime_skins_mut(&mut self) -> &mut [ModelSkin] {
        &mut self.runtime_skins
    }

    /// The runtime nodes that a skin animates, as opposed to joint nodes.
    pub fn skinned_nodes(&self) -> &[usize] {
        &self.skinned_nodes
    }

    /// Articulations in model order.
    pub fn runtime_articulations(&self) -> &[ModelArticulation] {
        &self.runtime_articulations
    }

    pub fn runtime_articulation(&self, name: &str) -> Option<&ModelArticulation> {
        let index = *self.runtime_articulations_by_name.get(name)?;
        self.runtime_articulations.get(index)
    }

    /// The public `ModelNode` handles, keyed by glTF node name.
    pub fn nodes_by_name(&self) -> &BTreeMap<String, ModelNode> {
        &self.nodes_by_name
    }

    /// The public handle for a named node, as `Model#getNode` returns.
    pub fn get_node(&self, name: &str) -> Option<&ModelNode> {
        self.nodes_by_name.get(name)
    }

    /// `model.modelMatrix * components.transform * axisCorrection * scale`.
    pub fn compute_model_matrix(&mut self, model_matrix: &Matrix4, computed_scale: f64) {
        let mut matrix = model_matrix.multiply_transformation(&self.components.transform);
        matrix = matrix.multiply_transformation(&self.axis_correction_matrix);
        self.computed_model_matrix = matrix.multiply_by_uniform_scale(computed_scale);
        self.computed_scale = computed_scale;
    }

    /// Builds the 2D/CV model matrix and the 2D bounding sphere.
    ///
    /// Cesium uses `Matrix4.multiply`; with two affine matrices
    /// `multiply_transformation` is identical.
    pub fn compute_model_matrix_2d(&mut self, map_projection: &dyn MapProjection) {
        let translation = self.computed_model_matrix.get_translation();
        if translation != Cartesian3::ZERO {
            self.computed_model_matrix_2d =
                crate::core::Transforms::basis_to_2d(map_projection, &self.computed_model_matrix);
        } else if let Some(bounding_sphere) = self.bounding_sphere {
            let to_2d = crate::core::Transforms::ellipsoid_to_2d_model_matrix(map_projection, bounding_sphere.center);
            self.computed_model_matrix_2d = to_2d.multiply_transformation(&self.computed_model_matrix);
        }
        self.bounding_sphere_2d =
            self.bounding_sphere.map(|bounding_sphere| bounding_sphere.transform(&self.computed_model_matrix_2d));
    }

    /// Updates the model matrix, any 2D matrix and marks every root node dirty
    /// so the update stage propagates the change to the subtree.
    pub fn update_model_matrix(
        &mut self,
        model_matrix: &Matrix4,
        frame_state: &FrameState,
        map_projection: Option<&dyn MapProjection>,
    ) {
        self.compute_model_matrix(model_matrix, self.computed_scale);
        if frame_state.mode != SceneMode::Scene3D {
            if let Some(map_projection) = map_projection {
                self.compute_model_matrix_2d(map_projection);
            }
        }
        for &root in &self.root_nodes {
            if let Some(node) = self.runtime_nodes[root].as_mut() {
                node.mark_transform_dirty();
            }
        }
    }

    /// Applies any modified articulation stages to their nodes.
    pub fn apply_articulations(&mut self) {
        let nodes = &mut self.runtime_nodes;
        for articulation in &mut self.runtime_articulations {
            articulation.apply(nodes);
        }
    }

    /// Sets a stage value from `"<articulation> <stage>"`.
    ///
    /// Keys that do not split into exactly two parts are ignored, as in
    /// Cesium.
    pub fn set_articulation_stage(&mut self, articulation_stage_key: &str, value: f64) {
        let mut names = articulation_stage_key.split(' ');
        let (Some(articulation_name), Some(stage_name), None) = (names.next(), names.next(), names.next()) else {
            return;
        };
        if let Some(&index) = self.runtime_articulations_by_name.get(articulation_name) {
            self.runtime_articulations[index].set_articulation_stage(stage_name, value);
        }
    }

    /// Updates joint matrices for every skinned node.
    pub fn update_joint_matrices(&mut self) {
        let nodes = &self.runtime_nodes;
        for skin in &mut self.runtime_skins {
            skin.update_joint_matrices(nodes);
        }
        for i in 0..self.skinned_nodes.len() {
            let node_index = self.skinned_nodes[i];
            let Some(skin_index) = self.components.nodes.get(node_index).and_then(|n| n.skin) else {
                continue;
            };
            let Some(skin) = self.runtime_skins.get(skin_index) else {
                continue;
            };
            if let Some(node) = self.runtime_nodes[node_index].as_mut() {
                node.update_joint_matrices(skin);
            }
        }
    }

    /// Runs the runtime update stages for this frame, without draw-command
    /// updates.
    ///
    /// `update_for_animations` is true while an animation is playing or the
    /// user changed a node. Returns the nodes whose transforms changed. Joint
    /// matrices are updated once per frame; Cesium recomputes them inside its
    /// per-node loop, which yields the same values.
    pub fn update_transforms(&mut self, frame_state: &FrameState, update_for_animations: bool) -> Vec<usize> {
        let updated = super::ModelMatrixUpdateStage::update_transforms(self, frame_state);
        self.update_joint_matrices_for_frame(frame_state, update_for_animations);
        updated
    }

    fn update_joint_matrices_for_frame(&mut self, frame_state: &FrameState, update_for_animations: bool) {
        let disable_animations = frame_state.mode != SceneMode::Scene3D && self.project_to_2d;
        if update_for_animations && !disable_animations {
            self.update_joint_matrices();
        }
    }

    /// Configures the model-level pipeline stages for this frame.
    pub fn configure_pipeline(&mut self, frame_state: &FrameState, properties: &ModelPipelineProperties) {
        self.model_pipeline_stages.clear();

        if properties.has_color {
            self.model_pipeline_stages.push(ModelPipelineStage::Color);
        }
        // Classification models skip the remaining model stages.
        if properties.classification_type.is_some() {
            return;
        }
        if properties.image_based_lighting_enabled {
            self.model_pipeline_stages.push(ModelPipelineStage::ImageBasedLighting);
        }
        if properties.clipping_enabled {
            self.model_pipeline_stages.push(ModelPipelineStage::ClippingPlanes);
        }
        if properties.clipping_polygons_enabled {
            self.model_pipeline_stages.push(ModelPipelineStage::ClippingPolygons);
        }
        if properties.has_silhouette {
            self.model_pipeline_stages.push(ModelPipelineStage::Silhouette);
        }
        if properties.split_direction != SplitDirection::None {
            self.model_pipeline_stages.push(ModelPipelineStage::Splitter);
        }
        if ModelType::is_3d_tiles(properties.model_type) {
            self.model_pipeline_stages.push(ModelPipelineStage::Tileset);
        }
        if frame_state.fog.enabled && frame_state.fog.renderable {
            self.model_pipeline_stages.push(ModelPipelineStage::Atmosphere);
        }
    }

    /// The post-order depth-first traversal order of the runtime primitives.
    ///
    /// Hidden subtrees are skipped when `visible_nodes_only` is true.
    pub fn runtime_primitive_order(&self, visible_nodes_only: bool) -> Vec<(usize, usize)> {
        let mut order = Vec::new();
        for &root in &self.root_nodes {
            self.collect_primitive_order(root, visible_nodes_only, &mut order);
        }
        order
    }

    fn collect_primitive_order(&self, node_index: usize, visible_nodes_only: bool, order: &mut Vec<(usize, usize)>) {
        let Some(node) = self.runtime_node(node_index) else {
            return;
        };
        if visible_nodes_only && !node.show {
            return;
        }
        for &child in node.children() {
            self.collect_primitive_order(child, visible_nodes_only, order);
        }
        for primitive_index in 0..node.runtime_primitives.len() {
            order.push((node_index, primitive_index));
        }
    }

    /// Calls `callback` for every runtime primitive in post-order.
    pub fn for_each_runtime_primitive(
        &self,
        visible_nodes_only: bool,
        mut callback: impl FnMut(&ModelRuntimePrimitive<G, D>),
    ) {
        for (node_index, primitive_index) in self.runtime_primitive_order(visible_nodes_only) {
            if let Some(primitive) = self.runtime_primitive(node_index, primitive_index) {
                callback(primitive);
            }
        }
    }

    /// Sets back-face culling on every draw command.
    pub fn update_back_face_culling(&mut self, back_face_culling: bool)
    where
        D: super::RuntimeDrawCommand,
    {
        let order = self.runtime_primitive_order(false);
        for (node_index, primitive_index) in order {
            if let Some(draw_command) =
                self.runtime_primitive_mut(node_index, primitive_index).and_then(|p| p.draw_command.as_mut())
            {
                draw_command.set_back_face_culling(back_face_culling);
            }
        }
    }

    /// Sets shadow rendering on every draw command.
    pub fn update_shadows(&mut self, shadows: bool)
    where
        D: super::RuntimeDrawCommand,
    {
        let order = self.runtime_primitive_order(false);
        for (node_index, primitive_index) in order {
            if let Some(draw_command) =
                self.runtime_primitive_mut(node_index, primitive_index).and_then(|p| p.draw_command.as_mut())
            {
                draw_command.set_shadows(shadows);
            }
        }
    }

    /// Sets debug bounding-volume rendering on every draw command.
    pub fn update_show_bounding_volume(&mut self, debug_show_bounding_volume: bool)
    where
        D: super::RuntimeDrawCommand,
    {
        let order = self.runtime_primitive_order(false);
        for (node_index, primitive_index) in order {
            if let Some(draw_command) =
                self.runtime_primitive_mut(node_index, primitive_index).and_then(|p| p.draw_command.as_mut())
            {
                draw_command.set_debug_show_bounding_volume(debug_show_bounding_volume);
            }
        }
    }

    /// Stores the draw command the render lane built for a runtime primitive.
    pub fn set_draw_command(&mut self, node_index: usize, primitive_index: usize, draw_command: D) -> bool {
        match self.runtime_primitive_mut(node_index, primitive_index) {
            Some(primitive) => {
                primitive.draw_command = Some(draw_command);
                true
            }
            None => false,
        }
    }

    /// Computes per-primitive and model bounding volumes from the render
    /// resources the render lane built.
    ///
    /// `resources` supplies each primitive's object-space position bounds and
    /// bounding sphere. Returns an error when the model has no primitive with
    /// position bounds, which Cesium would turn into an infinite sphere.
    pub fn compute_bounding_volumes(
        &mut self,
        resources: &ModelRenderResources<G>,
        model_matrix: &Matrix4,
        clamped_scale: f64,
    ) -> Result<ModelBoundingVolumes, RuntimeError> {
        let mut model_position_min = Cartesian3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut model_position_max = Cartesian3::new(-f64::MAX, -f64::MAX, -f64::MAX);
        let mut found = false;

        for node_index in 0..self.runtime_nodes.len() {
            let Some(node) = self.runtime_node(node_index) else {
                continue;
            };
            let Some(node_resources) = resources.node_render_resources.get(node_index).and_then(Option::as_ref) else {
                continue;
            };
            let node_transform = *node.computed_transform();
            let primitive_count = node.runtime_primitives.len();
            for primitive_index in 0..primitive_count {
                let Some(primitive_resources) = node_resources.primitive_render_resources.get(primitive_index) else {
                    continue;
                };
                let (Some(position_min), Some(position_max)) =
                    (primitive_resources.position_min, primitive_resources.position_max)
                else {
                    continue;
                };
                found = true;
                let primitive_position_min = node_transform.multiply_by_point(position_min);
                let primitive_position_max = node_transform.multiply_by_point(position_max);
                model_position_min = minimum_by_component(model_position_min, primitive_position_min);
                model_position_max = maximum_by_component(model_position_max, primitive_position_max);
                if let Some(primitive) = self.runtime_primitive_mut(node_index, primitive_index) {
                    primitive.bounding_sphere = Some(primitive_resources.bounding_sphere);
                }
            }
        }

        if !found {
            return Err(RuntimeError::new("Model has no primitive position bounds"));
        }

        let mut scene_graph_bounding_sphere =
            BoundingSphere::from_corner_points(model_position_min, model_position_max);
        scene_graph_bounding_sphere = scene_graph_bounding_sphere.transform_without_scale(&self.axis_correction_matrix);
        scene_graph_bounding_sphere = scene_graph_bounding_sphere.transform(&self.components.transform);
        self.bounding_sphere = Some(scene_graph_bounding_sphere);

        let mut model_bounding_sphere = scene_graph_bounding_sphere.transform(model_matrix);
        let initial_radius = model_bounding_sphere.radius;
        model_bounding_sphere.radius *= clamped_scale;

        Ok(ModelBoundingVolumes { scene_graph_bounding_sphere, model_bounding_sphere, initial_radius })
    }

    /// The `ModelPipelineProperties` this graph was built with, for callers that
    /// configure primitive pipelines.
    pub fn pipeline_properties(&self) -> &ModelPipelineProperties {
        &self.pipeline
    }

    /// The node's local transform parameters, for debugging.
    pub fn transform_parameters(&self, node_index: usize) -> Option<super::TransformParameters> {
        let node = self.runtime_node(node_index)?;
        Some(super::TransformParameters {
            translation: node.translation()?,
            rotation: node.rotation()?,
            scale: node.scale()?,
        })
    }

    /// Clears the per-frame user-animation flag, as `Model#update` does.
    pub fn clear_user_animation_dirty(&mut self) {
        self.user_animation_dirty = false;
    }

    /// Number of nodes whose update stage has not run yet. Used by tests and
    /// diagnostics.
    pub fn dirty_node_count(&self) -> usize {
        self.runtime_nodes
            .iter()
            .flatten()
            .filter(|node| node.update_stages.contains(&NodeUpdateStage::ModelMatrix) && node.is_transform_dirty())
            .count()
    }
}

/// The runtime stages that need the render lane's draw commands.
impl<G: Gl, D: super::RuntimeDrawCommand> ModelSceneGraph<G, D> {
    /// Runs the runtime update stages for this frame, as
    /// `ModelSceneGraph#update` does, including draw-command matrices and cull
    /// faces.
    ///
    /// `update_for_animations` is true while an animation is playing or the
    /// user changed a node.
    pub fn update(&mut self, frame_state: &FrameState, update_for_animations: bool) {
        let updated = super::ModelMatrixUpdateStage::update_transforms(self, frame_state);
        super::ModelMatrixUpdateStage::update_draw_commands(self, frame_state, &updated);
        self.update_joint_matrices_for_frame(frame_state, update_for_animations);
    }
}

fn minimum_by_component(left: Cartesian3, right: Cartesian3) -> Cartesian3 {
    Cartesian3::new(left.x.min(right.x), left.y.min(right.y), left.z.min(right.z))
}

fn maximum_by_component(left: Cartesian3, right: Cartesian3) -> Cartesian3 {
    Cartesian3::new(left.x.max(right.x), left.y.max(right.y), left.z.max(right.z))
}

impl<G: Gl, D> std::fmt::Debug for ModelSceneGraph<G, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelSceneGraph")
            .field("root_nodes", &self.root_nodes)
            .field("nodes", &self.components.nodes.len())
            .field("skins", &self.runtime_skins.len())
            .field("articulations", &self.runtime_articulations.len())
            .finish_non_exhaustive()
    }
}
