//! Generic CZML packet property processing (`processPacketData`,
//! `processProperty`, arrays, position arrays, shapes, references, deletions).
use super::{
    adapters::{
        bounding_rect_to_dynamic, cartesian2_to_dynamic, cartesian3_to_dynamic, color_to_dynamic,
        component_to_dynamic_property, distance_display_to_dynamic, f64_to_dynamic, near_far_to_dynamic,
        quaternion_to_dynamic, rectangle_to_dynamic, rotation_to_dynamic, CzmlPropertyTarget, CzmlSampledAdapter,
        HolesReferencesProperty, PositionArrayDynamicAdapter, PropertyArrayDynamicAdapter, SlotTarget,
        VelocityOrientationDynamicAdapter, VelocityVectorDynamicAdapter,
    },
    current_packet_id,
    unwrap::{
        combine_intervals, get_defined, interval_from_string, is_defined, unpack_cartesian2_values,
        unpack_cartesian3_values, unpack_cartographic_degrees_values, unpack_cartographic_radians_values,
        unwrap_interval, update_interpolation_settings, UnwrappedInterval,
    },
    CzmlValueType,
};
use crate::{
    core::{
        iso8601, BoundingRectangle, Cartesian2, Cartesian3, Color, DistanceDisplayCondition, JulianDate, NearFarScalar,
        Quaternion, Rectangle, ReferenceFrame, Resource, RuntimeError, TimeInterval,
    },
    data_sources::{
        entity_collection::EntitySource, CompositeProperty, ConstantProperty, DynamicValue, EntityCollection,
        JsonReference, PackedSampleValue, PositionPropertyArray, PositionPropertyRef, Property, PropertyArray,
        PropertyComponent, PropertyInput, PropertyRef, ReferenceProperty, Rotation, SampledValue,
        TimeIntervalCollectionProperty, VelocityOrientationProperty, VelocityVectorProperty,
    },
};
use serde_json::{json, Value};
use std::rc::Rc;

/// Shared context passed through packet data processors.
pub(crate) struct PacketContext<'a> {
    pub(crate) interval: Option<&'a TimeInterval<()>>,
    pub(crate) source_uri: Option<&'a Resource>,
    pub(crate) entity_collection: &'a Rc<EntityCollection>,
}

pub(crate) fn create_reference_property(
    entity_collection: &Rc<EntityCollection>,
    reference_string: &str,
) -> Result<Rc<ReferenceProperty>, RuntimeError> {
    let resolved = if reference_string.starts_with('#') {
        if let Some(id) = current_packet_id() {
            format!("{id}{reference_string}")
        } else {
            reference_string.to_owned()
        }
    } else {
        reference_string.to_owned()
    };
    let source: Rc<dyn EntitySource> = entity_collection.clone();
    ReferenceProperty::from_string(source, &resolved)
}

fn create_specialized_property(
    ty: CzmlValueType,
    entity_collection: &Rc<EntityCollection>,
    packet_data: &Value,
) -> Result<PropertyInput, RuntimeError> {
    if let Some(ref_str) = get_defined(packet_data, "reference").and_then(Value::as_str) {
        let prop = create_reference_property(entity_collection, ref_str)?;
        return Ok(PropertyInput::Component(prop));
    }

    if let Some(vel_str) = get_defined(packet_data, "velocityReference").and_then(Value::as_str) {
        let ref_prop = create_reference_property(entity_collection, vel_str)?;
        let pos_ref: PropertyRef<Cartesian3> = ref_prop.as_position();
        match ty {
            CzmlValueType::Cartesian3 | CzmlValueType::UnitCartesian3 => {
                let normalize = ty == CzmlValueType::UnitCartesian3;
                let vel = Rc::new(VelocityVectorProperty::new(Some(pos_ref), normalize));
                let adapter: PropertyRef<DynamicValue> = Rc::new(VelocityVectorDynamicAdapter(vel));
                return Ok(PropertyInput::Component(PropertyComponent::wrap(adapter)));
            }
            CzmlValueType::Quaternion => {
                let orient = Rc::new(VelocityOrientationProperty::new(Some(pos_ref), None));
                let adapter: PropertyRef<DynamicValue> = Rc::new(VelocityOrientationDynamicAdapter(orient));
                return Ok(PropertyInput::Component(PropertyComponent::wrap(adapter)));
            }
            _ => {}
        }
    }

    Err(RuntimeError::new(format!("{packet_data} is not valid CZML.")))
}

