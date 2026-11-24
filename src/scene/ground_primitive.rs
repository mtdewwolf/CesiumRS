//! CesiumJS `GroundPrimitive`: terrain- and 3D-Tiles-clamped classification
//! volumes built from surface geometry descriptions.
//!
//! The wrapper turns each supported geometry into an extruded shadow volume
//! between approximate terrain heights, augments instances with planar or
//! spherical texture-coordinate extents when materials are supported, and
//! delegates command generation to [`ClassificationPrimitive`].

use super::{
    classification_primitive::{ClassificationKind, ClassificationPrimitive, ClassificationPrimitiveOptions},
    FrameState, ShadowVolumeAppearance,
};
use crate::{
    core::{
        vertical_exaggeration, ApproximateTerrainHeights, BoundingSphere, GeometryInstance, Matrix4, Rectangle,
        RuntimeError,
    },
    renderer::Gl,
    scene::ScenePrimitive,
    workers::GeometryDescription,
};
use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
};

pub type GroundPrimitiveOptions = ClassificationPrimitiveOptions;

/// A ground classification primitive.
pub struct GroundPrimitive<G: Gl + 'static> {
    inner: Option<ClassificationPrimitive<G>>,
    pending: Option<ClassificationPrimitiveOptions>,
    appearance_present: bool,
    classification_type: super::ClassificationType,
    show: bool,
    debug_show_bounding_volume: bool,
    debug_show_shadow_volume: bool,
    ready: bool,
    destroyed: bool,
}

impl<G: Gl + 'static> GroundPrimitive<G> {
    pub fn new(options: GroundPrimitiveOptions) -> Self {
        let classification_type = options.classification_type;
        let show = options.primitive.show.unwrap_or(true);
        let debug_show_bounding_volume = options.primitive.debug_show_bounding_volume.unwrap_or(false);
        let appearance_present = options.primitive.appearance.is_some();
        Self {
            inner: None,
            pending: Some(options),
            appearance_present,
            classification_type,
            show,
            debug_show_bounding_volume,
            debug_show_shadow_volume: false,
            ready: false,
            destroyed: false,
        }
    }

    pub fn is_supported(frame: &FrameState) -> bool {
        ClassificationPrimitive::<G>::is_supported(frame)
    }

    pub fn supports_materials(frame: &FrameState) -> bool {
        frame.context_capabilities.depth_texture
    }

    pub fn initialize_terrain_heights() -> &'static ApproximateTerrainHeights {
        ApproximateTerrainHeights::bundled()
    }

    pub fn classification_type(&self) -> super::ClassificationType {
        self.classification_type
    }

    pub fn set_classification_type(&mut self, classification_type: super::ClassificationType) {
        self.classification_type = classification_type;
        if let Some(inner) = &mut self.inner {
            inner.classification_type = classification_type;
        }
    }

    pub fn show(&self) -> bool {
        self.show
    }

    pub fn set_show(&mut self, show: bool) {
        self.show = show;
        if let Some(inner) = &mut self.inner {
            inner.set_show(show);
        }
    }

    pub fn set_debug_show_bounding_volume(&mut self, show: bool) {
        self.debug_show_bounding_volume = show;
        if let Some(inner) = &mut self.inner {
            inner.set_debug_show_bounding_volume(show);
        }
    }

    pub fn debug_show_shadow_volume(&self) -> bool {
        self.debug_show_shadow_volume
    }

    pub fn set_debug_show_shadow_volume(&mut self, show: bool) {
        self.debug_show_shadow_volume = show;
        if let Some(inner) = &mut self.inner {
            inner.debug_show_shadow_volume = show;
        }
    }

    pub fn set_appearance(&mut self, appearance: Box<dyn super::Appearance>) {
        self.appearance_present = true;
        if let Some(options) = &mut self.pending {
            options.primitive.appearance = Some(appearance);
        } else if let Some(inner) = &mut self.inner {
            inner.set_appearance(appearance);
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn get_bounding_sphere(&self, id: &str) -> Option<BoundingSphere> {
        self.inner.as_ref()?.ground_bounding_sphere(id)
    }

    /// The exaggerated minimum and maximum terrain heights, once initialized.
    pub fn ground_heights(&self) -> Option<[f64; 2]> {
        self.inner.as_ref().and_then(|inner| inner.ground_heights_value())
    }

    pub fn get_geometry_instance_attributes(
        &mut self,
        id: &str,
    ) -> Result<Option<super::GeometryInstanceAttributes<'_, G>>, RuntimeError> {
        self.inner
            .as_mut()
            .ok_or_else(|| RuntimeError::new("must call update before calling getGeometryInstanceAttributes"))?
            .get_geometry_instance_attributes(id)
    }

    fn initialize(&mut self, frame: &FrameState) -> Result<(), RuntimeError> {
        if !ApproximateTerrainHeights::bundled().initialized() {
            if self.pending.as_ref().is_some_and(|options| options.primitive.asynchronous == Some(false)) {
                return Err(RuntimeError::new(
                    "For synchronous GroundPrimitives, you must call GroundPrimitive.initializeTerrainHeights() and wait for the returned promise to resolve.",
                ));
            }
            return Ok(());
        }
        let mut options = self.pending.take().expect("pending ground options");
        if options.primitive.appearance.is_none() && self.appearance_present {
            // Preserve a caller-provided empty marker; nothing to install.
        }
        let prepared = prepare_ground(&mut options.primitive, frame)?;
        let mut inner = ClassificationPrimitive::with_kind(options, ClassificationKind::Ground);
        inner.classification_type = self.classification_type;
        inner.set_show(self.show);
        inner.set_debug_show_bounding_volume(self.debug_show_bounding_volume);
        inner.debug_show_shadow_volume = self.debug_show_shadow_volume;
        inner.set_ground_bounds(prepared.spheres, prepared.spheres_2d, Some(prepared.heights));
        self.inner = Some(inner);
        Ok(())
    }

    pub fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if self.pending.as_ref().is_some_and(|options| options.primitive.geometry_instances.is_empty())
            && self.inner.is_none()
        {
            return Ok(());
        }
        if !ApproximateTerrainHeights::bundled().initialized() {
            if self.pending.as_ref().is_some_and(|options| options.primitive.asynchronous == Some(false)) {
                return Err(RuntimeError::new(
                    "For synchronous GroundPrimitives, you must call GroundPrimitive.initializeTerrainHeights() and wait for the returned promise to resolve.",
                ));
            }
            return Ok(());
        }
        if self.inner.is_none() {
            self.initialize(frame)?;
        }
        let Some(inner) = self.inner.as_mut() else { return Ok(()) };
        ScenePrimitive::update(inner, frame)?;
        if !self.ready && inner.is_ready() {
            self.ready = true;
        }
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        self.destroyed = true;
        self.pending = None;
        self.inner.take().map_or(Ok(()), |mut inner| ScenePrimitive::destroy(&mut inner))
    }
}

