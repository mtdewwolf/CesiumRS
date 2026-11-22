//! CesiumJS `KmlDataSource`: loads Keyhole Markup Language (KML 2.2 + `gx:` extensions)
//! and compressed KMZ archives into an [`EntityCollection`].
mod geometries;
mod styles;
mod xml;
pub mod zip;

use self::{
    geometries::{is_geometry_tag, process_geometry, process_position_graphics},
    styles::{compute_final_style, get_icon_href, process_description, process_styles, set_query_string},
    xml::{
        insert_namespaces, matches_namespace, node_text_content, query_boolean_value, query_child_nodes,
        query_color_value, query_first_node, query_numeric_attribute, query_numeric_value, query_string_attribute,
        query_string_value, read_coordinates, remove_duplicate_namespaces, resolve_href, UriResolver, ATOM_NAMESPACES,
        GX_NAMESPACES, KML_GX_NAMESPACES, KML_NAMESPACES,
    },
    zip::{detect_mime_type, is_zip_bytes, read_zip},
};
use super::{
    Availability, ChangeEvent, DataSource, DataSourceClock, DataSourceRef, DataSourceState, DefinitionChanged,
    DynamicValue, Entity, EntityCollection, EntityComponent, EntityOptions, EntityRef, ImageMaterialProperty,
    KmlCamera, KmlLookAt, KmlTour, KmlTourFlyTo, KmlTourView, KmlTourWait, PropertyInput, PropertyIntervals,
    RectangleGraphics,
};
use crate::core::{
    create_guid::create_guid,
    iso8601, math,
    query::{object_to_query, query_to_object, QueryValue},
    resource::{DerivedResourceOptions, QueryParameters},
    resource_client::decode_data_uri_text,
    uri_helpers::get_filename_from_uri,
    Cartesian2, Cartesian3, Cartographic, ClockRange, ClockStep, Credit, Ellipsoid, HeadingPitchRange,
    HeadingPitchRoll, JulianDate, PolygonHierarchy, Rectangle, Resource, ResourceClient, RuntimeError, TimeInterval,
};
use crate::scene::{Camera, CameraFrustum, SceneMode};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine};
use futures_util::future::LocalBoxFuture;
use roxmltree::{Document, Node};
use serde_json::json;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::{Rc, Weak},
    task::{Context, Poll},
};

/// Atom author metadata stored on [`KmlFeatureData`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KmlAuthor {
    pub name: Option<String>,
    pub uri: Option<String>,
    pub email: Option<String>,
}

/// Atom link metadata stored on [`KmlFeatureData`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KmlLink {
    pub href: Option<String>,
    pub hreflang: Option<String>,
    pub rel: Option<String>,
    pub link_type: Option<String>,
    pub title: Option<String>,
    pub length: Option<String>,
}

/// One `<Data name="...">` entry inside KML `<ExtendedData>`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KmlExtendedDataEntry {
    pub display_name: Option<String>,
    pub value: Option<String>,
}

/// KML Feature metadata attached to each loaded [`Entity`] under the `"kml"` property.
#[derive(Clone, Debug, Default)]
struct KmlFeatureDataInner {
    author: KmlAuthor,
    link: KmlLink,
    address: Option<String>,
    phone_number: Option<String>,
    snippet: Option<String>,
    extended_data: Option<Vec<(String, KmlExtendedDataEntry)>>,
    camera: Option<KmlCamera>,
    look_at: Option<KmlLookAt>,
}

/// CesiumJS `KmlFeatureData`: non-visual KML feature metadata attached to `entity.kml`.
pub struct KmlFeatureData {
    inner: RefCell<KmlFeatureDataInner>,
    changed: DefinitionChanged,
}

impl Default for KmlFeatureData {
    fn default() -> Self {
        Self::new()
    }
}

impl KmlFeatureData {
    pub fn new() -> Self {
        Self { inner: RefCell::new(KmlFeatureDataInner::default()), changed: DefinitionChanged::new() }
    }

    pub fn author(&self) -> KmlAuthor {
        self.inner.borrow().author.clone()
    }
    pub fn set_author(&self, author: KmlAuthor) {
        self.inner.borrow_mut().author = author;
    }

    pub fn link(&self) -> KmlLink {
        self.inner.borrow().link.clone()
    }
    pub fn set_link(&self, link: KmlLink) {
        self.inner.borrow_mut().link = link;
    }

    pub fn address(&self) -> Option<String> {
        self.inner.borrow().address.clone()
    }
    pub fn set_address(&self, address: Option<String>) {
        self.inner.borrow_mut().address = address;
    }

    pub fn phone_number(&self) -> Option<String> {
        self.inner.borrow().phone_number.clone()
    }
    pub fn set_phone_number(&self, phone_number: Option<String>) {
        self.inner.borrow_mut().phone_number = phone_number;
    }

    pub fn snippet(&self) -> Option<String> {
        self.inner.borrow().snippet.clone()
    }
    pub fn set_snippet(&self, snippet: Option<String>) {
        self.inner.borrow_mut().snippet = snippet;
    }

    pub fn extended_data(&self) -> Option<Vec<(String, KmlExtendedDataEntry)>> {
        self.inner.borrow().extended_data.clone()
    }
    pub fn set_extended_data(&self, extended_data: Option<Vec<(String, KmlExtendedDataEntry)>>) {
        self.inner.borrow_mut().extended_data = extended_data;
    }

    pub fn camera(&self) -> Option<KmlCamera> {
        self.inner.borrow().camera
    }
    pub fn set_camera(&self, camera: Option<KmlCamera>) {
        self.inner.borrow_mut().camera = camera;
    }

    pub fn look_at(&self) -> Option<KmlLookAt> {
        self.inner.borrow().look_at
    }
    pub fn set_look_at(&self, look_at: Option<KmlLookAt>) {
        self.inner.borrow_mut().look_at = look_at;
    }

    pub fn to_json(&self) -> serde_json::Value {
        let s = self.inner.borrow();
        let ext = s.extended_data.as_ref().map(|entries| {
            let mut map = serde_json::Map::new();
            for (k, v) in entries {
                map.insert(k.clone(), json!({ "displayName": v.display_name, "value": v.value }));
            }
            serde_json::Value::Object(map)
        });
        let camera = s.camera.map(|c| {
            json!({
                "position": c.position.to_array(),
                "headingPitchRoll": [c.heading_pitch_roll.heading, c.heading_pitch_roll.pitch, c.heading_pitch_roll.roll],
            })
        });
        let look_at = s.look_at.map(|l| {
            json!({
                "position": l.position.to_array(),
                "headingPitchRange": [l.heading_pitch_range.heading, l.heading_pitch_range.pitch, l.heading_pitch_range.range],
            })
        });
        json!({
            "author": { "name": s.author.name, "uri": s.author.uri, "email": s.author.email },
            "link": {
                "href": s.link.href,
                "hreflang": s.link.hreflang,
                "rel": s.link.rel,
                "type": s.link.link_type,
                "title": s.link.title,
                "length": s.link.length,
            },
            "address": s.address,
            "phoneNumber": s.phone_number,
            "snippet": s.snippet,
            "extendedData": ext,
            "camera": camera,
            "lookAt": look_at,
        })
    }
}

impl EntityComponent for KmlFeatureData {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn is_constant(&self) -> bool {
        true
    }
    fn get_value(
        &self,
        _time: &JulianDate,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let val = DynamicValue::from_json(self.to_json());
        if let Some(out) = result {
            *out = val.clone();
        }
        Ok(Some(val))
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        Ok(other.as_any().downcast_ref::<Self>().is_some_and(|o| self.to_json() == o.to_json()))
    }
}

/// Host-agnostic representation of a parsed KML `<ScreenOverlay>`.
#[derive(Clone, Debug, PartialEq)]
pub struct KmlScreenOverlay {
    pub icon_url: String,
    pub screen_xy: Option<(f64, f64, Option<String>, Option<String>)>,
    pub overlay_xy: Option<(f64, f64, Option<String>, Option<String>)>,
    pub size: Option<(f64, f64, Option<String>, Option<String>)>,
}

/// Container that collects parsed [`KmlScreenOverlay`] records.
pub type KmlScreenOverlayContainer = Rc<RefCell<Vec<KmlScreenOverlay>>>;

/// Payload emitted by [`KmlDataSource::unsupported_node_event`].
#[derive(Clone)]
pub struct KmlUnsupportedNodeEvent {
    pub data_source: DataSourceRef,
    pub parent_entity: Option<EntityRef>,
    pub node_name: String,
    pub entity_collection: Rc<EntityCollection>,
    pub source_uri: String,
}

