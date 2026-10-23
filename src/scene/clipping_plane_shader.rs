//! Executable clipping for Scene draws, using Cesium's model stage GLSL.
use super::ClippingPlaneCollection;
use crate::{core::*, renderer::*};
use std::{cell::RefCell, rc::Rc};
#[derive(Clone)]
pub struct SceneClippingPlanes {
    pub collection: Rc<RefCell<ClippingPlaneCollection>>,
    pub world_transform: Matrix4,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClippingShaderKey {
    pub count: usize,
    pub union: bool,
    pub float: bool,
    pub width: u32,
    pub height: u32,
}
pub type ClippingUniforms<G> = (Rc<Texture<G>>, Vec<(String, UniformValue)>);
impl PartialEq for SceneClippingPlanes {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.collection, &other.collection) && self.world_transform == other.world_transform
    }
}
impl std::fmt::Debug for SceneClippingPlanes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SceneClippingPlanes").field("world_transform", &self.world_transform).finish_non_exhaustive()
    }
}
impl SceneClippingPlanes {
    pub fn new(collection: Rc<RefCell<ClippingPlaneCollection>>, world_transform: Matrix4) -> Self {
        Self { collection, world_transform }
    }
    pub fn prepare<G: Gl + 'static>(&self, context: &Context<G>) -> Result<Option<ClippingShaderKey>, RuntimeError> {
        let mut collection = self.collection.borrow_mut();
        if !collection.enabled {
            return Ok(None);
        }
        collection.update(context)?;
        if collection.is_empty() {
            return Ok(None);
        }
        let texture = collection.texture::<G>().ok_or_else(|| RuntimeError::new("Missing clipping plane texture"))?;
        Ok(Some(ClippingShaderKey {
            count: collection.len(),
            union: collection.union_clipping_regions,
            float: context.capabilities()?.floating_point_texture,
            width: texture.width(),
            height: texture.height(),
        }))
    }
    pub fn shader(options: &ShaderProgramOptions, key: ClippingShaderKey) -> ShaderProgramOptions {
        let mut result = options.clone();
        let source = &mut result.fragment_shader_source;
        if source.defines.iter().any(|d| d == "HAS_CLIPPING_PLANES") {
            return result;
        }
        source.defines.extend([
            "HAS_CLIPPING_PLANES".into(),
            format!("CLIPPING_PLANES_LENGTH {}", key.count),
            format!("CLIPPING_PLANES_TEXTURE_WIDTH {}", key.width),
            format!("CLIPPING_PLANES_TEXTURE_HEIGHT {}", key.height),
        ]);
        if key.union {
            source.defines.push("UNION_CLIPPING_REGIONS".into());
        }
        if key.float {
            source.defines.push("USE_CLIPPING_PLANES_FLOAT_TEXTURE".into());
        }
        let color =
            if source.sources.iter().any(|s| s.contains("out_FragData")) { "out_FragData_0" } else { "out_FragColor" };
        for s in &mut source.sources {
            *s = ShaderSource::replace_main(s, "czm_scene_unclipped_main");
        }
        source.sources.push(format!("uniform sampler2D model_clippingPlanes;\nuniform vec4 model_clippingPlanesEdgeStyle;\nuniform mat4 model_clippingPlanesMatrix;\n{}\nvoid main(){{czm_scene_unclipped_main();modelClippingPlanesStage({color});}}",crate::shaders::model::MODEL_CLIPPING_PLANES_STAGE_FS.source));
        result
    }
    pub fn uniforms<G: Gl + 'static>(&self, context: &Context<G>) -> Result<ClippingUniforms<G>, RuntimeError> {
        let collection = self.collection.borrow();
        let texture = collection.texture::<G>().ok_or_else(|| RuntimeError::new("Missing clipping plane texture"))?;
        let matrix = context
            .uniform_state()
            .view_3d
            .multiply(&self.world_transform)
            .multiply(&collection.model_matrix)
            .inverse_transpose()?;
        let color = collection.edge_color;
        Ok((
            texture,
            vec![
                (
                    "model_clippingPlanesMatrix".into(),
                    UniformValue::Matrix { dimension: 4, values: matrix.0.iter().map(|v| *v as f32).collect() },
                ),
                (
                    "model_clippingPlanesEdgeStyle".into(),
                    UniformValue::Float(vec![
                        color.red as f32,
                        color.green as f32,
                        color.blue as f32,
                        collection.edge_width as f32,
                    ]),
                ),
            ],
        ))
    }
}
