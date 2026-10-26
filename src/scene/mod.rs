//! Port of the CesiumJS `Scene` package (`@cesium/engine` 23.0.0, CesiumJS 1.139).
//!
//! One Rust module per Cesium module, named in snake_case, as in [`core`](crate::core).
//! Cesium enums whose values are numbers or strings become Rust enums with
//! `value()` / `as_str()`, `ALL` and the Cesium constant `name()`. Functions
//! that Cesium guards with a `DeveloperError` take the enum, so the invalid
//! values cannot be passed.

/// A Cesium enum of numbers. `$key` is the Cesium constant name.
macro_rules! numeric_enum {
    ($(#[$meta:meta])* $name:ident: $repr:ty { $($variant:ident = $value:expr => $key:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr($repr)]
        pub enum $name { $($variant = $value),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            /// Cesium's value.
            pub const fn value(self) -> $repr { self as $repr }
            pub fn from_value(value: $repr) -> Option<Self> { Self::ALL.iter().copied().find(|v| v.value() == value) }
            /// The Cesium constant name.
            pub fn name(self) -> &'static str { match self { $(Self::$variant => $key),+ } }
        }
    };
}

/// A Cesium enum of strings. `$value` is Cesium's value and `$key` the
/// constant name.
macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $value:literal => $key:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
            /// Cesium's value.
            pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $value),+ } }
            pub fn from_str_value(value: &str) -> Option<Self> { Self::ALL.iter().copied().find(|v| v.as_str() == value) }
            /// The Cesium constant name.
            pub fn name(self) -> &'static str { match self { $(Self::$variant => $key),+ } }
        }
    };
}