/// Options for constructing or loading a [`KmlDataSource`].
#[derive(Clone)]
pub struct KmlOptions {
    pub source_uri: Option<String>,
    pub clamp_to_ground: bool,
    pub ellipsoid: Ellipsoid,
    pub screen_overlay_container: Option<KmlScreenOverlayContainer>,
    pub credit: Option<Credit>,
    pub resource_client: Option<ResourceClient>,
}

impl Default for KmlOptions {
    fn default() -> Self {
        Self {
            source_uri: None,
            clamp_to_ground: false,
            ellipsoid: Ellipsoid::WGS84,
            screen_overlay_container: None,
            credit: None,
            resource_client: None,
        }
    }
}

/// Supported inputs for [`KmlDataSource::load`].
pub enum KmlInput {
    Xml(String),
    Kmz(Vec<u8>),
    Bytes(Vec<u8>),
    Resource(Resource),
}

impl From<&str> for KmlInput {
    fn from(s: &str) -> Self {
        Self::Xml(s.to_owned())
    }
}

impl From<String> for KmlInput {
    fn from(s: String) -> Self {
        Self::Xml(s)
    }
}

impl From<Vec<u8>> for KmlInput {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Bytes(bytes)
    }
}

impl From<&[u8]> for KmlInput {
    fn from(bytes: &[u8]) -> Self {
        Self::Bytes(bytes.to_vec())
    }
}

impl From<Resource> for KmlInput {
    fn from(r: Resource) -> Self {
        Self::Resource(r)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RefreshMode {
    Interval,
    Expire,
    Stop,
}

#[derive(Clone, Copy)]
struct KmlCameraView {
    position: Cartesian3,
    direction: Cartesian3,
    up: Cartesian3,
    center: Cartesian3,
    bounds: Rectangle,
    width: f64,
    height: f64,
    heading: Option<f64>,
    pitch: Option<f64>,
    fov: Option<(f64, f64)>,
    mode: SceneMode,
    revision: u64,
    changed_at: JulianDate,
}

#[derive(Clone)]
struct NetworkLinkInfo {
    id: String,
    href: Resource,
    cookie: QueryParameters,
    last_updated: JulianDate,
    view_revision: u64,
    entity: EntityRef,
    view_bound_scale: f64,
    refresh_mode: RefreshMode,
    interval_seconds: f64,
    expire_time: Option<JulianDate>,
    screen_overlay_container: Option<KmlScreenOverlayContainer>,
}

struct NetworkLinkRefresh {
    link: NetworkLinkInfo,
    href: Resource,
    staged: Rc<KmlDataSource>,
    result: Result<Option<NetworkLinkControlInfo>, RuntimeError>,
}

type NetworkRefreshFuture = LocalBoxFuture<'static, Vec<NetworkLinkRefresh>>;

struct ProcessingData<'a> {
    parent_entity: Option<EntityRef>,
    entity_collection: &'a Rc<EntityCollection>,
    style_collection: &'a Rc<EntityCollection>,
    source_resource: &'a Resource,
    uri_resolver: Option<&'a UriResolver>,
    context: Option<&'a str>,
    screen_overlay_container: Option<&'a KmlScreenOverlayContainer>,
    resource_client: Option<&'a ResourceClient>,
}

struct PendingNetworkLink {
    network_entity: EntityRef,
    href: Resource,
    new_source_uri: Resource,
    refresh_mode: Option<String>,
    refresh_interval: f64,
    view_refresh_time: Option<f64>,
    view_bound_scale: f64,
    screen_overlay_container: Option<KmlScreenOverlayContainer>,
}

struct NetworkLinkControlInfo {
    has_update: bool,
    cookie: QueryParameters,
    min_refresh_period: f64,
    expires: Option<String>,
}

pub type NowProvider = Rc<dyn Fn() -> JulianDate>;

/// Port of CesiumJS `KmlDataSource`.
pub struct KmlDataSource {
    state: DataSourceState,
    owner: Weak<KmlDataSource>,
    clock: RefCell<Option<Rc<DataSourceClock>>>,
    ellipsoid: Cell<Ellipsoid>,
    clamp_to_ground: Cell<bool>,
    credit: RefCell<Option<Credit>>,
    resource_credits: RefCell<Vec<Credit>>,
    kml_tours: RefCell<Vec<Rc<KmlTour>>>,
    screen_overlays: RefCell<Vec<KmlScreenOverlay>>,
    screen_overlay_container: RefCell<Option<KmlScreenOverlayContainer>>,
    refresh_event: ChangeEvent<(DataSourceRef, String)>,
    unsupported_node_event: ChangeEvent<KmlUnsupportedNodeEvent>,
    network_links: RefCell<Vec<NetworkLinkInfo>>,
    refresh_task: RefCell<Option<NetworkRefreshFuture>>,
    refresh_generation: Cell<u64>,
    polling_refresh: Cell<bool>,
    resource_client: RefCell<Option<ResourceClient>>,
    now_provider: RefCell<Option<NowProvider>>,
    camera_view: Cell<Option<KmlCameraView>>,
}

impl KmlDataSource {
    pub fn new(options: KmlOptions) -> Rc<Self> {
        Rc::new_cyclic(|owner| Self {
            state: DataSourceState::new(owner.clone(), None),
            owner: owner.clone(),
            clock: RefCell::new(None),
            ellipsoid: Cell::new(options.ellipsoid),
            clamp_to_ground: Cell::new(options.clamp_to_ground),
            credit: RefCell::new(options.credit),
            resource_credits: RefCell::new(Vec::new()),
            kml_tours: RefCell::new(Vec::new()),
            screen_overlays: RefCell::new(Vec::new()),
            screen_overlay_container: RefCell::new(options.screen_overlay_container),
            refresh_event: ChangeEvent::new(),
            unsupported_node_event: ChangeEvent::new(),
            network_links: RefCell::new(Vec::new()),
            refresh_task: RefCell::default(),
            refresh_generation: Cell::default(),
            polling_refresh: Cell::default(),
            resource_client: RefCell::new(options.resource_client),
            now_provider: RefCell::new(None),
            camera_view: Cell::default(),
        })
    }

    /// Creates a new [`KmlDataSource`] and loads `data` with `options`.
    pub async fn load_new(data: impl Into<KmlInput>, options: KmlOptions) -> Result<Rc<Self>, RuntimeError> {
        let source = Self::new(options.clone());
        source.load(data, options).await?;
        Ok(source)
    }

    pub fn set_name(&self, name: Option<String>) {
        self.state.set_name(name);
    }

    pub fn ellipsoid(&self) -> Ellipsoid {
        self.ellipsoid.get()
    }

    pub fn clamp_to_ground(&self) -> bool {
        self.clamp_to_ground.get()
    }

    pub fn credit(&self) -> Option<Credit> {
        self.credit.borrow().clone()
    }

    pub fn resource_credits(&self) -> Vec<Credit> {
        self.resource_credits.borrow().clone()
    }

    pub fn kml_tours(&self) -> Vec<Rc<KmlTour>> {
        self.kml_tours.borrow().clone()
    }

    pub fn screen_overlays(&self) -> Vec<KmlScreenOverlay> {
        self.screen_overlays.borrow().clone()
    }

    pub fn refresh_event(&self) -> &ChangeEvent<(DataSourceRef, String)> {
        &self.refresh_event
    }

    pub fn unsupported_node_event(&self) -> &ChangeEvent<KmlUnsupportedNodeEvent> {
        &self.unsupported_node_event
    }

    /// Overrides the clock used for `JulianDate::now()` (useful for deterministic tests).
    pub fn set_now_provider(&self, provider: Option<NowProvider>) {
        *self.now_provider.borrow_mut() = provider;
    }

    /// Captures the host camera for view-dependent KML URLs and `onStop` refresh.
    pub fn update_camera(&self, camera: &mut Camera) -> Result<(), RuntimeError> {
        let ellipsoid = self.ellipsoid();
        let bounds = camera
            .compute_view_rectangle(&ellipsoid)
            .map_err(|error| RuntimeError::new(error.to_string()))?
            .unwrap_or_default();
        let center_window = Cartesian2::new(camera.client_width * 0.5, camera.client_height * 0.5);
        let center = camera
            .pick_ellipsoid(center_window, &ellipsoid)
            .unwrap_or_else(|| ellipsoid.cartographic_to_cartesian(&bounds.center()));
        let position = camera.position_wc();
        let direction = camera.direction_wc();
        let up = camera.up_wc();
        let mode = camera.mode();
        let previous = self.camera_view.get();
        let same_view = previous.filter(|view| {
            mode == view.mode
                && [position, direction, up]
                    .into_iter()
                    .zip([view.position, view.direction, view.up])
                    .all(|(a, b)| a.equals_epsilon(b, 1.0e-7))
        });
        let (revision, changed_at) = same_view.map_or_else(
            || (previous.map_or(1, |view| view.revision.wrapping_add(1)), self.now()),
            |view| (view.revision, view.changed_at),
        );
        let fov = match camera.frustum {
            CameraFrustum::Perspective(frustum) => Some((frustum.fov.to_degrees(), frustum.aspect_ratio)),
            _ => None,
        };
        self.camera_view.set(Some(KmlCameraView {
            position,
            direction,
            up,
            center,
            bounds,
            width: camera.client_width,
            height: camera.client_height,
            heading: camera.heading(),
            pitch: camera.pitch(),
            fov,
            mode,
            revision,
            changed_at,
        }));
        Ok(())
    }

