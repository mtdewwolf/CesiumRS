//! Typed property adapters, target-slot abstractions, and polygon hierarchy
//! composition for [`super::CzmlDataSource`].
use crate::{
    core::{
        BoundingRectangle, Cartesian2, Cartesian3, Color, DistanceDisplayCondition, JulianDate, NearFarScalar,
        PolygonHierarchy, Quaternion, Rectangle, RuntimeError,
    },
    data_sources::{
        entity_component::component_equals, graphics::Graphics, BillboardGraphics, BoxGraphics,
        Cesium3DTilesetGraphics, CheckerboardMaterialProperty, ColorMaterialProperty, ComponentRef, CorridorGraphics,
        CylinderGraphics, DefinitionChanged, DynamicValue, EllipseGraphics, EllipsoidGraphics, Entity, EntityComponent,
        GridMaterialProperty, ImageMaterialProperty, JsonReference, LabelGraphics, ModelGraphics,
        NodeTransformationProperty, PathGraphics, PointGraphics, PolygonGraphics, PolylineArrowMaterialProperty,
        PolylineDashMaterialProperty, PolylineGlowMaterialProperty, PolylineGraphics, PolylineOutlineMaterialProperty,
        PolylineVolumeGraphics, PositionPropertyArray, Property, PropertyArray, PropertyBag, PropertyComponent,
        PropertyInput, PropertyRef, RectangleGraphics, Rotation, SampledProperty, SampledValue, StripeMaterialProperty,
        Subscription, VelocityOrientationProperty, VelocityVectorProperty, WallGraphics,
    },
};
use serde_json::{Map, Value};
use std::{any::Any, cell::RefCell, rc::Rc};

/// Read/write abstraction over any CesiumRS object whose named properties can
/// be populated by CZML packet processors (`Entity`, `*Graphics`, `PropertyBag`,
/// `*MaterialProperty`, `NodeTransformationProperty`, or a temporary slot).
pub trait CzmlPropertyTarget {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput>;
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError>;
}

impl CzmlPropertyTarget for Entity {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        self.set_property(name, value)
    }
}

impl CzmlPropertyTarget for PropertyBag {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        if !self.has_property(name) {
            self.add_property(name, None, None)?;
        }
        self.set(name, value)
    }
}

macro_rules! impl_graphics_target {
    ($($ty:ty),* $(,)?) => {
        $(
            impl CzmlPropertyTarget for $ty {
                fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
                    Graphics::get(self, name)
                }
                fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
                    Graphics::set(self, name, value)
                }
            }
        )*
    };
}

impl_graphics_target!(
    BillboardGraphics,
    BoxGraphics,
    Cesium3DTilesetGraphics,
    CorridorGraphics,
    CylinderGraphics,
    EllipseGraphics,
    EllipsoidGraphics,
    LabelGraphics,
    ModelGraphics,
    PathGraphics,
    PointGraphics,
    PolygonGraphics,
    PolylineGraphics,
    PolylineVolumeGraphics,
    RectangleGraphics,
    WallGraphics,
);

impl CzmlPropertyTarget for ColorMaterialProperty {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        match name {
            "color" => self.set_color(value),
            _ => Err(RuntimeError::new(format!("{name} is not a ColorMaterialProperty field."))),
        }
    }
}

impl CzmlPropertyTarget for GridMaterialProperty {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        match name {
            "color" => self.set_color(value),
            "cellAlpha" => self.set_cell_alpha(value),
            "lineCount" => self.set_line_count(value),
            "lineThickness" => self.set_line_thickness(value),
            "lineOffset" => self.set_line_offset(value),
            _ => Err(RuntimeError::new(format!("{name} is not a GridMaterialProperty field."))),
        }
    }
}

impl CzmlPropertyTarget for ImageMaterialProperty {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        match name {
            "image" => self.set_image(value),
            "repeat" => self.set_repeat(value),
            "color" => self.set_color(value),
            "transparent" => self.set_transparent(value),
            _ => Err(RuntimeError::new(format!("{name} is not an ImageMaterialProperty field."))),
        }
    }
}

impl CzmlPropertyTarget for StripeMaterialProperty {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        match name {
            "orientation" => self.set_orientation(value),
            "evenColor" => self.set_even_color(value),
            "oddColor" => self.set_odd_color(value),
            "offset" => self.set_offset(value),
            "repeat" => self.set_repeat(value),
            _ => Err(RuntimeError::new(format!("{name} is not a StripeMaterialProperty field."))),
        }
    }
}

