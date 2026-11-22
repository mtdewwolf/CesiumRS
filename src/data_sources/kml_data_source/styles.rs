//! KML Style, StyleMap, IconStyle, LabelStyle, LineStyle, PolyStyle, BalloonStyle, and description rendering.
use super::{
    xml::{
        node_text_content, parse_color_string, query_boolean_value, query_child_nodes, query_color_value,
        query_first_node, query_nodes, query_numeric_attribute, query_numeric_value, query_string_attribute,
        query_string_value, resolve_href, UriResolver, GX_NAMESPACES, KML_NAMESPACES,
    },
    KmlDataSource, KmlFeatureData,
};
use crate::{
    core::{
        iso8601, math,
        query::{query_to_object, QueryValue},
        resource::DerivedResourceOptions,
        uri::Uri,
        BoundingRectangle, Cartesian2, Cartesian3, Color, NearFarScalar, Resource, ResourceClient, RuntimeError,
    },
    data_sources::{
        BillboardGraphics, ColorMaterialProperty, DefinitionChanged, DynamicValue, Entity, EntityCollection,
        EntityComponent, EntityOptions, EntityRef, LabelGraphics, PolygonGraphics, PolylineGraphics, PropertyInput,
    },
    scene::{HorizontalOrigin, LabelStyle},
};
use regex::Regex;
use roxmltree::{Document, Node};
use serde_json::json;
use std::{any::Any, rc::Rc, sync::OnceLock};

pub(crate) const BILLBOARD_SIZE: f64 = 32.0;
pub(crate) const BILLBOARD_NEAR_DISTANCE: f64 = 2_414_016.0;
pub(crate) const BILLBOARD_NEAR_RATIO: f64 = 1.0;
pub(crate) const BILLBOARD_FAR_DISTANCE: f64 = 1.6093e7;
pub(crate) const BILLBOARD_FAR_RATIO: f64 = 0.1;

/// Parsed KML `<BalloonStyle>` carried on style entities during style resolution.
#[derive(Clone)]
pub(crate) struct KmlBalloonStyle {
    pub(crate) bg_color: Color,
    pub(crate) text_color: Color,
    pub(crate) text: Option<String>,
    changed: DefinitionChanged,
}

impl KmlBalloonStyle {
    pub(crate) fn new(bg_color: Color, text_color: Color, text: Option<String>) -> Self {
        Self { bg_color, text_color, text, changed: DefinitionChanged::new() }
    }
}

impl EntityComponent for KmlBalloonStyle {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn is_constant(&self) -> bool {
        true
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        Ok(other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|o| self.bg_color == o.bg_color && self.text_color == o.text_color && self.text == o.text))
    }
}

pub(crate) fn near_far_value(s: NearFarScalar) -> PropertyInput {
    PropertyInput::Value(DynamicValue::from_json(json!({
        "near": s.near,
        "nearValue": s.near_value,
        "far": s.far,
        "farValue": s.far_value,
    })))
}

pub(crate) fn bounding_rect_value(r: BoundingRectangle) -> PropertyInput {
    PropertyInput::Value(DynamicValue::from_json(json!({
        "x": r.x,
        "y": r.y,
        "width": r.width,
        "height": r.height,
    })))
}

pub(crate) fn create_default_billboard() -> Result<Rc<BillboardGraphics>, RuntimeError> {
    let billboard = Rc::new(BillboardGraphics::new());
    billboard.set_width(Some(PropertyInput::value(BILLBOARD_SIZE)))?;
    billboard.set_height(Some(PropertyInput::value(BILLBOARD_SIZE)))?;
    let scale =
        NearFarScalar::new(BILLBOARD_NEAR_DISTANCE, BILLBOARD_NEAR_RATIO, BILLBOARD_FAR_DISTANCE, BILLBOARD_FAR_RATIO);
    billboard.set_scale_by_distance(Some(near_far_value(scale)))?;
    billboard.set_pixel_offset_scale_by_distance(Some(near_far_value(scale)))?;
    Ok(billboard)
}

