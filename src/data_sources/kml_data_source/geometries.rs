//! KML geometry processors: Point, LineString, LinearRing, Polygon, gx:Track, gx:MultiTrack, MultiGeometry.
use super::{
    create_entity,
    styles::{
        create_default_billboard, create_default_label, create_default_polygon, extract_material_color_input,
        extract_material_color_value,
    },
    xml::{
        node_text_content, query_boolean_value, query_child_nodes, query_first_node, query_numeric_value,
        query_string_value, read_coordinate, read_coordinates, GX_NAMESPACES, KML_NAMESPACES,
    },
    KmlDataSource,
};
use crate::{
    core::{
        iso8601, ArcType, Cartesian2, Cartesian3, Color, Ellipsoid, JulianDate, PolygonHierarchy, ReferenceFrame,
        RuntimeError, TimeInterval,
    },
    data_sources::{
        visualizer_values::{eval_dynamic, eval_f64_opt},
        Availability, BillboardGraphics, CompositePositionProperty, ConstantPositionProperty, DefinitionChanged,
        DynamicValue, EntityCollection, EntityComponent, EntityRef, GeoJsonPin, LabelGraphics, PathGraphics, PinSymbol,
        PolygonGraphics, PolylineGraphics, PositionPropertyArray, PositionPropertyRef, Property, PropertyComponent,
        PropertyInput, PropertyIntervals, ReferenceProperty, SampledPositionProperty, ScaledPositionProperty,
        TimeIntervalCollectionProperty, WallGraphics,
    },
    scene::HeightReference,
};
use roxmltree::Node;
use std::{any::Any, rc::Rc};

/// Wraps [`PositionPropertyArray`] as an [`EntityComponent`] for `PolylineGraphics.positions`.
pub(crate) struct PositionArrayComponent(pub(crate) Rc<PositionPropertyArray>);

impl EntityComponent for PositionArrayComponent {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.0.definition_changed()
    }
    fn is_constant(&self) -> bool {
        self.0.is_constant()
    }
    fn get_value(
        &self,
        time: &JulianDate,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let positions = self.0.get_value(Some(time), None)?.map(|v| DynamicValue::Positions(Rc::new(v)));
        if let (Some(out), Some(val)) = (result, positions.as_ref()) {
            *out = val.clone();
        }
        Ok(positions)
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else {
            return Ok(false);
        };
        self.0.equals(&*o.0)
    }
}

pub(crate) fn is_extrudable(altitude_mode: Option<&str>, gx_altitude_mode: Option<&str>) -> bool {
    matches!(altitude_mode, Some("absolute" | "relativeToGround"))
        || matches!(gx_altitude_mode, Some("relativeToSeaFloor"))
}

pub(crate) fn height_reference_from_altitude_mode(
    altitude_mode: Option<&str>,
    gx_altitude_mode: Option<&str>,
) -> HeightReference {
    if (altitude_mode.is_none() && gx_altitude_mode.is_none()) || altitude_mode == Some("clampToGround") {
        return HeightReference::ClampToGround;
    }
    if altitude_mode == Some("relativeToGround") {
        return HeightReference::RelativeToGround;
    }
    if altitude_mode == Some("absolute") {
        return HeightReference::None;
    }
    if gx_altitude_mode == Some("clampToSeaFloor") {
        return HeightReference::ClampToGround;
    }
    if gx_altitude_mode == Some("relativeToSeaFloor") {
        return HeightReference::RelativeToGround;
    }
    HeightReference::ClampToGround
}

fn create_position_property_from_altitude_mode(
    property: PositionPropertyRef,
    altitude_mode: Option<&str>,
    gx_altitude_mode: Option<&str>,
) -> PositionPropertyRef {
    if matches!(altitude_mode, Some("absolute" | "relativeToGround"))
        || matches!(gx_altitude_mode, Some("relativeToSeaFloor"))
    {
        return property;
    }
    Rc::new(ScaledPositionProperty::new(Some(property)))
}

