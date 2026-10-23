//! CesiumJS `Picking`: color-pick execution, rectangle and camera culling.
//! Color and depth-position picking share the Scene's View framebuffers.
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
    pub fn update_offscreen_camera_from_ray(
        ray: &Ray,
        width: Option<f64>,
        camera: &mut Camera,
    ) -> Result<CullingVolume> {
        if ray.direction.try_normalize().is_none() {
            return Err(RuntimeError::new("normalized result is not a number"));
        }
        if width.is_some_and(|width| !width.is_finite() || width <= 0.) {
            return Err(RuntimeError::new("width must be greater than zero"));
        }
        let direction = ray.direction;
        let right = direction.cross(direction.most_orthogonal_axis());
        let up = direction.cross(right);
        camera.position = ray.origin;
        camera.direction = direction;
        camera.up = up;
        camera.right = right;
        let CameraFrustum::Orthographic(frustum) = &mut camera.frustum else {
            return Err(RuntimeError::new("Offscreen picking requires an OrthographicFrustum"));
        };
        frustum.width = width.unwrap_or(0.1);
        let frustum = camera.frustum;
        Ok(frustum.compute_culling_volume(camera.position_wc(), camera.direction_wc(), camera.up_wc()))
    }
    pub fn ray_for_sample_height(ellipsoid: &Ellipsoid, cartographic: &Cartographic) -> Ray {
        let normal = ellipsoid.geodetic_surface_normal_cartographic(cartographic);
        Ray::new(
            ellipsoid.cartographic_to_cartesian(cartographic)
                + normal * crate::core::approximate_terrain_heights::DEFAULT_MAX_TERRAIN_HEIGHT,
            -normal,
        )
    }
    /// Execute successive pick passes while hiding previously picked geometry,
    /// features or primitives. Show handles are restored on success and error.
    pub fn drill_pick_results(
        limit: Option<usize>,
        mut pick: impl FnMut(usize) -> Result<Vec<super::RayPickResult>>,
    ) -> Result<Vec<super::RayPickResult>> {
        struct Restore(Vec<std::rc::Rc<std::cell::Cell<bool>>>);
        impl Drop for Restore {
            fn drop(&mut self) {
                for show in &self.0 {
                    show.set(true);
                }
            }
        }
        let limit = limit.unwrap_or(usize::MAX);
        let mut results = Vec::new();
        let mut restore = Restore(Vec::new());
        let mut hidden = std::collections::HashSet::new();
        loop {
            let picked = pick(limit.saturating_sub(results.len()))?;
            if picked.is_empty() {
                return Ok(results);
            }
            for result in picked {
                if result.position.is_some() && result.object.is_none() {
                    results.push(result);
                    return Ok(results);
                }
                let Some(object) = result.object.as_ref() else {
                    return Ok(results);
                };
                let Some(picked) = object.downcast_ref::<super::ScenePickObject>() else {
                    return Ok(results);
                };
                let handles = picked.show_handles();
                let identity = std::rc::Rc::as_ptr(object) as *const ();
                if !hidden.insert(identity) {
                    return Err(RuntimeError::new("Drill pick target did not hide its commands"));
                }
                if !result.exclude {
                    results.push(result);
                    if results.len() >= limit {
                        return Ok(results);
                    }
                }
                for show in handles {
                    show.set(false);
                    restore.0.push(show);
                }
            }
        }
    }
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
    fn ray_intersection(
        &mut self,
        ray: Ray,
        excluded: &[super::PickExclusion],
        width: Option<f64>,
        require_position: bool,
        most_detailed: bool,
    ) -> Result<Option<super::RayPickResult>> {
        if self.camera.mode() != SceneMode::Scene3D {
            return Err(RuntimeError::new("Ray intersections are only supported in 3D mode."));
        }
        let mut camera = if let Some(camera) = self.pick_offscreen_camera.take() {
            camera
        } else {
            let mut options =
                super::CameraOptions::new(self.camera.drawing_buffer_width, self.camera.drawing_buffer_height);
            options.client_width = self.camera.client_width;
            options.client_height = self.camera.client_height;
            options.map_projection = self.frame_state.uniform.map_projection.clone();
            options.ellipsoid = self.camera.ellipsoid;
            let mut camera = Camera::new(options);
            let mut frustum = OrthographicFrustum::new(0.1, 1.);
            frustum.near = 0.1;
            camera.frustum = CameraFrustum::Orthographic(frustum);
            camera
        };
        let culling = Picking::update_offscreen_camera_from_ray(&ray, width, &mut camera)?;
        let old_camera = std::mem::replace(&mut self.camera, camera);
        let offscreen = self.pick_offscreen_view.take().unwrap_or_default();
        let old_view = std::mem::replace(&mut self.view, offscreen);
        let old_resources = self.backend.view_resources.take();
        let previous = (
            self.framebuffer.take(),
            self.viewport,
            self.scissor_test.take(),
            self.blending_enabled,
            self.tileset_pick_pass,
        );
        let result = (|| {
            if let Some(resources) = self.view.resources::<G>() {
                self.backend.view_resources = Some(resources);
            } else {
                self.backend.initialize_view(&mut self.view, false)?;
            }
            let resources = self
                .view
                .resources::<G>()
                .ok_or_else(|| RuntimeError::new("Offscreen View resources are unavailable"))?;
            let rectangle = BoundingRectangle::new(0., 0., 1., 1.);
            let viewport = Viewport { x: 0, y: 0, width: 1, height: 1 };
            let mut framebuffer = resources
                .borrow_mut()
                .pick_framebuffer
                .take()
                .ok_or_else(|| RuntimeError::new("Offscreen pick framebuffer is unavailable"))?;
            let result = (|| {
                let pass = framebuffer.begin(&rectangle, viewport)?;
                self.framebuffer = pass.framebuffer.map(FrameResource::new);
                self.viewport = pass.viewport;
                self.scissor_test = pass.scissor_test;
                self.blending_enabled = pass.blending_enabled;
                self.tileset_pick_pass = Some(if most_detailed {
                    super::Cesium3DTilePass::MostDetailedPick
                } else {
                    super::Cesium3DTilePass::Pick
                });
                self.draw_passes_with_culling(
                    FramePasses { pick: true, offscreen: true, ..Default::default() },
                    Some(culling),
                    Some(Color::TRANSPARENT),
                )?;
                let object = framebuffer.end(&rectangle, 1)?.into_iter().next();
                let mut position = None;
                if self.capabilities.depth_texture {
                    let ranges = self.view.frustum_commands_list.iter().map(|f| (f.near, f.far)).collect::<Vec<_>>();
                    for (index, (near, far)) in ranges.into_iter().enumerate() {
                        let depth =
                            resources.borrow_mut().get_pick_depth(index)?.get_depth(&self.backend.context, 0., 0.)?;
                        if let Some(depth) = depth.filter(|d| *d > 0. && *d < 1.) {
                            let near = near * if index == 0 { 1. } else { 0.9999 };
                            position = Some(ray.get_point(near + depth * (far - near)));
                            break;
                        }
                    }
                }
                if object.is_none() && position.is_none() {
                    return Ok(None);
                }
                let exclude = (position.is_none() && require_position)
                    || object.as_ref().is_some_and(|object| excluded.iter().any(|excluded| excluded.matches(object)));
                Ok(Some(super::RayPickResult { object, position, exclude }))
            })();
            resources.borrow_mut().pick_framebuffer = Some(framebuffer);
            result
        })();
        self.pick_offscreen_camera = Some(std::mem::replace(&mut self.camera, old_camera));
        self.pick_offscreen_view = Some(std::mem::replace(&mut self.view, old_view));
        self.backend.view_resources = old_resources;
        (self.framebuffer, self.viewport, self.scissor_test, self.blending_enabled, self.tileset_pick_pass) = previous;
        let restore = self
            .backend
            .set_render_target(self.framebuffer.as_ref(), self.viewport)
            .and_then(|()| self.backend.set_pass_overrides(self.scissor_test.clone(), self.blending_enabled));
        let end = self.backend.end_frame();
        restore?;
        end?;
        result
    }
    pub(crate) fn drill_ray(
        &mut self,
        ray: Ray,
        limit: Option<usize>,
        excluded: &[super::PickExclusion],
        width: Option<f64>,
        require_position: bool,
        most_detailed: bool,
    ) -> Result<Vec<super::RayPickResult>> {
        Picking::drill_pick_results(limit, |_| {
            Ok(self.ray_intersection(ray, excluded, width, require_position, most_detailed)?.into_iter().collect())
        })
    }
    pub fn pick_from_ray(
        &mut self,
        ray: Ray,
        excluded: &[super::PickExclusion],
        width: Option<f64>,
    ) -> Result<Option<super::RayPickResult>> {
        Ok(self.drill_ray(ray, Some(1), excluded, width, false, false)?.into_iter().next())
    }
    pub fn drill_pick_from_ray(
        &mut self,
        ray: Ray,
        limit: Option<usize>,
        excluded: &[super::PickExclusion],
        width: Option<f64>,
    ) -> Result<Vec<super::RayPickResult>> {
        self.drill_ray(ray, limit, excluded, width, false, false)
    }
    pub fn sample_height(
        &mut self,
        position: &Cartographic,
        excluded: &[super::PickExclusion],
        width: Option<f64>,
    ) -> Result<Option<f64>> {
        if self.camera.mode() != SceneMode::Scene3D {
            return Err(RuntimeError::new("sampleHeight is only supported in 3D mode."));
        }
        if !self.sample_height_supported() {
            return Err(RuntimeError::new("sampleHeight requires depth texture support. Check sampleHeightSupported."));
        }
        let ellipsoid = self.camera.ellipsoid;
        let ray = Picking::ray_for_sample_height(&ellipsoid, position);
        let result = self.drill_ray(ray, Some(1), excluded, width, true, false)?.into_iter().next();
        Ok(result.and_then(|p| p.position).and_then(|p| ellipsoid.cartesian_to_cartographic(p)).map(|p| p.height))
    }
    pub fn clamp_to_height(
        &mut self,
        position: Cartesian3,
        excluded: &[super::PickExclusion],
        width: Option<f64>,
    ) -> Result<Option<Cartesian3>> {
        if self.camera.mode() != SceneMode::Scene3D {
            return Err(RuntimeError::new("clampToHeight is only supported in 3D mode."));
        }
        if !self.clamp_to_height_supported() {
            return Err(RuntimeError::new(
                "clampToHeight requires depth texture support. Check clampToHeightSupported.",
            ));
        }
        let ellipsoid = &self.camera.ellipsoid;
        let cartographic = ellipsoid
            .cartesian_to_cartographic(position)
            .ok_or_else(|| RuntimeError::new("Cartesian position has no cartographic coordinates"))?;
        let ray = Picking::ray_for_sample_height(ellipsoid, &cartographic);
        Ok(self.drill_ray(ray, Some(1), excluded, width, true, false)?.into_iter().next().and_then(|p| p.position))
    }
    pub fn drill_pick(
        &mut self,
        window_position: Cartesian2,
        limit: Option<usize>,
        width: Option<f64>,
        height: Option<f64>,
    ) -> Result<Vec<PickedObject>> {
        Ok(Picking::drill_pick_results(limit, |limit| {
            Ok(self
                .pick_objects(window_position, width, height, limit)?
                .into_iter()
                .map(|object| super::RayPickResult { object: Some(object), position: None, exclude: false })
                .collect())
        })?
        .into_iter()
        .filter_map(|result| result.object)
        .collect())
    }
    pub fn pick_voxel_coordinate(
        &mut self,
        window_position: Cartesian2,
        width: Option<f64>,
        height: Option<f64>,
    ) -> Result<[u8; 4]> {
        self.with_pick_pass(
            window_position,
            width,
            height,
            FramePasses { pick_voxel: true, ..Default::default() },
            |framebuffer, rectangle| framebuffer.read_center_pixel(rectangle),
        )
    }
    pub fn pick_metadata_with_info(
        &mut self,
        window_position: Cartesian2,
        info: super::PickedMetadataInfo,
    ) -> Result<super::MetadataValue> {
        let previous = self.metadata_pick.replace(info.clone());
        let previous_flag = self.frame_state.picking_metadata;
        let pixels = self.with_pick_pass(
            window_position,
            Some(1.),
            Some(1.),
            FramePasses { pick: true, ..Default::default() },
            |framebuffer, rectangle| framebuffer.read_center_pixel(rectangle),
        );
        self.metadata_pick = previous;
        self.frame_state.picking_metadata = previous_flag;
        let pixels = pixels?;
        super::MetadataPicking::decode_metadata_values(
            &info.class_property,
            info.value_transform.as_ref().map(|(offset, scale)| (offset, scale)),
            &pixels,
        )
        .map_err(|e| RuntimeError::new(e.to_string()))
    }
    pub fn pick_metadata(
        &mut self,
        window_position: Cartesian2,
        schema_id: Option<&str>,
        class_name: &str,
        property_name: &str,
    ) -> Result<Option<super::MetadataValue>> {
        let Some(object) = self.pick(window_position, None, None)? else {
            return Ok(None);
        };
        let Some(object) = object.downcast_ref::<super::ScenePickObject>() else {
            return Ok(None);
        };
        let info = object
            .metadata
            .iter()
            .find(|((schema, class, property), _)| {
                schema_id.is_none_or(|id| schema.as_deref() == Some(id))
                    && class == class_name
                    && property == property_name
            })
            .map(|(_, info)| info.clone());
        info.map(|info| self.pick_metadata_with_info(window_position, info)).transpose()
    }
    pub fn pick_position_world_coordinates(&mut self, window_position: Cartesian2) -> Result<Option<Cartesian3>> {
        if !self.use_depth_picking {
            return Ok(None);
        }
        if !self.pick_position_supported() {
            return Err(RuntimeError::new(
                "Picking from the depth buffer is not supported. Check pickPositionSupported.",
            ));
        }
        let key = format!(
            "({}, {})",
            math::js_number_to_string(window_position.x),
            math::js_number_to_string(window_position.y)
        );
        if let Some(result) = self.pick_position_cache.get(&key) {
            return Ok(*result);
        }
        let [width, height] = self.backend.dimensions()?;
        let state = SceneTransformState {
            mode: self.camera.mode(),
            morph_time: self.frame_state.morph_time,
            map_projection: self.camera.map_projection(),
            client_dimensions: Cartesian2::new(self.camera.client_width, self.camera.client_height),
            drawing_buffer_dimensions: Cartesian2::new(width as f64, height as f64),
        };
        let mut position = SceneTransforms::transform_window_to_drawing_buffer(&state, window_position);
        let resources =
            self.view.resources::<G>().ok_or_else(|| RuntimeError::new("View resources are unavailable"))?;
        if self.pick_translucent_depth {
            let viewport = Viewport { x: 0, y: 0, width, height };
            let pass =
                resources.borrow_mut().pick_depth_framebuffer.update(&self.backend.context, position, viewport)?;
            let mode = self.camera.mode();
            let culling = Picking::get_pick_culling_volume(
                &mut self.camera,
                mode,
                position,
                1.,
                1.,
                &BoundingRectangle::new(0., 0., width as f64, height as f64),
            )?;
            let previous = (self.framebuffer.take(), self.viewport, self.scissor_test.take(), self.blending_enabled);
            self.framebuffer = pass.framebuffer.map(FrameResource::new);
            self.viewport = pass.viewport;
            self.scissor_test = pass.scissor_test;
            self.blending_enabled = pass.blending_enabled;
            let draw = self.draw_passes_with_culling(
                FramePasses { pick: true, depth: true, ..Default::default() },
                Some(culling),
                Some(Color::TRANSPARENT),
            );
            (self.framebuffer, self.viewport, self.scissor_test, self.blending_enabled) = previous;
            let restore = self
                .backend
                .set_render_target(self.framebuffer.as_ref(), self.viewport)
                .and_then(|()| self.backend.set_pass_overrides(self.scissor_test.clone(), self.blending_enabled));
            let end = self.backend.end_frame();
            restore?;
            end?;
            draw?;
        } else {
            self.update_frame_state();
            self.backend.context.uniform_state().update(&self.frame_state.uniform, width)?;
        }
        position.y = height as f64 - position.y;
        let viewport = self.viewport.unwrap_or(Viewport { x: 0, y: 0, width, height });
        let viewport =
            BoundingRectangle::new(viewport.x as f64, viewport.y as f64, viewport.width as f64, viewport.height as f64);
        let ranges = self.view.frustum_commands_list.iter().map(|f| (f.near, f.far)).collect::<Vec<_>>();
        let mut result = None;
        for (index, (near, far)) in ranges.into_iter().enumerate() {
            let depth = resources.borrow_mut().get_pick_depth(index)?.get_depth(
                &self.backend.context,
                position.x,
                position.y,
            )?;
            if let Some(depth) = depth.filter(|d| *d > 0. && *d < 1.) {
                let near = if self.camera.mode() == SceneMode::Scene2D || index == 0 { near } else { near * 0.9999 };
                self.backend.set_frustum(&self.frame_state, near, far)?;
                let uniforms = self.backend.context.uniform_state().clone();
                result = Some(SceneTransforms::drawing_buffer_to_world_coordinates(
                    self.camera.frustum,
                    &uniforms,
                    &viewport,
                    position,
                    depth,
                    self.frame_state.use_log_depth,
                )?);
                if self.camera.mode() == SceneMode::Scene2D {
                    self.backend.context.uniform_state().update(&self.frame_state.uniform, width)?;
                }
                break;
            }
        }
        self.pick_position_cache.insert(key, result);
        Ok(result)
    }
    pub fn pick_position(&mut self, window_position: Cartesian2) -> Result<Option<Cartesian3>> {
        let Some(p) = self.pick_position_world_coordinates(window_position)? else {
            return Ok(None);
        };
        if self.camera.mode() == SceneMode::Scene3D {
            return Ok(Some(p));
        }
        let projection = self.camera.map_projection();
        Ok(Some(
            projection.ellipsoid().cartographic_to_cartesian(&projection.unproject(Cartesian3::new(p.y, p.z, p.x))),
        ))
    }
    fn with_pick_framebuffer<T>(
        &mut self,
        window_position: Cartesian2,
        width: Option<f64>,
        height: Option<f64>,
        read: impl FnOnce(&PickFramebuffer<G>, &BoundingRectangle) -> Result<T>,
    ) -> Result<T> {
        self.with_pick_pass(window_position, width, height, FramePasses { pick: true, ..Default::default() }, read)
    }
    fn with_pick_pass<T>(
        &mut self,
        window_position: Cartesian2,
        width: Option<f64>,
        height: Option<f64>,
        passes: FramePasses,
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
        let resources =
            self.backend.view_resources.clone().ok_or_else(|| RuntimeError::new("View resources are unavailable"))?;
        let mut framebuffer = match resources.borrow_mut().pick_framebuffer.take() {
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
            let draw = self.draw_passes_with_culling(passes, Some(volume), Some(Color::TRANSPARENT));
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
        resources.borrow_mut().pick_framebuffer = Some(framebuffer);
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