impl<G: Gl + 'static> Default for GroundPrimitive<G> {
    fn default() -> Self {
        Self::new(GroundPrimitiveOptions::default())
    }
}

impl<G: Gl + 'static> Deref for GroundPrimitive<G> {
    type Target = ClassificationPrimitive<G>;
    fn deref(&self) -> &Self::Target {
        self.inner.as_ref().expect("ground primitive has been initialized")
    }
}

impl<G: Gl + 'static> DerefMut for GroundPrimitive<G> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.as_mut().expect("ground primitive has been initialized")
    }
}

impl<G: Gl + 'static> ScenePrimitive for GroundPrimitive<G> {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        Self::update(self, frame)
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: super::Cesium3DTilePass) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Self::destroy(self)
    }
}

pub(crate) struct GroundBounds {
    pub heights: [f64; 2],
    pub spheres: Vec<Option<BoundingSphere>>,
    pub spheres_2d: Vec<Option<BoundingSphere>>,
}

fn footprint(description: &GeometryDescription) -> Option<(Rectangle, [f64; 6])> {
    let (rectangle, rotation) = match description {
        GeometryDescription::CircleGeometry(g) => (g.rectangle(), g.texture_coordinate_rotation_points()),
        GeometryDescription::CorridorGeometry(g) => (g.rectangle()?, g.texture_coordinate_rotation_points()),
        GeometryDescription::EllipseGeometry(g) => (g.rectangle(), g.texture_coordinate_rotation_points()),
        GeometryDescription::PolygonGeometry(g) => (g.rectangle(), g.texture_coordinate_rotation_points()),
        GeometryDescription::RectangleGeometry(g) => (g.rectangle(), g.texture_coordinate_rotation_points()),
        _ => return None,
    };
    Some((rectangle, rotation))
}

fn shadow_volume(description: &GeometryDescription, minimum: f64, maximum: f64) -> Option<GeometryDescription> {
    let min = |_, _: &crate::core::Ellipsoid| minimum;
    let max = |granularity: f64, ellipsoid: &crate::core::Ellipsoid| {
        let radius = ellipsoid.maximum_radius;
        maximum + (radius / (granularity * 0.5).cos() - radius)
    };
    Some(match description {
        GeometryDescription::CircleGeometry(g) => {
            GeometryDescription::CircleGeometry(g.create_shadow_volume(min, max)?)
        }
        GeometryDescription::CorridorGeometry(g) => {
            GeometryDescription::CorridorGeometry(g.create_shadow_volume(min, max))
        }
        GeometryDescription::EllipseGeometry(g) => {
            GeometryDescription::EllipseGeometry(g.create_shadow_volume(min, max)?)
        }
        GeometryDescription::PolygonGeometry(g) => {
            GeometryDescription::PolygonGeometry(g.create_shadow_volume(min, max))
        }
        GeometryDescription::RectangleGeometry(g) => {
            GeometryDescription::RectangleGeometry(g.create_shadow_volume(min, max))
        }
        _ => return None,
    })
}

