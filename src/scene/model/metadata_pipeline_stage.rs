//! Structural metadata shader structs and initialization for property
//! attributes, textures and GPU-packed tables. Usage sets are supplied by the
//! custom-shader / point-cloud styling stages to prune table reads.
use super::{model_utility, PrimitiveRenderResources};
use crate::{
    core::{Matrix3, RuntimeError},
    renderer::{Gl, ShaderBuilder, ShaderDestination as D, Texture, UniformValue},
    scene::{
        metadata_value::MetadataValue, model_components as mc, property_table::MetadataId, MetadataClassProperty,
        MetadataType, StructuralMetadata,
    },
    shaders,
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};
type Result<T> = std::result::Result<T, RuntimeError>;
const SIGNATURE:&str="void initializeMetadata(FeatureIds featureIds, out Metadata metadata, out MetadataClass metadataClass, out MetadataStatistics metadataStatistics, ProcessedAttributes attributes)";
const CLASS_FIELDS: [(&str, &str); 4] =
    [("noData", "noData"), ("default", "defaultValue"), ("min", "minValue"), ("max", "maxValue")];
const STAT_FIELDS: [(&str, &str); 7] = [
    ("min", "minValue"),
    ("max", "maxValue"),
    ("mean", "mean"),
    ("median", "median"),
    ("standardDeviation", "standardDeviation"),
    ("variance", "variance"),
    ("sum", "sum"),
];
#[derive(Clone, Debug, Default)]
pub struct MetadataUsage {
    pub vertex: HashSet<String>,
    pub fragment: HashSet<String>,
}
impl MetadataUsage {
    pub fn destination(&self, name: &str) -> D {
        D(u32::from(self.vertex.contains(name)) | (u32::from(self.fragment.contains(name)) * 2))
    }
    pub fn add_point_cloud_variables(&mut self, color: &[String], show: &[String], point_size: &[String]) {
        for key in color.iter().chain(show) {
            self.vertex.insert(key.clone());
            self.fragment.insert(key.clone());
        }
        self.vertex.extend(point_size.iter().cloned());
    }
}
pub struct MetadataStageOptions<'a> {
    pub webgl2: bool,
    pub statistics: Option<&'a Value>,
    pub usage: &'a MetadataUsage,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataStageWarning {
    DuplicateTable(usize),
    PropertyTablesRequireWebGl2,
}
enum Source<'a, G: Gl> {
    Attribute(String),
    Texture(&'a mc::TextureReader<G>),
    Table { id: String, feature: String, row: usize, texture: Option<Rc<Texture<G>>> },
}
struct Info<'a, G: Gl> {
    name: String,
    glsl: String,
    class: &'a MetadataClassProperty,
    statistics: Option<&'a Value>,
    destination: D,
    transform: Option<(&'a MetadataValue, &'a MetadataValue)>,
    source: Source<'a, G>,
}
fn error(e: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new(e.to_string())
}
fn id_string(id: &MetadataId) -> String {
    match id {
        MetadataId::Index(i) => i.to_string(),
        MetadataId::Key(k) => k.clone(),
    }
}
fn stats<'a>(statistics: Option<&'a Value>, class: &str, name: &str) -> Option<&'a Value> {
    statistics?.get("classes")?.get(class)?.get("properties")?.get(name).filter(|v| !v.is_null())
}
fn declare_base(shader: &mut ShaderBuilder) {
    for (suffix, dest) in [("VS", D::VERTEX), ("FS", D::FRAGMENT)] {
        for name in ["Metadata", "MetadataClass", "MetadataStatistics"] {
            shader.add_struct(&format!("{name}{suffix}"), name, dest);
        }
        shader.add_function(&format!("initializeMetadata{suffix}"), SIGNATURE, dest);
    }
    shader.add_function("setMetadataVaryings", "void setMetadataVaryings()", D::VERTEX);
    shader.add_vertex_lines(&[shaders::model::METADATA_STAGE_VS.source]);
    shader.add_fragment_lines(&[shaders::model::METADATA_STAGE_FS.source]);
}
fn float_type(ty: &str) -> &str {
    match ty {
        "int" | "uint" => "float",
        "ivec2" | "uvec2" => "vec2",
        "ivec3" | "uvec3" => "vec3",
        "ivec4" | "uvec4" => "vec4",
        _ => ty,
    }
}
fn statistics_field_type<'a>(field: &str, ty: &'a str) -> &'a str {
    if ["mean", "standardDeviation", "variance"].contains(&field) {
        float_type(ty)
    } else {
        ty
    }
}
fn declare_types<G: Gl>(shader: &mut ShaderBuilder, infos: &[Info<'_, G>]) -> Result<()> {
    let mut types = vec![];
    let mut statistics_types = vec![];
    for info in infos {
        if !types.contains(&info.glsl.as_str()) {
            types.push(info.glsl.as_str());
        }
        if info.statistics.is_some()
            && info.class.metadata_type() != MetadataType::Enum
            && !statistics_types.contains(&info.glsl.as_str())
        {
            statistics_types.push(info.glsl.as_str());
        }
    }
    for ty in types {
        let name = format!("{ty}MetadataClass");
        shader.add_struct(&name, &name, D::BOTH);
        for (_, field) in CLASS_FIELDS {
            shader.add_struct_field(&name, ty, field)?;
        }
    }
    for ty in statistics_types {
        let name = format!("{ty}MetadataStatistics");
        shader.add_struct(&name, &name, D::BOTH);
        for (spec, field) in STAT_FIELDS {
            let field_type = statistics_field_type(spec, ty);
            shader.add_struct_field(&name, field_type, field)?;
        }
    }
    Ok(())
}
fn number_values(value: &MetadataValue) -> Vec<f64> {
    match value {
        MetadataValue::Array(a) => a.iter().flat_map(number_values).collect(),
        MetadataValue::Cartesian2(v) => vec![v.x, v.y],
        MetadataValue::Cartesian3(v) => vec![v.x, v.y, v.z],
        MetadataValue::Cartesian4(v) => vec![v.x, v.y, v.z, v.w],
        MetadataValue::Matrix2(v) => v.0.to_vec(),
        MetadataValue::Matrix3(v) => v.0.to_vec(),
        MetadataValue::Matrix4(v) => v.0.to_vec(),
        v => vec![v.to_number()],
    }
}
fn uniform(value: &MetadataValue, ty: &str) -> UniformValue {
    let values = number_values(value);
    if let Some(dimension) = ty.strip_prefix("mat") {
        UniformValue::Matrix {
            dimension: dimension.parse().unwrap_or(4),
            values: values.iter().map(|v| *v as f32).collect(),
        }
    } else if ty == "int" || ty.starts_with("ivec") {
        UniformValue::Int(values.iter().map(|v| *v as i32).collect())
    } else if ty == "uint" || ty.starts_with("uvec") {
        UniformValue::UIntArray { components: values.len() as u32, values: values.iter().map(|v| *v as u32).collect() }
    } else {
        UniformValue::Float(values.iter().map(|v| *v as f32).collect())
    }
}
fn transform<G: Gl>(r: &mut PrimitiveRenderResources<G>, info: &Info<'_, G>, expression: String) -> String {
    let Some((offset, scale)) = info.transform else { return expression };
    for (suffix, value) in [("offset", offset), ("scale", scale)] {
        let name = format!("u_{}_{suffix}", info.name);
        r.shader_builder.add_uniform(&info.glsl, &name, info.destination);
        let v = uniform(value, &info.glsl);
        r.uniform_map.insert(name, Rc::new(move || v.clone()));
    }
    if info.glsl == "int" || info.glsl == "uint" || info.glsl.starts_with("ivec") || info.glsl.starts_with("uvec") {
        // czm_valueTransform only overloads float/vector/matrix types. Keep
        // integer transforms in their declared type, including unsigned bits.
        format!("(u_{}_offset + u_{}_scale * ({expression}))", info.name, info.name)
    } else {
        format!("czm_valueTransform(u_{}_offset, u_{}_scale, {expression})", info.name, info.name)
    }
}
fn append(shader: &mut ShaderBuilder, dest: D, lines: &[String]) -> Result<()> {
    if dest.includes_vertex_shader() {
        shader.add_function_lines("initializeMetadataVS", lines)?;
    }
    if dest.includes_fragment_shader() {
        shader.add_function_lines("initializeMetadataFS", lines)?;
    }
    Ok(())
}
fn metadata_fields(shader: &mut ShaderBuilder, dest: D, ty: &str, name: &str) -> Result<()> {
    if dest.includes_vertex_shader() {
        shader.add_struct_field("MetadataVS", ty, name)?;
    }
    if dest.includes_fragment_shader() {
        shader.add_struct_field("MetadataFS", ty, name)?;
    }
    Ok(())
}
fn class_and_statistics<G: Gl>(shader: &mut ShaderBuilder, info: &Info<'_, G>) -> Result<()> {
    let c = info.class;
    let fields = [c.no_data(), c.default(), c.min(), c.max()];
    let lines = CLASS_FIELDS
        .iter()
        .zip(fields)
        .filter_map(|((_, field), v)| {
            v.map(|v| format!("metadataClass.{}.{field} = {}({});", info.name, info.glsl, v.js_string()))
        })
        .collect::<Vec<_>>();
    shader.add_struct_field("MetadataClassFS", &format!("{}MetadataClass", info.glsl), &info.name)?;
    shader.add_function_lines("initializeMetadataFS", &lines)?;
    if info.destination.includes_vertex_shader() {
        shader.add_struct_field("MetadataClassVS", &format!("{}MetadataClass", info.glsl), &info.name)?;
        shader.add_function_lines("initializeMetadataVS", &lines)?;
    }
    if let Some(statistics) = info.statistics.filter(|_| c.metadata_type() != MetadataType::Enum) {
        let lines = STAT_FIELDS
            .iter()
            .filter_map(|(spec, field)| {
                statistics.get(*spec).filter(|v| !v.is_null()).map(|v| {
                    format!(
                        "metadataStatistics.{}.{field} = {}({});",
                        info.name,
                        statistics_field_type(spec, &info.glsl),
                        MetadataValue::from_json(v).js_string()
                    )
                })
            })
            .collect::<Vec<_>>();
        shader.add_struct_field("MetadataStatisticsFS", &format!("{}MetadataStatistics", info.glsl), &info.name)?;
        shader.add_function_lines("initializeMetadataFS", &lines)?;
        if info.destination.includes_vertex_shader() {
            shader.add_struct_field("MetadataStatisticsVS", &format!("{}MetadataStatistics", info.glsl), &info.name)?;
            shader.add_function_lines("initializeMetadataVS", &lines)?;
        }
    }
    Ok(())
}
fn property<G: Gl + 'static>(
    r: &mut PrimitiveRenderResources<G>,
    info: &Info<'_, G>,
    webgl2: bool,
    warnings: &mut Vec<MetadataStageWarning>,
) -> Result<()> {
    let mut lines = vec![];
    let expression = match &info.source {
        Source::Attribute(variable) => Some(format!("attributes.{variable}")),
        Source::Texture(reader) => {
            let index = reader.index.map(crate::core::math::js_number_to_string).unwrap_or_else(|| "undefined".into());
            let name = format!("u_propertyTexture_{index}");
            if !r.texture_uniform_map.contains_key(&name) {
                r.shader_builder.add_uniform("sampler2D", &name, D::FRAGMENT);
                let texture = reader.texture.clone();
                r.texture_uniform_map.insert(name.clone(), Rc::new(move || texture.clone()));
            }
            metadata_fields(&mut r.shader_builder, D::FRAGMENT, &info.glsl, &info.name)?;
            let coord = format!("attributes.texCoord_{}", crate::core::math::js_number_to_string(reader.tex_coord));
            let coord = if let Some(matrix) = reader.transform.filter(|m| *m != Matrix3::IDENTITY) {
                // Share samplers, but only share transforms when their matrix
                // values match. Different property readers can have different UVs.
                let value = UniformValue::Matrix { dimension: 3, values: matrix.0.iter().map(|v| *v as f32).collect() };
                let base = format!("{name}Transform");
                let uniform = match r.uniform_map.get(&base) {
                    Some(provider) if provider() != value => format!("{base}_{}", info.name),
                    _ => base,
                };
                if !r.uniform_map.contains_key(&uniform) {
                    r.shader_builder.add_uniform("mat3", &uniform, D::FRAGMENT);
                    r.uniform_map.insert(uniform.clone(), Rc::new(move || value.clone()));
                }
                format!("vec2({uniform} * vec3({coord}, 1.0))")
            } else {
                coord
            };
            let channels = reader.channels.as_deref().unwrap_or("r");
            let sample = format!("texture({name}, {coord}).{channels}");
            Some(if webgl2 {
                info.class.unpack_texture_in_shader(&sample, channels, &info.name, &mut lines).map_err(error)?
            } else {
                info.class.unpack_texture_in_shader_webgl1(&sample)
            })
        }
        Source::Table { id, feature, row, texture } => {
            if !webgl2 {
                if !warnings.contains(&MetadataStageWarning::PropertyTablesRequireWebGl2) {
                    warnings.push(MetadataStageWarning::PropertyTablesRequireWebGl2);
                }
                None
            } else if feature.is_empty() || texture.is_none() {
                None
            } else {
                let name = format!("u_propertyTableTexture_{id}");
                if !r.texture_uniform_map.contains_key(&name) {
                    r.shader_builder.add_uniform("sampler2D", &name, D::BOTH);
                    let texture = texture.clone();
                    r.texture_uniform_map.insert(name.clone(), Rc::new(move || texture.clone()));
                }
                metadata_fields(&mut r.shader_builder, info.destination, &info.glsl, &info.name)?;
                Some(
                    info.class
                        .unpack_texture_in_shader(
                            &format!("texelFetch({name}, ivec2(featureIds.{feature}, {row}), 0)"),
                            "rgba",
                            &info.name,
                            &mut lines,
                        )
                        .map_err(error)?,
                )
            }
        }
    };
    if let Some(expression) = expression {
        if matches!(&info.source, Source::Attribute(_)) {
            metadata_fields(&mut r.shader_builder, info.destination, &info.glsl, &info.name)?;
        }
        let expression = transform(r, info, expression);
        lines.push(format!("metadata.{} = {expression};", info.name));
        append(&mut r.shader_builder, info.destination, &lines)?;
    }
    class_and_statistics(&mut r.shader_builder, info)
}