pub mod batch_texture;
pub use batch_texture::BatchTexture;
pub mod alpha_mode;
pub mod clipping_plane;
pub mod clipping_plane_collection;
pub mod clipping_plane_shader;
pub mod tileset_stencil;
pub use clipping_plane::{ClippingPlane, ClippingPlaneHandle, PlaneChangeCallback};
pub use clipping_plane_collection::{ClippingPlaneCollection, ClippingPlaneEvent};
pub use clipping_plane_shader::{ClippingShaderKey, SceneClippingPlanes};
pub use tileset_stencil::TilesetDrawKind;
mod tileset_expansion;
pub use tileset_expansion::derive_implicit_volume;
mod tileset_content;
pub use tileset_content::{
    HeightSubscription, ImplicitContentRequest, MultipleTilesetContent, RenderableTileFactory, TileContentFailure,
    TileExpansion, TilesetResourceLoader,
};
mod tileset_traversal;
pub use tileset_traversal::{TileTraversalState, TilesetVolume};
pub mod cesium_3d_tileset;
pub use cesium_3d_tileset::{
    Cesium3DTileset, FeatureIdLabel, TileContentFuture, TileContentMetrics, TileContentRequest, TileId, TilesetCache,
    TilesetContent, TilesetContentLoader, TilesetEvent, TilesetOptions, TilesetPassState, TilesetStatistics,
    TilesetTile, TilesetTraversalKind,
};
pub mod atmosphere;
pub mod derived_command;
pub use derived_command::{DerivedCommand, PickMetadataShader, SceneDrawMode};
pub mod edge_framebuffer;
pub mod fog;
pub mod frame_state;
mod framebuffer_helpers;
pub mod pick_depth;
pub mod pick_depth_framebuffer;
pub mod scene_framebuffer;
pub use edge_framebuffer::EdgeFramebuffer;
pub use pick_depth::PickDepth;
pub mod globe_depth;
pub mod globe_translucency_framebuffer;
pub use globe_depth::GlobeDepth;
pub use globe_translucency_framebuffer::GlobeTranslucencyFramebuffer;
pub mod view_resources;
pub use pick_depth_framebuffer::PickDepthFramebuffer;
pub use scene_framebuffer::SceneFramebuffer;
pub use view_resources::ViewResources;
pub mod oit;
pub mod translucent_tile_classification;
pub use oit::{Oit, OitDrawKind};
pub use translucent_tile_classification::TranslucentTileClassification;
pub mod metadata_picking;
pub use metadata_picking::MetadataPicking;
pub mod pick_object;
pub use pick_object::{PickExclusion, PickedMetadataInfo, RayPickResult, ScenePickObject};
mod detailed_picking;
pub use detailed_picking::{MostDetailedPickRequest, SampledHeight};
pub mod scene_transforms;
pub use scene_transforms::{SceneTransformState, SceneTransforms};
pub mod pick_framebuffer;
pub mod picking;
pub use pick_framebuffer::{PickFramebuffer, PickReadback, PickedObject};
pub use picking::Picking;
// Keep the upstream Scene/Scene.js module path.
#[allow(clippy::module_inception)]
pub mod scene;
pub mod view;
pub use atmosphere::Atmosphere;
pub use fog::{Fog, FrameFog};
pub use frame_state::{FramePasses, FrameResource, FrameShadowState, FrameState};
pub use scene::{
    RendererSceneBackend, Scene, SceneBackend, SceneCommand, SceneCommandFilter, SceneComponent, SceneDrawOptions,
    SceneEventKind, SceneNotification, SceneOptions, ScenePrimitive, SceneRenderRequest, SceneStage, SceneSubmission,
};
pub use view::{DebugFrustumStatistics, FrustumCommands, SceneView};
pub mod arc_gis_base_map_type;
pub mod attribute_type;
pub mod axis;
pub mod batch_table_hierarchy;
pub mod billboard_load_state;
pub mod bing_maps_style;
pub mod blend_equation;
pub mod blend_function;
pub mod blend_option;
pub mod blending_state;
pub mod bounding_volume_semantics;
pub mod camera;
pub mod camera_event_type;
pub mod camera_flight_path;
pub mod cesium_3d_tile_color_blend_mode;
pub mod cesium_3d_tile_content_state;
pub mod cesium_3d_tile_content_type;
pub mod cesium_3d_tile_optimization_hint;
pub mod cesium_3d_tile_pass;
pub mod cesium_3d_tile_refine;
pub mod cesium_3d_tileset_metadata;
pub mod classification_type;
pub mod cloud_type;
pub mod color_blend_mode;
pub mod content_metadata;
pub mod cull_face;
pub mod depth_function;
pub mod directional_light;
pub mod dynamic_atmosphere_lighting_type;
pub mod expression_node_type;
pub mod find_content_metadata;
pub mod find_group_metadata;
pub mod find_tile_metadata;
pub mod get_binary_accessor;
pub mod get_clip_and_style_code;
pub mod get_metadata_class_property;
pub mod gltf_pipeline;
pub mod group_metadata;
pub mod has_extension;
pub mod height_reference;
pub mod horizontal_origin;
pub mod imagery_state;
pub mod implicit_availability_bitstream;
pub mod implicit_metadata_view;
pub mod implicit_subdivision_scheme;
pub mod implicit_subtree;
pub mod implicit_subtree_cache;
pub mod implicit_subtree_metadata;
pub mod implicit_tile_coordinates;
pub mod implicit_tileset;
pub mod instance_attribute_semantic;
pub mod ion_world_imagery_style;
pub mod job_type;
pub mod json_metadata_table;
pub mod label_style;
pub mod light;
pub mod map_mode_2d;
pub mod metadata_class;
pub mod metadata_class_property;
pub mod metadata_component_type;
pub mod metadata_entity;
pub mod metadata_enum;
pub mod metadata_enum_value;
pub mod metadata_schema;
pub mod metadata_semantic;
pub mod metadata_table;
pub mod metadata_table_property;
pub mod metadata_type;
pub mod metadata_value;
pub mod model;
pub mod model_animation_loop;
pub mod model_animation_state;
pub mod never_tile_discard_policy;
pub mod particle_burst;
pub mod post_process_stage_sample_mode;
pub mod property_attribute;
pub mod property_attribute_property;
pub mod property_table;
pub mod quadtree_occluders;
pub mod quadtree_tile_load_state;
pub mod resource_loader_state;
pub mod scene_mode;
pub mod sdf_settings;
pub mod sensor_volume_portion_to_display;
pub mod shadow_mode;
pub mod split_direction;
pub mod splitter;
pub mod stencil_constants;
pub mod stencil_function;
pub mod stencil_operation;
pub mod sun_light;
pub mod supported_image_formats;
pub mod terrain_state;
pub mod tile_bounding_region;
pub mod tile_bounding_s2_cell;
pub mod tile_bounding_sphere;
pub mod tile_bounding_volume;
pub mod tile_discard_policy;
pub mod tile_metadata;
pub mod tile_oriented_bounding_box;
pub mod tile_selection_result;
pub mod tile_state;
pub mod tileset_metadata;
pub mod tonemapper;
pub mod vertex_attribute_semantic;
pub mod vertical_origin;
pub mod voxel_metadata_order;

