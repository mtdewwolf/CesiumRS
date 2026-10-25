//! CesiumJS `ModelSilhouettePipelineStage`: assigns each model a stencil
//! reference and adds the shaders and uniforms used by the silhouette pass.
use super::PrimitiveRenderResources;
use crate::{
    renderer::{Gl, ShaderDestination, UniformValue},
    shaders,
};
use std::{cell::Cell, rc::Rc};

thread_local! {
    /// How many silhouettes have been created; the value is a stencil
    /// reference number, so it starts at one like Cesium's pre-increment.
    static SILHOUETTES_LENGTH: Cell<usize> = const { Cell::new(0) };
}

/// The current number of silhouettes created on this thread.
pub fn silhouettes_length() -> usize {
    SILHOUETTES_LENGTH.with(Cell::get)
}

/// Assign a silhouette ID and add the silhouette pass to a primitive.
pub fn process<G: Gl>(resources: &mut PrimitiveRenderResources<G>) {
    let silhouette_id = {
        let mut model = resources.model.borrow_mut();
        *model.silhouette_id.get_or_insert_with(|| {
            SILHOUETTES_LENGTH.with(|length| {
                let id = length.get() + 1;
                length.set(id);
                id
            })
        })
    };
    debug_assert!(silhouette_id > 0);

    let shader = &mut resources.shader_builder;
    shader.add_define("HAS_SILHOUETTE", None, ShaderDestination::BOTH);
    shader.add_vertex_lines(&[shaders::model::MODEL_SILHOUETTE_STAGE_VS.source]);
    shader.add_fragment_lines(&[shaders::model::MODEL_SILHOUETTE_STAGE_FS.source]);
    shader.add_uniform("vec4", "model_silhouetteColor", ShaderDestination::FRAGMENT);
    shader.add_uniform("float", "model_silhouetteSize", ShaderDestination::VERTEX);
    // Two draw commands share this program: the normal model draw and the
    // silhouette draw, which sets this uniform true to apply the stage.
    shader.add_uniform("bool", "model_silhouettePass", ShaderDestination::BOTH);

    let model = resources.model.clone();
    resources.uniform_map.insert(
        "model_silhouetteColor".into(),
        Rc::new(move || {
            let color = model.borrow().silhouette_color;
            UniformValue::Float(vec![color.red as f32, color.green as f32, color.blue as f32, color.alpha as f32])
        }),
    );
    let model = resources.model.clone();
    resources.uniform_map.insert(
        "model_silhouetteSize".into(),
        Rc::new(move || UniformValue::Float(vec![model.borrow().silhouette_size as f32])),
    );
    resources.uniform_map.insert("model_silhouettePass".into(), Rc::new(|| UniformValue::Int(vec![0])));
    resources.has_silhouette = true;
}
