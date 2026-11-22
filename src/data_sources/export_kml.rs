//! CesiumJS `exportKml`: exports an [`EntityCollection`] as KML 2.2 XML (plus external files)
//! or a compressed KMZ archive.
use super::{
    kml_data_source::zip::write_zip,
    visualizer_values::{
        dynamic_to_cartesian3, dynamic_to_color, eval_bool, eval_bounding_rectangle_opt, eval_cartesian2_opt,
        eval_cartesian3_opt, eval_color_opt, eval_dynamic, eval_f64, eval_f64_opt, eval_height_reference,
        eval_horizontal_origin, eval_vertical_origin,
    },
    BillboardGraphics, CompositePositionProperty, DynamicValue, EntityCollection, EntityRef, ModelGraphics,
    PathGraphics, PointGraphics, PolygonGraphics, PolylineGraphics, PositionProperty, PositionPropertyRef,
    PropertyInput, PropertyIntervals, RectangleGraphics, SampledPositionProperty, ScaledPositionProperty,
};
use crate::{
    core::{
        create_guid::create_guid, iso8601, math, resource_client::data_uri_response, uri_helpers::is_data_uri,
        Cartesian3, Color, Ellipsoid, JulianDate, PolygonHierarchy, Rectangle, ReferenceFrame, RuntimeError,
        TimeInterval,
    },
    scene::{HeightReference, HorizontalOrigin, VerticalOrigin},
};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
    sync::OnceLock,
};

const BILLBOARD_SIZE: f64 = 32.0;
const KML_NAMESPACE: &str = "http://www.opengis.net/kml/2.2";
const GX_NAMESPACE: &str = "http://www.google.com/kml/ext/2.2";

/// Callback invoked for each [`ModelGraphics`] encountered during export.
/// Returns the model URI for `<Link><href>` and may add external files to `external_files`.
pub type ExportKmlModelCallback =
    Rc<dyn Fn(&ModelGraphics, &JulianDate, &mut BTreeMap<String, Vec<u8>>) -> Result<String, RuntimeError>>;

/// Options for [`export_kml`].
#[derive(Clone)]
pub struct ExportKmlOptions {
    pub entities: Rc<EntityCollection>,
    pub ellipsoid: Ellipsoid,
    pub model_callback: Option<ExportKmlModelCallback>,
    pub time: Option<JulianDate>,
    pub default_availability: Option<TimeInterval<()>>,
    pub sample_duration: f64,
    pub kmz: bool,
}

impl ExportKmlOptions {
    pub fn new(entities: Rc<EntityCollection>) -> Self {
        Self {
            entities,
            ellipsoid: Ellipsoid::WGS84,
            model_callback: None,
            time: None,
            default_availability: None,
            sample_duration: 60.0,
            kmz: false,
        }
    }
}

/// Output of [`export_kml`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportKmlResult {
    Kml { kml: String, external_files: BTreeMap<String, Vec<u8>> },
    Kmz(Vec<u8>),
}

// ---------------------------------------------------------------------------
// Minimal XML DOM builder matching XMLSerializer output
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum XmlNode {
    Element(XmlElement),
    Text(String),
}

#[derive(Clone, Debug)]
struct XmlElement {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlNode>,
}

impl XmlElement {
    fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into(), attrs: Vec::new(), children: Vec::new() }
    }

    fn set_attr(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        if let Some((_, v)) = self.attrs.iter_mut().find(|(k, _)| k == &name) {
            *v = value;
        } else {
            self.attrs.push((name, value));
        }
    }

    fn append_child(&mut self, child: XmlElement) {
        self.children.push(XmlNode::Element(child));
    }

    fn append_text(&mut self, text: impl Into<String>) {
        self.children.push(XmlNode::Text(text.into()));
    }

    fn serialize(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.tag);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            escape_attr(v, out);
            out.push('"');
        }
        if self.children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for child in &self.children {
            match child {
                XmlNode::Element(el) => el.serialize(out),
                XmlNode::Text(t) => escape_text(t, out),
            }
        }
        out.push_str("</");
        out.push_str(&self.tag);
        out.push('>');
    }

    fn inner_xml(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            match child {
                XmlNode::Element(el) => el.serialize(&mut out),
                XmlNode::Text(t) => escape_text(t, &mut out),
            }
        }
        out
    }
}

fn escape_text(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
}

fn escape_attr(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
}

fn elem_text(tag: &str, text: impl Into<String>) -> XmlElement {
    let mut el = XmlElement::new(tag);
    el.append_text(text);
    el
}

fn elem_bool(tag: &str, value: bool) -> XmlElement {
    elem_text(tag, if value { "1" } else { "0" })
}

fn elem_num(tag: &str, value: f64) -> XmlElement {
    elem_text(tag, math::js_number_to_string(value))
}

// ---------------------------------------------------------------------------
// Export state helpers: ExternalFileHandler, StyleCache, IdManager
// ---------------------------------------------------------------------------

struct ExternalFileHandler {
    files: BTreeMap<String, Vec<u8>>,
    count: usize,
    model_callback: Option<ExportKmlModelCallback>,
}

impl ExternalFileHandler {
    fn new(model_callback: Option<ExportKmlModelCallback>) -> Self {
        Self { files: BTreeMap::new(), count: 0, model_callback }
    }

    fn texture(&mut self, val: &DynamicValue) -> Result<String, RuntimeError> {
        static IMAGE_TYPE_RE: OnceLock<Regex> = OnceLock::new();
        let url = match val {
            DynamicValue::String(s) => s.clone(),
            DynamicValue::Resource(r) => r.url(),
            _ => return Ok(String::new()),
        };
        if !is_data_uri(&url) {
            return Ok(url);
        }
        let re = IMAGE_TYPE_RE.get_or_init(|| Regex::new(r"^data:image/([^,;]+)").expect("image type regex"));
        self.count += 1;
        let mut filename = format!("texture_{}", self.count);
        if let Some(caps) = re.captures(&url) {
            filename.push('.');
            filename.push_str(&caps[1]);
        }
        let resp = data_uri_response(&url).map_err(|e| RuntimeError::new(e.to_string()))?;
        self.files.insert(filename.clone(), resp.body);
        Ok(filename)
    }

