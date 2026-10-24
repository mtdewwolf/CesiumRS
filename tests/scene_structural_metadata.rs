//! Property textures, StructuralMetadata, getMetadataProperty and the three
//! metadata parsers against CesiumJS
//! (`tools/cesium/fixtures/scene-structural-metadata.mjs`).
//!
//! Textures are named stand-ins. Property table textures are compared by
//! what is uploaded: the fixture stubs `Texture.create`, and the GL host
//! here records each `texImage2D`.
mod common;
use cesium_rs::core::{PixelDatatype, PixelFormat, RuntimeError, TypedArray};
use cesium_rs::renderer::{Context, Gl, GlValue, Sampler, Texture, TextureOptions};
use cesium_rs::scene::model_components::{Attribute, TextureReader};
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Map, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// Allocates objects, answers `MAX_TEXTURE_SIZE` and records uploaded pixels.
#[derive(Clone, Default)]
struct Host {
    next: Rc<Cell<u32>>,
    maximum_texture_size: Rc<Cell<f64>>,
    uploads: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        if name == "texImage2D" {
            if let Some(GlValue::Bytes(bytes) | GlValue::Pixels { bytes, .. }) = args.last() {
                self.uploads.borrow_mut().push(bytes.clone());
            }
        }
        // ALIASED_LINE_WIDTH_RANGE, ALIASED_POINT_SIZE_RANGE and MAX_VIEWPORT_DIMS are pairs.
        let pair = matches!(args.first(), Some(GlValue::Number(p)) if [0x846e, 0x846d, 0x0d3a].contains(&(*p as u32)));
        Ok(match name {
            "getParameter" if pair => GlValue::Numbers(vec![1., 8192.]),
            "getParameter" => GlValue::Number(self.maximum_texture_size.get()),
            "getExtension" => GlValue::Object(0),
            _ => GlValue::Null,
        })
    }
}

/// Stand-in textures by glTF index, and their names.
type NamedTextures = (GltfTextures<Host>, Vec<(String, Rc<Texture<Host>>)>);

struct Gpu {
    host: Host,
    context: Context<Host>,
}

impl Gpu {
    fn new(maximum_texture_size: f64) -> Self {
        let host = Host::default();
        host.maximum_texture_size.set(maximum_texture_size);
        Self { context: Context::new_webgl2(host.clone()), host }
    }

    /// Stand-in textures by glTF index, named `t<index>`.
    fn textures(&self, names: &Value) -> NamedTextures {
        let mut textures = GltfTextures::new();
        let mut named = Vec::new();
        for i in names.as_array().unwrap() {
            let texture = Rc::new(Texture::new(&self.context, TextureOptions::rgba(1, 1), None).unwrap());
            textures.insert(js_key(i), texture.clone());
            named.push((format!("t{}", js_key(i)), texture));
        }
        self.host.uploads.borrow_mut().clear();
        (textures, named)
    }
}

fn js_key(v: &Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => panic!("texture key"),
    }
}

// ---- values, as scene_metadata_tables.rs tags them ----

fn tag(value: &MetadataValue) -> Value {
    let all = |c: &[f64]| c.iter().map(|&x| num(x)).collect::<Vec<_>>();
    match value {
        MetadataValue::Null => json!({ "$null": true }),
        MetadataValue::Boolean(b) => json!(b),
        MetadataValue::Number(x) => num(*x),
        MetadataValue::BigInt(b) => json!({ "$bigint": b.to_string() }),
        MetadataValue::String(s) => json!(s),
        MetadataValue::Array(items) => Value::Array(items.iter().map(tag).collect()),
        MetadataValue::Cartesian2(c) => json!({ "$Cartesian2": all(&[c.x, c.y]) }),
        MetadataValue::Cartesian3(c) => json!({ "$Cartesian3": all(&[c.x, c.y, c.z]) }),
        MetadataValue::Cartesian4(c) => json!({ "$Cartesian4": all(&[c.x, c.y, c.z, c.w]) }),
        MetadataValue::Matrix2(m) => json!({ "$Matrix2": all(&m.0) }),
        MetadataValue::Matrix3(m) => json!({ "$Matrix3": all(&m.0) }),
        MetadataValue::Matrix4(m) => json!({ "$Matrix4": all(&m.0) }),
        MetadataValue::Object(v) => v.clone(),
    }
}

fn val(value: Option<&MetadataValue>) -> Value {
    value.map_or(Value::Null, tag)
}

