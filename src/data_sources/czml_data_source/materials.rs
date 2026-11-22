//! CZML material property processing (`processMaterialPacketData` and
//! `processMaterialProperty`) for all 9 CesiumJS material types and
//! interval-scoped [`CompositeMaterialProperty`].
use super::{
    adapters::CzmlPropertyTarget,
    packet::{process_packet_data, PacketContext},
    unwrap::{combine_intervals, get_defined},
    CzmlValueType,
};
use crate::{
    core::{RuntimeError, TimeInterval},
    data_sources::{
        CheckerboardMaterialProperty, ColorMaterialProperty, ComponentRef, CompositeMaterialProperty, EntityComponent,
        GridMaterialProperty, ImageMaterialProperty, PolylineArrowMaterialProperty, PolylineDashMaterialProperty,
        PolylineGlowMaterialProperty, PolylineOutlineMaterialProperty, PropertyInput, StripeMaterialProperty,
    },
};
use serde_json::Value;
use std::rc::Rc;

fn reuse_or_new<T: EntityComponent + Default + 'static>(existing: Option<ComponentRef>) -> (ComponentRef, bool) {
    if let Some(comp) = existing {
        if comp.as_any().is::<T>() {
            return (comp, false);
        }
    }
    (Rc::new(T::default()), true)
}