fn wrap_property_in_infinite_interval(property: PropertyRef<DynamicValue>) -> TimeInterval<PropertyRef<DynamicValue>> {
    let max = iso8601::maximum_interval::<()>();
    TimeInterval::new(max.start, max.stop, max.is_start_included, max.is_stop_included, Some(property))
}

fn convert_property_to_composite(existing: Option<PropertyInput>) -> Rc<CompositeProperty<DynamicValue>> {
    let composite = Rc::new(CompositeProperty::new());
    if let Some(PropertyInput::Component(c)) = existing {
        let prop = component_to_dynamic_property(c);
        composite.intervals().add_interval(wrap_property_in_infinite_interval(prop));
    }
    composite
}

fn existing_dynamic_property(input: Option<PropertyInput>) -> Option<PropertyRef<DynamicValue>> {
    match input {
        Some(PropertyInput::Component(c)) => Some(component_to_dynamic_property(c)),
        _ => None,
    }
}

fn new_sampled_adapter(ty: CzmlValueType) -> Result<PropertyRef<DynamicValue>, RuntimeError> {
    Ok(match ty {
        CzmlValueType::Number => Rc::new(CzmlSampledAdapter::<f64>::new(f64_to_dynamic)),
        CzmlValueType::Rotation => Rc::new(CzmlSampledAdapter::<Rotation>::new(rotation_to_dynamic)),
        CzmlValueType::Cartesian2 => Rc::new(CzmlSampledAdapter::<Cartesian2>::new(cartesian2_to_dynamic)),
        CzmlValueType::Cartesian3 | CzmlValueType::UnitCartesian3 => {
            Rc::new(CzmlSampledAdapter::<Cartesian3>::new(cartesian3_to_dynamic))
        }
        CzmlValueType::Quaternion => Rc::new(CzmlSampledAdapter::<Quaternion>::new(quaternion_to_dynamic)),
        CzmlValueType::Color => Rc::new(CzmlSampledAdapter::<Color>::new(color_to_dynamic)),
        CzmlValueType::Rectangle => Rc::new(CzmlSampledAdapter::<Rectangle>::new(rectangle_to_dynamic)),
        CzmlValueType::BoundingRectangle => {
            Rc::new(CzmlSampledAdapter::<BoundingRectangle>::new(bounding_rect_to_dynamic))
        }
        CzmlValueType::NearFarScalar => Rc::new(CzmlSampledAdapter::<NearFarScalar>::new(near_far_to_dynamic)),
        CzmlValueType::DistanceDisplayCondition => {
            Rc::new(CzmlSampledAdapter::<DistanceDisplayCondition>::new(distance_display_to_dynamic))
        }
        _ => return Err(RuntimeError::new(format!("Type {ty:?} does not support sampled CZML values."))),
    })
}

fn is_matching_sampled(ty: CzmlValueType, prop: &dyn Property<DynamicValue>) -> bool {
    let any = prop.as_any();
    match ty {
        CzmlValueType::Number => any.is::<CzmlSampledAdapter<f64>>(),
        CzmlValueType::Rotation => any.is::<CzmlSampledAdapter<Rotation>>(),
        CzmlValueType::Cartesian2 => any.is::<CzmlSampledAdapter<Cartesian2>>(),
        CzmlValueType::Cartesian3 | CzmlValueType::UnitCartesian3 => any.is::<CzmlSampledAdapter<Cartesian3>>(),
        CzmlValueType::Quaternion => any.is::<CzmlSampledAdapter<Quaternion>>(),
        CzmlValueType::Color => any.is::<CzmlSampledAdapter<Color>>(),
        CzmlValueType::Rectangle => any.is::<CzmlSampledAdapter<Rectangle>>(),
        CzmlValueType::BoundingRectangle => any.is::<CzmlSampledAdapter<BoundingRectangle>>(),
        CzmlValueType::NearFarScalar => any.is::<CzmlSampledAdapter<NearFarScalar>>(),
        CzmlValueType::DistanceDisplayCondition => any.is::<CzmlSampledAdapter<DistanceDisplayCondition>>(),
        _ => false,
    }
}

