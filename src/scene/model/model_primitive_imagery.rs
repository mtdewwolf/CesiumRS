//! CesiumJS `Scene/Model/ModelPrimitiveImagery`: manages imagery draping
//! and texture coordinates for a single model primitive.

use super::{ImageryCoverage, MappedPositions};
use crate::core::{Ellipsoid, Matrix4};
use crate::renderer::{Buffer, BufferData, BufferKind, BufferUsage, Context, Gl};
use crate::scene::model_components as mc;

/// Manages mapped positions, tile coverage, and texture coordinate attributes for one primitive.
#[derive(Clone)]
pub struct ModelPrimitiveImagery<G: Gl> {
    pub ready: bool,
    pub last_modification_counter: u64,
    pub mapped_positions_per_ellipsoid: Vec<MappedPositions>,
    pub coverages_per_layer: Vec<Vec<ImageryCoverage>>,
    pub imagery_tex_coord_attributes_per_projection: Vec<mc::Attribute<G>>,
    pub last_model_matrix: Matrix4,
}

impl<G: Gl> Default for ModelPrimitiveImagery<G> {
    fn default() -> Self {
        Self {
            ready: true,
            last_modification_counter: 0,
            mapped_positions_per_ellipsoid: Vec::new(),
            coverages_per_layer: Vec::new(),
            imagery_tex_coord_attributes_per_projection: Vec::new(),
            last_model_matrix: Matrix4::IDENTITY,
        }
    }
}

impl<G: Gl> ModelPrimitiveImagery<G> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn coverages_for_layer(&self, layer_index: usize) -> Option<&[ImageryCoverage]> {
        self.coverages_per_layer.get(layer_index).map(|c| c.as_slice())
    }

    pub fn imagery_tex_coord_attributes_per_projection(&self) -> &[mc::Attribute<G>] {
        &self.imagery_tex_coord_attributes_per_projection
    }

    pub fn mapped_positions_for_ellipsoid(&self, ellipsoid: &Ellipsoid) -> Option<&MappedPositions> {
        self.mapped_positions_per_ellipsoid
            .iter()
            .find(|m| &m.ellipsoid == ellipsoid)
    }

    /// Uploads GPU vertex buffers for the computed projection texture coordinates.
    pub fn upload_imagery_tex_coord_attributes(&mut self, context: &Context<G>) -> Result<(), crate::core::RuntimeError> {
        for attr in &mut self.imagery_tex_coord_attributes_per_projection {
            if let Some(ref ta) = attr.typed_array {
                let bytes = ta.to_le_bytes();
                let buffer = Buffer::new(
                    context,
                    BufferKind::Vertex,
                    BufferUsage::StaticDraw,
                    BufferData::Bytes(&bytes),
                )?;
                attr.buffer = Some(buffer);
            }
        }
        Ok(())
    }
}