    pub(crate) fn resolve_view_query(&self, resource: &Resource, view_bound_scale: f64) {
        process_network_link_query_string(resource, self.camera_view.get(), view_bound_scale, self.ellipsoid());
    }

    pub(crate) fn now(&self) -> JulianDate {
        if let Some(p) = self.now_provider.borrow().as_ref() {
            p()
        } else {
            JulianDate::now()
        }
    }

    /// Reads an entity's [`KmlFeatureData`] via a closure.
    pub fn with_kml_data<R>(entity: &Entity, f: impl FnOnce(&KmlFeatureData) -> R) -> Option<R> {
        let comp = entity.get_property("kml")?.component()?;
        let data = comp.as_any().downcast_ref::<KmlFeatureData>()?;
        Some(f(data))
    }

    /// Asynchronously loads KML XML, KMZ archive bytes, or a [`Resource`], replacing existing entities.
    pub async fn load(&self, data: impl Into<KmlInput>, options: KmlOptions) -> Result<(), RuntimeError> {
        self.set_loading(true)?;

        let old_name = self.name();
        self.state.set_name_silent(None);
        self.clamp_to_ground.set(options.clamp_to_ground);
        self.ellipsoid.set(options.ellipsoid);
        if options.resource_client.is_some() {
            *self.resource_client.borrow_mut() = options.resource_client.clone();
        }
        if options.credit.is_some() {
            *self.credit.borrow_mut() = options.credit.clone();
        }
        let overlay_container =
            options.screen_overlay_container.clone().or_else(|| self.screen_overlay_container.borrow().clone());
        if overlay_container.is_some() {
            *self.screen_overlay_container.borrow_mut() = overlay_container.clone();
        }
        self.kml_tours.borrow_mut().clear();
        self.clear_network_links();

        let source_uri = options.source_uri.as_deref().and_then(Resource::new);
        let client = self.resource_client.borrow().clone();

        let result = self
            .load_into(self.entities().clone(), data.into(), source_uri, None, None, overlay_container, client.as_ref())
            .await;

        match result {
            Ok(_) => {
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
                // Match upstream double-catch: inner `load` raises errorEvent,
                // then outer `KmlDataSource.prototype.load` sets loading=false and raises errorEvent.
                self.state.notify_error(error.clone());
                let _ = self.set_loading(false);
                self.state.notify_error(error.clone());
                Err(error)
            }
        }
    }

    fn compute_clock(&self) -> Option<Rc<DataSourceClock>> {
        let availability = self.entities().compute_availability();
        let mut start = availability.start;
        let mut stop = availability.stop;
        let is_min_start = start == iso8601::minimum_value();
        let is_max_stop = stop == iso8601::maximum_value();
        if is_min_start && is_max_stop {
            return None;
        }
        if is_min_start || is_max_stop {
            let mut g = self.now().to_gregorian_date();
            g.hour = 0;
            g.minute = 0;
            g.second = 0;
            g.millisecond = 0.0;
            let today_midnight = JulianDate::from_gregorian_date(&g);
            if is_min_start {
                start = today_midnight;
            }
            if is_max_stop {
                stop = today_midnight.add_days(1.0);
            }
        }
        let clock = DataSourceClock::new();
        clock.set_start_time(Some(Rc::new(start)));
        clock.set_stop_time(Some(Rc::new(stop)));
        clock.set_current_time(Some(Rc::new(start)));
        clock.set_clock_range(Some(ClockRange::LoopStop));
        clock.set_clock_step(Some(ClockStep::SystemClockMultiplier));
        let diff = JulianDate::seconds_difference(&stop, &start);
        let mult = math::js_round((diff / 60.0).clamp(1.0, 3.15569e7));
        clock.set_multiplier(Some(mult));
        Some(Rc::new(clock))
    }

