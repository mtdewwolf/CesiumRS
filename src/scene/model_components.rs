//! CesiumJS `ModelComponents`: the plain data a glTF loader produces and
//! Model consumes — attributes, indices, feature IDs, primitives, nodes,
//! skins, animations, articulations and materials.
//!
//! Defaults match the JavaScript constructors field for field: `undefined` is
//! `None`, arrays start empty, and the material factors are copies of the
//! `DEFAULT_*` constants.
//!
//! Mapping to Rust:
//! - Shared object references become indices into the array that owns them:
//!   node `children`, skin `joints`, animation target `node` and scene `nodes`
//!   index [`Components::nodes`]; a node's `skin` indexes [`Components::skins`];
//!   an animation channel's `sampler` indexes its [`Animation::samplers`].
//! - GPU buffers and textures are the Renderer's [`Buffer`] and a shared
//!   `Rc<`[`Texture`]`>` (readers share one texture, as in JavaScript), so the
//!   types that can hold them are generic over the [`Gl`] host.
//! - Values whose JavaScript type depends on the accessor type (numbers,
//!   Cartesians, matrices, quaternions) are a [`MathValue`].
//! - `type` fields are `attribute_type` / `stage_type`.
//! - StructuralMetadata and ModelPrimitiveImagery references use FrameResource
//!   handles until their typed runtime modules are ported.

use super::{AlphaMode, AttributeType, Axis, InstanceAttributeSemantic, VertexAttributeSemantic};
use crate::core::{
    ArticulationStageType, Cartesian2, Cartesian3, Cartesian4, ComponentDatatype, Credit, IndexDatatype,
    InterpolationType, Matrix2, Matrix3, Matrix4, PrimitiveType, Quaternion, TypedArray,
};
use crate::renderer::{Buffer, Gl, Texture};
use serde_json::{Map, Value};
use std::rc::Rc;

macro_rules! impl_clone_for_gl {
    ($name:ident { $($field:ident),* $(,)? }) => {
        impl<G: Gl> Clone for $name<G> {
            fn clone(&self) -> Self {
                Self {
                    $($field: self.$field.clone(),)*
                }
            }
        }
    };
}

/// A number, Cartesian, matrix or quaternion: whatever JavaScript type an
/// accessor element, a constant or an animation output reads as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MathValue {
    Number(f64),
    Cartesian2(Cartesian2),
    Cartesian3(Cartesian3),
    Cartesian4(Cartesian4),
    Matrix2(Matrix2),
    Matrix3(Matrix3),
    Matrix4(Matrix4),
    Quaternion(Quaternion),
}

impl MathValue {
    /// The value as GLSL components, in Cesium's column-major matrix order.
    pub fn to_f32_vec(self) -> Vec<f32> {
        match self {
            Self::Number(v) => vec![v as f32],
            Self::Cartesian2(v) => vec![v.x as f32, v.y as f32],
            Self::Cartesian3(v) => vec![v.x as f32, v.y as f32, v.z as f32],
            Self::Cartesian4(v) => vec![v.x as f32, v.y as f32, v.z as f32, v.w as f32],
            Self::Matrix2(v) => v.0.iter().map(|&x| x as f32).collect(),
            Self::Matrix3(v) => v.0.iter().map(|&x| x as f32).collect(),
            Self::Matrix4(v) => v.0.iter().map(|&x| x as f32).collect(),
            Self::Quaternion(v) => vec![v.x as f32, v.y as f32, v.z as f32, v.w as f32],
        }
    }

    /// The number of GLSL components the value supplies.
    pub fn component_count(self) -> usize {
        match self {
            Self::Number(_) => 1,
            Self::Cartesian2(_) => 2,
            Self::Cartesian3(_) => 3,
            Self::Cartesian4(_) | Self::Quaternion(_) => 4,
            Self::Matrix2(_) => 4,
            Self::Matrix3(_) => 9,
            Self::Matrix4(_) => 16,
        }
    }
}

/// An attribute's semantic: a vertex semantic, an instance semantic, or none
/// for custom attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeSemantic {
    Vertex(VertexAttributeSemantic),
    Instance(InstanceAttributeSemantic),
}

impl AttributeSemantic {
    /// Cesium's string value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vertex(s) => s.as_str(),
            Self::Instance(s) => s.as_str(),
        }
    }
}

