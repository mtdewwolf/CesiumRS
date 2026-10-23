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
            pub fn as_str(self) -> &'static str { match self { $(Self::$variant => $value),+ } }
            pub fn from_str_value(value: &str) -> Option<Self> { Self::ALL.iter().copied().find(|v| v.as_str() == value) }
            /// The Cesium constant name.
            pub fn name(self) -> &'static str { match self { $(Self::$variant => $key),+ } }
        }
    };
}

pub mod alpha_mode;
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
pub mod camera_event_type;
pub mod cesium_3d_tile_color_blend_mode;
pub mod cesium_3d_tile_content_state;
pub mod cesium_3d_tile_content_type;
pub mod cesium_3d_tile_optimization_hint;
pub mod cesium_3d_tile_pass;
pub mod cesium_3d_tile_refine;
pub mod classification_type;
pub mod cloud_type;
pub mod color_blend_mode;
pub mod cull_face;
pub mod depth_function;
pub mod directional_light;
pub mod dynamic_atmosphere_lighting_type;
pub mod expression_node_type;
pub mod get_binary_accessor;
pub mod get_clip_and_style_code;
pub mod get_metadata_class_property;
pub mod gltf_pipeline;
pub mod has_extension;
pub mod height_reference;
pub mod horizontal_origin;
pub mod imagery_state;
pub mod implicit_subdivision_scheme;
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
pub mod tile_discard_policy;
pub mod tile_selection_result;
pub mod tile_state;
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
pub use camera_event_type::CameraEventType;
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
pub use get_binary_accessor::get_binary_accessor;
pub use get_clip_and_style_code::get_clip_and_style_code;
pub use get_metadata_class_property::get_metadata_class_property;
pub use has_extension::has_extension;
pub use height_reference::{is_height_reference_clamp, is_height_reference_relative};
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
pub use property_table::{PropertyTable, PropertyTableOptions};
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
