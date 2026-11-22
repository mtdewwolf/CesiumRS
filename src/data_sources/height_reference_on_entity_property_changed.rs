//! CesiumJS `heightReferenceOnEntityPropertyChanged`: shared updater hook that
//! recreates a terrain-offset property whenever an observed geometry property
//! changes on an [`Entity`].
use super::{CallbackProperty, Entity, PropertyInput, PropertyRef};
use crate::core::Cartesian3;
use std::rc::Rc;

/// Host state representing a `GeometryUpdater` with optional height reference.
pub trait HeightReferenceUpdaterHost {
    type TerrainOffset;

    fn on_entity_property_changed(&mut self, entity: &Entity, property_name: &str);
    fn observed_property_names(&self) -> &[&str];
    fn entity(&self) -> &Entity;
    fn geometry_property_name(&self) -> &str;
    fn is_dynamic(&self) -> bool;
    fn take_terrain_offset(&mut self) -> Option<Self::TerrainOffset>;
    fn destroy_terrain_offset(&mut self, offset: Self::TerrainOffset);
    fn compute_center(&self) -> Rc<dyn Fn(Option<&crate::core::JulianDate>) -> Option<Cartesian3>>;
    fn create_terrain_offset(
        &mut self,
        center_position: PropertyRef<Cartesian3>,
        height_reference: PropertyInput,
    ) -> Self::TerrainOffset;
    fn set_terrain_offset(&mut self, offset: Option<Self::TerrainOffset>);
}

/// Port of CesiumJS `heightReferenceOnEntityPropertyChanged`.
pub fn height_reference_on_entity_property_changed<H: HeightReferenceUpdaterHost>(
    host: &mut H,
    entity: &Entity,
    property_name: &str,
) {
    host.on_entity_property_changed(entity, property_name);
    if !host.observed_property_names().contains(&property_name) {
        return;
    }

    let geometry_name = host.geometry_property_name().to_string();
    let geometry = match host.entity().get_property(&geometry_name) {
        Some(PropertyInput::Component(component)) => component,
        _ => return,
    };

    if let Some(existing) = host.take_terrain_offset() {
        host.destroy_terrain_offset(existing);
        host.set_terrain_offset(None);
    }

    if let Some(height_reference) = geometry.get_property("heightReference") {
        let compute = host.compute_center();
        let is_constant = !host.is_dynamic();
        let center_position: PropertyRef<Cartesian3> = Rc::new(CallbackProperty::new(
            Rc::new(move |time, _result| Ok(compute(Some(time)))),
            is_constant,
        ));
        let offset = host.create_terrain_offset(center_position, height_reference);
        host.set_terrain_offset(Some(offset));
    }
}