/// `ModelComponents.Quantization`: how a quantized attribute dequantizes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Quantization {
    pub oct_encoded: bool,
    pub oct_encoded_zxy: bool,
    pub normalization_range: Option<MathValue>,
    pub quantized_volume_offset: Option<MathValue>,
    pub quantized_volume_dimensions: Option<MathValue>,
    pub quantized_volume_step_size: Option<MathValue>,
    pub component_datatype: Option<ComponentDatatype>,
    pub attribute_type: Option<AttributeType>,
}

/// `ModelComponents.Attribute`: a vertex or instance attribute.
pub struct Attribute<G: Gl> {
    pub name: Option<String>,
    pub semantic: Option<AttributeSemantic>,
    pub set_index: Option<u32>,
    pub component_datatype: Option<ComponentDatatype>,
    pub attribute_type: Option<AttributeType>,
    pub normalized: bool,
    pub count: Option<usize>,
    pub min: Option<MathValue>,
    pub max: Option<MathValue>,
    pub constant: Option<MathValue>,
    pub quantization: Option<Quantization>,
    pub typed_array: Option<TypedArray>,
    pub buffer: Option<Buffer<G>>,
    pub byte_offset: Option<usize>,
    pub byte_stride: Option<usize>,
}

impl_clone_for_gl!(Attribute {
    name,
    semantic,
    set_index,
    component_datatype,
    attribute_type,
    normalized,
    count,
    min,
    max,
    constant,
    quantization,
    typed_array,
    buffer,
    byte_offset,
    byte_stride,
});

impl<G: Gl> Default for Attribute<G> {
    fn default() -> Self {
        Self {
            name: None,
            semantic: None,
            set_index: None,
            component_datatype: None,
            attribute_type: None,
            normalized: false,
            count: None,
            min: None,
            max: None,
            constant: None,
            quantization: None,
            typed_array: None,
            buffer: None,
            byte_offset: Some(0),
            byte_stride: None,
        }
    }
}

/// `ModelComponents.Indices`.
pub struct Indices<G: Gl> {
    pub index_datatype: Option<IndexDatatype>,
    pub count: Option<usize>,
    pub buffer: Option<Buffer<G>>,
    pub typed_array: Option<TypedArray>,
}

impl_clone_for_gl!(Indices { index_datatype, count, buffer, typed_array });

impl<G: Gl> Default for Indices<G> {
    fn default() -> Self {
        Self { index_datatype: None, count: None, buffer: None, typed_array: None }
    }
}

/// `ModelComponents.FeatureIdAttribute`: feature IDs from a vertex attribute.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeatureIdAttribute {
    pub feature_count: Option<u32>,
    pub null_feature_id: Option<u32>,
    pub property_table_id: Option<usize>,
    pub set_index: Option<u32>,
    pub label: Option<String>,
    pub positional_label: Option<String>,
}

/// `ModelComponents.FeatureIdImplicitRange`: feature IDs `offset + floor(i / repeat)`.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureIdImplicitRange {
    pub feature_count: Option<u32>,
    pub null_feature_id: Option<u32>,
    pub property_table_id: Option<usize>,
    pub offset: f64,
    pub repeat: Option<f64>,
    pub label: Option<String>,
    pub positional_label: Option<String>,
}

impl Default for FeatureIdImplicitRange {
    fn default() -> Self {
        Self {
            feature_count: None,
            null_feature_id: None,
            property_table_id: None,
            offset: 0.0,
            repeat: None,
            label: None,
            positional_label: None,
        }
    }
}

/// `ModelComponents.FeatureIdTexture`: feature IDs read from texture channels.
pub struct FeatureIdTexture<G: Gl> {
    pub feature_count: Option<u32>,
    pub null_feature_id: Option<u32>,
    pub property_table_id: Option<usize>,
    pub texture_reader: Option<TextureReader<G>>,
    pub label: Option<String>,
    pub positional_label: Option<String>,
}

impl_clone_for_gl!(FeatureIdTexture {
    feature_count,
    null_feature_id,
    property_table_id,
    texture_reader,
    label,
    positional_label,
});

impl<G: Gl> Default for FeatureIdTexture<G> {
    fn default() -> Self {
        Self {
            feature_count: None,
            null_feature_id: None,
            property_table_id: None,
            texture_reader: None,
            label: None,
            positional_label: None,
        }
    }
}

/// One entry of a primitive's or instances' `featureIds`, which JavaScript
/// keeps as an array of the three feature ID classes.
pub enum FeatureId<G: Gl> {
    Attribute(FeatureIdAttribute),
    ImplicitRange(FeatureIdImplicitRange),
    Texture(FeatureIdTexture<G>),
}

