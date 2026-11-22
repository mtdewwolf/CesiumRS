//! CesiumJS `DepthPlane`: computes the horizon depth quad for the ellipsoid
//! and renders depth-only to prevent rendering behind the globe.
//!
//! The update follows Cesium step for step: create the render state and draw
//! command once, (re)create the shader program when `useLogDepth` changes,
//! compute the depth quad every frame, create the vertex array on first use
//! and otherwise rewrite the position buffer with `bufferSubData`. Checked
//! against a SwiftShader capture of the pinned engine in
//! `tests/scene_single_renderables.rs`.

use std::{collections::BTreeMap, rc::Rc};

use super::{
    camera::Camera,
    frame_state::FrameState,
    scene::{Result, SceneCommand, SceneDrawOptions},
    scene_mode::SceneMode,
};
use crate::{
    core::{BoundingSphere, Cartesian3, Ellipsoid, Geometry, GeometryAttribute, GeometryAttributes, PrimitiveType, RuntimeError, TypedArray},
    renderer::{
        render_state::{ColorMask, Cull, DepthTest},
        AttributeSource, BufferUsage, Context, DrawCommand, DrawMetadata, GeometryUploadOptions, Gl, Pass, PassState,
        RenderState, ShaderProgram, ShaderProgramOptions, ShaderSource, VertexArray,
    },
    shaders::root::{DEPTH_PLANE_FS, DEPTH_PLANE_VS},
};

/// Computes the depth quad coordinates for an ellipsoid horizon from the camera
/// view. Returns 12 `f32` numbers representing the 4 corners:
/// `[upperLeft(3), lowerLeft(3), upperRight(3), lowerRight(3)]`.
///
/// A perspective camera inside the ellipsoid cannot form a tangent plane, so
/// the orthographic-style quad (`center = ZERO`, `east = rightWC`,
/// `north = upWC`) is used as the fallback.
pub fn compute_depth_quad(ellipsoid: &Ellipsoid, camera: &mut Camera) -> [f32; 12] {
    let radii = ellipsoid.radii;
    let is_ortho = matches!(camera.frustum, super::camera::CameraFrustum::Orthographic(_) | super::camera::CameraFrustum::OrthographicOffCenter(_));

    let (center, east_offset, north_offset) = if is_ortho {
        (Cartesian3::ZERO, camera.right_wc(), camera.up_wc())
    } else {
        let p = camera.position_wc();
        let q = Cartesian3::multiply_components(&ellipsoid.one_over_radii, p);
        let q_magnitude = q.magnitude();

        if q_magnitude <= 1.0 {
            // Camera is inside or on the ellipsoid surface.
            (Cartesian3::ZERO, camera.right_wc(), camera.up_wc())
        } else {
            let q_unit = q.normalize();
            let e_unit = Cartesian3::cross(&Cartesian3::UNIT_Z, q).normalize();
            let n_unit = Cartesian3::cross(&q_unit, e_unit).normalize();
            let w_magnitude = (q_magnitude * q_magnitude - 1.0).sqrt();

            let center = q_unit.multiply_by_scalar(1.0 / q_magnitude);
            let scalar = w_magnitude / q_magnitude;
            let east_offset = e_unit.multiply_by_scalar(scalar);
            let north_offset = n_unit.multiply_by_scalar(scalar);
            (center, east_offset, north_offset)
        }
    };

    let upper_left = Cartesian3::multiply_components(&radii, center.add(north_offset).subtract(east_offset));
    let lower_left = Cartesian3::multiply_components(&radii, center.subtract(north_offset).subtract(east_offset));
    let upper_right = Cartesian3::multiply_components(&radii, center.add(north_offset).add(east_offset));
    let lower_right = Cartesian3::multiply_components(&radii, center.subtract(north_offset).add(east_offset));

    [
        upper_left.x as f32,
        upper_left.y as f32,
        upper_left.z as f32,
        lower_left.x as f32,
        lower_left.y as f32,
        lower_left.z as f32,
        upper_right.x as f32,
        upper_right.y as f32,
        upper_right.z as f32,
        lower_right.x as f32,
        lower_right.y as f32,
        lower_right.z as f32,
    ]
}