fn add_samples_to_adapter(
    ty: CzmlValueType,
    prop: &dyn Property<DynamicValue>,
    samples: &[PackedSampleValue],
    epoch: Option<JulianDate>,
    packet_data: &Value,
) -> Result<(), RuntimeError> {
    fn feed<T: SampledValue>(
        prop: &dyn Property<DynamicValue>,
        samples: &[PackedSampleValue],
        epoch: Option<JulianDate>,
        packet_data: &Value,
    ) -> Result<(), RuntimeError> {
        let adapter = prop
            .as_any()
            .downcast_ref::<CzmlSampledAdapter<T>>()
            .ok_or_else(|| RuntimeError::new("SampledProperty type mismatch."))?;
        adapter.inner().add_samples_packed_array(samples, epoch)?;
        update_interpolation_settings(packet_data, adapter.inner());
        Ok(())
    }

    match ty {
        CzmlValueType::Number => feed::<f64>(prop, samples, epoch, packet_data),
        CzmlValueType::Rotation => feed::<Rotation>(prop, samples, epoch, packet_data),
        CzmlValueType::Cartesian2 => feed::<Cartesian2>(prop, samples, epoch, packet_data),
        CzmlValueType::Cartesian3 | CzmlValueType::UnitCartesian3 => {
            feed::<Cartesian3>(prop, samples, epoch, packet_data)
        }
        CzmlValueType::Quaternion => feed::<Quaternion>(prop, samples, epoch, packet_data),
        CzmlValueType::Color => feed::<Color>(prop, samples, epoch, packet_data),
        CzmlValueType::Rectangle => feed::<Rectangle>(prop, samples, epoch, packet_data),
        CzmlValueType::BoundingRectangle => feed::<BoundingRectangle>(prop, samples, epoch, packet_data),
        CzmlValueType::NearFarScalar => feed::<NearFarScalar>(prop, samples, epoch, packet_data),
        CzmlValueType::DistanceDisplayCondition => feed::<DistanceDisplayCondition>(prop, samples, epoch, packet_data),
        _ => Err(RuntimeError::new(format!("Type {ty:?} does not support sampled CZML values."))),
    }
}

fn try_remove_sampled<T: SampledValue>(prop: &dyn Property<DynamicValue>, interval: &TimeInterval<()>) -> bool {
    if let Some(adapter) = prop.as_any().downcast_ref::<CzmlSampledAdapter<T>>() {
        adapter.inner().remove_samples(interval);
        true
    } else {
        false
    }
}

pub(crate) fn remove_property_data(prop: &dyn Property<DynamicValue>, interval: &TimeInterval<()>) {
    if try_remove_sampled::<f64>(prop, interval)
        || try_remove_sampled::<Rotation>(prop, interval)
        || try_remove_sampled::<Cartesian2>(prop, interval)
        || try_remove_sampled::<Cartesian3>(prop, interval)
        || try_remove_sampled::<Quaternion>(prop, interval)
        || try_remove_sampled::<Color>(prop, interval)
        || try_remove_sampled::<Rectangle>(prop, interval)
        || try_remove_sampled::<BoundingRectangle>(prop, interval)
        || try_remove_sampled::<NearFarScalar>(prop, interval)
        || try_remove_sampled::<DistanceDisplayCondition>(prop, interval)
    {
        return;
    }
    if let Some(ticp) = prop.as_any().downcast_ref::<TimeIntervalCollectionProperty<DynamicValue>>() {
        let rm = TimeInterval::new(
            interval.start,
            interval.stop,
            interval.is_start_included,
            interval.is_stop_included,
            None,
        );
        ticp.intervals().remove_interval(&rm);
        return;
    }
    if let Some(composite) = prop.as_any().downcast_ref::<CompositeProperty<DynamicValue>>() {
        let snapshot = composite.intervals().read().intervals().to_vec();
        let probe = TimeInterval::new(
            interval.start,
            interval.stop,
            interval.is_start_included,
            interval.is_stop_included,
            None,
        );
        for iv in &snapshot {
            let intersection = iv.intersect(&probe, None);
            if !intersection.is_empty() {
                if let Some(inner) = &intersection.data {
                    remove_property_data(&**inner, interval);
                }
            }
        }
        composite.intervals().remove_interval(&probe);
    }
}