pub(crate) fn process_material_property(
    target: &dyn CzmlPropertyTarget,
    property_name: &str,
    packet_data: &Value,
    ctx: &PacketContext<'_>,
) -> Result<(), RuntimeError> {
    let combined_interval =
        combine_intervals(get_defined(packet_data, "interval").and_then(Value::as_str), ctx.interval)?;

    let mut composite_ref: Option<ComponentRef> = None;
    let mut interval_spec: Option<TimeInterval<()>> = None;
    let mut existing_interval_had_data = false;

    let existing_material: Option<ComponentRef> = if let Some(ci) = combined_interval {
        let comp = match target.get_czml_property(property_name) {
            Some(PropertyInput::Component(c)) if c.as_any().is::<CompositeMaterialProperty>() => c,
            _ => {
                let c: ComponentRef = Rc::new(CompositeMaterialProperty::new());
                target.set_czml_property(property_name, Some(PropertyInput::Component(c.clone())))?;
                c
            }
        };
        let composite = comp.as_any().downcast_ref::<CompositeMaterialProperty>().unwrap();
        let existing_iv =
            composite.intervals().read().find_interval(Some(&ci.start), Some(&ci.stop), None, None).cloned();
        let mat = if let Some(iv) = existing_iv {
            existing_interval_had_data = iv.data.is_some();
            interval_spec = Some(TimeInterval::new(iv.start, iv.stop, iv.is_start_included, iv.is_stop_included, None));
            iv.data
        } else {
            interval_spec = Some(ci);
            None
        };
        composite_ref = Some(comp);
        mat
    } else {
        target.get_czml_property(property_name).and_then(|v| v.component())
    };

    let sub_ctx =
        PacketContext { interval: None, source_uri: ctx.source_uri, entity_collection: ctx.entity_collection };
    let no_uri_ctx = PacketContext { interval: None, source_uri: None, entity_collection: ctx.entity_collection };

    let (result_material, replaced): (Option<ComponentRef>, bool) = if let Some(material_data) =
        get_defined(packet_data, "solidColor")
    {
        let (comp, replaced) = reuse_or_new::<ColorMaterialProperty>(existing_material);
        let mat = comp.as_any().downcast_ref::<ColorMaterialProperty>().unwrap();
        process_packet_data(CzmlValueType::Color, mat, "color", material_data.get("color"), &no_uri_ctx)?;
        (Some(comp), replaced)
    } else if let Some(material_data) = get_defined(packet_data, "grid") {
        let (comp, replaced) = reuse_or_new::<GridMaterialProperty>(existing_material);
        let mat = comp.as_any().downcast_ref::<GridMaterialProperty>().unwrap();
        process_packet_data(CzmlValueType::Color, mat, "color", material_data.get("color"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Number, mat, "cellAlpha", material_data.get("cellAlpha"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Cartesian2, mat, "lineCount", material_data.get("lineCount"), &sub_ctx)?;
        process_packet_data(
            CzmlValueType::Cartesian2,
            mat,
            "lineThickness",
            material_data.get("lineThickness"),
            &sub_ctx,
        )?;
        process_packet_data(CzmlValueType::Cartesian2, mat, "lineOffset", material_data.get("lineOffset"), &sub_ctx)?;
        (Some(comp), replaced)
    } else if let Some(material_data) = get_defined(packet_data, "image") {
        let (comp, replaced) = reuse_or_new::<ImageMaterialProperty>(existing_material);
        let mat = comp.as_any().downcast_ref::<ImageMaterialProperty>().unwrap();
        process_packet_data(CzmlValueType::Image, mat, "image", material_data.get("image"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Cartesian2, mat, "repeat", material_data.get("repeat"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Color, mat, "color", material_data.get("color"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Boolean, mat, "transparent", material_data.get("transparent"), &sub_ctx)?;
        (Some(comp), replaced)
    } else if let Some(material_data) = get_defined(packet_data, "stripe") {
        let (comp, replaced) = reuse_or_new::<StripeMaterialProperty>(existing_material);
        let mat = comp.as_any().downcast_ref::<StripeMaterialProperty>().unwrap();
        process_packet_data(
            CzmlValueType::StripeOrientation,
            mat,
            "orientation",
            material_data.get("orientation"),
            &sub_ctx,
        )?;
        process_packet_data(CzmlValueType::Color, mat, "evenColor", material_data.get("evenColor"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Color, mat, "oddColor", material_data.get("oddColor"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Number, mat, "offset", material_data.get("offset"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Number, mat, "repeat", material_data.get("repeat"), &sub_ctx)?;
        (Some(comp), replaced)
    } else if let Some(material_data) = get_defined(packet_data, "polylineOutline") {
        let (comp, replaced) = reuse_or_new::<PolylineOutlineMaterialProperty>(existing_material);
        let mat = comp.as_any().downcast_ref::<PolylineOutlineMaterialProperty>().unwrap();
        process_packet_data(CzmlValueType::Color, mat, "color", material_data.get("color"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Color, mat, "outlineColor", material_data.get("outlineColor"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Number, mat, "outlineWidth", material_data.get("outlineWidth"), &sub_ctx)?;
        (Some(comp), replaced)
    } else if let Some(material_data) = get_defined(packet_data, "polylineGlow") {
        let (comp, replaced) = reuse_or_new::<PolylineGlowMaterialProperty>(existing_material);
        let mat = comp.as_any().downcast_ref::<PolylineGlowMaterialProperty>().unwrap();
        process_packet_data(CzmlValueType::Color, mat, "color", material_data.get("color"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Number, mat, "glowPower", material_data.get("glowPower"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Number, mat, "taperPower", material_data.get("taperPower"), &sub_ctx)?;
        (Some(comp), replaced)
    } else if let Some(material_data) = get_defined(packet_data, "polylineArrow") {
        let (comp, replaced) = reuse_or_new::<PolylineArrowMaterialProperty>(existing_material);
        let mat = comp.as_any().downcast_ref::<PolylineArrowMaterialProperty>().unwrap();
        process_packet_data(CzmlValueType::Color, mat, "color", material_data.get("color"), &no_uri_ctx)?;
        (Some(comp), replaced)
    } else if let Some(material_data) = get_defined(packet_data, "polylineDash") {
        let (comp, replaced) = reuse_or_new::<PolylineDashMaterialProperty>(existing_material);
        let mat = comp.as_any().downcast_ref::<PolylineDashMaterialProperty>().unwrap();
        process_packet_data(CzmlValueType::Color, mat, "color", material_data.get("color"), &no_uri_ctx)?;
        process_packet_data(CzmlValueType::Color, mat, "gapColor", material_data.get("gapColor"), &no_uri_ctx)?;
        process_packet_data(CzmlValueType::Number, mat, "dashLength", material_data.get("dashLength"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Number, mat, "dashPattern", material_data.get("dashPattern"), &sub_ctx)?;
        (Some(comp), replaced)
    } else if let Some(material_data) = get_defined(packet_data, "checkerboard") {
        let (comp, replaced) = reuse_or_new::<CheckerboardMaterialProperty>(existing_material);
        let mat = comp.as_any().downcast_ref::<CheckerboardMaterialProperty>().unwrap();
        process_packet_data(CzmlValueType::Color, mat, "evenColor", material_data.get("evenColor"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Color, mat, "oddColor", material_data.get("oddColor"), &sub_ctx)?;
        process_packet_data(CzmlValueType::Cartesian2, mat, "repeat", material_data.get("repeat"), &sub_ctx)?;
        (Some(comp), replaced)
    } else {
        (existing_material, false)
    };

    if let (Some(comp), Some(iv)) = (composite_ref, interval_spec) {
        if replaced || !existing_interval_had_data {
            let composite = comp.as_any().downcast_ref::<CompositeMaterialProperty>().unwrap();
            composite.intervals().add_interval(TimeInterval::new(
                iv.start,
                iv.stop,
                iv.is_start_included,
                iv.is_stop_included,
                result_material,
            ));
        }
    } else if let Some(mat) = result_material {
        target.set_czml_property(property_name, Some(PropertyInput::Component(mat)))?;
    }

    Ok(())
}

pub(crate) fn process_material_packet_data(
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
            process_material_property(target, property_name, item, ctx)?;
        }
    } else {
        process_material_property(target, property_name, packet_data, ctx)?;
    }
    Ok(())
}