impl<G: Gl> Clone for FeatureId<G> {
    fn clone(&self) -> Self {
        match self {
            Self::Attribute(f) => Self::Attribute(f.clone()),
            Self::ImplicitRange(f) => Self::ImplicitRange(f.clone()),
            Self::Texture(f) => Self::Texture(f.clone()),
        }
    }
}

impl<G: Gl> FeatureId<G> {
    pub fn feature_count(&self) -> Option<u32> {
        match self {
            Self::Attribute(f) => f.feature_count,
            Self::ImplicitRange(f) => f.feature_count,
            Self::Texture(f) => f.feature_count,
        }
    }

    pub fn null_feature_id(&self) -> Option<u32> {
        match self {
            Self::Attribute(f) => f.null_feature_id,
            Self::ImplicitRange(f) => f.null_feature_id,
            Self::Texture(f) => f.null_feature_id,
        }
    }

    pub fn property_table_id(&self) -> Option<usize> {
        match self {
            Self::Attribute(f) => f.property_table_id,
            Self::ImplicitRange(f) => f.property_table_id,
            Self::Texture(f) => f.property_table_id,
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Attribute(f) => f.label.as_deref(),
            Self::ImplicitRange(f) => f.label.as_deref(),
            Self::Texture(f) => f.label.as_deref(),
        }
    }

    pub fn positional_label(&self) -> Option<&str> {
        match self {
            Self::Attribute(f) => f.positional_label.as_deref(),
            Self::ImplicitRange(f) => f.positional_label.as_deref(),
            Self::Texture(f) => f.positional_label.as_deref(),
        }
    }
}

/// `ModelComponents.MorphTarget`.
pub struct MorphTarget<G: Gl> {
    pub attributes: Vec<Attribute<G>>,
}

impl_clone_for_gl!(MorphTarget { attributes });

impl<G: Gl> Default for MorphTarget<G> {
    fn default() -> Self {
        Self { attributes: Vec::new() }
    }
}

/// Edge visibility data loaded by GltfLoader's extension handler.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdgeVisibility {
    pub visibility: Vec<MathValue>,
    pub material: Option<usize>,
    pub silhouette_normals: Option<Vec<MathValue>>,
}

/// `ModelComponents.Primitive`.
pub struct Primitive<G: Gl> {
    pub attributes: Vec<Attribute<G>>,
    pub morph_targets: Vec<MorphTarget<G>>,
    pub indices: Option<Indices<G>>,
    pub material: Option<Material<G>>,
    pub primitive_type: Option<PrimitiveType>,
    pub feature_ids: Vec<FeatureId<G>>,
    pub property_texture_ids: Vec<usize>,
    pub property_attribute_ids: Vec<usize>,
    pub outline_coordinates: Option<Attribute<G>>,
    pub edge_visibility: Option<EdgeVisibility>,
    pub model_primitive_imagery: Option<super::FrameResource>,
}

impl_clone_for_gl!(Primitive {
    attributes,
    morph_targets,
    indices,
    material,
    primitive_type,
    feature_ids,
    property_texture_ids,
    property_attribute_ids,
    outline_coordinates,
    edge_visibility,
    model_primitive_imagery,
});

impl<G: Gl> Default for Primitive<G> {
    fn default() -> Self {
        Self {
            attributes: Vec::new(),
            morph_targets: Vec::new(),
            indices: None,
            material: None,
            primitive_type: None,
            feature_ids: Vec::new(),
            property_texture_ids: Vec::new(),
            property_attribute_ids: Vec::new(),
            outline_coordinates: None,
            edge_visibility: None,
            model_primitive_imagery: None,
        }
    }
}

/// `ModelComponents.Instances`: GPU instancing attributes and feature IDs.
pub struct Instances<G: Gl> {
    pub attributes: Vec<Attribute<G>>,
    pub feature_ids: Vec<FeatureId<G>>,
    pub transform_in_world_space: bool,
}

impl_clone_for_gl!(Instances { attributes, feature_ids, transform_in_world_space });

impl<G: Gl> Default for Instances<G> {
    fn default() -> Self {
        Self { attributes: Vec::new(), feature_ids: Vec::new(), transform_in_world_space: false }
    }
}

/// `ModelComponents.Skin`. `joints` index [`Components::nodes`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skin {
    pub index: Option<usize>,
    pub joints: Vec<usize>,
    pub inverse_bind_matrices: Vec<Matrix4>,
}