pub(crate) fn process_property(
    ty: CzmlValueType,
    target: &dyn CzmlPropertyTarget,
    property_name: &str,
    packet_data: &Value,
    ctx: &PacketContext<'_>,
) -> Result<(), RuntimeError> {
    let combined_interval =
        combine_intervals(get_defined(packet_data, "interval").and_then(Value::as_str), ctx.interval)?;

    let max_interval = iso8601::maximum_interval::<()>();
    let has_interval = combined_interval.as_ref().is_some_and(|ci| !ci.equals(&max_interval));

    if get_defined(packet_data, "delete") == Some(&Value::Bool(true)) {
        if !has_interval {
            target.set_czml_property(property_name, None)?;
            return Ok(());
        }
        if let Some(prop) = existing_dynamic_property(target.get_czml_property(property_name)) {
            remove_property_data(&*prop, combined_interval.as_ref().unwrap());
        }
        return Ok(());
    }

    let is_value = !is_defined(packet_data.get("reference")) && !is_defined(packet_data.get("velocityReference"));

    let unwrapped = if is_value {
        match unwrap_interval(ty, packet_data, ctx.source_uri)? {
            UnwrappedInterval::None => return Ok(()),
            other => Some(other),
        }
    } else {
        None
    };

    let is_sampled = matches!(unwrapped, Some(UnwrappedInterval::Sampled(_)));

    // Constant value without an interval replaces anything previously stored.
    if !is_sampled && !has_interval {
        if is_value {
            let Some(UnwrappedInterval::Constant(val)) = unwrapped else { return Ok(()) };
            let prop: PropertyRef<DynamicValue> = Rc::new(ConstantProperty::new(Some(val)));
            target.set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(prop))))?;
        } else {
            let specialized = create_specialized_property(ty, ctx.entity_collection, packet_data)?;
            target.set_czml_property(property_name, Some(specialized))?;
        }
        return Ok(());
    }

    let epoch = match get_defined(packet_data, "epoch").and_then(Value::as_str) {
        Some(s) => Some(JulianDate::from_iso8601(s).map_err(|e| RuntimeError::new(e.to_string()))?),
        None => None,
    };

    // Sampled value without an interval replaces any non-matching property.
    if is_sampled && !has_interval {
        let Some(UnwrappedInterval::Sampled(samples)) = unwrapped else { return Ok(()) };
        let existing = existing_dynamic_property(target.get_czml_property(property_name));
        let prop = match existing {
            Some(p) if is_matching_sampled(ty, &*p) => p,
            _ => {
                let p = new_sampled_adapter(ty)?;
                target.set_czml_property(
                    property_name,
                    Some(PropertyInput::Component(PropertyComponent::wrap(p.clone()))),
                )?;
                p
            }
        };
        add_samples_to_adapter(ty, &*prop, &samples, epoch, packet_data)?;
        return Ok(());
    }

    let ci = combined_interval.unwrap();

    // Constant (or specialized) value with an interval.
    if !is_sampled && has_interval {
        let existing_input = target.get_czml_property(property_name);
        let existing = existing_dynamic_property(existing_input.clone());

        if is_value {
            let Some(UnwrappedInterval::Constant(val)) = unwrapped else { return Ok(()) };
            if existing.is_none() {
                let ticp = Rc::new(TimeIntervalCollectionProperty::<DynamicValue>::new());
                ticp.intervals().add_interval(TimeInterval::new(
                    ci.start,
                    ci.stop,
                    ci.is_start_included,
                    ci.is_stop_included,
                    Some(Rc::new(val)),
                ));
                let prop: PropertyRef<DynamicValue> = ticp;
                target
                    .set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(prop))))?;
                return Ok(());
            }
            let existing_prop = existing.unwrap();
            if let Some(ticp) = existing_prop.as_any().downcast_ref::<TimeIntervalCollectionProperty<DynamicValue>>() {
                ticp.intervals().add_interval(TimeInterval::new(
                    ci.start,
                    ci.stop,
                    ci.is_start_included,
                    ci.is_stop_included,
                    Some(Rc::new(val)),
                ));
            } else if let Some(composite) = existing_prop.as_any().downcast_ref::<CompositeProperty<DynamicValue>>() {
                let constant: PropertyRef<DynamicValue> = Rc::new(ConstantProperty::new(Some(val)));
                composite.intervals().add_interval(TimeInterval::new(
                    ci.start,
                    ci.stop,
                    ci.is_start_included,
                    ci.is_stop_included,
                    Some(constant),
                ));
            } else {
                let composite = convert_property_to_composite(existing_input);
                let constant: PropertyRef<DynamicValue> = Rc::new(ConstantProperty::new(Some(val)));
                composite.intervals().add_interval(TimeInterval::new(
                    ci.start,
                    ci.stop,
                    ci.is_start_included,
                    ci.is_stop_included,
                    Some(constant),
                ));
                let prop: PropertyRef<DynamicValue> = composite;
                target
                    .set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(prop))))?;
            }
            return Ok(());
        } else {
            let specialized_input = create_specialized_property(ty, ctx.entity_collection, packet_data)?;
            let specialized_prop = existing_dynamic_property(Some(specialized_input))
                .ok_or_else(|| RuntimeError::new("Expected component for specialized CZML property."))?;

            let composite = if let Some(existing_prop) = existing {
                if let Some(c) = existing_prop.as_any().downcast_ref::<CompositeProperty<DynamicValue>>() {
                    c.intervals().add_interval(TimeInterval::new(
                        ci.start,
                        ci.stop,
                        ci.is_start_included,
                        ci.is_stop_included,
                        Some(specialized_prop),
                    ));
                    return Ok(());
                } else {
                    convert_property_to_composite(existing_input)
                }
            } else {
                Rc::new(CompositeProperty::new())
            };
            composite.intervals().add_interval(TimeInterval::new(
                ci.start,
                ci.stop,
                ci.is_start_included,
                ci.is_stop_included,
                Some(specialized_prop),
            ));
            let prop: PropertyRef<DynamicValue> = composite;
            target.set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(prop))))?;
            return Ok(());
        }
    }

    // Sampled value with an interval -> always CompositeProperty containing SampledProperty.
    let Some(UnwrappedInterval::Sampled(samples)) = unwrapped else { return Ok(()) };
    let existing_input = target.get_czml_property(property_name);
    let existing = existing_dynamic_property(existing_input.clone());

    let composite_prop: PropertyRef<DynamicValue> = match existing {
        Some(ref p) if p.as_any().is::<CompositeProperty<DynamicValue>>() => p.clone(),
        None => {
            let c: PropertyRef<DynamicValue> = Rc::new(CompositeProperty::new());
            target
                .set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(c.clone()))))?;
            c
        }
        Some(_) => {
            let c: PropertyRef<DynamicValue> = convert_property_to_composite(existing_input);
            target
                .set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(c.clone()))))?;
            c
        }
    };

    let composite = composite_prop.as_any().downcast_ref::<CompositeProperty<DynamicValue>>().unwrap();
    let existing_interval_data = composite
        .intervals()
        .read()
        .find_interval(Some(&ci.start), Some(&ci.stop), Some(ci.is_start_included), Some(ci.is_stop_included))
        .and_then(|iv| iv.data.clone());

    let sampled_prop = match existing_interval_data {
        Some(p) if is_matching_sampled(ty, &*p) => p,
        _ => {
            let p = new_sampled_adapter(ty)?;
            composite.intervals().add_interval(TimeInterval::new(
                ci.start,
                ci.stop,
                ci.is_start_included,
                ci.is_stop_included,
                Some(p.clone()),
            ));
            p
        }
    };
    add_samples_to_adapter(ty, &*sampled_prop, &samples, epoch, packet_data)
}