    #[allow(clippy::too_many_arguments)]
    fn load_into<'a>(
        &'a self,
        entity_collection: Rc<EntityCollection>,
        data: KmlInput,
        source_uri: Option<Resource>,
        uri_resolver: Option<Rc<UriResolver>>,
        context: Option<String>,
        screen_overlay_container: Option<KmlScreenOverlayContainer>,
        resource_client: Option<&'a ResourceClient>,
    ) -> LocalBoxFuture<'a, Result<Option<NetworkLinkControlInfo>, RuntimeError>> {
        Box::pin(async move {
            let mut resolved_source = source_uri;
            let payload: Result<KmlPayload, RuntimeError> = match data {
                KmlInput::Xml(text) => Ok(KmlPayload::Xml(text)),
                KmlInput::Kmz(bytes) => Ok(KmlPayload::Kmz(bytes)),
                KmlInput::Bytes(bytes) => {
                    if is_zip_bytes(&bytes) {
                        Ok(KmlPayload::Kmz(bytes))
                    } else {
                        String::from_utf8(bytes)
                            .map(KmlPayload::Xml)
                            .map_err(|e| RuntimeError::new(format!("Invalid UTF-8 in KML data: {e}")))
                    }
                }
                KmlInput::Resource(resource) => {
                    if resolved_source.is_none() {
                        resolved_source = Some(resource.clone());
                    }
                    if let Some(credits) = resource.credits() {
                        self.resource_credits.borrow_mut().extend(credits);
                    }
                    if resource.is_data_uri() {
                        let resp = crate::core::resource_client::data_uri_response(&resource.url())
                            .map_err(|e| RuntimeError::new(e.to_string()))?;
                        if is_zip_bytes(&resp.body) {
                            Ok(KmlPayload::Kmz(resp.body))
                        } else {
                            let text =
                                decode_data_uri_text(&resource.url()).map_err(|e| RuntimeError::new(e.to_string()))?;
                            Ok(KmlPayload::Xml(text))
                        }
                    } else {
                        let client = resource_client.ok_or_else(|| {
                            RuntimeError::new("KmlOptions.resource_client is required to fetch a Resource.")
                        })?;
                        let fut = resource
                            .fetch_array_buffer(client)
                            .ok_or_else(|| RuntimeError::new("KML resource request was deferred."))?;
                        let bytes = fut
                            .await
                            .map_err(|e| RuntimeError::new(e.to_string()))?
                            .ok_or_else(|| RuntimeError::new("No KML data returned from resource."))?;
                        if is_zip_bytes(&bytes) {
                            Ok(KmlPayload::Kmz(bytes))
                        } else {
                            String::from_utf8(bytes)
                                .map(KmlPayload::Xml)
                                .map_err(|e| RuntimeError::new(format!("Invalid UTF-8 in KML response: {e}")))
                        }
                    }
                }
            };

            let source_resource = resolved_source.unwrap_or_default();
            match payload? {
                KmlPayload::Kmz(bytes) => {
                    self.load_kmz(
                        entity_collection,
                        &bytes,
                        &source_resource,
                        context,
                        screen_overlay_container,
                        resource_client,
                    )
                    .await
                }
                KmlPayload::Xml(raw_text) => {
                    self.load_kml_text(
                        entity_collection,
                        &raw_text,
                        &source_resource,
                        uri_resolver,
                        context,
                        screen_overlay_container,
                        resource_client,
                    )
                    .await
                }
            }
        })
    }

    async fn load_kmz(
        &self,
        entity_collection: Rc<EntityCollection>,
        bytes: &[u8],
        source_resource: &Resource,
        context: Option<String>,
        screen_overlay_container: Option<KmlScreenOverlayContainer>,
        resource_client: Option<&ResourceClient>,
    ) -> Result<Option<NetworkLinkControlInfo>, RuntimeError> {
        let entries = read_zip(bytes)?;
        let mut resolver = UriResolver::default();
        let mut doc_entry: Option<zip::ZipEntry> = None;

        for entry in entries {
            if entry.is_dir {
                continue;
            }
            let is_kml = entry.filename.to_ascii_lowercase().ends_with(".kml");
            if is_kml {
                let is_root = !entry.filename.contains('/');
                if doc_entry.is_none() || is_root {
                    if let Some(prev) = doc_entry.replace(entry) {
                        let data_uri = encode_zip_entry_data_uri(&prev);
                        resolver.insert(prev.filename, data_uri);
                    }
                } else {
                    let data_uri = encode_zip_entry_data_uri(&entry);
                    resolver.insert(entry.filename, data_uri);
                }
            } else {
                let data_uri = encode_zip_entry_data_uri(&entry);
                resolver.insert(entry.filename, data_uri);
            }
        }

        let Some(root_entry) = doc_entry else {
            return Err(RuntimeError::new("KMZ file does not contain a KML document."));
        };
        let kml_text = String::from_utf8(root_entry.data)
            .map_err(|e| RuntimeError::new(format!("Invalid UTF-8 in KMZ root KML: {e}")))?;
        let resolver_rc = Rc::new(resolver);

        self.load_kml_text(
            entity_collection,
            &kml_text,
            source_resource,
            Some(resolver_rc),
            context,
            screen_overlay_container,
            resource_client,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn load_kml_text(
        &self,
        entity_collection: Rc<EntityCollection>,
        raw_text: &str,
        source_resource: &Resource,
        uri_resolver: Option<Rc<UriResolver>>,
        context: Option<String>,
        screen_overlay_container: Option<KmlScreenOverlayContainer>,
        resource_client: Option<&ResourceClient>,
    ) -> Result<Option<NetworkLinkControlInfo>, RuntimeError> {
        let cleaned = remove_duplicate_namespaces(&insert_namespaces(raw_text));
        let doc =
            Document::parse_with_options(&cleaned, roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() })
                .map_err(|e| RuntimeError::new(e.to_string()))?;

        entity_collection.remove_all();

        let root_el = doc.root_element();
        let doc_node = if root_el.tag_name().name() == "Document" {
            Some(root_el)
        } else {
            query_first_node(root_el, "Document", KML_NAMESPACES)
        };
        let doc_name = doc_node
            .and_then(|d| query_string_value(d, "name", KML_NAMESPACES))
            .unwrap_or_else(|| get_filename_from_uri(&source_resource.get_url_component(false, false)));

        if self.name().is_none() {
            self.state.set_name_silent(Some(doc_name));
        }

        let style_collection = EntityCollection::new();
        process_styles(self, &doc, &style_collection, source_resource, false, uri_resolver.as_deref(), resource_client)
            .await?;

        let nlc_info = extract_network_link_control(root_el);

        let mut feature_root = root_el;
        if feature_root.tag_name().name() == "kml" {
            for child in feature_root.children().filter(Node::is_element) {
                if is_feature_tag(child.tag_name().name()) {
                    feature_root = child;
                    break;
                }
            }
        }

        let processing = ProcessingData {
            parent_entity: None,
            entity_collection: &entity_collection,
            style_collection: &style_collection,
            source_resource,
            uri_resolver: uri_resolver.as_deref(),
            context: context.as_deref(),
            screen_overlay_container: screen_overlay_container.as_ref(),
            resource_client,
        };

        let mut pending_links = Vec::new();
        entity_collection.suspend_events();
        let proc_res = self.process_feature_node(feature_root, &processing, &mut pending_links);
        let _ = entity_collection.resume_events();
        proc_res?;

        // Resolve queued NetworkLinks asynchronously after the synchronous tree walk.
        for pending in pending_links {
            let link_collection = EntityCollection::new();
            let href = pending.href.clone();
            self.resolve_view_query(&href, pending.view_bound_scale);
            let source = if href.is_data_uri() { pending.new_source_uri.clone() } else { href.clone() };
            let link_res = self
                .load_into(
                    link_collection.clone(),
                    KmlInput::Resource(href),
                    Some(source),
                    uri_resolver.clone(),
                    Some(pending.network_entity.id().to_owned()),
                    pending.screen_overlay_container.clone(),
                    resource_client,
                )
                .await;

            match link_res {
                Ok(child_nlc) => {
                    let main_entities = &entity_collection;
                    main_entities.suspend_events();
                    for new_entity in link_collection.values() {
                        if new_entity.parent().is_none() {
                            new_entity.set_parent(Some(pending.network_entity.clone()))?;
                            merge_availability_with_parent(&new_entity);
                        }
                        main_entities.add(new_entity)?;
                    }
                    main_entities.resume_events()?;

                    let now = self.now();
                    let min_refresh = child_nlc.as_ref().map_or(0.0, |control| control.min_refresh_period);
                    let (refresh_mode, interval_seconds, expire_time) = match pending.refresh_mode.as_deref() {
                        Some("onInterval") if pending.refresh_interval > 0.0 => {
                            (RefreshMode::Interval, pending.refresh_interval.max(min_refresh), None)
                        }
                        Some("onExpire") => {
                            let Some(expires) = child_nlc.as_ref().and_then(|control| control.expires.as_deref())
                            else {
                                continue;
                            };
                            let Ok(date) = JulianDate::from_iso8601(expires) else { continue };
                            (RefreshMode::Expire, 0.0, Some(apply_min_refresh(date, now, min_refresh)))
                        }
                        _ => {
                            let Some(delay) = pending.view_refresh_time else { continue };
                            if self.camera_view.get().is_none() {
                                continue;
                            }
                            (RefreshMode::Stop, delay, None)
                        }
                    };

                    self.network_links.borrow_mut().push(NetworkLinkInfo {
                        id: create_guid(),
                        href: pending.href,
                        cookie: child_nlc.map_or_else(Vec::new, |control| control.cookie),
                        last_updated: now,
                        view_revision: self.camera_view.get().map_or(0, |view| view.revision),
                        entity: pending.network_entity,
                        view_bound_scale: pending.view_bound_scale,
                        refresh_mode,
                        interval_seconds,
                        expire_time,
                        screen_overlay_container: pending.screen_overlay_container,
                    });
                }
                Err(error) => {
                    self.state.notify_error(error);
                }
            }
        }

        Ok(nlc_info)
    }

    /// Asynchronously refreshes network links due at the given wall-clock time.
    pub async fn refresh_network_links(&self, now: Option<JulianDate>) -> Result<usize, RuntimeError> {
        std::future::poll_fn(|context| self.poll_network_links(now, context)).await
    }

    fn clear_network_links(&self) {
        self.refresh_generation.set(self.refresh_generation.get().wrapping_add(1));
        self.refresh_task.take();
        self.network_links.borrow_mut().clear();
    }

    fn start_network_refresh(&self, now: JulianDate) -> Option<NetworkRefreshFuture> {
        let view = self.camera_view.get();
        let mut covered = HashSet::new();
        let links: Vec<_> = self
            .network_links
            .borrow()
            .iter()
            .filter(|link| match link.refresh_mode {
                RefreshMode::Interval => {
                    JulianDate::seconds_difference(&now, &link.last_updated) > link.interval_seconds
                }
                RefreshMode::Expire => link.expire_time.is_some_and(|time| JulianDate::greater_than(&now, &time)),
                RefreshMode::Stop => view.is_some_and(|view| {
                    view.revision != link.view_revision
                        && JulianDate::seconds_difference(&now, &view.changed_at) >= link.interval_seconds
                }),
            })
            .cloned()
            .collect();
        // A parent refresh replaces its subtree, including any nested network links.
        for link in &links {
            collect_descendant_ids(&link.entity, &mut covered);
        }
        let links: Vec<_> = links.into_iter().filter(|link| !covered.contains(link.entity.id())).collect();
        if links.is_empty() {
            return None;
        }

        let options = KmlOptions {
            ellipsoid: self.ellipsoid(),
            clamp_to_ground: self.clamp_to_ground(),
            resource_client: self.resource_client.borrow().clone(),
            ..Default::default()
        };
        Some(Box::pin(async move {
            let mut results = Vec::with_capacity(links.len());
            for mut link in links {
                let href = link.href.clone();
                if !link.cookie.is_empty() {
                    href.set_query_parameters(&link.cookie, false);
                }
                process_network_link_query_string(&href, view, link.view_bound_scale, options.ellipsoid);
                link.view_revision = view.map_or(0, |view| view.revision);
                let overlay_sink = link.screen_overlay_container.as_ref().map(|_| Rc::new(RefCell::new(Vec::new())));
                let staged = KmlDataSource::new(options.clone());
                staged.camera_view.set(view);
                staged.set_now_provider(Some(Rc::new(move || now)));
                let result = staged
                    .load_into(
                        staged.entities().clone(),
                        KmlInput::Resource(href.clone()),
                        Some(href.clone()),
                        None,
                        Some(link.entity.id().to_owned()),
                        overlay_sink,
                        options.resource_client.as_ref(),
                    )
                    .await;
                results.push(NetworkLinkRefresh { link, href, staged, result });
            }
            results
        }))
    }

    fn poll_network_links(
        &self,
        now: Option<JulianDate>,
        context: &mut Context<'_>,
    ) -> Poll<Result<usize, RuntimeError>> {
        if self.polling_refresh.replace(true) {
            return Poll::Ready(Ok(0));
        }
        let result = self.advance_network_refresh(now.unwrap_or_else(|| self.now()), context);
        self.polling_refresh.set(false);
        result
    }

    fn advance_network_refresh(&self, now: JulianDate, context: &mut Context<'_>) -> Poll<Result<usize, RuntimeError>> {
        let generation = self.refresh_generation.get();
        let Some(mut task) = self.refresh_task.take().or_else(|| self.start_network_refresh(now)) else {
            return Poll::Ready(Ok(0));
        };
        let results = task.as_mut().poll(context);
        if generation != self.refresh_generation.get() {
            return Poll::Ready(Ok(0));
        }
        match results {
            Poll::Pending => {
                self.refresh_task.replace(Some(task));
                Poll::Pending
            }
            Poll::Ready(results) => {
                let mut refreshed = 0;
                for result in results {
                    if generation != self.refresh_generation.get() {
                        break;
                    }
                    refreshed += usize::from(self.apply_network_refresh(now, result)?);
                }
                if refreshed > 0 {
                    self.state.notify_changed();
                }
                Poll::Ready(Ok(refreshed))
            }
        }
    }

    fn apply_network_refresh(&self, now: JulianDate, refresh: NetworkLinkRefresh) -> Result<bool, RuntimeError> {
        let NetworkLinkRefresh { mut link, href, staged, result } = refresh;
        let control = match result {
            Ok(control) => control,
            Err(error) => {
                self.state.notify_error(error);
                return Ok(false);
            }
        };
        let entities = self.entities();
        if !entities.contains(&link.entity) || control.as_ref().is_some_and(|control| control.has_update) {
            self.network_links.borrow_mut().retain(|existing| existing.id != link.id);
            return Ok(false);
        }

        let min_refresh = control.as_ref().map_or(0.0, |control| control.min_refresh_period);
        if let Some(control) = &control {
            link.cookie = control.cookie.clone();
        }
        link.last_updated = now;
        let keep_link = match link.refresh_mode {
            RefreshMode::Interval => {
                link.interval_seconds = link.interval_seconds.max(min_refresh);
                true
            }
            RefreshMode::Expire => {
                link.expire_time = control
                    .as_ref()
                    .and_then(|control| control.expires.as_deref())
                    .and_then(|expires| JulianDate::from_iso8601(expires).ok())
                    .map(|date| apply_min_refresh(date, now, min_refresh));
                link.expire_time.is_some()
            }
            RefreshMode::Stop => true,
        };

        let children = link.entity.children();
        let mut retired = HashSet::new();
        collect_descendant_ids(&link.entity, &mut retired);
        entities.suspend_events();
        let replacement: Result<(), RuntimeError> = (|| {
            for child in children {
                child.set_parent(None)?;
                remove_entity_subtree(entities, &child);
            }
            for entity in staged.entities().values() {
                if entity.parent().is_none() {
                    entity.set_parent(Some(link.entity.clone()))?;
                    merge_availability_with_parent(&entity);
                }
                entities.add(entity)?;
            }
            Ok(())
        })();
        let resumed = entities.resume_events();
        replacement.and(resumed)?;
        if !entities.contains(&link.entity) {
            return Ok(false);
        }

        let mut nested_links = staged.network_links.take();
        for nested in &mut nested_links {
            nested.screen_overlay_container = link.screen_overlay_container.clone();
        }
        let mut links = self.network_links.borrow_mut();
        links.retain(|existing| existing.id != link.id && !retired.contains(existing.entity.id()));
        links.extend(nested_links);
        if let Some(container) = &link.screen_overlay_container {
            container.borrow_mut().extend(staged.screen_overlays());
        }
        if keep_link {
            links.push(link);
        }
        drop(links);

        self.resource_credits.borrow_mut().extend(staged.resource_credits.take());
        self.kml_tours.borrow_mut().extend(staged.kml_tours.take());
        self.screen_overlays.borrow_mut().extend(staged.screen_overlays.take());

        let availability = entities.compute_availability();
        if availability.start != iso8601::minimum_value() || availability.stop != iso8601::maximum_value() {
            let clock = self.clock.borrow().clone();
            if let Some(clock) = clock {
                clock.set_start_time(Some(Rc::new(availability.start)));
                clock.set_stop_time(Some(Rc::new(availability.stop)));
            }
        }

        if let Some(source) = self.owner.upgrade() {
            self.refresh_event.raise((source, href.get_url_component(true, false)));
        }
        Ok(true)
    }

    fn process_feature_node(
        &self,
        node: Node<'_, '_>,
        processing: &ProcessingData<'_>,
        pending_links: &mut Vec<PendingNetworkLink>,
    ) -> Result<(), RuntimeError> {
        match node.tag_name().name() {
            "Document" => self.process_document(node, processing, pending_links),
            "Folder" => self.process_folder(node, processing, pending_links),
            "Placemark" => self.process_placemark(node, processing),
            "NetworkLink" => self.process_network_link(node, processing, pending_links),
            "GroundOverlay" => self.process_ground_overlay(node, processing),
            "ScreenOverlay" => self.process_screen_overlay(node, processing),
            "Tour" => self.process_tour(node),
            _ => {
                self.raise_unsupported_node(node, processing);
                Ok(())
            }
        }
    }

    fn process_document(
        &self,
        node: Node<'_, '_>,
        processing: &ProcessingData<'_>,
        pending_links: &mut Vec<PendingNetworkLink>,
    ) -> Result<(), RuntimeError> {
        for child in node.children().filter(Node::is_element) {
            if is_feature_tag(child.tag_name().name())
                && (matches_namespace(child, KML_NAMESPACES) || matches_namespace(child, GX_NAMESPACES))
            {
                self.process_feature_node(child, processing, pending_links)?;
            }
        }
        Ok(())
    }

    fn process_folder(
        &self,
        node: Node<'_, '_>,
        processing: &ProcessingData<'_>,
        pending_links: &mut Vec<PendingNetworkLink>,
    ) -> Result<(), RuntimeError> {
        let (entity, _) = self.process_feature(node, processing)?;
        let child_processing = ProcessingData {
            parent_entity: Some(entity),
            entity_collection: processing.entity_collection,
            style_collection: processing.style_collection,
            source_resource: processing.source_resource,
            uri_resolver: processing.uri_resolver,
            context: processing.context,
            screen_overlay_container: processing.screen_overlay_container,
            resource_client: processing.resource_client,
        };
        self.process_document(node, &child_processing, pending_links)
    }

    fn process_feature(
        &self,
        feature_node: Node<'_, '_>,
        processing: &ProcessingData<'_>,
    ) -> Result<(EntityRef, EntityRef), RuntimeError> {
        let entity = create_entity(feature_node, processing.entity_collection, processing.context)?;
        let style_entity = compute_final_style(
            self,
            feature_node,
            processing.style_collection,
            processing.source_resource,
            processing.uri_resolver,
        )?;

        let name = query_string_value(feature_node, "name", KML_NAMESPACES);
        entity.set_name(name);
        entity.set_parent(processing.parent_entity.clone())?;

        let mut availability = process_time_span(feature_node)?;
        if availability.is_none() {
            availability = process_time_stamp(feature_node)?;
        }
        entity.set_availability(availability);
        merge_availability_with_parent(&entity);

        let visibility = query_boolean_value(feature_node, "visibility", KML_NAMESPACES).unwrap_or(true);
        entity.set_show(ancestry_is_visible(processing.parent_entity.as_ref()) && visibility);

        let kml_comp = entity.get_property("kml").and_then(|p| p.component()).expect("kml property");
        let kml_data = kml_comp.as_any().downcast_ref::<KmlFeatureData>().expect("KmlFeatureData");

        if let Some(author_node) = query_first_node(feature_node, "author", ATOM_NAMESPACES) {
            kml_data.set_author(KmlAuthor {
                name: query_string_value(author_node, "name", ATOM_NAMESPACES),
                uri: query_string_value(author_node, "uri", ATOM_NAMESPACES),
                email: query_string_value(author_node, "email", ATOM_NAMESPACES),
            });
        }
        if let Some(link_node) = query_first_node(feature_node, "link", ATOM_NAMESPACES) {
            kml_data.set_link(KmlLink {
                href: query_string_attribute(link_node, "href"),
                hreflang: query_string_attribute(link_node, "hreflang"),
                rel: query_string_attribute(link_node, "rel"),
                link_type: query_string_attribute(link_node, "type"),
                title: query_string_attribute(link_node, "title"),
                length: query_string_attribute(link_node, "length"),
            });
        }

        kml_data.set_address(query_string_value(feature_node, "address", KML_NAMESPACES));
        kml_data.set_phone_number(query_string_value(feature_node, "phoneNumber", KML_NAMESPACES));
        kml_data.set_snippet(query_string_value(feature_node, "Snippet", KML_NAMESPACES));

        process_extended_data(feature_node, kml_data);
        process_description(
            feature_node,
            &entity,
            &style_entity,
            kml_data,
            processing.uri_resolver,
            processing.source_resource,
        )?;

        let ellipsoid = self.ellipsoid();
        if let Some(look_at) = parse_look_at(feature_node, &ellipsoid) {
            kml_data.set_look_at(Some(look_at));
        }
        if let Some(camera) = parse_camera(feature_node, &ellipsoid) {
            kml_data.set_camera(Some(camera));
        }

        Ok((entity, style_entity))
    }

    fn process_placemark(&self, placemark: Node<'_, '_>, processing: &ProcessingData<'_>) -> Result<(), RuntimeError> {
        let (entity, style_entity) = self.process_feature(placemark, processing)?;
        let mut has_geometry = false;
        for child in placemark.children().filter(Node::is_element) {
            if is_geometry_tag(child.tag_name().name()) {
                process_geometry(self, processing.entity_collection, child, &entity, &style_entity, Some(entity.id()))?;
                has_geometry = true;
                break;
            }
        }
        if !has_geometry {
            entity.merge(&style_entity)?;
            process_position_graphics(self, &entity, &style_entity, None)?;
        }
        Ok(())
    }

    fn process_ground_overlay(
        &self,
        ground_overlay: Node<'_, '_>,
        processing: &ProcessingData<'_>,
    ) -> Result<(), RuntimeError> {
        let (entity, _) = self.process_feature(ground_overlay, processing)?;
        let ellipsoid = self.ellipsoid();
        let positions = read_coordinates(query_first_node(ground_overlay, "LatLonQuad", GX_NAMESPACES), &ellipsoid);
        let z_index = query_numeric_value(ground_overlay, "drawOrder", KML_NAMESPACES);

        let is_quad = positions.is_some();
        let (poly_opt, rect_opt) = if let Some(positions) = positions {
            let polygon = styles::create_default_polygon()?;
            polygon.set_hierarchy(Some(PropertyInput::Value(DynamicValue::PolygonHierarchy(Rc::new(
                PolygonHierarchy::new(positions, Vec::new()),
            )))))?;
            polygon.set_z_index(z_index.map(PropertyInput::value))?;
            entity.set_property("polygon", Some(PropertyInput::Component(polygon.clone())))?;
            (Some(polygon), None)
        } else {
            let rect_gfx = Rc::new(RectangleGraphics::new());
            rect_gfx.set_z_index(z_index.map(PropertyInput::value))?;
            if let Some(lat_lon_box) = query_first_node(ground_overlay, "LatLonBox", KML_NAMESPACES) {
                let west = query_numeric_value(lat_lon_box, "west", KML_NAMESPACES)
                    .map(|v| math::negative_pi_to_pi(math::to_radians(v)))
                    .unwrap_or(0.0);
                let south = query_numeric_value(lat_lon_box, "south", KML_NAMESPACES)
                    .map(|v| math::clamp_to_latitude_range(math::to_radians(v)))
                    .unwrap_or(0.0);
                let east = query_numeric_value(lat_lon_box, "east", KML_NAMESPACES)
                    .map(|v| math::negative_pi_to_pi(math::to_radians(v)))
                    .unwrap_or(0.0);
                let north = query_numeric_value(lat_lon_box, "north", KML_NAMESPACES)
                    .map(|v| math::clamp_to_latitude_range(math::to_radians(v)))
                    .unwrap_or(0.0);
                let rect = Rectangle::new(west, south, east, north);
                rect_gfx.set_coordinates(Some(PropertyInput::Value(DynamicValue::from_json(json!({
                    "west": rect.west,
                    "south": rect.south,
                    "east": rect.east,
                    "north": rect.north,
                })))))?;
                if let Some(rotation) = query_numeric_value(lat_lon_box, "rotation", KML_NAMESPACES) {
                    let rad = math::to_radians(rotation);
                    rect_gfx.set_rotation(Some(PropertyInput::value(rad)))?;
                    rect_gfx.set_st_rotation(Some(PropertyInput::value(rad)))?;
                }
            }
            entity.set_property("rectangle", Some(PropertyInput::Component(rect_gfx.clone())))?;
            (None, Some(rect_gfx))
        };

        let icon_node = query_first_node(ground_overlay, "Icon", KML_NAMESPACES);
        let href = get_icon_href(icon_node, self, processing.source_resource, processing.uri_resolver, true);
        let color = query_color_value(ground_overlay, "color", KML_NAMESPACES);
        let material = if let Some(href) = href {
            let mat = Rc::new(ImageMaterialProperty::new());
            mat.set_image(Some(PropertyInput::value(href)))?;
            mat.set_color(color.map(PropertyInput::value))?;
            mat.set_transparent(Some(PropertyInput::value(true)))?;
            Some(PropertyInput::Component(mat))
        } else {
            color.map(PropertyInput::value)
        };

        let altitude_mode = query_string_value(ground_overlay, "altitudeMode", KML_NAMESPACES);
        let gx_altitude_mode = if altitude_mode.is_none() {
            query_string_value(ground_overlay, "altitudeMode", GX_NAMESPACES)
        } else {
            None
        };
        let use_height =
            altitude_mode.as_deref() == Some("absolute") || gx_altitude_mode.as_deref() == Some("relativeToSeaFloor");
        let altitude = if use_height { query_numeric_value(ground_overlay, "altitude", KML_NAMESPACES) } else { None };

        if is_quad {
            let polygon = poly_opt.expect("polygon");
            polygon.set_material(material)?;
            if use_height {
                polygon.set_height(altitude.map(PropertyInput::value))?;
                polygon.set_z_index(None)?;
            }
        } else {
            let rect_gfx = rect_opt.expect("rectangle");
            rect_gfx.set_material(material)?;
            if use_height {
                rect_gfx.set_height(altitude.map(PropertyInput::value))?;
                rect_gfx.set_z_index(None)?;
            }
        }

        Ok(())
    }

    fn process_screen_overlay(&self, node: Node<'_, '_>, processing: &ProcessingData<'_>) -> Result<(), RuntimeError> {
        let Some(container) = processing.screen_overlay_container else {
            return Ok(());
        };
        let icon_node = query_first_node(node, "Icon", KML_NAMESPACES);
        let Some(icon) = get_icon_href(icon_node, self, processing.source_resource, processing.uri_resolver, false)
        else {
            return Ok(());
        };

        let read_xy = |tag: &str| {
            let el = query_first_node(node, tag, KML_NAMESPACES)?;
            Some((
                query_numeric_attribute(el, "x").unwrap_or(0.0),
                query_numeric_attribute(el, "y").unwrap_or(0.0),
                query_string_attribute(el, "xunits"),
                query_string_attribute(el, "yunits"),
            ))
        };

        let overlay = KmlScreenOverlay {
            icon_url: icon.url(),
            screen_xy: read_xy("screenXY"),
            overlay_xy: read_xy("overlayXY"),
            size: read_xy("size"),
        };
        self.screen_overlays.borrow_mut().push(overlay.clone());
        container.borrow_mut().push(overlay);
        Ok(())
    }

    fn process_tour(&self, node: Node<'_, '_>) -> Result<(), RuntimeError> {
        let name = query_string_value(node, "name", KML_NAMESPACES).unwrap_or_default();
        let id = query_string_attribute(node, "id").unwrap_or_default();
        let tour = KmlTour::new(name, id);

        if let Some(playlist) = query_first_node(node, "Playlist", GX_NAMESPACES) {
            let ellipsoid = self.ellipsoid();
            for entry_node in playlist.children().filter(Node::is_element) {
                match entry_node.tag_name().name() {
                    "FlyTo" => {
                        let duration = query_numeric_value(entry_node, "duration", GX_NAMESPACES).unwrap_or(0.0);
                        let fly_to_mode =
                            query_string_value(entry_node, "flyToMode", GX_NAMESPACES).unwrap_or_default();
                        let view = parse_look_at(entry_node, &ellipsoid)
                            .map(KmlTourView::LookAt)
                            .or_else(|| parse_camera(entry_node, &ellipsoid).map(KmlTourView::Camera));
                        if let Some(view) = view {
                            tour.add_playlist_entry(Rc::new(KmlTourFlyTo::new(duration, fly_to_mode, view)));
                        }
                    }
                    "Wait" => {
                        let duration = query_numeric_value(entry_node, "duration", GX_NAMESPACES).unwrap_or(0.0);
                        tour.add_playlist_entry(Rc::new(KmlTourWait::new(duration)));
                    }
                    _ => {}
                }
            }
        }

        self.kml_tours.borrow_mut().push(tour);
        Ok(())
    }

    fn process_network_link(
        &self,
        node: Node<'_, '_>,
        processing: &ProcessingData<'_>,
        pending_links: &mut Vec<PendingNetworkLink>,
    ) -> Result<(), RuntimeError> {
        let (network_entity, _) = self.process_feature(node, processing)?;
        let link =
            query_first_node(node, "Link", KML_NAMESPACES).or_else(|| query_first_node(node, "Url", KML_NAMESPACES));
        let Some(link) = link else {
            return Ok(());
        };
        let Some(raw_href) = query_string_value(link, "href", KML_NAMESPACES) else {
            return Ok(());
        };
        let Some(href) = resolve_href(Some(&raw_href), processing.source_resource, processing.uri_resolver) else {
            return Ok(());
        };

        let mut view_bound_scale = 1.0;
        let mut view_refresh_time = None;
        let new_source_uri = if href.is_data_uri() {
            let src_url = processing.source_resource.get_url_component(false, false);
            if !src_url.to_ascii_lowercase().contains(".kmz") {
                processing
                    .source_resource
                    .get_derived_resource(DerivedResourceOptions { url: Some(raw_href.clone()), ..Default::default() })
                    .unwrap_or_else(|| Resource::new(raw_href.clone()).unwrap_or_default())
            } else {
                Resource::new(raw_href.clone()).unwrap_or_default()
            }
        } else {
            let view_refresh_mode = query_string_value(link, "viewRefreshMode", KML_NAMESPACES);
            if view_refresh_mode.as_deref() == Some("onRegion") {
                return Ok(());
            }
            view_bound_scale = query_numeric_value(link, "viewBoundScale", KML_NAMESPACES).unwrap_or(1.0);
            let default_view_format = if view_refresh_mode.as_deref() == Some("onStop") {
                view_refresh_time = Some(query_numeric_value(link, "viewRefreshTime", KML_NAMESPACES).unwrap_or(0.0));
                "BBOX=[bboxWest],[bboxSouth],[bboxEast],[bboxNorth]"
            } else {
                ""
            };
            let view_format = query_string_value(link, "viewFormat", KML_NAMESPACES)
                .unwrap_or_else(|| default_view_format.to_owned());
            let http_query = query_string_value(link, "httpQuery", KML_NAMESPACES);
            if !view_format.is_empty() {
                set_query_string(&href, &view_format);
            }
            if let Some(q) = http_query {
                set_query_string(&href, &q);
            }
            href.clone()
        };

        let refresh_mode = query_string_value(link, "refreshMode", KML_NAMESPACES);
        let refresh_interval = query_numeric_value(link, "refreshInterval", KML_NAMESPACES).unwrap_or(0.0);

        pending_links.push(PendingNetworkLink {
            network_entity,
            href,
            new_source_uri,
            refresh_mode,
            refresh_interval,
            view_refresh_time,
            view_bound_scale,
            screen_overlay_container: processing.screen_overlay_container.cloned(),
        });
        Ok(())
    }

    fn raise_unsupported_node(&self, node: Node<'_, '_>, processing: &ProcessingData<'_>) {
        if let Some(src) = self.owner.upgrade() {
            let ds_ref: DataSourceRef = src;
            self.unsupported_node_event.raise(KmlUnsupportedNodeEvent {
                data_source: ds_ref,
                parent_entity: processing.parent_entity.clone(),
                node_name: node.tag_name().name().to_owned(),
                entity_collection: processing.entity_collection.clone(),
                source_uri: processing.source_resource.get_url_component(false, false),
            });
        }
    }
}