pub fn process<G: Gl + 'static>(
    resources: &mut PrimitiveRenderResources<G>,
    primitive: &mc::Primitive<G>,
    node: &mc::Node<G>,
    metadata: Option<&StructuralMetadata<G>>,
    options: MetadataStageOptions<'_>,
) -> Result<Vec<MetadataStageWarning>> {
    let mut warnings = vec![];
    let mut infos = vec![];
    if let Some(metadata) = metadata {
        for &id in &primitive.property_attribute_ids {
            let attribute =
                metadata.get_property_attribute(id).ok_or_else(|| error(format!("Missing property attribute {id}")))?;
            for (name, p) in attribute.properties().iter() {
                let attr_name =
                    p.attribute().and_then(Value::as_str).ok_or_else(|| error("Missing metadata attribute name"))?;
                let attr = model_utility::get_attribute_by_name(&primitive.attributes, attr_name)
                    .ok_or_else(|| error(format!("Missing metadata attribute {attr_name}")))?;
                let attr_info = model_utility::get_attribute_info(attr)?;
                infos.push(Info {
                    name: model_utility::sanitize_glsl_identifier(name),
                    glsl: attr_info.glsl_type.into(),
                    class: p.class_property(),
                    statistics: stats(options.statistics, &attribute.class().id, name),
                    destination: D::BOTH,
                    transform: p.has_value_transform().then(|| (p.offset(), p.scale())),
                    source: Source::Attribute(attr_info.variable_name),
                });
            }
        }
        for &id in &primitive.property_texture_ids {
            let texture =
                metadata.get_property_texture(id).ok_or_else(|| error(format!("Missing property texture {id}")))?;
            for (name, p) in texture.properties().iter() {
                let class = p.class_property();
                let reader = p.texture_reader();
                if !class.is_gpu_compatible(reader.channels.as_deref().unwrap_or("r").len() as f64).map_err(error)? {
                    continue;
                }
                infos.push(Info {
                    name: model_utility::sanitize_glsl_identifier(name),
                    glsl: if options.webgl2 {
                        class.get_glsl_type().map_err(error)?
                    } else {
                        class.get_glsl_type_webgl1()
                    }
                    .unwrap_or("undefined")
                    .into(),
                    class,
                    statistics: stats(options.statistics, &texture.class().id, name),
                    destination: D::FRAGMENT,
                    transform: p.has_value_transform().then(|| (p.offset(), p.scale())),
                    source: Source::Texture(reader),
                });
            }
        }
        let mut mapping = HashMap::new();
        for feature in primitive.feature_ids.iter().chain(node.instances.iter().flat_map(|i| i.feature_ids.iter())) {
            if let Some(id) = feature.property_table_id() {
                let dest = if matches!(feature, mc::FeatureId::Texture(_)) { D::FRAGMENT } else { D::BOTH };
                if mapping
                    .insert(id.to_string(), (feature.positional_label().unwrap_or("").to_string(), dest))
                    .is_some()
                {
                    warnings.push(MetadataStageWarning::DuplicateTable(id));
                }
            }
        }
        for table in metadata.property_tables() {
            let (Some(class), Some(id)) = (table.class(), table.id()) else { continue };
            let id = id_string(id);
            let Some((feature, dest)) = mapping.get(&id) else { continue };
            let mut row = 0;
            for (name, cp) in class.properties.iter() {
                if !cp.is_gpu_compatible(4.).map_err(error)? {
                    continue;
                }
                let destination = D::intersection(&[options.usage.destination(name), *dest]).unwrap_or(D::NONE);
                if destination != D::NONE {
                    let p = table.properties().and_then(|p| p.get(name));
                    let transform = p.map(|p| (p.has_value_transform(), p.offset(), p.scale())).unwrap_or((
                        cp.has_value_transform(),
                        cp.offset(),
                        cp.scale(),
                    ));
                    let texture = table.texture().and_then(|t| t.get::<Rc<Texture<G>>>()).cloned();
                    infos.push(Info {
                        name: model_utility::sanitize_glsl_identifier(name),
                        glsl: cp.get_glsl_type().map_err(error)?.unwrap_or("undefined").into(),
                        class: cp,
                        statistics: stats(options.statistics, &class.id, name),
                        destination,
                        transform: transform.0.then_some((transform.1, transform.2)),
                        source: Source::Table { id: id.clone(), feature: feature.clone(), row, texture },
                    });
                }
                row += 1;
            }
        }
    }
    declare_types(&mut resources.shader_builder, &infos)?;
    declare_base(&mut resources.shader_builder);
    for info in &infos {
        property(resources, info, options.webgl2, &mut warnings)?;
    }
    Ok(warnings)
}