pub(crate) fn process_packet_data(
    ty: CzmlValueType,
    target: &dyn CzmlPropertyTarget,
    property_name: &str,
    packet_data: Option<&Value>,
    ctx: &PacketContext<'_>,
) -> Result<(), RuntimeError> {
    let Some(packet_data) = packet_data.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    if let Some(arr) = packet_data.as_array() {
        for item in arr {
            process_property(ty, target, property_name, item, ctx)?;
        }
    } else {
        process_property(ty, target, property_name, packet_data, ctx)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Reference arrays, generic arrays, position arrays, shapes, hole arrays
// ---------------------------------------------------------------------------

fn process_references_array(
    target: &dyn CzmlPropertyTarget,
    property_name: &str,
    references: &[Value],
    interval_str: Option<&str>,
    entity_collection: &Rc<EntityCollection>,
    as_positions: bool,
) -> Result<(), RuntimeError> {
    let array_prop: PropertyRef<DynamicValue> = if as_positions {
        let mut props: Vec<PositionPropertyRef> = Vec::with_capacity(references.len());
        for item in references {
            let s = item.as_str().ok_or_else(|| RuntimeError::new("Reference array entries must be strings."))?;
            props.push(create_reference_property(entity_collection, s)?.as_position());
        }
        Rc::new(PositionArrayDynamicAdapter(Rc::new(PositionPropertyArray::new(Some(props), ReferenceFrame::Fixed))))
    } else {
        let mut props: Vec<PropertyRef<DynamicValue>> = Vec::with_capacity(references.len());
        for item in references {
            let s = item.as_str().ok_or_else(|| RuntimeError::new("Reference array entries must be strings."))?;
            let r: PropertyRef<DynamicValue> = create_reference_property(entity_collection, s)?;
            props.push(r);
        }
        Rc::new(PropertyArrayDynamicAdapter(Rc::new(PropertyArray::new(Some(props)))))
    };

    if let Some(iv) = interval_from_string(interval_str)? {
        let existing_input = target.get_czml_property(property_name);
        let existing = existing_dynamic_property(existing_input.clone());
        let composite_prop: PropertyRef<DynamicValue> = match existing {
            Some(ref p) if p.as_any().is::<CompositeProperty<DynamicValue>>() => p.clone(),
            _ => {
                let c: PropertyRef<DynamicValue> = convert_property_to_composite(existing_input);
                target.set_czml_property(
                    property_name,
                    Some(PropertyInput::Component(PropertyComponent::wrap(c.clone()))),
                )?;
                c
            }
        };
        let composite = composite_prop.as_any().downcast_ref::<CompositeProperty<DynamicValue>>().unwrap();
        composite.intervals().add_interval(TimeInterval::new(
            iv.start,
            iv.stop,
            iv.is_start_included,
            iv.is_stop_included,
            Some(array_prop),
        ));
    } else {
        target.set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(array_prop))))?;
    }
    Ok(())
}