fn create_positions_from_altitude_mode(
    mut positions: Vec<Cartesian3>,
    altitude_mode: Option<&str>,
    gx_altitude_mode: Option<&str>,
    ellipsoid: &Ellipsoid,
) -> Vec<Cartesian3> {
    if matches!(altitude_mode, Some("absolute" | "relativeToGround"))
        || matches!(gx_altitude_mode, Some("relativeToSeaFloor"))
    {
        return positions;
    }
    for p in &mut positions {
        if let Some(scaled) = ellipsoid.scale_to_geodetic_surface(*p) {
            *p = scaled;
        }
    }
    positions
}

pub(crate) fn create_drop_line(
    entity_collection: &Rc<EntityCollection>,
    entity: &EntityRef,
    style_entity: &EntityRef,
) -> Result<(), RuntimeError> {
    let entity_pos_ref = ReferenceProperty::new(entity_collection.clone(), entity.id(), vec!["position".to_owned()])?;
    let entity_position = entity_pos_ref.as_position();
    let surface_position: PositionPropertyRef = Rc::new(ScaledPositionProperty::new(entity.position()));
    let polyline: Rc<PolylineGraphics> = match style_entity.get_property("polyline").and_then(|p| p.component()) {
        Some(c) => {
            if let Some(pl) = c.as_any().downcast_ref::<PolylineGraphics>() {
                Rc::new(pl.clone_definition()?)
            } else {
                Rc::new(PolylineGraphics::new())
            }
        }
        None => Rc::new(PolylineGraphics::new()),
    };
    let arr = Rc::new(PositionPropertyArray::new(Some(vec![entity_position, surface_position]), ReferenceFrame::Fixed));
    polyline.set_positions(Some(PropertyInput::Component(Rc::new(PositionArrayComponent(arr)))))?;
    entity.set_property("polyline", Some(PropertyInput::Component(polyline)))?;
    Ok(())
}

pub(crate) fn process_position_graphics(
    data_source: &KmlDataSource,
    entity: &EntityRef,
    style_entity: &EntityRef,
    height_reference: Option<HeightReference>,
) -> Result<(), RuntimeError> {
    let label_comp = match entity.get_property("label").and_then(|p| p.component()) {
        Some(c) if c.as_any().is::<LabelGraphics>() => c,
        _ => {
            let lbl = match style_entity.get_property("label").and_then(|p| p.component()) {
                Some(sc) => {
                    if let Some(sl) = sc.as_any().downcast_ref::<LabelGraphics>() {
                        Rc::new(sl.clone_definition()?)
                    } else {
                        create_default_label()?
                    }
                }
                None => create_default_label()?,
            };
            entity.set_property("label", Some(PropertyInput::Component(lbl.clone())))?;
            lbl
        }
    };
    let label = label_comp.as_any().downcast_ref::<LabelGraphics>().expect("label graphics");
    if let Some(name) = entity.name() {
        label.set_text(Some(PropertyInput::value(name)))?;
    }

    let bb_comp = match entity.get_property("billboard").and_then(|p| p.component()) {
        Some(c) if c.as_any().is::<BillboardGraphics>() => c,
        _ => {
            let bb = match style_entity.get_property("billboard").and_then(|p| p.component()) {
                Some(sc) => {
                    if let Some(sb) = sc.as_any().downcast_ref::<BillboardGraphics>() {
                        Rc::new(sb.clone_definition()?)
                    } else {
                        create_default_billboard()?
                    }
                }
                None => create_default_billboard()?,
            };
            entity.set_property("billboard", Some(PropertyInput::Component(bb.clone())))?;
            bb
        }
    };
    let billboard = bb_comp.as_any().downcast_ref::<BillboardGraphics>().expect("billboard graphics");

    let eval_time = iso8601::minimum_value();
    match billboard.image() {
        None => {
            billboard.set_image(Some(PropertyInput::Value(DynamicValue::Pin(Rc::new(GeoJsonPin {
                symbol: PinSymbol::Color,
                color: Color::YELLOW,
                size: 64.0,
            })))))?;
        }
        Some(img) => {
            let is_falsy = match eval_dynamic(Some(&img), &eval_time)? {
                None | Some(DynamicValue::Null | DynamicValue::Boolean(false)) => true,
                Some(DynamicValue::String(s)) => s.is_empty(),
                _ => false,
            };
            if is_falsy {
                billboard.set_image(None)?;
            }
        }
    }

    if let Some(scale) = eval_f64_opt(billboard.scale().as_ref(), &eval_time)? {
        if scale != 0.0 {
            label.set_pixel_offset(Some(PropertyInput::value(Cartesian2::new(scale * 16.0 + 1.0, 0.0))))?;
        } else {
            label.set_pixel_offset(None)?;
            label.set_horizontal_origin(None)?;
        }
    }

    if let Some(hr) = height_reference {
        if data_source.clamp_to_ground() {
            let hr_val = PropertyInput::value(hr as i32 as f64);
            billboard.set_height_reference(Some(hr_val.clone()))?;
            label.set_height_reference(Some(hr_val))?;
        }
    }

    Ok(())
}

