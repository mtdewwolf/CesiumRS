//! CesiumJS `Scene/Model/InstancingPipelineStage`: handles GPU mesh instancing
//! at the node level, including transform/translation/scale/feature-ID instance
//! vertex attributes, legacy world-space i3dm instancing uniforms, and 2D/CV
//! projection.

use super::NodeRenderResources;
use crate::core::{
    AttributeCompression, Cartesian2, Cartesian3, ComponentDatatype, MapProjection, Matrix4, Quaternion, RuntimeError,
    Transforms, TypedArray,
};
use crate::renderer::{
    Buffer, BufferData, BufferKind, BufferUsage, Context, Gl, ShaderDestination, UniformValue, VertexAttribute,
};
use crate::scene::{
    model_components as mc, AttributeType, InstanceAttributeSemantic, SceneMode, SceneTransformState, SceneTransforms,
};
use crate::shaders;
use std::cell::RefCell;
use std::rc::Rc;

/// Persistent instancing state stored on the runtime node across pipeline runs.
#[derive(Clone)]
pub struct RuntimeNodeInstancingState<G: Gl> {
    pub computed_transform: Matrix4,
    pub components_transform: Matrix4,
    pub axis_correction_matrix: Matrix4,
    pub instancing_translation_min: Option<Cartesian3>,
    pub instancing_translation_max: Option<Cartesian3>,
    pub instancing_reference_point_2d: Option<Cartesian3>,
    pub instancing_transforms_buffer: Option<Buffer<G>>,
    pub instancing_transforms_buffer_2d: Option<Buffer<G>>,
    pub instancing_translation_buffer_2d: Option<Buffer<G>>,
    pub transforms_typed_array: Option<Vec<f32>>,
}

