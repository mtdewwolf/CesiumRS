//! CesiumJS `SceneTransforms`: scene-dependent world, canvas and buffer coordinates.
//!
//! Canvas coordinates start at the top left. Depth reconstruction takes GL
//! drawing-buffer coordinates (bottom left), just as Cesium's picking path does.
use super::{Camera, CameraFrustum, FrameState, SceneMode};
use crate::core::{
    math, BoundingRectangle, Cartesian2, Cartesian3, Cartesian4, Cartographic, MapProjection, Matrix4, RuntimeError,
    Transforms,
};
use crate::renderer::UniformState;

/// The Scene inputs used by coordinate conversion, without borrowing its backend.
pub struct SceneTransformState<'a> {
    pub mode: SceneMode,
    pub morph_time: f64,
    pub map_projection: &'a dyn MapProjection,
    pub client_dimensions: Cartesian2,
    pub drawing_buffer_dimensions: Cartesian2,
}
impl<'a> SceneTransformState<'a> {
    pub fn from_frame(frame: &'a FrameState, client_dimensions: Cartesian2) -> Self {
        Self {
            mode: frame.mode,
            morph_time: frame.morph_time,
            map_projection: frame.uniform.map_projection.as_ref(),
            client_dimensions,
            drawing_buffer_dimensions: Cartesian2::new(
                frame.drawing_buffer_width as f64,
                frame.drawing_buffer_height as f64,
            ),
        }
    }
}

pub struct SceneTransforms;
impl SceneTransforms {
    pub fn compute_actual_ellipsoid_position(
        state: &SceneTransformState<'_>,
        position: Cartesian3,
    ) -> Option<Cartesian3> {
        if state.mode == SceneMode::Scene3D {
            return Some(position);
        }
        let cartographic = state.map_projection.ellipsoid().cartesian_to_cartographic(position)?;
        let p = state.map_projection.project(&cartographic);
        Some(match state.mode {
            SceneMode::ColumbusView => Cartesian3::new(p.z, p.x, p.y),
            SceneMode::Scene2D => Cartesian3::new(0., p.x, p.y),
            _ => Cartesian3::new(
                math::lerp(p.z, position.x, state.morph_time),
                math::lerp(p.x, position.y, state.morph_time),
                math::lerp(p.y, position.z, state.morph_time),
            ),
        })
    }

    pub fn clip_to_gl_window_coordinates(viewport: &BoundingRectangle, position: Cartesian4) -> Cartesian2 {
        let ndc = Cartesian3::new(position.x / position.w, position.y / position.w, position.z / position.w);
        let window = Matrix4::compute_viewport_transformation(viewport, 0., 1.).multiply_by_point(ndc);
        Cartesian2::new(window.x, window.y)
    }

    pub fn transform_window_to_drawing_buffer(state: &SceneTransformState<'_>, position: Cartesian2) -> Cartesian2 {
        Cartesian2::new(
            position.x * state.drawing_buffer_dimensions.x / state.client_dimensions.x,
            position.y * state.drawing_buffer_dimensions.y / state.client_dimensions.y,
        )
    }

    pub fn world_to_window_coordinates(
        state: &SceneTransformState<'_>,
        camera: &mut Camera,
        position: Cartesian3,
    ) -> Result<Option<Cartesian2>, RuntimeError> {
        Self::world_with_eye_offset_to_window_coordinates(state, camera, position, Cartesian3::ZERO)
    }

    pub fn world_to_drawing_buffer_coordinates(
        state: &SceneTransformState<'_>,
        camera: &mut Camera,
        position: Cartesian3,
    ) -> Result<Option<Cartesian2>, RuntimeError> {
        Ok(Self::world_to_window_coordinates(state, camera, position)?
            .map(|p| Self::transform_window_to_drawing_buffer(state, p)))
    }