    fn model(&mut self, model: &ModelGraphics, time: &JulianDate) -> Result<String, RuntimeError> {
        let Some(callback) = self.model_callback.as_ref() else {
            return Err(RuntimeError::new(
                "Encountered a model entity while exporting to KML, but no model callback was supplied.",
            ));
        };
        let mut extra = BTreeMap::new();
        let url = callback(model, time, &mut extra)?;
        for (k, v) in extra {
            self.files.insert(k, v);
        }
        Ok(url)
    }
}

struct StyleCache {
    ids: HashMap<String, String>,
    styles: Vec<XmlElement>,
    count: usize,
}

impl StyleCache {
    fn new() -> Self {
        Self { ids: HashMap::new(), styles: Vec::new(), count: 0 }
    }

    fn get(&mut self, mut element: XmlElement) -> String {
        let key = element.inner_xml();
        if let Some(existing) = self.ids.get(&key) {
            return existing.clone();
        }
        self.count += 1;
        let style_id = format!("style-{}", self.count);
        element.set_attr("id", &style_id);
        let style_ref = format!("#{style_id}");
        self.ids.insert(key, style_ref.clone());
        self.styles.push(element);
        style_ref
    }

    fn save(self, parent: &mut XmlElement) {
        let mut new_children = Vec::with_capacity(self.styles.len() + parent.children.len());
        for style in self.styles {
            new_children.push(XmlNode::Element(style));
        }
        new_children.append(&mut parent.children);
        parent.children = new_children;
    }
}

struct IdManager {
    ids: HashMap<String, usize>,
}

impl IdManager {
    fn new() -> Self {
        Self { ids: HashMap::new() }
    }

    fn get(&mut self, id: Option<&str>) -> String {
        let base = match id {
            Some(s) => s.to_owned(),
            None => create_guid(),
        };
        match self.ids.get_mut(&base) {
            None => {
                self.ids.insert(base.clone(), 0);
                base
            }
            Some(count) => {
                *count += 1;
                format!("{base}-{}", *count)
            }
        }
    }
}

struct ExportState {
    ellipsoid: Ellipsoid,
    id_manager: IdManager,
    style_cache: StyleCache,
    external_files: ExternalFileHandler,
    time: JulianDate,
    sample_duration: f64,
    default_availability: Rc<PropertyIntervals<()>>,
}

/// Exports an [`EntityCollection`] as KML 2.2 XML (and external files) or KMZ bytes.
pub fn export_kml(options: ExportKmlOptions) -> Result<ExportKmlResult, RuntimeError> {
    let entity_availability = options.entities.compute_availability();
    let time = options.time.unwrap_or(entity_availability.start);

    let mut default_interval = options.default_availability.unwrap_or(entity_availability);
    let sample_duration = options.sample_duration;
    if default_interval.start == iso8601::minimum_value() {
        if default_interval.stop == iso8601::maximum_value() {
            default_interval = TimeInterval::default();
        } else {
            default_interval.start = default_interval.stop.add_seconds(-10.0 * sample_duration);
        }
    } else if default_interval.stop == iso8601::maximum_value() {
        default_interval.stop = default_interval.start.add_seconds(10.0 * sample_duration);
    }

    let default_availability = Rc::new(PropertyIntervals::new(|a: &Option<()>, b: &Option<()>| a == b));
    default_availability.add_interval(default_interval);

    let mut state = ExportState {
        ellipsoid: options.ellipsoid,
        id_manager: IdManager::new(),
        style_cache: StyleCache::new(),
        external_files: ExternalFileHandler::new(options.model_callback),
        time,
        sample_duration,
        default_availability,
    };

    let root_entities: Vec<EntityRef> =
        options.entities.values().into_iter().filter(|e| e.parent().is_none()).collect();

    let mut kml_el = XmlElement::new("kml");
    kml_el.set_attr("xmlns", KML_NAMESPACE);
    kml_el.set_attr("xmlns:gx", GX_NAMESPACE);

    let mut doc_el = XmlElement::new("Document");
    recurse_entities(&mut state, &mut doc_el, &root_entities)?;
    state.style_cache.save(&mut doc_el);
    kml_el.append_child(doc_el);

    let mut kml_string = String::new();
    kml_el.serialize(&mut kml_string);

    if options.kmz {
        let mut zip_entries: Vec<(&str, &[u8])> = Vec::with_capacity(1 + state.external_files.files.len());
        zip_entries.push(("doc.kml", kml_string.as_bytes()));
        for (name, bytes) in &state.external_files.files {
            zip_entries.push((name.as_str(), bytes.as_slice()));
        }
        let kmz = write_zip(&zip_entries)?;
        Ok(ExportKmlResult::Kmz(kmz))
    } else {
        Ok(ExportKmlResult::Kml { kml: kml_string, external_files: state.external_files.files })
    }
}

fn entity_description(entity: &EntityRef, time: &JulianDate) -> Result<String, RuntimeError> {
    match eval_dynamic(entity.get_property("description").as_ref(), time)? {
        Some(DynamicValue::String(s)) => Ok(s),
        Some(other) => Ok(crate::data_sources::PropertyValue::property_string(&other)),
        None => Ok(String::new()),
    }
}