/// `frameState.context` cast to the concrete renderer type.
pub(crate) fn context_of<G: Gl + 'static>(frame: &FrameState) -> Result<Context<G>> {
    frame
        .context
        .as_ref()
        .and_then(|c| c.get::<Context<G>>())
        .cloned()
        .ok_or_else(|| RuntimeError::new("frameState.context is required."))
}

/// Renders a depth-only plane behind the globe to mask objects on the far side
/// of the Earth.
pub struct DepthPlane<G: Gl + 'static> {
    pub ellipsoid_offset: f64,
    pub mode: Option<SceneMode>,
    pub use_log_depth: bool,
    pub render_state: RenderState,
    pub bounding_volume: BoundingSphere,
    vertex_array: Option<Rc<VertexArray<G>>>,
    shader_program: Option<Rc<ShaderProgram<G>>>,
    command: Option<SceneCommand>,
    destroyed: bool,
}

impl<G: Gl + 'static> Default for DepthPlane<G> {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl<G: Gl + 'static> DepthPlane<G> {
    pub fn new(ellipsoid_offset: f64) -> Self {
        let render_state = RenderState {
            cull: Cull { enabled: true, ..Default::default() },
            depth_test: DepthTest { enabled: true, ..Default::default() },
            color_mask: ColorMask { red: false, green: false, blue: false, alpha: false },
            ..Default::default()
        };

        Self {
            ellipsoid_offset,
            mode: None,
            use_log_depth: false,
            render_state,
            bounding_volume: BoundingSphere::new(Cartesian3::ZERO, 0.0),
            vertex_array: None,
            shader_program: None,
            command: None,
            destroyed: false,
        }
    }

    /// Cesium's `DepthPlane.prototype.update`.
    pub fn update(&mut self, camera: &mut Camera, frame_state: &FrameState) -> Result<()> {
        self.mode = Some(frame_state.mode);
        if frame_state.mode != SceneMode::Scene3D {
            return Ok(());
        }

        let context = context_of::<G>(frame_state)?;
        // Allow offsetting the ellipsoid radius to address rendering artifacts
        // below ellipsoid zero elevation.
        let radii = frame_state.uniform.map_projection.ellipsoid().radii;
        let ellipsoid = Ellipsoid::new(
            radii.x + self.ellipsoid_offset,
            radii.y + self.ellipsoid_offset,
            radii.z + self.ellipsoid_offset,
        );

        let use_log_depth = frame_state.use_log_depth;

        if self.command.is_none() {
            self.bounding_volume = BoundingSphere::new(Cartesian3::ZERO, ellipsoid.maximum_radius);
        }

        let program_replaced = self.shader_program.is_none() || self.use_log_depth != use_log_depth;
        if program_replaced {
            self.use_log_depth = use_log_depth;

            let mut vs = ShaderSource { sources: vec![DEPTH_PLANE_VS.source.to_string()], ..Default::default() };
            let mut fs = ShaderSource { sources: vec![DEPTH_PLANE_FS.source.to_string()], ..Default::default() };
            if use_log_depth {
                fs.defines.push("LOG_DEPTH".into());
                vs.defines.push("LOG_DEPTH".into());
            }

            let options = ShaderProgramOptions {
                vertex_shader_source: vs,
                fragment_shader_source: fs,
                attribute_locations: BTreeMap::from([("position".to_string(), 0)]),
            };
            self.shader_program = Some(
                context.with_shader_cache(|c| c.replace_shader_program(self.shader_program.clone(), &options))?,
            );
        }

        // update depth plane
        let depth_quad = compute_depth_quad(&ellipsoid, camera);

        if self.vertex_array.is_none() {
            let mut attributes = GeometryAttributes::default();
            attributes.position =
                Some(GeometryAttribute::floats(3, &depth_quad.iter().map(|&v| f64::from(v)).collect::<Vec<_>>()));
            let geometry =
                Geometry::new(attributes, Some(TypedArray::Uint16(vec![0, 1, 2, 2, 1, 3])), PrimitiveType::Triangles);
            let va = VertexArray::from_geometry(
                &context,
                &geometry,
                GeometryUploadOptions {
                    buffer_usage: BufferUsage::DynamicDraw,
                    attribute_locations: [("position".to_string(), 0)].into_iter().collect(),
                    interleave: false,
                    vertex_array_attributes: vec![],
                },
            )?;
            self.vertex_array = Some(Rc::new(va));
        } else {
            let bytes: Vec<u8> = depth_quad.iter().flat_map(|v| v.to_le_bytes()).collect();
            if let Some(AttributeSource::Buffer(buffer)) =
                self.vertex_array.as_ref().and_then(|va| va.get_attribute(0)).map(|a| &a.source)
            {
                buffer.copy_from_array_view(&bytes, 0)?;
            }
        }

        // Build (or rebuild after a program replacement) the draw command.
        if self.command.is_none() || program_replaced {
            let mut metadata = DrawMetadata::default();
            metadata.bounding_volume = Some(self.bounding_volume);
            let mut options =
                SceneDrawOptions::new(self.vertex_array.clone().expect("vertex array"), self.shader_program.clone().expect("shader program"));
            options.state = self.render_state.clone();
            self.command = Some(SceneCommand::from_draw("depth-plane", context, Pass::Opaque, metadata, options));
        }

        Ok(())
    }

