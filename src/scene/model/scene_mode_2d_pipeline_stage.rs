//! CesiumJS `Scene/Model/SceneMode2DPipelineStage`: projects primitive position
//! attributes and bounding spheres to 2D / Columbus View mode.

use super::PrimitiveRenderResources;
use crate::core::{BoundingSphere, Cartesian2, Cartesian3, MapProjection, Matrix4, RuntimeError, TypedArray};
use crate::renderer::{Buffer, BufferData, BufferKind, BufferUsage, Context, Gl, ShaderDestination, UniformValue};
use crate::scene::{model_components as mc, SceneMode, SceneTransformState, SceneTransforms, VertexAttributeSemantic};
use std::cell::RefCell;
use std::rc::Rc;

/// Persistent 2D resources stored on the runtime primitive across pipeline runs.
#[derive(Clone)]
pub struct RuntimePrimitive2DState<G: Gl> {
    pub bounding_sphere_2d: Option<BoundingSphere>,
    pub position_buffer_2d: Option<Buffer<G>>,
}

impl<G: Gl> Default for RuntimePrimitive2DState<G> {
    fn default() -> Self {
        Self { bounding_sphere_2d: None, position_buffer_2d: None }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn process<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mut mc::Primitive<G>,
    runtime_primitive: &mut RuntimePrimitive2DState<G>,
    node_computed_transform: &Matrix4,
    mode: SceneMode,
    morph_time: f64,
    map_projection: &dyn MapProjection,
    context: &Context<G>,
    view_matrix: Rc<RefCell<Matrix4>>,
    model_resources: &mut Vec<Buffer<G>>,
) -> Result<(), RuntimeError> {
    let model_matrix = resources.model.borrow().scene_graph_computed_model_matrix;
    let computed_model_matrix = model_matrix.multiply_transformation(node_computed_transform);

    let transform_state = SceneTransformState {
        mode,
        morph_time,
        map_projection,
        client_dimensions: Cartesian2::ONE,
        drawing_buffer_dimensions: Cartesian2::ONE,
    };

    let bounding_sphere_2d = compute_bounding_sphere_2d(resources, &computed_model_matrix, &transform_state)?;
    runtime_primitive.bounding_sphere_2d = Some(bounding_sphere_2d);

    // If the model is instanced, 2D projection will be handled in the InstancingPipelineStage.
    if resources.has_instances {
        return Ok(());
    }

    let position_attribute = primitive
        .attributes
        .iter_mut()
        .find(|a| a.semantic == Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)))
        .ok_or_else(|| RuntimeError::new("Missing POSITION attribute"))?;

    if position_attribute.typed_array.is_some() {
        let buffer_2d = create_position_buffer_for_2d(
            position_attribute,
            &computed_model_matrix,
            &bounding_sphere_2d,
            morph_time,
            map_projection,
            context,
        )?;
        runtime_primitive.position_buffer_2d = Some(buffer_2d.clone());
        model_resources.push(buffer_2d);
        position_attribute.typed_array = None;
    }

    resources.position_buffer_2d = runtime_primitive.position_buffer_2d.clone();

    resources.shader_builder.add_define("USE_2D_POSITIONS", None, ShaderDestination::VERTEX);
    resources.shader_builder.add_uniform("mat4", "u_modelView2D", ShaderDestination::VERTEX);

    let model_matrix_2d = Matrix4::from_translation(bounding_sphere_2d.center);
    resources.uniform_map.insert(
        "u_modelView2D".into(),
        Rc::new(move || {
            let model_view_2d = view_matrix.borrow().multiply_transformation(&model_matrix_2d);
            UniformValue::Matrix { dimension: 4, values: model_view_2d.0.iter().map(|&v| v as f32).collect() }
        }),
    );

    Ok(())
}

pub fn compute_bounding_sphere_2d<G: Gl>(
    resources: &PrimitiveRenderResources<G>,
    model_matrix: &Matrix4,
    transform_state: &SceneTransformState<'_>,
) -> Result<BoundingSphere, RuntimeError> {
    let position_min = resources.position_min.ok_or_else(|| RuntimeError::new("Missing positionMin"))?;
    let position_max = resources.position_max.ok_or_else(|| RuntimeError::new("Missing positionMax"))?;

    let transformed_min = model_matrix.multiply_by_point(position_min);
    let projected_min = SceneTransforms::compute_actual_ellipsoid_position(transform_state, transformed_min)
        .unwrap_or(Cartesian3::ZERO);

    let transformed_max = model_matrix.multiply_by_point(position_max);
    let projected_max = SceneTransforms::compute_actual_ellipsoid_position(transform_state, transformed_max)
        .unwrap_or(Cartesian3::ZERO);

    Ok(BoundingSphere::from_corner_points(projected_min, projected_max))
}

fn dequantize_positions_typed_array(
    typed_array: &TypedArray,
    quantization: &mc::Quantization,
) -> Result<Vec<f32>, RuntimeError> {
    let length = typed_array.len();
    let mut dequantized = vec![0.0f32; length];

    let offset = match quantization.quantized_volume_offset {
        Some(mc::MathValue::Cartesian3(v)) => v,
        _ => return Err(RuntimeError::new("Missing quantizedVolumeOffset")),
    };
    let step_size = match quantization.quantized_volume_step_size {
        Some(mc::MathValue::Cartesian3(v)) => v,
        _ => return Err(RuntimeError::new("Missing quantizedVolumeStepSize")),
    };

    let mut i = 0;
    while i + 2 < length {
        let initial = Cartesian3::new(
            typed_array.get(i).unwrap_or(0.0),
            typed_array.get(i + 1).unwrap_or(0.0),
            typed_array.get(i + 2).unwrap_or(0.0),
        );
        let scaled = initial.multiply_components(step_size);
        let dequantized_pos = scaled + offset;

        dequantized[i] = dequantized_pos.x as f32;
        dequantized[i + 1] = dequantized_pos.y as f32;
        dequantized[i + 2] = dequantized_pos.z as f32;
        i += 3;
    }

    Ok(dequantized)
}

