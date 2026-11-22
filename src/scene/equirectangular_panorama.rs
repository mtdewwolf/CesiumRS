//! CesiumJS `EquirectangularPanorama`: displays 360-degree equirectangular
//! imagery projected onto an enclosing sphere.

use super::cull_face::CullFace;
use super::frame_state::FrameState;
use super::material::{Material, MaterialImage, MaterialValue, IMAGE_TYPE};
use super::scene::{Result, SceneCommand, SceneDrawOptions, ScenePrimitive};
use super::scene_mode::SceneMode;
use crate::core::{
    math::{PI, TWO_PI},
    BoundingSphere, Cartesian2, Cartesian3, Credit, EllipsoidGeometry, Matrix4, PrimitiveType, RuntimeError,
    VertexFormat,
};
use crate::renderer::render_state::{Cull, DepthTest};
use crate::renderer::{
    Context, DrawMetadata, GeometryUploadOptions, Gl, Pass, RenderState, ShaderProgram, ShaderProgramOptions,
    ShaderSource, VertexArray,
};
use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

const DEFAULT_RADIUS: f64 = 100_000.0;

const EQUIRECTANGULAR_PANORAMA_VS: &str = "\
in vec3 position;\n\
in vec2 st;\n\
out vec2 v_st;\n\
void main()\n\
{\n\
    v_st = st;\n\
    vec4 positionEC = czm_modelView * vec4(position, 1.0);\n\
    gl_Position = czm_projection * positionEC;\n\
    czm_vertexLogDepth();\n\
}\n";

const EQUIRECTANGULAR_PANORAMA_FS: &str = "\
in vec2 v_st;\n\
void main()\n\
{\n\
    czm_materialInput materialInput;\n\
    materialInput.s = v_st.s;\n\
    materialInput.st = v_st;\n\
    materialInput.str = vec3(v_st, 0.0);\n\
    czm_material material = czm_getMaterial(materialInput);\n\
    out_FragColor = vec4(material.diffuse, material.alpha);\n\
    czm_writeLogDepth();\n\
}\n";

struct EquirectangularGpu<G: Gl> {
    context: Context<G>,
    vertex_array: Option<Rc<VertexArray<G>>>,
    built_radius: Option<f64>,
    shader_program: Option<Rc<ShaderProgram<G>>>,
    material_shader: Option<String>,
    use_log_depth: Option<bool>,
}

