//! CesiumJS `Scene/Model/EdgeVisibilityPipelineStage`: builds derived line
//! geometry for model edges using EXT_mesh_primitive_edge_visibility data.

use super::{EdgeGeometry, ModelReader, PrimitiveRenderResources};
use crate::{
    core::{Cartesian3, ComponentDatatype, IndexDatatype, PrimitiveType, RuntimeError},
    renderer::{
        Buffer, BufferData, BufferKind, BufferUsage, Context, Gl, Pass, ShaderDestination, UniformValue, VertexArray,
        VertexAttribute,
    },
    scene::{model_components as mc, FrameState, VertexAttributeSemantic},
    shaders,
};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

/// Derives edge geometry and shader bindings for primitives containing
/// EXT_mesh_primitive_edge_visibility data.
pub fn process<G: Gl>(
    render_resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    frame_state: &mut FrameState,
    context: &Context<G>,
) -> Result<(), RuntimeError> {
    let edge_visibility = match &primitive.edge_visibility {
        Some(ev) => ev,
        None => return Ok(()),
    };

    frame_state.edge_visibility_requested = true;

    let shader_builder = &mut render_resources.shader_builder;

    shader_builder.add_define("HAS_EDGE_VISIBILITY", None, ShaderDestination::BOTH);
    shader_builder.add_define("HAS_EDGE_VISIBILITY_MRT", None, ShaderDestination::FRAGMENT);
    shader_builder.add_fragment_lines(&[shaders::model::EDGE_VISIBILITY_STAGE_FS.source]);
    shader_builder.add_uniform("bool", "u_isEdgePass", ShaderDestination::BOTH);

    let edge_type_location = shader_builder.add_attribute("float", "a_edgeType");
    shader_builder.add_varying("float", "v_edgeType", Some("flat"));

    let edge_feature_id_location = shader_builder.add_attribute("float", "a_edgeFeatureId");

    let silhouette_normal_location = shader_builder.add_attribute("vec3", "a_silhouetteNormal");
    shader_builder.add_varying("vec3", "v_silhouetteNormalView", Some("flat"));

    let face_normal_a_location = shader_builder.add_attribute("vec3", "a_faceNormalA");
    let face_normal_b_location = shader_builder.add_attribute("vec3", "a_faceNormalB");
    shader_builder.add_varying("vec3", "v_faceNormalAView", Some("flat"));
    shader_builder.add_varying("vec3", "v_faceNormalBView", Some("flat"));

    shader_builder.add_function_lines(
        "setDynamicVaryingsVS",
        &[
            "#ifdef HAS_EDGE_VISIBILITY",
            "  if (u_isEdgePass) {",
            "    v_edgeType = a_edgeType;",
            "#ifdef HAS_EDGE_FEATURE_ID",
            "    v_featureId_0 = a_edgeFeatureId;",
            "#endif",
            "    // Transform normals from model space to view space",
            "    v_silhouetteNormalView = czm_normal * a_silhouetteNormal;",
            "    v_faceNormalAView = czm_normal * a_faceNormalA;",
            "    v_faceNormalBView = czm_normal * a_faceNormalB;",
            "  }",
            "#endif",
        ],
    )?;

    let indices = match primitive.indices {
        Some(ref ind) => ModelReader::read_indices_as_typed_array(ind)?,
        None => return Ok(()),
    };

    let position_attr = primitive
        .attributes
        .iter()
        .find(|a| a.semantic == Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)))
        .ok_or_else(|| RuntimeError::new("Missing POSITION attribute"))?;

    let mut positions = if let Some(ref ta) = position_attr.typed_array {
        ta.to_f64_vec()
    } else {
        ModelReader::read_attribute_as_typed_array(position_attr)?.to_f64_vec()
    };

    if let Some(ref q) = position_attr.quantization {
        if !q.oct_encoded {
            if let (Some(ref offset_val), Some(ref step_val)) =
                (&q.quantized_volume_offset, &q.quantized_volume_step_size)
            {
                let offset = offset_val.to_f32_vec();
                let step = step_val.to_f32_vec();
                for i in (0..positions.len()).step_by(3) {
                    if i + 2 < positions.len() {
                        positions[i] = (offset[0] as f64) + positions[i] * (step[0] as f64);
                        positions[i + 1] = (offset[1] as f64) + positions[i + 1] * (step[1] as f64);
                        positions[i + 2] = (offset[2] as f64) + positions[i + 2] * (step[2] as f64);
                    }
                }
            }
        }
    }

    let triangle_indices = indices.to_f64_vec();
    let triangle_count = triangle_indices.len() / 3;
    let mut face_normals = vec![0.0f32; triangle_count * 3];
    let mut edge_map: HashMap<(usize, usize), Vec<usize>> = HashMap::new();

    for t in 0..triangle_count {
        let base = t * 3;
        let i0 = triangle_indices[base] as usize;
        let i1 = triangle_indices[base + 1] as usize;
        let i2 = triangle_indices[base + 2] as usize;

        if i0 * 3 + 2 >= positions.len() || i1 * 3 + 2 >= positions.len() || i2 * 3 + 2 >= positions.len() {
            continue;
        }

        let p0 = Cartesian3::new(positions[i0 * 3], positions[i0 * 3 + 1], positions[i0 * 3 + 2]);
        let p1 = Cartesian3::new(positions[i1 * 3], positions[i1 * 3 + 1], positions[i1 * 3 + 2]);
        let p2 = Cartesian3::new(positions[i2 * 3], positions[i2 * 3 + 1], positions[i2 * 3 + 2]);

        let e1 = p1.subtract(p0);
        let e2 = p2.subtract(p0);
        let normal = e1.cross(e2).normalize();

        face_normals[base] = normal.x as f32;
        face_normals[base + 1] = normal.y as f32;
        face_normals[base + 2] = normal.z as f32;

        let mut add_edge = |a: usize, b: usize| {
            let key = if a < b { (a, b) } else { (b, a) };
            let entry = edge_map.entry(key).or_default();
            if entry.len() < 2 {
                entry.push(t);
            }
        };
        add_edge(i0, i1);
        add_edge(i1, i2);
        add_edge(i2, i0);
    }

    let visibility_bytes: Vec<u8> = edge_visibility
        .visibility
        .iter()
        .map(|v| match v {
            mc::MathValue::Number(n) => *n as u8,
            _ => 0,
        })
        .collect();

    let vertex_count = position_attr.count.unwrap_or(positions.len() / 3);
    let mut edge_indices = Vec::new();
    struct EdgeInfo {
        edge_type: u8,
        mate_vertex_index: i32,
    }
    let mut edge_data = Vec::new();
    let mut seen_edge_hashes: HashSet<usize> = HashSet::new();
    let mut silhouette_edge_count: i32 = 0;

    let mut edge_index = 0;
    for i in (0..triangle_indices.len()).step_by(3) {
        if i + 2 >= triangle_indices.len() {
            break;
        }
        let v0 = triangle_indices[i] as usize;
        let v1 = triangle_indices[i + 1] as usize;
        let v2 = triangle_indices[i + 2] as usize;

        for e in 0..3 {
            let (a, b) = match e {
                0 => (v0, v1),
                1 => (v1, v2),
                _ => (v2, v0),
            };

            let byte_index = edge_index / 4;
            let bit_pair_offset = (edge_index % 4) * 2;
            edge_index += 1;

            if byte_index >= visibility_bytes.len() {
                break;
            }

            let byte = visibility_bytes[byte_index];
            let visibility_2bit = (byte >> bit_pair_offset) & 0x3;

            let should_include = visibility_2bit == 1 || visibility_2bit == 2 || visibility_2bit == 3;
            if should_include {
                let small = a.min(b);
                let big = a.max(b);
                let hash = small * vertex_count + big;

                if seen_edge_hashes.insert(hash) {
                    edge_indices.push(a);
                    edge_indices.push(b);

                    let mate_idx = if visibility_2bit == 1 {
                        let idx = silhouette_edge_count;
                        silhouette_edge_count += 1;
                        idx
                    } else {
                        -1
                    };

                    edge_data.push(EdgeInfo { edge_type: visibility_2bit, mate_vertex_index: mate_idx });
                }
            }
        }
    }

    if edge_indices.is_empty() {
        return Ok(());
    }

    let num_edges = edge_data.len();
    let mut edge_face_normals = vec![0.0f32; num_edges * 6];

    for i in 0..num_edges {
        let a = edge_indices[i * 2];
        let b = edge_indices[i * 2 + 1];
        let key = if a < b { (a, b) } else { (b, a) };
        if let Some(tri_list) = edge_map.get(&key) {
            if !tri_list.is_empty() {
                let t_a = tri_list[0];
                let a_base = t_a * 3;
                let n_ax = face_normals[a_base];
                let n_ay = face_normals[a_base + 1];
                let n_az = face_normals[a_base + 2];

                let (n_bx, n_by, n_bz) = if tri_list.len() > 1 {
                    let t_b = tri_list[1];
                    let b_base = t_b * 3;
                    (face_normals[b_base], face_normals[b_base + 1], face_normals[b_base + 2])
                } else {
                    (-n_ax, -n_ay, -n_az)
                };

                let base_idx = i * 6;
                edge_face_normals[base_idx] = n_ax;
                edge_face_normals[base_idx + 1] = n_ay;
                edge_face_normals[base_idx + 2] = n_az;
                edge_face_normals[base_idx + 3] = n_bx;
                edge_face_normals[base_idx + 4] = n_by;
                edge_face_normals[base_idx + 5] = n_bz;
            }
        }
    }

    let verts_per_edge = 2;
    let total_verts = num_edges * verts_per_edge;

    let mut edge_pos = Vec::with_capacity(total_verts * 3);
    let mut edge_types = Vec::with_capacity(total_verts);
    let mut sil_normals = Vec::with_capacity(total_verts * 3);
    let mut face_normals_a = Vec::with_capacity(total_verts * 3);
    let mut face_normals_b = Vec::with_capacity(total_verts * 3);

    let max_src_vertex = if positions.len() >= 3 { positions.len() / 3 - 1 } else { 0 };

    for i in 0..num_edges {
        let a = edge_indices[i * 2];
        let b = edge_indices[i * 2 + 1];

        if a > max_src_vertex || b > max_src_vertex {
            edge_pos.extend_from_slice(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
            edge_types.extend_from_slice(&[0.0, 0.0]);
            sil_normals.extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
            face_normals_a.extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
            face_normals_b.extend_from_slice(&[0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
            continue;
        }

        let ax = positions[a * 3] as f32;
        let ay = positions[a * 3 + 1] as f32;
        let az = positions[a * 3 + 2] as f32;
        let bx = positions[b * 3] as f32;
        let by = positions[b * 3 + 1] as f32;
        let bz = positions[b * 3 + 2] as f32;

        edge_pos.extend_from_slice(&[ax, ay, az, bx, by, bz]);

        let raw_type = edge_data[i].edge_type;
        let t = raw_type as f32 / 255.0;
        edge_types.extend_from_slice(&[t, t]);

        let mut nx = 0.0f32;
        let mut ny = 0.0f32;
        let mut nz = 1.0f32;

        if raw_type == 1 {
            if let Some(ref sil_normals_list) = edge_visibility.silhouette_normals {
                let mate_idx = edge_data[i].mate_vertex_index;
                if mate_idx >= 0 && (mate_idx as usize) < sil_normals_list.len() {
                    if let mc::MathValue::Cartesian3(c) = sil_normals_list[mate_idx as usize] {
                        nx = c.x as f32;
                        ny = c.y as f32;
                        nz = c.z as f32;
                    }
                }
            }
        }
        sil_normals.extend_from_slice(&[nx, ny, nz, nx, ny, nz]);

        let fn_idx = i * 6;
        let n_ax = edge_face_normals[fn_idx];
        let n_ay = edge_face_normals[fn_idx + 1];
        let n_az = edge_face_normals[fn_idx + 2];
        let n_bx = edge_face_normals[fn_idx + 3];
        let n_by = edge_face_normals[fn_idx + 4];
        let n_bz = edge_face_normals[fn_idx + 5];

        face_normals_a.extend_from_slice(&[n_ax, n_ay, n_az, n_ax, n_ay, n_az]);
        face_normals_b.extend_from_slice(&[n_bx, n_by, n_bz, n_bx, n_by, n_bz]);
    }

    let pos_bytes: Vec<u8> = edge_pos.iter().flat_map(|x| x.to_le_bytes()).collect();
    let type_bytes: Vec<u8> = edge_types.iter().flat_map(|x| x.to_le_bytes()).collect();
    let sil_bytes: Vec<u8> = sil_normals.iter().flat_map(|x| x.to_le_bytes()).collect();
    let fa_bytes: Vec<u8> = face_normals_a.iter().flat_map(|x| x.to_le_bytes()).collect();
    let fb_bytes: Vec<u8> = face_normals_b.iter().flat_map(|x| x.to_le_bytes()).collect();

    let pos_buffer = Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&pos_bytes))?;
    let type_buffer =
        Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&type_bytes))?;
    let sil_buffer = Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&sil_bytes))?;
    let fa_buffer = Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&fa_bytes))?;
    let fb_buffer = Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&fb_bytes))?;

    let mut va_attributes = vec![
        VertexAttribute {
            index: 0,
            enabled: true,
            source: crate::renderer::AttributeSource::Buffer(pos_buffer),
            components: 3,
            datatype: ComponentDatatype::Float,
            normalize: false,
            offset: 0,
            stride: 0,
            instance_divisor: 0,
        },
        VertexAttribute {
            index: edge_type_location,
            enabled: true,
            source: crate::renderer::AttributeSource::Buffer(type_buffer),
            components: 1,
            datatype: ComponentDatatype::Float,
            normalize: false,
            offset: 0,
            stride: 0,
            instance_divisor: 0,
        },
        VertexAttribute {
            index: silhouette_normal_location,
            enabled: true,
            source: crate::renderer::AttributeSource::Buffer(sil_buffer),
            components: 3,
            datatype: ComponentDatatype::Float,
            normalize: false,
            offset: 0,
            stride: 0,
            instance_divisor: 0,
        },
        VertexAttribute {
            index: face_normal_a_location,
            enabled: true,
            source: crate::renderer::AttributeSource::Buffer(fa_buffer),
            components: 3,
            datatype: ComponentDatatype::Float,
            normalize: false,
            offset: 0,
            stride: 0,
            instance_divisor: 0,
        },
        VertexAttribute {
            index: face_normal_b_location,
            enabled: true,
            source: crate::renderer::AttributeSource::Buffer(fb_buffer),
            components: 3,
            datatype: ComponentDatatype::Float,
            normalize: false,
            offset: 0,
            stride: 0,
            instance_divisor: 0,
        },
    ];

    let mut has_edge_feature_ids = false;
    if let Some(mc::FeatureId::Attribute(ref fid_attr)) = primitive.feature_ids.first() {
        let set_idx = fid_attr.set_index;
        let fid_attribute = primitive.attributes.iter().find(|a| {
            a.semantic == Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId))
                && a.set_index == set_idx
        });
        if let Some(attr) = fid_attribute {
            let fid_array = if let Some(ref ta) = attr.typed_array {
                ta.to_f64_vec()
            } else {
                ModelReader::read_attribute_as_typed_array(attr)?.to_f64_vec()
            };
            let mut edge_fids = Vec::with_capacity(total_verts);
            for i in 0..num_edges {
                let a = edge_indices[i * 2];
                let fid = fid_array.get(a).copied().unwrap_or(0.0) as f32;
                edge_fids.push(fid);
                edge_fids.push(fid);
            }
            let bytes: Vec<u8> = edge_fids.iter().flat_map(|x| x.to_le_bytes()).collect();
            let edge_fid_buf =
                Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?;
            va_attributes.push(VertexAttribute {
                index: edge_feature_id_location,
                enabled: true,
                source: crate::renderer::AttributeSource::Buffer(edge_fid_buf),
                components: 1,
                datatype: ComponentDatatype::Float,
                normalize: false,
                offset: 0,
                stride: 0,
                instance_divisor: 0,
            });
            has_edge_feature_ids = true;
        }
    }

    if has_edge_feature_ids {
        shader_builder.add_define("HAS_EDGE_FEATURE_ID", None, ShaderDestination::BOTH);
    }

    render_resources.uniform_map.insert("u_isEdgePass".into(), Rc::new(|| UniformValue::Int(vec![0])));

    let use_u32 = total_verts > 65534;
    let idx_datatype = if use_u32 { IndexDatatype::UnsignedInt } else { IndexDatatype::UnsignedShort };

    let idx_bytes: Vec<u8> = if use_u32 {
        (0..total_verts as u32).flat_map(|i| i.to_le_bytes()).collect()
    } else {
        (0..total_verts as u16).flat_map(|i| i.to_le_bytes()).collect()
    };

    let index_buffer =
        Buffer::new(context, BufferKind::Index(idx_datatype), BufferUsage::StaticDraw, BufferData::Bytes(&idx_bytes))?;

    let va = VertexArray::new(context, va_attributes, Some(index_buffer))?;

    render_resources.edge_geometry = Some(EdgeGeometry {
        vertex_array: Some(va),
        index_count: total_verts,
        primitive_type: PrimitiveType::Lines,
        pass: Pass::Cesium3DTileEdges,
    });

    Ok(())
}
