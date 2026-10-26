//! Owned model draw commands and Cesium's derived-command selection.
use super::{RuntimeDrawCommand, StyleCommandsNeeded};
use crate::{
    core::{webgl_constants as gl, BoundingSphere, Matrix4, PrimitiveType, RuntimeError},
    renderer::{
        render_state::*, Context, DrawMetadata, Gl, Pass, SampledTexture, ShaderProgram, UniformMap, UniformValue,
        VertexArray,
    },
    scene::{
        blending_state, stencil_constants as stencil, CullFace, FrameState, SceneCommand, SceneDrawOptions, SceneMode,
        ShadowMode,
    },
};
use std::rc::Rc;

/// Resources are shared by derived commands and survive while queued in Scene.
pub struct ModelCommandResources<G: Gl> {
    pub vertices: Rc<VertexArray<G>>,
    pub program: Rc<ShaderProgram<G>>,
    pub textures: Vec<(String, Rc<dyn SampledTexture<G>>)>,
    pub texture_providers: super::ModelTextureUniformProviders<G>,
    pub pick_ids: Vec<Rc<crate::renderer::PickId>>,
}

/// A draw description can be inspected without a GPU. Executable descriptions
/// additionally own `resources`; submission requires them.
pub struct ModelCommand<G: Gl> {
    pub resources: Option<Rc<ModelCommandResources<G>>>,
    pub state: RenderState,
    pub metadata: DrawMetadata,
    pub pass: Pass,
    pub primitive_type: PrimitiveType,
    pub count: Option<usize>,
    pub offset: usize,
    pub instance_count: u32,
    pub uniform_map: Option<UniformMap>,
    pub uniforms: Vec<(String, UniformValue)>,
}
impl<G: Gl> Clone for ModelCommand<G> {
    fn clone(&self) -> Self {
        Self {
            resources: self.resources.clone(),
            state: self.state.clone(),
            metadata: self.metadata.clone(),
            pass: self.pass,
            primitive_type: self.primitive_type,
            count: self.count,
            offset: self.offset,
            instance_count: self.instance_count,
            uniform_map: self.uniform_map.clone(),
            uniforms: self.uniforms.clone(),
        }
    }
}
impl<G: Gl> Default for ModelCommand<G> {
    fn default() -> Self {
        Self {
            resources: None,
            state: RenderState::default(),
            metadata: DrawMetadata::default(),
            pass: Pass::Opaque,
            primitive_type: PrimitiveType::Triangles,
            count: None,
            offset: 0,
            instance_count: 0,
            uniform_map: None,
            uniforms: Vec::new(),
        }
    }
}
impl<G: Gl + 'static> ModelCommand<G> {
    pub fn into_scene_command(self, context: Context<G>, id: impl Into<String>) -> Result<SceneCommand, RuntimeError> {
        let resources = self.resources.ok_or_else(|| RuntimeError::new("Model command has no GPU resources"))?;
        let mut options = SceneDrawOptions::new(resources.vertices.clone(), resources.program.clone());
        options.state = self.state;
        options.primitive_type = self.primitive_type;
        options.offset = self.offset;
        options.count = self.count;
        options.instance_count = self.instance_count;
        let base_uniforms = self.uniform_map;
        let overrides = self.uniforms;
        options.uniform_map = Some(Rc::new(move || {
            let mut values = base_uniforms.as_ref().map_or_else(|| Ok(Vec::new()), |map| map())?;
            values.retain(|(name, _)| !overrides.iter().any(|(key, _)| key == name));
            values.extend(overrides.clone());
            Ok(values)
        }));
        options.textures = resources.textures.clone();
        for (name, provider) in &resources.texture_providers {
            if let Some(texture) = provider() {
                options.textures.push((name.clone(), texture));
            }
        }
        Ok(SceneCommand::from_draw(id, context, self.pass, self.metadata, options))
    }
}