pub use alpha_mode::AlphaMode;
pub use arc_gis_base_map_type::ArcGisBaseMapType;
pub use attribute_type::AttributeType;
pub use axis::Axis;
pub use billboard_load_state::BillboardLoadState;
pub use bing_maps_style::BingMapsStyle;
pub use blend_equation::BlendEquation;
pub use blend_function::BlendFunction;
pub use blend_option::BlendOption;
pub use camera::{
    Camera, CameraCorrectionTween, CameraDestination, CameraError, CameraFlightOptions, CameraFrustum, CameraOffset,
    CameraOptions, CameraOrientation, CameraSurface, CameraView,
};
pub use camera_event_type::CameraEventType;
pub use camera_flight_path::{
    CameraEasing, CameraFlightCallback, CameraFlightPath, CameraFlightPathOptions, CameraFlightTween,
};
pub use cesium_3d_tile_color_blend_mode::Cesium3DTileColorBlendMode;
pub use cesium_3d_tile_content_state::Cesium3DTileContentState;
pub use cesium_3d_tile_content_type::Cesium3DTileContentType;
pub use cesium_3d_tile_optimization_hint::Cesium3DTileOptimizationHint;
pub use cesium_3d_tile_pass::{Cesium3DTilePass, PassOptions};
pub use cesium_3d_tile_refine::Cesium3DTileRefine;
pub use classification_type::ClassificationType;
pub use cloud_type::CloudType;
pub use color_blend_mode::ColorBlendMode;
pub use cull_face::CullFace;
pub use depth_function::DepthFunction;
pub use directional_light::{DirectionalLight, DirectionalLightOptions};
pub use dynamic_atmosphere_lighting_type::{DynamicAtmosphereLightingType, GlobeLightingFlags};
pub use expression_node_type::ExpressionNodeType;
pub use get_binary_accessor::{BinaryAccessor, ComponentType};
pub use height_reference::HeightReference;
pub use horizontal_origin::HorizontalOrigin;
pub use imagery_state::ImageryState;
pub use implicit_subdivision_scheme::ImplicitSubdivisionScheme;
pub use instance_attribute_semantic::InstanceAttributeSemantic;
pub use ion_world_imagery_style::IonWorldImageryStyle;
pub use job_type::JobType;
pub use label_style::LabelStyle;
pub use light::Light;
pub use map_mode_2d::MapMode2D;
pub use metadata_component_type::{BigIntRangeError, MetadataComponentType, MetadataNumber, ScalarCategory};
pub use metadata_semantic::MetadataSemantic;
pub use metadata_type::MetadataType;
pub use model_animation_loop::ModelAnimationLoop;
pub use model_animation_state::ModelAnimationState;
pub use never_tile_discard_policy::NeverTileDiscardPolicy;
pub use particle_burst::{ParticleBurst, ParticleBurstOptions};
pub use post_process_stage_sample_mode::PostProcessStageSampleMode;
pub use quadtree_occluders::QuadtreeOccluders;
pub use quadtree_tile_load_state::QuadtreeTileLoadState;
pub use resource_loader_state::ResourceLoaderState;
pub use scene_mode::SceneMode;
pub use sensor_volume_portion_to_display::SensorVolumePortionToDisplay;
pub use shadow_mode::ShadowMode;
pub use split_direction::SplitDirection;
pub use stencil_function::StencilFunction;
pub use stencil_operation::StencilOperation;
pub use sun_light::{SunLight, SunLightOptions};
pub use supported_image_formats::SupportedImageFormats;
pub use terrain_state::TerrainState;
pub use tile_discard_policy::TileDiscardPolicy;
pub use tile_selection_result::TileSelectionResult;
pub use tile_state::TileState;
pub use tonemapper::Tonemapper;
pub use vertex_attribute_semantic::VertexAttributeSemantic;
pub use vertical_origin::VerticalOrigin;
pub use voxel_metadata_order::VoxelMetadataOrder;