pub(crate) fn create_default_polygon() -> Result<Rc<PolygonGraphics>, RuntimeError> {
    let polygon = Rc::new(PolygonGraphics::new());
    polygon.set_outline(Some(PropertyInput::value(true)))?;
    polygon.set_outline_color(Some(PropertyInput::value(Color::WHITE)))?;
    Ok(polygon)
}

pub(crate) fn create_default_label() -> Result<Rc<LabelGraphics>, RuntimeError> {
    let label = Rc::new(LabelGraphics::new());
    let translucency = NearFarScalar::new(3_000_000.0, 1.0, 5_000_000.0, 0.0);
    label.set_translucency_by_distance(Some(near_far_value(translucency)))?;
    label.set_pixel_offset(Some(PropertyInput::value(Cartesian2::new(17.0, 0.0))))?;
    label.set_horizontal_origin(Some(PropertyInput::value(HorizontalOrigin::Left as i32 as f64)))?;
    label.set_font(Some(PropertyInput::value("16px sans-serif")))?;
    label.set_style(Some(PropertyInput::value(LabelStyle::FillAndOutline as i32 as f64)))?;
    Ok(label)
}

pub(crate) fn cleanup_query_prefix(s: &str) -> &str {
    s.strip_prefix('&').or_else(|| s.strip_prefix('?')).unwrap_or(s)
}

pub(crate) fn set_query_string(resource: &Resource, query: &str) {
    let cleaned = cleanup_query_prefix(query);
    if cleaned.is_empty() {
        return;
    }
    if let Some(entries) = query_to_object(cleaned) {
        let params: Vec<(String, Option<QueryValue>)> = entries.into_iter().map(|(k, v)| (k, Some(v))).collect();
        resource.set_query_parameters(&params, false);
    }
}

pub(crate) fn get_icon_href(
    icon_node: Option<Node<'_, '_>>,
    data_source: &KmlDataSource,
    source_resource: &Resource,
    uri_resolver: Option<&UriResolver>,
    can_refresh: bool,
) -> Option<Resource> {
    let icon_node = icon_node?;
    let mut href = query_string_value(icon_node, "href", KML_NAMESPACES)?;
    if href.is_empty() {
        return None;
    }

    if let Some(rest) = href.strip_prefix("root://icons/palette-") {
        if let Some(palette) = rest.chars().next() {
            let x = query_numeric_value(icon_node, "x", GX_NAMESPACES).unwrap_or(0.0);
            let y = query_numeric_value(icon_node, "y", GX_NAMESPACES).unwrap_or(0.0);
            let x_idx = (x / 32.0).min(7.0);
            let y_idx = 7.0 - (y / 32.0).min(7.0);
            let icon_num = 8.0 * y_idx + x_idx;
            href = format!(
                "https://maps.google.com/mapfiles/kml/pal{palette}/icon{}.png",
                math::js_number_to_string(icon_num)
            );
        }
    }

    let href_resource = resolve_href(Some(&href), source_resource, uri_resolver)?;

    if can_refresh {
        let view_refresh_mode = query_string_value(icon_node, "viewRefreshMode", KML_NAMESPACES);
        let view_bound_scale = query_numeric_value(icon_node, "viewBoundScale", KML_NAMESPACES).unwrap_or(1.0);
        let default_view_format = if view_refresh_mode.as_deref() == Some("onStop") {
            "BBOX=[bboxWest],[bboxSouth],[bboxEast],[bboxNorth]"
        } else {
            ""
        };
        let view_format = query_string_value(icon_node, "viewFormat", KML_NAMESPACES)
            .unwrap_or_else(|| default_view_format.to_owned());
        let http_query = query_string_value(icon_node, "httpQuery", KML_NAMESPACES);
        if !view_format.is_empty() {
            set_query_string(&href_resource, &view_format);
        }
        if let Some(q) = http_query {
            set_query_string(&href_resource, &q);
        }
        data_source.resolve_view_query(&href_resource, view_bound_scale);
    }

    Some(href_resource)
}