fn process_path_graphics(entity: &EntityRef, style_entity: &EntityRef) -> Result<(), RuntimeError> {
    let path_comp = match entity.get_property("path").and_then(|p| p.component()) {
        Some(c) if c.as_any().is::<PathGraphics>() => c,
        _ => {
            let p = Rc::new(PathGraphics::new());
            p.set_lead_time(Some(PropertyInput::value(0.0)))?;
            entity.set_property("path", Some(PropertyInput::Component(p.clone())))?;
            p
        }
    };
    let path = path_comp.as_any().downcast_ref::<PathGraphics>().expect("path graphics");
    if let Some(pl_comp) = style_entity.get_property("polyline").and_then(|p| p.component()) {
        if let Some(pl) = pl_comp.as_any().downcast_ref::<PolylineGraphics>() {
            path.set_material(pl.material())?;
            path.set_width(pl.width())?;
        }
    }
    Ok(())
}

pub(crate) fn is_geometry_tag(name: &str) -> bool {
    matches!(
        name,
        "Point" | "LineString" | "LinearRing" | "Polygon" | "Track" | "MultiTrack" | "MultiGeometry" | "Model"
    )
}

pub(crate) fn process_geometry(
    data_source: &KmlDataSource,
    entity_collection: &Rc<EntityCollection>,
    geometry_node: Node<'_, '_>,
    entity: &EntityRef,
    style_entity: &EntityRef,
    context: Option<&str>,
) -> Result<bool, RuntimeError> {
    match geometry_node.tag_name().name() {
        "Point" => process_point(data_source, entity_collection, geometry_node, entity, style_entity),
        "LineString" | "LinearRing" => {
            process_line_string_or_linear_ring(data_source, geometry_node, entity, style_entity)
        }
        "Polygon" => process_polygon(data_source, geometry_node, entity, style_entity),
        "Track" => process_track(data_source, entity_collection, geometry_node, entity, style_entity),
        "MultiTrack" => process_multi_track(data_source, entity_collection, geometry_node, entity, style_entity),
        "MultiGeometry" => {
            process_multi_geometry(data_source, entity_collection, geometry_node, entity, style_entity, context)
        }
        "Model" => Ok(false),
        _ => Ok(false),
    }
}

fn process_point(
    data_source: &KmlDataSource,
    entity_collection: &Rc<EntityCollection>,
    geometry_node: Node<'_, '_>,
    entity: &EntityRef,
    style_entity: &EntityRef,
) -> Result<bool, RuntimeError> {
    let coords_str = query_string_value(geometry_node, "coordinates", KML_NAMESPACES);
    let altitude_mode = query_string_value(geometry_node, "altitudeMode", KML_NAMESPACES);
    let gx_altitude_mode = query_string_value(geometry_node, "altitudeMode", GX_NAMESPACES);
    let extrude = query_boolean_value(geometry_node, "extrude", KML_NAMESPACES).unwrap_or(false);
    let position = read_coordinate(coords_str.as_deref(), &data_source.ellipsoid());

    entity.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(position), ReferenceFrame::Fixed))))?;
    let hr = height_reference_from_altitude_mode(altitude_mode.as_deref(), gx_altitude_mode.as_deref());
    process_position_graphics(data_source, entity, style_entity, Some(hr))?;

    if extrude && is_extrudable(altitude_mode.as_deref(), gx_altitude_mode.as_deref()) {
        create_drop_line(entity_collection, entity, style_entity)?;
    }

    Ok(true)
}