pub fn create_positions_typed_array_for_2d<G: Gl>(
    attribute: &mc::Attribute<G>,
    model_matrix: &Matrix4,
    reference_point: Cartesian3,
    transform_state_cv: &SceneTransformState<'_>,
) -> Result<Vec<f32>, RuntimeError> {
    let typed_array = attribute.typed_array.as_ref().ok_or_else(|| RuntimeError::new("Missing position typedArray"))?;

    let mut result = if let Some(quantization) = &attribute.quantization {
        dequantize_positions_typed_array(typed_array, quantization)?
    } else {
        (0..typed_array.len()).map(|i| typed_array.get(i).unwrap_or(0.0) as f32).collect()
    };

    let start_index = attribute.byte_offset.unwrap_or(0) / 4;
    let length = result.len();
    let stride = attribute.byte_stride.map_or(3, |s| s / 4);

    let mut i = start_index;
    while i + 2 < length {
        let initial_position = Cartesian3::new(result[i] as f64, result[i + 1] as f64, result[i + 2] as f64);
        if initial_position.x.is_nan() || initial_position.y.is_nan() || initial_position.z.is_nan() {
            i += stride;
            continue;
        }

        let transformed_position = model_matrix.multiply_by_point(initial_position);
        let projected_position =
            SceneTransforms::compute_actual_ellipsoid_position(transform_state_cv, transformed_position)
                .unwrap_or(Cartesian3::ZERO);

        let relative_position = projected_position - reference_point;
        result[i] = relative_position.x as f32;
        result[i + 1] = relative_position.y as f32;
        result[i + 2] = relative_position.z as f32;
        i += stride;
    }

    Ok(result)
}

fn create_position_buffer_for_2d<G: Gl>(
    position_attribute: &mc::Attribute<G>,
    model_matrix: &Matrix4,
    bounding_sphere_2d: &BoundingSphere,
    morph_time: f64,
    map_projection: &dyn MapProjection,
    context: &Context<G>,
) -> Result<Buffer<G>, RuntimeError> {
    let transform_state_cv = SceneTransformState {
        mode: SceneMode::ColumbusView,
        morph_time,
        map_projection,
        client_dimensions: Cartesian2::ONE,
        drawing_buffer_dimensions: Cartesian2::ONE,
    };

    let reference_point = bounding_sphere_2d.center;
    let projected_positions =
        create_positions_typed_array_for_2d(position_attribute, model_matrix, reference_point, &transform_state_cv)?;

    let bytes = TypedArray::Float32(projected_positions).to_le_bytes();
    Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::GeographicProjection;
    use crate::renderer::GlValue;
    use crate::scene::model::{ModelRenderResources, ModelUniformState, NodeRenderResources};
    use crate::scene::AttributeType;

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
    fn projects_positions_to_2d_and_unloads_typed_array() {
        let ctx = Context::new_webgl2(TestGl);
        let model_state = ModelUniformState {
            scene_graph_computed_model_matrix: Matrix4::from_translation(Cartesian3::new(6378137.0, 0.0, 0.0)),
            ..Default::default()
        };
        let state = Rc::new(RefCell::new(model_state));
        let model_res = ModelRenderResources::<TestGl>::new(state);
        let node_res = NodeRenderResources::new(&model_res, 0);

        let mut primitive = mc::Primitive {
            attributes: vec![mc::Attribute {
                semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
                attribute_type: Some(AttributeType::Vec3),
                count: Some(2),
                min: Some(mc::MathValue::Cartesian3(Cartesian3::new(-1.0, -1.0, -1.0))),
                max: Some(mc::MathValue::Cartesian3(Cartesian3::new(1.0, 1.0, 1.0))),
                typed_array: Some(TypedArray::Float32(vec![-1.0, -1.0, -1.0, 1.0, 1.0, 1.0])),
                byte_offset: Some(0),
                ..Default::default()
            }],
            ..Default::default()
        };

        let mut prim_res = PrimitiveRenderResources::new(&node_res, 0, &primitive, None, None).unwrap();
        let mut runtime_prim = RuntimePrimitive2DState::default();
        let projection = GeographicProjection::default();
        let view_matrix = Rc::new(RefCell::new(Matrix4::IDENTITY));
        let mut model_buffers = Vec::new();

        process(
            &mut prim_res,
            &mut primitive,
            &mut runtime_prim,
            &Matrix4::IDENTITY,
            SceneMode::Scene2D,
            0.0,
            &projection,
            &ctx,
            view_matrix,
            &mut model_buffers,
        )
        .unwrap();

        assert!(runtime_prim.bounding_sphere_2d.is_some());
        assert!(runtime_prim.position_buffer_2d.is_some());
        assert!(prim_res.position_buffer_2d.is_some());
        assert!(primitive.attributes[0].typed_array.is_none());
        assert_eq!(model_buffers.len(), 1);
        let (vs, _) = prim_res.shader_builder.sources();
        assert!(vs.defines.iter().any(|d| d == "USE_2D_POSITIONS"));
        assert!(vs.sources[0].contains("uniform mat4 u_modelView2D;"));
    }
}
