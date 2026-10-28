//! CesiumJS `DebugCameraPrimitive`: draws the outline and frustum planes
//! of a camera view frustum.
//!
//! Visualizes frustum near/far planes and boundaries, including multiple frustum
//! splits, with configurable color, updates on camera change, and pick ID support.

use super::camera::{Camera, CameraFrustum};
use super::frame_state::FrameState;
use super::per_instance_color_appearance::{PerInstanceColorAppearance, PerInstanceColorAppearanceOptions};
use super::primitive::{Primitive, PrimitiveOptions};
use super::scene::ScenePrimitive;
use crate::core::color_geometry_instance_attribute::ColorGeometryInstanceAttribute;
use crate::core::frustum_geometry::{Frustum, FrustumGeometry, FrustumOutlineGeometry};
use crate::core::orthographic_frustum::OrthographicFrustum;
use crate::core::{Color, GeometryInstance, Matrix3, Quaternion, VertexFormat};

/// Options for configuring a [`DebugCameraPrimitive`].
#[derive(Clone, Debug, PartialEq)]
pub struct DebugCameraPrimitiveOptions {
    pub frustum_splits: Option<Vec<f64>>,
    pub color: Option<Color>,
    pub update_on_change: Option<bool>,
    pub show: Option<bool>,
    pub id: Option<String>,
}

impl Default for DebugCameraPrimitiveOptions {
    fn default() -> Self {
        Self {
            frustum_splits: None,
            color: Some(Color::CYAN),
            update_on_change: Some(true),
            show: Some(true),
            id: None,
        }
    }
}

/// Visualizes the view frustum (planes and outline) of a camera.
pub struct DebugCameraPrimitive {
    pub frustum_splits: Option<Vec<f64>>,
    pub color: Color,
    pub update_on_change: bool,
    pub show: bool,
    pub id: Option<String>,

    outline_primitives: Vec<Primitive>,
    planes_primitives: Vec<Primitive>,
    destroyed: bool,
}

impl Default for DebugCameraPrimitive {
    fn default() -> Self {
        Self::new(DebugCameraPrimitiveOptions::default())
    }
}

impl DebugCameraPrimitive {
    pub fn new(options: DebugCameraPrimitiveOptions) -> Self {
        Self {
            frustum_splits: options.frustum_splits,
            color: options.color.unwrap_or(Color::CYAN),
            update_on_change: options.update_on_change.unwrap_or(true),
            show: options.show.unwrap_or(true),
            id: options.id,
            outline_primitives: Vec::new(),
            planes_primitives: Vec::new(),
            destroyed: false,
        }
    }

    pub fn outline_primitives(&self) -> &[Primitive] {
        &self.outline_primitives
    }

    pub fn planes_primitives(&self) -> &[Primitive] {
        &self.planes_primitives
    }