fn swizzle(value: crate::core::Cartesian3) -> crate::core::Cartesian3 {
    crate::core::Cartesian3::new(value.z, value.x, value.y)
}

pub(crate) fn prepare_ground(
    options: &mut super::PrimitiveOptions,
    frame: &FrameState,
) -> Result<GroundBounds, RuntimeError> {
    let projection = frame.uniform.map_projection.as_ref();
    let ellipsoid = projection.ellipsoid();
    let heights_table = ApproximateTerrainHeights::bundled();
    let footprints = (0..options.geometry_instances.len())
        .map(|index| {
            options
                .geometry_descriptions
                .get(&index)
                .and_then(footprint)
                .ok_or_else(|| RuntimeError::new("Not all of the geometry instances have GroundPrimitive support."))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let rectangle = footprints
        .iter()
        .map(|(rectangle, _)| *rectangle)
        .reduce(|left, right| left.union(&right))
        .ok_or_else(|| RuntimeError::new("GroundPrimitive requires at least one geometry instance."))?;
    let (minimum, maximum) = heights_table
        .get_minimum_maximum_heights(&rectangle, ellipsoid)
        .ok_or_else(|| RuntimeError::new("Approximate terrain heights are not initialized."))?;
    let exaggerate = |height| {
        vertical_exaggeration::get_height(
            height,
            frame.vertical_exaggeration,
            frame.vertical_exaggeration_relative_height,
        )
    };
    let (minimum, maximum) = (exaggerate(minimum), exaggerate(maximum));
    let spherical =
        footprints.iter().any(|(rectangle, _)| ShadowVolumeAppearance::should_use_spherical_coordinates(rectangle));
    let use_fragment_culling = frame.context_capabilities.depth_texture;
    let mut spheres = Vec::with_capacity(options.geometry_instances.len());
    let mut spheres_2d = Vec::with_capacity(options.geometry_instances.len());
    let mut instances = Vec::with_capacity(options.geometry_instances.len());
    let mut volumes = BTreeMap::new();
    for (index, (source, (rectangle, rotation))) in options.geometry_instances.iter().zip(footprints).enumerate() {
        let mut instance = GeometryInstance {
            geometry: None,
            model_matrix: Matrix4::IDENTITY,
            id: source.id.clone(),
            attributes: source.attributes.clone(),
            west_hemisphere_geometry: None,
            east_hemisphere_geometry: None,
        };
        let sphere = crate::core::OrientedBoundingBox::from_rectangle(&rectangle, minimum, maximum, ellipsoid)
            .map(|box_| BoundingSphere::from_oriented_bounding_box(&box_));
        spheres.push(sphere);
        let sphere_2d = if !frame.scene_3d_only {
            let mut sphere =
                BoundingSphere::from_rectangle_with_heights_2d(&rectangle, Some(projection), maximum, minimum);
            sphere.center = swizzle(sphere.center);
            Some(sphere)
        } else {
            None
        };
        spheres_2d.push(sphere_2d);
        if use_fragment_culling {
            let mut attributes = if spherical {
                ShadowVolumeAppearance::get_spherical_extent_geometry_instance_attributes(
                    &rectangle, &rotation, ellipsoid, projection,
                )?
            } else {
                ShadowVolumeAppearance::get_planar_texture_coordinate_attributes(
                    &rectangle,
                    &rotation,
                    ellipsoid,
                    projection,
                    Some(maximum),
                )?
            };
            attributes.retain(|(name, _)| !instance.attributes.iter().any(|(old, _)| old == name));
            attributes.append(&mut instance.attributes);
            instance.attributes = attributes;
        }
        let volume = shadow_volume(&options.geometry_descriptions[&index], minimum, maximum)
            .ok_or_else(|| RuntimeError::new("Unable to construct the ground shadow volume."))?;
        volumes.insert(index, volume);
        instances.push(instance);
    }
    options.geometry_instances = instances;
    options.geometry_descriptions = volumes;
    options.model_matrix = Some(Matrix4::IDENTITY);
    options.cull = Some(true);
    Ok(GroundBounds { heights: [minimum, maximum], spheres, spheres_2d })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::GlValue;

    struct DummyGl;
    impl Gl for DummyGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
            Ok([1, 1])
        }
        fn invoke(&self, _: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
            Ok(GlValue::Null)
        }
    }

    #[test]
    fn set_appearance_before_update_updates_pending_options() {
        let mut primitive = GroundPrimitive::<DummyGl>::default();
        assert!(primitive.pending.as_ref().unwrap().primitive.appearance.is_none());
        primitive.set_appearance(Box::new(super::super::PerInstanceColorAppearance::default()));
        assert!(primitive.pending.as_ref().unwrap().primitive.appearance.is_some());
    }
}
