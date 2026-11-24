//! CesiumJS `CubeMapPanorama`: displays imagery in cube-map format in a scene.
//! Only supported in 3D; the panorama is faded out when morphing to 2D or
//! Columbus view.
//!
//! The six sources are either URL strings or decoded images. URL loading is
//! asynchronous and owned by the host, which installs the resulting cube map
//! with [`CubeMapPanorama::set_cube_map`] and reports failures with
//! [`CubeMapPanorama::report_error`] so they are raised on the next update,
//! matching Cesium's deferred error behavior.

use super::{
    blending_state,
    scene::{SceneCommand, SceneDrawOptions},
    FrameState, SceneMode,
};
use crate::{
    core::{BoxGeometry, Cartesian3, Credit, ImageHost, Matrix3, Resource, ResourceClient, RuntimeError, VertexFormat},
    renderer::{
        load_cube_map_resources, render_state::DepthTest, Context, CubeMap, DrawMetadata, GeometryUploadOptions, Gl,
        Pass, RenderState, ShaderProgram, ShaderProgramOptions, ShaderSource, TextureSource, UniformValue, VertexArray,
    },
    shaders::root::{CUBE_MAP_PANORAMA_VS, SKY_BOX_FS, SKY_BOX_VS},
};
use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

/// The six cube-map face sources, in positiveX, negativeX, positiveY,
/// negativeY, positiveZ, negativeZ order.
pub enum CubeMapSources<G: Gl> {
    /// URL strings for the six faces.
    Urls([String; 6]),
    /// Decoded images for the six faces.
    Images([TextureSource<G>; 6]),
}

/// Options for constructing a [`CubeMapPanorama`].
pub struct CubeMapPanoramaOptions<G: Gl> {
    pub sources: CubeMapSources<G>,
    pub transform: Option<Matrix3>,
    pub show: Option<bool>,
    pub credit: Option<Credit>,
    pub return_command: Option<bool>,
}

impl<G: Gl> CubeMapPanoramaOptions<G> {
    pub fn new(sources: CubeMapSources<G>) -> Self {
        Self { sources, transform: None, show: Some(true), credit: None, return_command: Some(false) }
    }
}

/// A panorama that displays imagery in cube-map format.
pub struct CubeMapPanorama<G: Gl> {
    pub show: bool,
    pub sources: CubeMapSources<G>,
    transform: Option<Matrix3>,
    credit: Option<Credit>,
    return_command: bool,
    context: Context<G>,
    cube_map: Option<Rc<CubeMap<G>>>,
    vertex_array: Option<Rc<VertexArray<G>>>,
    shader_program: Option<Rc<ShaderProgram<G>>>,
    use_hdr: Option<bool>,
    sources_dirty: bool,
    add_to_panorama_command_list: bool,
    has_error: bool,
    error: Option<RuntimeError>,
    destroyed: bool,
}

