//! CesiumJS `DebugCameraPrimitive`: draws the outline and frustum planes
//! of a camera view frustum.
//!
//! Visualizes frustum near/far planes and boundaries, including multiple frustum
//! splits, with configurable color, updates on camera change, and pick ID support.

use super::camera::Camera;
use super::frame_state::FrameState;
use super::per_instance_color_appearance::{PerInstanceColorAppearance, PerInstanceColorAppearanceOptions};
use super::primitive::{Primitive, PrimitiveOptions};
use super::scene::ScenePrimitive;
use crate::core::color_geometry_instance_attribute::ColorGeometryInstanceAttribute;
use crate::core::frustum_geometry::{Frustum, FrustumGeometry, FrustumOutlineGeometry};
use crate::core::{Color, GeometryInstance, Matrix3, Quaternion, RuntimeError, VertexFormat};
use crate::renderer::{Gl, UniformCamera, UniformFrustum};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

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
pub struct DebugCameraPrimitive<G: Gl + 'static> {
    pub frustum_splits: Option<Vec<f64>>,
    pub color: Color,
    pub update_on_change: bool,
    pub show: bool,
    pub id: Option<String>,

    camera: Option<Rc<RefCell<Camera>>>,
    outline_primitives: Vec<Primitive<G>>,
    planes_primitives: Vec<Primitive<G>>,
    owner: String,
    destroyed: bool,
}

impl<G: Gl + 'static> Default for DebugCameraPrimitive<G> {
    fn default() -> Self {
        Self::new(DebugCameraPrimitiveOptions::default())
    }
}

impl<G: Gl + 'static> DebugCameraPrimitive<G> {
    pub fn new(options: DebugCameraPrimitiveOptions) -> Self {
        Self {
            frustum_splits: options.frustum_splits,
            color: options.color.unwrap_or(Color::CYAN),
            update_on_change: options.update_on_change.unwrap_or(true),
            show: options.show.unwrap_or(true),
            id: options.id,
            camera: None,
            outline_primitives: Vec::new(),
            planes_primitives: Vec::new(),
            owner: format!("DebugCameraPrimitive#{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)),
            destroyed: false,
        }
    }

    /// Track a camera when submitting this primitive through Scene.
    pub fn with_camera(options: DebugCameraPrimitiveOptions, camera: Rc<RefCell<Camera>>) -> Self {
        Self { camera: Some(camera), ..Self::new(options) }
    }

    pub fn outline_primitives(&self) -> &[Primitive<G>] {
        &self.outline_primitives
    }

    pub fn planes_primitives(&self) -> &[Primitive<G>] {
        &self.planes_primitives
    }

    pub fn update(&mut self, camera: &mut Camera, frame_state: &mut FrameState) -> crate::scene::scene::Result<()> {
        self.update_camera(&camera.uniform_camera(), frame_state)
    }

    fn update_camera(
        &mut self,
        camera: &UniformCamera,
        frame_state: &mut FrameState,
    ) -> crate::scene::scene::Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("DebugCameraPrimitive has been destroyed"));
        }
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
                UniformFrustum::Perspective(p) => (p.near, p.far),
                UniformFrustum::PerspectiveOffCenter(p) => (p.near, p.far),
                UniformFrustum::Orthographic(o) => (o.near, o.far),
                UniformFrustum::OrthographicOffCenter(o) => (o.near, o.far),
            };

            let splits = match &self.frustum_splits {
                Some(s) if s.len() > 1 => s.clone(),
                _ => vec![near_default, far_default],
            };

            let num_frustums = splits.len().saturating_sub(1);

            let position = camera.position;
            let direction = camera.direction;
            let up = camera.up;
            let right = camera.right.negate();

            let rotation =
                Matrix3([right.x, right.y, right.z, up.x, up.y, up.z, direction.x, direction.y, direction.z]);

            let orientation = Quaternion::from_rotation_matrix(&rotation);

            for i in 0..num_frustums {
                let near = splits[i];
                let far = splits[i + 1];

                let (frustum, draw_near_plane) = match &camera.frustum {
                    UniformFrustum::Perspective(p) => {
                        let mut p_copy = *p;
                        p_copy.near = near;
                        p_copy.far = far;
                        (Frustum::Perspective(p_copy), i == 0)
                    }
                    UniformFrustum::PerspectiveOffCenter(p) => {
                        let mut off_center = *p;
                        off_center.near = near;
                        off_center.far = far;
                        (Frustum::PerspectiveOffCenter(off_center), i == 0)
                    }
                    UniformFrustum::Orthographic(o) => {
                        let mut o_copy = *o;
                        o_copy.near = near;
                        o_copy.far = far;
                        (Frustum::Orthographic(o_copy), i == 0)
                    }
                    UniformFrustum::OrthographicOffCenter(o) => {
                        let mut off_center = *o;
                        off_center.near = near;
                        off_center.far = far;
                        (Frustum::OrthographicOffCenter(off_center), i == 0)
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
                    instance
                        .attributes
                        .push(("color".into(), ColorGeometryInstanceAttribute::from_color(&color).into()));
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
                        pick_primitive: Some(self.owner.clone()),
                        ..Default::default()
                    });
                    self.planes_primitives.push(planes_prim);
                }

                let outline_geom = FrustumOutlineGeometry { frustum, origin: position, orientation, draw_near_plane }
                    .create_geometry();

                if let Some(geom) = outline_geom {
                    let mut instance = GeometryInstance::new(geom, None);
                    instance
                        .attributes
                        .push(("color".into(), ColorGeometryInstanceAttribute::from_color(&self.color).into()));
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
                        pick_primitive: Some(self.owner.clone()),
                        ..Default::default()
                    });
                    self.outline_primitives.push(outline_prim);
                }
            }
        }

        for (outline, planes) in self.outline_primitives.iter_mut().zip(&mut self.planes_primitives) {
            outline.update(frame_state)?;
            planes.update(frame_state)?;
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
        self.camera = None;
        self.destroyed = true;
    }
}

impl<G: Gl + 'static> ScenePrimitive for DebugCameraPrimitive<G> {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> crate::scene::scene::Result<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> crate::scene::scene::Result<()> {
        let camera = match &self.camera {
            Some(camera) => camera
                .try_borrow_mut()
                .map_err(|_| RuntimeError::new("DebugCameraPrimitive camera is already borrowed"))?
                .uniform_camera(),
            None => frame.uniform.camera.clone(),
        };
        self.update_camera(&camera, frame)
    }

    fn post_passes_update(&mut self, _: &mut FrameState) -> crate::scene::scene::Result<()> {
        Ok(())
    }

    fn update_for_pass(&mut self, _: &mut FrameState, _: super::Cesium3DTilePass) -> crate::scene::scene::Result<()> {
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
    use crate::scene::primitive::tests::TestGl;

    #[test]
    fn default_properties_match_upstream() {
        let prim = DebugCameraPrimitive::<TestGl>::default();
        assert_eq!(prim.color, Color::CYAN);
        assert!(prim.update_on_change);
        assert!(prim.show);
        assert_eq!(prim.id, None);
        assert_eq!(prim.frustum_splits, None);
        assert!(!prim.is_destroyed());
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
        let prim = DebugCameraPrimitive::<TestGl>::new(options);
        assert_eq!(prim.color, Color::YELLOW);
        assert!(!prim.update_on_change);
        assert!(!prim.show);
        assert_eq!(prim.id, Some("debug_cam".into()));
        assert_eq!(prim.frustum_splits, Some(vec![1.0, 100.0, 1000.0]));
    }
}
