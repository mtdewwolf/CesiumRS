//! Built-in CZML entity and graphics packet updaters matching
//! `CzmlDataSource.updaters` in CesiumJS 23.0.0.
use super::{
    adapters::{CzmlPolygonHierarchyProperty, SlotTarget},
    materials::process_material_packet_data,
    packet::{
        process_array, process_packet_data, process_position_array, process_position_array_of_arrays, process_property,
        process_shape, PacketContext,
    },
    position::process_position_packet_data,
    unwrap::{combine_intervals, get_defined, get_property_type, interval_from_string, is_defined},
    CzmlUpdater, CzmlValueType,
};
use crate::{
    core::{query::js_object_order, ArcType, Resource, RuntimeError, TimeInterval},
    data_sources::{
        graphics::Graphics, BillboardGraphics, BoxGraphics, CallbackProperty, Cesium3DTilesetGraphics, ComponentRef,
        CorridorGraphics, CylinderGraphics, DynamicValue, EllipseGraphics, EllipsoidGraphics, EntityCollection,
        EntityRef, LabelGraphics, ModelGraphics, NodeTransformationProperty, PathGraphics, PointGraphics,
        PolygonGraphics, PolylineGraphics, PolylineVolumeGraphics, PropertyBag, PropertyComponent, PropertyInput,
        PropertyIntervals, PropertyRef, RectangleGraphics, WallGraphics,
    },
};
use serde_json::Value;
use std::rc::Rc;

fn get_or_create_graphics<T: Graphics + Default + 'static>(
    entity: &EntityRef,
    name: &str,
) -> Result<ComponentRef, RuntimeError> {
    if let Some(PropertyInput::Component(c)) = entity.get_property(name) {
        if c.as_any().is::<T>() {
            return Ok(c);
        }
    }
    let g: ComponentRef = Rc::new(T::default());
    entity.set_property(name, Some(PropertyInput::Component(g.clone())))?;
    Ok(g)
}

fn process_name(
    entity: &EntityRef,
    packet: &Value,
    _entity_collection: &Rc<EntityCollection>,
    _source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    if let Some(name) = get_defined(packet, "name").and_then(Value::as_str) {
        entity.set_name(Some(name.to_owned()));
    }
    Ok(())
}

fn process_description(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let ctx = PacketContext { interval: None, source_uri, entity_collection };
    process_packet_data(CzmlValueType::String, &**entity, "description", packet.get("description"), &ctx)
}

fn process_position(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let ctx = PacketContext { interval: None, source_uri, entity_collection };
    process_position_packet_data(&**entity, "position", packet.get("position"), &ctx)
}

fn process_view_from(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let ctx = PacketContext { interval: None, source_uri, entity_collection };
    process_packet_data(CzmlValueType::Cartesian3, &**entity, "viewFrom", packet.get("viewFrom"), &ctx)
}

fn process_orientation(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let ctx = PacketContext { interval: None, source_uri, entity_collection };
    process_packet_data(CzmlValueType::Quaternion, &**entity, "orientation", packet.get("orientation"), &ctx)
}

fn process_availability(
    entity: &EntityRef,
    packet: &Value,
    _entity_collection: &Rc<EntityCollection>,
    _source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "availability") else {
        return Ok(());
    };
    let intervals = if let Some(arr) = data.as_array() {
        let mut collection: Option<PropertyIntervals<()>> = None;
        for item in arr {
            let s = item
                .as_str()
                .ok_or_else(|| RuntimeError::new("Availability array entries must be ISO 8601 interval strings."))?;
            let iv = interval_from_string(Some(s))?.unwrap();
            collection.get_or_insert_with(|| PropertyIntervals::new(|_, _| true)).add_interval(iv);
        }
        collection.map(Rc::new)
    } else if let Some(s) = data.as_str() {
        let collection = PropertyIntervals::new(|_, _| true);
        if let Some(iv) = interval_from_string(Some(s))? {
            collection.add_interval(iv);
        }
        Some(Rc::new(collection))
    } else {
        return Err(RuntimeError::new("Invalid availability format."));
    };
    entity.set_availability(intervals);
    Ok(())
}

