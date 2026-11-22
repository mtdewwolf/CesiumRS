//! CesiumJS `GpxDataSource`: loads GPS Exchange Format (GPX 1.1) waypoints,
//! routes, tracks, metadata and simulation clock into an [`EntityCollection`].
use super::{
    Availability, BillboardGraphics, ConstantPositionProperty, ConstantProperty, DataSource, DataSourceClock,
    DataSourceState, DynamicValue, EntityRef, GeoJsonPin, LabelGraphics, PinSymbol, PolylineGraphics,
    PolylineOutlineMaterialProperty, PropertyComponent, PropertyInput, PropertyIntervals, SampledPositionProperty,
};
use crate::{
    core::{
        create_guid::create_guid, iso8601, Cartesian2, Cartesian3, ClockRange, ClockStep, Color, Ellipsoid,
        JulianDate, NearFarScalar, ReferenceFrame, Resource, ResourceClient, RuntimeError, TimeInterval,
    },
    scene::{HeightReference, HorizontalOrigin, LabelStyle, VerticalOrigin},
};
use regex::Regex;
use roxmltree::{Document, Node};
use serde_json::json;
use std::{cell::RefCell, rc::Rc, sync::OnceLock};

const GPX_NAMESPACE: &str = "http://www.topografix.com/GPX/1/1";
const BILLBOARD_SIZE: f64 = 32.0;
const BILLBOARD_NEAR_DISTANCE: f64 = 2414016.0;
const BILLBOARD_NEAR_RATIO: f64 = 1.0;
const BILLBOARD_FAR_DISTANCE: f64 = 1.6093e7;
const BILLBOARD_FAR_RATIO: f64 = 0.1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpxLink {
    pub href: Option<String>,
    pub text: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpxPerson {
    pub name: Option<String>,
    pub email: Option<String>,
    pub link: Option<GpxLink>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpxCopyright {
    pub author: Option<String>,
    pub year: Option<String>,
    pub license: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpxBounds {
    pub min_lat: Option<f64>,
    pub max_lat: Option<f64>,
    pub min_lon: Option<f64>,
    pub max_lon: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpxMetadata {
    pub name: Option<String>,
    pub desc: Option<String>,
    pub author: Option<GpxPerson>,
    pub copyright: Option<GpxCopyright>,
    pub link: Option<GpxLink>,
    pub time: Option<String>,
    pub keywords: Option<String>,
    pub bounds: Option<GpxBounds>,
}

#[derive(Clone, Default)]
pub struct GpxOptions {
    pub clamp_to_ground: bool,
    pub waypoint_image: Option<String>,
    pub track_image: Option<String>,
    pub route_image: Option<String>,
    pub track_color: Option<Color>,
    pub route_color: Option<Color>,
    pub resource_client: Option<ResourceClient>,
}

pub enum GpxInput {
    Xml(String),
    Resource(Resource),
}

impl From<&str> for GpxInput {
    fn from(s: &str) -> Self {
        Self::Xml(s.to_owned())
    }
}

impl From<String> for GpxInput {
    fn from(s: String) -> Self {
        Self::Xml(s)
    }
}

impl From<Resource> for GpxInput {
    fn from(r: Resource) -> Self {
        Self::Resource(r)
    }
}

pub struct GpxDataSource {
    state: DataSourceState,
    version: RefCell<Option<String>>,
    creator: RefCell<Option<String>>,
    metadata: RefCell<Option<GpxMetadata>>,
    clock: RefCell<Option<Rc<DataSourceClock>>>,
}

impl GpxDataSource {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|owner| Self {
            state: DataSourceState::new(owner.clone(), None),
            version: RefCell::new(None),
            creator: RefCell::new(None),
            metadata: RefCell::new(None),
            clock: RefCell::new(None),
        })
    }

    pub fn version(&self) -> Option<String> {
        self.version.borrow().clone()
    }

    pub fn creator(&self) -> Option<String> {
        self.creator.borrow().clone()
    }

    pub fn metadata(&self) -> Option<GpxMetadata> {
        self.metadata.borrow().clone()
    }

    pub async fn load_new(data: impl Into<GpxInput>, options: GpxOptions) -> Result<Rc<Self>, RuntimeError> {
        let source = Self::new();
        source.load(data, options).await?;
        Ok(source)
    }

    pub async fn load(&self, data: impl Into<GpxInput>, options: GpxOptions) -> Result<(), RuntimeError> {
        self.set_loading(true)?;
        let old_name = self.name();
        let text_res = match data.into() {
            GpxInput::Xml(xml) => Ok(xml),
            GpxInput::Resource(resource) => match options.resource_client.as_ref() {
                None => Err(RuntimeError::new("GpxOptions.resource_client is required to fetch a Resource.")),
                Some(client) => {
                    match resource.fetch_text(client) {
                        Some(fut) => match fut.await {
                            Ok(Some(text)) => Ok(text),
                            Ok(None) => Err(RuntimeError::new("No GPX text returned from resource.")),
                            Err(e) => Err(RuntimeError::new(e.to_string())),
                        },
                        None => Err(RuntimeError::new("GPX resource request was deferred.")),
                    }
                }
            },
        };

        let inner_res = match text_res {
            Ok(xml) => match Document::parse(&xml) {
                Ok(doc) => self.load_gpx(&doc, &options),
                Err(e) => Err(RuntimeError::new(e.to_string())),
            },
            Err(e) => Err(e),
        };

        match inner_res {
            Ok(()) => {
                // Match upstream GpxDataSource.prototype.load `.then()` handler:
                // recomputes the clock from availability and raises changedEvent
                // if clock (new object when present) or name changed.
                let new_clock = self.compute_clock();
                let mut changed = false;
                let had_clock = self.clock.borrow().is_some();
                if new_clock.is_some() || had_clock {
                    *self.clock.borrow_mut() = new_clock;
                    changed = true;
                }
                if old_name != self.name() {
                    changed = true;
                }
                if changed {
                    self.state.notify_changed();
                }
                self.set_loading(false)?;
                Ok(())
            }
            Err(error) => {
                // Match upstream double-catch: inner `load()` raises errorEvent,
                // then outer `GpxDataSource.prototype.load` sets loading=false
                // and raises errorEvent again.
                self.state.notify_error(error.clone());
                let _ = self.set_loading(false);
                self.state.notify_error(error.clone());
                Err(error)
            }
        }
    }

    fn load_gpx(&self, doc: &Document<'_>, options: &GpxOptions) -> Result<(), RuntimeError> {
        let entities = self.entities();
        entities.remove_all();

        let element = doc.root_element();
        let version = element.attribute("version").map(str::to_owned);
        let creator = element.attribute("creator").map(str::to_owned);
        let metadata = process_metadata(element);
        let name = metadata.as_ref().and_then(|m| m.name.clone());

        if element.tag_name().name() == "gpx" {
            for type_name in ["wpt", "rte", "trk"] {
                for child in element.children().filter(|c| is_gpx_tag(*c, type_name)) {
                    match type_name {
                        "wpt" => process_wpt(child, entities, options)?,
                        "rte" => process_rte(child, entities, options)?,
                        "trk" => process_trk(child, entities, options)?,
                        _ => {}
                    }
                }
            }
        }

        let clock = self.compute_clock();
        let mut changed = false;

        if self.name() != name {
            self.state.set_name_silent(name);
            changed = true;
        }
        if *self.creator.borrow() != creator {
            *self.creator.borrow_mut() = creator;
            changed = true;
        }
        if *self.metadata.borrow() != metadata {
            *self.metadata.borrow_mut() = metadata;
            changed = true;
        }
        if *self.version.borrow() != version {
            *self.version.borrow_mut() = version;
            changed = true;
        }
        let clock_changed = !matches!((self.clock.borrow().as_ref(), clock.as_ref()), (None, None));
        if clock_changed {
            *self.clock.borrow_mut() = clock;
            changed = true;
        }
        if changed {
            self.state.notify_changed();
        }
        self.set_loading(false)?;
        Ok(())
    }

    fn compute_clock(&self) -> Option<Rc<DataSourceClock>> {
        let availability = self.entities().compute_availability();
        let start = availability.start;
        let stop = availability.stop;
        let is_min_start = start == iso8601::minimum_value();
        let is_max_stop = stop == iso8601::maximum_value();
        if is_min_start && is_max_stop {
            return None;
        }
        let clock = DataSourceClock::new();
        clock.set_start_time(Some(Rc::new(start)));
        clock.set_stop_time(Some(Rc::new(stop)));
        clock.set_current_time(Some(Rc::new(start)));
        clock.set_clock_range(Some(ClockRange::LoopStop));
        clock.set_clock_step(Some(ClockStep::SystemClockMultiplier));
        let diff = JulianDate::seconds_difference(&stop, &start);
        let mult = (diff / 60.0).clamp(1.0, 3.15569e7).round();
        clock.set_multiplier(Some(mult));
        Some(Rc::new(clock))
    }
}

impl DataSource for GpxDataSource {
    fn state(&self) -> &DataSourceState {
        &self.state
    }
    fn clock(&self) -> Option<Rc<DataSourceClock>> {
        self.clock.borrow().clone()
    }
}

fn is_gpx_namespace(node: Node<'_, '_>) -> bool {
    matches!(node.tag_name().namespace(), None | Some("") | Some(GPX_NAMESPACE))
}

fn is_gpx_tag(node: Node<'_, '_>, tag: &str) -> bool {
    node.is_element() && node.tag_name().name() == tag && is_gpx_namespace(node)
}

fn query_first_node<'a, 'input>(node: Node<'a, 'input>, tag: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|c| is_gpx_tag(*c, tag))
}

fn query_nodes<'a, 'input>(node: Node<'a, 'input>, tag: &str) -> Vec<Node<'a, 'input>> {
    node.descendants().filter(|c| is_gpx_tag(*c, tag)).collect()
}

fn query_numeric_attr(node: Node<'_, '_>, attr: &str) -> Option<f64> {
    node.attribute(attr)?.trim().parse::<f64>().ok().filter(|v| !v.is_nan())
}

fn query_string_value(node: Node<'_, '_>, tag: &str) -> Option<String> {
    let n = query_first_node(node, tag)?;
    let mut text = String::new();
    for desc in n.descendants() {
        if desc.is_text() {
            if let Some(t) = desc.text() {
                text.push_str(t);
            }
        }
    }
    Some(text.trim().to_owned())
}

fn query_numeric_value(node: Node<'_, '_>, tag: &str) -> Option<f64> {
    query_string_value(node, tag)?.parse::<f64>().ok().filter(|v| !v.is_nan())
}

fn read_coordinate(node: Node<'_, '_>) -> Cartesian3 {
    let lon = query_numeric_attr(node, "lon").unwrap_or(f64::NAN);
    let lat = query_numeric_attr(node, "lat").unwrap_or(f64::NAN);
    let ele = query_numeric_value(node, "ele").unwrap_or(0.0);
    Cartesian3::from_degrees(lon, lat, ele, &Ellipsoid::WGS84)
}

fn get_or_create_entity(node: Node<'_, '_>, entities: &super::EntityCollection) -> EntityRef {
    let id = node.attribute("id").map(str::to_owned).unwrap_or_else(create_guid);
    entities.get_or_create_entity(id)
}

fn near_far_value(s: NearFarScalar) -> PropertyInput {
    PropertyInput::Value(DynamicValue::from_json(json!({
        "near": s.near,
        "nearValue": s.near_value,
        "far": s.far,
        "farValue": s.far_value,
    })))
}

fn default_billboard_image(options: &GpxOptions) -> PropertyInput {
    if let Some(image) = &options.waypoint_image {
        PropertyInput::value(image.clone())
    } else {
        PropertyInput::Value(DynamicValue::Pin(Rc::new(GeoJsonPin {
            symbol: PinSymbol::Maki("marker".to_owned()),
            color: Color::RED,
            size: BILLBOARD_SIZE,
        })))
    }
}

fn create_default_billboard(image: PropertyInput) -> Result<Rc<BillboardGraphics>, RuntimeError> {
    let billboard = Rc::new(BillboardGraphics::new());
    billboard.set_width(Some(PropertyInput::value(BILLBOARD_SIZE)))?;
    billboard.set_height(Some(PropertyInput::value(BILLBOARD_SIZE)))?;
    let scale = NearFarScalar::new(BILLBOARD_NEAR_DISTANCE, BILLBOARD_NEAR_RATIO, BILLBOARD_FAR_DISTANCE, BILLBOARD_FAR_RATIO);
    billboard.set_scale_by_distance(Some(near_far_value(scale)))?;
    billboard.set_pixel_offset_scale_by_distance(Some(near_far_value(scale)))?;
    let vo = Rc::new(ConstantProperty::new(Some(DynamicValue::Number(VerticalOrigin::Bottom as i32 as f64))));
    billboard.set_vertical_origin(Some(PropertyInput::Component(PropertyComponent::wrap(vo))))?;
    billboard.set_image(Some(image))?;
    Ok(billboard)
}

fn create_default_label() -> Result<Rc<LabelGraphics>, RuntimeError> {
    let label = Rc::new(LabelGraphics::new());
    let translucency = NearFarScalar::new(3_000_000.0, 1.0, 5_000_000.0, 0.0);
    label.set_translucency_by_distance(Some(near_far_value(translucency)))?;
    label.set_pixel_offset(Some(PropertyInput::value(Cartesian2::new(17.0, 0.0))))?;
    label.set_horizontal_origin(Some(PropertyInput::value(HorizontalOrigin::Left as i32 as f64)))?;
    label.set_font(Some(PropertyInput::value("16px sans-serif")))?;
    label.set_style(Some(PropertyInput::value(LabelStyle::FillAndOutline as i32 as f64)))?;
    Ok(label)
}

fn create_default_polyline(color: Option<Color>) -> Result<Rc<PolylineGraphics>, RuntimeError> {
    let polyline = Rc::new(PolylineGraphics::new());
    polyline.set_width(Some(PropertyInput::value(4.0)))?;
    let mat = Rc::new(PolylineOutlineMaterialProperty::new());
    mat.set_color(Some(PropertyInput::value(color.unwrap_or(Color::RED))))?;
    mat.set_outline_width(Some(PropertyInput::value(2.0)))?;
    mat.set_outline_color(Some(PropertyInput::value(Color::BLACK)))?;
    polyline.set_material(Some(PropertyInput::Component(mat)))?;
    Ok(polyline)
}

fn autolink_text(input: &str) -> String {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let re = URL_RE.get_or_init(|| Regex::new(r#"(?i)\b((?:https?://|www\.)[^\s<>"']+)"#).unwrap());
    re.replace_all(input, |caps: &regex::Captures<'_>| {
        let matched = &caps[1];
        let href = if matched.to_ascii_lowercase().starts_with("www.") {
            format!("http://{matched}")
        } else {
            matched.to_owned()
        };
        format!(r#"<a href="{href}" target="_blank">{matched}</a>"#)
    })
    .into_owned()
}

fn process_description(node: Node<'_, '_>) -> Option<String> {
    const FIELDS: &[(&str, &str)] = &[
        ("Time", "time"),
        ("Comment", "cmt"),
        ("Description", "desc"),
        ("Source", "src"),
        ("GPS track/route number", "number"),
        ("Type", "type"),
    ];
    let mut text = String::new();
    for (label, tag) in FIELDS {
        if let Some(val) = query_string_value(node, tag) {
            if !val.is_empty() {
                text.push_str(&format!("<p>{label}: {val}</p>"));
            }
        }
    }
    if text.is_empty() {
        return None;
    }
    let linked = autolink_text(&text);
    let bg = Color::WHITE.to_css_color_string();
    let fg = Color::BLACK.to_css_color_string();
    Some(format!(
        r#"<div class="cesium-infoBox-description-lighter" style="overflow:auto;word-wrap:break-word;background-color:{bg};color:{fg};">{linked}</div>"#
    ))
}

fn process_wpt(
    node: Node<'_, '_>,
    entities: &super::EntityCollection,
    options: &GpxOptions,
) -> Result<(), RuntimeError> {
    let position = read_coordinate(node);
    let entity = get_or_create_entity(node, entities);
    entity.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(position), ReferenceFrame::Fixed))))?;

    let billboard = create_default_billboard(default_billboard_image(options))?;
    let name = query_string_value(node, "name");
    entity.set_name(name.clone());

    let label = create_default_label()?;
    if let Some(name) = name {
        label.set_text(Some(PropertyInput::value(name)))?;
    }

    if let Some(desc) = process_description(node) {
        entity.set_property("description", Some(PropertyInput::value(desc)))?;
    }

    if options.clamp_to_ground {
        let hr = PropertyInput::value(HeightReference::ClampToGround as i32 as f64);
        billboard.set_height_reference(Some(hr.clone()))?;
        label.set_height_reference(Some(hr))?;
    }

    entity.set_property("billboard", Some(PropertyInput::Component(billboard)))?;
    entity.set_property("label", Some(PropertyInput::Component(label)))?;
    Ok(())
}