fn process_line_string_or_linear_ring(
    data_source: &KmlDataSource,
    geometry_node: Node<'_, '_>,
    entity: &EntityRef,
    style_entity: &EntityRef,
) -> Result<bool, RuntimeError> {
    let coordinates_node = query_first_node(geometry_node, "coordinates", KML_NAMESPACES);
    let altitude_mode = query_string_value(geometry_node, "altitudeMode", KML_NAMESPACES);
    let gx_altitude_mode = query_string_value(geometry_node, "altitudeMode", GX_NAMESPACES);
    let extrude = query_boolean_value(geometry_node, "extrude", KML_NAMESPACES).unwrap_or(false);
    let tessellate = query_boolean_value(geometry_node, "tessellate", KML_NAMESPACES).unwrap_or(false);
    let can_extrude = is_extrudable(altitude_mode.as_deref(), gx_altitude_mode.as_deref());
    let z_index = query_numeric_value(geometry_node, "drawOrder", GX_NAMESPACES);

    let ellipsoid = data_source.ellipsoid();
    let coordinates = read_coordinates(coordinates_node, &ellipsoid);

    let style_polyline = style_entity
        .get_property("polyline")
        .and_then(|p| p.component())
        .filter(|c| c.as_any().is::<PolylineGraphics>());

    if can_extrude && extrude {
        let wall = Rc::new(WallGraphics::new());
        if let Some(coords) = coordinates {
            wall.set_positions(Some(PropertyInput::Value(DynamicValue::Positions(Rc::new(coords)))))?;
        }
        let style_polygon = style_entity
            .get_property("polygon")
            .and_then(|p| p.component())
            .filter(|c| c.as_any().is::<PolygonGraphics>());
        if let Some(pg_comp) = &style_polygon {
            let pg = pg_comp.as_any().downcast_ref::<PolygonGraphics>().expect("polygon");
            wall.set_fill(pg.fill())?;
            wall.set_material(pg.material())?;
        }
        wall.set_outline(Some(PropertyInput::value(true)))?;
        if let Some(pl_comp) = &style_polyline {
            let pl = pl_comp.as_any().downcast_ref::<PolylineGraphics>().expect("polyline");
            let outline_color =
                extract_material_color_input(pl.material()).unwrap_or_else(|| PropertyInput::value(Color::WHITE));
            wall.set_outline_color(Some(outline_color))?;
            wall.set_outline_width(pl.width())?;
        } else if let Some(pg_comp) = &style_polygon {
            let pg = pg_comp.as_any().downcast_ref::<PolygonGraphics>().expect("polygon");
            let outline_color =
                extract_material_color_input(pg.material()).unwrap_or_else(|| PropertyInput::value(Color::WHITE));
            wall.set_outline_color(Some(outline_color))?;
        }
        entity.set_property("wall", Some(PropertyInput::Component(wall)))?;
    } else if data_source.clamp_to_ground() && !can_extrude && tessellate {
        let polyline = Rc::new(PolylineGraphics::new());
        polyline.set_clamp_to_ground(Some(PropertyInput::value(true)))?;
        if let Some(coords) = coordinates {
            polyline.set_positions(Some(PropertyInput::Value(DynamicValue::Positions(Rc::new(coords)))))?;
        }
        if let Some(pl_comp) = &style_polyline {
            let pl = pl_comp.as_any().downcast_ref::<PolylineGraphics>().expect("polyline");
            let color = extract_material_color_value(pl.material()).unwrap_or(Color::WHITE);
            polyline.set_material(Some(PropertyInput::value(color)))?;
            polyline.set_width(pl.width().or_else(|| Some(PropertyInput::value(1.0))))?;
        } else {
            polyline.set_material(Some(PropertyInput::value(Color::WHITE)))?;
            polyline.set_width(Some(PropertyInput::value(1.0)))?;
        }
        polyline.set_z_index(z_index.map(PropertyInput::value))?;
        entity.set_property("polyline", Some(PropertyInput::Component(polyline)))?;
    } else {
        let polyline = match &style_polyline {
            Some(pl_comp) => {
                let pl = pl_comp.as_any().downcast_ref::<PolylineGraphics>().expect("polyline");
                Rc::new(pl.clone_definition()?)
            }
            None => Rc::new(PolylineGraphics::new()),
        };
        if let Some(coords) = coordinates {
            let adjusted = create_positions_from_altitude_mode(
                coords,
                altitude_mode.as_deref(),
                gx_altitude_mode.as_deref(),
                &ellipsoid,
            );
            polyline.set_positions(Some(PropertyInput::Value(DynamicValue::Positions(Rc::new(adjusted)))))?;
        }
        if !tessellate || can_extrude {
            polyline.set_arc_type(Some(PropertyInput::value(ArcType::None as i32 as f64)))?;
        }
        entity.set_property("polyline", Some(PropertyInput::Component(polyline)))?;
    }

    Ok(true)
}