fn kind(e: &MetadataError) -> &'static str {
    match e {
        MetadataError::Developer(_) => "DeveloperError",
        MetadataError::Type(_) => "TypeError",
        MetadataError::Range(_) => "RangeError",
        MetadataError::Runtime(_) => "RuntimeError",
        MetadataError::Syntax(_) => "SyntaxError",
    }
}

fn thrown(e: MetadataError) -> Option<Out> {
    Some(Out::Err(e.to_string()))
}

fn opt(v: Option<&Value>) -> Value {
    v.cloned().unwrap_or(Value::Null)
}

/// `{ id: { buffer, byteOffset } }` as owned buffers.
fn buffers(views: &Value) -> Vec<(String, Vec<u8>, usize)> {
    views
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(id, v)| {
                    (
                        id.clone(),
                        nums(&v["buffer"]).into_iter().map(|b| b as u8).collect(),
                        u(&v["byteOffset"]) as usize,
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn byte_views(buffers: &[(String, Vec<u8>, usize)]) -> JsMap<ByteView<'_>> {
    buffers.iter().map(|(id, b, o)| (id.clone(), ByteView { buffer: b, byte_offset: *o })).collect()
}

// ---- summaries, as the fixture writes them ----

struct Names<'a>(&'a [(String, Rc<Texture<Host>>)]);

impl Names<'_> {
    fn of(&self, texture: Option<&Rc<Texture<Host>>>) -> Value {
        texture.and_then(|t| self.0.iter().find(|(_, x)| Rc::ptr_eq(x, t))).map_or(Value::Null, |(n, _)| json!(n))
    }
}

fn reader_info(r: &TextureReader<Host>, names: &Names) -> Value {
    json!([
        r.index.map(num),
        names.of(r.texture.as_ref()),
        num(r.tex_coord),
        r.transform.map(|m| m.0.iter().map(|&x| num(x)).collect::<Vec<_>>()),
        r.scale.filter(|s| !s.is_nan()).map(num),
        r.channels,
    ])
}

fn texture_property(p: &PropertyTextureProperty<Host>, names: &Names) -> Value {
    json!([
        reader_info(p.texture_reader(), names),
        p.has_value_transform(),
        val(Some(p.offset())),
        val(Some(p.scale())),
        p.class_property().id(),
        opt(p.extras()),
        opt(p.extensions()),
    ])
}

fn property_texture(t: &PropertyTexture<Host>, names: &Names) -> Vec<Value> {
    let properties: Map<String, Value> =
        t.properties().iter().map(|(k, p)| (k.to_string(), texture_property(p, names))).collect();
    vec![
        json!(t.id()),
        json!(t.name()),
        json!(t.class().id),
        Value::Object(properties),
        opt(t.extras()),
        opt(t.extensions()),
    ]
}

fn attribute_property(p: &PropertyAttributeProperty) -> Value {
    json!([
        opt(p.attribute()),
        p.has_value_transform(),
        val(Some(p.offset())),
        val(Some(p.scale())),
        p.class_property().id()
    ])
}

fn property_attribute(a: &PropertyAttribute) -> Value {
    let properties: Map<String, Value> =
        a.properties().iter().map(|(k, p)| (k.to_string(), attribute_property(p))).collect();
    json!([a.id(), a.name(), a.class().id, properties])
}

fn table_texture(t: &PropertyTable, uploads: &mut std::vec::IntoIter<Vec<u8>>) -> Value {
    let Some(resource) = t.texture() else { return Value::Null };
    let texture = resource.get::<Rc<Texture<Host>>>().expect("an Rc<Texture>");
    let sampler = texture.sampler();
    json!({
        "width": texture.width(), "height": texture.height(), "bytes": uploads.next().expect("an upload"),
        "flipY": texture.flip_y(), "pixelFormat": texture.pixel_format() == PixelFormat::Rgba,
        "pixelDatatype": texture.pixel_datatype() == PixelDatatype::UnsignedByte, "nearest": sampler == Sampler::NEAREST,
    })
}

fn attempt(r: Result<Option<MetadataValue>, MetadataError>) -> Value {
    match r {
        Ok(v) => json!({ "ok": val(v.as_ref()) }),
        Err(e) => json!({ "throws": kind(&e) }),
    }
}

fn property_table(t: &mut PropertyTable, uploads: &mut std::vec::IntoIter<Vec<u8>>) -> Result<Value, MetadataError> {
    let mut rows = Vec::new();
    for i in 0..t.count() {
        let mut row = Map::new();
        for id in t.get_property_ids(i)? {
            let value = attempt(t.get_property(i, &id));
            row.insert(id, value);
        }
        rows.push(Value::Object(row));
    }
    Ok(json!({
        "id": t.id(), "name": t.name(), "count": t.count(), "byteLength": t.byte_length(), "class": t.class().map(|c| c.id.clone()),
        "properties": t.properties().map(|p| p.keys().collect::<Vec<_>>()), "extras": opt(t.extras()), "extensions": opt(t.extensions()),
        "texture": table_texture(t, uploads), "rows": rows,
    }))
}

