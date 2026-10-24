//! Entity state, typed components, hierarchy, availability and model transforms.
use super::{
    create_property_descriptor,
    create_property_descriptor::{constant_component, ComponentFactory},
    create_raw_property_descriptor, ChangeEvent, ComponentRef, ConstantPositionProperty, DynamicValue,
    PositionComponent, PositionContext, PositionPropertyRef, PropertyBag, PropertyChange, PropertyComponent,
    PropertyDescriptor, PropertyInput, PropertyIntervals, PropertyRef, Subscription,
};
use crate::core::*;
use crate::scene::{height_reference::is_height_reference_clamp, HeightReference};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};
pub type EntityRef = Rc<Entity>;
pub type Availability = Rc<PropertyIntervals<()>>;
pub const ENTITY_PROPERTY_NAMES: &[&str] = &[
    "billboard",
    "box",
    "corridor",
    "cylinder",
    "description",
    "ellipse",
    "ellipsoid",
    "label",
    "model",
    "tileset",
    "orientation",
    "path",
    "plane",
    "point",
    "polygon",
    "polyline",
    "polylineVolume",
    "position",
    "properties",
    "rectangle",
    "viewFrom",
    "wall",
];
const GRAPHICS: &[&str] = &[
    "billboard",
    "box",
    "corridor",
    "cylinder",
    "ellipse",
    "ellipsoid",
    "label",
    "model",
    "tileset",
    "path",
    "plane",
    "point",
    "polygon",
    "polyline",
    "polylineVolume",
    "rectangle",
    "wall",
];
#[derive(Clone, Default)]
pub struct EntityTypeRegistry(Rc<RefCell<Vec<(String, ComponentFactory)>>>);
impl EntityTypeRegistry {
    pub fn register(&self, name: impl Into<String>, factory: ComponentFactory) -> Result<(), RuntimeError> {
        let name = name.into();
        if Entity::is_reserved(&name) || self.factory(&name).is_some() {
            return Err(RuntimeError::new(format!("{name} is already registered or reserved.")));
        }
        self.0.borrow_mut().push((name, factory));
        Ok(())
    }
    pub fn names(&self) -> Vec<String> {
        self.0.borrow().iter().map(|(n, _)| n.clone()).collect()
    }
    pub fn factory(&self, name: &str) -> Option<ComponentFactory> {
        self.0.borrow().iter().find(|(n, _)| n == name).map(|(_, f)| f.clone())
    }
}
#[derive(Clone)]
pub struct EntityChange {
    pub entity: Weak<Entity>,
    pub property: PropertyChange,
}
#[derive(Clone)]
pub struct EntityOptions {
    pub id: Option<String>,
    pub name: Option<String>,
    pub show: bool,
    pub tracking_reference_frame: TrackingReferenceFrame,
    pub parent: Option<EntityRef>,
    pub availability: Option<Availability>,
    pub position: Option<PositionPropertyRef>,
    pub orientation: Option<PropertyRef<Quaternion>>,
    pub properties: Option<Rc<PropertyBag>>,
    pub values: Vec<(String, PropertyInput)>,
    pub registry: EntityTypeRegistry,
    pub context: PositionContext,
}
impl Default for EntityOptions {
    fn default() -> Self {
        Self {
            id: None,
            name: None,
            show: true,
            tracking_reference_frame: TrackingReferenceFrame::Autodetect,
            parent: None,
            availability: None,
            position: None,
            orientation: None,
            properties: None,
            values: vec![],
            registry: EntityTypeRegistry::default(),
            context: PositionContext::default(),
        }
    }
}
pub struct HeightReferenceOptions<'a> {
    pub property: Option<&'a dyn super::Property<HeightReference>>,
    pub height_offset: f64,
    pub ellipsoid: &'a Ellipsoid,
}
pub struct Entity {
    id: String,
    name: RefCell<Option<String>>,
    show: Cell<bool>,
    tracking: Cell<TrackingReferenceFrame>,
    parent: RefCell<Option<EntityRef>>,
    children: RefCell<Vec<Weak<Entity>>>,
    availability: RefCell<Option<Availability>>,
    collection: RefCell<Option<Weak<super::EntityCollection>>>,
    names: RefCell<Vec<String>>,
    fields: RefCell<Vec<PropertyDescriptor>>,
    slot_changes: ChangeEvent<PropertyChange>,
    changed: ChangeEvent<EntityChange>,
    _slot_forward: Subscription,
    weak: Weak<Entity>,
    registry: EntityTypeRegistry,
    context: PositionContext,
}
impl Entity {
    pub fn new(options: EntityOptions) -> Result<EntityRef, RuntimeError> {
        let slots = ChangeEvent::<PropertyChange>::new();
        let changed = ChangeEvent::<EntityChange>::new();
        let registry = options.registry.clone();
        let context = options.context.clone();
        let mut names = ENTITY_PROPERTY_NAMES.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        names.extend(registry.names());
        let entity = Rc::new_cyclic(|weak: &Weak<Entity>| {
            let target = weak.clone();
            let forward = slots.add_listener(move |change| {
                if let Some(entity) = target.upgrade() {
                    entity.changed.raise(EntityChange { entity: entity.weak.clone(), property: change.clone() });
                }
            });
            Self {
                id: options.id.clone().unwrap_or_else(create_guid::create_guid),
                name: RefCell::new(options.name.clone()),
                show: Cell::new(options.show),
                tracking: Cell::new(options.tracking_reference_frame),
                parent: RefCell::new(None),
                children: RefCell::new(vec![]),
                availability: RefCell::new(None),
                collection: RefCell::new(None),
                names: RefCell::new(names),
                fields: RefCell::new(vec![]),
                slot_changes: slots.clone(),
                changed: changed.clone(),
                _slot_forward: forward,
                weak: weak.clone(),
                registry: registry.clone(),
                context: context.clone(),
            }
        });
        for name in entity.property_names() {
            entity.ensure_field(&name, false)?;
        }
        entity.set_parent(options.parent)?;
        entity.set_availability(options.availability);
        if let Some(value) = options.position {
            entity.set_position(Some(value))?;
        }
        if let Some(value) = options.orientation {
            entity.set_orientation(Some(value))?;
        }
        if let Some(value) = options.properties {
            entity.set_property("properties", Some(PropertyInput::Component(value)))?;
        }
        entity.merge_values(&options.values)?;
        Ok(entity)
    }
    pub fn with_id(id: impl Into<String>) -> EntityRef {
        Self::new(EntityOptions { id: Some(id.into()), ..Default::default() }).expect("plain entity options")
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn definition_changed(&self) -> &ChangeEvent<EntityChange> {
        &self.changed
    }
    pub fn property_names(&self) -> Vec<String> {
        self.names.borrow().clone()
    }
    pub fn name(&self) -> Option<String> {
        self.name.borrow().clone()
    }
    pub fn set_name(&self, value: Option<String>) {
        let old = self.name.replace(value.clone());
        if old != value {
            self.notify("name", value.map(PropertyInput::value), old.map(PropertyInput::value));
        }
    }
    pub fn show(&self) -> bool {
        self.show.get()
    }
    pub fn is_showing(&self) -> bool {
        self.show.get()
            && self.entity_collection().is_none_or(|c| c.show())
            && self.parent().is_none_or(|p| p.is_showing())
    }
    pub fn set_show(&self, value: bool) {
        if value == self.show.get() {
            return;
        }
        let before = self.is_showing();
        self.show.set(value);
        let after = self.is_showing();
        if before != after {
            self.update_show(after);
        }
        self.notify("show", Some(PropertyInput::value(value)), Some(PropertyInput::value(!value)));
    }
    pub fn tracking_reference_frame(&self) -> TrackingReferenceFrame {
        self.tracking.get()
    }
    pub fn set_tracking_reference_frame(&self, value: TrackingReferenceFrame) {
        let old = self.tracking.replace(value);
        if old != value {
            self.notify(
                "trackingReferenceFrame",
                Some(PropertyInput::value(value as u8 as f64)),
                Some(PropertyInput::value(old as u8 as f64)),
            );
        }
    }
    pub fn parent(&self) -> Option<EntityRef> {
        self.parent.borrow().clone()
    }
    pub fn children(&self) -> Vec<EntityRef> {
        let mut children = self.children.borrow_mut();
        children.retain(|c| c.strong_count() > 0);
        children.iter().filter_map(Weak::upgrade).collect()
    }
    pub fn set_parent(&self, value: Option<EntityRef>) -> Result<(), RuntimeError> {
        let old = self.parent();
        if super::position_property::same_ref(&old, &value) {
            return Ok(());
        }
        let mut next = value.clone();
        while let Some(parent) = next {
            if Weak::ptr_eq(&parent.weak, &self.weak) {
                return Err(RuntimeError::new("parent would create a cycle"));
            }
            next = parent.parent();
        }
        let before = self.is_showing();
        if let Some(old) = &old {
            old.children.borrow_mut().retain(|c| !Weak::ptr_eq(c, &self.weak));
        }
        *self.parent.borrow_mut() = value.clone();
        if let Some(parent) = &value {
            parent.children.borrow_mut().push(self.weak.clone());
        }
        let after = self.is_showing();
        if before != after {
            self.update_show(after);
        }
        self.notify(
            "parent",
            value.as_ref().map(|p| PropertyInput::Value(DynamicValue::Entity(Rc::downgrade(p)))),
            old.as_ref().map(|p| PropertyInput::Value(DynamicValue::Entity(Rc::downgrade(p)))),
        );
        Ok(())
    }
    fn update_show(&self, showing: bool) {
        for child in self.children() {
            if child.show() {
                child.update_show(showing);
            }
        }
        self.notify_is_showing(showing, !showing);
    }
    pub(crate) fn notify_is_showing(&self, value: bool, old: bool) {
        self.notify("isShowing", Some(PropertyInput::value(value)), Some(PropertyInput::value(old)));
    }
    pub fn availability(&self) -> Option<Availability> {
        self.availability.borrow().clone()
    }
    pub fn set_availability(&self, value: Option<Availability>) {
        let old = self.availability.replace(value.clone());
        if !super::position_property::same_ref(&old, &value) {
            self.notify(
                "availability",
                value.as_ref().map(|v| PropertyInput::Value(DynamicValue::Availability(Rc::downgrade(v)))),
                old.as_ref().map(|v| PropertyInput::Value(DynamicValue::Availability(Rc::downgrade(v)))),
            );
        }
    }
    pub fn is_available(&self, time: &JulianDate) -> bool {
        self.availability().is_none_or(|a| a.read().contains(time))
    }
    pub fn entity_collection(&self) -> Option<Rc<super::EntityCollection>> {
        self.collection.borrow().as_ref().and_then(Weak::upgrade)
    }
    pub(crate) fn set_entity_collection(&self, value: Weak<super::EntityCollection>) {
        *self.collection.borrow_mut() = Some(value);
    }
    fn notify(&self, name: &str, new_value: Option<PropertyInput>, old_value: Option<PropertyInput>) {
        self.changed.raise(EntityChange {
            entity: self.weak.clone(),
            property: PropertyChange { name: name.into(), new_value, old_value, child_changed: false },
        });
    }
    pub fn get_property(&self, name: &str) -> Option<PropertyInput> {
        match name {
            "name" => self.name().map(PropertyInput::value),
            "show" => Some(PropertyInput::value(self.show())),
            "isShowing" => Some(PropertyInput::value(self.is_showing())),
            "id" => Some(PropertyInput::value(self.id.clone())),
            "parent" => self.parent().map(|v| PropertyInput::Value(DynamicValue::Entity(Rc::downgrade(&v)))),
            "availability" => {
                self.availability().map(|v| PropertyInput::Value(DynamicValue::Availability(Rc::downgrade(&v))))
            }
            "trackingReferenceFrame" => Some(PropertyInput::value(self.tracking.get() as u8 as f64)),
            _ => self.fields.borrow().iter().find(|f| f.name() == name).and_then(PropertyDescriptor::get),
        }
    }
    fn factory(&self, name: &str) -> Option<ComponentFactory> {
        if let Some(factory) = self.registry.factory(name) {
            return Some(factory);
        }
        if name == "position" {
            let context = self.context.clone();
            return Some(Rc::new(move |value| {
                let value = match value {
                    DynamicValue::Null => None,
                    DynamicValue::Cartesian3(v) => Some(v),
                    _ => return Err(RuntimeError::new("position requires Cartesian3 or a position component")),
                };
                Ok(PositionComponent::wrap(Rc::new(ConstantPositionProperty::with_context(
                    value,
                    ReferenceFrame::Fixed,
                    context.clone(),
                ))))
            }));
        }
        if name == "properties" {
            return Some(Rc::new(|value| {
                let values = match value {
                    DynamicValue::Record(v) => v,
                    DynamicValue::Json(v) => {
                        let value = v.value();
                        let object =
                            value.as_object().ok_or_else(|| RuntimeError::new("properties requires an object"))?;
                        object.iter().map(|(k, v)| (k.clone(), Some(DynamicValue::from_json(v.clone())))).collect()
                    }
                    DynamicValue::Null => vec![],
                    _ => return Err(RuntimeError::new("properties requires an object")),
                };
                let bag = Rc::new(PropertyBag::new());
                for (k, v) in values {
                    bag.add_property(k.clone(), None, None)?;
                    if let Some(value) = v {
                        bag.set(&k, Some(PropertyInput::Value(value)))?;
                    }
                }
                Ok(bag as ComponentRef)
            }));
        }
        if GRAPHICS.contains(&name) {
            let name = name.to_owned();
            return Some(Rc::new(move |_| {
                Err(RuntimeError::new(format!("{name} requires a typed graphics component")))
            }));
        }
        ENTITY_PROPERTY_NAMES.contains(&name).then(|| Rc::new(constant_component) as ComponentFactory)
    }
    fn ensure_field(&self, name: &str, custom: bool) -> Result<PropertyDescriptor, RuntimeError> {
        if let Some(field) = self.fields.borrow().iter().find(|f| f.name() == name).cloned() {
            return Ok(field);
        }
        let field = if custom {
            create_raw_property_descriptor(name, true, &self.slot_changes)
        } else {
            create_property_descriptor(name, false, self.factory(name), &self.slot_changes)
        };
        self.fields.borrow_mut().push(field.clone());
        Ok(field)
    }
    pub fn set_property(&self, name: &str, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        if !self.names.borrow().iter().any(|n| n == name)
            && !ENTITY_PROPERTY_NAMES.contains(&name)
            && self.registry.factory(name).is_none()
        {
            return Err(RuntimeError::new(format!("{name} is not a registered property.")));
        }
        self.ensure_field(name, false)?.set(value)
    }
    pub fn position(&self) -> Option<PositionPropertyRef> {
        self.get_property("position").and_then(|v| v.component()).and_then(|v| v.position_property())
    }
    pub fn set_position(&self, value: Option<PositionPropertyRef>) -> Result<(), RuntimeError> {
        self.set_property("position", value.map(|p| PropertyInput::Component(PositionComponent::wrap(p))))
    }
    pub fn set_orientation(&self, value: Option<PropertyRef<Quaternion>>) -> Result<(), RuntimeError> {
        self.set_property("orientation", value.map(|p| PropertyInput::Component(PropertyComponent::wrap(p))))
    }
    pub fn add_property(&self, name: impl Into<String>) -> Result<(), RuntimeError> {
        let name = name.into();
        if self.names.borrow().contains(&name) {
            return Err(RuntimeError::new(format!("{name} is already a registered property.")));
        }
        if Self::is_reserved(&name) || self.registry.factory(&name).is_some() {
            return Err(RuntimeError::new(format!("{name} is a reserved property name.")));
        }
        self.names.borrow_mut().push(name.clone());
        self.ensure_field(&name, true)?;
        Ok(())
    }
    pub fn remove_property(&self, name: &str) -> Result<(), RuntimeError> {
        let index = self
            .names
            .borrow()
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| RuntimeError::new(format!("{name} is not a registered property.")))?;
        self.names.borrow_mut().remove(index);
        if !ENTITY_PROPERTY_NAMES.contains(&name) && self.registry.factory(name).is_none() {
            let field = {
                let mut fields = self.fields.borrow_mut();
                let index = fields.iter().position(|f| f.name() == name).unwrap();
                fields.remove(index)
            };
            field.deactivate();
        }
        Ok(())
    }
    pub fn is_reserved(name: &str) -> bool {
        ENTITY_PROPERTY_NAMES.contains(&name)
            || [
                "id",
                "name",
                "show",
                "isShowing",
                "parent",
                "availability",
                "children",
                "trackingReferenceFrame",
                "definitionChanged",
                "propertyNames",
                "entityCollection",
                "addProperty",
                "removeProperty",
                "merge",
                "isAvailable",
                "computeModelMatrix",
                "computeModelMatrixForHeightReference",
                "constructor",
                "toString",
                "valueOf",
                "hasOwnProperty",
                "__proto__",
            ]
            .contains(&name)
    }
    pub fn merge(&self, source: &Entity) -> Result<(), RuntimeError> {
        if self.name().is_none() {
            self.set_name(source.name());
        }
        if self.availability().is_none() {
            self.set_availability(source.availability());
        }
        let values = source
            .property_names()
            .into_iter()
            .map(|n| {
                let value = source.get_property(&n);
                (n, value)
            })
            .collect::<Vec<_>>();
        self.merge_entries(&values)
    }
    pub fn merge_values(&self, source: &[(String, PropertyInput)]) -> Result<(), RuntimeError> {
        self.merge_entries(&source.iter().map(|(name, value)| (name.clone(), Some(value.clone()))).collect::<Vec<_>>())
    }
    pub fn merge_entries(&self, source: &[(String, Option<PropertyInput>)]) -> Result<(), RuntimeError> {
        for (name, value) in source {
            if ["id", "parent", "children", "show", "trackingReferenceFrame"].contains(&name.as_str()) {
                continue;
            }
            if name == "name" {
                if self.name().is_none() {
                    if let Some(PropertyInput::Value(DynamicValue::String(v))) = value {
                        self.set_name(Some(v.clone()));
                    }
                }
                continue;
            }
            if name == "availability" {
                continue;
            }
            let target = self.get_property(name);
            if target.as_ref().is_none_or(|v| !v.is_defined())
                && !self.names.borrow().contains(name)
                && !ENTITY_PROPERTY_NAMES.contains(&name.as_str())
                && self.registry.factory(name).is_none()
            {
                self.add_property(name.clone())?;
            }
            let Some(value) = value.as_ref() else { continue };
            if !value.is_defined() {
                continue;
            }
            if let Some(target) = target.filter(PropertyInput::is_defined) {
                if let PropertyInput::Component(target) = target {
                    if target.supports_merge() {
                        let PropertyInput::Component(source) = value else {
                            return Err(RuntimeError::new("merge source must be a component"));
                        };
                        target.merge_component(&**source)?;
                    }
                }
            } else if let PropertyInput::Component(component) = value {
                if component.supports_merge() && component.supports_clone() {
                    self.set_property(name, Some(PropertyInput::Component(component.clone_component(None)?)))?;
                } else {
                    self.set_property(name, Some(value.clone()))?;
                }
            } else {
                self.set_property(name, Some(value.clone()))?;
            }
        }
        Ok(())
    }
    fn component_value(&self, name: &str, time: &JulianDate) -> Result<Option<DynamicValue>, RuntimeError> {
        match self.get_property(name) {
            None => Ok(None),
            Some(PropertyInput::Component(p)) => {
                let mut scratch = if name == "orientation" {
                    DynamicValue::Quaternion(Quaternion::new(0., 0., 0., 0.))
                } else {
                    DynamicValue::Cartesian3(Cartesian3::ZERO)
                };
                p.get_value(time, Some(&mut scratch))
            }
            Some(PropertyInput::Value(_)) => Err(RuntimeError::new("property.getValue is not a function")),
        }
    }
    pub fn position_at(&self, time: &JulianDate) -> Result<Option<Cartesian3>, RuntimeError> {
        match self.component_value("position", time)? {
            None | Some(DynamicValue::Null) => Ok(None),
            Some(DynamicValue::Cartesian3(v)) => Ok(Some(v)),
            _ => Err(RuntimeError::new("position requires Cartesian3")),
        }
    }
    fn matrix_at(&self, time: &JulianDate, position: Cartesian3) -> Result<Matrix4, RuntimeError> {
        match self.component_value("orientation", time)? {
            None | Some(DynamicValue::Null) => {
                Ok(Transforms::east_north_up_to_fixed_frame(position, &(self.context.ellipsoid)()))
            }
            Some(DynamicValue::Quaternion(q)) => {
                Ok(Matrix4::from_rotation_translation(&Matrix3::from_quaternion(&q), position))
            }
            _ => Err(RuntimeError::new("orientation requires Quaternion")),
        }
    }
    pub fn compute_model_matrix(
        &self,
        time: &JulianDate,
        result: Option<&mut Matrix4>,
    ) -> Result<Option<Matrix4>, RuntimeError> {
        let value = self.position_at(time)?.map(|p| self.matrix_at(time, p)).transpose()?;
        Ok(super::property::write_result(value, result))
    }
    pub fn compute_model_matrix_for_height_reference(
        &self,
        time: &JulianDate,
        options: HeightReferenceOptions<'_>,
        result: Option<&mut Matrix4>,
    ) -> Result<Option<Matrix4>, RuntimeError> {
        let reference = options
            .property
            .map(|p| p.get_value(Some(time), None))
            .transpose()?
            .flatten()
            .unwrap_or(HeightReference::None);
        let position = self.position_at(time)?;
        if reference == HeightReference::None
            || position.is_none_or(|p| p.equals_epsilon(Cartesian3::ZERO, math::EPSILON8))
        {
            return self.compute_model_matrix(time, result);
        }
        let mut carto = options
            .ellipsoid
            .cartesian_to_cartographic(position.unwrap())
            .ok_or_else(|| RuntimeError::new("position cannot be projected"))?;
        if is_height_reference_clamp(reference) {
            carto.height = options.height_offset;
        } else {
            carto.height += options.height_offset;
        }
        let matrix = self.matrix_at(time, options.ellipsoid.cartographic_to_cartesian(&carto))?;
        Ok(super::property::write_result(Some(matrix), result))
    }
    pub fn supports_materials_for_entities_on_terrain(depth_texture: bool) -> bool {
        depth_texture
    }
    pub fn supports_polylines_on_terrain(depth_texture: bool) -> bool {
        depth_texture
    }
}