/// `ModelComponents.Node`. `children` index [`Components::nodes`]; `skin`
/// indexes [`Components::skins`].
pub struct Node<G: Gl> {
    pub name: Option<String>,
    pub index: Option<usize>,
    pub children: Vec<usize>,
    pub primitives: Vec<Primitive<G>>,
    pub instances: Option<Instances<G>>,
    pub skin: Option<usize>,
    pub matrix: Option<Matrix4>,
    pub translation: Option<Cartesian3>,
    pub rotation: Option<Quaternion>,
    pub scale: Option<Cartesian3>,
    pub morph_weights: Vec<f64>,
    pub articulation_name: Option<String>,
}

impl_clone_for_gl!(Node {
    name,
    index,
    children,
    primitives,
    instances,
    skin,
    matrix,
    translation,
    rotation,
    scale,
    morph_weights,
    articulation_name,
});

impl<G: Gl> Default for Node<G> {
    fn default() -> Self {
        Self {
            name: None,
            index: None,
            children: Vec::new(),
            primitives: Vec::new(),
            instances: None,
            skin: None,
            matrix: None,
            translation: None,
            rotation: None,
            scale: None,
            morph_weights: Vec::new(),
            articulation_name: None,
        }
    }
}

/// `ModelComponents.Scene`: the root nodes, as indices into [`Components::nodes`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scene {
    pub nodes: Vec<usize>,
}

string_enum!(
    /// `ModelComponents.AnimatedPropertyType`: the node property an animation channel drives.
    AnimatedPropertyType {
        Translation = "translation" => "TRANSLATION",
        Rotation = "rotation" => "ROTATION",
        Scale = "scale" => "SCALE",
        Weights = "weights" => "WEIGHTS",
    }
);

/// `ModelComponents.AnimationSampler`: keyframe times and values.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationSampler {
    pub input: Vec<f64>,
    pub interpolation: Option<InterpolationType>,
    pub output: Vec<MathValue>,
}

/// `ModelComponents.AnimationTarget`. `node` indexes [`Components::nodes`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationTarget {
    pub node: Option<usize>,
    pub path: Option<AnimatedPropertyType>,
}

/// `ModelComponents.AnimationChannel`. `sampler` indexes its [`Animation::samplers`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationChannel {
    pub sampler: Option<usize>,
    pub target: Option<AnimationTarget>,
}

/// `ModelComponents.Animation`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Animation {
    pub name: Option<String>,
    pub samplers: Vec<AnimationSampler>,
    pub channels: Vec<AnimationChannel>,
}

/// `ModelComponents.ArticulationStage` (`AGI_articulations`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArticulationStage {
    pub name: Option<String>,
    pub stage_type: Option<ArticulationStageType>,
    pub minimum_value: Option<f64>,
    pub maximum_value: Option<f64>,
    pub initial_value: Option<f64>,
}

/// `ModelComponents.Articulation`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Articulation {
    pub name: Option<String>,
    pub stages: Vec<ArticulationStage>,
}

/// `ModelComponents.Asset`: the glTF asset's credits.
#[derive(Clone, Debug, Default)]
pub struct Asset {
    pub credits: Vec<Credit>,
}

/// `ModelComponents.Components`: everything the loader produced.
pub struct Components<G: Gl> {
    pub asset: Asset,
    pub scene: Option<Scene>,
    pub nodes: Vec<Node<G>>,
    pub skins: Vec<Skin>,
    pub animations: Vec<Animation>,
    pub articulations: Vec<Articulation>,
    pub structural_metadata: Option<super::FrameResource>,
    pub up_axis: Option<Axis>,
    pub forward_axis: Option<Axis>,
    pub transform: Matrix4,
    pub extensions: Map<String, Value>,
    /// Typed NGA_gpm_local extension data.
    pub gpm_local: Option<Rc<super::model::extensions::gpm::GltfGpmLocal>>,
}

impl_clone_for_gl!(Components {
    asset,
    scene,
    nodes,
    skins,
    animations,
    articulations,
    structural_metadata,
    up_axis,
    forward_axis,
    transform,
    extensions,
    gpm_local,
});

impl<G: Gl> Default for Components<G> {
    fn default() -> Self {
        Self {
            asset: Asset::default(),
            scene: None,
            nodes: Vec::new(),
            skins: Vec::new(),
            animations: Vec::new(),
            articulations: Vec::new(),
            structural_metadata: None,
            up_axis: None,
            forward_axis: None,
            transform: Matrix4::IDENTITY,
            extensions: Map::new(),
            gpm_local: None,
        }
    }
}

