//! GeoJSON feature metadata, simplestyle properties and entity creation.
use super::{array, coordinate, field, topology::Topology, CrsFunction, GeoJsonOptions, GeoJsonPin, PinSymbol};
use crate::{
    core::{
        create_guid::create_guid, query::js_object_order, ArcType, Cartesian3, Color, PolygonHierarchy, RuntimeError,
    },
    data_sources::{
        color_material_property::color_material_component, BillboardGraphics, CallbackProperty, ComponentRef,
        ConstantProperty, DynamicValue, EntityCollection, EntityRef, PolygonGraphics, PolylineGraphics, PropertyBag,
        PropertyComponent, PropertyInput, PropertyValue,
    },
    scene::{HeightReference, VerticalOrigin},
};
use serde_json::Value;
use std::{cell::OnceCell, fmt::Write, rc::Rc};

const STYLE: &[&str] = &[
    "title",
    "description",
    "marker-size",
    "marker-symbol",
    "marker-color",
    "stroke",
    "stroke-opacity",
    "stroke-width",
    "fill",
    "fill-opacity",
];
pub(super) fn supported(kind: &str) -> bool {
    matches!(
        kind,
        "Feature"
            | "FeatureCollection"
            | "GeometryCollection"
            | "Point"
            | "MultiPoint"
            | "LineString"
            | "MultiLineString"
            | "Polygon"
            | "MultiPolygon"
            | "Topology"
    )
}
fn entries(value: &Value) -> Vec<(String, &Value)> {
    match value {
        Value::Object(map) => js_object_order(map.iter().map(|(k, v)| (k.clone(), v)).collect()),
        Value::Array(list) => list.iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
        _ => Vec::new(),
    }
}
fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        _ => DynamicValue::from_json(v.clone()).property_string(),
    }
}
fn name_property(p: &Value) -> Option<String> {
    if !p["title"].is_null() {
        return Some("title".into());
    }
    entries(p)
        .into_iter()
        .filter_map(|(key, v)| {
            if v.is_null() || v.as_bool() == Some(false) || v.as_f64() == Some(0.) || v.as_str() == Some("") {
                return None;
            }
            let k = key.to_lowercase();
            let rank = if k == "title" {
                1
            } else if k == "name" {
                2
            } else if k.contains("title") {
                3
            } else if k.contains("name") {
                4
            } else {
                return None;
            };
            Some((rank, key))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, key)| key)
}
fn describe(v: &Value, name: Option<&str>) -> String {
    let mut rows = String::new();
    for (k, v) in entries(v) {
        if Some(k.as_str()) == name || STYLE.contains(&k.as_str()) || v.is_null() {
            continue;
        }
        let value = if v.is_array() || v.is_object() { describe(v, None) } else { text(v) };
        write!(rows, "<tr><th>{k}</th><td>{value}</td></tr>").unwrap();
    }
    if rows.is_empty() {
        rows
    } else {
        format!("<table class=\"cesium-infoBox-defaultTable\"><tbody>{rows}</tbody></table>")
    }
}
fn value(v: impl Into<DynamicValue>) -> Option<PropertyInput> {
    Some(PropertyInput::value(v))
}

