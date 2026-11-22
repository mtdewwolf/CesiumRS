//! CesiumJS `GroundGeometryUpdater`: shared base logic for ground-clamped
//! geometry updaters (`Corridor`, `Ellipse`, `Polygon`, `Rectangle`).
use super::{
    geometry_updater::{GeometryScene, GeometryUpdaterBase, HeightReferenceSlotProperty},
    CallbackProperty, Entity, EntityComponent, EntityRef, PropertyInput, PropertyRef, TerrainOffsetProperty,
};
use crate::{
    core::{Cartesian3, GeometryOffsetAttribute, JulianDate},
    scene::{height_reference::is_height_reference_clamp, HeightReference},
};
use std::rc::Rc;

/// Result of [`GroundGeometryUpdater::get_geometry_extruded_height`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroundExtrudedHeight {
    Height(f64),
    ClampToGround,
}

/// Shared base state for ground-capable geometry updaters (port of CesiumJS
/// `GroundGeometryUpdater`).
pub struct GroundGeometryUpdater {
    pub base: GeometryUpdaterBase,
    pub z_index: Option<PropertyInput>,
    pub terrain_offset_property: Option<Rc<TerrainOffsetProperty>>,
}

impl GroundGeometryUpdater {
    /// CesiumJS `GroundGeometryUpdater.CLAMP_TO_GROUND` sentinel string.
    pub const CLAMP_TO_GROUND: &'static str = "clamp";

    pub fn new(
        entity: EntityRef,
        scene: Rc<dyn GeometryScene>,
        geometry_property_name: &'static str,
        observed_property_names: &'static [&'static str],
    ) -> Self {
        Self {
            base: GeometryUpdaterBase::new(entity, scene, geometry_property_name, observed_property_names),
            z_index: Some(PropertyInput::value(0.0)),
            terrain_offset_property: None,
        }
    }

    pub fn z_index(&self) -> Option<PropertyInput> {
        self.z_index.clone()
    }

    pub fn terrain_offset_property(&self) -> Option<Rc<TerrainOffsetProperty>> {
        self.terrain_offset_property.clone()
    }

    /// CesiumJS `GroundGeometryUpdater.prototype._isOnTerrain`.
    pub fn is_on_terrain(base: &GeometryUpdaterBase, _entity: &Entity, geometry: &dyn EntityComponent) -> bool {
        base.fill_enabled
            && geometry.get_property("height").is_none()
            && geometry.get_property("extrudedHeight").is_none()
            && base.scene.ground_primitive_supported()
    }

    /// CesiumJS `GroundGeometryUpdater.prototype._getIsClosed`.
    pub fn get_is_closed(height: Option<f64>, extruded_height: Option<f64>) -> bool {
        height == Some(0.0) || (extruded_height.is_some() && extruded_height != height)
    }

    /// CesiumJS `GroundGeometryUpdater.getGeometryHeight`.
    pub fn get_geometry_height(height: Option<f64>, height_reference: HeightReference) -> Option<f64> {
        let height = height?;
        if !is_height_reference_clamp(height_reference) {
            Some(height)
        } else {
            Some(0.0)
        }
    }

    /// CesiumJS `GroundGeometryUpdater.getGeometryExtrudedHeight`.
    pub fn get_geometry_extruded_height(
        extruded_height: Option<f64>,
        extruded_height_reference: HeightReference,
    ) -> Option<GroundExtrudedHeight> {
        let extruded_height = extruded_height?;
        if !is_height_reference_clamp(extruded_height_reference) {
            Some(GroundExtrudedHeight::Height(extruded_height))
        } else {
            Some(GroundExtrudedHeight::ClampToGround)
        }
    }

    /// CesiumJS `GroundGeometryUpdater.computeGeometryOffsetAttribute`.
    pub fn compute_geometry_offset_attribute(
        height: Option<f64>,
        height_reference: Option<HeightReference>,
        extruded_height: Option<f64>,
        extruded_height_reference: Option<HeightReference>,
    ) -> Option<GeometryOffsetAttribute> {
        let hr = match (height, height_reference) {
            (Some(_), Some(hr)) => hr,
            _ => HeightReference::None,
        };
        let ehr = match (extruded_height, extruded_height_reference) {
            (Some(_), Some(ehr)) => ehr,
            _ => HeightReference::None,
        };

        let mut n = 0u8;
        if hr != HeightReference::None {
            n += 1;
        }
        if ehr == HeightReference::RelativeToGround {
            n += 1;
        }
        match n {
            2 => Some(GeometryOffsetAttribute::All),
            1 => Some(GeometryOffsetAttribute::Top),
            _ => None,
        }
    }

    /// Updates `zIndex` and recreates `TerrainOffsetProperty` after
    /// `GeometryUpdaterBase::on_entity_property_changed_with`.
    pub fn finish_property_changed<FCenter>(&mut self, property_name: &str, compute_center: FCenter)
    where
        FCenter: Fn(&JulianDate) -> Option<Cartesian3> + 'static,
    {
        if !self.base.observed_property_names.contains(&property_name) {
            return;
        }
        let geometry = match self.base.entity.get_property(self.base.geometry_property_name) {
            Some(PropertyInput::Component(c)) => c,
            _ => return,
        };

        self.z_index = geometry.get_property("zIndex").or_else(|| Some(PropertyInput::value(0.0)));

        if let Some(existing) = self.terrain_offset_property.take() {
            existing.destroy();
        }

        let hr_slot = geometry.get_property("heightReference");
        let ehr_slot = geometry.get_property("extrudedHeightReference");
        if hr_slot.is_some() || ehr_slot.is_some() {
            let is_constant = !self.base.dynamic;
            let center_position: PropertyRef<Cartesian3> =
                Rc::new(CallbackProperty::new(Rc::new(move |time, _result| Ok(compute_center(time))), is_constant));
            let hr = hr_slot.map(HeightReferenceSlotProperty::wrap);
            let ehr = ehr_slot.map(HeightReferenceSlotProperty::wrap);
            let offset_scene = Rc::clone(&self.base.scene).as_terrain_offset_scene();
            self.terrain_offset_property = Some(TerrainOffsetProperty::new(offset_scene, center_position, hr, ehr));
        }
    }

    pub fn destroy(&mut self) {
        if let Some(existing) = self.terrain_offset_property.take() {
            existing.destroy();
        }
        self.base.destroy();
    }
}