/// `ModelComponents.TextureReader`: a texture and how a shader samples it.
/// `index` and `scale` are numbers from glTF JSON, so they may be undefined
/// when `GltfLoaderUtil.createModelTextureReader` copies them.
pub struct TextureReader<G: Gl> {
    pub texture: Option<Rc<Texture<G>>>,
    pub index: Option<f64>,
    pub tex_coord: f64,
    pub transform: Option<Matrix3>,
    pub scale: Option<f64>,
    pub channels: Option<String>,
}

impl_clone_for_gl!(TextureReader { texture, index, tex_coord, transform, scale, channels });

impl<G: Gl> Default for TextureReader<G> {
    fn default() -> Self {
        Self {
            texture: None,
            index: None,
            tex_coord: 0.0,
            transform: Some(Matrix3::IDENTITY),
            scale: Some(1.0),
            channels: None,
        }
    }
}

/// `ModelComponents.MetallicRoughness`.
pub struct MetallicRoughness<G: Gl> {
    pub base_color_texture: Option<TextureReader<G>>,
    pub metallic_roughness_texture: Option<TextureReader<G>>,
    pub base_color_factor: Cartesian4,
    pub metallic_factor: f64,
    pub roughness_factor: f64,
}

impl_clone_for_gl!(MetallicRoughness {
    base_color_texture,
    metallic_roughness_texture,
    base_color_factor,
    metallic_factor,
    roughness_factor,
});

impl<G: Gl> MetallicRoughness<G> {
    pub const DEFAULT_BASE_COLOR_FACTOR: Cartesian4 = Cartesian4::ONE;
    pub const DEFAULT_METALLIC_FACTOR: f64 = 1.0;
    pub const DEFAULT_ROUGHNESS_FACTOR: f64 = 1.0;
}

impl<G: Gl> Default for MetallicRoughness<G> {
    fn default() -> Self {
        Self {
            base_color_texture: None,
            metallic_roughness_texture: None,
            base_color_factor: Self::DEFAULT_BASE_COLOR_FACTOR,
            metallic_factor: Self::DEFAULT_METALLIC_FACTOR,
            roughness_factor: Self::DEFAULT_ROUGHNESS_FACTOR,
        }
    }
}

/// `ModelComponents.SpecularGlossiness` (`KHR_materials_pbrSpecularGlossiness`).
pub struct SpecularGlossiness<G: Gl> {
    pub diffuse_texture: Option<TextureReader<G>>,
    pub specular_glossiness_texture: Option<TextureReader<G>>,
    pub diffuse_factor: Cartesian4,
    pub specular_factor: Cartesian3,
    pub glossiness_factor: f64,
}

impl_clone_for_gl!(SpecularGlossiness {
    diffuse_texture,
    specular_glossiness_texture,
    diffuse_factor,
    specular_factor,
    glossiness_factor,
});

impl<G: Gl> SpecularGlossiness<G> {
    pub const DEFAULT_DIFFUSE_FACTOR: Cartesian4 = Cartesian4::ONE;
    pub const DEFAULT_SPECULAR_FACTOR: Cartesian3 = Cartesian3::ONE;
    pub const DEFAULT_GLOSSINESS_FACTOR: f64 = 1.0;
}

impl<G: Gl> Default for SpecularGlossiness<G> {
    fn default() -> Self {
        Self {
            diffuse_texture: None,
            specular_glossiness_texture: None,
            diffuse_factor: Self::DEFAULT_DIFFUSE_FACTOR,
            specular_factor: Self::DEFAULT_SPECULAR_FACTOR,
            glossiness_factor: Self::DEFAULT_GLOSSINESS_FACTOR,
        }
    }
}

/// `ModelComponents.Specular` (`KHR_materials_specular`).
pub struct Specular<G: Gl> {
    pub specular_factor: f64,
    pub specular_texture: Option<TextureReader<G>>,
    pub specular_color_factor: Cartesian3,
    pub specular_color_texture: Option<TextureReader<G>>,
}

impl_clone_for_gl!(Specular { specular_factor, specular_texture, specular_color_factor, specular_color_texture });

impl<G: Gl> Specular<G> {
    pub const DEFAULT_SPECULAR_FACTOR: f64 = 1.0;
    pub const DEFAULT_SPECULAR_COLOR_FACTOR: Cartesian3 = Cartesian3::ONE;
}

