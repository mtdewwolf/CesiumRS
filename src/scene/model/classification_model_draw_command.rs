//! Stencil-volume classification, batched separately for terrain and tiles.
use super::{
    model_draw_command::{no_color, ModelCommand},
    RuntimeDrawCommand,
};
use crate::{
    core::{webgl_constants as gl, BoundingSphere, Matrix4, PrimitiveType, RuntimeError},
    renderer::{render_state::*, Gl, Pass},
    scene::{blending_state, stencil_constants as stencil, ClassificationType, CullFace, FrameState},
};

pub struct ClassificationModelDrawCommand<G: Gl> {
    pub command: ModelCommand<G>,
    pub classification_type: ClassificationType,
    pub batch_lengths: Vec<usize>,
    pub batch_offsets: Vec<usize>,
    pub allow_picking: bool,
    pub debug_wireframe: bool,
    pub pick_id: Option<String>,
    local_bounding_sphere: BoundingSphere,
    model_matrix: Matrix4,
}
impl<G: Gl> ClassificationModelDrawCommand<G> {
    pub fn new(
        command: ModelCommand<G>,
        sphere: BoundingSphere,
        classification_type: ClassificationType,
        batch_lengths: Vec<usize>,
        batch_offsets: Vec<usize>,
        allow_picking: bool,
        pick_id: Option<String>,
    ) -> Result<Self, RuntimeError> {
        if batch_lengths.len() != batch_offsets.len() {
            return Err(RuntimeError::new("Classification batch lengths and offsets must have equal lengths"));
        }
        if batch_lengths
            .iter()
            .zip(&batch_offsets)
            .any(|(&n, &o)| o.checked_add(n).is_none_or(|end| command.count.is_some_and(|count| end > count)))
        {
            return Err(RuntimeError::new("Classification batch exceeds primitive count"));
        }
        let model_matrix = command.metadata.model_matrix.unwrap_or(Matrix4::IDENTITY);
        Ok(Self {
            command,
            classification_type,
            batch_lengths,
            batch_offsets,
            allow_picking,
            debug_wireframe: false,
            pick_id,
            local_bounding_sphere: sphere,
            model_matrix,
        })
    }
    fn batches(&self, commands: &[ModelCommand<G>], multiplier: usize, result: &mut Vec<ModelCommand<G>>) {
        for (&count, &offset) in self.batch_lengths.iter().zip(&self.batch_offsets) {
            for command in commands {
                let mut batch = command.clone();
                batch.count = Some(count * multiplier);
                batch.offset = offset * multiplier;
                result.push(batch);
            }
        }
    }
    pub fn commands(&self, frame: &FrameState) -> Vec<ModelCommand<G>> {
        let mut result = Vec::new();
        if frame.passes.render && self.debug_wireframe {
            let mut command = self.command.clone();
            command.pass = Pass::Opaque;
            self.batches(&[command], 2, &mut result);
            return result;
        }
        let terrain = self.classification_type != ClassificationType::Cesium3DTile;
        let tiles = self.classification_type != ClassificationType::Terrain;
        if frame.passes.render {
            for (enabled, pass) in [(terrain, Pass::TerrainClassification), (tiles, Pass::Cesium3DTileClassification)] {
                if enabled {
                    self.batches(&[self.depth(pass), self.color(pass, false)], 1, &mut result);
                }
            }
            if frame.invert_classification && tiles {
                self.batches(&[self.depth(Pass::Cesium3DTileClassificationIgnoreShow)], 1, &mut result);
            }
        }
        if frame.passes.pick && self.allow_picking {
            for (enabled, pass) in [(terrain, Pass::TerrainClassification), (tiles, Pass::Cesium3DTileClassification)] {
                if enabled {
                    let mut depth = self.depth(pass);
                    depth.metadata.cull = true;
                    depth.metadata.pick_only = true;
                    self.batches(&[depth, self.color(pass, true)], 1, &mut result);
                }
            }
        }
        result
    }
    fn depth(&self, pass: Pass) -> ModelCommand<G> {
        let mut command = self.command.clone();
        command.pass = pass;
        command.metadata.cull = false;
        command.metadata.pick_id = None;
        let func = if pass == Pass::TerrainClassification { gl::ALWAYS } else { gl::EQUAL };
        command.state = RenderState {
            color_mask: no_color(),
            depth_mask: false,
            depth_test: DepthTest { enabled: true, func: gl::LEQUAL },
            stencil_mask: stencil::CLASSIFICATION_MASK as i64,
            stencil_test: StencilTest {
                enabled: true,
                front_function: func,
                back_function: func,
                reference: stencil::CESIUM_3D_TILE_MASK,
                mask: stencil::CESIUM_3D_TILE_MASK as i64,
                front_operation: StencilOperation { z_fail: gl::DECR_WRAP, ..Default::default() },
                back_operation: StencilOperation { z_fail: gl::INCR_WRAP, ..Default::default() },
            },
            ..Default::default()
        };
        command
    }
    fn color(&self, pass: Pass, picking: bool) -> ModelCommand<G> {
        let mut command = self.command.clone();
        command.pass = pass;
        command.metadata.cull = picking;
        command.metadata.pick_only = picking;
        command.metadata.pick_id = if picking { self.pick_id.clone() } else { None };
        let zero = StencilOperation { fail: gl::ZERO, z_fail: gl::ZERO, z_pass: gl::ZERO };
        command.state = RenderState {
            depth_mask: false,
            stencil_mask: stencil::CLASSIFICATION_MASK as i64,
            stencil_test: StencilTest {
                enabled: true,
                front_function: gl::NOTEQUAL,
                back_function: gl::NOTEQUAL,
                reference: 0,
                mask: stencil::CLASSIFICATION_MASK as i64,
                front_operation: zero.clone(),
                back_operation: zero,
            },
            blending: if picking { Blending::default() } else { blending_state::pre_multiplied_alpha_blend() },
            ..Default::default()
        };
        command
    }
}
impl<G: Gl> RuntimeDrawCommand for ClassificationModelDrawCommand<G> {
    fn model_matrix(&self) -> &Matrix4 {
        &self.model_matrix
    }
    fn set_model_matrix(&mut self, matrix: Matrix4) {
        self.model_matrix = matrix;
        self.command.metadata.model_matrix = Some(matrix);
        self.command.metadata.bounding_volume = Some(self.local_bounding_sphere.transform(&matrix));
    }
    fn primitive_type(&self) -> Option<PrimitiveType> {
        Some(self.command.primitive_type)
    }
    fn set_cull_face(&mut self, face: CullFace) {
        self.command.state.cull.face = face.value();
    }
    fn set_back_face_culling(&mut self, _: bool) {}
    fn set_shadows(&mut self, _: bool) {}
    fn set_debug_show_bounding_volume(&mut self, _: bool) {}
}
