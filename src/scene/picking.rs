//! CesiumJS `Picking`: color-pick execution, rectangle and camera culling.
//! Depth/voxel/metadata decoding, drill and most-detailed ray picking are
//! separate remaining parts of the upstream module.
use super::{
    Camera, CameraFrustum, FramePasses, FrameResource, PickFramebuffer, PickReadback, PickedObject,
    RendererSceneBackend, Scene, SceneBackend, SceneMode, SceneTransformState, SceneTransforms,
};
use crate::{
    core::*,
    renderer::{Gl, Viewport},
};
use std::future::Future;
type Result<T> = std::result::Result<T, RuntimeError>;

pub struct Picking;
impl Picking {
    pub fn compute_drawing_buffer_rectangle(
        drawing_buffer_height: f64,
        position: Cartesian2,
        width: Option<f64>,
        height: Option<f64>,
    ) -> BoundingRectangle {
        let width = width.unwrap_or(3.);
        let height = height.unwrap_or(width);
        BoundingRectangle::new(
            position.x - (width - 1.) * 0.5,
            drawing_buffer_height - position.y - (height - 1.) * 0.5,
            width,
            height,
        )
    }
    /// Narrows primitive culling without changing the render projection.
    pub fn get_pick_culling_volume(
        camera: &mut Camera,
        mode: SceneMode,
        position: Cartesian2,
        width: f64,
        height: f64,
        viewport: &BoundingRectangle,
    ) -> Result<CullingVolume> {
        let x = 2. * (position.x - viewport.x) / viewport.width - 1.;
        let y = 2. * (viewport.height - position.y - viewport.y) / viewport.height - 1.;
        if let Some(f) = camera.frustum.orthographic() {
            let mut identity = camera.clone_camera();
            identity.update(mode).map_err(|e| RuntimeError::new(e.to_string()))?;
            identity.set_transform(Matrix4::IDENTITY).map_err(|e| RuntimeError::new(e.to_string()))?;
            let mut origin = identity.position
                + identity.right * (x * (f.right - f.left) * 0.5)
                + identity.up * (y * (f.top - f.bottom) * 0.5);
            if mode == SceneMode::Scene2D {
                origin = Cartesian3::new(origin.z, origin.x, origin.y);
            }
            // The source uses a one-pixel orthographic volume regardless of
            // the requested read rectangle's width and height.
            let pixel = f.get_pixel_dimensions(viewport.width, viewport.height, 1., 1.);
            let pick = OrthographicOffCenterFrustum::new(
                -pixel.x * 0.5,
                pixel.x * 0.5,
                pixel.y * 0.5,
                -pixel.y * 0.5,
                f.near,
                f.far,
            );
            Ok(pick.compute_culling_volume(origin, camera.direction_wc(), camera.up_wc()))
        } else {
            let CameraFrustum::Perspective(f) = camera.frustum else { unreachable!("orthographic handled above") };
            let tan_phi = (f.fovy() * 0.5).tan();
            let tan_theta = f.aspect_ratio * tan_phi;
            let x_dir = x * f.near * tan_theta;
            let y_dir = y * f.near * tan_phi;
            let pixel = f.get_pixel_dimensions(viewport.width, viewport.height, 1., 1.);
            let (half_width, half_height) = (pixel.x * width * 0.5, pixel.y * height * 0.5);
            Ok(PerspectiveOffCenterFrustum::new(
                x_dir - half_width,
                x_dir + half_width,
                y_dir + half_height,
                y_dir - half_height,
                f.near,
                f.far,
            )
            .compute_culling_volume(camera.position_wc(), camera.direction_wc(), camera.up_wc()))
        }
    }
}

