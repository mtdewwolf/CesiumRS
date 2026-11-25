//! CesiumJS `CzmlDataSource`: processes CZML 1.0 streams and documents into
//! time-dynamic entities, properties, materials, graphics, and clock state.
mod adapters;
mod graphics;
mod materials;
mod packet;
mod position;
mod unwrap;

pub use adapters::CzmlPropertyTarget;

use self::{
    materials::process_material_packet_data,
    packet::{process_packet_data, PacketContext},
    position::process_position_packet_data,
    unwrap::{get_defined, interval_from_string, parse_clock_range, parse_clock_step},
};
use super::{DataSource, DataSourceClock, DataSourceState, EntityCollection, EntityRef};
use crate::core::{
    create_guid::create_guid, iso8601, uri_helpers::get_filename_from_uri, ClockRange, ClockStep, Credit, JulianDate,
    Resource, ResourceClient, RuntimeError, TimeInterval,
};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

thread_local! {
    static CURRENT_ID: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(crate) fn current_packet_id() -> Option<String> {
    CURRENT_ID.with(|cell| cell.borrow().clone())
}

struct CurrentIdGuard(Option<String>);

impl CurrentIdGuard {
    fn enter(id: String) -> Self {
        let prev = CURRENT_ID.with(|cell| cell.replace(Some(id)));
        Self(prev)
    }
}

impl Drop for CurrentIdGuard {
    fn drop(&mut self) {
        let prev = self.0.take();
        CURRENT_ID.with(|cell| {
            *cell.borrow_mut() = prev;
        });
    }
}

/// Value type descriptor passed to [`CzmlDataSource::process_packet_data`],
/// mirroring the constructor-type parameter in CesiumJS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CzmlValueType {
    ArcType,
    Array,
    Boolean,
    BoundingRectangle,
    Cartesian2,
    Cartesian3,
    UnitCartesian3,
    Color,
    ClassificationType,
    ColorBlendMode,
    CornerType,
    HeightReference,
    HorizontalOrigin,
    Image,
    JulianDate,
    LabelStyle,
    Number,
    NearFarScalar,
    DistanceDisplayCondition,
    Object,
    Quaternion,
    Rotation,
    SensorVolumePortionToDisplay,
    ShadowMode,
    String,
    StripeOrientation,
    Rectangle,
    Uri,
    VerticalOrigin,
}

/// Signature for a CZML packet updater function.
pub type CzmlUpdaterFn =
    dyn Fn(&EntityRef, &Value, &Rc<EntityCollection>, Option<&Resource>) -> Result<(), RuntimeError>;

/// Handle to a registered CZML packet updater function.
#[derive(Clone)]
pub struct CzmlUpdater(Rc<CzmlUpdaterFn>);

impl CzmlUpdater {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&EntityRef, &Value, &Rc<EntityCollection>, Option<&Resource>) -> Result<(), RuntimeError> + 'static,
    {
        Self(Rc::new(f))
    }

    pub fn call(
        &self,
        entity: &EntityRef,
        packet: &Value,
        entity_collection: &Rc<EntityCollection>,
        source_uri: Option<&Resource>,
    ) -> Result<(), RuntimeError> {
        (self.0)(entity, packet, entity_collection, source_uri)
    }
}

impl PartialEq for CzmlUpdater {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for CzmlUpdater {}

/// Options for [`CzmlDataSource::load`], [`CzmlDataSource::process`], and
/// [`CzmlDataSource::load_new`].
#[derive(Clone, Default)]
pub struct CzmlLoadOptions {
    /// Overrides the URI used for resolving relative links and default name.
    pub source_uri: Option<String>,
    /// Credit displayed for the data source.
    pub credit: Option<Credit>,
    /// Host HTTP client used when loading a URL or [`Resource`].
    pub resource_client: Option<ResourceClient>,
}

/// Accepted input forms for [`CzmlDataSource::load`] and [`CzmlDataSource::process`].
pub enum CzmlInput<'a> {
    Json(Value),
    JsonRef(&'a Value),
    Text(String),
    Resource(Resource),
}

impl From<Value> for CzmlInput<'static> {
    fn from(v: Value) -> Self {
        Self::Json(v)
    }
}

impl<'a> From<&'a Value> for CzmlInput<'a> {
    fn from(v: &'a Value) -> Self {
        Self::JsonRef(v)
    }
}

