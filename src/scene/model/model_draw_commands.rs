//! Build executable, owned draws from the model pipeline's render resources.
use super::{
    classification_model_draw_command::ClassificationModelDrawCommand, model_draw_command::*, model_utility,
    PrimitiveRenderResources, RuntimeDrawCommand,
};
use crate::{
    core::{BoundingSphere, Matrix4, PrimitiveType, RuntimeError},
    renderer::{
        AttributeSource, Context, DrawMetadata, Gl, ShaderProgram, ShaderProgramOptions, VertexArray, VertexAttribute,
    },
    scene::{CullFace, FrameState, SceneMode, ShadowMode},
    shaders,
};
use std::rc::Rc;

pub enum ModelDrawCommandKind<G: Gl> {
    Model(Box<ModelDrawCommand<G>>),
    Classification(Box<ClassificationModelDrawCommand<G>>),
}
impl<G: Gl> ModelDrawCommandKind<G> {
    pub fn command(&self) -> &ModelCommand<G> {
        match self {
            Self::Model(c) => &c.command,
            Self::Classification(c) => &c.command,
        }
    }
    pub fn command_mut(&mut self) -> &mut ModelCommand<G> {
        match self {
            Self::Model(c) => &mut c.command,
            Self::Classification(c) => &mut c.command,
        }
    }
    pub fn commands(&self, frame: &FrameState, selection: &ModelCommandSelection) -> ModelCommandLists<G> {
        match self {
            Self::Model(c) => c.commands(frame, selection),
            Self::Classification(c) => ModelCommandLists { commands: c.commands(frame), ..Default::default() },
        }
    }
    pub fn set_shadow_mode(&mut self, mode: ShadowMode) {
        if let Self::Model(c) = self {
            c.set_shadow_mode(mode);
        }
    }
    fn runtime(&self) -> &dyn RuntimeDrawCommand {
        match self {
            Self::Model(c) => c.as_ref(),
            Self::Classification(c) => c.as_ref(),
        }
    }
    fn runtime_mut(&mut self) -> &mut dyn RuntimeDrawCommand {
        match self {
            Self::Model(c) => c.as_mut(),
            Self::Classification(c) => c.as_mut(),
        }
    }
}
impl<G: Gl> RuntimeDrawCommand for ModelDrawCommandKind<G> {
    fn model_matrix(&self) -> &Matrix4 {
        self.runtime().model_matrix()
    }
    fn set_model_matrix(&mut self, m: Matrix4) {
        self.runtime_mut().set_model_matrix(m);
    }
    fn primitive_type(&self) -> Option<PrimitiveType> {
        self.runtime().primitive_type()
    }
    fn set_cull_face(&mut self, c: CullFace) {
        self.runtime_mut().set_cull_face(c);
    }
    fn set_back_face_culling(&mut self, b: bool) {
        self.runtime_mut().set_back_face_culling(b);
    }
    fn set_shadows(&mut self, b: bool) {
        self.runtime_mut().set_shadows(b);
    }
    fn set_debug_show_bounding_volume(&mut self, b: bool) {
        self.runtime_mut().set_debug_show_bounding_volume(b);
    }
}