impl<G: Gl> Default for Specular<G> {
    fn default() -> Self {
        Self {
            specular_factor: Self::DEFAULT_SPECULAR_FACTOR,
            specular_texture: None,
            specular_color_factor: Self::DEFAULT_SPECULAR_COLOR_FACTOR,
            specular_color_texture: None,
        }
    }
}

/// `ModelComponents.Anisotropy` (`KHR_materials_anisotropy`).
pub struct Anisotropy<G: Gl> {
    pub anisotropy_strength: f64,
    pub anisotropy_rotation: f64,
    pub anisotropy_texture: Option<TextureReader<G>>,
}

impl_clone_for_gl!(Anisotropy { anisotropy_strength, anisotropy_rotation, anisotropy_texture });

impl<G: Gl> Anisotropy<G> {
    pub const DEFAULT_ANISOTROPY_STRENGTH: f64 = 0.0;
    pub const DEFAULT_ANISOTROPY_ROTATION: f64 = 0.0;
}

impl<G: Gl> Default for Anisotropy<G> {
    fn default() -> Self {
        Self {
            anisotropy_strength: Self::DEFAULT_ANISOTROPY_STRENGTH,
            anisotropy_rotation: Self::DEFAULT_ANISOTROPY_ROTATION,
            anisotropy_texture: None,
        }
    }
}

/// `ModelComponents.Clearcoat` (`KHR_materials_clearcoat`).
pub struct Clearcoat<G: Gl> {
    pub clearcoat_factor: f64,
    pub clearcoat_texture: Option<TextureReader<G>>,
    pub clearcoat_roughness_factor: f64,
    pub clearcoat_roughness_texture: Option<TextureReader<G>>,
    pub clearcoat_normal_texture: Option<TextureReader<G>>,
}

impl_clone_for_gl!(Clearcoat {
    clearcoat_factor,
    clearcoat_texture,
    clearcoat_roughness_factor,
    clearcoat_roughness_texture,
    clearcoat_normal_texture,
});

impl<G: Gl> Clearcoat<G> {
    pub const DEFAULT_CLEARCOAT_FACTOR: f64 = 0.0;
    pub const DEFAULT_CLEARCOAT_ROUGHNESS_FACTOR: f64 = 0.0;
}

impl<G: Gl> Default for Clearcoat<G> {
    fn default() -> Self {
        Self {
            clearcoat_factor: Self::DEFAULT_CLEARCOAT_FACTOR,
            clearcoat_texture: None,
            clearcoat_roughness_factor: Self::DEFAULT_CLEARCOAT_ROUGHNESS_FACTOR,
            clearcoat_roughness_texture: None,
            clearcoat_normal_texture: None,
        }
    }
}

/// `ModelComponents.Material`.
pub struct Material<G: Gl> {
    pub metallic_roughness: MetallicRoughness<G>,
    pub specular_glossiness: Option<SpecularGlossiness<G>>,
    pub specular: Option<Specular<G>>,
    pub anisotropy: Option<Anisotropy<G>>,
    pub clearcoat: Option<Clearcoat<G>>,
    pub emissive_texture: Option<TextureReader<G>>,
    pub normal_texture: Option<TextureReader<G>>,
    pub occlusion_texture: Option<TextureReader<G>>,
    pub emissive_factor: Cartesian3,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: Option<f64>,
    pub double_sided: bool,
    pub unlit: bool,
    pub point_diameter: Option<f64>,
}

impl_clone_for_gl!(Material {
    metallic_roughness,
    specular_glossiness,
    specular,
    anisotropy,
    clearcoat,
    emissive_texture,
    normal_texture,
    occlusion_texture,
    emissive_factor,
    alpha_mode,
    alpha_cutoff,
    double_sided,
    unlit,
    point_diameter,
});

impl<G: Gl> Material<G> {
    pub const DEFAULT_EMISSIVE_FACTOR: Cartesian3 = Cartesian3::ZERO;
}

impl<G: Gl> Default for Material<G> {
    fn default() -> Self {
        Self {
            metallic_roughness: MetallicRoughness::default(),
            specular_glossiness: None,
            specular: None,
            anisotropy: None,
            clearcoat: None,
            emissive_texture: None,
            normal_texture: None,
            occlusion_texture: None,
            emissive_factor: Self::DEFAULT_EMISSIVE_FACTOR,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: Some(0.5),
            double_sided: false,
            unlit: false,
            point_diameter: None,
        }
    }
}