fn process_billboard_icon(
    data_source: &KmlDataSource,
    node: Node<'_, '_>,
    target_entity: &EntityRef,
    source_resource: &Resource,
    uri_resolver: Option<&UriResolver>,
) -> Result<(), RuntimeError> {
    let scale = query_numeric_value(node, "scale", KML_NAMESPACES);
    let heading = query_numeric_value(node, "heading", KML_NAMESPACES);
    let color = query_color_value(node, "color", KML_NAMESPACES);

    let icon_node = query_first_node(node, "Icon", KML_NAMESPACES);
    let icon = get_icon_href(icon_node, data_source, source_resource, uri_resolver, false);

    // If `<Icon>` is present but blank, mark as `false` so `process_position_graphics`
    // suppresses the default yellow pin.
    let image_input = match (icon_node, icon) {
        (_, Some(res)) => Some(PropertyInput::value(res)),
        (Some(_), None) => Some(PropertyInput::value(false)),
        (None, None) => None,
    };

    let (x, y, w, h) = match icon_node {
        Some(n) => (
            query_numeric_value(n, "x", GX_NAMESPACES),
            query_numeric_value(n, "y", GX_NAMESPACES),
            query_numeric_value(n, "w", GX_NAMESPACES),
            query_numeric_value(n, "h", GX_NAMESPACES),
        ),
        None => (None, None, None, None),
    };

    let hot_spot_node = query_first_node(node, "hotSpot", KML_NAMESPACES);
    let (hot_spot_x, hot_spot_y, hot_spot_x_unit, hot_spot_y_unit) = match hot_spot_node {
        Some(hs) => (
            query_numeric_attribute(hs, "x"),
            query_numeric_attribute(hs, "y"),
            query_string_attribute(hs, "xunits"),
            query_string_attribute(hs, "yunits"),
        ),
        None => (None, None, None, None),
    };

    let billboard = match target_entity.get_property("billboard").and_then(|p| p.component()) {
        Some(c) if c.as_any().is::<BillboardGraphics>() => c,
        _ => {
            let bb = create_default_billboard()?;
            target_entity.set_property("billboard", Some(PropertyInput::Component(bb.clone())))?;
            bb
        }
    };
    let bb = billboard.as_any().downcast_ref::<BillboardGraphics>().expect("billboard graphics");

    bb.set_image(image_input)?;
    bb.set_scale(scale.map(PropertyInput::value))?;
    bb.set_color(color.map(PropertyInput::value))?;

    if x.is_some() || y.is_some() || w.is_some() || h.is_some() {
        let rect = BoundingRectangle::new(x.unwrap_or(0.0), y.unwrap_or(0.0), w.unwrap_or(0.0), h.unwrap_or(0.0));
        bb.set_image_sub_region(Some(bounding_rect_value(rect)))?;
    }

    if let Some(heading) = heading {
        if heading != 0.0 {
            bb.set_rotation(Some(PropertyInput::value(math::to_radians(-heading))))?;
            bb.set_aligned_axis(Some(PropertyInput::value(Cartesian3::UNIT_Z)))?;
        }
    }

    let eff_scale = scale.unwrap_or(1.0);
    let x_offset = hot_spot_x.map(|hx| {
        let base = match hot_spot_x_unit.as_deref() {
            Some("pixels") => -hx * eff_scale,
            Some("insetPixels") => (hx - BILLBOARD_SIZE) * eff_scale,
            Some("fraction") => -hx * BILLBOARD_SIZE * eff_scale,
            _ => f64::NAN,
        };
        base + BILLBOARD_SIZE * 0.5 * eff_scale
    });
    let y_offset = hot_spot_y.map(|hy| {
        let base = match hot_spot_y_unit.as_deref() {
            Some("pixels") => hy * eff_scale,
            Some("insetPixels") => (-hy + BILLBOARD_SIZE) * eff_scale,
            Some("fraction") => hy * BILLBOARD_SIZE * eff_scale,
            _ => f64::NAN,
        };
        base - BILLBOARD_SIZE * 0.5 * eff_scale
    });
    if x_offset.is_some() || y_offset.is_some() {
        bb.set_pixel_offset(Some(PropertyInput::value(Cartesian2::new(
            x_offset.unwrap_or(0.0),
            y_offset.unwrap_or(0.0),
        ))))?;
    }

    Ok(())
}