pub use batch_table_hierarchy::BatchTableHierarchy;
pub use bounding_volume_semantics::{BoundingVolumePrefix, BoundingVolumeSemanticValues, SemanticBoundingVolume};
pub use cesium_3d_tileset_metadata::Cesium3DTilesetMetadata;
pub use content_metadata::ContentMetadata;
pub use find_content_metadata::find_content_metadata;
pub use find_group_metadata::find_group_metadata;
pub use find_tile_metadata::find_tile_metadata;
pub use get_binary_accessor::get_binary_accessor;
pub use get_clip_and_style_code::get_clip_and_style_code;
pub use get_metadata_class_property::get_metadata_class_property;
pub use group_metadata::GroupMetadata;
pub use has_extension::has_extension;
pub use height_reference::{is_height_reference_clamp, is_height_reference_relative};
pub use implicit_availability_bitstream::{ImplicitAvailabilityBitstream, ImplicitAvailabilityBitstreamOptions};
pub use implicit_metadata_view::ImplicitMetadataView;
pub use implicit_subtree::{ImplicitSubtree, SubtreeSource};
pub use implicit_subtree_cache::{CachedSubtree, ImplicitSubtreeCache, ImplicitSubtreeCacheNode};
pub use implicit_subtree_metadata::ImplicitSubtreeMetadata;
pub use implicit_tile_coordinates::ImplicitTileCoordinates;
pub use implicit_tileset::ImplicitTileset;
pub use json_metadata_table::JsonMetadataTable;
pub use metadata_class::MetadataClass;
pub use metadata_class_property::{MetadataClassProperty, MetadataClassPropertyOptions};
pub use metadata_entity::{MetadataEntity, MetadataProperties};
pub use metadata_enum::MetadataEnum;
pub use metadata_enum_value::MetadataEnumValue;
pub use metadata_schema::MetadataSchema;
pub use metadata_table::MetadataTable;
pub use metadata_table_property::{ByteView, MetadataTableProperty, MetadataTypedArray};
pub use metadata_value::{JsMap, MetadataError, MetadataValue, Numeric};
pub use property_attribute::PropertyAttribute;
pub use property_attribute_property::PropertyAttributeProperty;
pub use property_table::{MetadataId, PropertyTable, PropertyTableOptions};
pub use tile_bounding_region::{TileBoundingRegion, TileBoundingRegionOptions};
pub use tile_bounding_s2_cell::{TileBoundingS2Cell, TileBoundingS2CellOptions};
pub use tile_bounding_sphere::TileBoundingSphere;
pub use tile_bounding_volume::{DebugVolume, TileBoundingVolume, TileBoundingVolumeFrame};
pub use tile_metadata::TileMetadata;
pub use tile_oriented_bounding_box::TileOrientedBoundingBox;
pub use tileset_metadata::TilesetMetadata;
pub use tonemapper::validate_tonemapper;

/// The Cesium class a typed value reads as, which `AttributeType` and
/// `MetadataType` return from `getMathType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MathType {
    Number,
    Cartesian2,
    Cartesian3,
    Cartesian4,
    Matrix2,
    Matrix3,
    Matrix4,
}