fn process_polygon(
    data_source: &KmlDataSource,
    geometry_node: Node<'_, '_>,
    entity: &EntityRef,
    style_entity: &EntityRef,
) -> Result<bool, RuntimeError> {
    let outer_boundary = query_first_node(geometry_node, "outerBoundaryIs", KML_NAMESPACES);
    let linear_ring = outer_boundary.and_then(|n| query_first_node(n, "LinearRing", KML_NAMESPACES));
    let coords_node = linear_ring.and_then(|n| query_first_node(n, "coordinates", KML_NAMESPACES));

    let ellipsoid = data_source.ellipsoid();
    let outer_coords = read_coordinates(coords_node, &ellipsoid);
    let extrude = query_boolean_value(geometry_node, "extrude", KML_NAMESPACES).unwrap_or(false);
    let altitude_mode = query_string_value(geometry_node, "altitudeMode", KML_NAMESPACES);
    let gx_altitude_mode = query_string_value(geometry_node, "altitudeMode", GX_NAMESPACES);
    let can_extrude = is_extrudable(altitude_mode.as_deref(), gx_altitude_mode.as_deref());

    let polygon = match style_entity.get_property("polygon").and_then(|p| p.component()) {
        Some(pg_comp) => {
            if let Some(pg) = pg_comp.as_any().downcast_ref::<PolygonGraphics>() {
                Rc::new(pg.clone_definition()?)
            } else {
                create_default_polygon()?
            }
        }
        None => create_default_polygon()?,
    };

    if let Some(pl_comp) = style_entity.get_property("polyline").and_then(|p| p.component()) {
        if let Some(pl) = pl_comp.as_any().downcast_ref::<PolylineGraphics>() {
            let outline_color =
                extract_material_color_input(pl.material()).unwrap_or_else(|| PropertyInput::value(Color::WHITE));
            polygon.set_outline_color(Some(outline_color))?;
            polygon.set_outline_width(pl.width())?;
        }
    }

    if can_extrude {
        polygon.set_per_position_height(Some(PropertyInput::value(true)))?;
        polygon.set_extruded_height(if extrude { Some(PropertyInput::value(0.0)) } else { None })?;
    } else if !data_source.clamp_to_ground() {
        polygon.set_height(Some(PropertyInput::value(0.0)))?;
    }

    if let Some(coords) = outer_coords {
        let mut holes = Vec::new();
        for inner_node in query_child_nodes(geometry_node, "innerBoundaryIs", KML_NAMESPACES) {
            for ring in query_child_nodes(inner_node, "LinearRing", KML_NAMESPACES) {
                let hole_coords_node = query_first_node(ring, "coordinates", KML_NAMESPACES);
                if let Some(hole_coords) = read_coordinates(hole_coords_node, &ellipsoid) {
                    holes.push(PolygonHierarchy::new(hole_coords, Vec::new()));
                }
            }
        }
        let hierarchy = PolygonHierarchy::new(coords, holes);
        polygon.set_hierarchy(Some(PropertyInput::Value(DynamicValue::PolygonHierarchy(Rc::new(hierarchy)))))?;
    }

    entity.set_property("polygon", Some(PropertyInput::Component(polygon)))?;
    Ok(true)
}