/// Processes a constant (or interval-scoped) `DynamicValue` directly into a
/// property target, mirroring `processPacketData(Array, ...)` with pre-unpacked
/// array payloads (`positions`, `shape`, `holes`).
fn process_preunpacked_value(
    target: &dyn CzmlPropertyTarget,
    property_name: &str,
    packet_data: &Value,
    value: DynamicValue,
) -> Result<(), RuntimeError> {
    let combined = interval_from_string(get_defined(packet_data, "interval").and_then(Value::as_str))?;
    let max_interval = iso8601::maximum_interval::<()>();
    let has_interval = combined.as_ref().is_some_and(|ci| !ci.equals(&max_interval));

    if !has_interval {
        let prop: PropertyRef<DynamicValue> = Rc::new(ConstantProperty::new(Some(value)));
        target.set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(prop))))?;
        return Ok(());
    }

    let ci = combined.unwrap();
    let existing_input = target.get_czml_property(property_name);
    let existing = existing_dynamic_property(existing_input.clone());

    if existing.is_none() {
        let ticp = Rc::new(TimeIntervalCollectionProperty::<DynamicValue>::new());
        ticp.intervals().add_interval(TimeInterval::new(
            ci.start,
            ci.stop,
            ci.is_start_included,
            ci.is_stop_included,
            Some(Rc::new(value)),
        ));
        let prop: PropertyRef<DynamicValue> = ticp;
        target.set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(prop))))?;
        return Ok(());
    }

    let existing_prop = existing.unwrap();
    if let Some(ticp) = existing_prop.as_any().downcast_ref::<TimeIntervalCollectionProperty<DynamicValue>>() {
        ticp.intervals().add_interval(TimeInterval::new(
            ci.start,
            ci.stop,
            ci.is_start_included,
            ci.is_stop_included,
            Some(Rc::new(value)),
        ));
    } else if let Some(composite) = existing_prop.as_any().downcast_ref::<CompositeProperty<DynamicValue>>() {
        let constant: PropertyRef<DynamicValue> = Rc::new(ConstantProperty::new(Some(value)));
        composite.intervals().add_interval(TimeInterval::new(
            ci.start,
            ci.stop,
            ci.is_start_included,
            ci.is_stop_included,
            Some(constant),
        ));
    } else {
        let composite = convert_property_to_composite(existing_input);
        let constant: PropertyRef<DynamicValue> = Rc::new(ConstantProperty::new(Some(value)));
        composite.intervals().add_interval(TimeInterval::new(
            ci.start,
            ci.stop,
            ci.is_start_included,
            ci.is_stop_included,
            Some(constant),
        ));
        let prop: PropertyRef<DynamicValue> = composite;
        target.set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(prop))))?;
    }
    Ok(())
}