impl DataSource for KmlDataSource {
    fn state(&self) -> &DataSourceState {
        &self.state
    }
    fn clock(&self) -> Option<Rc<DataSourceClock>> {
        self.clock.borrow().clone()
    }
    fn update(&self, _time: &JulianDate) -> Result<bool, RuntimeError> {
        let mut context = Context::from_waker(futures_util::task::noop_waker_ref());
        if let Poll::Ready(result) = self.poll_network_links(None, &mut context) {
            result?;
        }
        Ok(true)
    }
    fn destroy(&self) {
        self.clear_network_links();
        self.screen_overlays.borrow_mut().clear();
        if let Some(container) = self.screen_overlay_container.borrow().as_ref() {
            container.borrow_mut().clear();
        }
    }
}

enum KmlPayload {
    Xml(String),
    Kmz(Vec<u8>),
}

fn is_feature_tag(name: &str) -> bool {
    matches!(
        name,
        "Document"
            | "Folder"
            | "Placemark"
            | "NetworkLink"
            | "GroundOverlay"
            | "PhotoOverlay"
            | "ScreenOverlay"
            | "Tour"
    )
}

fn encode_zip_entry_data_uri(entry: &zip::ZipEntry) -> String {
    let mime = detect_mime_type(&entry.filename);
    let b64 = BASE64_STANDARD.encode(&entry.data);
    format!("data:{mime};base64,{b64}")
}

