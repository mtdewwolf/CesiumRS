//! Shader rewrites applied by Cesium's Primitive before renderer assembly.

use crate::{core::RuntimeError, renderer::ShaderSource};
use regex::{Captures, Regex};
use std::sync::LazyLock;

const PICK_MAIN: &str = "out vec4 v_pickColor; \nvoid main() \n{ \n    czm_non_pick_main(); \n    v_pickColor = czm_batchTable_pickColor(batchId); \n}";
const SHOW_MAIN: &str =
    "void main() \n{ \n    czm_non_show_main(); \n    gl_Position *= czm_batchTable_show(batchId); \n}";

fn wrap_main(source: &str, name: &str, main: &str) -> String {
    format!("{}\n{main}", ShaderSource::replace_main(source, name))
}

pub(super) struct ShaderRewrites {
    pub offset: bool,
    pub show: bool,
    pub distance_display_condition: bool,
    pub color: bool,
    pub depth_fail_color: bool,
    pub compress_vertices: bool,
    pub scene_3d_only: bool,
    pub rtc: bool,
}

impl ShaderRewrites {
    pub fn apply(
        &self,
        mut vertex: String,
        fragment: String,
        depth_fail: bool,
    ) -> Result<(String, String), RuntimeError> {
        if self.offset && !depth_fail {
            vertex = append_offset(vertex);
        }
        if self.show {
            vertex = wrap_main(&vertex, "czm_non_show_main", SHOW_MAIN);
        }
        if self.distance_display_condition {
            vertex = append_distance_display_condition(&vertex, self.scene_3d_only);
        }
        vertex = wrap_main(&vertex, "czm_non_pick_main", PICK_MAIN);
        if self.color || self.depth_fail_color {
            static COLOR_INPUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"in\s+vec4\s+color;").unwrap());
            static COLOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bcolor\b").unwrap());
            if COLOR_INPUT.is_match(&vertex) {
                if depth_fail && !self.depth_fail_color {
                    return Err(RuntimeError::new("A depthFailColor per-instance attribute is required when using a depth fail appearance that uses a color attribute."));
                }
                let accessor = if depth_fail { "czm_batchTable_depthFailColor" } else { "czm_batchTable_color" };
                vertex = COLOR
                    .replace_all(&COLOR_INPUT.replace_all(&vertex, ""), format!("{accessor}(batchId)"))
                    .into_owned();
            }
        }
        if self.compress_vertices {
            vertex = modify_for_encoded_normals(vertex);
        }
        vertex = modify_position(vertex, self.scene_3d_only, self.rtc);
        let mut fragment = format!("in vec4 v_pickColor;\n{fragment}");
        if depth_fail {
            vertex = ShaderSource::replace_main(&vertex, "czm_non_depth_clamp_main");
            vertex +=
                "void main() {\n    czm_non_depth_clamp_main();\n    gl_Position = czm_depthClamp(gl_Position);}\n";
            fragment = ShaderSource::replace_main(&fragment, "czm_non_depth_clamp_main");
            fragment += concat!(
                "void main() {\n",
                "    czm_non_depth_clamp_main();\n",
                "    #if defined(LOG_DEPTH)\n",
                "        czm_writeLogDepth();\n",
                "    #else\n",
                "        czm_writeDepthClamp();\n",
                "    #endif\n",
                "}\n",
            );
        }
        Ok((vertex, fragment))
    }
}

/// Rewrites appearances that supply a per-vertex pickColor in place of v_pickColor.
pub(super) fn update_pick_color_attribute(source: &str) -> String {
    static INPUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"in\s+vec4\s+pickColor;").unwrap());
    static PICK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bpickColor\b").unwrap());
    PICK.replace_all(&INPUT.replace_all(source, ""), "czm_batchTable_pickColor(batchId)").into_owned()
}