    pub fn update(&mut self, camera: &mut Camera, frame_state: &mut FrameState) -> crate::scene::scene::Result<()> {
        if !self.show {
            return Ok(());
        }

        if self.update_on_change {
            for p in &mut self.outline_primitives {
                let _ = p.destroy();
            }
            for p in &mut self.planes_primitives {
                let _ = p.destroy();
            }
            self.outline_primitives.clear();
            self.planes_primitives.clear();
        }

        if self.planes_primitives.is_empty() {
            let (near_default, far_default) = match &camera.frustum {
                CameraFrustum::Perspective(p) => (p.near, p.far),
                CameraFrustum::Orthographic(o) => (o.near, o.far),
                CameraFrustum::OrthographicOffCenter(o) => (o.near, o.far),
            };

            let splits = match &self.frustum_splits {
                Some(s) if s.len() > 1 => s.clone(),
                _ => vec![near_default, far_default],
            };

            let num_frustums = splits.len().saturating_sub(1);

            let position = camera.position_wc();
            let direction = camera.direction_wc();
            let up = camera.up_wc();
            let right = camera.right_wc().negate();

            let rotation = Matrix3([
                right.x, right.y, right.z,
                up.x, up.y, up.z,
                direction.x, direction.y, direction.z,
            ]);

            let orientation = Quaternion::from_rotation_matrix(&rotation);

            for i in 0..num_frustums {
                let near = splits[i];
                let far = splits[i + 1];

                let (frustum, draw_near_plane) = match &camera.frustum {
                    CameraFrustum::Perspective(p) => {
                        let mut p_copy = *p;
                        p_copy.near = near;
                        p_copy.far = far;
                        (Frustum::Perspective(p_copy), i == 0)
                    }
                    CameraFrustum::Orthographic(o) => {
                        let mut o_copy = *o;
                        o_copy.near = near;
                        o_copy.far = far;
                        (Frustum::Orthographic(o_copy), i == 0)
                    }
                    CameraFrustum::OrthographicOffCenter(o) => {
                        let width = (o.right - o.left).abs();
                        let height = (o.top - o.bottom).abs();
                        let aspect = if height > 0.0 { width / height } else { 1.0 };
                        let mut ortho = OrthographicFrustum::new(width, aspect);
                        ortho.near = near;
                        ortho.far = far;
                        (Frustum::Orthographic(ortho), i == 0)
                    }
                };

                let planes_geom = FrustumGeometry {
                    frustum,
                    origin: position,
                    orientation,
                    vertex_format: VertexFormat::DEFAULT,
                    draw_near_plane,
                }
                .create_geometry();

                if let Some(geom) = planes_geom {
                    let mut instance = GeometryInstance::new(geom, None);
                    let color = Color::new(self.color.red, self.color.green, self.color.blue, 0.1);
                    instance.attributes.push((
                        "color".into(),
                        ColorGeometryInstanceAttribute::from_color(&color).into(),
                    ));
                    instance.id = self.id.clone();

                    let planes_prim = Primitive::new(PrimitiveOptions {
                        geometry_instances: vec![instance],
                        appearance: Some(Box::new(PerInstanceColorAppearance::new(
                            PerInstanceColorAppearanceOptions {
                                translucent: Some(true),
                                flat: Some(true),
                                ..Default::default()
                            },
                        ))),
                        asynchronous: Some(false),
                        ..Default::default()
                    });
                    self.planes_primitives.push(planes_prim);
                }

                let outline_geom = FrustumOutlineGeometry {
                    frustum,
                    origin: position,
                    orientation,
                    draw_near_plane,
                }
                .create_geometry();

                if let Some(geom) = outline_geom {
                    let mut instance = GeometryInstance::new(geom, None);
                    instance.attributes.push((
                        "color".into(),
                        ColorGeometryInstanceAttribute::from_color(&self.color).into(),
                    ));
                    instance.id = self.id.clone();

                    let outline_prim = Primitive::new(PrimitiveOptions {
                        geometry_instances: vec![instance],
                        appearance: Some(Box::new(PerInstanceColorAppearance::new(
                            PerInstanceColorAppearanceOptions {
                                translucent: Some(false),
                                flat: Some(true),
                                ..Default::default()
                            },
                        ))),
                        asynchronous: Some(false),
                        ..Default::default()
                    });
                    self.outline_primitives.push(outline_prim);
                }
            }
        }

        for p in &mut self.outline_primitives {
            p.update(frame_state)?;
        }
        for p in &mut self.planes_primitives {
            p.update(frame_state)?;
        }
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) {
        for p in &mut self.outline_primitives {
            let _ = p.destroy();
        }
        for p in &mut self.planes_primitives {
            let _ = p.destroy();
        }
        self.outline_primitives.clear();
        self.planes_primitives.clear();
        self.destroyed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_properties_match_upstream() {
        let prim = DebugCameraPrimitive::default();
        assert_eq!(prim.color, Color::CYAN);
        assert_eq!(prim.update_on_change, true);
        assert_eq!(prim.show, true);
        assert_eq!(prim.id, None);
        assert_eq!(prim.frustum_splits, None);
        assert_eq!(prim.is_destroyed(), false);
    }

    #[test]
    fn custom_options_set_fields() {
        let options = DebugCameraPrimitiveOptions {
            frustum_splits: Some(vec![1.0, 100.0, 1000.0]),
            color: Some(Color::YELLOW),
            update_on_change: Some(false),
            show: Some(false),
            id: Some("debug_cam".into()),
        };
        let prim = DebugCameraPrimitive::new(options);
        assert_eq!(prim.color, Color::YELLOW);
        assert_eq!(prim.update_on_change, false);
        assert_eq!(prim.show, false);
        assert_eq!(prim.id, Some("debug_cam".into()));
        assert_eq!(prim.frustum_splits, Some(vec![1.0, 100.0, 1000.0]));
    }
}