fn process_track(
    data_source: &KmlDataSource,
    entity_collection: &Rc<EntityCollection>,
    geometry_node: Node<'_, '_>,
    entity: &EntityRef,
    style_entity: &EntityRef,
) -> Result<bool, RuntimeError> {
    let altitude_mode = query_string_value(geometry_node, "altitudeMode", KML_NAMESPACES);
    let gx_altitude_mode = query_string_value(geometry_node, "altitudeMode", GX_NAMESPACES);
    let coord_nodes = query_child_nodes(geometry_node, "coord", GX_NAMESPACES);
    let time_nodes = query_child_nodes(geometry_node, "when", KML_NAMESPACES);
    let extrude = query_boolean_value(geometry_node, "extrude", KML_NAMESPACES).unwrap_or(false);
    let can_extrude = is_extrudable(altitude_mode.as_deref(), gx_altitude_mode.as_deref());
    let ellipsoid = data_source.ellipsoid();

    let len = coord_nodes.len().min(time_nodes.len());
    let mut coordinates = Vec::with_capacity(len);
    let mut times = Vec::with_capacity(len);
    for i in 0..len {
        let pos = read_coordinate(Some(&node_text_content(coord_nodes[i])), &ellipsoid);
        let t = JulianDate::from_iso8601(node_text_content(time_nodes[i]).trim())
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        coordinates.push(pos);
        times.push(t);
    }

    let property = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
    property.add_samples(&times, &coordinates, None)?;
    entity.set_position(Some(property))?;

    let hr = height_reference_from_altitude_mode(altitude_mode.as_deref(), gx_altitude_mode.as_deref());
    process_position_graphics(data_source, entity, style_entity, Some(hr))?;
    process_path_graphics(entity, style_entity)?;

    let availability: Availability = Rc::new(PropertyIntervals::new(|a: &Option<()>, b: &Option<()>| a == b));
    if !times.is_empty() {
        availability.add_interval(TimeInterval::new(times[0], times[times.len() - 1], true, true, None));
    }
    entity.set_availability(Some(availability));

    if can_extrude && extrude {
        create_drop_line(entity_collection, entity, style_entity)?;
    }

    Ok(true)
}

struct MultiTrackTargets<'a> {
    composite: &'a CompositePositionProperty,
    availability: &'a Availability,
    drop_show: &'a TimeIntervalCollectionProperty<bool>,
}

fn add_to_multi_track(
    times: &[JulianDate],
    positions: &[Cartesian3],
    targets: &MultiTrackTargets<'_>,
    extrude: bool,
    altitude_mode: Option<&str>,
    gx_altitude_mode: Option<&str>,
    include_end_points: bool,
) -> Result<(), RuntimeError> {
    if times.is_empty() {
        return Ok(());
    }
    let start = times[0];
    let stop = times[times.len() - 1];

    let sampled = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
    sampled.add_samples(times, positions, None)?;
    let data = create_position_property_from_altitude_mode(sampled, altitude_mode, gx_altitude_mode);

    targets.composite.intervals().add_interval(TimeInterval::new(
        start,
        stop,
        include_end_points,
        include_end_points,
        Some(data),
    ));
    targets.availability.add_interval(TimeInterval::new(start, stop, include_end_points, include_end_points, None));
    targets.drop_show.intervals().add_interval(TimeInterval::new(
        start,
        stop,
        include_end_points,
        include_end_points,
        Some(Rc::new(extrude)),
    ));
    Ok(())
}

