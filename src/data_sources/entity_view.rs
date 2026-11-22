//! CesiumJS `EntityView`: tracks an entity with a [`Camera`] in 3D, 2D, or Columbus View.
use super::{
    visualizer_values::eval_cartesian3_opt, EntityRef, PositionContext, PositionPropertyRef, Property, PropertyRef,
    VelocityVectorProperty,
};
use crate::{
    core::{
        math, BoundingSphere, Cartesian3, Ellipsoid, HeadingPitchRange, JulianDate, Matrix3, Matrix4, Quaternion,
        RuntimeError, TrackingReferenceFrame, Transforms,
    },
    scene::{Camera, CameraOffset, Scene, SceneBackend, SceneMode},
};
use std::rc::Rc;

const NORTH_UP_AXIS_FACTOR: f64 = 1.25;

/// Default camera offset used to initialize [`EntityView`], matching `EntityView.defaultOffset3D`.
pub const DEFAULT_OFFSET_3D: Cartesian3 = Cartesian3::new(-14000.0, 3500.0, 3500.0);

/// Port of CesiumJS `EntityView`.
pub struct EntityView {
    pub entity: EntityRef,
    pub ellipsoid: Ellipsoid,
    pub bounding_sphere: Option<BoundingSphere>,
    default_offset_3d: Cartesian3,
    context: PositionContext,
    last_entity: Option<EntityRef>,
    mode: Option<SceneMode>,
    last_cartesian: Cartesian3,
    velocity_property: VelocityVectorProperty,
    offset_3d: Cartesian3,
}

impl EntityView {
    pub fn new(entity: EntityRef, ellipsoid: Option<Ellipsoid>) -> Self {
        Self::with_context(entity, ellipsoid, PositionContext::default())
    }

    pub fn with_context(entity: EntityRef, ellipsoid: Option<Ellipsoid>, context: PositionContext) -> Self {
        let velocity_property = VelocityVectorProperty::with_context(
            entity.position().map(|p| p as PropertyRef<Cartesian3>),
            true,
            context.clone(),
        );
        Self {
            entity,
            ellipsoid: ellipsoid.unwrap_or(Ellipsoid::WGS84),
            bounding_sphere: None,
            default_offset_3d: DEFAULT_OFFSET_3D,
            context,
            last_entity: None,
            mode: None,
            last_cartesian: Cartesian3::ZERO,
            velocity_property,
            offset_3d: Cartesian3::ZERO,
        }
    }

    pub fn default_offset_3d(&self) -> Cartesian3 {
        self.default_offset_3d
    }

    pub fn set_default_offset_3d(&mut self, offset: Cartesian3) {
        self.default_offset_3d = offset;
    }

    /// Updates a [`Scene`]'s camera to track `self.entity`.
    pub fn update_scene<B: SceneBackend>(
        &mut self,
        time: &JulianDate,
        scene: &mut Scene<B>,
        bounding_sphere: Option<BoundingSphere>,
    ) -> Result<(), RuntimeError> {
        let mode = scene.mode();
        self.update(time, mode, &mut scene.camera, bounding_sphere)
    }