impl<G: Gl> Default for RuntimeNodeInstancingState<G> {
    fn default() -> Self {
        Self {
            computed_transform: Matrix4::IDENTITY,
            components_transform: Matrix4::IDENTITY,
            axis_correction_matrix: Matrix4::IDENTITY,
            instancing_translation_min: None,
            instancing_translation_max: None,
            instancing_reference_point_2d: None,
            instancing_transforms_buffer: None,
            instancing_transforms_buffer_2d: None,
            instancing_translation_buffer_2d: None,
            transforms_typed_array: None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn process<G: Gl>(
    resources: &mut NodeRenderResources<G>,
    node: &mut mc::Node<G>,
    runtime_node: &mut RuntimeNodeInstancingState<G>,
    mode: SceneMode,
    scene_3d_only: bool,
    morph_time: f64,
    map_projection: Rc<dyn MapProjection>,
    context: &Context<G>,
    view_matrix: Rc<RefCell<Matrix4>>,
    view_3d_matrix: Rc<RefCell<Matrix4>>,
    model_resources: &mut Vec<Buffer<G>>,
) -> Result<(), RuntimeError> {
    let instances = node.instances.as_mut().ok_or_else(|| RuntimeError::new("Node does not have instances"))?;
    let count = instances.attributes.first().and_then(|a| a.count).unwrap_or(0);

    resources.shader_builder.add_define("HAS_INSTANCING", None, ShaderDestination::BOTH);
    resources.shader_builder.add_vertex_lines(&[shaders::model::INSTANCING_STAGE_COMMON.source]);

    let (project_to_2d, enable_pick) = {
        let model = resources.model.borrow();
        (model.project_to_2d, model.enable_pick)
    };

    let use_2d = mode != SceneMode::Scene3D && !scene_3d_only && project_to_2d;
    let keep_typed_array = enable_pick && !context.webgl2();

    let mut instancing_vertex_attributes = Vec::new();

    process_transform_attributes(
        resources,
        instances,
        runtime_node,
        &mut instancing_vertex_attributes,
        use_2d,
        keep_typed_array,
        morph_time,
        map_projection.as_ref(),
        context,
        model_resources,
    )?;

    process_feature_id_attributes(resources, instances, &mut instancing_vertex_attributes)?;

    if instances.transform_in_world_space {
        resources.shader_builder.add_define("USE_LEGACY_INSTANCING", None, ShaderDestination::VERTEX);
        resources.shader_builder.add_uniform("mat4", "u_instance_modifiedModelView", ShaderDestination::VERTEX);
        resources.shader_builder.add_uniform("mat4", "u_instance_nodeTransform", ShaderDestination::VERTEX);

        let model_state = resources.model.clone();
        let components_transform = runtime_node.components_transform;
        let view_mat = view_matrix.clone();
        let view_3d_mat = view_3d_matrix;
        let map_proj = map_projection;
        resources.uniform_map.insert(
            "u_instance_modifiedModelView".into(),
            Rc::new(move || {
                let model_matrix = model_state.borrow().model_matrix;
                let mut modified_model_matrix = model_matrix.multiply_transformation(&components_transform);
                let result = if use_2d {
                    view_3d_mat.borrow().multiply_transformation(&modified_model_matrix)
                } else {
                    if mode != SceneMode::Scene3D {
                        modified_model_matrix = Transforms::basis_to_2d(map_proj.as_ref(), &modified_model_matrix);
                    }
                    view_mat.borrow().multiply_transformation(&modified_model_matrix)
                };
                UniformValue::Matrix { dimension: 4, values: result.0.iter().map(|&v| v as f32).collect() }
            }),
        );

        let axis_correction = runtime_node.axis_correction_matrix;
        let node_computed = runtime_node.computed_transform;
        resources.uniform_map.insert(
            "u_instance_nodeTransform".into(),
            Rc::new(move || {
                let result = axis_correction.multiply_transformation(&node_computed);
                UniformValue::Matrix { dimension: 4, values: result.0.iter().map(|&v| v as f32).collect() }
            }),
        );

        resources.shader_builder.add_vertex_lines(&[shaders::model::LEGACY_INSTANCING_STAGE_VS.source]);
    } else {
        resources.shader_builder.add_vertex_lines(&[shaders::model::INSTANCING_STAGE_VS.source]);
    }

    if use_2d {
        resources.shader_builder.add_define("USE_2D_INSTANCING", None, ShaderDestination::VERTEX);
        resources.shader_builder.add_uniform("mat4", "u_modelView2D", ShaderDestination::VERTEX);

        let ref_point_2d = runtime_node.instancing_reference_point_2d.unwrap_or(Cartesian3::ZERO);
        let model_matrix_2d = Matrix4::from_translation(ref_point_2d);
        resources.uniform_map.insert(
            "u_modelView2D".into(),
            Rc::new(move || {
                let result = view_matrix.borrow().multiply_transformation(&model_matrix_2d);
                UniformValue::Matrix { dimension: 4, values: result.0.iter().map(|&v| v as f32).collect() }
            }),
        );
    }

    resources.instance_count = count as u32;
    resources.has_instances = true;
    resources.attributes.extend(instancing_vertex_attributes);

    Ok(())
}

fn project_transform_to_2d(
    transform: &Matrix4,
    model_matrix: &Matrix4,
    node_transform: &Matrix4,
    map_projection: &dyn MapProjection,
) -> Matrix4 {
    let projected = model_matrix.multiply_transformation(transform).multiply_transformation(node_transform);
    Transforms::basis_to_2d(map_projection, &projected)
}

fn project_position_to_2d(
    position: Cartesian3,
    model_matrix: &Matrix4,
    node_transform: &Matrix4,
    transform_state: &SceneTransformState<'_>,
) -> Cartesian3 {
    let translation_matrix = Matrix4::from_translation(position);
    let projected_transform =
        model_matrix.multiply_transformation(&translation_matrix).multiply_transformation(node_transform);
    let final_position = projected_transform.get_translation();
    SceneTransforms::compute_actual_ellipsoid_position(transform_state, final_position).unwrap_or(Cartesian3::ZERO)
}

fn get_model_matrix_and_node_transform<G: Gl>(
    resources: &NodeRenderResources<G>,
    runtime_node: &RuntimeNodeInstancingState<G>,
    transform_in_world_space: bool,
) -> (Matrix4, Matrix4) {
    let model = resources.model.borrow();
    if transform_in_world_space {
        let model_matrix = model.model_matrix.multiply_transformation(&runtime_node.components_transform);
        let node_computed_transform =
            runtime_node.axis_correction_matrix.multiply_transformation(&runtime_node.computed_transform);
        (model_matrix, node_computed_transform)
    } else {
        let model_matrix =
            model.scene_graph_computed_model_matrix.multiply_transformation(&runtime_node.computed_transform);
        (model_matrix, Matrix4::IDENTITY)
    }
}

fn project_transforms_to_2d<G: Gl>(
    transforms: &[Matrix4],
    resources: &NodeRenderResources<G>,
    runtime_node: &RuntimeNodeInstancingState<G>,
    transform_in_world_space: bool,
    map_projection: &dyn MapProjection,
) -> Vec<Matrix4> {
    let (model_matrix, node_computed_transform) =
        get_model_matrix_and_node_transform(resources, runtime_node, transform_in_world_space);
    let reference_point = runtime_node.instancing_reference_point_2d.unwrap_or(Cartesian3::ZERO);

    transforms
        .iter()
        .map(|transform| {
            let projected_transform =
                project_transform_to_2d(transform, &model_matrix, &node_computed_transform, map_projection);
            let position = projected_transform.get_translation();
            let final_translation = position - reference_point;
            projected_transform.set_translation(final_translation)
        })
        .collect()
}

fn project_translations_to_2d<G: Gl>(
    translations: &[Cartesian3],
    resources: &NodeRenderResources<G>,
    runtime_node: &RuntimeNodeInstancingState<G>,
    transform_in_world_space: bool,
    transform_state_cv: &SceneTransformState<'_>,
) -> Vec<Cartesian3> {
    let (model_matrix, node_computed_transform) =
        get_model_matrix_and_node_transform(resources, runtime_node, transform_in_world_space);
    let reference_point = runtime_node.instancing_reference_point_2d.unwrap_or(Cartesian3::ZERO);

    translations
        .iter()
        .map(|&translation| {
            let projected_position =
                project_position_to_2d(translation, &model_matrix, &node_computed_transform, transform_state_cv);
            projected_position - reference_point
        })
        .collect()
}

fn compute_reference_point_2d<G: Gl>(
    resources: &NodeRenderResources<G>,
    runtime_node: &mut RuntimeNodeInstancingState<G>,
    transform_state_cv: &SceneTransformState<'_>,
) {
    let model_matrix = resources.model.borrow().scene_graph_computed_model_matrix;
    let min = runtime_node.instancing_translation_min.unwrap_or(Cartesian3::ZERO);
    let max = runtime_node.instancing_translation_max.unwrap_or(Cartesian3::ZERO);

    let transformed_min = model_matrix.multiply_by_point(min);
    let projected_min = SceneTransforms::compute_actual_ellipsoid_position(transform_state_cv, transformed_min)
        .unwrap_or(Cartesian3::ZERO);

    let transformed_max = model_matrix.multiply_by_point(max);
    let projected_max = SceneTransforms::compute_actual_ellipsoid_position(transform_state_cv, transformed_max)
        .unwrap_or(Cartesian3::ZERO);

    runtime_node.instancing_reference_point_2d = Some(projected_min.lerp(projected_max, 0.5));
}

pub fn transforms_to_typed_array(transforms: &[Matrix4]) -> Vec<f32> {
    let elements = 12;
    let count = transforms.len();
    let mut result = vec![0.0f32; count * elements];

    for (i, transform) in transforms.iter().enumerate() {
        let offset = elements * i;
        result[offset] = transform[0] as f32;
        result[offset + 1] = transform[4] as f32;
        result[offset + 2] = transform[8] as f32;
        result[offset + 3] = transform[12] as f32;
        result[offset + 4] = transform[1] as f32;
        result[offset + 5] = transform[5] as f32;
        result[offset + 6] = transform[9] as f32;
        result[offset + 7] = transform[13] as f32;
        result[offset + 8] = transform[2] as f32;
        result[offset + 9] = transform[6] as f32;
        result[offset + 10] = transform[10] as f32;
        result[offset + 11] = transform[14] as f32;
    }

    result
}

pub fn translations_to_typed_array(translations: &[Cartesian3]) -> Vec<f32> {
    let elements = 3;
    let count = translations.len();
    let mut result = vec![0.0f32; count * elements];

    for (i, translation) in translations.iter().enumerate() {
        let offset = elements * i;
        result[offset] = translation.x as f32;
        result[offset + 1] = translation.y as f32;
        result[offset + 2] = translation.z as f32;
    }

    result
}

fn dequantize_typed_array(typed_array: &TypedArray, components: usize, count: usize) -> Option<Vec<f32>> {
    match typed_array {
        TypedArray::Int8(v) => AttributeCompression::dequantize(v, components, count),
        TypedArray::Uint8(v) => AttributeCompression::dequantize(v, components, count),
        TypedArray::Int16(v) => AttributeCompression::dequantize(v, components, count),
        TypedArray::Uint16(v) => AttributeCompression::dequantize(v, components, count),
        TypedArray::Int32(v) => AttributeCompression::dequantize(v, components, count),
        TypedArray::Uint32(v) => AttributeCompression::dequantize(v, components, count),
        TypedArray::Float32(v) => Some(v.clone()),
        TypedArray::Float64(v) => Some(v.iter().map(|&x| x as f32).collect()),
    }
}

pub fn get_instance_transforms_as_matrices<G: Gl>(
    instances: &mut mc::Instances<G>,
    count: usize,
    runtime_node: &mut RuntimeNodeInstancingState<G>,
) -> Vec<Matrix4> {
    let mut instancing_translation_max = Cartesian3::new(-f64::MAX, -f64::MAX, -f64::MAX);
    let mut instancing_translation_min = Cartesian3::new(f64::MAX, f64::MAX, f64::MAX);

    let mut has_translation = false;
    let mut translation_values = vec![0.0f32; count * 3];

    let mut has_rotation = false;
    let mut rotation_values = vec![0.0f32; count * 4];

    let mut has_scale = false;
    let mut scale_values = vec![1.0f32; count * 3];

    for attr in &mut instances.attributes {
        match attr.semantic {
            Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Translation)) => {
                if let Some(ta) = attr.typed_array.take() {
                    has_translation = true;
                    translation_values = (0..ta.len()).map(|i| ta.get(i).unwrap_or(0.0) as f32).collect();
                }
            }
            Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Rotation)) => {
                if let Some(ta) = attr.typed_array.take() {
                    has_rotation = true;
                    if attr.normalized {
                        let comps = attr.attribute_type.unwrap_or(AttributeType::Vec4).get_number_of_components();
                        if let Some(deq) = dequantize_typed_array(&ta, comps, count) {
                            rotation_values = deq;
                        }
                    } else {
                        rotation_values = (0..ta.len()).map(|i| ta.get(i).unwrap_or(0.0) as f32).collect();
                    }
                }
            }
            Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Scale)) => {
                if let Some(ta) = attr.typed_array.take() {
                    has_scale = true;
                    scale_values = (0..ta.len()).map(|i| ta.get(i).unwrap_or(1.0) as f32).collect();
                }
            }
            _ => {}
        }
    }
    let _ = (has_translation, has_scale);

    let mut transforms = Vec::with_capacity(count);
    for i in 0..count {
        let translation = Cartesian3::new(
            *translation_values.get(i * 3).unwrap_or(&0.0) as f64,
            *translation_values.get(i * 3 + 1).unwrap_or(&0.0) as f64,
            *translation_values.get(i * 3 + 2).unwrap_or(&0.0) as f64,
        );

        instancing_translation_max = instancing_translation_max.maximum_by_component(translation);
        instancing_translation_min = instancing_translation_min.minimum_by_component(translation);

        let rotation = Quaternion::new(
            *rotation_values.get(i * 4).unwrap_or(&0.0) as f64,
            *rotation_values.get(i * 4 + 1).unwrap_or(&0.0) as f64,
            *rotation_values.get(i * 4 + 2).unwrap_or(&0.0) as f64,
            if has_rotation { *rotation_values.get(i * 4 + 3).unwrap_or(&1.0) as f64 } else { 1.0 },
        );

        let scale = Cartesian3::new(
            *scale_values.get(i * 3).unwrap_or(&1.0) as f64,
            *scale_values.get(i * 3 + 1).unwrap_or(&1.0) as f64,
            *scale_values.get(i * 3 + 2).unwrap_or(&1.0) as f64,
        );

        transforms.push(Matrix4::from_translation_quaternion_rotation_scale(translation, &rotation, scale));
    }

    runtime_node.instancing_translation_min = Some(instancing_translation_min);
    runtime_node.instancing_translation_max = Some(instancing_translation_max);

    transforms
}