pub(super) struct Loader<'a> {
    entities: &'a EntityCollection,
    options: &'a GeoJsonOptions,
    project: &'a CrsFunction,
    stroke_material: ComponentRef,
    fill_material: ComponentRef,
    width: PropertyInput,
}
impl<'a> Loader<'a> {
    pub fn new(
        entities: &'a EntityCollection,
        options: &'a GeoJsonOptions,
        project: &'a CrsFunction,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            entities,
            options,
            project,
            stroke_material: color_material_component(options.stroke)?,
            fill_material: color_material_component(options.fill)?,
            width: PropertyInput::Component(PropertyComponent::wrap(Rc::new(ConstantProperty::new(Some(
                options.stroke_width,
            ))))),
        })
    }
    pub fn object(&self, object: &Value) -> Result<(), RuntimeError> {
        match object["type"].as_str() {
            Some("FeatureCollection") => {
                for f in array(&object["features"])? {
                    self.feature(f)?;
                }
                Ok(())
            }
            Some("Feature") => self.feature(object),
            _ => self.geometry(object, object),
        }
    }
    fn feature(&self, f: &Value) -> Result<(), RuntimeError> {
        match f.get("geometry") {
            None => Err(RuntimeError::new("feature.geometry is required.")),
            Some(Value::Null) => self.entity(f).map(|_| ()),
            Some(g) => self.geometry(f, g),
        }
    }
    fn geometry(&self, f: &Value, g: &Value) -> Result<(), RuntimeError> {
        let coords = &g["coordinates"];
        match g["type"].as_str() {
            Some("Point") => self.point(f, coords)?,
            Some("LineString") => self.line(f, coords)?,
            Some("Polygon") => self.polygon(f, coords)?,
            Some("MultiPoint") => {
                for p in array(coords)? {
                    self.point(f, p)?;
                }
            }
            Some("MultiLineString") => {
                for l in array(coords)? {
                    self.line(f, l)?;
                }
            }
            Some("MultiPolygon") => {
                for p in array(coords)? {
                    self.polygon(f, p)?;
                }
            }
            Some("GeometryCollection") => {
                for child in array(&g["geometries"])? {
                    self.geometry(f, child)?;
                }
            }
            Some("Topology") => {
                let topology = Topology::new(g)?;
                for (_, o) in entries(&g["objects"]) {
                    if o["type"] == "GeometryCollection" {
                        for child in array(&o["geometries"])? {
                            self.feature(&topology.feature(child)?)?;
                        }
                    } else {
                        self.feature(&topology.feature(o)?)?;
                    }
                }
            }
            _ => return Err(RuntimeError::new(format!("Unknown geometry type: {}", field(g, "type")))),
        }
        Ok(())
    }
    fn entity(&self, f: &Value) -> Result<EntityRef, RuntimeError> {
        let id = if f["type"] == "Feature" && !f["id"].is_null() {
            let base = text(&f["id"]);
            let mut id = base.clone();
            let mut i = 2;
            while self.entities.get_by_id(&id).is_some() {
                id = format!("{base}_{i}");
                i += 1;
            }
            id
        } else {
            create_guid()
        };
        let entity = self.entities.get_or_create_entity(id);
        let props = &f["properties"];
        if !props.is_null() {
            let bag = PropertyBag::from_json_ref(props)?;
            entity.set_property("properties", Some(PropertyInput::Component(bag)))?;
            let name = name_property(props);
            if let Some(key) = &name {
                entity.set_name(Some(text(&props[key])));
            }
            let description = match props.get("description") {
                Some(Value::Null) => None,
                Some(v) => value(DynamicValue::from_json(v.clone())),
                None => match &self.options.describe {
                    Some(callback) => Some(callback(props, name.as_deref())?),
                    None => {
                        let props = props.clone();
                        let html = OnceCell::new();
                        let callback = CallbackProperty::new(
                            Rc::new(move |_, _| {
                                Ok(Some(html.get_or_init(|| describe(&props, name.as_deref())).clone()))
                            }),
                            true,
                        );
                        Some(PropertyInput::Component(PropertyComponent::wrap(Rc::new(callback))))
                    }
                },
            };
            entity.set_property("description", description)?;
        }
        Ok(entity)
    }
    fn point(&self, feature: &Value, coords: &Value) -> Result<(), RuntimeError> {
        let p = &feature["properties"];
        let size = match p["marker-size"].as_str() {
            Some("small") => 24.,
            Some("medium") => 48.,
            Some("large") => 64.,
            _ => self.options.marker_size,
        };
        let color = match p["marker-color"].as_str() {
            Some(css) => Color::from_css_color_string(css).ok_or_else(|| RuntimeError::new("Invalid marker-color."))?,
            None => self.options.marker_color,
        };
        let symbol = p["marker-symbol"].as_str().or(self.options.marker_symbol.as_deref());
        let symbol = match symbol {
            None => PinSymbol::Color,
            Some(s) if s.encode_utf16().count() == 1 => PinSymbol::Text(s.to_uppercase()),
            Some(s) => PinSymbol::Maki(s.into()),
        };
        let pin = Rc::new(GeoJsonPin { symbol, color, size });
        let (coords, coord_len) = coordinate(coords)?;
        let billboard = BillboardGraphics::new();
        billboard.set_vertical_origin(value(VerticalOrigin::Bottom.value() as f64))?;
        billboard.set_image(value(pin))?;
        if coord_len == 2 && self.options.clamp_to_ground {
            billboard.set_height_reference(value(HeightReference::ClampToGround.value() as f64))?;
        }
        let entity = self.entity(feature)?;
        entity.set_property("billboard", Some(PropertyInput::Component(Rc::new(billboard))))?;
        entity.set_property("position", value((self.project)(&coords[..coord_len])?))
    }
    fn positions(&self, coordinates: &Value) -> Result<Vec<Cartesian3>, RuntimeError> {
        let arr = array(coordinates)?;
        let mut out = Vec::with_capacity(arr.len());
        for p in arr {
            let (coords, len) = coordinate(p)?;
            out.push((self.project)(&coords[..len])?);
        }
        Ok(out)
    }
    fn stroke_override(&self, p: &Value) -> Option<Color> {
        let mut color = p["stroke"].as_str().and_then(Color::from_css_color_string);
        if let Some(alpha) = p["stroke-opacity"].as_f64().filter(|a| *a != 1.) {
            color.get_or_insert(self.options.stroke).alpha = alpha;
        }
        color
    }
    fn width(&self, p: &Value) -> Option<PropertyInput> {
        Some(p["stroke-width"].as_f64().map(PropertyInput::value).unwrap_or_else(|| self.width.clone()))
    }
    fn line(&self, feature: &Value, coords: &Value) -> Result<(), RuntimeError> {
        let p = &feature["properties"];
        let line = Rc::new(PolylineGraphics::new());
        line.set_clamp_to_ground(value(self.options.clamp_to_ground))?;
        line.set_material(
            self.stroke_override(p)
                .map(PropertyInput::value)
                .or_else(|| Some(PropertyInput::Component(self.stroke_material.clone()))),
        )?;
        line.set_width(self.width(p))?;
        line.set_positions(value(Rc::new(self.positions(coords)?)))?;
        line.set_arc_type(value(ArcType::Rhumb as i32 as f64))?;
        let entity = self.entity(feature)?;
        entity.set_property("polyline", Some(PropertyInput::Component(line)))
    }
    fn polygon(&self, feature: &Value, coords: &Value) -> Result<(), RuntimeError> {
        let rings = array(coords)?;
        let Some(outer) = rings.first() else {
            return Ok(());
        };
        let Some(first) = array(outer)?.first() else {
            return Ok(());
        };
        let p = &feature["properties"];
        let mut fill = self.options.fill;
        if let Some(css) = p["fill"].as_str() {
            fill = Color::from_css_color_string(css).ok_or_else(|| RuntimeError::new("Invalid fill color."))?;
        }
        fill.alpha = p["fill-opacity"].as_f64().unwrap_or(self.options.fill.alpha);
        let polygon = PolygonGraphics::new();
        polygon.set_outline(value(true))?;
        polygon.set_outline_color(
            self.stroke_override(p).map(PropertyInput::value).or_else(|| self.stroke_material.get_property("color")),
        )?;
        polygon.set_outline_width(self.width(p))?;
        let fill_override = !p["fill"].is_null() || fill.alpha != self.options.fill.alpha;
        polygon.set_material(if fill_override {
            value(fill)
        } else {
            Some(PropertyInput::Component(self.fill_material.clone()))
        })?;
        polygon.set_arc_type(value(ArcType::Rhumb as i32 as f64))?;
        let mut holes = Vec::new();
        for ring in &rings[1..] {
            holes.push(PolygonHierarchy::new(self.positions(ring)?, vec![]));
        }
        polygon.set_hierarchy(value(Rc::new(PolygonHierarchy::new(self.positions(outer)?, holes))))?;
        if array(first)?.len() > 2 {
            polygon.set_per_position_height(value(true))?;
        } else if !self.options.clamp_to_ground {
            polygon.set_height(value(0.))?;
        }
        self.entity(feature)?.set_property("polygon", Some(PropertyInput::Component(Rc::new(polygon))))
    }
}
