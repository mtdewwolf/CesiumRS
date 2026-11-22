//! CesiumJS SkyBox: mode/pass filtering, panorama delegation, and built-in
//! GPU cube-map panorama renderer (`CubeMapSkyBoxPanorama`).
use super::{
    blending_state,
    panorama::Panorama,
    scene::{SceneCommand, SceneDrawOptions, ScenePrimitive},
    Cesium3DTilePass, FrameState, SceneMode,
};
use crate::{
    core::{BoxGeometry, Cartesian3, Credit, Matrix4, PrimitiveType, RuntimeError, VertexFormat},
    renderer::{
        render_state::DepthTest, Context, CubeMap, DrawMetadata, GeometryUploadOptions, Gl, Pass, RenderState,
        SampledTexture, ShaderProgram, ShaderProgramOptions, ShaderSource, VertexArray,
    },
    shaders::root::{SKY_BOX_FS, SKY_BOX_VS},
};
use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

pub trait SkyBoxPanorama: Panorama {
    type Command;
    type Sources;
    fn sources(&self) -> &Self::Sources;
    fn set_sources(&mut self, sources: Self::Sources);
    fn update(&mut self, frame: &FrameState, hdr: bool) -> Result<Option<Self::Command>, RuntimeError>;
    fn destroy(&mut self) -> Result<(), RuntimeError>;
}

/// Built-in GPU cube-map panorama renderer for [`SkyBox`], backed by a unit
/// cube `VertexArray`, `SKY_BOX_VS`/`SKY_BOX_FS`, and a `CubeMap<G>`.
pub struct CubeMapSkyBoxPanorama<G: Gl> {
    context: Context<G>,
    sources: [String; 6],
    cube_map: Option<Rc<CubeMap<G>>>,
    show: bool,
    transform: Matrix4,
    credit: Option<Credit>,
    vertex_array: Option<Rc<VertexArray<G>>>,
    shader_program: Option<Rc<ShaderProgram<G>>>,
    shader_hdr: Option<bool>,
    destroyed: bool,
}

impl<G: Gl + 'static> CubeMapSkyBoxPanorama<G> {
    pub fn new(context: Context<G>, sources: [String; 6], cube_map: Option<Rc<CubeMap<G>>>) -> Self {
        Self {
            context,
            sources,
            cube_map,
            show: true,
            transform: Matrix4::IDENTITY,
            credit: None,
            vertex_array: None,
            shader_program: None,
            shader_hdr: None,
            destroyed: false,
        }
    }

    pub fn set_cube_map(&mut self, cube_map: Option<Rc<CubeMap<G>>>) {
        self.cube_map = cube_map;
    }

    pub fn cube_map(&self) -> Option<&Rc<CubeMap<G>>> {
        self.cube_map.as_ref()
    }
}

impl<G: Gl + 'static> Panorama for CubeMapSkyBoxPanorama<G> {
    fn show(&self) -> bool {
        self.show
    }
    fn set_show(&mut self, show: bool) {
        self.show = show;
    }
    fn transform(&self) -> &Matrix4 {
        &self.transform
    }
    fn credit(&self) -> Option<&Credit> {
        self.credit.as_ref()
    }
}