fn get_instance_translations_as_cartesian3s<G: Gl>(
    translation_attribute: &mut mc::Attribute<G>,
    count: usize,
    runtime_node: &mut RuntimeNodeInstancingState<G>,
) -> Vec<Cartesian3> {
    let ta = translation_attribute.typed_array.take();
    let mut instancing_translation_min = Cartesian3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut instancing_translation_max = Cartesian3::new(-f64::MAX, -f64::MAX, -f64::MAX);
    let mut translations = Vec::with_capacity(count);

    for i in 0..count {
        let translation = if let Some(ref arr) = ta {
            Cartesian3::new(
                arr.get(i * 3).unwrap_or(0.0),
                arr.get(i * 3 + 1).unwrap_or(0.0),
                arr.get(i * 3 + 2).unwrap_or(0.0),
            )
        } else {
            Cartesian3::ZERO
        };
        instancing_translation_min = instancing_translation_min.minimum_by_component(translation);
        instancing_translation_max = instancing_translation_max.maximum_by_component(translation);
        translations.push(translation);
    }

    runtime_node.instancing_translation_min = Some(instancing_translation_min);
    runtime_node.instancing_translation_max = Some(instancing_translation_max);

    translations
}

fn create_vertex_buffer<G: Gl>(typed_array: &[f32], context: &Context<G>) -> Result<Buffer<G>, RuntimeError> {
    let bytes = TypedArray::Float32(typed_array.to_vec()).to_le_bytes();
    Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))
}