impl<G: Gl + 'static> EquirectangularGpu<G> {
    fn new(context: Context<G>) -> Self {
        Self {
            context,
            vertex_array: None,
            built_radius: None,
            shader_program: None,
            material_shader: None,
            use_log_depth: None,
        }
    }

    fn build_command(
        &mut self,
        radius: f64,
        transform: Matrix4,
        bounding_sphere: BoundingSphere,
        material: &Material,
        use_log_depth: bool,
    ) -> Result<SceneCommand> {
        if self.vertex_array.is_none() || self.built_radius != Some(radius) {
            if let Some(old) = self.vertex_array.take() {
                let _ = old.destroy();
            }
            let ellip = EllipsoidGeometry::new(
                Cartesian3::new(radius, radius, radius),
                None,
                0.0,
                TWO_PI,
                0.0,
                PI,
                64.0,
                64.0,
                VertexFormat::POSITION_AND_ST,
                None,
            )
            .ok_or_else(|| RuntimeError::new("Failed to build EquirectangularPanorama sphere"))?;
            let geometry = ellip
                .create_geometry()
                .ok_or_else(|| RuntimeError::new("Failed to create EquirectangularPanorama geometry"))?;
            let va = VertexArray::from_geometry(
                &self.context,
                &geometry,
                GeometryUploadOptions {
                    attribute_locations: HashMap::from([("position".to_string(), 0), ("st".to_string(), 1)]),
                    ..Default::default()
                },
            )?;
            self.vertex_array = Some(Rc::new(va));
            self.built_radius = Some(radius);
        }

        if self.shader_program.is_none()
            || self.material_shader.as_deref() != Some(material.shader_source.as_str())
            || self.use_log_depth != Some(use_log_depth)
        {
            if let Some(old) = self.shader_program.take() {
                let _ = old.destroy();
            }
            let mut vs = ShaderSource::new(EQUIRECTANGULAR_PANORAMA_VS);
            let mut fs = ShaderSource {
                sources: vec![material.shader_source.clone(), EQUIRECTANGULAR_PANORAMA_FS.into()],
                ..Default::default()
            };
            if use_log_depth {
                vs.defines.push("LOG_DEPTH".into());
                fs.defines.push("LOG_DEPTH".into());
            }
            let program = ShaderProgram::from_sources(
                &self.context,
                &ShaderProgramOptions {
                    vertex_shader_source: vs,
                    fragment_shader_source: fs,
                    attribute_locations: BTreeMap::from([("position".to_string(), 0), ("st".to_string(), 1)]),
                },
            )?;
            self.shader_program = Some(Rc::new(program));
            self.material_shader = Some(material.shader_source.clone());
            self.use_log_depth = Some(use_log_depth);
        }

        let va = self.vertex_array.as_ref().expect("built above").clone();
        let program = self.shader_program.as_ref().expect("compiled above").clone();
        let mut draw_opts = SceneDrawOptions::new(va, program);
        draw_opts.primitive_type = PrimitiveType::Triangles;
        draw_opts.state = RenderState {
            cull: Cull { enabled: true, face: CullFace::Front.value() },
            depth_test: DepthTest { enabled: true, ..Default::default() },
            depth_mask: false,
            ..Default::default()
        };
        draw_opts.uniforms = material.uniform_values();
        draw_opts.textures = material.resolve_sampled_textures(&self.context)?;

        let metadata = DrawMetadata {
            bounding_volume: Some(bounding_sphere),
            model_matrix: Some(transform),
            cull: false,
            owner: Some("EquirectangularPanorama".into()),
            ..Default::default()
        };
        Ok(SceneCommand::from_draw(
            "EquirectangularPanorama",
            self.context.clone(),
            Pass::Environment,
            metadata,
            draw_opts,
        ))
    }

    fn destroy(&mut self) {
        if let Some(p) = self.shader_program.take() {
            let _ = p.destroy();
        }
        if let Some(va) = self.vertex_array.take() {
            let _ = va.destroy();
        }
    }
}

trait EquirectangularBackend {
    fn build_command(
        &mut self,
        radius: f64,
        transform: Matrix4,
        bounding_sphere: BoundingSphere,
        material: &Material,
        use_log_depth: bool,
    ) -> Result<SceneCommand>;
    fn destroy(&mut self);
}

impl<G: Gl + 'static> EquirectangularBackend for EquirectangularGpu<G> {
    fn build_command(
        &mut self,
        radius: f64,
        transform: Matrix4,
        bounding_sphere: BoundingSphere,
        material: &Material,
        use_log_depth: bool,
    ) -> Result<SceneCommand> {
        EquirectangularGpu::build_command(self, radius, transform, bounding_sphere, material, use_log_depth)
    }
    fn destroy(&mut self) {
        EquirectangularGpu::destroy(self);
    }
}

/// Options for configuring an [`EquirectangularPanorama`].
#[derive(Clone, Debug, PartialEq)]
pub struct EquirectangularPanoramaOptions {
    pub image: String,
    pub transform: Option<Matrix4>,
    pub radius: Option<f64>,
    pub repeat_horizontal: Option<f64>,
    pub repeat_vertical: Option<f64>,
    pub credit: Option<Credit>,
    pub show: Option<bool>,
}

impl EquirectangularPanoramaOptions {
    /// Creates options with the required image URL or path.
    pub fn new(image: impl Into<String>) -> Self {
        Self {
            image: image.into(),
            transform: Some(Matrix4::IDENTITY),
            radius: Some(DEFAULT_RADIUS),
            repeat_horizontal: Some(1.0),
            repeat_vertical: Some(1.0),
            credit: None,
            show: Some(true),
        }
    }
}

/// A 360-degree panorama primitive displaying equirectangular imagery mapped
/// inside a sphere.
pub struct EquirectangularPanorama {
    pub image: String,
    pub transform: Matrix4,
    pub radius: f64,
    pub repeat_horizontal: f64,
    pub repeat_vertical: f64,
    pub credit: Option<Credit>,
    pub show: bool,
    pub material: Material,
    bounding_sphere: BoundingSphere,
    gpu: Option<Box<dyn EquirectangularBackend>>,
    destroyed: bool,
}