impl<G: Gl + 'static> SkyBoxPanorama for CubeMapSkyBoxPanorama<G> {
    type Command = SceneCommand;
    type Sources = [String; 6];

    fn sources(&self) -> &Self::Sources {
        &self.sources
    }

    fn set_sources(&mut self, sources: Self::Sources) {
        self.sources = sources;
    }

    fn update(&mut self, _frame: &FrameState, hdr: bool) -> Result<Option<Self::Command>, RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("CubeMapSkyBoxPanorama was destroyed"));
        }
        if self.vertex_array.is_none() {
            let box_geo = BoxGeometry::from_dimensions(Cartesian3::new(2.0, 2.0, 2.0), VertexFormat::POSITION_ONLY, None)
                .ok_or_else(|| RuntimeError::new("Failed to build SkyBox unit cube"))?;
            let geometry = box_geo
                .create_geometry()
                .ok_or_else(|| RuntimeError::new("Failed to create SkyBox cube geometry"))?;
            let va = VertexArray::from_geometry(
                &self.context,
                &geometry,
                GeometryUploadOptions {
                    attribute_locations: HashMap::from([("position".to_string(), 0)]),
                    ..Default::default()
                },
            )?;
            self.vertex_array = Some(Rc::new(va));
        }
        if self.shader_program.is_none() || self.shader_hdr != Some(hdr) {
            if let Some(old) = self.shader_program.take() {
                let _ = old.destroy();
            }
            let vs = ShaderSource::new(SKY_BOX_VS.source);
            let mut fs = ShaderSource::new(SKY_BOX_FS.source);
            if hdr {
                fs.defines.push("HDR".into());
            }
            let program = ShaderProgram::from_sources(
                &self.context,
                &ShaderProgramOptions {
                    vertex_shader_source: vs,
                    fragment_shader_source: fs,
                    attribute_locations: BTreeMap::from([("position".to_string(), 0)]),
                },
            )?;
            self.shader_program = Some(Rc::new(program));
            self.shader_hdr = Some(hdr);
        }

        let cube_map: Rc<dyn SampledTexture<G>> = match &self.cube_map {
            Some(cm) => cm.clone(),
            None => self.context.default_cube_map()?,
        };
        let va = self.vertex_array.as_ref().expect("built above").clone();
        let program = self.shader_program.as_ref().expect("compiled above").clone();
        let mut draw_opts = SceneDrawOptions::new(va, program);
        draw_opts.primitive_type = PrimitiveType::Triangles;
        draw_opts.state = RenderState {
            depth_test: DepthTest { enabled: false, ..Default::default() },
            depth_mask: false,
            blending: blending_state::alpha_blend(),
            ..Default::default()
        };
        draw_opts.textures.push(("u_cubeMap".into(), cube_map));

        let metadata = DrawMetadata {
            cull: false,
            owner: Some("SkyBox".into()),
            ..Default::default()
        };
        Ok(Some(SceneCommand::from_draw(
            "SkyBox",
            self.context.clone(),
            Pass::Environment,
            metadata,
            draw_opts,
        )))
    }

    fn destroy(&mut self) -> Result<(), RuntimeError> {
        if !self.destroyed {
            if let Some(p) = self.shader_program.take() {
                let _ = p.destroy();
            }
            if let Some(va) = self.vertex_array.take() {
                let _ = va.destroy();
            }
            self.cube_map = None;
            self.destroyed = true;
        }
        Ok(())
    }
}

pub struct SkyBox<P: SkyBoxPanorama> {
    pub show: bool,
    panorama: P,
    destroyed: bool,
}
impl<P: SkyBoxPanorama> SkyBox<P> {
    pub fn new(panorama: P) -> Self {
        Self { show: true, panorama, destroyed: false }
    }
    pub fn panorama(&self) -> &P {
        &self.panorama
    }
    pub fn panorama_mut(&mut self) -> &mut P {
        &mut self.panorama
    }
    pub fn sources(&self) -> &P::Sources {
        self.panorama.sources()
    }
    pub fn set_sources(&mut self, sources: P::Sources) {
        self.panorama.set_sources(sources);
    }
    pub fn update(&mut self, frame: &FrameState, hdr: bool) -> Result<Option<P::Command>, RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("SkyBox was destroyed"));
        }
        // Pinned source exposes show but does not consult it in update.
        if !matches!(frame.mode, SceneMode::Scene3D | SceneMode::Morphing) || !frame.passes.render {
            return Ok(None);
        }
        self.panorama.update(frame, hdr)
    }
    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        if !self.destroyed {
            self.panorama.destroy()?;
            self.destroyed = true;
        }
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn earth_sources(base: &str) -> [String; 6] {
        ["px", "mx", "py", "my", "pz", "mz"].map(|s| format!("{base}Assets/Textures/SkyBox/tycho2t3_80_{s}.jpg"))
    }
}

impl<G: Gl + 'static> SkyBox<CubeMapSkyBoxPanorama<G>> {
    /// Creates a GPU-backed `SkyBox` using the built-in `CubeMapSkyBoxPanorama`.
    pub fn with_context(context: Context<G>, sources: [String; 6], cube_map: Option<Rc<CubeMap<G>>>) -> Self {
        Self::new(CubeMapSkyBoxPanorama::new(context, sources, cube_map))
    }
}

impl<G: Gl + 'static> ScenePrimitive for SkyBox<CubeMapSkyBoxPanorama<G>> {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if !self.show {
            return Ok(());
        }
        if let Some(cmd) = SkyBox::update(self, frame, frame.high_dynamic_range)? {
            frame.command_list.push(cmd);
        }
        Ok(())
    }
    fn post_passes_update(&mut self, _frame: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update_for_pass(&mut self, _frame: &mut FrameState, _pass: Cesium3DTilePass) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        SkyBox::destroy(self)
    }
}