#[allow(clippy::too_many_arguments)]
fn process_transform_attributes<G: Gl>(
    resources: &mut NodeRenderResources<G>,
    instances: &mut mc::Instances<G>,
    runtime_node: &mut RuntimeNodeInstancingState<G>,
    instancing_vertex_attributes: &mut Vec<Rc<VertexAttribute<G>>>,
    use_2d: bool,
    keep_typed_array: bool,
    morph_time: f64,
    map_projection: &dyn MapProjection,
    context: &Context<G>,
    model_resources: &mut Vec<Buffer<G>>,
) -> Result<(), RuntimeError> {
    let has_rotation = instances
        .attributes
        .iter()
        .any(|a| a.semantic == Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Rotation)));

    if has_rotation {
        process_transform_matrix_attributes(
            resources,
            instances,
            runtime_node,
            instancing_vertex_attributes,
            use_2d,
            keep_typed_array,
            morph_time,
            map_projection,
            context,
            model_resources,
        )
    } else {
        process_transform_vec3_attributes(
            resources,
            instances,
            runtime_node,
            instancing_vertex_attributes,
            use_2d,
            keep_typed_array,
            morph_time,
            map_projection,
            context,
            model_resources,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn process_transform_matrix_attributes<G: Gl>(
    resources: &mut NodeRenderResources<G>,
    instances: &mut mc::Instances<G>,
    runtime_node: &mut RuntimeNodeInstancingState<G>,
    instancing_vertex_attributes: &mut Vec<Rc<VertexAttribute<G>>>,
    use_2d: bool,
    keep_typed_array: bool,
    morph_time: f64,
    map_projection: &dyn MapProjection,
    context: &Context<G>,
    model_resources: &mut Vec<Buffer<G>>,
) -> Result<(), RuntimeError> {
    let count = instances.attributes.first().and_then(|a| a.count).unwrap_or(0);

    resources.shader_builder.add_define("HAS_INSTANCE_MATRICES", None, ShaderDestination::BOTH);

    let mut transforms = None;
    let buffer = if let Some(existing) = runtime_node.instancing_transforms_buffer.clone() {
        existing
    } else {
        let computed = get_instance_transforms_as_matrices(instances, count, runtime_node);
        let transforms_typed_array = transforms_to_typed_array(&computed);
        let buf = create_vertex_buffer(&transforms_typed_array, context)?;
        model_resources.push(buf.clone());

        if keep_typed_array {
            runtime_node.transforms_typed_array = Some(transforms_typed_array);
        }
        runtime_node.instancing_transforms_buffer = Some(buf.clone());
        transforms = Some(computed);
        buf
    };

    process_matrix_attributes(resources, buffer, instancing_vertex_attributes, "Transform");

    if !use_2d {
        return Ok(());
    }

    let transform_state_cv = SceneTransformState {
        mode: SceneMode::ColumbusView,
        morph_time,
        map_projection,
        client_dimensions: Cartesian2::ONE,
        drawing_buffer_dimensions: Cartesian2::ONE,
    };

    compute_reference_point_2d(resources, runtime_node, &transform_state_cv);

    let buffer_2d = if let Some(existing_2d) = runtime_node.instancing_transforms_buffer_2d.clone() {
        existing_2d
    } else {
        let raw_transforms = transforms.unwrap_or_default();
        let projected_transforms = project_transforms_to_2d(
            &raw_transforms,
            resources,
            runtime_node,
            instances.transform_in_world_space,
            map_projection,
        );
        let projected_typed_array = transforms_to_typed_array(&projected_transforms);
        let buf_2d = create_vertex_buffer(&projected_typed_array, context)?;
        model_resources.push(buf_2d.clone());
        runtime_node.instancing_transforms_buffer_2d = Some(buf_2d.clone());
        buf_2d
    };

    process_matrix_attributes(resources, buffer_2d, instancing_vertex_attributes, "Transform2D");

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_transform_vec3_attributes<G: Gl>(
    resources: &mut NodeRenderResources<G>,
    instances: &mut mc::Instances<G>,
    runtime_node: &mut RuntimeNodeInstancingState<G>,
    instancing_vertex_attributes: &mut Vec<Rc<VertexAttribute<G>>>,
    use_2d: bool,
    keep_typed_array: bool,
    morph_time: f64,
    map_projection: &dyn MapProjection,
    context: &Context<G>,
    model_resources: &mut Vec<Buffer<G>>,
) -> Result<(), RuntimeError> {
    if let Some(scale_attribute) = instances
        .attributes
        .iter()
        .find(|a| a.semantic == Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Scale)))
    {
        resources.shader_builder.add_define("HAS_INSTANCE_SCALE", None, ShaderDestination::BOTH);
        if let Some(scale_buf) = scale_attribute.buffer.clone() {
            process_vec3_attribute(
                resources,
                scale_buf,
                scale_attribute.byte_offset.unwrap_or(0),
                scale_attribute.byte_stride.unwrap_or(0),
                instancing_vertex_attributes,
                "Scale",
            );
        }
    }

    let Some(translation_idx) = instances
        .attributes
        .iter()
        .position(|a| a.semantic == Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Translation)))
    else {
        return Ok(());
    };

    let count = instances.attributes[translation_idx].count.unwrap_or(0);
    let mut instancing_translations = None;
    if instances.attributes[translation_idx].typed_array.is_some() {
        instancing_translations = Some(get_instance_translations_as_cartesian3s(
            &mut instances.attributes[translation_idx],
            count,
            runtime_node,
        ));
    } else if runtime_node.instancing_translation_min.is_none() {
        if let Some(mc::MathValue::Cartesian3(min)) = instances.attributes[translation_idx].min {
            runtime_node.instancing_translation_min = Some(min);
        }
        if let Some(mc::MathValue::Cartesian3(max)) = instances.attributes[translation_idx].max {
            runtime_node.instancing_translation_max = Some(max);
        }
    }

    resources.shader_builder.add_define("HAS_INSTANCE_TRANSLATION", None, ShaderDestination::BOTH);

    if let Some(trans_buf) = instances.attributes[translation_idx].buffer.clone() {
        process_vec3_attribute(
            resources,
            trans_buf,
            instances.attributes[translation_idx].byte_offset.unwrap_or(0),
            instances.attributes[translation_idx].byte_stride.unwrap_or(0),
            instancing_vertex_attributes,
            "Translation",
        );
    }

    if !use_2d && !keep_typed_array {
        return Ok(());
    }

    let transform_state_cv = SceneTransformState {
        mode: SceneMode::ColumbusView,
        morph_time,
        map_projection,
        client_dimensions: Cartesian2::ONE,
        drawing_buffer_dimensions: Cartesian2::ONE,
    };

    compute_reference_point_2d(resources, runtime_node, &transform_state_cv);

    let buffer_2d = if let Some(existing_2d) = runtime_node.instancing_translation_buffer_2d.clone() {
        existing_2d
    } else {
        let raw_translations = instancing_translations.unwrap_or_default();
        let projected_translations = project_translations_to_2d(
            &raw_translations,
            resources,
            runtime_node,
            instances.transform_in_world_space,
            &transform_state_cv,
        );
        let projected_typed_array = translations_to_typed_array(&projected_translations);

        if keep_typed_array {
            runtime_node.transforms_typed_array = Some(projected_typed_array.clone());
        }

        let buf_2d = create_vertex_buffer(&projected_typed_array, context)?;
        model_resources.push(buf_2d.clone());
        runtime_node.instancing_translation_buffer_2d = Some(buf_2d.clone());
        buf_2d
    };

    if !use_2d {
        return Ok(());
    }

    process_vec3_attribute(resources, buffer_2d, 0, 0, instancing_vertex_attributes, "Translation2D");

    Ok(())
}