fn process_rte(
    node: Node<'_, '_>,
    entities: &super::EntityCollection,
    options: &GpxOptions,
) -> Result<(), RuntimeError> {
    let entity = get_or_create_entity(node, entities);
    if let Some(desc) = process_description(node) {
        entity.set_property("description", Some(PropertyInput::value(desc)))?;
    }

    let route_points = query_nodes(node, "rtept");
    let mut coords = Vec::with_capacity(route_points.len());
    for pt in route_points {
        process_wpt(pt, entities, options)?;
        coords.push(read_coordinate(pt));
    }

    let polyline = create_default_polyline(options.route_color)?;
    if options.clamp_to_ground {
        polyline.set_clamp_to_ground(Some(PropertyInput::value(true)))?;
    }
    polyline.set_positions(Some(PropertyInput::Value(DynamicValue::Positions(Rc::new(coords)))))?;
    entity.set_property("polyline", Some(PropertyInput::Component(polyline)))?;
    Ok(())
}

fn process_trk(
    node: Node<'_, '_>,
    entities: &super::EntityCollection,
    options: &GpxOptions,
) -> Result<(), RuntimeError> {
    let entity = get_or_create_entity(node, entities);
    if let Some(desc) = process_description(node) {
        entity.set_property("description", Some(PropertyInput::value(desc)))?;
    }

    let segs = query_nodes(node, "trkseg");
    let mut positions = Vec::new();
    let mut times = Vec::new();
    let mut is_time_dynamic = true;
    let sampled = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));

    for seg in segs {
        let mut seg_positions = Vec::new();
        let mut seg_times = Vec::new();
        for pt in query_nodes(seg, "trkpt") {
            seg_positions.push(read_coordinate(pt));
            if let Some(t) = query_string_value(pt, "time") {
                seg_times.push(JulianDate::from_iso8601(&t).map_err(|e| RuntimeError::new(e.to_string()))?);
            }
        }
        positions.extend_from_slice(&seg_positions);
        if !seg_times.is_empty() {
            times.extend_from_slice(&seg_times);
            sampled.add_samples(&times, &positions, None)?;
        } else {
            is_time_dynamic = false;
        }
    }

    if is_time_dynamic && !times.is_empty() {
        let billboard = create_default_billboard(default_billboard_image(options))?;
        if options.clamp_to_ground {
            billboard.set_height_reference(Some(PropertyInput::value(HeightReference::ClampToGround as i32 as f64)))?;
        }
        entity.set_property("billboard", Some(PropertyInput::Component(billboard)))?;
        entity.set_position(Some(sampled))?;
        let availability = Availability::new(PropertyIntervals::new(|a, b| a == b));
        availability.add_interval(TimeInterval::new(times[0], times[times.length_minus_one()], true, true, None));
        entity.set_availability(Some(availability));
    }

    let polyline = create_default_polyline(options.track_color)?;
    polyline.set_positions(Some(PropertyInput::Value(DynamicValue::Positions(Rc::new(positions)))))?;
    if options.clamp_to_ground {
        polyline.set_clamp_to_ground(Some(PropertyInput::value(true)))?;
    }
    entity.set_property("polyline", Some(PropertyInput::Component(polyline)))?;
    Ok(())
}