fn append_offset(vertex: String) -> String {
    static INPUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"in\s+float\s+batchId;").unwrap());
    static POSITION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"vec4\s+([A-Za-z0-9_]+)\s+=\s+czm_computePosition\(\);").unwrap());
    POSITION
        .replace_all(
            &INPUT.replace_all(&vertex, "in float batchId;\nin float applyOffset;"),
            concat!(
                "vec4 $1 = czm_computePosition();\n",
                "    if (czm_sceneMode == czm_sceneMode3D)\n",
                "    {\n",
                "        $1 = $1 + vec4(czm_batchTable_offset(batchId) * applyOffset, 0.0);",
                "    }\n",
                "    else\n",
                "    {\n",
                "        $1 = $1 + vec4(czm_batchTable_offset2D(batchId) * applyOffset, 0.0);",
                "    }\n",
            ),
        )
        .into_owned()
}

fn modify_position(mut vertex: String, scene_3d_only: bool, rtc: bool) -> String {
    static POSITION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"in\s+vec[34]\s+(.*)3DHigh;").unwrap());
    static RTC_INPUT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"in\s+vec[34]\s+position3D(?:High|Low);").unwrap());
    static RTC_MULTIPLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"czm_modelViewRelativeToEye\s+\*\s+").unwrap());

    let mut forward = String::new();
    let mut attributes = String::new();
    let mut functions = String::new();

    for hit in POSITION.captures_iter(&vertex) {
        let name = &hit[1];
        let mut chars = name.chars();
        let Some(first) = chars.next() else { continue };
        let function = format!("vec4 czm_compute{}{}()", first.to_uppercase(), chars.as_str());
        if name != "position" {
            forward += &format!("{function};\n");
        }
        if rtc {
            forward += "uniform mat4 u_modifiedModelView;\n";
            attributes += "in vec4 position;\n";
            functions += &format!("{function}\n{{\n    return u_modifiedModelView * position;\n}}\n\n");
        } else if scene_3d_only {
            functions +=
                &format!("{function}\n{{\n    return czm_translateRelativeToEye({name}3DHigh, {name}3DLow);\n}}\n\n");
        } else {
            attributes += &format!("in vec3 {name}2DHigh;\nin vec3 {name}2DLow;\n");
            let three_d = format!("czm_translateRelativeToEye({name}3DHigh, {name}3DLow)");
            let two_d = format!("czm_translateRelativeToEye({name}2DHigh.zxy, {name}2DLow.zxy)");
            functions += &format!("{function}\n{{\n    vec4 p;\n{}    return p;\n}}\n\n", morph("p", &three_d, &two_d));
        }
    }
    if rtc && !functions.is_empty() {
        vertex = RTC_INPUT.replace_all(&vertex, "").into_owned();
        vertex = RTC_MULTIPLY.replace_all(&vertex, "").into_owned();
        vertex = vertex.replace("czm_modelViewProjectionRelativeToEye", "czm_projection");
    }
    [forward, attributes, vertex, functions].join("\n")
}

fn morph(value: &str, three_d: &str, two_d: &str) -> String {
    format!(
        "    if (czm_morphTime == 1.0)\n    {{\n        {value} = {three_d};\n    }}\n    else if (czm_morphTime == 0.0)\n    {{\n        {value} = {two_d};\n    }}\n    else\n    {{\n        {value} = czm_columbusViewMorph(\n                {two_d},\n                {three_d},\n                czm_morphTime);\n    }}\n"
    )
}