pub(crate) fn create_entity(
    node: Node<'_, '_>,
    entity_collection: &EntityCollection,
    context: Option<&str>,
) -> Result<EntityRef, RuntimeError> {
    let raw_id = query_string_attribute(node, "id").filter(|s| !s.is_empty());
    let mut id = raw_id.unwrap_or_else(create_guid);
    if let Some(ctx) = context {
        id = format!("{ctx}{id}");
    }
    if entity_collection.get_by_id(&id).is_some() {
        let fresh = create_guid();
        id = match context {
            Some(ctx) => format!("{ctx}{fresh}"),
            None => fresh,
        };
    }
    let entity = Entity::new(EntityOptions { id: Some(id), ..Default::default() })?;
    entity_collection.add(entity.clone())?;
    if !entity.property_names().iter().any(|n| n == "kml") {
        entity.add_property("kml")?;
    }
    entity.set_property("kml", Some(PropertyInput::Component(Rc::new(KmlFeatureData::new()))))?;
    Ok(entity)
}

fn ancestry_is_visible(parent: Option<&EntityRef>) -> bool {
    let mut cur = parent.cloned();
    while let Some(p) = cur {
        if !p.show() {
            return false;
        }
        cur = p.parent();
    }
    true
}

fn process_time_stamp(feature_node: Node<'_, '_>) -> Result<Option<Availability>, RuntimeError> {
    let Some(node) = query_first_node(feature_node, "TimeStamp", KML_GX_NAMESPACES) else {
        return Ok(None);
    };
    let Some(when_str) = query_string_value(node, "when", KML_GX_NAMESPACES) else {
        return Ok(None);
    };
    if when_str.is_empty() {
        return Ok(None);
    }
    let when = JulianDate::from_iso8601(&when_str).map_err(|e| RuntimeError::new(e.to_string()))?;
    let result: Availability = Rc::new(PropertyIntervals::new(|a: &Option<()>, b: &Option<()>| a == b));
    result.add_interval(TimeInterval::new(when, iso8601::maximum_value(), true, true, None));
    Ok(Some(result))
}