fn class_info(c: &MetadataClass) -> Value {
    c.properties
        .iter()
        .map(|(id, p)| json!([id, p.metadata_type().as_str(), p.component_type().map(|c| c.as_str()), p.name()]))
        .collect()
}

fn summary(m: &mut StructuralMetadata<Host>, names: &Names, uploads: Vec<Vec<u8>>) -> Result<Value, MetadataError> {
    let mut uploads = uploads.into_iter();
    let schema: Map<String, Value> = m.schema().classes.iter().map(|(id, c)| (id.to_string(), class_info(c))).collect();
    let mut tables = Vec::new();
    for i in 0..m.property_tables().len() {
        tables.push(property_table(m.get_property_table_mut(i).unwrap(), &mut uploads)?);
    }
    Ok(json!({
        "schema": schema,
        "propertyTableCount": m.property_table_count(),
        "propertyTablesByteLength": m.property_tables_byte_length(),
        "propertyTables": tables,
        "propertyTextures": m.property_textures().iter().map(|t| property_texture(t, names)).collect::<Vec<_>>(),
        "propertyAttributes": m.property_attributes().iter().map(property_attribute).collect::<Vec<_>>(),
        "statistics": opt(m.statistics()), "extras": opt(m.extras()), "extensions": opt(m.extensions()),
    }))
}

fn schema(json: &Value) -> Arc<MetadataSchema> {
    Arc::new(MetadataSchema::from_json(json).unwrap())
}

fn table_schema() -> Arc<MetadataSchema> {
    schema(&load("scene", "MetadataTableProperty")[0].a[0])
}

thread_local! {
    /// One-time warnings already logged, as CesiumJS's `oneTimeWarning`
    /// remembers them for the process; cases replay in fixture order.
    static LOGGED: RefCell<std::collections::HashSet<&'static str>> = RefCell::default();
}

/// The messages CesiumJS writes to the console: a one-time warning only the
/// first time its id is seen.
fn messages(warnings: &[StructuralMetadataWarning]) -> Vec<String> {
    warnings
        .iter()
        .filter(|w| w.id().is_none_or(|id| LOGGED.with(|l| l.borrow_mut().insert(id))))
        .map(StructuralMetadataWarning::message)
        .collect()
}

#[test]
fn property_texture_property() {
    let gpu = Gpu::new(16384.);
    let class = table_schema().classes.get("t").unwrap().clone();
    run(
        "scene",
        "PropertyTextureProperty",
        |_| EXACT,
        |_, a| {
            let (textures, named) = gpu.textures(&a[2]);
            let class_property = class.properties.get(s(&a[0])).unwrap().clone();
            match PropertyTextureProperty::new(&a[1], class_property, &textures) {
                Ok(p) => ok(texture_property(&p, &Names(&named))),
                Err(e) => thrown(e),
            }
        },
    );
}

#[test]
fn property_textures() {
    let gpu = Gpu::new(16384.);
    let class = table_schema().classes.get("t").unwrap().clone();
    run(
        "scene",
        "PropertyTexture",
        |_| EXACT,
        |_, a| {
            let (textures, named) = gpu.textures(&json!([0, 2]));
            let name = a[1].as_str().map(str::to_string);
            let id = a[2].as_u64().map(|i| MetadataId::Index(i as usize));
            match PropertyTexture::new(&a[0], class.clone(), &textures, name, id) {
                Ok(t) => {
                    let mut out = property_texture(&t, &Names(&named));
                    out.push(json!(t.get_property("f32").map(|_| "f32")));
                    out.push(json!(t.get_property("nope").map(|_| "nope")));
                    ok(Value::Array(out))
                }
                Err(e) => thrown(e),
            }
        },
    );
}

