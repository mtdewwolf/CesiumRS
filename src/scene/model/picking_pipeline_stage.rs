//! CesiumJS `Scene/Model/PickingPipelineStage`: configures picking uniforms,
//! per-instance pick ID vertex buffers, or batch table pick textures.

use super::{model_utility, ModelStatistics, PrimitiveRenderResources};
use crate::core::{ComponentDatatype, RuntimeError, TypedArray};
use crate::renderer::{
    Buffer, BufferData, BufferKind, BufferUsage, Context, Gl, ShaderDestination, Texture, UniformValue, VertexAttribute,
};
use crate::scene::model_components as mc;
use std::cell::RefCell;
use std::rc::Rc;

/// Pick texture and pick ID GLSL expression from a `Cesium3DTileFeatureTable` / `BatchTexture`.
pub struct PickBatchTextureSource<G: Gl> {
    pub pick_texture: Option<Rc<Texture<G>>>,
    pub default_texture: Option<Rc<Texture<G>>>,
    pub pick_id_expression: String,
}

impl<G: Gl> Default for PickBatchTextureSource<G> {
    fn default() -> Self {
        Self {
            pick_texture: None,
            default_texture: None,
            pick_id_expression: "texture(model_pickTexture, feature.st)".into(),
        }
    }
}

/// Object stored in a `Context` `PickId` created by `PickingPipelineStage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelPickObject {
    /// Whether `primitive` on `detailPickObject` points to the tileset (`model.content.tileset`) vs `model`.
    pub has_content: bool,
    pub runtime_node: usize,
    pub runtime_primitive: usize,
    pub instance_id: Option<usize>,
}

pub fn process<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    node: &mc::Node<G>,
    primitive: &mc::Primitive<G>,
    context: &Context<G>,
    statistics: &mut ModelStatistics,
    has_content: bool,
    feature_tables: &[Option<Rc<RefCell<PickBatchTextureSource<G>>>>],
) -> Result<(), RuntimeError> {
    if resources.has_property_table {
        let model = resources.model.borrow();
        let mut selected_feature_ids = None;
        if let Some(instances) = &node.instances {
            let fid = model_utility::get_feature_ids_by_label(&instances.feature_ids, &model.instance_feature_id_label);
            if fid.is_some_and(|f| f.property_table_id().is_some()) {
                selected_feature_ids = fid;
            }
        }
        if selected_feature_ids.is_none() {
            selected_feature_ids =
                model_utility::get_feature_ids_by_label(&primitive.feature_ids, &model.feature_id_label);
        }
        drop(model);

        let property_table_id = selected_feature_ids
            .and_then(|f| f.property_table_id())
            .ok_or_else(|| RuntimeError::new("Missing selected feature ID propertyTableId"))?;
        let feature_table = feature_tables
            .get(property_table_id)
            .and_then(|t| t.as_ref())
            .ok_or_else(|| RuntimeError::new("Missing feature table for propertyTableId"))?
            .clone();

        resources.shader_builder.add_uniform("sampler2D", "model_pickTexture", ShaderDestination::FRAGMENT);

        let ft_for_tex = feature_table.clone();
        resources.texture_uniform_map.insert(
            "model_pickTexture".into(),
            Rc::new(move || {
                let ft = ft_for_tex.borrow();
                ft.pick_texture.clone().or_else(|| ft.default_texture.clone())
            }),
        );

        let pick_id_expr = feature_table.borrow().pick_id_expression.clone();
        resources.pick_id = Some(pick_id_expr);
        return Ok(());
    }

    if node.instances.is_some() {
        process_instanced_pick_ids(resources, context, statistics, has_content)?;
    } else {
        let pick_object = Rc::new(ModelPickObject {
            has_content,
            runtime_node: resources.runtime_node,
            runtime_primitive: resources.runtime_primitive,
            instance_id: None,
        });
        let pick_id = Rc::new(context.create_pick_id(pick_object)?);
        resources.pick_ids.push(pick_id.clone());

        resources.shader_builder.add_uniform("vec4", "czm_pickColor", ShaderDestination::FRAGMENT);
        resources.uniform_map.insert(
            "czm_pickColor".into(),
            Rc::new(move || {
                let c = pick_id.color;
                UniformValue::Float(vec![c.red as f32, c.green as f32, c.blue as f32, c.alpha as f32])
            }),
        );
        resources.pick_id = Some("czm_pickColor".into());
    }

    Ok(())
}

fn process_instanced_pick_ids<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    context: &Context<G>,
    statistics: &mut ModelStatistics,
    has_content: bool,
) -> Result<(), RuntimeError> {
    let instance_count = resources.instance_count as usize;
    let mut pick_ids = vec![0.0f32; instance_count * 4];
    for i in 0..instance_count {
        let pick_object = Rc::new(ModelPickObject {
            has_content,
            runtime_node: resources.runtime_node,
            runtime_primitive: resources.runtime_primitive,
            instance_id: Some(i),
        });
        let pick_id = Rc::new(context.create_pick_id(pick_object)?);
        let c = pick_id.color;
        pick_ids[i * 4] = c.red as f32;
        pick_ids[i * 4 + 1] = c.green as f32;
        pick_ids[i * 4 + 2] = c.blue as f32;
        pick_ids[i * 4 + 3] = c.alpha as f32;
        resources.pick_ids.push(pick_id);
    }

    let bytes = TypedArray::Float32(pick_ids).to_le_bytes();
    let pick_ids_buffer = Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?;
    // Destruction of resources allocated by the Model is handled by Model.destroy().
    let has_cpu_copy = false;
    statistics.add_buffer(&pick_ids_buffer, has_cpu_copy);

    let pick_ids_vertex_attribute = VertexAttribute {
        index: resources.attribute_index,
        enabled: true,
        source: crate::renderer::AttributeSource::Buffer(pick_ids_buffer),
        components: 4,
        datatype: ComponentDatatype::Float,
        normalize: false,
        offset: 0,
        stride: ComponentDatatype::Float.get_size_in_bytes() * 4,
        instance_divisor: 1,
    };
    resources.attribute_index += 1;
    resources.attributes.push(Rc::new(pick_ids_vertex_attribute));

    let shader = &mut resources.shader_builder;
    shader.add_define("USE_PICKING", None, ShaderDestination::BOTH);
    shader.add_attribute("vec4", "a_pickColor");
    shader.add_varying("vec4", "v_pickColor", None);
    resources.pick_id = Some("v_pickColor".into());

    Ok(())
}