#[derive(Clone, Debug)]
pub struct ModelDrawCommandOptions {
    pub double_sided: bool,
    pub has_silhouette: bool,
    pub has_skip_level_of_detail: bool,
    pub silhouette_id: usize,
    pub silhouette_alpha: f64,
    pub invisible: bool,
    pub shadows: ShadowMode,
}
impl Default for ModelDrawCommandOptions {
    fn default() -> Self {
        Self {
            double_sided: false,
            has_silhouette: false,
            has_skip_level_of_detail: false,
            silhouette_id: 0,
            silhouette_alpha: 1.,
            invisible: false,
            shadows: ShadowMode::Enabled,
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct ModelCommandSelection {
    pub style: Option<StyleCommandsNeeded>,
    pub project_to_2d: bool,
    pub bounding_sphere_2d: Option<BoundingSphere>,
    pub mixed_content: bool,
    pub final_resolution: bool,
    pub selection_depth: u32,
}
pub struct ModelCommandLists<G: Gl> {
    pub commands: Vec<ModelCommand<G>>,
    pub backfaces: Vec<ModelCommand<G>>,
    pub silhouettes: Vec<ModelCommand<G>>,
    pub edges: Vec<ModelCommand<G>>,
}
impl<G: Gl> Default for ModelCommandLists<G> {
    fn default() -> Self {
        Self { commands: vec![], backfaces: vec![], silhouettes: vec![], edges: vec![] }
    }
}

pub struct ModelDrawCommand<G: Gl> {
    pub command: ModelCommand<G>,
    pub edge_command: Option<ModelCommand<G>>,
    pub local_bounding_sphere: BoundingSphere,
    pub options: ModelDrawCommandOptions,
    model_matrix: Matrix4,
}
impl<G: Gl> ModelDrawCommand<G> {
    pub fn new(
        command: ModelCommand<G>,
        local_bounding_sphere: BoundingSphere,
        options: ModelDrawCommandOptions,
    ) -> Self {
        let model_matrix = command.metadata.model_matrix.unwrap_or(Matrix4::IDENTITY);
        Self { command, edge_command: None, local_bounding_sphere, options, model_matrix }
    }
    pub fn bounding_volume(&self) -> BoundingSphere {
        self.local_bounding_sphere.transform(&self.model_matrix)
    }
    pub fn set_shadow_mode(&mut self, mode: ShadowMode) {
        self.options.shadows = mode;
        self.command.metadata.cast_shadows = mode.cast_shadows();
        self.command.metadata.receive_shadows = mode.receive_shadows();
    }
    fn duplicate_2d(
        &self,
        command: ModelCommand<G>,
        frame: &FrameState,
        selection: &ModelCommandSelection,
        out: &mut Vec<ModelCommand<G>>,
    ) {
        out.push(command.clone());
        if frame.mode != SceneMode::Scene2D || selection.project_to_2d {
            return;
        }
        let Some(sphere) = selection.bounding_sphere_2d else {
            return;
        };
        let idl = std::f64::consts::PI * frame.uniform.map_projection.ellipsoid().maximum_radius;
        let left = sphere.center.y - sphere.radius;
        let right = sphere.center.y + sphere.radius;
        if !((left < idl && right > idl) || (left < -idl && right > -idl)) {
            return;
        }
        let mut other = command;
        let mut matrix = self.model_matrix;
        matrix.0[13] -= matrix.0[13].signum() * 2. * idl;
        other.metadata.model_matrix = Some(matrix);
        other.metadata.bounding_volume = Some(self.local_bounding_sphere.transform(&matrix));
        out.push(other);
    }
    pub fn commands(&self, frame: &FrameState, selection: &ModelCommandSelection) -> ModelCommandLists<G> {
        let mut result = ModelCommandLists::default();
        let opaque = self.command.pass != Pass::Translucent;
        let silhouette = self.options.has_silhouette;
        if opaque && !silhouette && selection.style.is_some_and(|s| s != StyleCommandsNeeded::AllOpaque) {
            let mut translucent = self.command.clone();
            translucent.pass = Pass::Translucent;
            translucent.state.cull.enabled = false;
            translucent.state.depth_mask = false;
            translucent.state.blending = blending_state::alpha_blend();
            self.duplicate_2d(translucent, frame, selection, &mut result.commands);
            if selection.style == Some(StyleCommandsNeeded::AllTranslucent) {
                return result;
            }
        }
        let mut command = self.command.clone();
        if opaque && self.options.has_skip_level_of_detail && selection.mixed_content {
            if !selection.final_resolution {
                let mut backface = command.clone();
                backface.state.cull = Cull { enabled: true, face: gl::FRONT };
                backface.state.color_mask = no_color();
                backface.state.polygon_offset = PolygonOffset { enabled: true, factor: 5., units: 5. };
                backface.metadata.cast_shadows = false;
                backface.metadata.receive_shadows = false;
                backface.uniforms.push(("u_polygonOffset".into(), UniformValue::Float(vec![5., 5.])));
                self.duplicate_2d(backface, frame, selection, &mut result.backfaces);
            }
            let operation = StencilOperation { z_pass: gl::REPLACE, ..Default::default() };
            command.state.stencil_test = StencilTest {
                enabled: true,
                front_function: gl::GEQUAL,
                back_function: gl::GEQUAL,
                reference: stencil::CESIUM_3D_TILE_MASK
                    | (selection.selection_depth as i32).wrapping_shl(stencil::SKIP_LOD_BIT_SHIFT as u32),
                mask: stencil::SKIP_LOD_MASK as i64,
                front_operation: operation.clone(),
                back_operation: operation,
            };
            command.state.stencil_mask = (stencil::CESIUM_3D_TILE_MASK | stencil::SKIP_LOD_MASK) as i64;
        } else if silhouette {
            let reference = (self.options.silhouette_id % 255) as i32;
            let operation = StencilOperation { z_pass: gl::REPLACE, ..Default::default() };
            command.state.stencil_test = StencilTest {
                enabled: true,
                front_function: gl::ALWAYS,
                back_function: gl::ALWAYS,
                reference,
                mask: -1,
                front_operation: operation.clone(),
                back_operation: operation,
            };
            if self.options.invisible {
                command.state.color_mask = no_color();
            }
            if !frame.passes.pick {
                let mut color = self.command.clone();
                color.state.cull.enabled = false;
                color.state.stencil_test = StencilTest {
                    enabled: true,
                    front_function: gl::NOTEQUAL,
                    back_function: gl::NOTEQUAL,
                    reference,
                    mask: -1,
                    ..Default::default()
                };
                if !opaque || self.options.silhouette_alpha < 1. {
                    color.pass = Pass::Translucent;
                    color.state.depth_mask = false;
                    color.state.blending = blending_state::alpha_blend();
                }
                color.metadata.cast_shadows = false;
                color.metadata.receive_shadows = false;
                color.metadata.debug_show_bounding_volume = false;
                color.uniforms.push(("model_silhouettePass".into(), UniformValue::Int(vec![1])));
                self.duplicate_2d(color, frame, selection, &mut result.silhouettes);
            }
        }
        self.duplicate_2d(command, frame, selection, &mut result.commands);
        if let Some(edge) = &self.edge_command {
            self.duplicate_2d(edge.clone(), frame, selection, &mut result.edges);
        }
        result
    }
}
impl<G: Gl> RuntimeDrawCommand for ModelDrawCommand<G> {
    fn model_matrix(&self) -> &Matrix4 {
        &self.model_matrix
    }
    fn set_model_matrix(&mut self, matrix: Matrix4) {
        self.model_matrix = matrix;
        self.command.metadata.model_matrix = Some(matrix);
        self.command.metadata.bounding_volume = Some(self.bounding_volume());
        if let Some(edge) = &mut self.edge_command {
            edge.metadata.model_matrix = Some(matrix);
            edge.metadata.bounding_volume = self.command.metadata.bounding_volume;
        }
    }
    fn primitive_type(&self) -> Option<PrimitiveType> {
        Some(self.command.primitive_type)
    }
    fn set_cull_face(&mut self, face: CullFace) {
        if !self.options.double_sided && self.command.pass != Pass::Translucent {
            self.command.state.cull.face = face.value();
        }
    }
    fn set_back_face_culling(&mut self, enabled: bool) {
        if !self.options.double_sided && self.command.pass != Pass::Translucent {
            self.command.state.cull.enabled = enabled;
        }
    }
    fn set_shadows(&mut self, enabled: bool) {
        self.set_shadow_mode(if enabled { ShadowMode::Enabled } else { ShadowMode::Disabled });
    }
    fn set_debug_show_bounding_volume(&mut self, show: bool) {
        self.command.metadata.debug_show_bounding_volume = show;
    }
}
pub(crate) fn no_color() -> ColorMask {
    ColorMask { red: false, green: false, blue: false, alpha: false }
}
