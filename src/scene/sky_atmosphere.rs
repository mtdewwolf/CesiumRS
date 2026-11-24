//! CesiumJS `SkyAtmosphere`: an atmosphere drawn around the limb of an
//! ellipsoid, based on Nishita et al. 1993. Only supported in 3D; faded out
//! when morphing to 2D or Columbus view.
//!
//! A unit ellipsoid geometry is scaled by the outer-ellipsoid scale matrix and
//! reoriented every frame so it faces the camera, matching Cesium's
//! per-vertex/per-fragment scattering shader selection.

use super::{
    blending_state,
    scene::{SceneCommand, SceneDrawOptions},
    Axis, DynamicAtmosphereLightingType, FrameState, GlobeTranslucency, SceneMode,
};
use crate::{
    core::{
        math::{equals_epsilon, EPSILON7, PI, TWO_PI},
        Cartesian3, Ellipsoid, EllipsoidGeometry, Matrix4, RuntimeError, VertexFormat,
    },
    renderer::{
        render_state::Cull, Context, DrawMetadata, GeometryUploadOptions, Gl, Pass, RenderState, ShaderProgram,
        ShaderProgramOptions, ShaderSource, UniformValue, VertexArray,
    },
    shaders::root::{ATMOSPHERE_COMMON, SKY_ATMOSPHERE_COMMON, SKY_ATMOSPHERE_FS, SKY_ATMOSPHERE_VS},
};
use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

/// The outer-ellipsoid scale applied to the atmosphere's radii, as in Cesium.
const OUTER_ELLIPSOID_SCALE: f64 = 1.025;

/// An atmosphere drawn around the limb of the provided ellipsoid.
pub struct SkyAtmosphere<G: Gl> {
    pub show: bool,
    /// Compute atmosphere per-fragment instead of per-vertex.
    pub per_fragment_atmosphere: bool,
    pub atmosphere_light_intensity: f64,
    pub atmosphere_rayleigh_coefficient: Cartesian3,
    pub atmosphere_mie_coefficient: Cartesian3,
    pub atmosphere_rayleigh_scale_height: f64,
    pub atmosphere_mie_scale_height: f64,
    pub atmosphere_mie_anisotropy: f64,
    pub hue_shift: f64,
    pub saturation_shift: f64,
    pub brightness_shift: f64,
    ellipsoid: Ellipsoid,
    context: Context<G>,
    scale_matrix: Matrix4,
    model_matrix: Matrix4,
    radii_and_dynamic_atmosphere_color: Cartesian3,
    vertex_array: Option<Rc<VertexArray<G>>>,
    shader_program: Option<Rc<ShaderProgram<G>>>,
    flags: Option<u32>,
    destroyed: bool,
}