fn process_properties(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(props_obj) = get_defined(packet, "properties").and_then(Value::as_object) else {
        return Ok(());
    };
    let bag_comp: ComponentRef = match entity.get_property("properties") {
        Some(PropertyInput::Component(c)) if c.as_any().is::<PropertyBag>() => c,
        _ => {
            let b: ComponentRef = Rc::new(PropertyBag::new());
            entity.set_property("properties", Some(PropertyInput::Component(b.clone())))?;
            b
        }
    };
    let bag = bag_comp.as_any().downcast_ref::<PropertyBag>().unwrap();
    let ctx = PacketContext { interval: None, source_uri, entity_collection };

    let ordered = js_object_order(props_obj.iter().map(|(k, v)| (k.clone(), v)).collect());
    for (key, property_data) in ordered {
        if !bag.has_property(&key) {
            bag.add_property(key.clone(), None, None)?;
        }
        if property_data.is_null() {
            continue;
        }
        if let Some(arr) = property_data.as_array() {
            for item in arr {
                process_property(get_property_type(item), bag, &key, item, &ctx)?;
            }
        } else {
            process_property(get_property_type(property_data), bag, &key, property_data, &ctx)?;
        }
    }
    Ok(())
}

fn process_billboard(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "billboard") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<BillboardGraphics>(entity, "billboard")?;
    let billboard = comp.as_any().downcast_ref::<BillboardGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, billboard, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::Image, billboard, "image", data.get("image"), &ctx)?;
    process_packet_data(CzmlValueType::Number, billboard, "scale", data.get("scale"), &ctx)?;
    process_packet_data(CzmlValueType::Cartesian2, billboard, "pixelOffset", data.get("pixelOffset"), &ctx)?;
    process_packet_data(CzmlValueType::Cartesian3, billboard, "eyeOffset", data.get("eyeOffset"), &ctx)?;
    process_packet_data(
        CzmlValueType::HorizontalOrigin,
        billboard,
        "horizontalOrigin",
        data.get("horizontalOrigin"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::VerticalOrigin, billboard, "verticalOrigin", data.get("verticalOrigin"), &ctx)?;
    process_packet_data(
        CzmlValueType::HeightReference,
        billboard,
        "heightReference",
        data.get("heightReference"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Color, billboard, "color", data.get("color"), &ctx)?;
    process_packet_data(CzmlValueType::Rotation, billboard, "rotation", data.get("rotation"), &ctx)?;
    process_packet_data(CzmlValueType::UnitCartesian3, billboard, "alignedAxis", data.get("alignedAxis"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, billboard, "sizeInMeters", data.get("sizeInMeters"), &ctx)?;
    process_packet_data(CzmlValueType::Number, billboard, "width", data.get("width"), &ctx)?;
    process_packet_data(CzmlValueType::Number, billboard, "height", data.get("height"), &ctx)?;
    process_packet_data(CzmlValueType::NearFarScalar, billboard, "scaleByDistance", data.get("scaleByDistance"), &ctx)?;
    process_packet_data(
        CzmlValueType::NearFarScalar,
        billboard,
        "translucencyByDistance",
        data.get("translucencyByDistance"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::NearFarScalar,
        billboard,
        "pixelOffsetScaleByDistance",
        data.get("pixelOffsetScaleByDistance"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::BoundingRectangle,
        billboard,
        "imageSubRegion",
        data.get("imageSubRegion"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        billboard,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::Number,
        billboard,
        "disableDepthTestDistance",
        data.get("disableDepthTestDistance"),
        &ctx,
    )?;
    Ok(())
}

fn process_box(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "box") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<BoxGraphics>(entity, "box")?;
    let box_g = comp.as_any().downcast_ref::<BoxGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, box_g, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::Cartesian3, box_g, "dimensions", data.get("dimensions"), &ctx)?;
    process_packet_data(CzmlValueType::HeightReference, box_g, "heightReference", data.get("heightReference"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, box_g, "fill", data.get("fill"), &ctx)?;
    process_material_packet_data(box_g, "material", data.get("material"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, box_g, "outline", data.get("outline"), &ctx)?;
    process_packet_data(CzmlValueType::Color, box_g, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, box_g, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(CzmlValueType::ShadowMode, box_g, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        box_g,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    Ok(())
}

fn process_corridor(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "corridor") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<CorridorGraphics>(entity, "corridor")?;
    let corridor = comp.as_any().downcast_ref::<CorridorGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, corridor, "show", data.get("show"), &ctx)?;
    process_position_array(corridor, "positions", data.get("positions"), entity_collection)?;
    process_packet_data(CzmlValueType::Number, corridor, "width", data.get("width"), &ctx)?;
    process_packet_data(CzmlValueType::Number, corridor, "height", data.get("height"), &ctx)?;
    process_packet_data(
        CzmlValueType::HeightReference,
        corridor,
        "heightReference",
        data.get("heightReference"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Number, corridor, "extrudedHeight", data.get("extrudedHeight"), &ctx)?;
    process_packet_data(
        CzmlValueType::HeightReference,
        corridor,
        "extrudedHeightReference",
        data.get("extrudedHeightReference"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::CornerType, corridor, "cornerType", data.get("cornerType"), &ctx)?;
    process_packet_data(CzmlValueType::Number, corridor, "granularity", data.get("granularity"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, corridor, "fill", data.get("fill"), &ctx)?;
    process_material_packet_data(corridor, "material", data.get("material"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, corridor, "outline", data.get("outline"), &ctx)?;
    process_packet_data(CzmlValueType::Color, corridor, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, corridor, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(CzmlValueType::ShadowMode, corridor, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        corridor,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::ClassificationType,
        corridor,
        "classificationType",
        data.get("classificationType"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Number, corridor, "zIndex", data.get("zIndex"), &ctx)?;
    Ok(())
}

fn process_cylinder(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "cylinder") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<CylinderGraphics>(entity, "cylinder")?;
    let cylinder = comp.as_any().downcast_ref::<CylinderGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, cylinder, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::Number, cylinder, "length", data.get("length"), &ctx)?;
    process_packet_data(CzmlValueType::Number, cylinder, "topRadius", data.get("topRadius"), &ctx)?;
    process_packet_data(CzmlValueType::Number, cylinder, "bottomRadius", data.get("bottomRadius"), &ctx)?;
    process_packet_data(
        CzmlValueType::HeightReference,
        cylinder,
        "heightReference",
        data.get("heightReference"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Boolean, cylinder, "fill", data.get("fill"), &ctx)?;
    process_material_packet_data(cylinder, "material", data.get("material"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, cylinder, "outline", data.get("outline"), &ctx)?;
    process_packet_data(CzmlValueType::Color, cylinder, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, cylinder, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(
        CzmlValueType::Number,
        cylinder,
        "numberOfVerticalLines",
        data.get("numberOfVerticalLines"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Number, cylinder, "slices", data.get("slices"), &ctx)?;
    process_packet_data(CzmlValueType::ShadowMode, cylinder, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        cylinder,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    Ok(())
}

fn process_ellipse(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "ellipse") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<EllipseGraphics>(entity, "ellipse")?;
    let ellipse = comp.as_any().downcast_ref::<EllipseGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, ellipse, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipse, "semiMajorAxis", data.get("semiMajorAxis"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipse, "semiMinorAxis", data.get("semiMinorAxis"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipse, "height", data.get("height"), &ctx)?;
    process_packet_data(CzmlValueType::HeightReference, ellipse, "heightReference", data.get("heightReference"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipse, "extrudedHeight", data.get("extrudedHeight"), &ctx)?;
    process_packet_data(
        CzmlValueType::HeightReference,
        ellipse,
        "extrudedHeightReference",
        data.get("extrudedHeightReference"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Rotation, ellipse, "rotation", data.get("rotation"), &ctx)?;
    process_packet_data(CzmlValueType::Rotation, ellipse, "stRotation", data.get("stRotation"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipse, "granularity", data.get("granularity"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, ellipse, "fill", data.get("fill"), &ctx)?;
    process_material_packet_data(ellipse, "material", data.get("material"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, ellipse, "outline", data.get("outline"), &ctx)?;
    process_packet_data(CzmlValueType::Color, ellipse, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipse, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(
        CzmlValueType::Number,
        ellipse,
        "numberOfVerticalLines",
        data.get("numberOfVerticalLines"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::ShadowMode, ellipse, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        ellipse,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::ClassificationType,
        ellipse,
        "classificationType",
        data.get("classificationType"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Number, ellipse, "zIndex", data.get("zIndex"), &ctx)?;
    Ok(())
}

fn process_ellipsoid(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "ellipsoid") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<EllipsoidGraphics>(entity, "ellipsoid")?;
    let ellipsoid = comp.as_any().downcast_ref::<EllipsoidGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, ellipsoid, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::Cartesian3, ellipsoid, "radii", data.get("radii"), &ctx)?;
    process_packet_data(CzmlValueType::Cartesian3, ellipsoid, "innerRadii", data.get("innerRadii"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipsoid, "minimumClock", data.get("minimumClock"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipsoid, "maximumClock", data.get("maximumClock"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipsoid, "minimumCone", data.get("minimumCone"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipsoid, "maximumCone", data.get("maximumCone"), &ctx)?;
    process_packet_data(
        CzmlValueType::HeightReference,
        ellipsoid,
        "heightReference",
        data.get("heightReference"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Boolean, ellipsoid, "fill", data.get("fill"), &ctx)?;
    process_material_packet_data(ellipsoid, "material", data.get("material"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, ellipsoid, "outline", data.get("outline"), &ctx)?;
    process_packet_data(CzmlValueType::Color, ellipsoid, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipsoid, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipsoid, "stackPartitions", data.get("stackPartitions"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipsoid, "slicePartitions", data.get("slicePartitions"), &ctx)?;
    process_packet_data(CzmlValueType::Number, ellipsoid, "subdivisions", data.get("subdivisions"), &ctx)?;
    process_packet_data(CzmlValueType::ShadowMode, ellipsoid, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        ellipsoid,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    Ok(())
}

fn process_label(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "label") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<LabelGraphics>(entity, "label")?;
    let label = comp.as_any().downcast_ref::<LabelGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, label, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::String, label, "text", data.get("text"), &ctx)?;
    process_packet_data(CzmlValueType::String, label, "font", data.get("font"), &ctx)?;
    process_packet_data(CzmlValueType::LabelStyle, label, "style", data.get("style"), &ctx)?;
    process_packet_data(CzmlValueType::Number, label, "scale", data.get("scale"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, label, "showBackground", data.get("showBackground"), &ctx)?;
    process_packet_data(CzmlValueType::Color, label, "backgroundColor", data.get("backgroundColor"), &ctx)?;
    process_packet_data(CzmlValueType::Cartesian2, label, "backgroundPadding", data.get("backgroundPadding"), &ctx)?;
    process_packet_data(CzmlValueType::Cartesian2, label, "pixelOffset", data.get("pixelOffset"), &ctx)?;
    process_packet_data(CzmlValueType::Cartesian3, label, "eyeOffset", data.get("eyeOffset"), &ctx)?;
    process_packet_data(
        CzmlValueType::HorizontalOrigin,
        label,
        "horizontalOrigin",
        data.get("horizontalOrigin"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::VerticalOrigin, label, "verticalOrigin", data.get("verticalOrigin"), &ctx)?;
    process_packet_data(CzmlValueType::HeightReference, label, "heightReference", data.get("heightReference"), &ctx)?;
    process_packet_data(CzmlValueType::Color, label, "fillColor", data.get("fillColor"), &ctx)?;
    process_packet_data(CzmlValueType::Color, label, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, label, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(
        CzmlValueType::NearFarScalar,
        label,
        "translucencyByDistance",
        data.get("translucencyByDistance"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::NearFarScalar,
        label,
        "pixelOffsetScaleByDistance",
        data.get("pixelOffsetScaleByDistance"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::NearFarScalar, label, "scaleByDistance", data.get("scaleByDistance"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        label,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::Number,
        label,
        "disableDepthTestDistance",
        data.get("disableDepthTestDistance"),
        &ctx,
    )?;
    Ok(())
}

fn process_node_transformations(
    model: &ModelGraphics,
    data: &Value,
    constrained: Option<&TimeInterval<()>>,
    source_uri: Option<&Resource>,
    entity_collection: &Rc<EntityCollection>,
) -> Result<(), RuntimeError> {
    let Some(obj) = data.as_object() else { return Ok(()) };
    let combined = combine_intervals(get_defined(data, "interval").and_then(Value::as_str), constrained)?;
    let ctx = PacketContext { interval: combined.as_ref(), source_uri, entity_collection };

    let ordered = js_object_order(obj.iter().map(|(k, v)| (k.clone(), v)).collect());
    for (node_name, node_data) in ordered {
        if node_name == "interval" || node_data.is_null() {
            continue;
        }
        let bag_comp: ComponentRef = match model.node_transformations() {
            Some(PropertyInput::Component(c)) if c.as_any().is::<PropertyBag>() => c,
            _ => {
                let b: ComponentRef = Rc::new(PropertyBag::new());
                model.set_node_transformations(Some(PropertyInput::Component(b.clone())))?;
                b
            }
        };
        let bag = bag_comp.as_any().downcast_ref::<PropertyBag>().unwrap();
        if !bag.has_property(&node_name) {
            bag.add_property(node_name.clone(), None, None)?;
        }
        let nt_comp: ComponentRef = match bag.get(&node_name) {
            Some(PropertyInput::Component(c)) if c.as_any().is::<NodeTransformationProperty>() => c,
            _ => {
                let nt: ComponentRef = Rc::new(NodeTransformationProperty::new());
                bag.set(&node_name, Some(PropertyInput::Component(nt.clone())))?;
                nt
            }
        };
        let nt = nt_comp.as_any().downcast_ref::<NodeTransformationProperty>().unwrap();
        process_packet_data(CzmlValueType::Cartesian3, nt, "translation", node_data.get("translation"), &ctx)?;
        process_packet_data(CzmlValueType::Quaternion, nt, "rotation", node_data.get("rotation"), &ctx)?;
        process_packet_data(CzmlValueType::Cartesian3, nt, "scale", node_data.get("scale"), &ctx)?;
    }
    Ok(())
}

fn process_articulations(
    model: &ModelGraphics,
    data: &Value,
    constrained: Option<&TimeInterval<()>>,
    source_uri: Option<&Resource>,
    entity_collection: &Rc<EntityCollection>,
) -> Result<(), RuntimeError> {
    let Some(obj) = data.as_object() else { return Ok(()) };
    let combined = combine_intervals(get_defined(data, "interval").and_then(Value::as_str), constrained)?;
    let ctx = PacketContext { interval: combined.as_ref(), source_uri, entity_collection };

    let ordered = js_object_order(obj.iter().map(|(k, v)| (k.clone(), v)).collect());
    for (key, stage_data) in ordered {
        if key == "interval" || stage_data.is_null() {
            continue;
        }
        let bag_comp: ComponentRef = match model.articulations() {
            Some(PropertyInput::Component(c)) if c.as_any().is::<PropertyBag>() => c,
            _ => {
                let b: ComponentRef = Rc::new(PropertyBag::new());
                model.set_articulations(Some(PropertyInput::Component(b.clone())))?;
                b
            }
        };
        let bag = bag_comp.as_any().downcast_ref::<PropertyBag>().unwrap();
        if !bag.has_property(&key) {
            bag.add_property(key.clone(), None, None)?;
        }
        process_packet_data(CzmlValueType::Number, bag, &key, Some(stage_data), &ctx)?;
    }
    Ok(())
}

fn process_model(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "model") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<ModelGraphics>(entity, "model")?;
    let model = comp.as_any().downcast_ref::<ModelGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, model, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::Uri, model, "uri", data.get("gltf"), &ctx)?;
    process_packet_data(CzmlValueType::Number, model, "scale", data.get("scale"), &ctx)?;
    process_packet_data(CzmlValueType::Number, model, "minimumPixelSize", data.get("minimumPixelSize"), &ctx)?;
    process_packet_data(CzmlValueType::Number, model, "maximumScale", data.get("maximumScale"), &ctx)?;
    process_packet_data(
        CzmlValueType::Boolean,
        model,
        "incrementallyLoadTextures",
        data.get("incrementallyLoadTextures"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Boolean, model, "runAnimations", data.get("runAnimations"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, model, "clampAnimations", data.get("clampAnimations"), &ctx)?;
    process_packet_data(CzmlValueType::ShadowMode, model, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(CzmlValueType::HeightReference, model, "heightReference", data.get("heightReference"), &ctx)?;
    process_packet_data(CzmlValueType::Color, model, "silhouetteColor", data.get("silhouetteColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, model, "silhouetteSize", data.get("silhouetteSize"), &ctx)?;
    process_packet_data(CzmlValueType::Color, model, "color", data.get("color"), &ctx)?;
    process_packet_data(CzmlValueType::ColorBlendMode, model, "colorBlendMode", data.get("colorBlendMode"), &ctx)?;
    process_packet_data(CzmlValueType::Number, model, "colorBlendAmount", data.get("colorBlendAmount"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        model,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;

    if let Some(nt_data) = get_defined(data, "nodeTransformations") {
        if let Some(arr) = nt_data.as_array() {
            for item in arr {
                process_node_transformations(model, item, interval.as_ref(), source_uri, entity_collection)?;
            }
        } else {
            process_node_transformations(model, nt_data, interval.as_ref(), source_uri, entity_collection)?;
        }
    }

    if let Some(art_data) = get_defined(data, "articulations") {
        if let Some(arr) = art_data.as_array() {
            for item in arr {
                process_articulations(model, item, interval.as_ref(), source_uri, entity_collection)?;
            }
        } else {
            process_articulations(model, art_data, interval.as_ref(), source_uri, entity_collection)?;
        }
    }
    Ok(())
}

fn process_path(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "path") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<PathGraphics>(entity, "path")?;
    let path = comp.as_any().downcast_ref::<PathGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, path, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::Number, path, "leadTime", data.get("leadTime"), &ctx)?;
    process_packet_data(CzmlValueType::Number, path, "trailTime", data.get("trailTime"), &ctx)?;
    process_packet_data(CzmlValueType::Number, path, "width", data.get("width"), &ctx)?;
    process_packet_data(CzmlValueType::Number, path, "resolution", data.get("resolution"), &ctx)?;
    process_material_packet_data(path, "material", data.get("material"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        path,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    Ok(())
}

fn process_point(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "point") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<PointGraphics>(entity, "point")?;
    let point = comp.as_any().downcast_ref::<PointGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, point, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::Number, point, "pixelSize", data.get("pixelSize"), &ctx)?;
    process_packet_data(CzmlValueType::HeightReference, point, "heightReference", data.get("heightReference"), &ctx)?;
    process_packet_data(CzmlValueType::Color, point, "color", data.get("color"), &ctx)?;
    process_packet_data(CzmlValueType::Color, point, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, point, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(CzmlValueType::NearFarScalar, point, "scaleByDistance", data.get("scaleByDistance"), &ctx)?;
    process_packet_data(
        CzmlValueType::NearFarScalar,
        point,
        "translucencyByDistance",
        data.get("translucencyByDistance"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        point,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::Number,
        point,
        "disableDepthTestDistance",
        data.get("disableDepthTestDistance"),
        &ctx,
    )?;
    Ok(())
}

fn process_polygon(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "polygon") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<PolygonGraphics>(entity, "polygon")?;
    let polygon = comp.as_any().downcast_ref::<PolygonGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, polygon, "show", data.get("show"), &ctx)?;

    // Maintain `_positions` and `_holes` inside CzmlPolygonHierarchyProperty
    // across incremental `process()` calls.
    let existing_hierarchy = match polygon.hierarchy() {
        Some(PropertyInput::Component(c)) if c.as_any().is::<CzmlPolygonHierarchyProperty>() => Some(c),
        _ => None,
    };
    let hierarchy_comp: ComponentRef =
        existing_hierarchy.clone().unwrap_or_else(|| Rc::new(CzmlPolygonHierarchyProperty::new()));
    let hierarchy = hierarchy_comp.as_any().downcast_ref::<CzmlPolygonHierarchyProperty>().unwrap();

    if is_defined(data.get("positions")) {
        let pos_slot = SlotTarget::new(hierarchy.positions_slot());
        process_position_array(&pos_slot, "_positions", data.get("positions"), entity_collection)?;
        hierarchy.set_positions_slot(pos_slot.into_inner());
    }
    if is_defined(data.get("holes")) {
        let holes_slot = SlotTarget::new(hierarchy.holes_slot());
        process_position_array_of_arrays(&holes_slot, "_holes", data.get("holes"), entity_collection)?;
        hierarchy.set_holes_slot(holes_slot.into_inner());
    }
    if hierarchy.has_any() && existing_hierarchy.is_none() {
        polygon.set_hierarchy(Some(PropertyInput::Component(hierarchy_comp)))?;
    }

    process_packet_data(CzmlValueType::Number, polygon, "height", data.get("height"), &ctx)?;
    process_packet_data(CzmlValueType::HeightReference, polygon, "heightReference", data.get("heightReference"), &ctx)?;
    process_packet_data(CzmlValueType::Number, polygon, "extrudedHeight", data.get("extrudedHeight"), &ctx)?;
    process_packet_data(
        CzmlValueType::HeightReference,
        polygon,
        "extrudedHeightReference",
        data.get("extrudedHeightReference"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Rotation, polygon, "stRotation", data.get("stRotation"), &ctx)?;
    process_packet_data(CzmlValueType::Number, polygon, "granularity", data.get("granularity"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, polygon, "fill", data.get("fill"), &ctx)?;
    process_material_packet_data(polygon, "material", data.get("material"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, polygon, "outline", data.get("outline"), &ctx)?;
    process_packet_data(CzmlValueType::Color, polygon, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, polygon, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, polygon, "perPositionHeight", data.get("perPositionHeight"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, polygon, "closeTop", data.get("closeTop"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, polygon, "closeBottom", data.get("closeBottom"), &ctx)?;
    process_packet_data(CzmlValueType::ArcType, polygon, "arcType", data.get("arcType"), &ctx)?;
    process_packet_data(CzmlValueType::ShadowMode, polygon, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        polygon,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::ClassificationType,
        polygon,
        "classificationType",
        data.get("classificationType"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Number, polygon, "zIndex", data.get("zIndex"), &ctx)?;
    Ok(())
}

fn process_polyline(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "polyline") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<PolylineGraphics>(entity, "polyline")?;
    let polyline = comp.as_any().downcast_ref::<PolylineGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, polyline, "show", data.get("show"), &ctx)?;
    process_position_array(polyline, "positions", data.get("positions"), entity_collection)?;
    process_packet_data(CzmlValueType::Number, polyline, "width", data.get("width"), &ctx)?;
    process_packet_data(CzmlValueType::Number, polyline, "granularity", data.get("granularity"), &ctx)?;
    process_material_packet_data(polyline, "material", data.get("material"), &ctx)?;
    process_material_packet_data(polyline, "depthFailMaterial", data.get("depthFailMaterial"), &ctx)?;
    process_packet_data(CzmlValueType::ArcType, polyline, "arcType", data.get("arcType"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, polyline, "clampToGround", data.get("clampToGround"), &ctx)?;
    process_packet_data(CzmlValueType::ShadowMode, polyline, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        polyline,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::ClassificationType,
        polyline,
        "classificationType",
        data.get("classificationType"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Number, polyline, "zIndex", data.get("zIndex"), &ctx)?;

    // Backwards compatibility: adapt CZML `followSurface` boolean to `arcType`.
    if is_defined(data.get("followSurface")) && !is_defined(data.get("arcType")) {
        let slot = SlotTarget::default();
        process_packet_data(CzmlValueType::Boolean, &slot, "followSurface", data.get("followSurface"), &ctx)?;
        if let Some(PropertyInput::Component(follow_prop)) = slot.into_inner() {
            let is_const = follow_prop.is_constant();
            let adapter: PropertyRef<DynamicValue> = Rc::new(CallbackProperty::new(
                Rc::new(move |time, result| {
                    let val = follow_prop.get_value(time, None)?;
                    let arc = if matches!(val, Some(DynamicValue::Boolean(true))) {
                        ArcType::Geodesic as i32 as f64
                    } else {
                        ArcType::None as i32 as f64
                    };
                    let out = DynamicValue::Number(arc);
                    if let Some(r) = result {
                        *r = out.clone();
                    }
                    Ok(Some(out))
                }),
                is_const,
            ));
            polyline.set_arc_type(Some(PropertyInput::Component(PropertyComponent::wrap(adapter))))?;
        }
    }
    Ok(())
}

fn process_polyline_volume(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "polylineVolume") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<PolylineVolumeGraphics>(entity, "polylineVolume")?;
    let pv = comp.as_any().downcast_ref::<PolylineVolumeGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_position_array(pv, "positions", data.get("positions"), entity_collection)?;
    process_shape(pv, "shape", data.get("shape"), entity_collection)?;
    process_packet_data(CzmlValueType::Boolean, pv, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::CornerType, pv, "cornerType", data.get("cornerType"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, pv, "fill", data.get("fill"), &ctx)?;
    process_material_packet_data(pv, "material", data.get("material"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, pv, "outline", data.get("outline"), &ctx)?;
    process_packet_data(CzmlValueType::Color, pv, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, pv, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(CzmlValueType::Number, pv, "granularity", data.get("granularity"), &ctx)?;
    process_packet_data(CzmlValueType::ShadowMode, pv, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        pv,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    Ok(())
}

fn process_rectangle(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "rectangle") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<RectangleGraphics>(entity, "rectangle")?;
    let rect = comp.as_any().downcast_ref::<RectangleGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, rect, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::Rectangle, rect, "coordinates", data.get("coordinates"), &ctx)?;
    process_packet_data(CzmlValueType::Number, rect, "height", data.get("height"), &ctx)?;
    process_packet_data(CzmlValueType::HeightReference, rect, "heightReference", data.get("heightReference"), &ctx)?;
    process_packet_data(CzmlValueType::Number, rect, "extrudedHeight", data.get("extrudedHeight"), &ctx)?;
    process_packet_data(
        CzmlValueType::HeightReference,
        rect,
        "extrudedHeightReference",
        data.get("extrudedHeightReference"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Rotation, rect, "rotation", data.get("rotation"), &ctx)?;
    process_packet_data(CzmlValueType::Rotation, rect, "stRotation", data.get("stRotation"), &ctx)?;
    process_packet_data(CzmlValueType::Number, rect, "granularity", data.get("granularity"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, rect, "fill", data.get("fill"), &ctx)?;
    process_material_packet_data(rect, "material", data.get("material"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, rect, "outline", data.get("outline"), &ctx)?;
    process_packet_data(CzmlValueType::Color, rect, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, rect, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(CzmlValueType::ShadowMode, rect, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        rect,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    process_packet_data(
        CzmlValueType::ClassificationType,
        rect,
        "classificationType",
        data.get("classificationType"),
        &ctx,
    )?;
    process_packet_data(CzmlValueType::Number, rect, "zIndex", data.get("zIndex"), &ctx)?;
    Ok(())
}

fn process_tileset(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "tileset") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<Cesium3DTilesetGraphics>(entity, "tileset")?;
    let tileset = comp.as_any().downcast_ref::<Cesium3DTilesetGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, tileset, "show", data.get("show"), &ctx)?;
    process_packet_data(CzmlValueType::Uri, tileset, "uri", data.get("uri"), &ctx)?;
    process_packet_data(
        CzmlValueType::Number,
        tileset,
        "maximumScreenSpaceError",
        data.get("maximumScreenSpaceError"),
        &ctx,
    )?;
    Ok(())
}

fn process_wall(
    entity: &EntityRef,
    packet: &Value,
    entity_collection: &Rc<EntityCollection>,
    source_uri: Option<&Resource>,
) -> Result<(), RuntimeError> {
    let Some(data) = get_defined(packet, "wall") else { return Ok(()) };
    let interval = interval_from_string(get_defined(data, "interval").and_then(Value::as_str))?;
    let comp = get_or_create_graphics::<WallGraphics>(entity, "wall")?;
    let wall = comp.as_any().downcast_ref::<WallGraphics>().unwrap();
    let ctx = PacketContext { interval: interval.as_ref(), source_uri, entity_collection };

    process_packet_data(CzmlValueType::Boolean, wall, "show", data.get("show"), &ctx)?;
    process_position_array(wall, "positions", data.get("positions"), entity_collection)?;
    process_array(wall, "minimumHeights", data.get("minimumHeights"), entity_collection)?;
    process_array(wall, "maximumHeights", data.get("maximumHeights"), entity_collection)?;
    process_packet_data(CzmlValueType::Number, wall, "granularity", data.get("granularity"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, wall, "fill", data.get("fill"), &ctx)?;
    process_material_packet_data(wall, "material", data.get("material"), &ctx)?;
    process_packet_data(CzmlValueType::Boolean, wall, "outline", data.get("outline"), &ctx)?;
    process_packet_data(CzmlValueType::Color, wall, "outlineColor", data.get("outlineColor"), &ctx)?;
    process_packet_data(CzmlValueType::Number, wall, "outlineWidth", data.get("outlineWidth"), &ctx)?;
    process_packet_data(CzmlValueType::ShadowMode, wall, "shadows", data.get("shadows"), &ctx)?;
    process_packet_data(
        CzmlValueType::DistanceDisplayCondition,
        wall,
        "distanceDisplayCondition",
        data.get("distanceDisplayCondition"),
        &ctx,
    )?;
    Ok(())
}

/// Returns the 23 built-in CZML packet updaters in the exact CesiumJS order.
pub(crate) fn builtin_updaters() -> Vec<CzmlUpdater> {
    vec![
        CzmlUpdater::new(process_billboard),
        CzmlUpdater::new(process_box),
        CzmlUpdater::new(process_corridor),
        CzmlUpdater::new(process_cylinder),
        CzmlUpdater::new(process_ellipse),
        CzmlUpdater::new(process_ellipsoid),
        CzmlUpdater::new(process_label),
        CzmlUpdater::new(process_model),
        CzmlUpdater::new(process_name),
        CzmlUpdater::new(process_description),
        CzmlUpdater::new(process_path),
        CzmlUpdater::new(process_point),
        CzmlUpdater::new(process_polygon),
        CzmlUpdater::new(process_polyline),
        CzmlUpdater::new(process_polyline_volume),
        CzmlUpdater::new(process_properties),
        CzmlUpdater::new(process_rectangle),
        CzmlUpdater::new(process_position),
        CzmlUpdater::new(process_tileset),
        CzmlUpdater::new(process_view_from),
        CzmlUpdater::new(process_wall),
        CzmlUpdater::new(process_orientation),
        CzmlUpdater::new(process_availability),
    ]
}