#[test]
fn parse_structural_metadata_cases() {
    run(
        "scene",
        "parseStructuralMetadata",
        |_| EXACT,
        |_, a| {
            let gpu = Gpu::new(n(&a[4]));
            let (textures, named) = gpu.textures(&a[3]);
            let bufs = buffers(&a[2]);
            let views = byte_views(&bufs);
            let options = ParseStructuralMetadataOptions {
                extension: &a[1],
                schema: schema(&a[0]),
                buffer_views: &views,
                textures: &textures,
                context: Some(&gpu.context),
            };
            let mut warnings = Vec::new();
            let parsed = parse_structural_metadata(&options, &mut warnings);
            let uploads = gpu.host.uploads.borrow().clone();
            match parsed.and_then(|mut m| summary(&mut m, &Names(&named), uploads)) {
                Ok(result) => ok(json!({ "result": result, "warnings": messages(&warnings) })),
                Err(e) => thrown(e),
            }
        },
    );
}

#[test]
fn structural_metadata() {
    run(
        "scene",
        "StructuralMetadata",
        |_| EXACT,
        |f, a| {
            let gpu = Gpu::new(16384.);
            let schema = schema(&a[0]);
            let sensor = schema.classes.get("sensor").unwrap().clone();
            let result = (|| -> Result<Value, MetadataError> {
                if f == "bare" {
                    let mut m = StructuralMetadata::<Host>::new(StructuralMetadataOptions {
                        schema: schema.clone(),
                        property_tables: None,
                        property_textures: Vec::new(),
                        property_attributes: Vec::new(),
                        statistics: None,
                        extras: None,
                        extensions: None,
                    });
                    let missing = m.get_property_table(0).map(|_| "table");
                    return Ok(json!([summary(&mut m, &Names(&[]), Vec::new())?, missing]));
                }
                let (textures, named) = gpu.textures(&json!([0, 1]));
                let bufs = buffers(&a[2]);
                let views = byte_views(&bufs);
                let options = ParseStructuralMetadataOptions {
                    extension: &a[1],
                    schema: schema.clone(),
                    buffer_views: &views,
                    textures: &textures,
                    context: Some(&gpu.context),
                };
                let mut warnings = Vec::new();
                let parsed = parse_structural_metadata(&options, &mut warnings)?;
                let uploads = gpu.host.uploads.borrow().clone();
                let only: GltfTextures<Host> = [("1".to_string(), textures["1"].clone())].into_iter().collect();
                let texture = PropertyTexture::new(
                    &json!({ "properties": { "temp": { "index": 1 } } }),
                    sensor.clone(),
                    &only,
                    Some("only".into()),
                    Some(MetadataId::Index(0)),
                )?;
                let attribute = PropertyAttribute::new(
                    &json!({ "properties": { "level": { "attribute": "_L" } } }),
                    sensor.clone(),
                    Some("attrs".into()),
                    Some(4),
                )?;
                let mut m = StructuralMetadata::new(StructuralMetadataOptions {
                    schema: schema.clone(),
                    property_tables: Some(parsed.property_tables().to_vec()),
                    property_textures: vec![texture],
                    property_attributes: vec![attribute],
                    statistics: Some(json!({ "s": 1 })),
                    extras: Some(json!({ "e": 2 })),
                    extensions: Some(json!({ "X": 3 })),
                });
                let lookups = json!([
                    m.get_property_table(0).and_then(|t| t.name()),
                    m.get_property_table(1).and_then(|t| t.id()),
                    m.get_property_table(5).map(|_| "table"),
                    m.get_property_texture(0).and_then(|t| t.name()),
                    m.get_property_texture(1).map(|_| "texture"),
                    m.get_property_attribute(0).and_then(|t| t.name()),
                    m.get_property_attribute(2).map(|_| "attribute"),
                ]);
                let mut out = vec![summary(&mut m, &Names(&named), uploads)?];
                out.extend(lookups.as_array().unwrap().iter().cloned());
                Ok(json!({ "result": out, "warnings": messages(&warnings) }))
            })();
            match result {
                Ok(v) => ok(v),
                Err(e) => thrown(e),
            }
        },
    );
}

#[test]
fn get_metadata_property_cases() {
    run(
        "scene",
        "getMetadataProperty",
        |_| EXACT,
        |f, a| {
            if f == "undefined" {
                return ok(json!(get_metadata_property::<Host>(None, "sensor", "temp").map(|_| "found")));
            }
            let gpu = Gpu::new(16384.);
            let (textures, _) = gpu.textures(&json!([0, 1]));
            let views = JsMap::new();
            let options = ParseStructuralMetadataOptions {
                extension: &a[1],
                schema: schema(&a[0]),
                buffer_views: &views,
                textures: &textures,
                context: Some(&gpu.context),
            };
            let m = parse_structural_metadata(&options, &mut Vec::new()).unwrap();
            let found: Vec<Value> = a[2]
                .as_array()
                .unwrap()
                .iter()
                .map(|q| match get_metadata_property(Some(&m), s(&q[0]), s(&q[1])) {
                    Some(p) => {
                        json!([p.class_property().id(), p.texture_reader().channels, p.texture_reader().index.map(num)])
                    }
                    None => Value::Null,
                })
                .collect();
            ok(found)
        },
    );
}