pub(crate) fn process_array(
    target: &dyn CzmlPropertyTarget,
    property_name: &str,
    packet_data: Option<&Value>,
    entity_collection: &Rc<EntityCollection>,
) -> Result<(), RuntimeError> {
    let Some(packet_data) = packet_data.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let items: Vec<&Value> = match packet_data.as_array() {
        Some(arr) => arr.iter().collect(),
        None => vec![packet_data],
    };
    let ctx = PacketContext { interval: None, source_uri: None, entity_collection };
    for item in items {
        if let Some(refs) = get_defined(item, "references").and_then(Value::as_array) {
            process_references_array(
                target,
                property_name,
                refs,
                get_defined(item, "interval").and_then(Value::as_str),
                entity_collection,
                false,
            )?;
        } else {
            process_property(CzmlValueType::Array, target, property_name, item, &ctx)?;
        }
    }
    Ok(())
}

pub(crate) fn process_position_array(
    target: &dyn CzmlPropertyTarget,
    property_name: &str,
    packet_data: Option<&Value>,
    entity_collection: &Rc<EntityCollection>,
) -> Result<(), RuntimeError> {
    let Some(packet_data) = packet_data.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let items: Vec<&Value> = match packet_data.as_array() {
        Some(arr) => arr.iter().collect(),
        None => vec![packet_data],
    };
    let ctx = PacketContext { interval: None, source_uri: None, entity_collection };
    for item in items {
        if get_defined(item, "delete") == Some(&Value::Bool(true)) {
            process_property(CzmlValueType::Array, target, property_name, item, &ctx)?;
            continue;
        }
        if let Some(refs) = get_defined(item, "references").and_then(Value::as_array) {
            process_references_array(
                target,
                property_name,
                refs,
                get_defined(item, "interval").and_then(Value::as_str),
                entity_collection,
                true,
            )?;
            continue;
        }
        let positions = if let Some(arr) = get_defined(item, "cartesian").and_then(Value::as_array) {
            Some(unpack_cartesian3_values(arr)?)
        } else if let Some(arr) = get_defined(item, "cartographicRadians").and_then(Value::as_array) {
            Some(unpack_cartographic_radians_values(arr)?)
        } else if let Some(arr) = get_defined(item, "cartographicDegrees").and_then(Value::as_array) {
            Some(unpack_cartographic_degrees_values(arr)?)
        } else {
            None
        };
        if let Some(positions) = positions {
            process_preunpacked_value(target, property_name, item, DynamicValue::Positions(Rc::new(positions)))?;
        }
    }
    Ok(())
}

