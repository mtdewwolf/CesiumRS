//! CPU ray intersection with a model, including instancing and quantization.
use super::{model_utility, ModelReader, ModelSceneGraph};
use crate::{
    core::{
        intersection_tests, vertical_exaggeration, AttributeCompression, Cartesian3, ComponentDatatype, Matrix4, Ray,
        RuntimeError, Transforms,
    },
    renderer::Gl,
    scene::{model_components as mc, FrameState, SceneMode, VertexAttributeSemantic},
};

#[derive(Clone, Copy, Debug)]
pub struct ModelPickOptions {
    pub ready: bool,
    pub webgl2: bool,
    pub back_face_culling: bool,
    pub model_matrix: Matrix4,
    pub vertical_exaggeration: f64,
    pub relative_height: f64,
}
impl Default for ModelPickOptions {
    fn default() -> Self {
        Self {
            ready: true,
            webgl2: true,
            back_face_culling: true,
            model_matrix: Matrix4::IDENTITY,
            vertical_exaggeration: 1.,
            relative_height: 0.,
        }
    }
}
pub fn pick_model<G: Gl, D>(
    graph: &ModelSceneGraph<G, D>,
    ray: &Ray,
    frame: &FrameState,
    options: ModelPickOptions,
) -> Result<Option<Cartesian3>, RuntimeError> {
    if !options.ready || frame.mode == SceneMode::Morphing {
        return Ok(None);
    }
    let mut min_t = f64::MAX;
    for runtime in graph.runtime_nodes().iter().flatten() {
        let Some(node) = graph.node(runtime.node_index()) else {
            continue;
        };
        let mut node_transform = *runtime.computed_transform();
        let mut model_matrix = *graph.computed_model_matrix();
        if node.instances.as_ref().is_some_and(|i| i.transform_in_world_space) {
            model_matrix = options.model_matrix.multiply_transformation(&graph.components().transform);
            node_transform = graph.axis_correction_matrix().multiply_transformation(&node_transform);
        }
        let mut computed = model_matrix.multiply_transformation(&node_transform);
        if frame.mode != SceneMode::Scene3D {
            computed = Transforms::basis_to_2d(frame.uniform.map_projection.as_ref(), &computed);
        }
        let mut transforms = Vec::new();
        if let Some(instances) = &node.instances {
            let count = instances.attributes.first().and_then(|a| a.count).unwrap_or(0);
            let values = match &runtime.instancing.transforms_typed_array {
                Some(values) => Some(values.iter().map(|v| *v as f64).collect::<Vec<_>>()),
                None if options.webgl2 => match &runtime.instancing.instancing_transforms_buffer {
                    Some(buffer) => {
                        let bytes = buffer.get_buffer_data(0, buffer.size_in_bytes())?;
                        Some(bytes.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap()) as f64).collect())
                    }
                    None => None,
                },
                None => None,
            };
            if let Some(values) = values {
                for i in 0..count {
                    let a = values
                        .get(i * 12..i * 12 + 12)
                        .ok_or_else(|| RuntimeError::new("Invalid instance transform buffer"))?;
                    let transform = Matrix4([
                        a[0], a[4], a[8], 0., a[1], a[5], a[9], 0., a[2], a[6], a[10], 0., a[3], a[7], a[11], 1.,
                    ]);
                    transforms.push(if instances.transform_in_world_space {
                        model_matrix.multiply_transformation(&transform.multiply_transformation(&node_transform))
                    } else {
                        transform.multiply_transformation(&computed)
                    });
                }
            }
        }
        if transforms.is_empty() {
            transforms.push(computed);
        }
        for runtime_primitive in &runtime.runtime_primitives {
            let Some(primitive) = node.primitives.get(runtime_primitive.primitive_index) else {
                continue;
            };
            if node.instances.is_none()
                && runtime_primitive
                    .bounding_sphere
                    .is_some_and(|s| intersection_tests::ray_sphere(ray, &s.transform(&computed)).is_none())
            {
                continue;
            }
            let Some(indices) = &primitive.indices else {
                continue;
            };
            let Some(position) = model_utility::get_attribute_by_semantic(
                &primitive.attributes,
                mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position),
                None,
            ) else {
                continue;
            };
            if !options.webgl2 && (indices.typed_array.is_none() || position.typed_array.is_none()) {
                return Ok(None);
            }
            if indices.typed_array.is_none() && indices.buffer.is_none()
                || position.typed_array.is_none() && position.buffer.is_none()
            {
                return Ok(None);
            }
            let indices = ModelReader::read_indices_as_typed_array(indices)?;
            let vertices = ModelReader::read_attribute_as_raw_compact_typed_array(position)?;
            let components = position
                .quantization
                .as_ref()
                .and_then(|q| q.attribute_type)
                .or(position.attribute_type)
                .ok_or_else(|| RuntimeError::new("Missing position attribute type"))?
                .get_number_of_components();
            for triangle in 0..indices.len() / 3 {
                let mut points = [Cartesian3::ZERO; 3];
                for transform in &transforms {
                    for (corner, point) in points.iter_mut().enumerate() {
                        let i = indices.get(triangle * 3 + corner).unwrap_or(0.) as usize * components;
                        let x = vertices.get(i).ok_or_else(|| RuntimeError::new("Position index out of range"))?;
                        let y = vertices.get(i + 1).unwrap_or(0.);
                        let z = vertices.get(i + 2).unwrap_or(0.);
                        let mut value = Cartesian3::new(x, y, z);
                        if let Some(q) = &position.quantization {
                            // Cesium normalizes quantized GPU readback, but retained
                            // CPU typed arrays already have the loader's representation.
                            if position.typed_array.is_none() && position.normalized {
                                value = normalize(
                                    value,
                                    q.component_datatype
                                        .or(position.component_datatype)
                                        .unwrap_or(ComponentDatatype::Float),
                                );
                            }
                            if q.oct_encoded {
                                let range = match q.normalization_range {
                                    Some(mc::MathValue::Number(v)) => v,
                                    _ => 255.,
                                };
                                value = AttributeCompression::oct_decode_in_range(value.x, value.y, range);
                                if q.oct_encoded_zxy {
                                    value = Cartesian3::new(value.z, value.x, value.y);
                                }
                            } else {
                                let step = match q.quantized_volume_step_size {
                                    Some(mc::MathValue::Cartesian3(v)) => v,
                                    _ => return Err(RuntimeError::new("Missing quantized position step")),
                                };
                                let offset = match q.quantized_volume_offset {
                                    Some(mc::MathValue::Cartesian3(v)) => v,
                                    _ => return Err(RuntimeError::new("Missing quantized position offset")),
                                };
                                value = value.multiply_components(step).add(offset);
                            }
                        }
                        value = transform.multiply_by_point(value);
                        if options.vertical_exaggeration != 1. {
                            value = vertical_exaggeration::get_position(
                                value,
                                frame.uniform.map_projection.ellipsoid(),
                                options.vertical_exaggeration,
                                options.relative_height,
                            );
                        }
                        *point = value;
                    }
                    if let Some(t) = intersection_tests::ray_triangle_parametric(
                        ray,
                        points[0],
                        points[1],
                        points[2],
                        options.back_face_culling,
                    ) {
                        if t >= 0. && t < min_t {
                            min_t = t;
                        }
                    }
                }
            }
        }
    }
    if min_t == f64::MAX {
        return Ok(None);
    }
    let mut point = ray.origin.add(ray.direction.multiply_by_scalar(min_t));
    if frame.mode != SceneMode::Scene3D {
        let projected = Cartesian3::new(point.y, point.z, point.x);
        let c = frame.uniform.map_projection.unproject(projected);
        point = frame.uniform.map_projection.ellipsoid().cartographic_to_cartesian(&c);
    }
    Ok(Some(point))
}
fn normalize(value: Cartesian3, datatype: ComponentDatatype) -> Cartesian3 {
    let (scale, signed) = match datatype {
        ComponentDatatype::Byte => (127., true),
        ComponentDatatype::UnsignedByte => (255., false),
        ComponentDatatype::Short => (32767., true),
        ComponentDatatype::UnsignedShort => (65535., false),
        ComponentDatatype::Int => (2147483647., true),
        ComponentDatatype::UnsignedInt => (4294967295., false),
        _ => (1., false),
    };
    let norm = |v: f64| if signed { (v / scale).max(-1.) } else { v / scale };
    Cartesian3::new(norm(value.x), norm(value.y), norm(value.z))
}