fn process_multi_track(
    data_source: &KmlDataSource,
    entity_collection: &Rc<EntityCollection>,
    geometry_node: Node<'_, '_>,
    entity: &EntityRef,
    style_entity: &EntityRef,
) -> Result<bool, RuntimeError> {
    let interpolate = query_boolean_value(geometry_node, "interpolate", GX_NAMESPACES).unwrap_or(false);
    let track_nodes = query_child_nodes(geometry_node, "Track", GX_NAMESPACES);

    let mut last_stop: Option<JulianDate> = None;
    let mut last_stop_pos: Option<Cartesian3> = None;
    let mut need_drop_line = false;

    let drop_show = Rc::new(TimeIntervalCollectionProperty::<bool>::new());
    let availability: Availability = Rc::new(PropertyIntervals::new(|a: &Option<()>, b: &Option<()>| a == b));
    let composite = Rc::new(CompositePositionProperty::new(ReferenceFrame::Fixed));
    let targets = MultiTrackTargets { composite: &composite, availability: &availability, drop_show: &drop_show };
    let ellipsoid = data_source.ellipsoid();

    for track_node in track_nodes {
        let time_nodes = query_child_nodes(track_node, "when", KML_NAMESPACES);
        let coord_nodes = query_child_nodes(track_node, "coord", GX_NAMESPACES);
        let altitude_mode = query_string_value(track_node, "altitudeMode", KML_NAMESPACES);
        let gx_altitude_mode = query_string_value(track_node, "altitudeMode", GX_NAMESPACES);
        let can_extrude = is_extrudable(altitude_mode.as_deref(), gx_altitude_mode.as_deref());
        let extrude = query_boolean_value(track_node, "extrude", KML_NAMESPACES).unwrap_or(false);

        let len = coord_nodes.len().min(time_nodes.len());
        let mut positions = Vec::with_capacity(len);
        let mut times = Vec::with_capacity(len);
        for i in 0..len {
            let pos = read_coordinate(Some(&node_text_content(coord_nodes[i])), &ellipsoid);
            let t = JulianDate::from_iso8601(node_text_content(time_nodes[i]).trim())
                .map_err(|e| RuntimeError::new(e.to_string()))?;
            positions.push(pos);
            times.push(t);
        }

        if interpolate && !times.is_empty() {
            if let (Some(prev_t), Some(prev_p)) = (last_stop, last_stop_pos) {
                add_to_multi_track(
                    &[prev_t, times[0]],
                    &[prev_p, positions[0]],
                    &targets,
                    false,
                    Some("absolute"),
                    None,
                    false,
                )?;
            }
            last_stop = times.last().copied();
            last_stop_pos = positions.last().copied();
        }

        let track_extrude = can_extrude && extrude;
        add_to_multi_track(
            &times,
            &positions,
            &targets,
            track_extrude,
            altitude_mode.as_deref(),
            gx_altitude_mode.as_deref(),
            true,
        )?;
        need_drop_line = need_drop_line || track_extrude;
    }

    entity.set_availability(Some(availability));
    entity.set_position(Some(composite))?;
    process_position_graphics(data_source, entity, style_entity, None)?;
    process_path_graphics(entity, style_entity)?;
    if need_drop_line {
        create_drop_line(entity_collection, entity, style_entity)?;
        if let Some(pl_comp) = entity.get_property("polyline").and_then(|p| p.component()) {
            if let Some(pl) = pl_comp.as_any().downcast_ref::<PolylineGraphics>() {
                pl.set_show(Some(PropertyInput::Component(PropertyComponent::wrap(drop_show))))?;
            }
        }
    }

    Ok(true)
}

fn process_multi_geometry(
    data_source: &KmlDataSource,
    entity_collection: &Rc<EntityCollection>,
    geometry_node: Node<'_, '_>,
    entity: &EntityRef,
    style_entity: &EntityRef,
    context: Option<&str>,
) -> Result<bool, RuntimeError> {
    let mut has_geometry = false;
    for child in geometry_node.children().filter(Node::is_element) {
        if is_geometry_tag(child.tag_name().name()) {
            let child_entity = create_entity(child, entity_collection, context)?;
            child_entity.set_parent(Some(entity.clone()))?;
            child_entity.set_name(entity.name());
            child_entity.set_availability(entity.availability());
            child_entity.set_property("description", entity.get_property("description"))?;
            child_entity.set_property("kml", entity.get_property("kml"))?;
            if process_geometry(data_source, entity_collection, child, &child_entity, style_entity, None)? {
                has_geometry = true;
            }
        }
    }
    Ok(has_geometry)
}