fn append_distance_display_condition(vertex: &str, scene_3d_only: bool) -> String {
    let mut main = String::from(concat!(
        "void main() \n",
        "{ \n",
        "    czm_non_distanceDisplayCondition_main(); \n",
        "    vec2 distanceDisplayCondition = czm_batchTable_distanceDisplayCondition(batchId);\n",
        "    vec3 boundingSphereCenter3DHigh = czm_batchTable_boundingSphereCenter3DHigh(batchId);\n",
        "    vec3 boundingSphereCenter3DLow = czm_batchTable_boundingSphereCenter3DLow(batchId);\n",
        "    float boundingSphereRadius = czm_batchTable_boundingSphereRadius(batchId);\n",
    ));
    let three_d = "czm_translateRelativeToEye(boundingSphereCenter3DHigh, boundingSphereCenter3DLow)";
    if scene_3d_only {
        main += &format!("    vec4 centerRTE = {three_d};\n");
    } else {
        main += concat!(
            "    vec3 boundingSphereCenter2DHigh = czm_batchTable_boundingSphereCenter2DHigh(batchId);\n",
            "    vec3 boundingSphereCenter2DLow = czm_batchTable_boundingSphereCenter2DLow(batchId);\n",
            "    vec4 centerRTE;\n",
        );
        main += &morph(
            "centerRTE",
            three_d,
            "czm_translateRelativeToEye(boundingSphereCenter2DHigh.zxy, boundingSphereCenter2DLow.zxy)",
        );
    }
    main += concat!(
        "    float radiusSq = boundingSphereRadius * boundingSphereRadius; \n",
        "    float distanceSq; \n",
        "    if (czm_sceneMode == czm_sceneMode2D) \n",
        "    { \n",
        "        distanceSq = czm_eyeHeight2D.y - radiusSq; \n",
        "    } \n",
        "    else \n",
        "    { \n",
        "        distanceSq = dot(centerRTE.xyz, centerRTE.xyz) - radiusSq; \n",
        "    } \n",
        "    distanceSq = max(distanceSq, 0.0); \n",
        "    float nearSq = distanceDisplayCondition.x * distanceDisplayCondition.x; \n",
        "    float farSq = distanceDisplayCondition.y * distanceDisplayCondition.y; \n",
        "    float show = (distanceSq >= nearSq && distanceSq <= farSq) ? 1.0 : 0.0; \n",
        "    gl_Position *= show; \n",
        "}",
    );
    wrap_main(vertex, "czm_non_distanceDisplayCondition_main", &main)
}

fn modify_for_encoded_normals(mut vertex: String) -> String {
    static INPUT: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"in\s+(vec[23])\s+(normal|st|tangent|bitangent);").unwrap());
    let mut present = [false; 4];
    let names = ["st", "normal", "tangent", "bitangent"];
    for hit in INPUT.captures_iter(&vertex) {
        let i = names.iter().position(|name| *name == &hit[2]).unwrap();
        present[i] |= &hit[1] == if i == 0 { "vec2" } else { "vec3" };
    }
    let [st, normal, tangent, bitangent] = present;
    if !normal && !st {
        return vertex;
    }

    let components = 1 + u32::from(st && normal) + u32::from(tangent || bitangent);
    let declaration = match components {
        1 => "in float compressedAttributes;",
        2 => "in vec2 compressedAttributes;",
        _ => "in vec3 compressedAttributes;",
    };
    let mut globals = String::new();
    let mut decode = String::new();
    for (i, name) in names.iter().enumerate() {
        if present[i] {
            let ty = if i == 0 { "vec2" } else { "vec3" };
            globals += &format!("{ty} {name};\n");
        }
    }
    if st {
        let component = if components > 1 { ".x" } else { "" };
        decode += &format!("    st = czm_decompressTextureCoordinates(compressedAttributes{component});\n");
    }
    if normal && tangent && bitangent {
        let swizzle = if st { "yz" } else { "xy" };
        decode += &format!("    czm_octDecode(compressedAttributes.{swizzle}, normal, tangent, bitangent);\n");
    } else {
        let normal_component = if components == 1 {
            ""
        } else if st {
            ".y"
        } else {
            ".x"
        };
        let other_component = if st && normal { ".z" } else { ".y" };
        for (i, name) in names.iter().enumerate().skip(1) {
            if present[i] {
                let component = if i == 1 { normal_component } else { other_component };
                decode += &format!("    {name} = czm_octDecode(compressedAttributes{component});\n");
            }
        }
    }
    vertex = INPUT
        .replace_all(&vertex, |hit: &Captures<'_>| {
            let matching = &hit[1] == if &hit[2] == "st" { "vec2" } else { "vec3" };
            if matching {
                String::new()
            } else {
                hit[0].to_owned()
            }
        })
        .into_owned();
    vertex = ShaderSource::replace_main(&vertex, "czm_non_compressed_main");
    format!("{declaration}\n{globals}\n{vertex}\nvoid main() \n{{ \n{decode}    czm_non_compressed_main(); \n}}")
}
