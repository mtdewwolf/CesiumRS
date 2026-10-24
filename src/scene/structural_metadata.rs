//! CesiumJS `StructuralMetadata`: a model's `EXT_structural_metadata` (or
//! legacy `EXT_feature_metadata`, or 3D Tiles 1.0 batch table) metadata: its
//! schema and the property tables, textures and attributes that use it.
//!
//! `destroy` is `Drop`: the property tables' textures are released with
//! their last reference.

use super::{MetadataSchema, PropertyAttribute, PropertyTable, PropertyTexture};
use crate::renderer::Gl;
use serde_json::Value;
use std::sync::Arc;

pub struct StructuralMetadataOptions<G: Gl> {
    pub schema: Arc<MetadataSchema>,
    /// `None` is `undefined`, which counts zero tables.
    pub property_tables: Option<Vec<PropertyTable>>,
    pub property_textures: Vec<PropertyTexture<G>>,
    pub property_attributes: Vec<PropertyAttribute>,
    pub statistics: Option<Value>,
    pub extras: Option<Value>,
    pub extensions: Option<Value>,
}

pub struct StructuralMetadata<G: Gl> {
    schema: Arc<MetadataSchema>,
    property_table_count: usize,
    property_tables: Vec<PropertyTable>,
    property_textures: Vec<PropertyTexture<G>>,
    property_attributes: Vec<PropertyAttribute>,
    statistics: Option<Value>,
    extras: Option<Value>,
    extensions: Option<Value>,
}

impl<G: Gl> StructuralMetadata<G> {
    pub fn new(options: StructuralMetadataOptions<G>) -> Self {
        let property_tables = options.property_tables.unwrap_or_default();
        Self {
            schema: options.schema,
            property_table_count: property_tables.len(),
            property_tables,
            property_textures: options.property_textures,
            property_attributes: options.property_attributes,
            statistics: options.statistics,
            extras: options.extras,
            extensions: options.extensions,
        }
    }

    pub fn schema(&self) -> &Arc<MetadataSchema> {
        &self.schema
    }

    pub fn statistics(&self) -> Option<&Value> {
        self.statistics.as_ref()
    }

    pub fn extras(&self) -> Option<&Value> {
        self.extras.as_ref()
    }

    pub fn extensions(&self) -> Option<&Value> {
        self.extensions.as_ref()
    }

    pub fn property_table_count(&self) -> usize {
        self.property_table_count
    }

    pub fn property_tables(&self) -> &[PropertyTable] {
        &self.property_tables
    }

    pub fn property_textures(&self) -> &[PropertyTexture<G>] {
        &self.property_textures
    }

    pub fn property_attributes(&self) -> &[PropertyAttribute] {
        &self.property_attributes
    }

    /// The bytes of every property table's binary data.
    pub fn property_tables_byte_length(&self) -> usize {
        self.property_tables.iter().map(PropertyTable::byte_length).sum()
    }

    pub fn get_property_table(&self, property_table_id: usize) -> Option<&PropertyTable> {
        self.property_tables.get(property_table_id)
    }

    /// Reading values caches them, so property reads need the table mutably.
    pub fn get_property_table_mut(&mut self, property_table_id: usize) -> Option<&mut PropertyTable> {
        self.property_tables.get_mut(property_table_id)
    }

    pub fn get_property_texture(&self, property_texture_id: usize) -> Option<&PropertyTexture<G>> {
        self.property_textures.get(property_texture_id)
    }

    pub fn get_property_attribute(&self, property_attribute_id: usize) -> Option<&PropertyAttribute> {
        self.property_attributes.get(property_attribute_id)
    }
}

/// What the metadata parsers log instead of throwing. `message` is the text
/// CesiumJS writes to the console.
#[derive(Clone, Debug, PartialEq)]
pub enum StructuralMetadataWarning {
    /// A property table's values could not be packed into a texture, so the
    /// table has none (`console.warn`).
    PropertyTableTextureFailed { table: String, error: String },
    /// The table is wider or taller than the context's maximum texture size
    /// (the one-time warning `PropertyTableTextureExceedsMaximumSize`).
    PropertyTableTextureExceedsMaximumSize { table: String, maximum_texture_size: u32 },
    /// A batch table used the 3D Tiles 1.0 `HIERARCHY` property (the
    /// deprecation warning `batchTableHierarchyExtension`).
    BatchTableHierarchyDeprecated,
    /// A point cloud property was cast to floats for WebGL (the one-time
    /// warning `Cast pnts property to floats`).
    PntsPropertyCastToFloats { attribute: String },
}

impl StructuralMetadataWarning {
    /// The identifier CesiumJS de-duplicates one-time warnings by; `None` for
    /// a plain `console.warn`.
    pub fn id(&self) -> Option<&'static str> {
        match self {
            Self::PropertyTableTextureFailed { .. } => None,
            Self::PropertyTableTextureExceedsMaximumSize { .. } => Some("PropertyTableTextureExceedsMaximumSize"),
            Self::BatchTableHierarchyDeprecated => Some("batchTableHierarchyExtension"),
            Self::PntsPropertyCastToFloats { .. } => Some("Cast pnts property to floats"),
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::PropertyTableTextureFailed { table, error } => {
                format!("Failed to create texture for property table \"{table}\": {error}")
            }
            Self::PropertyTableTextureExceedsMaximumSize { table, maximum_texture_size } => format!(
                "Cannot create a texture for the property table \"{table}\" because it exceeds the maximum texture size of {maximum_texture_size}."
            ),
            Self::BatchTableHierarchyDeprecated => "The batch table HIERARCHY property has been moved to an extension. Use extensions.3DTILES_batch_table_hierarchy instead.".into(),
            Self::PntsPropertyCastToFloats { attribute } => format!(
                "Point cloud property \"{attribute}\" will be cast to a float array because INT, UNSIGNED_INT, and DOUBLE are not valid WebGL vertex attribute types. Some precision may be lost."
            ),
        }
    }
}