impl<G: Gl + 'static> CubeMapPanorama<G> {
    pub fn new(context: Context<G>, options: CubeMapPanoramaOptions<G>) -> Self {
        Self {
            show: options.show.unwrap_or(true),
            sources: options.sources,
            transform: options.transform,
            credit: options.credit,
            return_command: options.return_command.unwrap_or(false),
            context,
            cube_map: None,
            vertex_array: None,
            shader_program: None,
            use_hdr: None,
            sources_dirty: true,
            add_to_panorama_command_list: false,
            has_error: false,
            error: None,
            destroyed: false,
        }
    }

    /// The transform of the panorama (a 3x3 rotation applied in the shader).
    pub fn transform(&self) -> Option<&Matrix3> {
        self.transform.as_ref()
    }

    /// The credit of the panorama.
    pub fn credit(&self) -> Option<&Credit> {
        self.credit.as_ref()
    }

    /// Replaces the sources and marks the cube map for regeneration.
    pub fn set_sources(&mut self, sources: CubeMapSources<G>) {
        self.sources = sources;
        self.sources_dirty = true;
    }

    /// Sets the loaded cube map (used after asynchronous URL loading).
    pub fn set_cube_map(&mut self, cube_map: Option<Rc<CubeMap<G>>>) {
        if let Some(old) = self.cube_map.take() {
            let _ = old.destroy();
        }
        self.cube_map = cube_map;
        self.sources_dirty = false;
        self.add_to_panorama_command_list = true;
    }

    /// The loaded cube map, if any.
    pub fn cube_map(&self) -> Option<&Rc<CubeMap<G>>> {
        self.cube_map.as_ref()
    }

    /// Defers an asynchronous load error to the next update, as Cesium does.
    pub fn report_error(&mut self, error: RuntimeError) {
        self.has_error = true;
        self.error = Some(error);
    }

    /// Builds the cube map from decoded images. URL sources stay pending until
    /// the host installs a loaded cube map.
    fn build_cube_map(&mut self) -> Result<(), RuntimeError> {
        let sources = std::mem::replace(&mut self.sources, CubeMapSources::Urls(Default::default()));
        match sources {
            CubeMapSources::Images(images) => {
                let [width, height] = images[0].dimensions();
                let options = crate::renderer::TextureOptions::rgba(width, height);
                let map = Rc::new(CubeMap::new(&self.context, options, Some(images))?);
                self.set_cube_map(Some(map));
            }
            CubeMapSources::Urls(urls) => self.sources = CubeMapSources::Urls(urls),
        }
        self.sources_dirty = false;
        self.add_to_panorama_command_list = true;
        Ok(())
    }

    /// Loads URL sources through the host resource client and installs the
    /// resulting cube map. This is the asynchronous half of Cesium's
    /// `loadCubeMap` call in `update`.
    pub async fn load_cube_map<H: ImageHost<Image = G::Object> + 'static>(
        &mut self,
        client: &ResourceClient,
        host: Rc<H>,
    ) -> Result<(), RuntimeError> {
        let CubeMapSources::Urls(urls) = &self.sources else {
            return Ok(());
        };
        let resources: Vec<Resource> = urls.clone().into_iter().filter_map(Resource::new).collect();
        let resources: [Resource; 6] =
            resources.try_into().map_err(|_| RuntimeError::new("Cube map panorama requires six valid source URLs"))?;
        let cube_map = load_cube_map_resources(&self.context, resources, false, client, host).await?;
        self.set_cube_map(Some(Rc::new(cube_map)));
        Ok(())
    }

    fn build_resources(&mut self, use_hdr: bool) -> Result<(), RuntimeError> {
        if self.vertex_array.is_none() {
            let geometry =
                BoxGeometry::from_dimensions(Cartesian3::new(2.0, 2.0, 2.0), VertexFormat::POSITION_ONLY, None)
                    .and_then(|geometry| geometry.create_geometry())
                    .ok_or_else(|| RuntimeError::new("Failed to build the CubeMapPanorama unit cube"))?;
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
        if self.shader_program.is_none() || self.use_hdr != Some(use_hdr) {
            if let Some(old) = self.shader_program.take() {
                let _ = old.destroy();
            }
            let mut fragment = ShaderSource::new(SKY_BOX_FS.source);
            if use_hdr {
                fragment.defines.push("HDR".into());
            }
            let vertex = if self.transform.is_some() {
                ShaderSource::new(CUBE_MAP_PANORAMA_VS.source)
            } else {
                ShaderSource::new(SKY_BOX_VS.source)
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
            self.use_hdr = Some(use_hdr);
            self.add_to_panorama_command_list = true;
        }
        Ok(())
    }

    /// `CubeMapPanorama.prototype.update`. Returns the draw command when
    /// `returnCommand` is set; otherwise it is pushed to the frame's panorama
    /// command list and `None` is returned.
    pub fn update(&mut self, frame: &mut FrameState, use_hdr: bool) -> Result<Option<SceneCommand>, RuntimeError> {
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

        // Rethrow any error that occurred asynchronously.
        if self.has_error {
            self.has_error = false;
            return Err(self.error.take().unwrap_or_else(|| RuntimeError::new("CubeMapPanorama load failed")));
        }

        if self.sources_dirty {
            self.build_cube_map()?;
        }
        self.build_resources(use_hdr)?;

        let Some(cube_map) = self.cube_map.clone() else {
            return Ok(None);
        };
        if let Some(credit) = &self.credit {
            if !self.return_command {
                frame.credits.push(credit.clone());
            }
        }

        let vertices = self.vertex_array.as_ref().expect("built above").clone();
        let program = self.shader_program.as_ref().expect("compiled above").clone();
        let mut options = SceneDrawOptions::new(vertices, program);
        options.primitive_type = crate::core::PrimitiveType::Triangles;
        options.state = RenderState {
            depth_test: DepthTest { enabled: false, ..Default::default() },
            depth_mask: false,
            blending: blending_state::alpha_blend(),
            ..Default::default()
        };
        options.textures = vec![("u_cubeMap".into(), cube_map as Rc<dyn crate::renderer::SampledTexture<G>>)];
        if let Some(transform) = &self.transform {
            options.uniforms = vec![(
                "u_cubeMapPanoramaTransform".into(),
                UniformValue::Matrix { dimension: 3, values: transform.0.iter().map(|v| *v as f32).collect() },
            )];
        }

        let metadata = DrawMetadata { cull: false, owner: Some("CubeMapPanorama".into()), ..Default::default() };
        let command =
            SceneCommand::from_draw("CubeMapPanorama", self.context.clone(), Pass::Environment, metadata, options);

        if self.return_command {
            return Ok(Some(command));
        }
        if self.add_to_panorama_command_list {
            frame.panorama_command_list.push(command);
            self.add_to_panorama_command_list = false;
        }
        Ok(None)
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) {
        if let Some(vertices) = self.vertex_array.take() {
            let _ = vertices.destroy();
        }
        if let Some(program) = self.shader_program.take() {
            let _ = program.destroy();
        }
        if let Some(cube_map) = self.cube_map.take() {
            let _ = cube_map.destroy();
        }
        self.destroyed = true;
    }
}