impl EquirectangularPanorama {
    /// Creates a new `EquirectangularPanorama` from options.
    pub fn new(options: EquirectangularPanoramaOptions) -> Self {
        let radius = options.radius.unwrap_or(DEFAULT_RADIUS);
        let transform = options.transform.unwrap_or(Matrix4::IDENTITY);
        let repeat_h = options.repeat_horizontal.unwrap_or(1.0);
        let repeat_v = options.repeat_vertical.unwrap_or(1.0);
        let show = options.show.unwrap_or(true);

        let mut material = Material::from_type(IMAGE_TYPE);
        let _ = material.set_uniform(
            "image",
            MaterialValue::Sampler2D(MaterialImage::Url(options.image.clone())),
        );
        let _ = material.set_uniform(
            "repeat",
            MaterialValue::Vec2(Cartesian2::new(-repeat_h, repeat_v)),
        );

        let base_sphere = BoundingSphere::new(Cartesian3::ZERO, radius);
        let bounding_sphere = BoundingSphere::transform(&base_sphere, &transform);

        Self {
            image: options.image,
            transform,
            radius,
            repeat_horizontal: repeat_h,
            repeat_vertical: repeat_v,
            credit: options.credit,
            show,
            material,
            bounding_sphere,
            gpu: None,
            destroyed: false,
        }
    }

    /// Creates a new `EquirectangularPanorama` attached to a renderer `Context<G>`.
    pub fn new_with_context<G: Gl + 'static>(
        options: EquirectangularPanoramaOptions,
        context: Context<G>,
    ) -> Self {
        let mut pano = Self::new(options);
        pano.gpu = Some(Box::new(EquirectangularGpu::new(context)));
        pano
    }

    /// Attaches a renderer `Context<G>` for GPU sphere mesh and shader execution.
    pub fn attach_context<G: Gl + 'static>(&mut self, context: Context<G>) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("EquirectangularPanorama is destroyed"));
        }
        self.gpu = Some(Box::new(EquirectangularGpu::new(context)));
        Ok(())
    }

    /// Updates and emits a draw command using the given renderer `Context<G>`.
    pub fn update_with_context<G: Gl + 'static>(
        &mut self,
        context: &Context<G>,
        frame: &mut FrameState,
    ) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("EquirectangularPanorama is destroyed"));
        }
        if self.gpu.is_none() {
            self.attach_context(context.clone())?;
        }
        ScenePrimitive::update(self, frame)
    }

    /// The enclosing bounding sphere in world coordinates.
    pub fn bounding_sphere(&self) -> &BoundingSphere {
        &self.bounding_sphere
    }

    /// Returns `true` if this primitive has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}

impl ScenePrimitive for EquirectangularPanorama {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("EquirectangularPanorama is destroyed"));
        }
        if !self.show || frame.mode != SceneMode::Scene3D {
            return Ok(());
        }

        let base_sphere = BoundingSphere::new(Cartesian3::ZERO, self.radius);
        self.bounding_sphere = BoundingSphere::transform(&base_sphere, &self.transform);

        if frame.passes.render {
            if let Some(gpu) = self.gpu.as_mut() {
                let cmd = gpu.build_command(
                    self.radius,
                    self.transform,
                    self.bounding_sphere,
                    &self.material,
                    frame.use_log_depth,
                )?;
                frame.command_list.push(cmd);
            }
        }

        Ok(())
    }

    fn post_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update_for_pass(&mut self, frame: &mut FrameState, _pass: super::Cesium3DTilePass) -> Result<()> {
        self.update(frame)
    }

    fn destroy(&mut self) -> Result<()> {
        if let Some(mut gpu) = self.gpu.take() {
            gpu.destroy();
        }
        self.destroyed = true;
        Ok(())
    }
}

impl super::panorama::Panorama for EquirectangularPanorama {
    fn show(&self) -> bool { self.show }
    fn set_show(&mut self, show: bool) { self.show = show; }
    fn transform(&self) -> &Matrix4 { &self.transform }
    fn credit(&self) -> Option<&Credit> { self.credit.as_ref() }
}