    pub fn world_with_eye_offset_to_window_coordinates(
        state: &SceneTransformState<'_>,
        camera: &mut Camera,
        position: Cartesian3,
        eye_offset: Cartesian3,
    ) -> Result<Option<Cartesian2>, RuntimeError> {
        let Some(actual) = Self::compute_actual_ellipsoid_position(state, position) else {
            return Ok(None);
        };
        let mut viewport = BoundingRectangle::new(0., 0., state.client_dimensions.x, state.client_dimensions.y);
        if state.mode == SceneMode::Scene2D {
            let max =
                state.map_projection.project(&Cartographic::new(std::f64::consts::PI, std::f64::consts::FRAC_PI_2, 0.));
            let wc = camera.position_wc();
            let x = wc.y;
            let eye = Cartesian3::new(math::sign(x) * max.x - x, 0., -wc.x);
            let projection = camera.frustum.uniform_frustum().parameters().0;
            let window = Transforms::point_to_gl_window_coordinates(
                &projection,
                &Matrix4::compute_viewport_transformation(&viewport, 0., 1.),
                eye,
            );
            if x != 0. && window.x > 0. && window.x < state.client_dimensions.x {
                // Cesium temporarily changes and restores the scene camera. A
                // private copy also leaves the camera intact when conversion fails.
                let mut wrapped = camera.clone_camera();
                wrapped.update(SceneMode::Scene2D).map_err(|e| RuntimeError::new(e.to_string()))?;
                let CameraFrustum::OrthographicOffCenter(mut f) = wrapped.frustum else {
                    return Err(RuntimeError::new("An OrthographicOffCenterFrustum is required in 2D."));
                };
                if window.x > state.client_dimensions.x * 0.5 {
                    viewport.width = window.x;
                    f.right = max.x - x;
                } else {
                    viewport.x += window.x;
                    viewport.width -= window.x;
                    f.left = -max.x - x;
                }
                wrapped.frustum = CameraFrustum::OrthographicOffCenter(f);
                let mut result =
                    Self::clip_to_gl_window_coordinates(&viewport, world_to_clip(actual, eye_offset, &mut wrapped)?);
                if window.x > state.client_dimensions.x * 0.5 {
                    viewport.x += window.x;
                } else {
                    viewport.x -= viewport.width;
                }
                wrapped.position.x = -wrapped.position.x;
                let (left, right) = (f.left, f.right);
                f.left = -right;
                f.right = -left;
                wrapped.frustum = CameraFrustum::OrthographicOffCenter(f);
                let other =
                    Self::clip_to_gl_window_coordinates(&viewport, world_to_clip(actual, eye_offset, &mut wrapped)?);
                if result.x < 0. || result.x > state.client_dimensions.x {
                    result.x = other.x;
                }
                result.y = state.client_dimensions.y - result.y;
                return Ok(Some(result));
            }
        }
        let clip = world_to_clip(actual, eye_offset, camera)?;
        if clip.z < 0. && matches!(camera.frustum, CameraFrustum::Perspective(_)) {
            return Ok(None);
        }
        let mut result = Self::clip_to_gl_window_coordinates(&viewport, clip);
        result.y = state.client_dimensions.y - result.y;
        Ok(Some(result))
    }

    /// Reconstructs against the current uniform-state frustum, which may be
    /// one of the Scene's depth partitions rather than the full camera range.
    pub fn drawing_buffer_to_world_coordinates(
        camera_frustum: CameraFrustum,
        uniforms: &UniformState,
        viewport: &BoundingRectangle,
        position: Cartesian2,
        mut depth: f64,
        use_log_depth: bool,
    ) -> Result<Cartesian3, RuntimeError> {
        let [near, far] = uniforms.frustum;
        if use_log_depth {
            let depth_from_near = (depth * (far - near + 1.).log2()).exp2() - 1.;
            depth = far * (1. - near / (depth_from_near + near)) / (far - near);
        }
        let ndc = Cartesian4::new(
            (position.x - viewport.x) / viewport.width * 2. - 1.,
            (position.y - viewport.y) / viewport.height * 2. - 1.,
            depth * 2. - 1.,
            1.,
        );
        let world = if let Some(f) = camera_frustum.orthographic() {
            uniforms.inverse_view.multiply_by_vector(Cartesian4::new(
                (ndc.x * (f.right - f.left) + f.left + f.right) * 0.5,
                (ndc.y * (f.top - f.bottom) + f.bottom + f.top) * 0.5,
                (ndc.z * (near - far) - near - far) * 0.5,
                1.,
            ))
        } else {
            let world = uniforms.projection.multiply(&uniforms.view).inverse()?.multiply_by_vector(ndc);
            world.multiply_by_scalar(1. / world.w)
        };
        Ok(Cartesian3::new(world.x, world.y, world.z))
    }
}

fn world_to_clip(
    position: Cartesian3,
    eye_offset: Cartesian3,
    camera: &mut Camera,
) -> Result<Cartesian4, RuntimeError> {
    let mut ec = camera.view_matrix().multiply_by_vector(Cartesian4::new(position.x, position.y, position.z, 1.));
    let xyz = Cartesian3::new(ec.x, ec.y, ec.z);
    if xyz.magnitude() == 0. {
        return Err(RuntimeError::new("normalized result is not a number"));
    }
    let normalized = xyz.normalize();
    if normalized.x.is_nan() || normalized.y.is_nan() || normalized.z.is_nan() {
        return Err(RuntimeError::new("normalized result is not a number"));
    }
    let z = eye_offset.multiply_components(normalized);
    ec.x += eye_offset.x + z.x;
    ec.y += eye_offset.y + z.y;
    ec.z += z.z;
    Ok(camera.frustum.uniform_frustum().parameters().0.multiply_by_vector(ec))
}