impl From<&str> for CzmlInput<'static> {
    fn from(s: &str) -> Self {
        Self::Text(s.to_owned())
    }
}

impl From<String> for CzmlInput<'static> {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<Resource> for CzmlInput<'static> {
    fn from(r: Resource) -> Self {
        Self::Resource(r)
    }
}

#[derive(Clone, Default)]
struct ClockPacket {
    interval: Option<String>,
    current_time: Option<String>,
    range: Option<String>,
    step: Option<String>,
    multiplier: Option<f64>,
}

#[derive(Clone, Default)]
struct DocumentPacket {
    name: Option<String>,
    clock: Option<ClockPacket>,
}

/// Port of CesiumJS `CzmlDataSource`.
pub struct CzmlDataSource {
    state: DataSourceState,
    clock: RefCell<Option<Rc<DataSourceClock>>>,
    document_packet: RefCell<DocumentPacket>,
    version: RefCell<Option<String>>,
    credit: RefCell<Option<Credit>>,
    resource_credits: RefCell<Vec<Credit>>,
    source_uri: RefCell<Option<Resource>>,
    updaters: RefCell<Vec<CzmlUpdater>>,
    custom_updaters: Cell<bool>,
}

impl CzmlDataSource {
    pub fn new(name: Option<String>) -> Rc<Self> {
        Rc::new_cyclic(|owner| Self {
            state: DataSourceState::new(owner.clone(), name),
            clock: RefCell::new(None),
            document_packet: RefCell::new(DocumentPacket::default()),
            version: RefCell::new(None),
            credit: RefCell::new(None),
            resource_credits: RefCell::new(Vec::new()),
            source_uri: RefCell::new(None),
            updaters: RefCell::new(Vec::new()),
            custom_updaters: Cell::new(false),
        })
    }

    fn ensure_updaters_initialized(&self) {
        let mut list = self.updaters.borrow_mut();
        if list.is_empty() && !self.custom_updaters.get() {
            *list = graphics::builtin_updaters();
        }
    }