pub(crate) fn process_position_array_of_arrays(
    target: &dyn CzmlPropertyTarget,
    property_name: &str,
    packet_data: Option<&Value>,
    entity_collection: &Rc<EntityCollection>,
) -> Result<(), RuntimeError> {
    let Some(packet_data) = packet_data.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let items: Vec<&Value> = match packet_data.as_array() {
        Some(arr) => arr.iter().collect(),
        None => vec![packet_data],
    };
    let ctx = PacketContext { interval: None, source_uri: None, entity_collection };
    for item in items {
        if get_defined(item, "delete") == Some(&Value::Bool(true)) {
            process_property(CzmlValueType::Array, target, property_name, item, &ctx)?;
            continue;
        }
        if let Some(refs_outer) = get_defined(item, "references").and_then(Value::as_array) {
            let interval_str = get_defined(item, "interval").and_then(Value::as_str);
            let mut hole_props: Vec<PropertyRef<DynamicValue>> = Vec::with_capacity(refs_outer.len());
            for inner in refs_outer {
                let inner_refs = inner
                    .as_array()
                    .ok_or_else(|| RuntimeError::new("Polygon hole references must be an array of arrays."))?;
                let slot = SlotTarget::default();
                process_references_array(&slot, "positions", inner_refs, interval_str, entity_collection, true)?;
                if let Some(p) = existing_dynamic_property(slot.into_inner()) {
                    hole_props.push(p);
                }
            }
            let prop: PropertyRef<DynamicValue> = Rc::new(HolesReferencesProperty::new(hole_props));
            target.set_czml_property(property_name, Some(PropertyInput::Component(PropertyComponent::wrap(prop))))?;
            continue;
        }
        let holes: Option<Vec<Vec<Cartesian3>>> =
            if let Some(outer) = get_defined(item, "cartesian").and_then(Value::as_array) {
                let mut res = Vec::with_capacity(outer.len());
                for inner in outer {
                    let arr = inner.as_array().ok_or_else(|| RuntimeError::new("Expected array of positions."))?;
                    res.push(unpack_cartesian3_values(arr)?);
                }
                Some(res)
            } else if let Some(outer) = get_defined(item, "cartographicRadians").and_then(Value::as_array) {
                let mut res = Vec::with_capacity(outer.len());
                for inner in outer {
                    let arr = inner.as_array().ok_or_else(|| RuntimeError::new("Expected array of positions."))?;
                    res.push(unpack_cartographic_radians_values(arr)?);
                }
                Some(res)
            } else if let Some(outer) = get_defined(item, "cartographicDegrees").and_then(Value::as_array) {
                let mut res = Vec::with_capacity(outer.len());
                for inner in outer {
                    let arr = inner.as_array().ok_or_else(|| RuntimeError::new("Expected array of positions."))?;
                    res.push(unpack_cartographic_degrees_values(arr)?);
                }
                Some(res)
            } else {
                None
            };
        if let Some(holes) = holes {
            let encoded: Vec<Value> =
                holes.iter().map(|ring| Value::Array(ring.iter().map(|p| json!(p.to_array())).collect())).collect();
            process_preunpacked_value(target, property_name, item, DynamicValue::Json(JsonReference::array(encoded)))?;
        }
    }
    Ok(())
}

pub(crate) fn process_shape(
    target: &dyn CzmlPropertyTarget,
    property_name: &str,
    packet_data: Option<&Value>,
    entity_collection: &Rc<EntityCollection>,
) -> Result<(), RuntimeError> {
    let Some(packet_data) = packet_data.filter(|v| !v.is_null()) else {
        return Ok(());
    };
    let items: Vec<&Value> = match packet_data.as_array() {
        Some(arr) => arr.iter().collect(),
        None => vec![packet_data],
    };
    let ctx = PacketContext { interval: None, source_uri: None, entity_collection };
    for item in items {
        if get_defined(item, "delete") == Some(&Value::Bool(true)) {
            process_property(CzmlValueType::Array, target, property_name, item, &ctx)?;
            continue;
        }
        if let Some(refs) = get_defined(item, "references").and_then(Value::as_array) {
            process_references_array(
                target,
                property_name,
                refs,
                get_defined(item, "interval").and_then(Value::as_str),
                entity_collection,
                false,
            )?;
            continue;
        }
        let coords =
            get_defined(item, "cartesian2").or_else(|| get_defined(item, "cartesian")).and_then(Value::as_array);
        if let Some(arr) = coords {
            let pts = unpack_cartesian2_values(arr)?;
            let encoded: Vec<Value> = pts.iter().map(|p| json!([p.x, p.y])).collect();
            process_preunpacked_value(target, property_name, item, DynamicValue::Json(JsonReference::array(encoded)))?;
        }
    }
    Ok(())
}