pub(crate) fn apply_style(
    data_source: &KmlDataSource,
    style_node: Node<'_, '_>,
    target_entity: &EntityRef,
    source_resource: &Resource,
    uri_resolver: Option<&UriResolver>,
) -> Result<(), RuntimeError> {
    for node in style_node.children().filter(Node::is_element) {
        match node.tag_name().name() {
            "IconStyle" => {
                process_billboard_icon(data_source, node, target_entity, source_resource, uri_resolver)?;
            }
            "LabelStyle" => {
                let label_comp = match target_entity.get_property("label").and_then(|p| p.component()) {
                    Some(c) if c.as_any().is::<LabelGraphics>() => c,
                    _ => {
                        let lbl = create_default_label()?;
                        target_entity.set_property("label", Some(PropertyInput::Component(lbl.clone())))?;
                        lbl
                    }
                };
                let label = label_comp.as_any().downcast_ref::<LabelGraphics>().expect("label graphics");
                if let Some(scale) = query_numeric_value(node, "scale", KML_NAMESPACES) {
                    label.set_scale(Some(PropertyInput::value(scale)))?;
                }
                if let Some(color) = query_color_value(node, "color", KML_NAMESPACES) {
                    label.set_fill_color(Some(PropertyInput::value(color)))?;
                }
                if let Some(name) = target_entity.name() {
                    label.set_text(Some(PropertyInput::value(name)))?;
                }
            }
            "LineStyle" => {
                let poly_comp = match target_entity.get_property("polyline").and_then(|p| p.component()) {
                    Some(c) if c.as_any().is::<PolylineGraphics>() => c,
                    _ => {
                        let pl = Rc::new(PolylineGraphics::new());
                        target_entity.set_property("polyline", Some(PropertyInput::Component(pl.clone())))?;
                        pl
                    }
                };
                let polyline = poly_comp.as_any().downcast_ref::<PolylineGraphics>().expect("polyline graphics");
                polyline.set_width(query_numeric_value(node, "width", KML_NAMESPACES).map(PropertyInput::value))?;
                polyline.set_material(query_color_value(node, "color", KML_NAMESPACES).map(PropertyInput::value))?;
            }
            "PolyStyle" => {
                let poly_comp = match target_entity.get_property("polygon").and_then(|p| p.component()) {
                    Some(c) if c.as_any().is::<PolygonGraphics>() => c,
                    _ => {
                        let pg = create_default_polygon()?;
                        target_entity.set_property("polygon", Some(PropertyInput::Component(pg.clone())))?;
                        pg
                    }
                };
                let polygon = poly_comp.as_any().downcast_ref::<PolygonGraphics>().expect("polygon graphics");
                if let Some(color) = query_color_value(node, "color", KML_NAMESPACES) {
                    polygon.set_material(Some(PropertyInput::value(color)))?;
                }
                if let Some(fill) = query_boolean_value(node, "fill", KML_NAMESPACES) {
                    polygon.set_fill(Some(PropertyInput::value(fill)))?;
                }
                if let Some(outline) = query_boolean_value(node, "outline", KML_NAMESPACES) {
                    polygon.set_outline(Some(PropertyInput::value(outline)))?;
                }
            }
            "BalloonStyle" => {
                let bg = parse_color_string(query_string_value(node, "bgColor", KML_NAMESPACES).as_deref(), false)
                    .unwrap_or(Color::WHITE);
                let fg = parse_color_string(query_string_value(node, "textColor", KML_NAMESPACES).as_deref(), false)
                    .unwrap_or(Color::BLACK);
                let text = query_string_value(node, "text", KML_NAMESPACES);
                if !target_entity.property_names().iter().any(|n| n == "balloonStyle") {
                    target_entity.add_property("balloonStyle")?;
                }
                target_entity.set_property(
                    "balloonStyle",
                    Some(PropertyInput::Component(Rc::new(KmlBalloonStyle::new(bg, fg, text)))),
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

pub(crate) fn compute_final_style(
    data_source: &KmlDataSource,
    feature_node: Node<'_, '_>,
    style_collection: &EntityCollection,
    source_resource: &Resource,
    uri_resolver: Option<&UriResolver>,
) -> Result<EntityRef, RuntimeError> {
    let result = Entity::new(EntityOptions::default())?;

    // Google Earth uses the last inline Style/StyleMap only.
    let mut inline_style = None;
    for child in feature_node.children() {
        if child.is_element() && matches!(child.tag_name().name(), "Style" | "StyleMap") {
            inline_style = Some(child);
        }
    }

    if let Some(inline_node) = inline_style {
        if inline_node.tag_name().name() == "Style" {
            apply_style(data_source, inline_node, &result, source_resource, uri_resolver)?;
        } else {
            for pair in query_child_nodes(inline_node, "Pair", KML_NAMESPACES) {
                if query_string_value(pair, "key", KML_NAMESPACES).as_deref() == Some("normal") {
                    if let Some(style_url) = query_string_value(pair, "styleUrl", KML_NAMESPACES) {
                        let style_entity = style_collection
                            .get_by_id(&style_url)
                            .or_else(|| style_collection.get_by_id(&format!("#{style_url}")));
                        if let Some(se) = style_entity {
                            result.merge(&se)?;
                        }
                    } else if let Some(style_node) = query_first_node(pair, "Style", KML_NAMESPACES) {
                        apply_style(data_source, style_node, &result, source_resource, uri_resolver)?;
                    }
                }
            }
        }
    }

    // Google Earth uses the first external styleUrl only.
    if let Some(external_style) = query_string_value(feature_node, "styleUrl", KML_NAMESPACES) {
        let id = if !external_style.starts_with('#') && external_style.contains('#') {
            let mut tokens = external_style.splitn(2, '#');
            let uri = tokens.next().unwrap_or("");
            let fragment = tokens.next().unwrap_or("");
            let res = source_resource
                .get_derived_resource(DerivedResourceOptions { url: Some(uri.to_owned()), ..Default::default() })
                .unwrap_or_else(|| source_resource.clone());
            format!("{}#{fragment}", res.get_url_component(false, false))
        } else {
            external_style
        };

        let style_entity = style_collection.get_by_id(&id).or_else(|| style_collection.get_by_id(&format!("#{id}")));
        if let Some(se) = style_entity {
            result.merge(&se)?;
        }
    }

    Ok(result)
}

pub(crate) fn process_local_styles(
    data_source: &KmlDataSource,
    doc: &Document<'_>,
    style_collection: &EntityCollection,
    source_resource: &Resource,
    is_external: bool,
    uri_resolver: Option<&UriResolver>,
) -> Result<Vec<Resource>, RuntimeError> {
    let root = doc.root_element();
    let prefix = if is_external { source_resource.get_url_component(false, false) } else { String::new() };

    for node in query_nodes(root, "Style", KML_NAMESPACES) {
        if let Some(raw_id) = query_string_attribute(node, "id") {
            let id = format!("{prefix}#{raw_id}");
            if style_collection.get_by_id(&id).is_none() {
                let style_entity = Entity::with_id(id);
                style_collection.add(style_entity.clone())?;
                apply_style(data_source, node, &style_entity, source_resource, uri_resolver)?;
            }
        }
    }

    for style_map in query_nodes(root, "StyleMap", KML_NAMESPACES) {
        if let Some(raw_id) = query_string_attribute(style_map, "id") {
            for pair in query_child_nodes(style_map, "Pair", KML_NAMESPACES) {
                if query_string_value(pair, "key", KML_NAMESPACES).as_deref() == Some("normal") {
                    let id = format!("{prefix}#{raw_id}");
                    if style_collection.get_by_id(&id).is_none() {
                        let style_entity = style_collection.get_or_create_entity(id);
                        if let Some(mut style_url) = query_string_value(pair, "styleUrl", KML_NAMESPACES) {
                            if !style_url.starts_with('#') {
                                style_url.insert(0, '#');
                            }
                            if is_external {
                                style_url = format!("{prefix}{style_url}");
                            }
                            if let Some(base) = style_collection.get_by_id(&style_url) {
                                style_entity.merge(&base)?;
                            }
                        } else if let Some(node) = query_first_node(pair, "Style", KML_NAMESPACES) {
                            apply_style(data_source, node, &style_entity, source_resource, uri_resolver)?;
                        }
                    }
                }
            }
        }
    }

    // Collect external style document references (`uri#styleId`).
    let mut external_resources = Vec::new();
    for node in root.descendants().filter(|n| n.is_element() && n.tag_name().name() == "styleUrl") {
        let style_ref = node_text_content(node);
        let trimmed = style_ref.trim();
        if !trimmed.starts_with('#') {
            let tokens: Vec<&str> = trimmed.split('#').collect();
            if tokens.len() == 2 && !tokens[0].is_empty() {
                if let Some(res) = resolve_href(Some(tokens[0]), source_resource, uri_resolver) {
                    external_resources.push(res);
                }
            }
        }
    }
    Ok(external_resources)
}

pub(crate) async fn process_styles(
    data_source: &KmlDataSource,
    doc: &Document<'_>,
    style_collection: &EntityCollection,
    source_resource: &Resource,
    is_external: bool,
    uri_resolver: Option<&UriResolver>,
    resource_client: Option<&ResourceClient>,
) -> Result<(), RuntimeError> {
    let mut pending =
        process_local_styles(data_source, doc, style_collection, source_resource, is_external, uri_resolver)?;
    let mut visited = std::collections::HashSet::new();
    while let Some(res) = pending.pop() {
        let key = res.get_url_component(false, false);
        if !visited.insert(key) {
            continue;
        }
        let xml_text = if res.is_data_uri() {
            crate::core::resource_client::decode_data_uri_text(&res.url())
                .map_err(|e| RuntimeError::new(e.to_string()))?
        } else if let Some(client) = resource_client {
            match res.fetch_text(client) {
                Some(fut) => fut
                    .await
                    .map_err(|e| RuntimeError::new(e.to_string()))?
                    .ok_or_else(|| RuntimeError::new("Empty external KML style document."))?,
                None => continue,
            }
        } else {
            continue;
        };
        let cleaned = super::xml::remove_duplicate_namespaces(&super::xml::insert_namespaces(&xml_text));
        let ext_doc =
            Document::parse_with_options(&cleaned, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() })
                .map_err(|e| RuntimeError::new(e.to_string()))?;
        let more = process_local_styles(data_source, &ext_doc, style_collection, &res, true, uri_resolver)?;
        pending.extend(more);
    }
    Ok(())
}

/// Extracts the color slot or evaluated color from a material `PropertyInput`.
pub(crate) fn extract_material_color_input(material: Option<PropertyInput>) -> Option<PropertyInput> {
    match material? {
        PropertyInput::Component(c) => {
            if let Some(cm) = c.as_any().downcast_ref::<ColorMaterialProperty>() {
                cm.color()
            } else {
                c.get_property("color")
            }
        }
        PropertyInput::Value(DynamicValue::Color(c)) => Some(PropertyInput::value(c)),
        _ => None,
    }
}

pub(crate) fn extract_material_color_value(material: Option<PropertyInput>) -> Option<Color> {
    let input = extract_material_color_input(material)?;
    let time = iso8601::minimum_value();
    crate::data_sources::visualizer_values::eval_color_opt(Some(&input), &time).ok().flatten()
}

fn autolink_html(input: &str) -> String {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let re = URL_RE.get_or_init(|| Regex::new(r#"(?i)\b((?:https?://|www\.)[^\s<>"']+)"#).expect("url regex"));
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    let mut in_anchor = false;
    while !rest.is_empty() {
        if let Some(lt) = rest.find('<') {
            let text_chunk = &rest[..lt];
            if in_anchor {
                out.push_str(text_chunk);
            } else {
                out.push_str(&re.replace_all(text_chunk, |caps: &regex::Captures<'_>| {
                    let matched = &caps[1];
                    let href = if matched.to_ascii_lowercase().starts_with("www.") {
                        format!("http://{matched}")
                    } else {
                        matched.to_owned()
                    };
                    format!(r#"<a href="{href}" target="_blank">{matched}</a>"#)
                }));
            }
            let after_lt = &rest[lt..];
            if let Some(gt) = after_lt.find('>') {
                let tag = &after_lt[..=gt];
                let lower = tag.to_ascii_lowercase();
                if lower.starts_with("<a ") || lower == "<a>" {
                    in_anchor = true;
                } else if lower.starts_with("</a") {
                    in_anchor = false;
                }
                out.push_str(tag);
                rest = &after_lt[gt + 1..];
            } else {
                out.push_str(after_lt);
                break;
            }
        } else {
            if in_anchor {
                out.push_str(rest);
            } else {
                out.push_str(&re.replace_all(rest, |caps: &regex::Captures<'_>| {
                    let matched = &caps[1];
                    let href = if matched.to_ascii_lowercase().starts_with("www.") {
                        format!("http://{matched}")
                    } else {
                        matched.to_owned()
                    };
                    format!(r#"<a href="{href}" target="_blank">{matched}</a>"#)
                }));
            }
            break;
        }
    }
    out
}

fn rewrite_html_links(html: &str, uri_resolver: Option<&UriResolver>, source_resource: &Resource) -> String {
    static TAG_RE: OnceLock<Regex> = OnceLock::new();
    static ATTR_RE: OnceLock<Regex> = OnceLock::new();
    let tag_re = TAG_RE.get_or_init(|| Regex::new(r#"(?is)<([a-z][a-z0-9]*)(\s+[^>]*)?(/?)>"#).expect("tag regex"));
    let attr_re = ATTR_RE.get_or_init(|| {
        Regex::new(r#"(?is)([a-z_:][a-z0-9_.:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("attr regex")
    });

    let target_attr = |tag: &str| -> Option<&'static str> {
        match tag {
            "a" | "link" | "area" => Some("href"),
            "img" | "iframe" | "video" | "audio" | "source" | "track" | "input" | "embed" | "script" => Some("src"),
            _ => None,
        }
    };

    let has_kmz = uri_resolver.is_some_and(|r| !r.keys.is_empty());

    tag_re
        .replace_all(html, |caps: &regex::Captures<'_>| {
            let tag_name = &caps[1];
            let lower_tag = tag_name.to_ascii_lowercase();
            let attrs_str = caps.get(2).map_or("", |m| m.as_str());
            let self_close = caps.get(3).map_or("", |m| m.as_str());

            let attr_to_rewrite = target_attr(&lower_tag);
            let is_anchor = lower_tag == "a";
            let is_video = lower_tag == "video";
            if !is_anchor && attr_to_rewrite.is_none() {
                return caps[0].to_owned();
            }

            let mut attrs: Vec<(String, String)> = Vec::new();
            for ac in attr_re.captures_iter(attrs_str) {
                let k = ac[1].to_owned();
                let v = ac.get(2).or_else(|| ac.get(3)).map_or("", |m| m.as_str()).to_owned();
                attrs.push((k, v));
            }

            if is_anchor {
                if let Some((_, v)) = attrs.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case("target")) {
                    *v = "_blank".to_owned();
                } else {
                    attrs.push(("target".to_owned(), "_blank".to_owned()));
                }
            }

            let mut extra_download: Option<String> = None;
            let source_url = source_resource.get_url_component(false, false);

            for (k, v) in &mut attrs {
                let matches_attr = attr_to_rewrite.is_some_and(|a| k.eq_ignore_ascii_case(a))
                    || (is_video && k.eq_ignore_ascii_case("poster"));
                if !matches_attr {
                    continue;
                }
                let mut embedded = false;
                if has_kmz {
                    if let Some(resolver) = uri_resolver {
                        let rel_uri = Uri::parse(v);
                        let resolved_key = rel_uri.absolute_to(".").map_or_else(|| v.clone(), |u| u.to_string());
                        if let Some(data_uri) = resolver.get(&resolved_key) {
                            *v = data_uri.to_owned();
                            embedded = true;
                            if is_anchor {
                                extra_download = Some(resolved_key);
                            }
                        }
                    }
                }
                if !embedded && !source_url.is_empty() {
                    if let Some(res) = resolve_href(Some(v), source_resource, None) {
                        *v = res.url();
                    }
                }
            }

            if let Some(dl) = extra_download {
                if !attrs.iter().any(|(k, _)| k.eq_ignore_ascii_case("download")) {
                    attrs.push(("download".to_owned(), dl));
                }
            }

            let mut rebuilt = format!("<{tag_name}");
            for (k, v) in attrs {
                rebuilt.push_str(&format!(r#" {k}="{v}""#));
            }
            rebuilt.push_str(self_close);
            rebuilt.push('>');
            rebuilt
        })
        .into_owned()
}

pub(crate) fn process_description(
    node: Node<'_, '_>,
    entity: &EntityRef,
    style_entity: &EntityRef,
    kml_data: &KmlFeatureData,
    uri_resolver: Option<&UriResolver>,
    source_resource: &Resource,
) -> Result<(), RuntimeError> {
    let description = query_string_value(node, "description", KML_NAMESPACES);
    let balloon_style = entity
        .get_property("balloonStyle")
        .or_else(|| style_entity.get_property("balloonStyle"))
        .and_then(|p| p.component())
        .and_then(|c| c.as_any().downcast_ref::<KmlBalloonStyle>().cloned());

    let mut background = Color::WHITE;
    let mut foreground = Color::BLACK;
    let mut text = description.clone();

    if let Some(bs) = &balloon_style {
        background = bs.bg_color;
        foreground = bs.text_color;
        if bs.text.is_some() {
            text = bs.text.clone();
        }
    }

    let extended_data = kml_data.extended_data();
    let resolved_text = if let Some(mut t) = text {
        t = t.replacen("$[name]", entity.name().as_deref().unwrap_or(""), 1);
        t = t.replacen("$[description]", description.as_deref().unwrap_or(""), 1);
        t = t.replacen("$[address]", kml_data.address().as_deref().unwrap_or(""), 1);
        t = t.replacen("$[Snippet]", kml_data.snippet().as_deref().unwrap_or(""), 1);
        t = t.replacen("$[id]", entity.id(), 1);
        t = t.replacen("$[geDirections]", "", 1);

        if let Some(ext) = &extended_data {
            static TOKEN_RE: OnceLock<Regex> = OnceLock::new();
            let re = TOKEN_RE.get_or_init(|| Regex::new(r"\$\[.+?\]").expect("token regex"));
            let tokens: Vec<String> = re.find_iter(&t).map(|m| m.as_str().to_owned()).collect();
            for token in tokens {
                let inner = &token[2..token.len() - 1];
                let (prop_name, is_display_name) = match inner.strip_suffix("/displayName") {
                    Some(base) => (base, true),
                    None => (inner, false),
                };
                if let Some((_, entry)) = ext.iter().find(|(k, _)| k == prop_name) {
                    let val = if is_display_name { entry.display_name.as_deref() } else { entry.value.as_deref() };
                    if let Some(v) = val {
                        t = t.replacen(&token, v, 1);
                    }
                }
            }
        }
        Some(t)
    } else if let Some(ext) = &extended_data {
        if ext.is_empty() {
            None
        } else {
            let mut table = String::from(
                r#"<table class="cesium-infoBox-defaultTable cesium-infoBox-defaultTable-lighter"><tbody>"#,
            );
            for (key, entry) in ext {
                let header = entry.display_name.as_deref().unwrap_or(key.as_str());
                let val = entry.value.as_deref().unwrap_or("");
                table.push_str(&format!("<tr><th>{header}</th><td>{val}</td></tr>"));
            }
            table.push_str("</tbody></table>");
            Some(table)
        }
    } else {
        None
    };

    let Some(mut final_text) = resolved_text else {
        return Ok(());
    };

    final_text = autolink_html(&final_text);
    final_text = rewrite_html_links(&final_text, uri_resolver, source_resource);

    let bg_css = background.to_css_color_string();
    let fg_css = foreground.to_css_color_string();
    let wrapped = format!(
        r#"<div class="cesium-infoBox-description-lighter" style="overflow:auto;word-wrap:break-word;background-color:{bg_css};color:{fg_css};">{final_text}</div>"#
    );
    entity.set_property("description", Some(PropertyInput::value(wrapped)))?;
    Ok(())
}
