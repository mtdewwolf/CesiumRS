//! Scene's derived shader sources and render states. ShaderCache owns the
//! compiled variants; ordinary and derived commands share their GPU inputs.
use super::{MetadataClassProperty, MetadataValue};
use crate::{
    core::{math::js_number_to_string, RuntimeError},
    renderer::{render_state::ColorMask, *},
};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub struct PickMetadataShader {
    pub schema_id: Option<String>,
    pub class_name: String,
    pub property_name: String,
    pub glsl_type: String,
    pub component_count: usize,
    pub normalized: bool,
    pub maximum: f64,
    pub value_transform: Option<(MetadataValue, MetadataValue)>,
}
impl PickMetadataShader {
    pub fn new(
        schema_id: Option<String>,
        class_name: String,
        property_name: String,
        class: &MetadataClassProperty,
        value_transform: Option<(MetadataValue, MetadataValue)>,
    ) -> Result<Self, RuntimeError> {
        let component_count = if class.is_array() {
            class.array_length_count().unwrap_or(0)
        } else {
            class.metadata_type().get_component_count()
        };
        Ok(Self {
            schema_id,
            class_name,
            property_name,
            glsl_type: class
                .get_glsl_type()
                .map_err(|e| RuntimeError::new(e.to_string()))?
                .unwrap_or("undefined")
                .into(),
            component_count,
            normalized: class.normalized(),
            maximum: class.value_type().map_or(f64::NAN, |t| t.get_maximum().to_f64()),
            value_transform,
        })
    }
    pub fn cache_key(&self) -> String {
        format!(
            "pickMetadata-{}-{}-{}",
            self.schema_id.as_deref().unwrap_or("undefined"),
            self.class_name,
            self.property_name
        )
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SceneDrawMode {
    pub logarithmic_depth: bool,
    pub high_dynamic_range: bool,
    pub depth_only: bool,
    pub pick: bool,
    pub pick_metadata: bool,
}
pub struct DerivedCommand;
impl DerivedCommand {
    pub fn depth_only(options: &ShaderProgramOptions) -> ShaderProgramOptions {
        let mut result = options.clone();
        let fs = &options.fragment_shader_source;
        let writes = regex::Regex::new(r"\b(?:gl_FragDepth|discard)\b").unwrap();
        if !fs.sources.iter().any(|s| writes.is_match(s)) {
            result.fragment_shader_source = if fs.defines.iter().any(|d| d == "LOG_DEPTH") {
                ShaderSource {
                    sources: vec![
                        "void main()\n{\n    out_FragColor = vec4(1.0);\n    czm_writeLogDepth();\n}\n".into()
                    ],
                    defines: vec!["LOG_DEPTH".into()],
                    ..Default::default()
                }
            } else {
                ShaderSource::new("void main()\n{\n    out_FragColor = vec4(1.0);\n}\n")
            };
        }
        result
    }
    pub fn depth_only_render_state(state: &RenderState) -> RenderState {
        let mut result = state.clone();
        result.depth_mask = true;
        result.color_mask = ColorMask { red: false, green: false, blue: false, alpha: false };
        result
    }
    pub fn logarithmic_depth(options: &ShaderProgramOptions) -> ShaderProgramOptions {
        if options.fragment_shader_source.defines.iter().any(|d| d == "LOG_DEPTH_READ_ONLY") {
            return options.clone();
        }
        let mut result = options.clone();
        let vs = &mut result.vertex_shader_source;
        let fs = &mut result.fragment_shader_source;
        vs.defines.push("LOG_DEPTH".into());
        fs.defines.push("LOG_DEPTH".into());
        let called = |sources: &[String], name: &str| {
            regex::Regex::new(&format!(r"\s+{name}\(")).unwrap().is_match(&sources.join("\n"))
        };
        if !called(&vs.sources, "czm_vertexLogDepth") {
            for s in &mut vs.sources {
                *s = ShaderSource::replace_main(s, "czm_log_depth_main");
            }
            vs.sources.push("\n\nvoid main()\n{\n    czm_log_depth_main();\n    czm_vertexLogDepth();\n}\n".into());
        }
        let mut log_source = String::new();
        if !called(&fs.sources, "czm_writeLogDepth") && !fs.defines.iter().any(|d| d == "LOG_DEPTH_WRITE") {
            for s in &mut fs.sources {
                *s = ShaderSource::replace_main(s, "czm_log_depth_main");
            }
            log_source = "\nvoid main()\n{\n    czm_log_depth_main();\n    czm_writeLogDepth();\n}\n".into();
        }
        fs.sources.push(log_source);
        result
    }
    pub fn hdr(options: &ShaderProgramOptions) -> ShaderProgramOptions {
        let mut result = options.clone();
        result.vertex_shader_source.defines.push("HDR".into());
        result.fragment_shader_source.defines.push("HDR".into());
        result
    }
    pub fn pick(options: &ShaderProgramOptions, pick_id: &str) -> ShaderProgramOptions {
        let mut result = options.clone();
        let fs = &mut result.fragment_shader_source;
        let output =
            if fs.sources.iter().any(|s| s.contains("out_FragData")) { "out_FragData_0" } else { "out_FragColor" };
        for source in &mut fs.sources {
            *source = ShaderSource::replace_main(source, "czm_non_pick_main");
        }
        fs.sources.push(format!("void main ()\n{{\n    czm_non_pick_main();\n    if ({output}.a == 0.0) {{\n        discard;\n    }}\n    {output} = {pick_id};\n}} "));
        // The source constructs a fresh ShaderSource here.
        fs.include_built_ins = true;
        fs.pick_color_qualifier = None;
        result
    }
    pub fn pick_render_state(state: &RenderState) -> RenderState {
        let mut result = state.clone();
        result.blending.enabled = false;
        result.depth_mask = true;
        result
    }
    pub fn pick_metadata(options: &ShaderProgramOptions, info: &PickMetadataShader) -> ShaderProgramOptions {
        let mut result = options.clone();
        let fs = &mut result.fragment_shader_source;
        let mut values = vec!["0.0".to_owned(); 4];
        for (i, value) in values.iter_mut().enumerate().take(info.component_count) {
            let scalar = info.component_count == 1;
            let name = ["x", "y", "z", "w"][i];
            *value = if scalar { "float(value)".into() } else { format!("float(value.{name})") };
            if let Some((offset, scale)) = &info.value_transform {
                let component = |v: &MetadataValue| {
                    if scalar {
                        v.to_number()
                    } else {
                        let c = super::tileset_expansion::metadata_json(v);
                        c.get(i).and_then(serde_json::Value::as_f64).unwrap_or(f64::NAN)
                    }
                };
                *value = format!(
                    "(({value} - float({})) / float({}))",
                    js_number_to_string(component(offset)),
                    js_number_to_string(component(scale))
                );
            }
            if !info.normalized {
                *value = format!("({value}) / float({})", js_number_to_string(info.maximum));
            }
        }
        fs.defines.push("METADATA_PICKING_ENABLED".into());
        let replacements = [
            ("METADATA_PICKING_VALUE_TYPE", info.glsl_type.as_str()),
            ("METADATA_PICKING_VALUE_STRING", &format!("metadata.{}", info.property_name)),
            ("METADATA_PICKING_VALUE_COMPONENT_X", &values[0]),
            ("METADATA_PICKING_VALUE_COMPONENT_Y", &values[1]),
            ("METADATA_PICKING_VALUE_COMPONENT_Z", &values[2]),
            ("METADATA_PICKING_VALUE_COMPONENT_W", &values[3]),
        ];
        for define in &mut fs.defines {
            for (key, value) in replacements {
                if define.split_whitespace().next() == Some(key) {
                    *define = format!("{key} {value}");
                }
            }
        }
        fs.include_built_ins = true;
        fs.pick_color_qualifier = None;
        result
    }
    /// Compile/cache a variant from uncombined ShaderSource objects. Preserving
    /// those sources avoids duplicate GLSL builtins when a variant adds one.
    pub fn program<G: Gl>(
        cache: &mut ShaderCache<G>,
        parent: &Rc<ShaderProgram<G>>,
        keyword: &str,
        derived: ShaderProgramOptions,
    ) -> Result<Rc<ShaderProgram<G>>, RuntimeError> {
        if let Some(program) = cache.get_derived_shader_program(parent, keyword) {
            return Ok(program);
        }
        cache.create_derived_shader_program(parent, keyword, &derived)
    }
}