trait LastIndex {
    fn length_minus_one(&self) -> usize;
}
impl<T> LastIndex for Vec<T> {
    fn length_minus_one(&self) -> usize {
        self.len().saturating_sub(1)
    }
}

fn process_metadata(node: Node<'_, '_>) -> Option<GpxMetadata> {
    let m = query_first_node(node, "metadata")?;
    let meta = GpxMetadata {
        name: query_string_value(m, "name"),
        desc: query_string_value(m, "desc"),
        author: get_person(m),
        copyright: get_copyright(m),
        link: get_link(m),
        time: query_string_value(m, "time"),
        keywords: query_string_value(m, "keywords"),
        bounds: get_bounds(m),
    };
    if meta.name.is_some()
        || meta.desc.is_some()
        || meta.author.is_some()
        || meta.copyright.is_some()
        || meta.link.is_some()
        || meta.time.is_some()
        || meta.keywords.is_some()
        || meta.bounds.is_some()
    {
        Some(meta)
    } else {
        None
    }
}

fn get_person(node: Node<'_, '_>) -> Option<GpxPerson> {
    let p = query_first_node(node, "author")?;
    let person = GpxPerson { name: query_string_value(p, "name"), email: get_email(p), link: get_link(p) };
    if person.name.is_some() || person.email.is_some() || person.link.is_some() {
        Some(person)
    } else {
        None
    }
}