fn process_matrix_attributes<G: Gl>(
    resources: &mut NodeRenderResources<G>,
    buffer: Buffer<G>,
    instancing_vertex_attributes: &mut Vec<Rc<VertexAttribute<G>>>,
    attribute_string: &str,
) {
    let vertex_size_in_floats = 12;
    let component_byte_size = ComponentDatatype::Float.get_size_in_bytes();
    let stride_in_bytes = component_byte_size * vertex_size_in_floats;

    for row in 0..3 {
        let index = resources.attribute_index;
        resources.attribute_index += 1;
        let mut attr = VertexAttribute::buffer(index, buffer.clone(), 4);
        attr.datatype = ComponentDatatype::Float;
        attr.normalize = false;
        attr.offset = component_byte_size * 4 * row;
        attr.stride = stride_in_bytes;
        attr.instance_divisor = 1;
        instancing_vertex_attributes.push(Rc::new(attr));

        resources.shader_builder.add_attribute("vec4", &format!("a_instancing{attribute_string}Row{row}"));
    }
}

fn process_vec3_attribute<G: Gl>(
    resources: &mut NodeRenderResources<G>,
    buffer: Buffer<G>,
    byte_offset: usize,
    byte_stride: usize,
    instancing_vertex_attributes: &mut Vec<Rc<VertexAttribute<G>>>,
    attribute_string: &str,
) {
    let index = resources.attribute_index;
    resources.attribute_index += 1;
    let mut attr = VertexAttribute::buffer(index, buffer, 3);
    attr.datatype = ComponentDatatype::Float;
    attr.normalize = false;
    attr.offset = byte_offset;
    attr.stride = byte_stride;
    attr.instance_divisor = 1;
    instancing_vertex_attributes.push(Rc::new(attr));

    resources.shader_builder.add_attribute("vec3", &format!("a_instance{attribute_string}"));
}

