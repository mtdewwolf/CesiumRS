//! CZML position property processing (`processPositionPacketData` and
//! `processPositionProperty`), including `FIXED`/`INERTIAL` reference frames,
//! `cartesianVelocity` derivatives, intervals, references, and deletions.
use super::{
    adapters::CzmlPropertyTarget,
    packet::{create_reference_property, PacketContext},
    unwrap::{
        combine_intervals, get_defined, is_defined, parse_reference_frame, unwrap_cartesian_interval,
        update_interpolation_settings, UnwrappedCartesian,
    },
};
use crate::{
    core::{iso8601, JulianDate, ReferenceFrame, RuntimeError, TimeInterval},
    data_sources::{
        CompositePositionProperty, ConstantPositionProperty, PositionComponent, PositionProperty, PositionPropertyRef,
        PropertyInput, SampledPositionProperty, TimeIntervalCollectionPositionProperty,
    },
};
use serde_json::Value;
use std::rc::Rc;

fn wrap_position_in_infinite_interval(property: PositionPropertyRef) -> TimeInterval<PositionPropertyRef> {
    let max = iso8601::maximum_interval::<()>();
    TimeInterval::new(max.start, max.stop, max.is_start_included, max.is_stop_included, Some(property))
}

fn convert_position_to_composite(existing: PositionPropertyRef) -> Rc<CompositePositionProperty> {
    let composite = Rc::new(CompositePositionProperty::new(existing.reference_frame()));
    composite.intervals().add_interval(wrap_position_in_infinite_interval(existing));
    composite
}

fn existing_position_property(input: Option<PropertyInput>) -> Option<PositionPropertyRef> {
    input.and_then(|v| v.component()).and_then(|c| c.position_property())
}

fn remove_position_property_data(prop: &dyn PositionProperty, interval: &TimeInterval<()>) {
    if let Some(sampled) = prop.as_any().downcast_ref::<SampledPositionProperty>() {
        sampled.remove_samples(interval);
        return;
    }
    if let Some(ticp) = prop.as_any().downcast_ref::<TimeIntervalCollectionPositionProperty>() {
        let probe = TimeInterval::new(
            interval.start,
            interval.stop,
            interval.is_start_included,
            interval.is_stop_included,
            None,
        );
        ticp.intervals().remove_interval(&probe);
        return;
    }
    if let Some(composite) = prop.as_any().downcast_ref::<CompositePositionProperty>() {
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
                    remove_position_property_data(&**inner, interval);
                }
            }
        }
        composite.intervals().remove_interval(&probe);
    }
}