fn get_email(node: Node<'_, '_>) -> Option<String> {
    let e = query_first_node(node, "email")?;
    let id = query_string_value(e, "id").unwrap_or_else(|| "undefined".to_owned());
    let domain = query_string_value(e, "domain").unwrap_or_else(|| "undefined".to_owned());
    Some(format!("{id}@{domain}"))
}

fn get_link(node: Node<'_, '_>) -> Option<GpxLink> {
    let l = query_first_node(node, "link")?;
    let link = GpxLink {
        href: l.attribute("href").map(str::to_owned),
        text: query_string_value(l, "text"),
        mime_type: query_string_value(l, "type"),
    };
    if link.href.is_some() || link.text.is_some() || link.mime_type.is_some() {
        Some(link)
    } else {
        None
    }
}

fn get_copyright(node: Node<'_, '_>) -> Option<GpxCopyright> {
    let c = query_first_node(node, "copyright")?;
    let copy = GpxCopyright {
        author: c.attribute("author").map(str::to_owned),
        year: query_string_value(c, "year"),
        license: query_string_value(c, "license"),
    };
    if copy.author.is_some() || copy.year.is_some() || copy.license.is_some() {
        Some(copy)
    } else {
        None
    }
}

fn get_bounds(node: Node<'_, '_>) -> Option<GpxBounds> {
    let b = query_first_node(node, "bounds")?;
    let bounds = GpxBounds {
        min_lat: query_numeric_value(b, "minlat"),
        max_lat: query_numeric_value(b, "maxlat"),
        min_lon: query_numeric_value(b, "minlon"),
        max_lon: query_numeric_value(b, "maxlon"),
    };
    if bounds.min_lat.is_some() || bounds.max_lat.is_some() || bounds.min_lon.is_some() || bounds.max_lon.is_some() {
        Some(bounds)
    } else {
        None
    }
}