fn process_time_span(feature_node: Node<'_, '_>) -> Result<Option<Availability>, RuntimeError> {
    let Some(node) = query_first_node(feature_node, "TimeSpan", KML_GX_NAMESPACES) else {
        return Ok(None);
    };
    let begin_date = match query_first_node(node, "begin", KML_GX_NAMESPACES) {
        Some(n) => {
            let s = node_text_content(n);
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(JulianDate::from_iso8601(trimmed).map_err(|e| RuntimeError::new(e.to_string()))?)
            }
        }
        None => None,
    };
    let end_date = match query_first_node(node, "end", KML_GX_NAMESPACES) {
        Some(n) => {
            let s = node_text_content(n);
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(JulianDate::from_iso8601(trimmed).map_err(|e| RuntimeError::new(e.to_string()))?)
            }
        }
        None => None,
    };

    let (start, stop) = match (begin_date, end_date) {
        (Some(mut b), Some(mut e)) => {
            if JulianDate::less_than(&e, &b) {
                std::mem::swap(&mut b, &mut e);
            }
            (b, e)
        }
        (Some(b), None) => (b, iso8601::maximum_value()),
        (None, Some(e)) => (iso8601::minimum_value(), e),
        (None, None) => return Ok(None),
    };
    let result: Availability = Rc::new(PropertyIntervals::new(|a: &Option<()>, b: &Option<()>| a == b));
    result.add_interval(TimeInterval::new(start, stop, true, true, None));
    Ok(Some(result))
}

fn merge_availability_with_parent(child: &EntityRef) {
    let Some(parent) = child.parent() else {
        return;
    };
    let Some(parent_avail) = parent.availability() else {
        return;
    };
    // Upstream CesiumJS quirk (`KmlDataSource.js:3873`):
    // `childAvailability.intersect(parentAvailability)` returns a new collection
    // without mutating `childAvailability` in place, so only a child with no
    // availability of its own inherits the parent's availability.
    if child.availability().is_none() {
        let copied: Availability = Rc::new(PropertyIntervals::new(|a: &Option<()>, b: &Option<()>| a == b));
        for iv in parent_avail.read().intervals() {
            copied.add_interval(iv.clone());
        }
        child.set_availability(Some(copied));
    }
}