impl MathType {
    /// The JavaScript constructor's `name`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Number => "Number",
            Self::Cartesian2 => "Cartesian2",
            Self::Cartesian3 => "Cartesian3",
            Self::Cartesian4 => "Cartesian4",
            Self::Matrix2 => "Matrix2",
            Self::Matrix3 => "Matrix3",
            Self::Matrix4 => "Matrix4",
        }
    }
}

/// glTF's `/^(\w+)_\d+$/`: `TEXCOORD_0` is `TEXCOORD`. Other names are
/// returned as they are.
pub(crate) fn strip_set_index(semantic: &str) -> &str {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    match semantic.rsplit_once('_') {
        Some((prefix, index))
            if !prefix.is_empty()
                && !index.is_empty()
                && index.chars().all(|c| c.is_ascii_digit())
                && prefix.chars().all(word) =>
        {
            prefix
        }
        _ => semantic,
    }
}

pub mod resource_loader;
pub use resource_loader::{ResourceLoadFuture, ResourceLoader};
pub mod buffer_loader;
pub use buffer_loader::BufferLoader;
pub mod metadata_schema_loader;
pub use metadata_schema_loader::MetadataSchemaLoader;

pub mod gltf_json_loader;
pub use gltf_json_loader::{GltfJsonInput, GltfJsonLoader, LoadedGltfJson};

pub mod gltf_buffer_view_loader;
pub use gltf_buffer_view_loader::GltfBufferViewLoader;

pub mod gltf_loader_util;
pub use gltf_loader_util::{GltfLoaderUtil, ModelTextureReader};
pub mod resource_cache_key;
pub use resource_cache_key::{BufferKeyOptions, GltfKeyContext, ResourceCacheKey, VertexKeySource};
pub mod resource_cache_statistics;
pub use resource_cache_statistics::ResourceCacheStatistics;

pub mod resource_cache;
pub use resource_cache::{CacheLease, CacheResourceKind, ResourceCache, WeakResourceCache};

pub mod gltf_draco_loader;
pub use gltf_draco_loader::{DracoDecodeWorker, GltfDracoLoader};
pub mod spz;
pub use spz::{load_spz, GaussianCloud};
pub mod gltf_spz_loader;
pub use gltf_spz_loader::GltfSpzLoader;

pub mod gltf_geometry_loader;
pub use gltf_geometry_loader::{
    BufferUploadBudget, GltfIndexBufferLoader, GltfResource, GltfVertexBufferLoader, GpuBufferOptions,
    ModelQuantization, VertexBufferSource,
};

pub mod draco_loader;
pub mod model_components;
pub use draco_loader::{DracoLoader, DracoTask};
pub use gltf_draco_loader::DecodedDraco;
pub use resource_cache_key::{IndexBufferKeyOptions, SchemaSource, VertexBufferKeyOptions};

pub mod cesium_3d_tile_feature_table;
pub use cesium_3d_tile_feature_table::{Cesium3DTileFeatureTable, GlobalProperty};
pub mod b3dm_parser;
pub use b3dm_parser::{ParsedB3dm, TileFormatWarning};
pub mod i3dm_parser;
pub use i3dm_parser::ParsedI3dm;
pub mod pnts_parser;
pub use pnts_parser::{ParsedPnts, PntsAttribute, PntsDraco};

pub mod gltf_image_loader;
pub use gltf_image_loader::{GltfImage, GltfImageLoader, LoadedGltfImage};

pub mod gltf_texture_loader;
pub use gltf_texture_loader::{GltfTextureLoader, GltfTextureOptions, ModelImageHost, ResourceUploadBudget};