    /// Creates a new `CzmlDataSource` and loads `czml` into it.
    pub async fn load_new<'a>(
        czml: impl Into<CzmlInput<'a>>,
        options: &CzmlLoadOptions,
    ) -> Result<Rc<Self>, RuntimeError> {
        let source = Self::new(None);
        source.load(czml, options).await?;
        Ok(source)
    }

    pub fn version(&self) -> Option<String> {
        self.version.borrow().clone()
    }

    pub fn credit(&self) -> Option<Credit> {
        self.credit.borrow().clone()
    }

    pub fn resource_credits(&self) -> Vec<Credit> {
        self.resource_credits.borrow().clone()
    }

    pub fn source_uri(&self) -> Option<Resource> {
        self.source_uri.borrow().clone()
    }

    /// Returns a snapshot of the registered packet updaters.
    pub fn updaters(&self) -> Vec<CzmlUpdater> {
        self.ensure_updaters_initialized();
        self.updaters.borrow().clone()
    }

    /// Adds a custom updater function if not already registered.
    pub fn register_updater(&self, updater: CzmlUpdater) {
        self.ensure_updaters_initialized();
        self.custom_updaters.set(true);
        let mut list = self.updaters.borrow_mut();
        if !list.contains(&updater) {
            list.push(updater);
        }
    }

    /// Removes a previously registered updater function.
    pub fn unregister_updater(&self, updater: &CzmlUpdater) -> bool {
        self.ensure_updaters_initialized();
        let mut list = self.updaters.borrow_mut();
        if let Some(pos) = list.iter().position(|u| u == updater) {
            list.remove(pos);
            self.custom_updaters.set(true);
            true
        } else {
            false
        }
    }

    /// Replaces existing entities and loads the provided CZML data.
    pub async fn load<'a>(
        &self,
        czml: impl Into<CzmlInput<'a>>,
        options: &CzmlLoadOptions,
    ) -> Result<(), RuntimeError> {
        self.load_impl(czml.into(), options, true).await
    }

    /// Incrementally processes CZML data without clearing existing entities.
    pub async fn process<'a>(
        &self,
        czml: impl Into<CzmlInput<'a>>,
        options: &CzmlLoadOptions,
    ) -> Result<(), RuntimeError> {
        self.load_impl(czml.into(), options, false).await
    }

    /// Public helper for custom updaters mirroring `CzmlDataSource.processPacketData`.
    pub fn process_packet_data(
        ty: CzmlValueType,
        target: &dyn CzmlPropertyTarget,
        property_name: &str,
        packet_data: Option<&Value>,
        interval: Option<&TimeInterval<()>>,
        source_uri: Option<&Resource>,
        entity_collection: &Rc<EntityCollection>,
    ) -> Result<(), RuntimeError> {
        let ctx = PacketContext { interval, source_uri, entity_collection };
        process_packet_data(ty, target, property_name, packet_data, &ctx)
    }

    /// Public helper for custom updaters mirroring `CzmlDataSource.processPositionPacketData`.
    pub fn process_position_packet_data(
        target: &dyn CzmlPropertyTarget,
        property_name: &str,
        packet_data: Option<&Value>,
        interval: Option<&TimeInterval<()>>,
        source_uri: Option<&Resource>,
        entity_collection: &Rc<EntityCollection>,
    ) -> Result<(), RuntimeError> {
        let ctx = PacketContext { interval, source_uri, entity_collection };
        process_position_packet_data(target, property_name, packet_data, &ctx)
    }

    /// Public helper for custom updaters mirroring `CzmlDataSource.processMaterialPacketData`.
    pub fn process_material_packet_data(
        target: &dyn CzmlPropertyTarget,
        property_name: &str,
        packet_data: Option<&Value>,
        interval: Option<&TimeInterval<()>>,
        source_uri: Option<&Resource>,
        entity_collection: &Rc<EntityCollection>,
    ) -> Result<(), RuntimeError> {
        let ctx = PacketContext { interval, source_uri, entity_collection };
        process_material_packet_data(target, property_name, packet_data, &ctx)
    }

    async fn load_impl<'a>(
        &self,
        input: CzmlInput<'a>,
        options: &CzmlLoadOptions,
        clear: bool,
    ) -> Result<(), RuntimeError> {
        *self.credit.borrow_mut() = options.credit.clone();

        let mut source_uri = match &options.source_uri {
            Some(uri) => Resource::new(uri.clone()),
            None => None,
        };

        self.set_loading(true)?;

        enum ResolvedJson<'a> {
            Owned(Value),
            Borrowed(&'a Value),
        }

        let resolved: Result<(ResolvedJson<'a>, Option<Resource>), RuntimeError> = async {
            let input = match input {
                CzmlInput::Text(s) if s.trim_start().starts_with(['[', '{']) => {
                    CzmlInput::Json(serde_json::from_str(&s).map_err(|e| RuntimeError::new(e.to_string()))?)
                }
                CzmlInput::Text(url) => {
                    CzmlInput::Resource(Resource::new(url).ok_or_else(|| RuntimeError::new("Invalid CZML URL."))?)
                }
                other => other,
            };

            let json = match input {
                CzmlInput::Json(v) => ResolvedJson::Owned(v),
                CzmlInput::JsonRef(v) => ResolvedJson::Borrowed(v),
                CzmlInput::Resource(resource) => {
                    let client = options
                        .resource_client
                        .as_ref()
                        .ok_or_else(|| RuntimeError::new("Resource loading requires a ResourceClient."))?;
                    if source_uri.is_none() {
                        source_uri = Some(resource.clone());
                    }
                    if let Some(credits) = resource.credits() {
                        self.resource_credits.borrow_mut().extend(credits);
                    }
                    ResolvedJson::Owned(
                        resource
                            .fetch_json(client)
                            .ok_or_else(|| RuntimeError::new("CZML request was not scheduled."))?
                            .await
                            .map_err(|e| RuntimeError::new(e.to_string()))?
                            .ok_or_else(|| RuntimeError::new("Empty CZML response."))?,
                    )
                }
                CzmlInput::Text(_) => unreachable!(),
            };
            Ok((json, source_uri))
        }
        .await;

        let res = match resolved {
            Ok((json, uri)) => {
                *self.source_uri.borrow_mut() = uri.clone();
                let val = match &json {
                    ResolvedJson::Owned(v) => v,
                    ResolvedJson::Borrowed(v) => v,
                };
                self.load_czml(val, uri.as_ref(), clear)
            }
            Err(e) => Err(e),
        };

        if let Err(err) = &res {
            let _ = self.set_loading(false);
            self.state.notify_error(err.clone());
        }
        res
    }

    fn load_czml(&self, czml: &Value, source_uri: Option<&Resource>, clear: bool) -> Result<(), RuntimeError> {
        let _iso_cache = unwrap::IsoCacheGuard::enter();
        self.set_loading(true)?;
        let entities = self.entities().clone();

        if clear {
            *self.version.borrow_mut() = None;
            *self.document_packet.borrow_mut() = DocumentPacket::default();
            entities.remove_all();
        }

        let custom = self.custom_updaters.get();
        let updaters = if custom { self.updaters.borrow().clone() } else { Vec::new() };
        if let Some(arr) = czml.as_array() {
            for packet in arr {
                self.process_czml_packet(packet, &entities, custom.then_some(updaters.as_slice()), source_uri)?;
            }
        } else if czml.is_object() {
            self.process_czml_packet(czml, &entities, custom.then_some(updaters.as_slice()), source_uri)?;
        } else {
            return Err(RuntimeError::new("CZML input must be a JSON object or array of packets."));
        }

        let mut raise_changed = self.update_clock(clear)?;

        let doc_name = self.document_packet.borrow().name.clone();
        if let Some(doc_name) = doc_name {
            if self.name().as_deref() != Some(doc_name.as_str()) {
                self.state.set_name_silent(Some(doc_name));
                raise_changed = true;
            }
        } else if self.name().is_none() {
            if let Some(uri) = source_uri {
                self.state.set_name_silent(Some(get_filename_from_uri(&uri.get_url_component(false, false))));
                raise_changed = true;
            }
        }

        self.set_loading(false)?;
        if raise_changed {
            self.state.notify_changed();
        }
        Ok(())
    }

    fn process_czml_packet(
        &self,
        packet: &Value,
        entities: &Rc<EntityCollection>,
        custom_updaters: Option<&[CzmlUpdater]>,
        source_uri: Option<&Resource>,
    ) -> Result<(), RuntimeError> {
        if !packet.is_object() {
            return Err(RuntimeError::new("CZML packet must be a JSON object."));
        }

        let object_id =
            get_defined(packet, "id").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(create_guid);

        let _guard = CurrentIdGuard::enter(object_id.clone());

        if self.version.borrow().is_none() && object_id != "document" {
            return Err(RuntimeError::new("The first CZML packet is required to be the document object."));
        }

        if get_defined(packet, "delete") == Some(&Value::Bool(true)) {
            entities.remove_by_id(&object_id);
        } else if object_id == "document" {
            self.process_document(packet)?;
        } else {
            let entity = entities.get_or_create_entity(object_id);
            if let Some(parent_id) = get_defined(packet, "parent").and_then(Value::as_str) {
                let parent = entities.get_or_create_entity(parent_id);
                entity.set_parent(Some(parent))?;
            }
            match custom_updaters {
                Some(updaters) => {
                    for updater in updaters.iter().rev() {
                        updater.call(&entity, packet, entities, source_uri)?;
                    }
                }
                None => {
                    graphics::apply_builtin_packet(&entity, packet, entities, source_uri)?;
                }
            }
        }
        Ok(())
    }

    fn process_document(&self, packet: &Value) -> Result<(), RuntimeError> {
        if let Some(version_str) = get_defined(packet, "version").and_then(Value::as_str) {
            let tokens: Vec<&str> = version_str.split('.').collect();
            if tokens.len() == 2 {
                if tokens[0] != "1" {
                    return Err(RuntimeError::new("Cesium only supports CZML version 1."));
                }
                *self.version.borrow_mut() = Some(version_str.to_owned());
            }
        }

        if self.version.borrow().is_none() {
            return Err(RuntimeError::new(
                "CZML version information invalid.  It is expected to be a property on the document object in the <Major>.<Minor> version format.",
            ));
        }

        let mut doc = self.document_packet.borrow_mut();
        if let Some(name) = get_defined(packet, "name").and_then(Value::as_str) {
            doc.name = Some(name.to_owned());
        }

        if let Some(clock_val) = get_defined(packet, "clock") {
            let interval = get_defined(clock_val, "interval").and_then(Value::as_str).map(str::to_owned);
            let current_time = get_defined(clock_val, "currentTime").and_then(Value::as_str).map(str::to_owned);
            let range = get_defined(clock_val, "range").and_then(Value::as_str).map(str::to_owned);
            let step = get_defined(clock_val, "step").and_then(Value::as_str).map(str::to_owned);
            let multiplier = get_defined(clock_val, "multiplier").and_then(Value::as_f64);

            match &mut doc.clock {
                None => {
                    doc.clock = Some(ClockPacket { interval, current_time, range, step, multiplier });
                }
                Some(existing) => {
                    if interval.is_some() {
                        existing.interval = interval;
                    }
                    if current_time.is_some() {
                        existing.current_time = current_time;
                    }
                    if range.is_some() {
                        existing.range = range;
                    }
                    if step.is_some() {
                        existing.step = step;
                    }
                    if multiplier.is_some() {
                        existing.multiplier = multiplier;
                    }
                }
            }
        }
        Ok(())
    }

    fn update_clock(&self, replace: bool) -> Result<bool, RuntimeError> {
        let clock_packet = self.document_packet.borrow().clock.clone();
        let existing = self.clock.borrow().clone();
        let previous = existing.as_deref().filter(|_| !replace);

        let clock = if let Some(clock_packet) = clock_packet {
            let clock = previous.cloned().unwrap_or_else(|| {
                let start = iso8601::minimum_value();
                let stop = iso8601::maximum_value();
                new_clock(start, stop, 1.0)
            });
            if let Some(iv) = interval_from_string(clock_packet.interval.as_deref())? {
                clock.set_start_time(Some(Rc::new(iv.start)));
                clock.set_stop_time(Some(Rc::new(iv.stop)));
            }
            if let Some(ct) = clock_packet.current_time.as_deref() {
                let jd = JulianDate::from_iso8601(ct).map_err(|e| RuntimeError::new(e.to_string()))?;
                clock.set_current_time(Some(Rc::new(jd)));
            }
            if let Some(range) = clock_packet.range.as_deref() {
                clock.set_clock_range(Some(parse_clock_range(range).unwrap_or(ClockRange::LoopStop)));
            }
            if let Some(step) = clock_packet.step.as_deref() {
                clock.set_clock_step(Some(parse_clock_step(step).unwrap_or(ClockStep::SystemClockMultiplier)));
            }
            if let Some(mult) = clock_packet.multiplier {
                clock.set_multiplier(Some(mult));
            }
            clock
        } else {
            let availability = self.entities().compute_availability();
            if availability.start == iso8601::minimum_value() {
                return Ok(self.clock.take().is_some());
            }
            let start = availability.start;
            let stop = availability.stop;
            let multiplier = js_round(JulianDate::seconds_difference(&stop, &start) / 120.0);
            let clock = new_clock(start, stop, multiplier);
            if let Some(current) = previous
                .and_then(DataSourceClock::current_time)
                .filter(|t| JulianDate::less_than_or_equals(&start, t) && JulianDate::less_than_or_equals(t, &stop))
            {
                clock.set_current_time(Some(current));
            }
            clock
        };

        if let Some(existing) = existing {
            if clock.equals(&existing) {
                return Ok(false);
            }
            clock.clone_into(&existing);
        } else {
            *self.clock.borrow_mut() = Some(Rc::new(clock));
        }
        Ok(true)
    }
}

impl DataSource for CzmlDataSource {
    fn state(&self) -> &DataSourceState {
        &self.state
    }
    fn clock(&self) -> Option<Rc<DataSourceClock>> {
        self.clock.borrow().clone()
    }
}

fn new_clock(start: JulianDate, stop: JulianDate, multiplier: f64) -> DataSourceClock {
    let clock = DataSourceClock::new();
    clock.set_start_time(Some(Rc::new(start)));
    clock.set_stop_time(Some(Rc::new(stop)));
    clock.set_current_time(Some(Rc::new(start)));
    clock.set_clock_range(Some(ClockRange::LoopStop));
    clock.set_clock_step(Some(ClockStep::SystemClockMultiplier));
    clock.set_multiplier(Some(multiplier));
    clock
}

fn js_round(x: f64) -> f64 {
    if x.fract() == -0.5 {
        x.ceil()
    } else {
        x.round()
    }
}
