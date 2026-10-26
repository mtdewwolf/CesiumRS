//! `Scene/Model`: the glTF model runtime. One Rust module per Cesium module.

pub mod cartesian_rectangle;
pub mod custom_shader;
pub mod custom_shader_mode;
pub mod custom_shader_pipeline_stage;
pub mod custom_shader_translucency_mode;
pub mod extensions;
pub mod imagery_configuration;
pub mod imagery_flags;
pub mod lighting_model;
pub mod model_alpha_options;
pub mod model_lighting_options;
pub mod model_reader;
pub mod model_type;
pub mod style_commands_needed;
pub mod texture_manager;
pub mod texture_uniform;
pub mod uniform_type;
pub mod varying_type;

pub use cartesian_rectangle::CartesianRectangle;
pub use custom_shader::{
    CustomShader, CustomShaderOptions, CustomShaderUniformValue, FragmentVariableSets, UniformSpecifier, VariableSet,
    VertexVariableSets,
};
pub use custom_shader_mode::CustomShaderMode;
pub use custom_shader_pipeline_stage::{CustomShaderWarning, CustomShaderWarnings};
pub use custom_shader_translucency_mode::CustomShaderTranslucencyMode;
pub use edge_visibility_pipeline_stage as edge_visibility_stage;
pub use image_based_lighting_pipeline_stage::ImageBasedLightingOptions;
pub use imagery_configuration::{ImageryConfiguration, ImageryLayerSettings};
pub use imagery_flags::ImageryFlags;
pub use lighting_model::LightingModel;
pub use model_alpha_options::ModelAlphaOptions;
pub use model_lighting_options::ModelLightingOptions;
pub use model_reader::ModelReader;
pub use model_type::ModelType;
pub use point_cloud_styling_pipeline_stage::{PointCloudStyleFunctions, PointCloudStylingOptions};
pub use style_commands_needed::StyleCommandsNeeded;
pub use texture_manager::{sampler_requires_mipmap, LoadedImageSource, TextureManager, TextureManagerWarning};
pub use texture_uniform::{TextureUniform, TextureUniformOptions};
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

// Keep the upstream Model module name and its public import path.
#[allow(clippy::module_inception)]
pub mod model;
pub mod model_3d_tile_content;
pub mod model_statistics;
pub use model::{Model, ModelLoader, ModelOptions, ModelUpdateServices};
pub use model_3d_tile_content::{Model3DTileContent, ModelTileFactory};
pub mod model_draw_command;
pub mod model_draw_commands;
pub mod pick_model;
pub use pick_model::{pick_model, ModelPickOptions};
pub mod classification_model_draw_command;
pub use classification_model_draw_command::ClassificationModelDrawCommand;
pub use model_draw_command::{ModelCommand, ModelCommandSelection, ModelDrawCommand, ModelDrawCommandOptions};
pub use model_draw_commands::ModelDrawCommandKind;
pub use model_statistics::ModelStatistics;

pub mod primitive_outline_generator;
pub use primitive_outline_generator::PrimitiveOutlineGenerator;
pub mod model_utility;
pub mod render_resources;
pub use render_resources::{
    EdgeGeometry, ModelRenderResources, ModelTextureUniformProviders, ModelUniformProviders, ModelUniformState,
    NodeRenderResources, PipelineResources, PrimitiveRenderResources,
};
pub mod alpha_pipeline_stage;
pub mod atmosphere_pipeline_stage;
pub mod batch_texture_pipeline_stage;
pub mod classification_pipeline_stage;
pub mod cpu_styling_pipeline_stage;
pub mod dequantization_pipeline_stage;
pub mod edge_detection_pipeline_stage;
pub mod edge_visibility_pipeline_stage;
pub mod feature_id_pipeline_stage;
pub mod geometry_pipeline_stage;
pub mod image_based_lighting_pipeline_stage;
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
pub mod point_cloud_styling_pipeline_stage;
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
pub mod imagery_coverage;
pub mod imagery_input;
pub mod imagery_pipeline_stage;
pub mod model_imagery;
pub mod model_imagery_mapping;
pub mod model_primitive_imagery;

pub use imagery_coverage::ImageryCoverage;
pub use imagery_input::ImageryInput;
pub use imagery_pipeline_stage::ImageryPipelineStage;
pub use model_imagery::ModelImagery;
pub use model_imagery_mapping::ModelImageryMapping;
pub use model_primitive_imagery::ModelPrimitiveImagery;