pub mod property_texture_property;
pub use property_texture_property::{GltfTextures, PropertyTextureProperty};
pub mod property_texture;
pub use property_texture::PropertyTexture;
pub mod structural_metadata;
pub use structural_metadata::{StructuralMetadata, StructuralMetadataOptions, StructuralMetadataWarning};
pub mod get_metadata_property;
pub use get_metadata_property::get_metadata_property;
pub mod parse_structural_metadata;
pub use parse_structural_metadata::{parse_structural_metadata, ParseStructuralMetadataOptions};
pub mod parse_feature_metadata_legacy;
pub use parse_feature_metadata_legacy::{parse_feature_metadata_legacy, ParseFeatureMetadataLegacyOptions};
pub mod parse_batch_table;
pub use parse_batch_table::{parse_batch_table, ParseBatchTableOptions};
pub mod preprocess_3d_tile_content;
pub use preprocess_3d_tile_content::{preprocess_3d_tile_content, PreprocessedTileContent, TileContentPayload};
pub mod tile_replacement_queue;
pub use tile_replacement_queue::TileReplacementQueue;
pub mod cesium_3d_content_group;
pub use cesium_3d_content_group::Cesium3DContentGroup;
pub mod particle;
pub use particle::{Particle, ParticleOptions, ParticleUpdateFunction};
pub mod particle_emitter;
pub use particle_emitter::{BoxEmitter, CircleEmitter, ConeEmitter, ParticleEmitter, SphereEmitter};
pub mod point_cloud_shading;
pub use point_cloud_shading::{PointCloudShading, PointCloudShadingOptions};
pub mod imagery_layer_feature_info;
pub use imagery_layer_feature_info::ImageryLayerFeatureInfo;
pub mod keyframe_node;
pub use keyframe_node::{KeyframeLoadState, KeyframeNode};
pub mod discard_empty_tile_image_policy;
pub use discard_empty_tile_image_policy::DiscardEmptyTileImagePolicy;
pub mod gltf_structural_metadata_loader;
pub use gltf_structural_metadata_loader::{GltfStructuralMetadataLoader, GltfStructuralMetadataLoaderOptions};

pub mod get_mesh_primitives;
pub use get_mesh_primitives::get_mesh_primitives;
pub mod primitive_load_plan;
pub use primitive_load_plan::{AttributeLoadPlan, IndicesLoadPlan, PlannedAttribute, PrimitiveLoadPlan};

pub mod gltf_loader;
pub use gltf_loader::{GltfLoader, GltfLoaderOptions};

// Interaction, primitives and effects.
pub mod appearance;
pub use appearance::{default_render_state, Appearance, AppearanceState};
pub mod clipping_polygon;
pub use clipping_polygon::{ClippingPolygon, ClippingPolygonOptions};
pub mod clipping_polygon_collection;
pub use clipping_polygon_collection::{
    ClippingPolygonCollection, ClippingPolygonCollectionOptions, ClippingPolygonEvent, PackedClippingPolygons,
};
pub mod get_clipping_function;
pub mod material;
pub use material::{
    Fabric, Material, MaterialCache, MaterialCubeMap, MaterialImage, MaterialOptions, MaterialValue, TranslucentRule,
};
pub mod material_appearance;
pub use material_appearance::{MaterialAppearance, MaterialAppearanceOptions, MaterialSupport};
pub mod ellipsoid_surface_appearance;
pub use ellipsoid_surface_appearance::{EllipsoidSurfaceAppearance, EllipsoidSurfaceAppearanceOptions};
pub mod per_instance_color_appearance;
pub use per_instance_color_appearance::{PerInstanceColorAppearance, PerInstanceColorAppearanceOptions};
pub mod polyline_color_appearance;
pub use polyline_color_appearance::{PolylineColorAppearance, PolylineColorAppearanceOptions};
pub mod polyline_material_appearance;
pub use polyline_material_appearance::{PolylineMaterialAppearance, PolylineMaterialAppearanceOptions};
pub mod debug_appearance;
pub use debug_appearance::{DebugAppearance, DebugAppearanceOptions};
pub mod primitive_state;
pub use primitive_state::PrimitiveState;
pub mod primitive_pipeline;
pub use primitive_pipeline::{CombineGeometryParameters, CombineGeometryResults, PickOffset, PrimitivePipeline};
pub mod primitive;
pub use primitive::{Primitive, PrimitiveOptions};
pub mod primitive_collection;
pub use primitive_collection::{PrimitiveCollection, PrimitiveCollectionOptions};
pub mod shadow_volume_appearance;
pub use shadow_volume_appearance::{ShaderDependencies, ShadowVolumeAppearance};