impl<G: Gl + 'static> Scene<RendererSceneBackend<G>> {
    fn with_pick_framebuffer<T>(
        &mut self,
        window_position: Cartesian2,
        width: Option<f64>,
        height: Option<f64>,
        read: impl FnOnce(&PickFramebuffer<G>, &BoundingRectangle) -> Result<T>,
    ) -> Result<T> {
        let [w, h] = self.backend.dimensions()?;
        let state = SceneTransformState {
            mode: self.camera.mode(),
            morph_time: self.frame_state.morph_time,
            map_projection: self.camera.map_projection(),
            client_dimensions: Cartesian2::new(self.camera.client_width, self.camera.client_height),
            drawing_buffer_dimensions: Cartesian2::new(w as f64, h as f64),
        };
        let position = SceneTransforms::transform_window_to_drawing_buffer(&state, window_position);
        let rectangle = Picking::compute_drawing_buffer_rectangle(h as f64, position, width, height);
        if !rectangle.width.is_finite()
            || !rectangle.height.is_finite()
            || rectangle.width < 1.
            || rectangle.height < 1.
        {
            return Err(RuntimeError::new("Pick width and height must be positive"));
        }
        let viewport = Viewport { x: 0, y: 0, width: w, height: h };
        let mode = self.camera.mode();
        let volume = Picking::get_pick_culling_volume(
            &mut self.camera,
            mode,
            position,
            rectangle.width,
            rectangle.height,
            &BoundingRectangle::new(0., 0., w as f64, h as f64),
        )?;
        let mut framebuffer = match self.backend.pick_framebuffer.take() {
            Some(f) => f,
            None => PickFramebuffer::new(self.backend.context.clone())?,
        };
        let result = (|| {
            let pass_state = framebuffer.begin(&rectangle, viewport)?;
            let old_target = std::mem::replace(&mut self.framebuffer, pass_state.framebuffer.map(FrameResource::new));
            let old_viewport = std::mem::replace(&mut self.viewport, pass_state.viewport);
            let old_scissor = std::mem::replace(&mut self.scissor_test, pass_state.scissor_test);
            let old_blending = std::mem::replace(&mut self.blending_enabled, pass_state.blending_enabled);
            let old_invert = self.frame_state.invert_classification;
            let draw = self.draw_passes_with_culling(
                FramePasses { pick: true, ..Default::default() },
                Some(volume),
                Some(Color::TRANSPARENT),
            );
            self.framebuffer = old_target;
            self.viewport = old_viewport;
            self.scissor_test = old_scissor;
            self.blending_enabled = old_blending;
            self.frame_state.invert_classification = old_invert;
            // Reapply the caller's target even on shader/primitive errors.
            let restore = self
                .backend
                .set_render_target(self.framebuffer.as_ref(), self.viewport)
                .and_then(|()| self.backend.set_pass_overrides(self.scissor_test.clone(), self.blending_enabled));
            let result = draw.and_then(|()| read(&framebuffer, &rectangle));
            let end = self.backend.end_frame();
            restore?;
            end?;
            result
        })();
        self.backend.pick_framebuffer = Some(framebuffer);
        result
    }
    pub fn pick(
        &mut self,
        window_position: Cartesian2,
        width: Option<f64>,
        height: Option<f64>,
    ) -> Result<Option<PickedObject>> {
        Ok(self.pick_objects(window_position, width, height, 1)?.into_iter().next())
    }
    /// Collects visible object IDs within one rectangle in center-out order.
    pub fn pick_objects(
        &mut self,
        window_position: Cartesian2,
        width: Option<f64>,
        height: Option<f64>,
        limit: usize,
    ) -> Result<Vec<PickedObject>> {
        self.with_pick_framebuffer(window_position, width, height, |framebuffer, rectangle| {
            framebuffer.end(rectangle, limit)
        })
    }
    pub fn begin_pick_async(
        &mut self,
        window_position: Cartesian2,
        width: Option<f64>,
        height: Option<f64>,
        limit: usize,
    ) -> Result<PickReadback<G>> {
        self.with_pick_framebuffer(window_position, width, height, |framebuffer, rectangle| {
            framebuffer.end_async(rectangle, limit)
        })
    }
    pub async fn pick_async<S, F>(
        &mut self,
        window_position: Cartesian2,
        width: Option<f64>,
        height: Option<f64>,
        schedule: S,
    ) -> Result<Option<PickedObject>>
    where
        S: FnMut() -> F,
        F: Future<Output = Result<()>>,
    {
        Ok(self.begin_pick_async(window_position, width, height, 1)?.wait(schedule).await?.into_iter().next())
    }
}
