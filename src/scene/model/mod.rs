//! `Scene/Model`: the glTF model runtime. One Rust module per Cesium module.

pub mod cartesian_rectangle;
pub mod custom_shader_mode;
pub mod custom_shader_translucency_mode;
pub mod extensions;
pub mod imagery_configuration;
pub mod imagery_flags;
pub mod lighting_model;
pub mod model_alpha_options;
pub mod model_lighting_options;
pub mod model_type;
pub mod style_commands_needed;
pub mod uniform_type;
pub mod varying_type;

pub use cartesian_rectangle::CartesianRectangle;
pub use custom_shader_mode::CustomShaderMode;
pub use custom_shader_translucency_mode::CustomShaderTranslucencyMode;
pub use imagery_configuration::{ImageryConfiguration, ImageryLayerSettings};
pub use imagery_flags::ImageryFlags;
pub use lighting_model::LightingModel;
pub use model_alpha_options::ModelAlphaOptions;
pub use model_lighting_options::ModelLightingOptions;
pub use model_type::ModelType;
pub use style_commands_needed::StyleCommandsNeeded;
pub use uniform_type::UniformType;
pub use varying_type::VaryingType;

pub mod b3dm_loader;
pub mod geo_json_loader;
pub mod i3dm_loader;
pub mod pnts_loader;
pub use b3dm_loader::{B3dmLoader, B3dmLoaderOptions};
pub use geo_json_loader::GeoJsonLoader;
pub use i3dm_loader::{I3dmLoader, I3dmLoaderOptions};
pub use pnts_loader::{PntsLoader, PntsLoaderOptions};

pub mod model_statistics;
pub use model_statistics::ModelStatistics;

pub mod primitive_outline_generator;
pub use primitive_outline_generator::PrimitiveOutlineGenerator;
pub mod model_utility;
pub mod render_resources;
pub use render_resources::{
    ModelRenderResources, ModelTextureUniformProviders, ModelUniformProviders, ModelUniformState, NodeRenderResources,
    PipelineResources, PrimitiveRenderResources,
};
pub mod alpha_pipeline_stage;
pub mod atmosphere_pipeline_stage;
pub mod batch_texture_pipeline_stage;
pub mod classification_pipeline_stage;
pub mod cpu_styling_pipeline_stage;
pub mod dequantization_pipeline_stage;
pub mod edge_detection_pipeline_stage;
pub mod feature_id_pipeline_stage;
pub mod geometry_pipeline_stage;
pub mod instancing_pipeline_stage;
pub mod lighting_pipeline_stage;
pub mod material_pipeline_stage;
pub mod metadata_picking_pipeline_stage;
pub mod model_clipping_planes_pipeline_stage;
pub mod model_clipping_polygons_pipeline_stage;
pub mod model_color_pipeline_stage;
pub mod model_silhouette_pipeline_stage;
pub mod model_splitter_pipeline_stage;
pub mod morph_targets_pipeline_stage;
pub mod node_statistics_pipeline_stage;
pub mod picking_pipeline_stage;
pub mod primitive_outline_pipeline_stage;
pub mod primitive_statistics_pipeline_stage;
pub mod scene_mode_2d_pipeline_stage;
pub mod selected_feature_id_pipeline_stage;
pub mod skinning_pipeline_stage;
pub mod tileset_pipeline_stage;
pub mod vertical_exaggeration_pipeline_stage;
pub mod wireframe_pipeline_stage;

pub mod model_feature;
pub mod model_feature_table;
pub use model_feature::ModelFeature;
pub use model_feature_table::{ModelFeatureStyle, ModelFeatureTable};
pub mod metadata_pipeline_stage;

pub mod mapped_positions;
pub use mapped_positions::MappedPositions;
pub mod model_animation;
pub mod model_animation_channel;
pub mod model_animation_collection;
pub use model_animation::{AnimationTimeCallback, ModelAnimation, ModelAnimationEvent, ModelAnimationOptions};
pub use model_animation_channel::{
    AnimationChannelSpline, Cartesian3Spline, ModelAnimationChannel, QuaternionAnimationSpline, ScalarSpline,
};
pub use model_animation_collection::ModelAnimationCollection;
pub mod model_articulation;
pub mod model_articulation_stage;
pub use model_articulation::ModelArticulation;
pub use model_articulation_stage::ModelArticulationStage;
pub mod model_matrix_update_stage;
pub use model_matrix_update_stage::ModelMatrixUpdateStage;
pub mod model_node;
pub use model_node::ModelNode;
pub mod model_runtime_node;
pub use model_runtime_node::{ModelRuntimeNode, NodePipelineStage, NodeUpdateStage, TransformParameters};
pub mod model_runtime_primitive;
pub use model_runtime_primitive::{FeatureIdFlags, ModelRuntimePrimitive, PrimitivePipelineStage, RuntimeDrawCommand};
pub mod model_scene_graph;
pub use model_scene_graph::{
    CustomShaderPipelineProperties, ModelBoundingVolumes, ModelPipelineProperties, ModelPipelineStage, ModelSceneGraph,
    ModelSceneGraphOptions, PointCloudShadingProperties,
};
pub mod model_skin;
pub use model_skin::ModelSkin;
