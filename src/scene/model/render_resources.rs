//! Model, node and primitive pipeline resources. Children clone mutable shader
//! and render-state configuration while GPU attributes and uniform providers
//! retain shared ownership, matching Cesium's shallow/deep copy boundaries.
use super::{model_utility, CustomShader, ModelAlphaOptions, ModelLightingOptions, ModelType};
use crate::{
    core::{webgl_constants as gl, BoundingSphere, Cartesian3, Color, Matrix4, PrimitiveType, RuntimeError},
    renderer::{
        render_state::DepthTest, Buffer, Gl, Pass, PickId, RenderState, ShaderBuilder, Texture, UniformMap,
        UniformValue, VertexArray, VertexAttribute,
    },
    scene::{model_components as mc, ClassificationType, ColorBlendMode},
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    rc::Rc,
};

/// Model properties read by pipeline uniform callbacks. Hosts can update them
/// between draws; providers read the latest values on every submission.
#[derive(Clone, Debug)]
pub struct ModelUniformState {
    pub opaque_pass: Pass,
    pub color: Color,
    /// Whether `model.color` was explicitly defined (`defined(model.color)`).
    pub has_color: bool,
    pub color_blend_mode: ColorBlendMode,
    pub color_blend_amount: f64,
    pub split_direction: f64,
    /// The format the model was loaded from.
    pub model_type: ModelType,
    /// Whether the model may be projected to 2D in non-3D scene modes.
    pub project_to_2d: bool,
    /// Whether CPU/GPU picking (`model._enablePick`) is enabled.
    pub enable_pick: bool,
    /// Set for classification models.
    pub classification_type: Option<ClassificationType>,
    /// HDR light color, or `None` to use the scene light.
    pub light_color: Option<Cartesian3>,
    pub silhouette_color: Color,
    pub silhouette_size: f64,
    /// Assigned by [`super::model_silhouette_pipeline_stage::process`].
    pub silhouette_id: Option<usize>,
    pub outline_color: Color,
    pub show_outline: bool,
    /// The model's bounding sphere, used by atmosphere and clipping stages.
    pub bounding_sphere: BoundingSphere,
    /// Active primitive feature ID label (`model.featureIdLabel`).
    pub feature_id_label: String,
    /// Active instance feature ID label (`model.instanceFeatureIdLabel`).
    pub instance_feature_id_label: String,
    /// Whether `model.pointCloudShading.attenuation` is enabled.
    pub point_cloud_shading_attenuation: bool,
    /// Optional `model.pointCloudShading.normalShading` override (`Some(false)` disables point cloud normals).
    pub point_cloud_shading_normal_shading: Option<bool>,
    /// Whether back-face culling (`model.backFaceCulling`) is enabled.
    pub back_face_culling: bool,
    /// `model.modelMatrix`.
    pub model_matrix: Matrix4,
    /// `model.sceneGraph.computedModelMatrix`.
    pub scene_graph_computed_model_matrix: Matrix4,
    /// `model._pointSize`.
    pub point_size: f64,
}
impl Default for ModelUniformState {
    fn default() -> Self {
        Self {
            opaque_pass: Pass::Opaque,
            color: Color::WHITE,
            has_color: false,
            color_blend_mode: ColorBlendMode::Highlight,
            color_blend_amount: 0.5,
            split_direction: 0.,
            model_type: ModelType::Gltf,
            project_to_2d: false,
            enable_pick: false,
            classification_type: None,
            light_color: None,
            silhouette_color: Color::RED,
            silhouette_size: 0.,
            silhouette_id: None,
            outline_color: Color::BLACK,
            show_outline: true,
            bounding_sphere: BoundingSphere::default(),
            feature_id_label: "featureId_0".into(),
            instance_feature_id_label: "instanceFeatureId_0".into(),
            point_cloud_shading_attenuation: false,
            point_cloud_shading_normal_shading: None,
            back_face_culling: true,
            model_matrix: Matrix4::IDENTITY,
            scene_graph_computed_model_matrix: Matrix4::IDENTITY,
            point_size: 1.,
        }
    }
}
pub type ModelUniformProviders = BTreeMap<String, Rc<dyn Fn() -> UniformValue>>;
pub type ModelTextureUniformProviders<G> = BTreeMap<String, Rc<dyn Fn() -> Option<Rc<Texture<G>>>>>;
pub struct PipelineResources {
    pub model: Rc<RefCell<ModelUniformState>>,
    pub shader_builder: ShaderBuilder,
    pub uniform_map: ModelUniformProviders,
    pub alpha_options: Rc<RefCell<ModelAlphaOptions>>,
    pub render_state_options: RenderState,
    pub has_silhouette: bool,
    pub has_skip_level_of_detail: bool,
}
impl Clone for PipelineResources {
    fn clone(&self) -> Self {
        Self {
            model: self.model.clone(),
            shader_builder: self.shader_builder.clone(),
            uniform_map: self.uniform_map.clone(),
            alpha_options: Rc::new(RefCell::new(*self.alpha_options.borrow())),
            render_state_options: self.render_state_options.clone(),
            has_silhouette: self.has_silhouette,
            has_skip_level_of_detail: self.has_skip_level_of_detail,
        }
    }
}
impl PipelineResources {
    fn new(model: Rc<RefCell<ModelUniformState>>) -> Self {
        Self {
            model,
            shader_builder: ShaderBuilder::default(),
            uniform_map: BTreeMap::new(),
            alpha_options: Rc::new(RefCell::new(ModelAlphaOptions::default())),
            render_state_options: RenderState {
                depth_test: DepthTest { enabled: true, func: gl::LEQUAL },
                ..Default::default()
            },
            has_silhouette: false,
            has_skip_level_of_detail: false,
        }
    }
    /// Adapt the named providers to the renderer's executable command API.
    pub fn renderer_uniform_map(&self) -> UniformMap {
        let providers = self.uniform_map.clone();
        Rc::new(move || Ok(providers.iter().map(|(name, value)| (name.clone(), value())).collect()))
    }
}
pub struct ModelRenderResources<G: Gl> {
    pub pipeline: PipelineResources,
    pub texture_uniform_map: ModelTextureUniformProviders<G>,
    pub node_render_resources: Vec<Option<NodeRenderResources<G>>>,
    /// `model.customShader`. The model state itself is GPU-independent, so the
    /// shared custom shader lives on the GPU render resources and is
    /// shallow-copied into nodes and primitives, as upstream does.
    pub custom_shader: Option<Rc<CustomShader<G>>>,
}
impl<G: Gl> ModelRenderResources<G> {
    pub fn new(model: Rc<RefCell<ModelUniformState>>) -> Self {
        Self {
            pipeline: PipelineResources::new(model),
            texture_uniform_map: BTreeMap::new(),
            node_render_resources: vec![],
            custom_shader: None,
        }
    }
}
pub struct NodeRenderResources<G: Gl> {
    pub pipeline: PipelineResources,
    pub texture_uniform_map: ModelTextureUniformProviders<G>,
    pub custom_shader: Option<Rc<CustomShader<G>>>,
    /// Index of the node in the model components/runtime scene graph.
    pub runtime_node: usize,
    pub attributes: Vec<Rc<VertexAttribute<G>>>,
    pub attribute_index: u32,
    pub feature_id_vertex_attribute_set_index: u32,
    pub instance_count: u32,
    /// Whether the runtime node has GPU instances.
    pub has_instances: bool,
    pub primitive_render_resources: Vec<PrimitiveRenderResources<G>>,
}
impl<G: Gl> NodeRenderResources<G> {
    pub fn new(model: &ModelRenderResources<G>, runtime_node: usize) -> Self {
        Self {
            pipeline: model.pipeline.clone(),
            texture_uniform_map: model.texture_uniform_map.clone(),
            custom_shader: model.custom_shader.clone(),
            runtime_node,
            attributes: vec![],
            attribute_index: 1,
            feature_id_vertex_attribute_set_index: 0,
            instance_count: 0,
            has_instances: false,
            primitive_render_resources: vec![],
        }
    }
}
pub struct PrimitiveRenderResources<G: Gl> {
    pub pipeline: PipelineResources,
    pub texture_uniform_map: ModelTextureUniformProviders<G>,
    pub custom_shader: Option<Rc<CustomShader<G>>>,
    pub runtime_node: usize,
    pub runtime_primitive: usize,
    pub attributes: Vec<Rc<VertexAttribute<G>>>,
    pub attribute_index: u32,
    pub feature_id_vertex_attribute_set_index: u32,
    pub instance_count: u32,
    pub has_instances: bool,
    pub count: Option<usize>,
    pub has_property_table: bool,
    pub indices: Option<mc::Indices<G>>,
    /// Projected positions for 2D/CV, supplied by the runtime primitive.
    pub position_buffer_2d: Option<Buffer<G>>,
    pub wireframe_index_buffer: Option<Buffer<G>>,
    /// Classification batches, computed by the classification stage.
    pub batch_lengths: Option<Vec<usize>>,
    pub batch_offsets: Option<Vec<usize>>,
    pub primitive_type: Option<PrimitiveType>,
    pub position_min: Option<Cartesian3>,
    pub position_max: Option<Cartesian3>,
    pub bounding_sphere: BoundingSphere,
    pub lighting_options: ModelLightingOptions,
    pub pick_id: Option<String>,
    pub pick_ids: Vec<Rc<PickId>>,
    pub edge_geometry: Option<EdgeGeometry<G>>,
}
impl<G: Gl> PrimitiveRenderResources<G> {
    pub fn new(
        node: &NodeRenderResources<G>,
        runtime_primitive: usize,
        primitive: &mc::Primitive<G>,
        translation_min: Option<Cartesian3>,
        translation_max: Option<Cartesian3>,
    ) -> Result<Self, RuntimeError> {
        let (position_min, position_max) =
            model_utility::get_position_min_max(primitive, translation_min, translation_max)?;
        let bounding_sphere = BoundingSphere::from_corner_points(
            position_min.ok_or_else(|| RuntimeError::new("Missing position minimum"))?,
            position_max.ok_or_else(|| RuntimeError::new("Missing position maximum"))?,
        );
        let count = if let Some(indices) = &primitive.indices {
            indices.count
        } else {
            model_utility::get_attribute_by_semantic(
                &primitive.attributes,
                mc::AttributeSemantic::Vertex(crate::scene::VertexAttributeSemantic::Position),
                None,
            )
            .and_then(|a| a.count)
        };
        Ok(Self {
            pipeline: node.pipeline.clone(),
            texture_uniform_map: node.texture_uniform_map.clone(),
            custom_shader: node.custom_shader.clone(),
            runtime_node: node.runtime_node,
            runtime_primitive,
            attributes: node.attributes.clone(),
            attribute_index: node.attribute_index,
            feature_id_vertex_attribute_set_index: node.feature_id_vertex_attribute_set_index,
            instance_count: node.instance_count,
            has_instances: node.has_instances,
            count,
            has_property_table: false,
            indices: primitive.indices.as_ref().map(|i| mc::Indices {
                index_datatype: i.index_datatype,
                count: i.count,
                buffer: i.buffer.clone(),
                typed_array: i.typed_array.clone(),
            }),
            position_buffer_2d: None,
            wireframe_index_buffer: None,
            batch_lengths: None,
            batch_offsets: None,
            primitive_type: primitive.primitive_type,
            position_min,
            position_max,
            bounding_sphere,
            lighting_options: ModelLightingOptions::default(),
            pick_id: None,
            pick_ids: Vec::new(),
            edge_geometry: None,
        })
    }
}
macro_rules! pipeline_access {
    ($t:ident) => {
        impl<G: Gl> Deref for $t<G> {
            type Target = PipelineResources;
            fn deref(&self) -> &Self::Target {
                &self.pipeline
            }
        }
        impl<G: Gl> DerefMut for $t<G> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.pipeline
            }
        }
    };
}
pipeline_access!(ModelRenderResources);
pipeline_access!(NodeRenderResources);
pipeline_access!(PrimitiveRenderResources);

pub struct EdgeGeometry<G: Gl> {
    pub vertex_array: Option<VertexArray<G>>,
    pub index_count: usize,
    pub primitive_type: PrimitiveType,
    pub pass: Pass,
}