pub(crate) fn process_position_property(
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
        if let Some(prop) = existing_position_property(target.get_czml_property(property_name)) {
            remove_position_property_data(&*prop, combined_interval.as_ref().unwrap());
        }
        return Ok(());
    }

    let number_of_derivatives = usize::from(is_defined(packet_data.get("cartesianVelocity")));
    let is_value = !is_defined(packet_data.get("reference"));

    let mut reference_frame = ReferenceFrame::Fixed;
    let unwrapped = if is_value {
        if let Some(frame) =
            get_defined(packet_data, "referenceFrame").and_then(Value::as_str).and_then(parse_reference_frame)
        {
            reference_frame = frame;
        }
        Some(unwrap_cartesian_interval(packet_data, false)?)
    } else {
        None
    };

    let is_sampled = matches!(unwrapped, Some(UnwrappedCartesian::Sampled(_)));

    // Constant value without an interval replaces anything previously stored.
    if !is_sampled && !has_interval {
        if is_value {
            let Some(UnwrappedCartesian::Constant(pos)) = unwrapped else { return Ok(()) };
            let prop: PositionPropertyRef = Rc::new(ConstantPositionProperty::new(Some(pos), reference_frame));
            target.set_czml_property(property_name, Some(PropertyInput::Component(PositionComponent::wrap(prop))))?;
        } else {
            let ref_str = get_defined(packet_data, "reference")
                .and_then(Value::as_str)
                .ok_or_else(|| RuntimeError::new("Position reference must be a string."))?;
            let ref_prop = create_reference_property(ctx.entity_collection, ref_str)?;
            target.set_czml_property(property_name, Some(PropertyInput::Component(ref_prop)))?;
        }
        return Ok(());
    }

    let epoch = match get_defined(packet_data, "epoch").and_then(Value::as_str) {
        Some(s) => Some(JulianDate::from_iso8601(s).map_err(|e| RuntimeError::new(e.to_string()))?),
        None => None,
    };

    // Sampled value without an interval replaces any incompatible property.
    if is_sampled && !has_interval {
        let Some(UnwrappedCartesian::Sampled(samples)) = unwrapped else { return Ok(()) };
        let existing = existing_position_property(target.get_czml_property(property_name));
        let sampled_prop: PositionPropertyRef = match existing {
            Some(ref p)
                if p.as_any()
                    .downcast_ref::<SampledPositionProperty>()
                    .is_some_and(|s| s.reference_frame() == reference_frame) =>
            {
                p.clone()
            }
            _ => {
                let p: PositionPropertyRef =
                    Rc::new(SampledPositionProperty::new(reference_frame, number_of_derivatives));
                target.set_czml_property(
                    property_name,
                    Some(PropertyInput::Component(PositionComponent::wrap(p.clone()))),
                )?;
                p
            }
        };
        let sampled = sampled_prop.as_any().downcast_ref::<SampledPositionProperty>().unwrap();
        sampled.add_samples_packed_array(&samples, epoch)?;
        update_interpolation_settings(packet_data, sampled);
        return Ok(());
    }

    let ci = combined_interval.unwrap();

    // Constant value (or reference) with an interval.
    if !is_sampled && has_interval {
        let existing = existing_position_property(target.get_czml_property(property_name));

        if is_value {
            let Some(UnwrappedCartesian::Constant(pos)) = unwrapped else { return Ok(()) };
            if existing.is_none() {
                let ticp = Rc::new(TimeIntervalCollectionPositionProperty::new(reference_frame));
                ticp.intervals().add_interval(TimeInterval::new(
                    ci.start,
                    ci.stop,
                    ci.is_start_included,
                    ci.is_stop_included,
                    Some(Rc::new(pos)),
                ));
                let prop: PositionPropertyRef = ticp;
                target
                    .set_czml_property(property_name, Some(PropertyInput::Component(PositionComponent::wrap(prop))))?;
                return Ok(());
            }
            let existing_prop = existing.unwrap();
            if let Some(ticp) = existing_prop.as_any().downcast_ref::<TimeIntervalCollectionPositionProperty>() {
                if ticp.reference_frame() == reference_frame {
                    ticp.intervals().add_interval(TimeInterval::new(
                        ci.start,
                        ci.stop,
                        ci.is_start_included,
                        ci.is_stop_included,
                        Some(Rc::new(pos)),
                    ));
                    return Ok(());
                }
            }
            if let Some(composite) = existing_prop.as_any().downcast_ref::<CompositePositionProperty>() {
                let constant: PositionPropertyRef = Rc::new(ConstantPositionProperty::new(Some(pos), reference_frame));
                composite.intervals().add_interval(TimeInterval::new(
                    ci.start,
                    ci.stop,
                    ci.is_start_included,
                    ci.is_stop_included,
                    Some(constant),
                ));
            } else {
                let composite = convert_position_to_composite(existing_prop);
                let constant: PositionPropertyRef = Rc::new(ConstantPositionProperty::new(Some(pos), reference_frame));
                composite.intervals().add_interval(TimeInterval::new(
                    ci.start,
                    ci.stop,
                    ci.is_start_included,
                    ci.is_stop_included,
                    Some(constant),
                ));
                let prop: PositionPropertyRef = composite;
                target
                    .set_czml_property(property_name, Some(PropertyInput::Component(PositionComponent::wrap(prop))))?;
            }
            return Ok(());
        } else {
            let ref_str = get_defined(packet_data, "reference")
                .and_then(Value::as_str)
                .ok_or_else(|| RuntimeError::new("Position reference must be a string."))?;
            let ref_pos = create_reference_property(ctx.entity_collection, ref_str)?.as_position();
            let composite = match existing {
                None => {
                    let c = Rc::new(CompositePositionProperty::new(ReferenceFrame::Fixed));
                    let prop: PositionPropertyRef = c.clone();
                    target.set_czml_property(
                        property_name,
                        Some(PropertyInput::Component(PositionComponent::wrap(prop))),
                    )?;
                    c
                }
                Some(ref p) if p.as_any().is::<CompositePositionProperty>() => {
                    let c = p.as_any().downcast_ref::<CompositePositionProperty>().unwrap();
                    c.intervals().add_interval(TimeInterval::new(
                        ci.start,
                        ci.stop,
                        ci.is_start_included,
                        ci.is_stop_included,
                        Some(ref_pos),
                    ));
                    return Ok(());
                }
                Some(p) => {
                    let c = convert_position_to_composite(p);
                    let prop: PositionPropertyRef = c.clone();
                    target.set_czml_property(
                        property_name,
                        Some(PropertyInput::Component(PositionComponent::wrap(prop))),
                    )?;
                    c
                }
            };
            composite.intervals().add_interval(TimeInterval::new(
                ci.start,
                ci.stop,
                ci.is_start_included,
                ci.is_stop_included,
                Some(ref_pos),
            ));
            return Ok(());
        }
    }

    // Sampled position with an interval -> CompositePositionProperty containing SampledPositionProperty.
    let Some(UnwrappedCartesian::Sampled(samples)) = unwrapped else { return Ok(()) };
    let existing = existing_position_property(target.get_czml_property(property_name));

    let composite_prop: PositionPropertyRef = match existing {
        None => {
            let c: PositionPropertyRef = Rc::new(CompositePositionProperty::new(reference_frame));
            target
                .set_czml_property(property_name, Some(PropertyInput::Component(PositionComponent::wrap(c.clone()))))?;
            c
        }
        Some(ref p) if p.as_any().is::<CompositePositionProperty>() => p.clone(),
        Some(p) => {
            let c: PositionPropertyRef = convert_position_to_composite(p);
            target
                .set_czml_property(property_name, Some(PropertyInput::Component(PositionComponent::wrap(c.clone()))))?;
            c
        }
    };

    let composite = composite_prop.as_any().downcast_ref::<CompositePositionProperty>().unwrap();
    let existing_interval_data = composite
        .intervals()
        .read()
        .find_interval(Some(&ci.start), Some(&ci.stop), Some(ci.is_start_included), Some(ci.is_stop_included))
        .and_then(|iv| iv.data.clone());

    let sampled_prop: PositionPropertyRef = match existing_interval_data {
        Some(ref p)
            if p.as_any()
                .downcast_ref::<SampledPositionProperty>()
                .is_some_and(|s| s.reference_frame() == reference_frame) =>
        {
            p.clone()
        }
        _ => {
            let p: PositionPropertyRef = Rc::new(SampledPositionProperty::new(reference_frame, number_of_derivatives));
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

    let sampled = sampled_prop.as_any().downcast_ref::<SampledPositionProperty>().unwrap();
    sampled.add_samples_packed_array(&samples, epoch)?;
    update_interpolation_settings(packet_data, sampled);
    Ok(())
}

pub(crate) fn process_position_packet_data(
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
            process_position_property(target, property_name, item, ctx)?;
        }
    } else {
        process_position_property(target, property_name, packet_data, ctx)?;
    }
    Ok(())
}