fn recurse_entities(
    state: &mut ExportState,
    parent_node: &mut XmlElement,
    entities: &[EntityRef],
) -> Result<(), RuntimeError> {
    for entity in entities {
        let mut overlays = Vec::new();
        let mut geometries = Vec::new();
        let mut styles = Vec::new();

        create_point(state, entity, &mut geometries, &mut styles)?;
        if let Some(pl_comp) = entity.get_property("polyline").and_then(|p| p.component()) {
            if let Some(pl) = pl_comp.as_any().downcast_ref::<PolylineGraphics>() {
                create_line_string(state, pl, &mut geometries, &mut styles)?;
            }
        }
        if let Some(rect_comp) = entity.get_property("rectangle").and_then(|p| p.component()) {
            if let Some(rect) = rect_comp.as_any().downcast_ref::<RectangleGraphics>() {
                create_rectangle_or_overlay(state, rect, &mut geometries, &mut styles, &mut overlays)?;
            }
        }
        if let Some(pg_comp) = entity.get_property("polygon").and_then(|p| p.component()) {
            if let Some(pg) = pg_comp.as_any().downcast_ref::<PolygonGraphics>() {
                create_polygon(state, pg, &mut geometries, &mut styles)?;
            }
        }
        if let Some(m_comp) = entity.get_property("model").and_then(|p| p.component()) {
            if let Some(model) = m_comp.as_any().downcast_ref::<ModelGraphics>() {
                create_model(state, entity, model, &mut geometries, &mut styles)?;
            }
        }

        let time_span = entity.availability().map(|avail| {
            let r = avail.read();
            let mut ts = XmlElement::new("TimeSpan");
            if let Some(start) = r.start() {
                if start != iso8601::minimum_value() {
                    ts.append_child(elem_text("begin", start.to_iso8601(None)));
                }
            }
            if let Some(stop) = r.stop() {
                if stop != iso8601::maximum_value() {
                    ts.append_child(elem_text("end", stop.to_iso8601(None)));
                }
            }
            ts
        });

        let desc = entity_description(entity, &state.time)?;
        let base_name = entity.name().unwrap_or_default();

        for mut overlay in overlays {
            overlay.set_attr("id", state.id_manager.get(Some(entity.id())));
            overlay.append_child(elem_text("name", &base_name));
            overlay.append_child(elem_bool("visibility", entity.show()));
            overlay.append_child(elem_text("description", &desc));
            if let Some(ts) = &time_span {
                overlay.append_child(ts.clone());
            }
            parent_node.append_child(overlay);
        }

        if !geometries.is_empty() {
            let mut placemark = XmlElement::new("Placemark");
            placemark.set_attr("id", state.id_manager.get(Some(entity.id())));

            let mut name = base_name.clone();
            if let Some(lbl_comp) = entity.get_property("label").and_then(|p| p.component()) {
                if let Some(label) = lbl_comp.as_any().downcast_ref::<super::LabelGraphics>() {
                    let mut label_style = XmlElement::new("LabelStyle");
                    if let Some(DynamicValue::String(text)) = eval_dynamic(label.text().as_ref(), &state.time)? {
                        if !text.is_empty() {
                            name = text;
                        }
                    }
                    if let Some(color) = eval_color_opt(label.fill_color().as_ref(), &state.time)? {
                        label_style.append_child(elem_text("color", color_to_string(color)));
                        label_style.append_child(elem_text("colorMode", "normal"));
                    }
                    if let Some(scale) = eval_f64_opt(label.scale().as_ref(), &state.time)? {
                        label_style.append_child(elem_num("scale", scale));
                    }
                    styles.push(label_style);
                }
            }

            placemark.append_child(elem_text("name", &name));
            placemark.append_child(elem_bool("visibility", entity.show()));
            placemark.append_child(elem_text("description", &desc));
            if let Some(ts) = &time_span {
                placemark.append_child(ts.clone());
            }

            if !styles.is_empty() {
                let mut style = XmlElement::new("Style");
                for s in styles {
                    style.append_child(s);
                }
                let style_url = state.style_cache.get(style);
                placemark.append_child(elem_text("styleUrl", style_url));
            }

            if geometries.len() == 1 {
                placemark.append_child(geometries.into_iter().next().expect("single geometry"));
            } else {
                let mut multi = XmlElement::new("MultiGeometry");
                for g in geometries {
                    multi.append_child(g);
                }
                placemark.append_child(multi);
            }

            parent_node.append_child(placemark);
        }

        let children = entity.children();
        if !children.is_empty() {
            let mut folder = XmlElement::new("Folder");
            folder.set_attr("id", state.id_manager.get(Some(entity.id())));
            folder.append_child(elem_text("name", &base_name));
            folder.append_child(elem_bool("visibility", entity.show()));
            folder.append_child(elem_text("description", &desc));
            recurse_entities(state, &mut folder, &children)?;
            parent_node.append_child(folder);
        }
    }
    Ok(())
}

