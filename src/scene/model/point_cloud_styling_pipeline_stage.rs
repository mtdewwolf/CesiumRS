//! CesiumJS `Scene/Model/PointCloudStylingPipelineStage`: applies runtime color,
//! size, show styles, and attenuation / back-face culling to point clouds.

use super::PrimitiveRenderResources;
use crate::{
    core::{Cartesian3, Matrix4, RuntimeError},
    renderer::{Gl, Pass, ShaderDestination, UniformFrustum, UniformValue},
    scene::{
        model_components as mc, point_cloud_shading::PointCloudShading, FrameState, SceneMode, VertexAttributeSemantic,
    },
    shaders,
};
use std::rc::Rc;

/// Generated shader functions from a Cesium3DTileStyle for point clouds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointCloudStyleFunctions {
    pub color_style_function: Option<String>,
    pub show_style_function: Option<String>,
    pub point_size_style_function: Option<String>,
    pub style_translucent: bool,
}

/// Options controlling point cloud styling and attenuation.
#[derive(Clone, Debug, PartialEq)]
pub struct PointCloudStylingOptions {
    pub shading: PointCloudShading,
    pub style_functions: Option<PointCloudStyleFunctions>,
    pub default_point_size: f64,
    pub geometric_error: Option<f64>,
    pub time_since_load: f64,
    pub node_transform: Matrix4,
}

impl Default for PointCloudStylingOptions {
    fn default() -> Self {
        Self {
            shading: PointCloudShading::default(),
            style_functions: None,
            default_point_size: 1.0,
            geometric_error: None,
            time_since_load: 0.0,
            node_transform: Matrix4::IDENTITY,
        }
    }
}

/// Processes a point cloud primitive for styling and shading.
pub fn process<G: Gl>(
    render_resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    options: &PointCloudStylingOptions,
    frame_state: &FrameState,
) -> Result<(), RuntimeError> {
    if let Some(ref style_info) = options.style_functions {
        if let Some(ref color_fn) = style_info.color_style_function {
            if color_fn.contains("attributes.normalMC") {
                let has_normals = primitive
                    .attributes
                    .iter()
                    .any(|a| a.semantic == Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Normal)));
                if !has_normals {
                    return Err(RuntimeError::new(
                        "Style references the NORMAL semantic but the point cloud does not have normals",
                    ));
                }
            }

            render_resources.shader_builder.add_define("HAS_POINT_CLOUD_COLOR_STYLE", None, ShaderDestination::BOTH);
            render_resources.shader_builder.add_vertex_lines(&[color_fn.as_str()]);
            render_resources.shader_builder.add_varying("vec4", "v_pointCloudColor", None);
        }

        if let Some(ref show_fn) = style_info.show_style_function {
            render_resources.shader_builder.add_define("HAS_POINT_CLOUD_SHOW_STYLE", None, ShaderDestination::BOTH);
            render_resources.shader_builder.add_vertex_lines(&[show_fn.as_str()]);
            render_resources.shader_builder.add_varying("float", "v_pointCloudShow", None);
        }

        if let Some(ref size_fn) = style_info.point_size_style_function {
            render_resources.shader_builder.add_define(
                "HAS_POINT_CLOUD_POINT_SIZE_STYLE",
                None,
                ShaderDestination::VERTEX,
            );
            render_resources.shader_builder.add_vertex_lines(&[size_fn.as_str()]);
        }

        render_resources.shader_builder.add_define("COMPUTE_POSITION_WC_STYLE", None, ShaderDestination::VERTEX);

        if style_info.style_translucent {
            render_resources.alpha_options.borrow_mut().pass = Some(Pass::Translucent);
        }
    }

    if options.shading.attenuation {
        render_resources.shader_builder.add_define("HAS_POINT_CLOUD_ATTENUATION", None, ShaderDestination::VERTEX);
    }

    if options.shading.back_face_culling {
        render_resources.shader_builder.add_define(
            "HAS_POINT_CLOUD_BACK_FACE_CULLING",
            None,
            ShaderDestination::VERTEX,
        );
    }

    render_resources.shader_builder.add_uniform("vec4", "model_pointCloudParameters", ShaderDestination::VERTEX);
    render_resources.shader_builder.add_vertex_lines(&[shaders::model::POINT_CLOUD_STYLING_STAGE_VS.source]);

    // Compute parameters:
    // x: point size
    let point_size = options.shading.maximum_attenuation.unwrap_or(options.default_point_size);
    let pixel_ratio = frame_state.pixel_ratio;
    let param_x = (point_size * pixel_ratio) as f32;

    // y: geometric error
    let geom_error = if let Some(ge) = options.geometric_error {
        ge
    } else if let Some(res) = options.shading.base_resolution {
        res
    } else {
        estimate_geometric_error(render_resources, primitive, &options.node_transform)
    };
    let param_y = (geom_error * options.shading.geometric_error_scale) as f32;

    // z: depth multiplier
    let is_ortho = matches!(
        frame_state.uniform.camera.frustum,
        UniformFrustum::Orthographic(_) | UniformFrustum::OrthographicOffCenter(_)
    );
    let depth_multiplier = if frame_state.mode == SceneMode::Scene2D || is_ortho {
        f32::INFINITY
    } else {
        let sse_denom = match &frame_state.uniform.camera.frustum {
            UniformFrustum::Perspective(f) => f.sse_denominator(),
            _ => 0.0,
        };
        if sse_denom > 0.0 {
            (frame_state.drawing_buffer_height as f64 / sse_denom) as f32
        } else {
            1.0
        }
    };

    // w: time
    let param_w = options.time_since_load as f32;

    render_resources.uniform_map.insert(
        "model_pointCloudParameters".into(),
        Rc::new(move || UniformValue::Float(vec![param_x, param_y, depth_multiplier, param_w])),
    );

    Ok(())
}

fn estimate_geometric_error<G: Gl>(
    render_resources: &PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    node_transform: &Matrix4,
) -> f64 {
    let position_attr = primitive
        .attributes
        .iter()
        .find(|a| a.semantic == Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)));

    let (min, max, count) = if let Some(attr) = position_attr {
        let min = attr
            .min
            .and_then(|v| match v {
                mc::MathValue::Cartesian3(c) => Some(c),
                _ => None,
            })
            .or(render_resources.position_min);
        let max = attr
            .max
            .and_then(|v| match v {
                mc::MathValue::Cartesian3(c) => Some(c),
                _ => None,
            })
            .or(render_resources.position_max);
        (min, max, attr.count.unwrap_or(1))
    } else {
        (render_resources.position_min, render_resources.position_max, 1)
    };

    if let (Some(min), Some(max)) = (min, max) {
        let dimensions = Cartesian3::new(max.x - min.x, max.y - min.y, max.z - min.z);
        // Multiply by transform as vector (ignoring translation)
        let transformed_dim = node_transform.multiply_by_point_as_vector(dimensions);
        let volume = (transformed_dim.x * transformed_dim.y * transformed_dim.z).abs();
        if count > 0 && volume > 0.0 {
            return (volume / count as f64).cbrt();
        }
    }

    0.0
}