#[test]
fn parse_feature_metadata_legacy_cases() {
    run(
        "scene",
        "parseFeatureMetadataLegacy",
        |_| EXACT,
        |_, a| {
            let gpu = Gpu::new(16384.);
            let names = if a[1].as_object().is_some_and(|o| o.contains_key("featureTables")) {
                json!([0, 1])
            } else {
                json!([])
            };
            let (textures, named) = gpu.textures(&names);
            let bufs = buffers(&a[2]);
            let views = byte_views(&bufs);
            let options = ParseFeatureMetadataLegacyOptions {
                extension: &a[1],
                schema: schema(&a[0]),
                buffer_views: &views,
                textures: &textures,
            };
            match parse_feature_metadata_legacy(&options).and_then(|mut m| summary(&mut m, &Names(&named), Vec::new()))
            {
                Ok(v) => ok(v),
                Err(e) => thrown(e),
            }
        },
    );
}

fn typed_array(name: &str, values: &[f64]) -> TypedArray {
    match name {
        "Int8Array" => TypedArray::Int8(values.iter().map(|&x| x as i8).collect()),
        "Uint8Array" => TypedArray::Uint8(values.iter().map(|&x| x as u8).collect()),
        "Int16Array" => TypedArray::Int16(values.iter().map(|&x| x as i16).collect()),
        "Uint16Array" => TypedArray::Uint16(values.iter().map(|&x| x as u16).collect()),
        "Int32Array" => TypedArray::Int32(values.iter().map(|&x| x as i32).collect()),
        "Uint32Array" => TypedArray::Uint32(values.iter().map(|&x| x as u32).collect()),
        "Float32Array" => TypedArray::Float32(values.iter().map(|&x| x as f32).collect()),
        _ => TypedArray::Float64(values.to_vec()),
    }
}

fn custom_attribute(a: &Attribute<Host>) -> Value {
    let typed = a.typed_array.as_ref().unwrap();
    json!([
        a.name,
        a.count,
        a.attribute_type.map(|t| t.as_str()),
        a.component_datatype.map(|d| d as u32),
        typed_array_name(typed),
        typed.to_f64_vec().into_iter().map(num).collect::<Vec<_>>(),
    ])
}

#[test]
fn parse_batch_table_cases() {
    run(
        "scene",
        "parseBatchTable",
        |_| EXACT,
        |_, a| {
            let tile: Option<Vec<u8>> = a[2].as_array().map(|b| b.iter().map(|x| x.as_u64().unwrap() as u8).collect());
            let decoded: Option<HashMap<String, TypedArray>> = a[5]
                .as_object()
                .map(|m| m.iter().map(|(id, v)| (id.clone(), typed_array(s(&v[0]), &nums(&v[1])))).collect());
            let options = ParseBatchTableOptions {
                count: u(&a[1]) as usize,
                batch_table: &a[0],
                binary_body: tile.as_deref().map(|buffer| ByteView { buffer, byte_offset: u(&a[3]) as usize }),
                parse_as_property_attributes: a[4] == true,
                typed_arrays: decoded.as_ref(),
            };
            let mut output = Vec::new();
            let mut warnings = Vec::new();
            let parsed = parse_batch_table::<Host>(&options, (a[4] == true).then_some(&mut output), &mut warnings);
            match parsed.and_then(|mut m| summary(&mut m, &Names(&[]), Vec::new())) {
                Ok(metadata) => ok(json!({
                    "metadata": metadata,
                    "attributes": output.iter().map(custom_attribute).collect::<Vec<_>>(),
                    "warnings": warnings.iter().map(|w| json!([w.id(), w.message()])).collect::<Vec<_>>(),
                })),
                Err(e) => thrown(e),
            }
        },
    );
}

#[test]
fn sanitizes_glsl_identifiers() {
    // ModelUtility.sanitizeGlslIdentifier, which parseBatchTable names attributes with.
    for (input, expected) in [
        ("Temperature ℃", "Temperature_"),
        ("gl_Position", "Position"),
        ("1st", "_1st"),
        ("a--b  c", "a_b_c"),
        ("", ""),
        ("😀x", "_x"),
        ("gl_9", "_9"),
    ] {
        assert_eq!(parse_batch_table::sanitize_glsl_identifier(input), expected, "{input}");
    }
}