    /// Updates `camera` in `scene_mode` to track `self.entity` at `time`.
    pub fn update(
        &mut self,
        time: &JulianDate,
        scene_mode: SceneMode,
        camera: &mut Camera,
        bounding_sphere: Option<BoundingSphere>,
    ) -> Result<(), RuntimeError> {
        if scene_mode == SceneMode::Morphing {
            return Ok(());
        }

        let entity = Rc::clone(&self.entity);
        let tracking_reference_frame = entity.tracking_reference_frame();
        self.bounding_sphere = bounding_sphere;
        let Some(position_property) = entity.position() else {
            return Ok(());
        };
        let object_changed = !self.last_entity.as_ref().is_some_and(|prev| Rc::ptr_eq(prev, &entity));
        let scene_mode_changed = self.mode != Some(scene_mode);

        let mut update_look_at = object_changed || scene_mode_changed;
        let mut save_camera = true;

        if object_changed {
            let view_from_slot = entity.get_property("viewFrom");
            let has_view_from = view_from_slot.is_some();

            if !has_view_from {
                if let Some(sphere) = bounding_sphere {
                    let mut pitch = -math::PI_OVER_FOUR;
                    if let Some(position) = position_property.get_value(Some(time), None)? {
                        let ratio = (position.magnitude() / self.ellipsoid.maximum_radius).max(1.0);
                        let factor = 2.0 - 1.0 / ratio;
                        pitch *= factor;
                    }
                    let offset = HeadingPitchRange::new(0.0, pitch, 0.0);
                    camera.view_bounding_sphere(&sphere, Some(offset)).map_err(|e| RuntimeError::new(e.0))?;
                    update_look_at = false;
                    save_camera = false;
                } else {
                    self.offset_3d = self.default_offset_3d;
                }
            } else if let Some(view_from) = eval_cartesian3_opt(view_from_slot.as_ref(), time)? {
                self.offset_3d = view_from;
            } else {
                self.offset_3d = self.default_offset_3d;
            }
        } else if !scene_mode_changed && self.mode != Some(SceneMode::Scene2D) {
            self.offset_3d = camera.position;
        }

        self.last_entity = Some(Rc::clone(&entity));
        self.mode = Some(scene_mode);

        self.update_transform(
            scene_mode,
            camera,
            update_look_at,
            save_camera,
            &position_property,
            tracking_reference_frame,
            time,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn update_transform(
        &mut self,
        mode: SceneMode,
        camera: &mut Camera,
        update_look_at: bool,
        save_camera: bool,
        position_property: &PositionPropertyRef,
        tracking_reference_frame: TrackingReferenceFrame,
        time: &JulianDate,
    ) -> Result<(), RuntimeError> {
        let ellipsoid = self.ellipsoid;
        if let Some(mut cartesian) = position_property.get_value(Some(time), Some(&mut self.last_cartesian))? {
            let mut has_basis = false;
            let mut x_basis = Cartesian3::ZERO;
            let mut y_basis = Cartesian3::ZERO;
            let mut z_basis = Cartesian3::ZERO;

            if mode == SceneMode::Scene3D {
                let mut delta_time = time.add_seconds(0.001);
                let mut invert_velocity = false;
                let mut delta_cartesian = position_property.get_value(Some(&delta_time), None)?;
                if delta_cartesian.is_none() {
                    delta_time = time.add_seconds(-0.001);
                    delta_cartesian = position_property.get_value(Some(&delta_time), None)?;
                    invert_velocity = true;
                }

                if let Some(delta_cartesian) = delta_cartesian {
                    let to_fixed = (self.context.frame_transform)(time)?;
                    let to_fixed_delta = (self.context.frame_transform)(&delta_time)?;
                    let to_inertial = to_fixed.transpose();
                    let to_inertial_delta = to_fixed_delta.transpose();

                    let inertial_cartesian = to_inertial.multiply_by_vector(cartesian);
                    let inertial_delta_cartesian = to_inertial_delta.multiply_by_vector(delta_cartesian);
                    let diff = inertial_cartesian - inertial_delta_cartesian;
                    let inertial_velocity = diff.magnitude() * 1000.0;

                    let mu = math::GRAVITATIONALPARAMETER;
                    let semi_major_axis =
                        -mu / (inertial_velocity * inertial_velocity - (2.0 * mu) / inertial_cartesian.magnitude());

                    if semi_major_axis < 0.0 || semi_major_axis > NORTH_UP_AXIS_FACTOR * ellipsoid.maximum_radius {
                        x_basis = cartesian.normalize().negate();
                        z_basis = Cartesian3::UNIT_Z;
                        y_basis = z_basis.cross(x_basis);
                        if y_basis.magnitude() > math::EPSILON7 {
                            x_basis = x_basis.normalize();
                            y_basis = y_basis.normalize();
                            z_basis = x_basis.cross(y_basis).normalize();
                            has_basis = true;
                        }
                    } else if !cartesian.equals_epsilon(delta_cartesian, math::EPSILON7) {
                        z_basis = inertial_cartesian.normalize();
                        let delta_dir = inertial_delta_cartesian.normalize();
                        y_basis = z_basis.cross(delta_dir);
                        if invert_velocity {
                            y_basis = y_basis.multiply_by_scalar(-1.0);
                        }
                        if !y_basis.equals_epsilon(Cartesian3::ZERO, math::EPSILON7) {
                            x_basis = y_basis.cross(z_basis);
                            x_basis = to_fixed.multiply_by_vector(x_basis).normalize();
                            y_basis = to_fixed.multiply_by_vector(y_basis).normalize();
                            z_basis = to_fixed.multiply_by_vector(z_basis).normalize();
                            has_basis = true;
                        }
                    }
                }
            }

            if let Some(sphere) = self.bounding_sphere {
                cartesian = sphere.center;
            }

            let saved = save_camera.then_some((camera.position, camera.direction, camera.up));

            let orientation = match self.entity.get_property("orientation") {
                Some(super::PropertyInput::Component(c)) => match c.get_value(time, None)? {
                    Some(super::DynamicValue::Quaternion(q)) => Some(q),
                    _ => None,
                },
                Some(super::PropertyInput::Value(super::DynamicValue::Quaternion(q))) => Some(q),
                _ => None,
            };

            let velocity = self.velocity_property.get_value(Some(time), None)?;

            let transform = if tracking_reference_frame == TrackingReferenceFrame::Inertial {
                if let Some(q) = orientation {
                    Matrix4::from_translation_quaternion_rotation_scale(cartesian, &q, Cartesian3::ONE)
                } else if !has_basis {
                    Transforms::east_north_up_to_fixed_frame(cartesian, &ellipsoid)
                } else {
                    basis_matrix(x_basis, y_basis, z_basis, cartesian)
                }
            } else if tracking_reference_frame == TrackingReferenceFrame::Velocity {
                if let Some(v) = velocity {
                    let rotation = Transforms::rotation_matrix_from_position_velocity(cartesian, v, &ellipsoid);
                    Matrix4::from_rotation_translation(&rotation, cartesian)
                } else if !has_basis {
                    Transforms::east_north_up_to_fixed_frame(cartesian, &ellipsoid)
                } else {
                    basis_matrix(x_basis, y_basis, z_basis, cartesian)
                }
            } else if tracking_reference_frame == TrackingReferenceFrame::Enu || !has_basis {
                Transforms::east_north_up_to_fixed_frame(cartesian, &ellipsoid)
            } else {
                basis_matrix(x_basis, y_basis, z_basis, cartesian)
            };

            camera.set_transform(transform).map_err(|e| RuntimeError::new(e.0))?;

            if let Some((position, direction, up)) = saved {
                camera.position = position;
                camera.direction = direction;
                camera.up = up;
                camera.right = direction.cross(up);
            }
        }

        if update_look_at {
            let offset = if mode == SceneMode::Scene2D || self.offset_3d == Cartesian3::ZERO {
                None
            } else {
                Some(CameraOffset::Cartesian(self.offset_3d))
            };
            let current = camera.transform();
            camera.look_at_transform(current, offset).map_err(|e| RuntimeError::new(e.0))?;
        }

        let _ = (Matrix3::IDENTITY, Quaternion::IDENTITY);
        Ok(())
    }
}

fn basis_matrix(x: Cartesian3, y: Cartesian3, z: Cartesian3, translation: Cartesian3) -> Matrix4 {
    // Matches CesiumJS EntityView.js lines 277-292 (including transform[15] = 0.0).
    Matrix4([
        x.x,
        x.y,
        x.z,
        0.0,
        y.x,
        y.y,
        y.z,
        0.0,
        z.x,
        z.y,
        z.z,
        0.0,
        translation.x,
        translation.y,
        translation.z,
        0.0,
    ])
}