#[derive(Clone, Debug)]
pub struct ModelDrawBuildOptions {
    pub model_matrix: Matrix4,
    pub node_transform: Matrix4,
    pub bounding_sphere_2d: Option<BoundingSphere>,
    pub project_to_2d: bool,
    pub owner: Option<String>,
    pub cull: bool,
    pub debug_show_bounding_volume: bool,
    pub allow_picking: bool,
    pub enable_debug_wireframe: bool,
    pub debug_wireframe: bool,
    pub derived: ModelDrawCommandOptions,
}
impl Default for ModelDrawBuildOptions {
    fn default() -> Self {
        Self {
            model_matrix: Matrix4::IDENTITY,
            node_transform: Matrix4::IDENTITY,
            bounding_sphere_2d: None,
            project_to_2d: false,
            owner: None,
            cull: true,
            debug_show_bounding_volume: false,
            allow_picking: true,
            enable_debug_wireframe: false,
            debug_wireframe: false,
            derived: Default::default(),
        }
    }
}
fn clone_attribute<G: Gl>(a: &VertexAttribute<G>) -> VertexAttribute<G> {
    VertexAttribute {
        index: a.index,
        enabled: a.enabled,
        source: match &a.source {
            AttributeSource::Buffer(b) => AttributeSource::Buffer(b.clone()),
            AttributeSource::Constant(v) => AttributeSource::Constant(v.clone()),
        },
        components: a.components,
        datatype: a.datatype,
        normalize: a.normalize,
        offset: a.offset,
        stride: a.stride,
        instance_divisor: a.instance_divisor,
    }
}
pub fn build_model_draw_command<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    context: &Context<G>,
    frame: &FrameState,
    options: ModelDrawBuildOptions,
) -> Result<ModelDrawCommandKind<G>, RuntimeError> {
    let mut builder = resources.shader_builder.clone();
    builder.add_vertex_lines(&[shaders::model::MODEL_VS.source]);
    builder.add_fragment_lines(&[shaders::model::MODEL_FS.source]);
    let (vertex_shader_source, fragment_shader_source) = builder.sources();
    let program = Rc::new(ShaderProgram::from_sources(
        context,
        &ShaderProgramOptions {
            vertex_shader_source,
            fragment_shader_source,
            attribute_locations: builder.attribute_locations().clone(),
        },
    )?);
    let index_buffer = match &resources.wireframe_index_buffer {
        Some(buffer) => Some(buffer.clone()),
        None => match &resources.indices {
            Some(indices) => {
                Some(indices.buffer.clone().ok_or_else(|| RuntimeError::new("Indices must be provided as a Buffer"))?)
            }
            None => None,
        },
    };
    let vertices = Rc::new(VertexArray::new(
        context,
        resources.attributes.iter().map(|a| clone_attribute(a)).collect(),
        index_buffer,
    )?);
    let matrix = Matrix4::multiply_transformation(&options.model_matrix, &options.node_transform);
    let projected = frame.mode != SceneMode::Scene3D && !frame.scene_3d_only && options.project_to_2d;
    let bounding_volume = if projected {
        options.bounding_sphere_2d.unwrap_or_else(|| resources.bounding_sphere.transform(&matrix))
    } else {
        resources.bounding_sphere.transform(&matrix)
    };
    let primitive_type = resources.primitive_type.unwrap_or(PrimitiveType::Triangles);
    let mut state = resources.render_state_options.clone();
    state.cull.face = model_utility::get_cull_face(&matrix, primitive_type).value();
    let classification_type = resources.model.borrow().classification_type;
    let metadata = DrawMetadata {
        bounding_volume: Some(bounding_volume),
        model_matrix: Some(matrix),
        owner: options.owner,
        cull: options.cull,
        pick_id: if classification_type.is_none() { resources.pick_id.clone() } else { None },
        pick_metadata_allowed: true,
        debug_show_bounding_volume: options.debug_show_bounding_volume,
        cast_shadows: classification_type.is_none() && options.derived.shadows.cast_shadows(),
        receive_shadows: classification_type.is_none() && options.derived.shadows.receive_shadows(),
        ..Default::default()
    };
    let gpu = Rc::new(ModelCommandResources {
        vertices,
        program: program.clone(),
        textures: vec![],
        texture_providers: resources.texture_uniform_map.clone(),
        pick_ids: resources.pick_ids.clone(),
    });
    let command = ModelCommand {
        resources: Some(gpu),
        state,
        metadata,
        pass: resources.alpha_options.borrow().pass.unwrap_or(crate::renderer::Pass::Opaque),
        primitive_type,
        count: resources.count,
        instance_count: resources.instance_count,
        uniform_map: Some(resources.renderer_uniform_map()),
        ..Default::default()
    };
    if let Some(kind) = classification_type {
        let mut result = ClassificationModelDrawCommand::new(
            command,
            resources.bounding_sphere,
            kind,
            resources.batch_lengths.clone().unwrap_or_default(),
            resources.batch_offsets.clone().unwrap_or_default(),
            options.allow_picking,
            resources.pick_id.clone(),
        )?;
        result.debug_wireframe = options.enable_debug_wireframe && options.debug_wireframe;
        return Ok(ModelDrawCommandKind::Classification(Box::new(result)));
    }
    let mut result = ModelDrawCommand::new(command, resources.bounding_sphere, options.derived);
    if let Some(edge) = resources.edge_geometry.take() {
        if let Some(vertices) = edge.vertex_array {
            let mut command = result.command.clone();
            command.resources = Some(Rc::new(ModelCommandResources {
                vertices: Rc::new(vertices),
                program,
                textures: vec![],
                texture_providers: resources.texture_uniform_map.clone(),
                pick_ids: resources.pick_ids.clone(),
            }));
            command.primitive_type = edge.primitive_type;
            command.count = Some(edge.index_count);
            command.pass = edge.pass;
            command.metadata.cast_shadows = false;
            command.metadata.receive_shadows = false;
            command.uniforms.push(("u_isEdgePass".into(), crate::renderer::UniformValue::Int(vec![1])));
            result.edge_command = Some(command);
        }
    }
    Ok(ModelDrawCommandKind::Model(Box::new(result)))
}