impl CzmlPropertyTarget for CheckerboardMaterialProperty {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        match name {
            "evenColor" => self.set_even_color(value),
            "oddColor" => self.set_odd_color(value),
            "repeat" => self.set_repeat(value),
            _ => Err(RuntimeError::new(format!("{name} is not a CheckerboardMaterialProperty field."))),
        }
    }
}

impl CzmlPropertyTarget for PolylineOutlineMaterialProperty {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        match name {
            "color" => self.set_color(value),
            "outlineColor" => self.set_outline_color(value),
            "outlineWidth" => self.set_outline_width(value),
            _ => Err(RuntimeError::new(format!("{name} is not a PolylineOutlineMaterialProperty field."))),
        }
    }
}

impl CzmlPropertyTarget for PolylineGlowMaterialProperty {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        match name {
            "color" => self.set_color(value),
            "glowPower" => self.set_glow_power(value),
            "taperPower" => self.set_taper_power(value),
            _ => Err(RuntimeError::new(format!("{name} is not a PolylineGlowMaterialProperty field."))),
        }
    }
}

impl CzmlPropertyTarget for PolylineArrowMaterialProperty {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        match name {
            "color" => self.set_color(value),
            _ => Err(RuntimeError::new(format!("{name} is not a PolylineArrowMaterialProperty field."))),
        }
    }
}

impl CzmlPropertyTarget for PolylineDashMaterialProperty {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        match name {
            "color" => self.set_color(value),
            "gapColor" => self.set_gap_color(value),
            "dashLength" => self.set_dash_length(value),
            "dashPattern" => self.set_dash_pattern(value),
            _ => Err(RuntimeError::new(format!("{name} is not a PolylineDashMaterialProperty field."))),
        }
    }
}

impl CzmlPropertyTarget for NodeTransformationProperty {
    fn get_czml_property(&self, name: &str) -> Option<PropertyInput> {
        self.get_property(name)
    }
    fn set_czml_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        match name {
            "translation" => self.set_translation(value),
            "rotation" => self.set_rotation(value),
            "scale" => self.set_scale(value),
            _ => Err(RuntimeError::new(format!("{name} is not a NodeTransformationProperty field."))),
        }
    }
}

/// Single-slot property target used for temporary properties (`followSurface`,
/// polygon `_positions` / `_holes`, inner reference arrays).
#[derive(Default)]
pub(crate) struct SlotTarget {
    value: RefCell<Option<PropertyInput>>,
}

impl SlotTarget {
    pub(crate) fn new(initial: Option<PropertyInput>) -> Self {
        Self { value: RefCell::new(initial) }
    }
    pub(crate) fn into_inner(self) -> Option<PropertyInput> {
        self.value.into_inner()
    }
}