impl<G: Gl + 'static> SkyAtmosphere<G> {
    /// Creates an atmosphere drawn around `ellipsoid` (defaults to WGS84).
    pub fn new(context: Context<G>, ellipsoid: Option<Ellipsoid>) -> Self {
        let ellipsoid = ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let scale_vector = ellipsoid.radii.multiply_by_scalar(OUTER_ELLIPSOID_SCALE);
        // A zero dynamic-lighting flag treats the sun as directly overhead.
        let radii_and_dynamic_atmosphere_color =
            Cartesian3::new(ellipsoid.maximum_radius * OUTER_ELLIPSOID_SCALE, ellipsoid.maximum_radius, 0.0);
        Self {
            show: true,
            per_fragment_atmosphere: false,
            atmosphere_light_intensity: 50.0,
            atmosphere_rayleigh_coefficient: Cartesian3::new(5.5e-6, 13.0e-6, 28.4e-6),
            atmosphere_mie_coefficient: Cartesian3::new(21e-6, 21e-6, 21e-6),
            atmosphere_rayleigh_scale_height: 10000.0,
            atmosphere_mie_scale_height: 3200.0,
            atmosphere_mie_anisotropy: 0.9,
            hue_shift: 0.0,
            saturation_shift: 0.0,
            brightness_shift: 0.0,
            ellipsoid,
            context,
            scale_matrix: Matrix4::from_scale(scale_vector),
            model_matrix: Matrix4::IDENTITY,
            radii_and_dynamic_atmosphere_color,
            vertex_array: None,
            shader_program: None,
            flags: None,
            destroyed: false,
        }
    }

    /// The ellipsoid the atmosphere is drawn around.
    pub fn ellipsoid(&self) -> Ellipsoid {
        self.ellipsoid
    }

    /// The current model matrix, set by the last [`SkyAtmosphere::update`].
    pub fn model_matrix(&self) -> &Matrix4 {
        &self.model_matrix
    }

    /// The outer-ellipsoid scale matrix.
    pub fn scale_matrix(&self) -> &Matrix4 {
        &self.scale_matrix
    }

    /// The packed outer radius, inner radius and dynamic-lighting flag.
    pub fn radii_and_dynamic_atmosphere_color(&self) -> Cartesian3 {
        self.radii_and_dynamic_atmosphere_color
    }

    /// The shader defines used by the current program (`None` before the
    /// first update).
    pub fn shader_defines(&self) -> Option<&[String]> {
        self.shader_program
            .as_ref()
            .and_then(|program| program.source_options())
            .map(|options| options.fragment_shader_source.defines.as_slice())
    }

    /// Sets the dynamic-lighting enum value for the shader.
    pub fn set_dynamic_lighting(&mut self, lighting: DynamicAtmosphereLightingType) {
        self.radii_and_dynamic_atmosphere_color.z = lighting.value() as f64;
    }

    /// `SkyAtmosphere.prototype.update`. `globe_show` is the globe's `show`
    /// flag: `None` means there is no globe (per-fragment atmosphere).
    pub fn update(
        &mut self,
        frame: &FrameState,
        globe_show: Option<bool>,
    ) -> Result<Option<SceneCommand>, RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if !self.show {
            return Ok(None);
        }
        if !matches!(frame.mode, SceneMode::Scene3D | SceneMode::Morphing) {
            return Ok(None);
        }
        if !frame.passes.render {
            return Ok(None);
        }

        // Align the ellipsoid geometry so it always faces the camera to reduce
        // per-vertex atmosphere artifacts.
        let inverse_view_rotation = frame.uniform.camera.inverse_view.get_matrix3();
        let rotation_matrix = Matrix4::from_rotation_translation(&inverse_view_rotation, Cartesian3::ZERO);
        let rotation_offset_matrix = rotation_matrix.multiply_transformation(&Axis::Y_UP_TO_Z_UP);
        self.model_matrix = self.scale_matrix.multiply(&rotation_offset_matrix);

        let color_correct = has_color_correction(self);
        let translucent = frame
            .globe_translucency_state
            .as_ref()
            .and_then(|state| state.get::<GlobeTranslucency>())
            .is_some_and(|translucency| translucency.enabled);
        let per_fragment_atmosphere = self.per_fragment_atmosphere || translucent || globe_show != Some(true);

        if self.vertex_array.is_none() {
            let geometry = EllipsoidGeometry::new(
                Cartesian3::ONE,
                None,
                0.0,
                TWO_PI,
                0.0,
                PI,
                256.0,
                256.0,
                VertexFormat::POSITION_ONLY,
                None,
            )
            .and_then(|geometry| geometry.create_geometry())
            .ok_or_else(|| RuntimeError::new("Failed to build the SkyAtmosphere ellipsoid geometry"))?;
            let vertices = VertexArray::from_geometry(
                &self.context,
                &geometry,
                GeometryUploadOptions {
                    attribute_locations: HashMap::from([("position".to_string(), 0)]),
                    ..Default::default()
                },
            )?;
            self.vertex_array = Some(Rc::new(vertices));
        }

        let flags = u32::from(color_correct) | ((per_fragment_atmosphere as u32) << 2) | ((translucent as u32) << 3);
        if self.flags != Some(flags) {
            self.flags = Some(flags);
            let mut defines = Vec::new();
            if color_correct {
                defines.push("COLOR_CORRECT".to_string());
            }
            if per_fragment_atmosphere {
                defines.push("PER_FRAGMENT_ATMOSPHERE".to_string());
            }
            if translucent {
                defines.push("GLOBE_TRANSLUCENT".to_string());
            }
            if let Some(old) = self.shader_program.take() {
                let _ = old.destroy();
            }
            let vertex = ShaderSource {
                sources: vec![
                    ATMOSPHERE_COMMON.source.to_string(),
                    SKY_ATMOSPHERE_COMMON.source.to_string(),
                    SKY_ATMOSPHERE_VS.source.to_string(),
                ],
                defines: defines.clone(),
                ..Default::default()
            };
            let fragment = ShaderSource {
                sources: vec![
                    ATMOSPHERE_COMMON.source.to_string(),
                    SKY_ATMOSPHERE_COMMON.source.to_string(),
                    SKY_ATMOSPHERE_FS.source.to_string(),
                ],
                defines,
                ..Default::default()
            };
            let program = ShaderProgram::from_sources(
                &self.context,
                &ShaderProgramOptions {
                    vertex_shader_source: vertex,
                    fragment_shader_source: fragment,
                    attribute_locations: BTreeMap::from([("position".to_string(), 0)]),
                },
            )?;
            self.shader_program = Some(Rc::new(program));
        }

        let vertices = self.vertex_array.as_ref().expect("built above").clone();
        let program = self.shader_program.as_ref().expect("compiled above").clone();
        let mut options = SceneDrawOptions::new(vertices, program);
        options.primitive_type = crate::core::PrimitiveType::Triangles;
        options.state = RenderState {
            cull: Cull { enabled: true, face: super::CullFace::Front.value() },
            blending: blending_state::alpha_blend(),
            depth_mask: false,
            ..Default::default()
        };
        let radii = &self.radii_and_dynamic_atmosphere_color;
        let rayleigh = &self.atmosphere_rayleigh_coefficient;
        let mie = &self.atmosphere_mie_coefficient;
        options.uniforms = vec![
            (
                "u_radiiAndDynamicAtmosphereColor".into(),
                UniformValue::Float(vec![radii.x as f32, radii.y as f32, radii.z as f32]),
            ),
            (
                "u_hsbShift".into(),
                UniformValue::Float(vec![
                    self.hue_shift as f32,
                    self.saturation_shift as f32,
                    self.brightness_shift as f32,
                ]),
            ),
            ("u_atmosphereLightIntensity".into(), UniformValue::Float(vec![self.atmosphere_light_intensity as f32])),
            (
                "u_atmosphereRayleighCoefficient".into(),
                UniformValue::Float(vec![rayleigh.x as f32, rayleigh.y as f32, rayleigh.z as f32]),
            ),
            ("u_atmosphereMieCoefficient".into(), UniformValue::Float(vec![mie.x as f32, mie.y as f32, mie.z as f32])),
            (
                "u_atmosphereRayleighScaleHeight".into(),
                UniformValue::Float(vec![self.atmosphere_rayleigh_scale_height as f32]),
            ),
            ("u_atmosphereMieScaleHeight".into(), UniformValue::Float(vec![self.atmosphere_mie_scale_height as f32])),
            ("u_atmosphereMieAnisotropy".into(), UniformValue::Float(vec![self.atmosphere_mie_anisotropy as f32])),
        ];

        let metadata = DrawMetadata {
            model_matrix: Some(self.model_matrix),
            cull: false,
            owner: Some("SkyAtmosphere".into()),
            ..Default::default()
        };
        Ok(Some(SceneCommand::from_draw("SkyAtmosphere", self.context.clone(), Pass::Environment, metadata, options)))
    }

    /// Destroys the WebGL resources held by this atmosphere.
    pub fn destroy(&mut self) {
        if let Some(vertices) = self.vertex_array.take() {
            let _ = vertices.destroy();
        }
        if let Some(program) = self.shader_program.take() {
            let _ = program.destroy();
        }
        self.destroyed = true;
    }

    /// Returns `true` if this atmosphere has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}

fn has_color_correction(atmosphere: &SkyAtmosphere<impl Gl>) -> bool {
    !(equals_epsilon(atmosphere.hue_shift, 0.0, EPSILON7, EPSILON7)
        && equals_epsilon(atmosphere.saturation_shift, 0.0, EPSILON7, EPSILON7)
        && equals_epsilon(atmosphere.brightness_shift, 0.0, EPSILON7, EPSILON7))
}
