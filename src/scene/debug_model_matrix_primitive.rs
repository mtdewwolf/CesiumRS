//! CesiumJS `DebugModelMatrixPrimitive`: draws the axes of a reference frame
//! defined by a matrix that transforms to world coordinates.
//!
//! The X axis is red, Y is green, and Z is blue. This is for debugging only.

use super::frame_state::FrameState;
use super::polyline_color_appearance::{PolylineColorAppearance, PolylineColorAppearanceOptions};
use super::primitive::{Primitive, PrimitiveOptions};
use super::scene::ScenePrimitive;
use crate::core::polyline_geometry::{PolylineGeometry, PolylineGeometryOptions};
use crate::core::{ArcType, Cartesian3, Color, GeometryInstance, Matrix4};

/// Options for configuring a [`DebugModelMatrixPrimitive`].
#[derive(Clone, Debug, PartialEq)]
pub struct DebugModelMatrixPrimitiveOptions {
    pub length: Option<f64>,
    pub width: Option<f64>,
    pub model_matrix: Option<Matrix4>,
    pub show: Option<bool>,
    pub id: Option<String>,
}

impl Default for DebugModelMatrixPrimitiveOptions {
    fn default() -> Self {
        Self {
            length: Some(10_000_000.0),
            width: Some(2.0),
            model_matrix: Some(Matrix4::IDENTITY),
            show: Some(true),
            id: None,
        }
    }
}

/// Draws red, green, and blue axes representing a 4x4 reference frame.
pub struct DebugModelMatrixPrimitive {
    pub length: f64,
    pub width: f64,
    pub show: bool,
    pub model_matrix: Matrix4,
    pub id: Option<String>,

    applied_length: Option<f64>,
    applied_width: Option<f64>,
    applied_model_matrix: Option<Matrix4>,
    applied_id: Option<String>,
    primitive: Option<Primitive>,
    destroyed: bool,
}

impl Default for DebugModelMatrixPrimitive {
    fn default() -> Self {
        Self::new(DebugModelMatrixPrimitiveOptions::default())
    }
}

impl DebugModelMatrixPrimitive {
    pub fn new(options: DebugModelMatrixPrimitiveOptions) -> Self {
        Self {
            length: options.length.unwrap_or(10_000_000.0),
            width: options.width.unwrap_or(2.0),
            show: options.show.unwrap_or(true),
            model_matrix: options.model_matrix.unwrap_or(Matrix4::IDENTITY),
            id: options.id,
            applied_length: None,
            applied_width: None,
            applied_model_matrix: None,
            applied_id: None,
            primitive: None,
            destroyed: false,
        }
    }

    pub fn primitive(&self) -> Option<&Primitive> {
        self.primitive.as_ref()
    }

    pub fn update(&mut self, frame_state: &mut FrameState) -> crate::scene::scene::Result<()> {
        if !self.show {
            return Ok(());
        }

        let needs_rebuild = self.primitive.is_none()
            || self.applied_model_matrix != Some(self.model_matrix)
            || self.applied_length != Some(self.length)
            || self.applied_width != Some(self.width)
            || self.applied_id != self.id;

        if needs_rebuild {
            self.applied_model_matrix = Some(self.model_matrix);
            self.applied_length = Some(self.length);
            self.applied_width = Some(self.width);
            self.applied_id = self.id.clone();

            let mut matrix = self.model_matrix;
            // Workaround projecting (0, 0, 0)
            if matrix[12] == 0.0 && matrix[13] == 0.0 && matrix[14] == 0.0 {
                matrix[14] = 0.01;
            }

            let scaled_matrix = matrix.multiply_by_uniform_scale(self.length);

            let make_instance = |axis: Cartesian3, color: Color| -> Option<GeometryInstance> {
                let options = PolylineGeometryOptions {
                    positions: vec![Cartesian3::ZERO, axis],
                    colors: Some(vec![color, color]),
                    width: self.width,
                    vertex_format: PolylineColorAppearance::VERTEX_FORMAT,
                    arc_type: ArcType::None,
                    ..PolylineGeometryOptions::new(vec![])
                };
                let geom = PolylineGeometry::new(options).create_geometry()?;
                let mut inst = GeometryInstance::new(geom, Some(scaled_matrix));
                inst.id = self.id.clone();
                Some(inst)
            };

            let mut instances = Vec::with_capacity(3);
            if let Some(x) = make_instance(Cartesian3::UNIT_X, Color::RED) {
                instances.push(x);
            }
            if let Some(y) = make_instance(Cartesian3::UNIT_Y, Color::GREEN) {
                instances.push(y);
            }
            if let Some(z) = make_instance(Cartesian3::UNIT_Z, Color::BLUE) {
                instances.push(z);
            }

            self.primitive = Some(Primitive::new(PrimitiveOptions {
                geometry_instances: instances,
                appearance: Some(Box::new(PolylineColorAppearance::new(
                    PolylineColorAppearanceOptions::default(),
                ))),
                asynchronous: Some(false),
                ..Default::default()
            }));
        }

        if let Some(p) = self.primitive.as_mut() {
            p.update(frame_state)?;
        }
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) {
        if let Some(mut p) = self.primitive.take() {
            let _ = p.destroy();
        }
        self.destroyed = true;
    }
}

impl ScenePrimitive for DebugModelMatrixPrimitive {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> crate::scene::scene::Result<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> crate::scene::scene::Result<()> {
        self.update(frame)
    }

    fn post_passes_update(&mut self, _frame: &mut FrameState) -> crate::scene::scene::Result<()> {
        Ok(())
    }

    fn update_for_pass(
        &mut self,
        _frame: &mut FrameState,
        _pass: super::Cesium3DTilePass,
    ) -> crate::scene::scene::Result<()> {
        Ok(())
    }

    fn destroy(&mut self) -> crate::scene::scene::Result<()> {
        self.destroy();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_properties_match_upstream() {
        let prim = DebugModelMatrixPrimitive::default();
        assert_eq!(prim.length, 10_000_000.0);
        assert_eq!(prim.width, 2.0);
        assert_eq!(prim.show, true);
        assert_eq!(prim.model_matrix, Matrix4::IDENTITY);
        assert_eq!(prim.id, None);
        assert_eq!(prim.is_destroyed(), false);
    }

    #[test]
    fn custom_options_set_fields() {
        let options = DebugModelMatrixPrimitiveOptions {
            length: Some(500.0),
            width: Some(4.0),
            model_matrix: Some(Matrix4::from_translation(Cartesian3::new(1.0, 2.0, 3.0))),
            show: Some(false),
            id: Some("debug_axes".into()),
        };
        let prim = DebugModelMatrixPrimitive::new(options);
        assert_eq!(prim.length, 500.0);
        assert_eq!(prim.width, 4.0);
        assert_eq!(prim.show, false);
        assert_eq!(prim.id, Some("debug_axes".into()));
    }
}