fn process_feature_id_attributes<G: Gl>(
    resources: &mut NodeRenderResources<G>,
    instances: &mc::Instances<G>,
    instancing_vertex_attributes: &mut Vec<Rc<VertexAttribute<G>>>,
) -> Result<(), RuntimeError> {
    for attribute in &instances.attributes {
        if attribute.semantic != Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::FeatureId)) {
            continue;
        }

        let set_index = attribute.set_index.unwrap_or(0);
        if set_index >= resources.feature_id_vertex_attribute_set_index {
            resources.feature_id_vertex_attribute_set_index = set_index + 1;
        }

        let index = resources.attribute_index;
        resources.attribute_index += 1;

        let buffer =
            attribute.buffer.clone().ok_or_else(|| RuntimeError::new("Missing instance featureId attribute buffer"))?;
        let components_per_attribute = attribute
            .attribute_type
            .ok_or_else(|| RuntimeError::new("Missing instance featureId attribute type"))?
            .get_number_of_components() as u32;
        let component_datatype = attribute
            .component_datatype
            .ok_or_else(|| RuntimeError::new("Missing instance featureId componentDatatype"))?;

        let mut vertex_attr = VertexAttribute::buffer(index, buffer, components_per_attribute);
        vertex_attr.datatype = component_datatype;
        vertex_attr.normalize = false;
        vertex_attr.offset = attribute.byte_offset.unwrap_or(0);
        vertex_attr.stride = attribute.byte_stride.unwrap_or(0);
        vertex_attr.instance_divisor = 1;
        instancing_vertex_attributes.push(Rc::new(vertex_attr));

        resources.shader_builder.add_attribute("float", &format!("a_instanceFeatureId_{set_index}"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::GeographicProjection;
    use crate::renderer::GlValue;
    use crate::scene::model::{ModelRenderResources, ModelUniformState};

    #[derive(Default)]
    struct TestGl;
    impl Gl for TestGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
            Ok([8, 8])
        }
        fn invoke(&self, method: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
            if method.starts_with("create") {
                Ok(GlValue::Object(1))
            } else {
                Ok(GlValue::Null)
            }
        }
    }

    #[test]
    fn processes_matrix_and_feature_id_instancing() {
        let ctx = Context::new_webgl2(TestGl);
        let state = Rc::new(RefCell::new(ModelUniformState::default()));
        let model_res = ModelRenderResources::<TestGl>::new(state);
        let mut node_res = NodeRenderResources::new(&model_res, 0);

        let fid_buf =
            Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&[0u8; 8])).unwrap();

        let mut node = mc::Node {
            instances: Some(mc::Instances {
                attributes: vec![
                    mc::Attribute {
                        semantic: Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Translation)),
                        attribute_type: Some(AttributeType::Vec3),
                        component_datatype: Some(ComponentDatatype::Float),
                        count: Some(2),
                        typed_array: Some(TypedArray::Float32(vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])),
                        ..Default::default()
                    },
                    mc::Attribute {
                        semantic: Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::Rotation)),
                        attribute_type: Some(AttributeType::Vec4),
                        component_datatype: Some(ComponentDatatype::Float),
                        count: Some(2),
                        typed_array: Some(TypedArray::Float32(vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0])),
                        ..Default::default()
                    },
                    mc::Attribute {
                        semantic: Some(mc::AttributeSemantic::Instance(InstanceAttributeSemantic::FeatureId)),
                        set_index: Some(0),
                        attribute_type: Some(AttributeType::Scalar),
                        component_datatype: Some(ComponentDatatype::Float),
                        count: Some(2),
                        buffer: Some(fid_buf),
                        ..Default::default()
                    },
                ],
                transform_in_world_space: true,
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut runtime_node = RuntimeNodeInstancingState::default();
        let projection: Rc<dyn MapProjection> = Rc::new(GeographicProjection::default());
        let view = Rc::new(RefCell::new(Matrix4::IDENTITY));
        let view_3d = Rc::new(RefCell::new(Matrix4::IDENTITY));
        let mut model_buffers = Vec::new();

        process(
            &mut node_res,
            &mut node,
            &mut runtime_node,
            SceneMode::Scene3D,
            false,
            0.0,
            projection,
            &ctx,
            view,
            view_3d,
            &mut model_buffers,
        )
        .unwrap();

        assert_eq!(node_res.instance_count, 2);
        assert!(node_res.has_instances);
        assert_eq!(node_res.attributes.len(), 4);
        assert_eq!(runtime_node.instancing_translation_min, Some(Cartesian3::new(1.0, 2.0, 3.0)));
        assert_eq!(runtime_node.instancing_translation_max, Some(Cartesian3::new(4.0, 5.0, 6.0)));
        let (vs, _) = node_res.shader_builder.sources();
        assert!(vs.defines.iter().any(|d| d == "HAS_INSTANCING"));
        assert!(vs.defines.iter().any(|d| d == "HAS_INSTANCE_MATRICES"));
        assert!(vs.defines.iter().any(|d| d == "USE_LEGACY_INSTANCING"));
        assert!(vs.sources[0].contains("in float a_instanceFeatureId_0;"));
    }
}