impl CzmlPropertyTarget for SlotTarget {
    fn get_czml_property(&self, _name: &str) -> Option<PropertyInput> {
        self.value.borrow().clone()
    }
    fn set_czml_property(&self, _name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        *self.value.borrow_mut() = value;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DynamicValue conversions for Core types without dedicated DynamicValue arms
// ---------------------------------------------------------------------------

pub(crate) fn near_far_to_dynamic(s: NearFarScalar) -> DynamicValue {
    let mut map = Map::new();
    map.insert("near".into(), Value::from(s.near));
    map.insert("nearValue".into(), Value::from(s.near_value));
    map.insert("far".into(), Value::from(s.far));
    map.insert("farValue".into(), Value::from(s.far_value));
    DynamicValue::Json(JsonReference::object(map))
}

pub(crate) fn distance_display_to_dynamic(d: DistanceDisplayCondition) -> DynamicValue {
    let mut map = Map::new();
    map.insert("near".into(), Value::from(d.near));
    map.insert("far".into(), Value::from(d.far));
    DynamicValue::Json(JsonReference::object(map))
}

pub(crate) fn bounding_rect_to_dynamic(b: BoundingRectangle) -> DynamicValue {
    let mut map = Map::new();
    map.insert("x".into(), Value::from(b.x));
    map.insert("y".into(), Value::from(b.y));
    map.insert("width".into(), Value::from(b.width));
    map.insert("height".into(), Value::from(b.height));
    DynamicValue::Json(JsonReference::object(map))
}

pub(crate) fn rectangle_to_dynamic(r: Rectangle) -> DynamicValue {
    let mut map = Map::new();
    map.insert("west".into(), Value::from(r.west));
    map.insert("south".into(), Value::from(r.south));
    map.insert("east".into(), Value::from(r.east));
    map.insert("north".into(), Value::from(r.north));
    DynamicValue::Json(JsonReference::object(map))
}

pub(crate) fn rotation_to_dynamic(r: Rotation) -> DynamicValue {
    DynamicValue::Number(r.0)
}

pub(crate) fn f64_to_dynamic(v: f64) -> DynamicValue {
    DynamicValue::Number(v)
}

pub(crate) fn cartesian2_to_dynamic(v: Cartesian2) -> DynamicValue {
    DynamicValue::Cartesian2(v)
}

pub(crate) fn cartesian3_to_dynamic(v: Cartesian3) -> DynamicValue {
    DynamicValue::Cartesian3(v)
}

pub(crate) fn quaternion_to_dynamic(v: Quaternion) -> DynamicValue {
    DynamicValue::Quaternion(v)
}

pub(crate) fn color_to_dynamic(v: Color) -> DynamicValue {
    DynamicValue::Color(v)
}

// ---------------------------------------------------------------------------
// Property<DynamicValue> adapters for typed properties
// ---------------------------------------------------------------------------

/// Adapts a `SampledProperty<T>` into `Property<DynamicValue>` while keeping
/// typed access to the underlying `SampledProperty<T>` for incremental sample
/// additions, interpolation updates, and interval deletions.
pub(crate) struct CzmlSampledAdapter<T: SampledValue> {
    inner: Rc<SampledProperty<T>>,
    to_dynamic: fn(T) -> DynamicValue,
}

impl<T: SampledValue> CzmlSampledAdapter<T> {
    pub(crate) fn new(to_dynamic: fn(T) -> DynamicValue) -> Self {
        Self { inner: Rc::new(SampledProperty::new(None)), to_dynamic }
    }
    pub(crate) fn inner(&self) -> &Rc<SampledProperty<T>> {
        &self.inner
    }
}

impl<T: SampledValue> Property<DynamicValue> for CzmlSampledAdapter<T> {
    fn is_constant(&self) -> bool {
        self.inner.is_constant()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.inner.definition_changed()
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let value = self.inner.get_value(time, None)?.map(self.to_dynamic);
        if let (Some(v), Some(out)) = (&value, result) {
            *out = v.clone();
        }
        Ok(value)
    }
    fn equals(&self, other: &dyn Property<DynamicValue>) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        self.inner.equals(&*other.inner)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Adapts `VelocityVectorProperty` into `Property<DynamicValue>`.
pub(crate) struct VelocityVectorDynamicAdapter(pub(crate) Rc<VelocityVectorProperty>);

impl Property<DynamicValue> for VelocityVectorDynamicAdapter {
    fn is_constant(&self) -> bool {
        self.0.is_constant()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.0.definition_changed()
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let value = self.0.get_value(time, None)?.map(DynamicValue::Cartesian3);
        if let (Some(v), Some(out)) = (&value, result) {
            *out = v.clone();
        }
        Ok(value)
    }
    fn equals(&self, other: &dyn Property<DynamicValue>) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        self.0.equals(&*other.0)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Adapts `VelocityOrientationProperty` into `Property<DynamicValue>`.
pub(crate) struct VelocityOrientationDynamicAdapter(pub(crate) Rc<VelocityOrientationProperty>);

impl Property<DynamicValue> for VelocityOrientationDynamicAdapter {
    fn is_constant(&self) -> bool {
        self.0.is_constant()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.0.definition_changed()
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let value = self.0.get_value(time, None)?.map(DynamicValue::Quaternion);
        if let (Some(v), Some(out)) = (&value, result) {
            *out = v.clone();
        }
        Ok(value)
    }
    fn equals(&self, other: &dyn Property<DynamicValue>) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        self.0.equals(&*other.0)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Adapts `PositionPropertyArray` into `Property<DynamicValue>` returning
/// `DynamicValue::Positions`.
pub(crate) struct PositionArrayDynamicAdapter(pub(crate) Rc<PositionPropertyArray>);

impl Property<DynamicValue> for PositionArrayDynamicAdapter {
    fn is_constant(&self) -> bool {
        self.0.is_constant()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.0.definition_changed()
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let value = self.0.get_value(time, None)?.map(|v| DynamicValue::Positions(Rc::new(v)));
        if let (Some(v), Some(out)) = (&value, result) {
            *out = v.clone();
        }
        Ok(value)
    }
    fn equals(&self, other: &dyn Property<DynamicValue>) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        self.0.equals(&*other.0)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Adapts `PropertyArray<DynamicValue>` into `Property<DynamicValue>` returning
/// a JSON array of the evaluated elements.
pub(crate) struct PropertyArrayDynamicAdapter(pub(crate) Rc<PropertyArray<DynamicValue>>);

impl Property<DynamicValue> for PropertyArrayDynamicAdapter {
    fn is_constant(&self) -> bool {
        self.0.is_constant()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.0.definition_changed()
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let value = self
            .0
            .get_value(time, None)?
            .map(|items| DynamicValue::Json(JsonReference::array(items.iter().map(DynamicValue::to_json).collect())));
        if let (Some(v), Some(out)) = (&value, result) {
            *out = v.clone();
        }
        Ok(value)
    }
    fn equals(&self, other: &dyn Property<DynamicValue>) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        self.0.equals(&*other.0)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Adapts a `Vec<PropertyRef<DynamicValue>>` (each yielding `Positions`) for
/// polygon `holes` specified via `references` arrays of arrays.
pub(crate) struct HolesReferencesProperty {
    holes: Vec<PropertyRef<DynamicValue>>,
    changed: DefinitionChanged,
    _subscriptions: Vec<Subscription>,
}

impl HolesReferencesProperty {
    pub(crate) fn new(holes: Vec<PropertyRef<DynamicValue>>) -> Self {
        let changed = DefinitionChanged::new();
        let mut subscriptions = Vec::with_capacity(holes.len());
        for hole in &holes {
            let weak = changed.downgrade();
            subscriptions.push(hole.definition_changed().add_listener(move |_| {
                if let Some(ev) = weak.upgrade() {
                    ev.raise();
                }
            }));
        }
        Self { holes, changed, _subscriptions: subscriptions }
    }
}

impl Property<DynamicValue> for HolesReferencesProperty {
    fn is_constant(&self) -> bool {
        self.holes.iter().all(|h| h.is_constant())
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let mut out = Vec::with_capacity(self.holes.len());
        for hole in &self.holes {
            if let Some(val) = hole.get_value(time, None)? {
                out.push(val.to_json());
            }
        }
        let value = DynamicValue::Json(JsonReference::array(out));
        if let Some(r) = result {
            *r = value.clone();
        }
        Ok(Some(value))
    }
    fn equals(&self, other: &dyn Property<DynamicValue>) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        crate::data_sources::property::array_equals(Some(&self.holes), Some(&other.holes))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Adapts any `EntityComponent` (such as `ReferenceProperty`) to
/// `Property<DynamicValue>` when wrapping into a `CompositeProperty`.
pub(crate) struct ComponentAsDynamicProperty(pub(crate) ComponentRef);

impl Property<DynamicValue> for ComponentAsDynamicProperty {
    fn is_constant(&self) -> bool {
        self.0.is_constant()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.0.definition_changed()
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let time = time.copied().unwrap_or_else(|| (crate::data_sources::property::system_clock())());
        self.0.get_value(&time, result)
    }
    fn equals(&self, other: &dyn Property<DynamicValue>) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        self.0.equals(&*other.0)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Extracts a `PropertyRef<DynamicValue>` from a `PropertyInput::Component`.
pub(crate) fn component_to_dynamic_property(component: ComponentRef) -> PropertyRef<DynamicValue> {
    if let Some(pc) = component.as_any().downcast_ref::<PropertyComponent<DynamicValue>>() {
        pc.property().clone()
    } else {
        Rc::new(ComponentAsDynamicProperty(component))
    }
}

// ---------------------------------------------------------------------------
// CzmlPolygonHierarchyProperty (CesiumJS PolygonHierarchyProperty)
// ---------------------------------------------------------------------------

/// Combines CZML `polygon.positions` and `polygon.holes` properties into a
/// single `PolygonHierarchy` property on [`PolygonGraphics`], supporting
/// incremental updates across multiple `process()` calls.
pub(crate) struct CzmlPolygonHierarchyProperty {
    positions: RefCell<Option<PropertyInput>>,
    holes: RefCell<Option<PropertyInput>>,
    changed: DefinitionChanged,
    pos_sub: RefCell<Option<Subscription>>,
    holes_sub: RefCell<Option<Subscription>>,
}

impl CzmlPolygonHierarchyProperty {
    pub(crate) fn new() -> Self {
        Self {
            positions: RefCell::new(None),
            holes: RefCell::new(None),
            changed: DefinitionChanged::new(),
            pos_sub: RefCell::new(None),
            holes_sub: RefCell::new(None),
        }
    }

    pub(crate) fn positions_slot(&self) -> Option<PropertyInput> {
        self.positions.borrow().clone()
    }

    pub(crate) fn holes_slot(&self) -> Option<PropertyInput> {
        self.holes.borrow().clone()
    }

    pub(crate) fn set_positions_slot(&self, value: Option<PropertyInput>) {
        let old_sub = self.pos_sub.borrow_mut().take();
        drop(old_sub);
        *self.positions.borrow_mut() = value.clone();
        if let Some(PropertyInput::Component(c)) = value {
            let weak = self.changed.downgrade();
            *self.pos_sub.borrow_mut() = Some(c.definition_changed().add_listener(move |_| {
                if let Some(ev) = weak.upgrade() {
                    ev.raise();
                }
            }));
        }
        self.changed.raise();
    }

    pub(crate) fn set_holes_slot(&self, value: Option<PropertyInput>) {
        let old_sub = self.holes_sub.borrow_mut().take();
        drop(old_sub);
        *self.holes.borrow_mut() = value.clone();
        if let Some(PropertyInput::Component(c)) = value {
            let weak = self.changed.downgrade();
            *self.holes_sub.borrow_mut() = Some(c.definition_changed().add_listener(move |_| {
                if let Some(ev) = weak.upgrade() {
                    ev.raise();
                }
            }));
        }
        self.changed.raise();
    }

    pub(crate) fn has_any(&self) -> bool {
        self.positions.borrow().is_some() || self.holes.borrow().is_some()
    }
}

fn eval_positions_value(input: &Option<PropertyInput>, time: &JulianDate) -> Result<Vec<Cartesian3>, RuntimeError> {
    let val = match input {
        None => return Ok(Vec::new()),
        Some(PropertyInput::Value(v)) => Some(v.clone()),
        Some(PropertyInput::Component(c)) => c.get_value(time, None)?,
    };
    match val {
        None | Some(DynamicValue::Null) => Ok(Vec::new()),
        Some(DynamicValue::Positions(p)) => Ok((*p).clone()),
        Some(DynamicValue::Json(j)) => Ok(json_to_cartesian3_list(&j.value())),
        _ => Ok(Vec::new()),
    }
}

fn eval_holes_value(input: &Option<PropertyInput>, time: &JulianDate) -> Result<Vec<PolygonHierarchy>, RuntimeError> {
    let val = match input {
        None => return Ok(Vec::new()),
        Some(PropertyInput::Value(v)) => Some(v.clone()),
        Some(PropertyInput::Component(c)) => c.get_value(time, None)?,
    };
    match val {
        None | Some(DynamicValue::Null) => Ok(Vec::new()),
        Some(DynamicValue::Json(j)) => {
            let v = j.value();
            let Some(arr) = v.as_array() else { return Ok(Vec::new()) };
            Ok(arr
                .iter()
                .map(|hole_val| PolygonHierarchy::new(json_to_cartesian3_list(hole_val), Vec::new()))
                .collect())
        }
        _ => Ok(Vec::new()),
    }
}

fn json_to_cartesian3_list(v: &Value) -> Vec<Cartesian3> {
    let Some(arr) = v.as_array() else { return Vec::new() };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        if let Some(coords) = item.as_array() {
            if coords.len() >= 3 {
                if let (Some(x), Some(y), Some(z)) = (coords[0].as_f64(), coords[1].as_f64(), coords[2].as_f64()) {
                    out.push(Cartesian3::new(x, y, z));
                }
            }
        } else if let Some(obj) = item.as_object() {
            if let (Some(x), Some(y), Some(z)) = (
                obj.get("x").and_then(Value::as_f64),
                obj.get("y").and_then(Value::as_f64),
                obj.get("z").and_then(Value::as_f64),
            ) {
                out.push(Cartesian3::new(x, y, z));
            }
        }
    }
    out
}

impl EntityComponent for CzmlPolygonHierarchyProperty {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn is_constant(&self) -> bool {
        let pos_const = self.positions.borrow().as_ref().is_none_or(|p| match p {
            PropertyInput::Value(_) => true,
            PropertyInput::Component(c) => c.is_constant(),
        });
        let holes_const = self.holes.borrow().as_ref().is_none_or(|p| match p {
            PropertyInput::Value(_) => true,
            PropertyInput::Component(c) => c.is_constant(),
        });
        pos_const && holes_const
    }
    fn get_value(
        &self,
        time: &JulianDate,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let pos_slot = self.positions.borrow().clone();
        let holes_slot = self.holes.borrow().clone();
        let positions = eval_positions_value(&pos_slot, time)?;
        let holes = eval_holes_value(&holes_slot, time)?;
        let value = DynamicValue::PolygonHierarchy(Rc::new(PolygonHierarchy::new(positions, holes)));
        if let Some(out) = result {
            *out = value.clone();
        }
        Ok(Some(value))
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        if std::ptr::eq(self, other) {
            return Ok(true);
        }
        Ok(component_equals(self.positions.borrow().as_ref(), other.positions.borrow().as_ref())?
            && component_equals(self.holes.borrow().as_ref(), other.holes.borrow().as_ref())?)
    }
}