fn process_extended_data(node: Node<'_, '_>, kml_data: &KmlFeatureData) {
    let Some(ext_node) = query_first_node(node, "ExtendedData", KML_NAMESPACES) else {
        return;
    };
    let mut entries = Vec::new();
    for data_node in query_child_nodes(ext_node, "Data", KML_NAMESPACES) {
        if let Some(name) = query_string_attribute(data_node, "name") {
            let entry = KmlExtendedDataEntry {
                display_name: query_string_value(data_node, "displayName", KML_NAMESPACES),
                value: query_string_value(data_node, "value", KML_NAMESPACES),
            };
            if let Some(existing) = entries.iter_mut().find(|(k, _)| k == &name) {
                existing.1 = entry;
            } else {
                entries.push((name, entry));
            }
        }
    }
    kml_data.set_extended_data(Some(entries));
}

fn parse_camera(node: Node<'_, '_>, ellipsoid: &Ellipsoid) -> Option<KmlCamera> {
    let cam = query_first_node(node, "Camera", KML_NAMESPACES)?;
    let lon = query_numeric_value(cam, "longitude", KML_NAMESPACES).unwrap_or(0.0);
    let lat = query_numeric_value(cam, "latitude", KML_NAMESPACES).unwrap_or(0.0);
    let alt = query_numeric_value(cam, "altitude", KML_NAMESPACES).unwrap_or(0.0);
    let heading = query_numeric_value(cam, "heading", KML_NAMESPACES).unwrap_or(0.0);
    let tilt = query_numeric_value(cam, "tilt", KML_NAMESPACES).unwrap_or(0.0);
    let roll = query_numeric_value(cam, "roll", KML_NAMESPACES).unwrap_or(0.0);
    let pos = Cartesian3::from_degrees(lon, lat, alt, ellipsoid);
    let hpr = HeadingPitchRoll::from_degrees(heading, tilt - 90.0, roll);
    Some(KmlCamera::new(pos, hpr))
}

fn parse_look_at(node: Node<'_, '_>, ellipsoid: &Ellipsoid) -> Option<KmlLookAt> {
    let look_at = query_first_node(node, "LookAt", KML_NAMESPACES)?;
    let lon = query_numeric_value(look_at, "longitude", KML_NAMESPACES).unwrap_or(0.0);
    let lat = query_numeric_value(look_at, "latitude", KML_NAMESPACES).unwrap_or(0.0);
    let alt = query_numeric_value(look_at, "altitude", KML_NAMESPACES).unwrap_or(0.0);
    let heading = math::to_radians(query_numeric_value(look_at, "heading", KML_NAMESPACES).unwrap_or(0.0));
    let tilt = math::to_radians(query_numeric_value(look_at, "tilt", KML_NAMESPACES).unwrap_or(0.0));
    let range = query_numeric_value(look_at, "range", KML_NAMESPACES).unwrap_or(0.0);
    let hpr = HeadingPitchRange::new(heading, tilt - math::PI_OVER_TWO, range);
    let pos = Cartesian3::from_degrees(lon, lat, alt, ellipsoid);
    Some(KmlLookAt::new(pos, hpr))
}

fn extract_network_link_control(root: Node<'_, '_>) -> Option<NetworkLinkControlInfo> {
    let nlc = query_first_node(root, "NetworkLinkControl", KML_NAMESPACES)?;
    let has_update = query_first_node(nlc, "Update", KML_NAMESPACES).is_some();
    let cookie_str = query_string_value(nlc, "cookie", KML_NAMESPACES).unwrap_or_default();
    let cookie = query_to_object(&cookie_str).unwrap_or_default().into_iter().map(|(k, v)| (k, Some(v))).collect();
    let min_refresh_period = query_numeric_value(nlc, "minRefreshPeriod", KML_NAMESPACES).unwrap_or(0.0);
    let expires = query_string_value(nlc, "expires", KML_NAMESPACES);
    Some(NetworkLinkControlInfo { has_update, cookie, min_refresh_period, expires })
}

fn apply_min_refresh(date: JulianDate, now: JulianDate, minimum: f64) -> JulianDate {
    let remaining = JulianDate::seconds_difference(&date, &now);
    if remaining > 0.0 && remaining < minimum {
        now.add_seconds(minimum)
    } else {
        date
    }
}

fn process_network_link_query_string(
    resource: &Resource,
    view: Option<KmlCameraView>,
    scale: f64,
    ellipsoid: Ellipsoid,
) {
    let entries: Vec<_> = resource
        .options()
        .query_parameters
        .iter()
        .map(|(key, value)| (key.clone(), value.clone().unwrap_or_else(|| QueryValue::Single(String::new()))))
        .collect();
    if entries.is_empty() {
        return;
    }
    let mut query = object_to_query(&entries).replace("%5B", "[").replace("%5D", "]");
    let mut bounds = Rectangle::MAX_VALUE;
    let mut center = None;
    let mut camera = None;
    let mut range = None;
    let mut fov = None;
    let camera_view = view.filter(|view| view.mode != SceneMode::Morphing);
    if let Some(view) = camera_view {
        bounds = view.bounds;
        let target = ellipsoid.cartesian_to_cartographic(view.center).unwrap_or_else(|| view.bounds.center());
        if (scale - 1.0).abs() > 1e-9 {
            let half_width = bounds.width() * scale * 0.5;
            let half_height = bounds.height() * scale * 0.5;
            bounds = Rectangle::new(
                math::negative_pi_to_pi(target.longitude - half_width),
                math::clamp_to_latitude_range(target.latitude - half_height),
                math::negative_pi_to_pi(target.longitude + half_width),
                math::clamp_to_latitude_range(target.latitude + half_height),
            );
        }
        center = Some(target);
        camera = ellipsoid.cartesian_to_cartographic(view.position);
        range = Some(view.position.distance(view.center));
        fov = view.fov.map(
            |(angle, aspect)| {
                if aspect > 1.0 {
                    (angle, angle / aspect)
                } else {
                    (angle * aspect, angle)
                }
            },
        );
    }

    let degrees = |angle: Option<f64>| angle.map(f64::to_degrees);
    let lon = |cartographic: Option<Cartographic>| degrees(cartographic.map(|p| p.longitude));
    let lat = |cartographic: Option<Cartographic>| degrees(cartographic.map(|p| p.latitude));
    let substitutions = [
        ("bboxWest", degrees(Some(bounds.west))),
        ("bboxSouth", degrees(Some(bounds.south))),
        ("bboxEast", degrees(Some(bounds.east))),
        ("bboxNorth", degrees(Some(bounds.north))),
        ("lookatLon", lon(center)),
        ("lookatLat", lat(center)),
        ("lookatRange", range),
        ("lookatTilt", degrees(camera_view.and_then(|v| v.pitch))),
        ("lookatHeading", degrees(camera_view.and_then(|v| v.heading))),
        ("lookatTerrainLon", lon(center)),
        ("lookatTerrainLat", lat(center)),
        ("lookatTerrainAlt", center.map(|p| p.height)),
        ("cameraLon", lon(camera)),
        ("cameraLat", lat(camera)),
        ("cameraAlt", degrees(camera.map(|p| p.height))),
        ("horizFov", fov.map(|f| f.0)),
        ("vertFov", fov.map(|f| f.1)),
        ("horizPixels", view.map(|v| v.width)),
        ("vertPixels", view.map(|v| v.height)),
    ];
    for (key, value) in substitutions {
        let value = value.map_or_else(String::new, |number| (number + 0.0).to_string());
        query = query.replacen(&format!("[{key}]"), &value, 1);
    }
    for (key, value) in [
        ("terrainEnabled", "1"),
        ("clientVersion", "1"),
        ("kmlVersion", "2.2"),
        ("clientName", "Cesium"),
        ("language", "English"),
    ] {
        query = query.replacen(&format!("[{key}]"), value, 1);
    }
    if let Some(parsed) = query_to_object(&query) {
        resource.options_mut().query_parameters = parsed.into_iter().map(|(key, value)| (key, Some(value))).collect();
    }
}

fn collect_descendant_ids(entity: &EntityRef, out: &mut HashSet<String>) {
    for child in entity.children() {
        out.insert(child.id().to_owned());
        collect_descendant_ids(&child, out);
    }
}

fn remove_entity_subtree(collection: &EntityCollection, entity: &EntityRef) {
    collection.remove(Some(entity));
    for child in entity.children() {
        remove_entity_subtree(collection, &child);
    }
}