    /// Cesium's `DepthPlane.prototype.execute`.
    pub fn execute(&self, context: &Context<G>, pass_state: &PassState<G>) -> Result<()> {
        if self.mode != Some(SceneMode::Scene3D) {
            return Ok(());
        }
        let (Some(va), Some(program)) = (self.vertex_array.as_ref(), self.shader_program.as_ref()) else {
            return Ok(());
        };
        let mut command = DrawCommand::new(va.as_ref(), program.as_ref());
        command.render_state = self.render_state.clone();
        command.metadata.bounding_volume = Some(self.bounding_volume);
        command.execute(context, pass_state)
    }

    /// The queued draw command, once the first 3D update has run.
    pub fn command(&self) -> Option<&SceneCommand> {
        self.command.as_ref()
    }

    pub fn vertex_array(&self) -> Option<&Rc<VertexArray<G>>> {
        self.vertex_array.as_ref()
    }

    pub fn shader_program(&self) -> Option<&Rc<ShaderProgram<G>>> {
        self.shader_program.as_ref()
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// Cesium's `DepthPlane.prototype.destroy`.
    pub fn destroy(&mut self) {
        if self.destroyed {
            return;
        }
        if let Some(va) = self.vertex_array.take() {
            let _ = va.destroy();
        }
        self.shader_program = None;
        self.command = None;
        self.destroyed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Cartographic;
    use crate::renderer::GlValue;
    use crate::scene::camera::CameraOptions;

    #[derive(Default)]
    struct TestGl;
    impl Gl for TestGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2]> {
            Ok([256, 256])
        }
        fn invoke(&self, method: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>> {
            if method.starts_with("create") {
                Ok(GlValue::Object(1))
            } else {
                Ok(GlValue::Null)
            }
        }
    }

    #[test]
    fn default_depth_plane_properties() {
        let dp = DepthPlane::<TestGl>::default();
        assert_eq!(dp.ellipsoid_offset, 0.0);
        assert!(dp.render_state.cull.enabled);
        assert!(dp.render_state.depth_test.enabled);
        assert!(!dp.render_state.color_mask.red);
        assert!(!dp.render_state.color_mask.alpha);
        assert!(!dp.is_destroyed());
    }

    #[test]
    fn computes_depth_quad_from_orbit() {
        let ellipsoid = Ellipsoid::WGS84;
        let mut camera = Camera::new(CameraOptions::new(1024.0, 768.0));
        let pos = ellipsoid.cartographic_to_cartesian(&Cartographic::from_degrees(0.0, 0.0, 10_000_000.0));
        camera.position = pos;
        camera.direction = Cartesian3::new(-1.0, 0.0, 0.0);
        camera.up = Cartesian3::UNIT_Z;
        camera.right = Cartesian3::UNIT_Y;

        let quad = compute_depth_quad(&ellipsoid, &mut camera);
        assert_eq!(quad.len(), 12);

        for val in quad {
            assert!(val.is_finite());
        }
    }
}