enum PointGraphicKind<'a> {
    Billboard(&'a BillboardGraphics),
    Point(&'a PointGraphics),
    Model(&'a ModelGraphics),
    PathOnly,
}

fn create_point(
    state: &mut ExportState,
    entity: &EntityRef,
    geometries: &mut Vec<XmlElement>,
    styles: &mut Vec<XmlElement>,
) -> Result<(), RuntimeError> {
    let bb_comp = entity.get_property("billboard").and_then(|p| p.component());
    let pt_comp = entity.get_property("point").and_then(|p| p.component());
    let path_comp = entity.get_property("path").and_then(|p| p.component());

    let bb = bb_comp.as_ref().and_then(|c| c.as_any().downcast_ref::<BillboardGraphics>());
    let pt = pt_comp.as_ref().and_then(|c| c.as_any().downcast_ref::<PointGraphics>());
    let has_path = path_comp.as_ref().is_some_and(|c| c.as_any().is::<PathGraphics>());

    let kind = if let Some(b) = bb {
        PointGraphicKind::Billboard(b)
    } else if let Some(p) = pt {
        PointGraphicKind::Point(p)
    } else if has_path {
        PointGraphicKind::PathOnly
    } else {
        return Ok(());
    };

    let Some(pos_prop) = entity.position() else {
        return Ok(());
    };

    if !pos_prop.is_constant() {
        return create_tracks(state, entity, &pos_prop, kind, geometries, styles);
    }

    let pos = pos_prop.get_value(Some(&state.time), None)?.unwrap_or(Cartesian3::ZERO);
    let mut point_geom = XmlElement::new("Point");
    let hr = match kind {
        PointGraphicKind::Billboard(b) => {
            eval_height_reference(b.height_reference().as_ref(), &state.time, HeightReference::None)?
        }
        PointGraphicKind::Point(p) => {
            eval_height_reference(p.height_reference().as_ref(), &state.time, HeightReference::None)?
        }
        _ => HeightReference::None,
    };
    point_geom.append_child(elem_text("altitudeMode", altitude_mode_str(hr)));
    point_geom.append_child(elem_text("coordinates", get_coordinates(&[pos], &state.ellipsoid)));
    geometries.push(point_geom);

    match kind {
        PointGraphicKind::Billboard(b) => styles.push(create_icon_style_from_billboard(state, b)?),
        PointGraphicKind::Point(p) => styles.push(create_icon_style_from_point(state, p)?),
        _ => {}
    }

    Ok(())
}

fn create_tracks(
    state: &mut ExportState,
    entity: &EntityRef,
    pos_prop: &PositionPropertyRef,
    kind: PointGraphicKind<'_>,
    geometries: &mut Vec<XmlElement>,
    styles: &mut Vec<XmlElement>,
) -> Result<(), RuntimeError> {
    let mut interval_props: Vec<(TimeInterval<()>, PositionPropertyRef)> = Vec::new();
    if let Some(comp) = pos_prop.as_any().downcast_ref::<CompositePositionProperty>() {
        for iv in comp.intervals().read().intervals() {
            if let Some(data) = &iv.data {
                interval_props.push((
                    TimeInterval::new(iv.start, iv.stop, iv.is_start_included, iv.is_stop_included, None),
                    data.clone(),
                ));
            }
        }
    } else {
        let avail = entity.availability().unwrap_or_else(|| state.default_availability.clone());
        for iv in avail.read().intervals() {
            interval_props.push((iv.clone(), pos_prop.clone()));
        }
    }

    let mut tracks = Vec::new();
    for (interval, prop) in interval_props {
        let is_scaled = prop.as_any().is::<ScaledPositionProperty>();
        let hr = if is_scaled {
            HeightReference::ClampToGround
        } else {
            match kind {
                PointGraphicKind::Billboard(b) => {
                    eval_height_reference(b.height_reference().as_ref(), &state.time, HeightReference::None)?
                }
                PointGraphicKind::Point(p) => {
                    eval_height_reference(p.height_reference().as_ref(), &state.time, HeightReference::None)?
                }
                PointGraphicKind::Model(m) => {
                    eval_height_reference(m.height_reference().as_ref(), &state.time, HeightReference::None)?
                }
                PointGraphicKind::PathOnly => HeightReference::None,
            }
        };

        let mut pos_times = Vec::new();
        let mut pos_values = Vec::new();

        if prop.is_constant() {
            let pos = prop.get_value(Some(&state.time), None)?.unwrap_or(Cartesian3::ZERO);
            let coords = get_coordinates(&[pos], &state.ellipsoid);
            pos_times.push(interval.start.to_iso8601(None));
            pos_values.push(coords.clone());
            pos_times.push(interval.stop.to_iso8601(None));
            pos_values.push(coords);
        } else if let Some(sampled) = prop.as_any().downcast_ref::<SampledPositionProperty>() {
            for t in sampled.times() {
                pos_times.push(t.to_iso8601(None));
                let pos =
                    sampled.get_value_in_reference_frame(&t, ReferenceFrame::Fixed, None)?.unwrap_or(Cartesian3::ZERO);
                pos_values.push(get_coordinates(&[pos], &state.ellipsoid));
            }
        } else {
            let duration = state.sample_duration;
            let mut cursor = interval.start;
            if !interval.is_start_included {
                cursor = cursor.add_seconds(duration);
            }
            while JulianDate::less_than(&cursor, &interval.stop) {
                if let Some(pos) = prop.get_value(Some(&cursor), None)? {
                    pos_times.push(cursor.to_iso8601(None));
                    pos_values.push(get_coordinates(&[pos], &state.ellipsoid));
                }
                cursor = cursor.add_seconds(duration);
            }
            if interval.is_stop_included && cursor == interval.stop {
                if let Some(pos) = prop.get_value(Some(&cursor), None)? {
                    pos_times.push(cursor.to_iso8601(None));
                    pos_values.push(get_coordinates(&[pos], &state.ellipsoid));
                }
            }
        }

        let mut track_geom = XmlElement::new("gx:Track");
        track_geom.append_child(elem_text("altitudeMode", altitude_mode_str(hr)));
        for (t, coord) in pos_times.into_iter().zip(pos_values) {
            track_geom.append_child(elem_text("when", t));
            track_geom.append_child(elem_text("gx:coord", coord));
        }
        if let PointGraphicKind::Model(model) = kind {
            track_geom.append_child(create_model_geometry(state, model)?);
        }
        tracks.push(track_geom);
    }

    if tracks.len() == 1 {
        geometries.push(tracks.into_iter().next().expect("single track"));
    } else if tracks.len() > 1 {
        let mut multi = XmlElement::new("gx:MultiTrack");
        for t in tracks {
            multi.append_child(t);
        }
        geometries.push(multi);
    }

    match kind {
        PointGraphicKind::Billboard(b) => styles.push(create_icon_style_from_billboard(state, b)?),
        PointGraphicKind::Point(p) => styles.push(create_icon_style_from_point(state, p)?),
        _ => {}
    }

    if let Some(path_comp) = entity.get_property("path").and_then(|p| p.component()) {
        if let Some(path) = path_comp.as_any().downcast_ref::<PathGraphics>() {
            let width = eval_f64_opt(path.width().as_ref(), &state.time)?;
            let material = path.material();
            if material.is_some() || width.is_some() {
                let mut line_style = XmlElement::new("LineStyle");
                if let Some(w) = width {
                    line_style.append_child(elem_num("width", w));
                }
                process_material(state, material.as_ref(), &mut line_style)?;
                styles.push(line_style);
            }
        }
    }

    Ok(())
}

fn create_icon_style_from_point(state: &ExportState, point: &PointGraphics) -> Result<XmlElement, RuntimeError> {
    let mut icon_style = XmlElement::new("IconStyle");
    if let Some(color) = eval_color_opt(point.color().as_ref(), &state.time)? {
        icon_style.append_child(elem_text("color", color_to_string(color)));
        icon_style.append_child(elem_text("colorMode", "normal"));
    }
    if let Some(pixel_size) = eval_f64_opt(point.pixel_size().as_ref(), &state.time)? {
        icon_style.append_child(elem_num("scale", pixel_size / BILLBOARD_SIZE));
    }
    Ok(icon_style)
}

fn create_icon_style_from_billboard(
    state: &mut ExportState,
    billboard: &BillboardGraphics,
) -> Result<XmlElement, RuntimeError> {
    let mut icon_style = XmlElement::new("IconStyle");

    if let Some(img_val) = eval_dynamic(billboard.image().as_ref(), &state.time)? {
        let href = state.external_files.texture(&img_val)?;
        let mut icon = XmlElement::new("Icon");
        icon.append_child(elem_text("href", href));
        if let Some(sub) = eval_bounding_rectangle_opt(billboard.image_sub_region().as_ref(), &state.time)? {
            icon.append_child(elem_num("gx:x", sub.x));
            icon.append_child(elem_num("gx:y", sub.y));
            icon.append_child(elem_num("gx:w", sub.width));
            icon.append_child(elem_num("gx:h", sub.height));
        }
        icon_style.append_child(icon);
    }

    if let Some(color) = eval_color_opt(billboard.color().as_ref(), &state.time)? {
        icon_style.append_child(elem_text("color", color_to_string(color)));
        icon_style.append_child(elem_text("colorMode", "normal"));
    }

    let scale_opt = eval_f64_opt(billboard.scale().as_ref(), &state.time)?;
    if let Some(scale) = scale_opt {
        icon_style.append_child(elem_num("scale", scale));
    }

    if let Some(mut offset) = eval_cartesian2_opt(billboard.pixel_offset().as_ref(), &state.time)? {
        let scale = scale_opt.unwrap_or(1.0);
        offset = offset.divide_by_scalar(scale);
        let width = eval_f64(billboard.width().as_ref(), &state.time, BILLBOARD_SIZE)?;
        let height = eval_f64(billboard.height().as_ref(), &state.time, BILLBOARD_SIZE)?;

        let h_origin =
            eval_horizontal_origin(billboard.horizontal_origin().as_ref(), &state.time, HorizontalOrigin::Center)?;
        match h_origin {
            HorizontalOrigin::Center => offset.x -= width * 0.5,
            HorizontalOrigin::Right => offset.x -= width,
            HorizontalOrigin::Left => {}
        }

        let v_origin = eval_vertical_origin(billboard.vertical_origin().as_ref(), &state.time, VerticalOrigin::Center)?;
        match v_origin {
            VerticalOrigin::Top => offset.y += height,
            VerticalOrigin::Center => offset.y += height * 0.5,
            VerticalOrigin::Bottom | VerticalOrigin::Baseline => {}
        }

        let mut hot_spot = XmlElement::new("hotSpot");
        hot_spot.set_attr("x", math::js_number_to_string(-offset.x));
        hot_spot.set_attr("y", math::js_number_to_string(offset.y));
        hot_spot.set_attr("xunits", "pixels");
        hot_spot.set_attr("yunits", "pixels");
        icon_style.append_child(hot_spot);
    }

    if let Some(rot) = eval_f64_opt(billboard.rotation().as_ref(), &state.time)? {
        if let Some(axis) = eval_cartesian3_opt(billboard.aligned_axis().as_ref(), &state.time)? {
            if axis == Cartesian3::UNIT_Z {
                let mut deg = math::to_degrees(-rot);
                if deg == 0.0 {
                    deg = 360.0;
                }
                icon_style.append_child(elem_num("heading", deg));
            }
        }
    }

    Ok(icon_style)
}

fn create_line_string(
    state: &ExportState,
    polyline: &PolylineGraphics,
    geometries: &mut Vec<XmlElement>,
    styles: &mut Vec<XmlElement>,
) -> Result<(), RuntimeError> {
    let mut line_geom = XmlElement::new("LineString");
    let clamp = eval_bool(polyline.clamp_to_ground().as_ref(), &state.time, false)?;
    if clamp {
        line_geom.append_child(elem_bool("tessellate", true));
        line_geom.append_child(elem_text("altitudeMode", "clampToGround"));
    } else {
        line_geom.append_child(elem_text("altitudeMode", "absolute"));
    }

    let positions = eval_positions(polyline.positions().as_ref(), &state.time)?;
    line_geom.append_child(elem_text("coordinates", get_coordinates(&positions, &state.ellipsoid)));

    if clamp {
        if let Some(z_index) = eval_f64_opt(polyline.z_index().as_ref(), &state.time)? {
            line_geom.append_child(elem_num("gx:drawOrder", z_index));
        }
    }
    geometries.push(line_geom);

    let mut line_style = XmlElement::new("LineStyle");
    if let Some(width) = eval_f64_opt(polyline.width().as_ref(), &state.time)? {
        line_style.append_child(elem_num("width", width));
    }
    process_material(state, polyline.material().as_ref(), &mut line_style)?;
    styles.push(line_style);
    Ok(())
}

fn create_rectangle_or_overlay(
    state: &mut ExportState,
    rect_gfx: &RectangleGraphics,
    geometries: &mut Vec<XmlElement>,
    styles: &mut Vec<XmlElement>,
    overlays: &mut Vec<XmlElement>,
) -> Result<(), RuntimeError> {
    let mat_type = material_type(rect_gfx.material().as_ref(), &state.time)?;
    if mat_type.as_deref() == Some("Image") {
        create_ground_overlay(state, rect_gfx, overlays)?;
        return Ok(());
    }

    let mut poly_geom = XmlElement::new("Polygon");
    let extruded_height = eval_f64(rect_gfx.extruded_height().as_ref(), &state.time, 0.0)?;
    if extruded_height > 0.0 {
        poly_geom.append_child(elem_bool("extrude", true));
    }

    let mut height = eval_f64(rect_gfx.height().as_ref(), &state.time, 0.0)?;
    if extruded_height > 0.0 {
        height = extruded_height;
    }

    let rect = eval_rectangle(rect_gfx.coordinates().as_ref(), &state.time)?.unwrap_or_default();
    let corners = [rect.northeast(), rect.southeast(), rect.southwest(), rect.northwest()];
    let coord_str = corners
        .iter()
        .map(|c| {
            format!(
                "{},{},{}",
                math::js_number_to_string(math::to_degrees(c.longitude)),
                math::js_number_to_string(math::to_degrees(c.latitude)),
                math::js_number_to_string(height),
            )
        })
        .collect::<Vec<_>>()
        .join(" ");

    let mut outer = XmlElement::new("outerBoundaryIs");
    let mut ring = XmlElement::new("LinearRing");
    ring.append_child(elem_text("coordinates", coord_str));
    outer.append_child(ring);
    poly_geom.append_child(outer);

    let hr = eval_height_reference(rect_gfx.height_reference().as_ref(), &state.time, HeightReference::None)?;
    poly_geom.append_child(elem_text("altitudeMode", altitude_mode_str(hr)));
    geometries.push(poly_geom);

    append_poly_style(
        state,
        rect_gfx.fill().as_ref(),
        rect_gfx.material().as_ref(),
        rect_gfx.outline().as_ref(),
        rect_gfx.outline_width().as_ref(),
        rect_gfx.outline_color().as_ref(),
        styles,
    )?;
    Ok(())
}

fn create_ground_overlay(
    state: &mut ExportState,
    rect_gfx: &RectangleGraphics,
    overlays: &mut Vec<XmlElement>,
) -> Result<(), RuntimeError> {
    let mut overlay = XmlElement::new("GroundOverlay");
    let hr = eval_height_reference(rect_gfx.height_reference().as_ref(), &state.time, HeightReference::None)?;
    overlay.append_child(elem_text("altitudeMode", altitude_mode_str(hr)));

    if let Some(height) = eval_f64_opt(rect_gfx.height().as_ref(), &state.time)? {
        overlay.append_child(elem_num("altitude", height));
    }

    let rect = eval_rectangle(rect_gfx.coordinates().as_ref(), &state.time)?.unwrap_or_default();
    let mut box_el = XmlElement::new("LatLonBox");
    box_el.append_child(elem_num("north", math::to_degrees(rect.north)));
    box_el.append_child(elem_num("south", math::to_degrees(rect.south)));
    box_el.append_child(elem_num("east", math::to_degrees(rect.east)));
    box_el.append_child(elem_num("west", math::to_degrees(rect.west)));
    overlay.append_child(box_el);

    if let Some(mat_val) = eval_dynamic(rect_gfx.material().as_ref(), &state.time)? {
        if let Some(img_val) = mat_val.get("image") {
            let href = state.external_files.texture(&img_val)?;
            let mut icon = XmlElement::new("Icon");
            icon.append_child(elem_text("href", href));
            overlay.append_child(icon);
        }
        if let Some(color_val) = mat_val.get("color").and_then(|v| dynamic_to_color(&v)) {
            overlay.append_child(elem_text("color", color_to_string(color_val)));
        }
    }

    overlays.push(overlay);
    Ok(())
}

fn make_linear_ring(positions: &[Cartesian3], ellipsoid: &Ellipsoid, height: f64, per_pos_height: bool) -> XmlElement {
    let coords = positions
        .iter()
        .map(|p| {
            let c = ellipsoid.cartesian_to_cartographic(*p).unwrap_or_default();
            let h = if per_pos_height { c.height } else { height };
            format!(
                "{},{},{}",
                math::js_number_to_string(math::to_degrees(c.longitude)),
                math::js_number_to_string(math::to_degrees(c.latitude)),
                math::js_number_to_string(h),
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut ring = XmlElement::new("LinearRing");
    ring.append_child(elem_text("coordinates", coords));
    ring
}

fn create_polygon(
    state: &ExportState,
    polygon: &PolygonGraphics,
    geometries: &mut Vec<XmlElement>,
    styles: &mut Vec<XmlElement>,
) -> Result<(), RuntimeError> {
    let mut poly_geom = XmlElement::new("Polygon");
    let extruded_height = eval_f64(polygon.extruded_height().as_ref(), &state.time, 0.0)?;
    if extruded_height > 0.0 {
        poly_geom.append_child(elem_bool("extrude", true));
    }

    let mut height = eval_f64(polygon.height().as_ref(), &state.time, 0.0)?;
    let per_pos_height = eval_bool(polygon.per_position_height().as_ref(), &state.time, false)?;
    if !per_pos_height && extruded_height > 0.0 {
        height = extruded_height;
    }

    let hierarchy = eval_polygon_hierarchy(polygon.hierarchy().as_ref(), &state.time)?;
    let mut outer = XmlElement::new("outerBoundaryIs");
    outer.append_child(make_linear_ring(&hierarchy.positions, &state.ellipsoid, height, per_pos_height));
    poly_geom.append_child(outer);

    for hole in &hierarchy.holes {
        let mut inner = XmlElement::new("innerBoundaryIs");
        inner.append_child(make_linear_ring(&hole.positions, &state.ellipsoid, height, per_pos_height));
        poly_geom.append_child(inner);
    }

    let hr = eval_height_reference(polygon.height_reference().as_ref(), &state.time, HeightReference::None)?;
    poly_geom.append_child(elem_text("altitudeMode", altitude_mode_str(hr)));
    geometries.push(poly_geom);

    append_poly_style(
        state,
        polygon.fill().as_ref(),
        polygon.material().as_ref(),
        polygon.outline().as_ref(),
        polygon.outline_width().as_ref(),
        polygon.outline_color().as_ref(),
        styles,
    )?;
    Ok(())
}

fn append_poly_style(
    state: &ExportState,
    fill_prop: Option<&PropertyInput>,
    material_prop: Option<&PropertyInput>,
    outline_prop: Option<&PropertyInput>,
    outline_width_prop: Option<&PropertyInput>,
    outline_color_prop: Option<&PropertyInput>,
    styles: &mut Vec<XmlElement>,
) -> Result<(), RuntimeError> {
    let mut poly_style = XmlElement::new("PolyStyle");
    let fill = eval_bool(fill_prop, &state.time, false)?;
    if fill {
        poly_style.append_child(elem_bool("fill", true));
    }
    process_material(state, material_prop, &mut poly_style)?;

    let outline = eval_bool(outline_prop, &state.time, false)?;
    if outline {
        poly_style.append_child(elem_bool("outline", true));
        let mut line_style = XmlElement::new("LineStyle");
        let outline_width = eval_f64(outline_width_prop, &state.time, 1.0)?;
        line_style.append_child(elem_num("width", outline_width));
        let outline_color = eval_color_opt(outline_color_prop, &state.time)?.unwrap_or(Color::BLACK);
        line_style.append_child(elem_text("color", color_to_string(outline_color)));
        line_style.append_child(elem_text("colorMode", "normal"));
        styles.push(line_style);
    }
    styles.push(poly_style);
    Ok(())
}

fn create_model_geometry(state: &mut ExportState, model: &ModelGraphics) -> Result<XmlElement, RuntimeError> {
    let mut model_geom = XmlElement::new("Model");
    if let Some(scale) = eval_f64_opt(model.scale().as_ref(), &state.time)? {
        let mut scale_el = XmlElement::new("scale");
        scale_el.append_child(elem_num("x", scale));
        scale_el.append_child(elem_num("y", scale));
        scale_el.append_child(elem_num("z", scale));
        model_geom.append_child(scale_el);
    }
    let uri = state.external_files.model(model, &state.time)?;
    let mut link = XmlElement::new("Link");
    link.append_child(elem_text("href", uri));
    model_geom.append_child(link);
    Ok(model_geom)
}

fn create_model(
    state: &mut ExportState,
    entity: &EntityRef,
    model: &ModelGraphics,
    geometries: &mut Vec<XmlElement>,
    styles: &mut Vec<XmlElement>,
) -> Result<(), RuntimeError> {
    let Some(pos_prop) = entity.position() else {
        return Ok(());
    };
    if !pos_prop.is_constant() {
        return create_tracks(state, entity, &pos_prop, PointGraphicKind::Model(model), geometries, styles);
    }

    let mut model_geom = create_model_geometry(state, model)?;
    let hr = eval_height_reference(model.height_reference().as_ref(), &state.time, HeightReference::None)?;
    model_geom.append_child(elem_text("altitudeMode", altitude_mode_str(hr)));

    let pos = pos_prop.get_value(Some(&state.time), None)?.unwrap_or(Cartesian3::ZERO);
    let carto = state.ellipsoid.cartesian_to_cartographic(pos).unwrap_or_default();
    let mut loc = XmlElement::new("Location");
    loc.append_child(elem_num("longitude", math::to_degrees(carto.longitude)));
    loc.append_child(elem_num("latitude", math::to_degrees(carto.latitude)));
    loc.append_child(elem_num("altitude", carto.height));
    model_geom.append_child(loc);

    geometries.push(model_geom);
    Ok(())
}

fn material_type(material_prop: Option<&PropertyInput>, time: &JulianDate) -> Result<Option<String>, RuntimeError> {
    match material_prop {
        Some(PropertyInput::Component(c)) => c.get_type(time),
        Some(PropertyInput::Value(DynamicValue::Color(_))) => Ok(Some("Color".to_owned())),
        _ => Ok(None),
    }
}

fn process_material(
    state: &ExportState,
    material_prop: Option<&PropertyInput>,
    style: &mut XmlElement,
) -> Result<(), RuntimeError> {
    let Some(material_prop) = material_prop else {
        return Ok(());
    };
    let Some(material) = eval_dynamic(Some(material_prop), &state.time)? else {
        return Ok(());
    };
    let Some(kind) = material_type(Some(material_prop), &state.time)? else {
        return Ok(());
    };

    let color: Option<Color> = match kind.as_str() {
        "Image" => Some(Color::WHITE),
        "Color" | "Grid" | "PolylineGlow" | "PolylineArrow" | "PolylineDash" => {
            material.get("color").and_then(|v| dynamic_to_color(&v)).or_else(|| dynamic_to_color(&material))
        }
        "PolylineOutline" => {
            if let Some(oc) = material.get("outlineColor").and_then(|v| dynamic_to_color(&v)) {
                style.append_child(elem_text("gx:outerColor", color_to_string(oc)));
            }
            if let Some(DynamicValue::Number(ow)) = material.get("outlineWidth") {
                style.append_child(elem_num("gx:outerWidth", ow));
            }
            material.get("color").and_then(|v| dynamic_to_color(&v))
        }
        "Stripe" => material.get("oddColor").and_then(|v| dynamic_to_color(&v)),
        _ => None,
    };

    if let Some(c) = color {
        style.append_child(elem_text("color", color_to_string(c)));
        style.append_child(elem_text("colorMode", "normal"));
    }
    Ok(())
}

fn altitude_mode_str(hr: HeightReference) -> &'static str {
    match hr {
        HeightReference::None => "absolute",
        HeightReference::ClampToGround | HeightReference::ClampToTerrain | HeightReference::ClampTo3DTile => {
            "clampToGround"
        }
        HeightReference::RelativeToGround | HeightReference::RelativeToTerrain | HeightReference::RelativeTo3DTile => {
            "relativeToGround"
        }
    }
}

fn get_coordinates(coordinates: &[Cartesian3], ellipsoid: &Ellipsoid) -> String {
    coordinates
        .iter()
        .map(|p| {
            let c = ellipsoid.cartesian_to_cartographic(*p).unwrap_or_default();
            format!(
                "{},{},{}",
                math::js_number_to_string(math::to_degrees(c.longitude)),
                math::js_number_to_string(math::to_degrees(c.latitude)),
                math::js_number_to_string(c.height),
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Converts a [`Color`] to KML's lowercase 8-char `aabbggrr` hex string.
pub(crate) fn color_to_string(color: Color) -> String {
    let bytes = color.to_bytes();
    let mut out = String::with_capacity(8);
    for &b in bytes.iter().rev() {
        let clamped = b.clamp(0, 255) as u8;
        out.push_str(&format!("{clamped:02x}"));
    }
    out
}

fn eval_positions(slot: Option<&PropertyInput>, time: &JulianDate) -> Result<Vec<Cartesian3>, RuntimeError> {
    match eval_dynamic(slot, time)? {
        Some(DynamicValue::Positions(p)) => Ok((*p).clone()),
        Some(DynamicValue::Json(j)) => Ok(json_to_positions(&j.value())),
        _ => Ok(Vec::new()),
    }
}

fn json_to_positions(v: &Value) -> Vec<Cartesian3> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            if let Some(a) = item.as_array() {
                if a.len() >= 3 {
                    return Some(Cartesian3::new(a[0].as_f64()?, a[1].as_f64()?, a[2].as_f64()?));
                }
            }
            dynamic_to_cartesian3(&DynamicValue::from_json(item.clone()))
        })
        .collect()
}

fn json_to_hierarchy(v: &Value) -> PolygonHierarchy {
    if v.is_array() {
        return PolygonHierarchy::new(json_to_positions(v), Vec::new());
    }
    let positions = v.get("positions").map(json_to_positions).unwrap_or_default();
    let holes = v
        .get("holes")
        .and_then(Value::as_array)
        .map(|hs| hs.iter().map(json_to_hierarchy).collect())
        .unwrap_or_default();
    PolygonHierarchy::new(positions, holes)
}

fn eval_polygon_hierarchy(slot: Option<&PropertyInput>, time: &JulianDate) -> Result<PolygonHierarchy, RuntimeError> {
    match eval_dynamic(slot, time)? {
        Some(DynamicValue::PolygonHierarchy(h)) => Ok((*h).clone()),
        Some(DynamicValue::Positions(p)) => Ok(PolygonHierarchy::new((*p).clone(), Vec::new())),
        Some(DynamicValue::Json(j)) => Ok(json_to_hierarchy(&j.value())),
        _ => Ok(PolygonHierarchy::default()),
    }
}

fn eval_rectangle(slot: Option<&PropertyInput>, time: &JulianDate) -> Result<Option<Rectangle>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Json(j)) => {
            let v = j.value();
            if let Some(arr) = v.as_array() {
                if arr.len() >= 4 {
                    return Ok(Some(Rectangle::new(
                        arr[0].as_f64().unwrap_or(0.0),
                        arr[1].as_f64().unwrap_or(0.0),
                        arr[2].as_f64().unwrap_or(0.0),
                        arr[3].as_f64().unwrap_or(0.0),
                    )));
                }
            }
            match (
                v.get("west").and_then(Value::as_f64),
                v.get("south").and_then(Value::as_f64),
                v.get("east").and_then(Value::as_f64),
                v.get("north").and_then(Value::as_f64),
            ) {
                (Some(w), Some(s), Some(e), Some(n)) => Some(Rectangle::new(w, s, e, n)),
                _ => None,
            }
        }
        Some(DynamicValue::Record(fields)) => {
            let get = |name: &str| {
                fields.iter().find(|(k, _)| k == name).and_then(|(_, v)| match v {
                    Some(DynamicValue::Number(x)) => Some(*x),
                    _ => None,
                })
            };
            match (get("west"), get("south"), get("east"), get("north")) {
                (Some(w), Some(s), Some(e), Some(n)) => Some(Rectangle::new(w, s, e, n)),
                _ => None,
            }
        }
        _ => None,
    })
}
