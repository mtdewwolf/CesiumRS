//! CesiumJS ShaderSource: dependency graph, picking and GLSL version assembly.
use super::{
    context::*,
    shader::{ShaderCapabilities, ShaderSource},
};
use crate::core::RuntimeError;
use regex::Regex;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::OnceLock,
};
pub fn builtin_sources() -> &'static HashMap<String, String> {
    static BUILTINS: OnceLock<HashMap<String, String>> = OnceLock::new();
    BUILTINS.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/renderer/glsl-builtins.json"))
            .expect("pinned Cesium GLSL table")
    })
}
pub(crate) fn remove_comments(source: &str) -> String {
    let source = Regex::new(r"//[^\r\n]*").unwrap().replace_all(source, "");
    Regex::new(r"(?s)/\*\*.*?\*/")
        .unwrap()
        .replace_all(&source, |c: &regex::Captures<'_>| "\n".repeat(c[0].bytes().filter(|b| *b == b'\n').count()))
        .into_owned()
}
struct Node {
    name: String,
    source: String,
    dependencies: Vec<usize>,
    required_by: Vec<usize>,
    evaluated: bool,
}
pub fn resolve_dependencies(source: &str, registry: &HashMap<String, String>) -> Result<String> {
    fn add(name: &str, source: &str, nodes: &mut Vec<Node>) -> usize {
        if let Some(i) = nodes.iter().position(|n| n.name == name) {
            return i;
        }
        nodes.push(Node {
            name: name.into(),
            source: remove_comments(source),
            dependencies: vec![],
            required_by: vec![],
            evaluated: false,
        });
        nodes.len() - 1
    }
    fn generate(index: usize, nodes: &mut Vec<Node>, registry: &HashMap<String, String>, re: &Regex) {
        if nodes[index].evaluated {
            return;
        }
        nodes[index].evaluated = true;
        let mut seen = HashSet::new();
        let matches: Vec<_> = re
            .find_iter(&nodes[index].source)
            .map(|m| m.as_str().to_string())
            .filter(|s| seen.insert(s.clone()))
            .collect();
        for name in matches {
            if name == nodes[index].name {
                continue;
            }
            if let Some(source) = registry.get(&name) {
                let dependency = add(&name, source, nodes);
                nodes[index].dependencies.push(dependency);
                nodes[dependency].required_by.push(index);
                generate(dependency, nodes, registry, re);
            }
        }
    }
    let mut nodes = vec![];
    add("main", source, &mut nodes);
    generate(0, &mut nodes, registry, &Regex::new(r"\bczm_[a-zA-Z0-9_]*").unwrap());
    let mut free = VecDeque::new();
    for i in (0..nodes.len()).rev() {
        if nodes[i].required_by.is_empty() {
            free.push_back(i);
        }
    }
    let mut sorted = vec![];
    while let Some(i) = free.pop_front() {
        sorted.push(i);
        for d in nodes[i].dependencies.clone() {
            if let Some(p) = nodes[d].required_by.iter().position(|v| *v == i) {
                nodes[d].required_by.remove(p);
            }
            if nodes[d].required_by.is_empty() {
                free.push_back(d);
            }
        }
    }
    ensure(sorted.len() == nodes.len(), "A circular dependency was found in the built-in functions/structs/constants")?;
    let text = sorted.iter().rev().map(|i| format!("{}\n", nodes[*i].source)).collect::<String>();
    Ok(text.replacen(&nodes[0].source, "", 1))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickColorQualifier {
    Uniform,
    In,
}
impl PickColorQualifier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::In => "in",
        }
    }
}
impl ShaderSource {
    pub fn cache_key(&self) -> String {
        let mut defines = self.defines.clone();
        defines.sort_by_cached_key(|s| s.encode_utf16().collect::<Vec<_>>());
        format!(
            "{}:{}:{}:{}",
            defines.join(","),
            self.pick_color_qualifier.map(PickColorQualifier::as_str).unwrap_or("undefined"),
            self.include_built_ins,
            self.sources.join("\n")
        )
    }
    pub fn create_pick_vertex_shader_source(source: &str) -> String {
        format!("{}\nin vec4 pickColor; \nout vec4 czm_pickColor; \nvoid main() \n{{ \n    czm_old_main(); \n    czm_pickColor = pickColor; \n}}",Self::replace_main(source,"czm_old_main"))
    }
    pub fn create_pick_fragment_shader_source(source: &str, qualifier: PickColorQualifier) -> String {
        format!("{}\n{} vec4 czm_pickColor; \nvoid main() \n{{ \n    czm_old_main(); \n    if (out_FragColor.a == 0.0) {{ \n       discard; \n    }} \n    out_FragColor = czm_pickColor; \n}}",Self::replace_main(source,"czm_old_main"),qualifier.as_str())
    }
    pub fn find_normal_varying(&self) -> Option<&'static str> {
        if self.sources.iter().any(|s| s.contains("#ifdef HAS_NORMALS")) {
            return self.defines.iter().any(|s| s == "HAS_NORMALS").then_some("v_normalEC");
        }
        ["v_normalEC", "v_normal"].into_iter().find(|s| self.sources.iter().any(|v| v.contains(s)))
    }
    pub fn find_position_varying(&self) -> Option<&'static str> {
        self.sources.iter().any(|s| s.contains("v_positionEC")).then_some("v_positionEC")
    }
    pub fn combine(&self, fragment: bool, capabilities: ShaderCapabilities, webgl2: bool) -> Result<String> {
        self.combine_with_registry(fragment, capabilities, webgl2, builtin_sources())
    }
    pub fn combine_with_registry(
        &self,
        fragment: bool,
        capabilities: ShaderCapabilities,
        webgl2: bool,
        registry: &HashMap<String, String>,
    ) -> Result<String> {
        let source = remove_comments(&self.sources.iter().map(|s| format!("\n#line 0\n{s}")).collect::<String>());
        let version_re = Regex::new(r"#version\s+(.*?)\n").unwrap();
        let versions: Vec<_> = version_re.captures_iter(&source).map(|c| c[1].to_string()).collect();
        ensure(versions.windows(2).all(|p| p[0] == p[1]), "Inconsistent GLSL versions")?;
        let source = version_re.replace_all(&source, "\n");
        let ext_re = Regex::new(r"#extension[^\r\n]*\n").unwrap();
        let mut result = ext_re.find_iter(&source).map(|m| m.as_str()).collect::<String>();
        let source = ext_re.replace_all(&source, "\n");
        let source = Regex::new(r"precision\s(lowp|mediump|highp)\s(float|int);").unwrap().replace(&source, "");
        let source = if let Some(q) = self.pick_color_qualifier {
            Self::create_pick_fragment_shader_source(&source, q)
        } else {
            source.into_owned()
        };
        if fragment {
            result.push_str("\n#ifdef GL_FRAGMENT_PRECISION_HIGH\n    precision highp float;\n    precision highp int;\n#else\n    precision mediump float;\n    precision mediump int;\n    #define highp mediump\n#endif\n");
        }
        if webgl2 {
            result.push_str("precision highp sampler3D;\n\n");
        }
        for define in &self.defines {
            if !define.is_empty() {
                result.push_str(&format!("#define {define}\n"));
            }
        }
        if capabilities.texture_float_linear {
            result.push_str("#define OES_texture_float_linear\n\n");
        }
        if capabilities.floating_point_texture {
            result.push_str("#define OES_texture_float\n\n");
        }
        let builtins = if self.include_built_ins { resolve_dependencies(&source, registry)? } else { String::new() };
        result.push_str("\n#line 0\n");
        let combined = format!("{builtins}{source}");
        if webgl2
            && fragment
            && !Regex::new(r"layout\s*\(location\s*=\s*0\)\s*out\s+vec4\s+out_FragColor;").unwrap().is_match(&combined)
            && !combined.contains("czm_out_FragColor")
            && combined.contains("out_FragColor")
        {
            result.push_str("layout(location = 0) out vec4 out_FragColor;\n\n");
        }
        result.push_str(&combined);
        Ok(if webgl2 { format!("#version 300 es\n{result}") } else { demodernize_shader(&result, fragment) })
    }
}
/// The same limited GLSL 300 -> 100 rewrite Cesium uses, including its exact
/// regex matching behavior. This is intentionally not a general GLSL compiler.
pub fn demodernize_shader(input: &str, fragment: bool) -> String {
    let mut out = input.replace("version 300 es", "").replace("texture(", "texture2D(");
    if fragment {
        out = Regex::new(r"\n\s*(in)\s+(vec\d|mat\d|float)").unwrap().replace_all(&out, "\nvarying $2").into_owned();
        if Regex::new(r"out_FragData_(\d+)").unwrap().is_match(&out) {
            out = format!("#extension GL_EXT_draw_buffers : enable\n{out}");
            out = Regex::new(r"layout\s+\(location\s*=\s*\d+\)\s*out\s+vec4\s+out_FragData_\d+;")
                .unwrap()
                .replace_all(&out, "")
                .into_owned();
            out = Regex::new(r"out_FragData_(\d+)").unwrap().replace_all(&out, "gl_FragData[$1]").into_owned();
        }
        out = Regex::new(r"layout\s+\(location\s*=\s*0\)\s*out\s+vec4\s+out_FragColor;")
            .unwrap()
            .replace_all(&out, "")
            .into_owned()
            .replace("out_FragColor", "gl_FragColor");
        if out.contains("gl_FragDepth") {
            out = format!("#extension GL_EXT_frag_depth : enable\n{}", out.replace("gl_FragDepth", "gl_FragDepthEXT"));
        }
        out=format!("#ifdef GL_OES_standard_derivatives\n#extension GL_OES_standard_derivatives : enable\n#endif\n#ifdef GL_EXT_shader_texture_lod\n#extension GL_EXT_shader_texture_lod : enable\n#endif\n{out}");
    } else {
        out = Regex::new(r"(in)\s+(vec\d|mat\d|float)").unwrap().replace_all(&out, "attribute $2").into_owned();
        out = Regex::new(r"(out)\s+(vec\d|mat\d|float)\s+([\w]+);")
            .unwrap()
            .replace_all(&out, "varying $2 $3;")
            .into_owned();
    }
    format!("#version 100\n{out}")
}
// Retain a fallible registry API for applications adding their own czm symbols.
pub fn parse_builtin_registry(json: &str) -> Result<HashMap<String, String>> {
    serde_json::from_str(json).map_err(|e| RuntimeError::new(e.to_string()))
}
