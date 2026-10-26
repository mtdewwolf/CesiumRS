//! CesiumJS `DebugAppearance`: visualizes one vertex attribute as a color.

use super::appearance::{default_render_state, Appearance, AppearanceState};
use super::material::Material;
use crate::core::VertexFormat;
use crate::renderer::render_state::RenderState;

#[derive(Clone, Debug, Default)]
pub struct DebugAppearanceOptions {
    /// Required, as in Cesium.
    pub attribute_name: String,
    pub per_instance_attribute: Option<bool>,
    pub glsl_datatype: Option<String>,
    pub translucent: Option<bool>,
    pub closed: Option<bool>,
    pub vertex_shader_source: Option<String>,
    pub fragment_shader_source: Option<String>,
    pub render_state: Option<RenderState>,
}

#[derive(Clone, Debug)]
pub struct DebugAppearance {
    state: AppearanceState,
    attribute_name: String,
    glsl_datatype: String,
}

impl DebugAppearance {
    pub fn try_new(options: DebugAppearanceOptions) -> Result<Self, crate::core::RuntimeError> {
        if options.attribute_name.is_empty() {
            return Err(crate::core::RuntimeError::new("options.attributeName is required."));
        }
        let attribute_name = options.attribute_name;
        let per_instance = options.per_instance_attribute.unwrap_or(false);
        let mut glsl_datatype = options.glsl_datatype.unwrap_or_else(|| "vec3".to_string());
        let varying = format!("v_{attribute_name}");

        let get_color = if matches!(attribute_name.as_str(), "normal" | "tangent" | "bitangent") {
            format!("vec4 getColor() {{ return vec4(({varying} + vec3(1.0)) * 0.5, 1.0); }}\n")
        } else {
            if attribute_name == "st" {
                glsl_datatype = "vec2".to_string();
            }
            match glsl_datatype.as_str() {
                "float" => format!("vec4 getColor() {{ return vec4(vec3({varying}), 1.0); }}\n"),
                "vec2" => format!("vec4 getColor() {{ return vec4({varying}, 0.0, 1.0); }}\n"),
                "vec3" => format!("vec4 getColor() {{ return vec4({varying}, 1.0); }}\n"),
                "vec4" => format!("vec4 getColor() {{ return {varying}; }}\n"),
                _ => {
                    return Err(crate::core::RuntimeError::new(
                        "options.glslDatatype must be float, vec2, vec3, or vec4.",
                    ))
                }
            }
        };

        let vs = format!(
            "in vec3 position3DHigh;\nin vec3 position3DLow;\nin float batchId;\n{}{}out {glsl_datatype} {varying};\nvoid main()\n{{\nvec4 p = czm_translateRelativeToEye(position3DHigh, position3DLow);\n{}gl_Position = czm_modelViewProjectionRelativeToEye * p;\n}}",
            if per_instance {
                String::new()
            } else {
                format!("in {glsl_datatype} {attribute_name};\n")
            },
            "",
            if per_instance {
                format!("{varying} = czm_batchTable_{attribute_name}(batchId);\n")
            } else {
                format!("{varying} = {attribute_name};\n")
            }
        );
        let fs =
            format!("in {glsl_datatype} {varying};\n{get_color}\nvoid main()\n{{\nout_FragColor = getColor();\n}}");

        let translucent = options.translucent.unwrap_or(false);
        let closed = options.closed.unwrap_or(false);
        let state = AppearanceState {
            material: None,
            translucent,
            closed,
            flat: None,
            face_forward: None,
            vertex_shader_source: options.vertex_shader_source.unwrap_or(vs),
            fragment_shader_source: options.fragment_shader_source.unwrap_or(fs),
            // Cesium passes `false` for translucent here, so no alpha blending
            // is configured even when the debug colors have alpha.
            render_state: default_render_state(false, false, options.render_state),
            vertex_format: VertexFormat::POSITION_ONLY,
        };
        Ok(Self { state, attribute_name, glsl_datatype })
    }

    pub fn attribute_name(&self) -> &str {
        &self.attribute_name
    }
    pub fn glsl_datatype(&self) -> &str {
        &self.glsl_datatype
    }
}

impl Appearance for DebugAppearance {
    fn material(&self) -> Option<&Material> {
        None
    }
    fn material_mut(&mut self) -> Option<&mut Material> {
        None
    }
    fn translucent(&self) -> bool {
        self.state.translucent
    }
    fn closed(&self) -> bool {
        self.state.closed
    }
    fn vertex_shader_source(&self) -> String {
        self.state.vertex_shader_source.clone()
    }
    fn fragment_shader_source(&self) -> String {
        self.state.fragment_shader_source.clone()
    }
    fn render_state(&self) -> &RenderState {
        &self.state.render_state
    }
    fn vertex_format(&self) -> VertexFormat {
        self.state.vertex_format
    }
    fn get_fragment_shader_source(&self) -> String {
        self.state.get_fragment_shader_source()
    }
    fn is_translucent(&self) -> bool {
        self.state.is_translucent()
    }
    fn get_render_state(&self) -> RenderState {
        self.state.get_render_state()
    }
}
