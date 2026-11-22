//! CZML value-type inference, interval unwrapping, coordinate conversions,
//! enum parsing, and interpolation/extrapolation configuration.
use super::{
    adapters::{bounding_rect_to_dynamic, distance_display_to_dynamic, near_far_to_dynamic, rectangle_to_dynamic},
    CzmlValueType,
};
use crate::{
    core::{
        math::to_radians, resource::DerivedResourceOptions, ArcType, BoundingRectangle, Cartesian2, Cartesian3,
        Cartographic, ClockRange, ClockStep, Color, CornerType, DistanceDisplayCondition, Ellipsoid, ExtrapolationType,
        JulianDate, NearFarScalar, Quaternion, Rectangle, ReferenceFrame, Resource, RuntimeError, Spherical,
        TimeInterval,
    },
    data_sources::{
        DynamicValue, Interpolation, JsonReference, PackedSampleValue, SampledProperty, SampledValue, StripeOrientation,
    },
    scene::{
        ClassificationType, ColorBlendMode, HeightReference, HorizontalOrigin, LabelStyle,
        SensorVolumePortionToDisplay, ShadowMode, VerticalOrigin,
    },
};
use serde_json::Value;
use std::rc::Rc;

/// Unwrapped representation of a CZML property interval.
pub(crate) enum UnwrappedInterval {
    /// Not a recognized value for this type; skip.
    None,
    /// Single constant value.
    Constant(DynamicValue),
    /// Time-tagged packed sample array `[t0, v0..., t1, v1..., ...]`.
    Sampled(Vec<PackedSampleValue>),
}

pub(crate) fn is_defined(v: Option<&Value>) -> bool {
    matches!(v, Some(v) if !v.is_null())
}

pub(crate) fn get_defined<'a>(obj: &'a Value, key: &str) -> Option<&'a Value> {
    obj.get(key).filter(|v| !v.is_null())
}

pub(crate) fn interval_from_string(interval_string: Option<&str>) -> Result<Option<TimeInterval<()>>, RuntimeError> {
    let Some(s) = interval_string else { return Ok(None) };
    TimeInterval::from_iso8601(s, true, true, None).map(Some).map_err(|e| RuntimeError::new(e.to_string()))
}

pub(crate) fn combine_intervals(
    packet_interval: Option<&str>,
    constrained: Option<&TimeInterval<()>>,
) -> Result<Option<TimeInterval<()>>, RuntimeError> {
    let combined = interval_from_string(packet_interval)?;
    Ok(match (combined, constrained) {
        (Some(a), Some(b)) => Some(a.intersect(b, None)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b.clone()),
        (None, None) => None,
    })
}

pub(crate) fn parse_time_tag(v: &Value) -> Result<PackedSampleValue, RuntimeError> {
    if let Some(n) = v.as_f64() {
        Ok(PackedSampleValue::Number(n))
    } else if let Some(s) = v.as_str() {
        Ok(PackedSampleValue::Iso8601(s.to_owned()))
    } else {
        Err(RuntimeError::new(format!("Invalid CZML sample time tag: {v}")))
    }
}

pub(crate) fn as_f64_strict(v: &Value) -> Result<f64, RuntimeError> {
    v.as_f64().ok_or_else(|| RuntimeError::new(format!("Expected numeric CZML value, got {v}")))
}

pub(crate) fn parse_f64_slice(arr: &[Value]) -> Result<Vec<f64>, RuntimeError> {
    arr.iter().map(as_f64_strict).collect()
}

pub(crate) fn parse_packed_numbers(arr: &[Value], packed_len: usize) -> Result<Vec<PackedSampleValue>, RuntimeError> {
    let stride = packed_len + 1;
    if !arr.len().is_multiple_of(stride) {
        return Err(RuntimeError::new("Invalid packed CZML sample array length."));
    }
    let mut out = Vec::with_capacity(arr.len());
    for chunk in arr.chunks(stride) {
        out.push(parse_time_tag(&chunk[0])?);
        for item in &chunk[1..] {
            out.push(PackedSampleValue::Number(as_f64_strict(item)?));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Enum parsers
// ---------------------------------------------------------------------------

pub(crate) fn parse_arc_type(s: &str) -> Option<ArcType> {
    match s {
        "NONE" => Some(ArcType::None),
        "GEODESIC" => Some(ArcType::Geodesic),
        "RHUMB" => Some(ArcType::Rhumb),
        _ => None,
    }
}

pub(crate) fn parse_classification_type(s: &str) -> Option<ClassificationType> {
    match s {
        "TERRAIN" => Some(ClassificationType::Terrain),
        "CESIUM_3D_TILE" => Some(ClassificationType::Cesium3DTile),
        "BOTH" => Some(ClassificationType::Both),
        _ => None,
    }
}

pub(crate) fn parse_color_blend_mode(s: &str) -> Option<ColorBlendMode> {
    match s {
        "HIGHLIGHT" => Some(ColorBlendMode::Highlight),
        "REPLACE" => Some(ColorBlendMode::Replace),
        "MIX" => Some(ColorBlendMode::Mix),
        _ => None,
    }
}

pub(crate) fn parse_corner_type(s: &str) -> Option<CornerType> {
    match s {
        "ROUNDED" => Some(CornerType::Rounded),
        "MITERED" => Some(CornerType::Mitered),
        "BEVELED" => Some(CornerType::Beveled),
        _ => None,
    }
}

pub(crate) fn parse_height_reference(s: &str) -> Option<HeightReference> {
    match s {
        "NONE" => Some(HeightReference::None),
        "CLAMP_TO_GROUND" => Some(HeightReference::ClampToGround),
        "RELATIVE_TO_GROUND" => Some(HeightReference::RelativeToGround),
        "CLAMP_TO_TERRAIN" => Some(HeightReference::ClampToTerrain),
        "RELATIVE_TO_TERRAIN" => Some(HeightReference::RelativeToTerrain),
        "CLAMP_TO_3D_TILE" => Some(HeightReference::ClampTo3DTile),
        "RELATIVE_TO_3D_TILE" => Some(HeightReference::RelativeTo3DTile),
        _ => None,
    }
}

pub(crate) fn parse_horizontal_origin(s: &str) -> Option<HorizontalOrigin> {
    match s {
        "CENTER" => Some(HorizontalOrigin::Center),
        "LEFT" => Some(HorizontalOrigin::Left),
        "RIGHT" => Some(HorizontalOrigin::Right),
        _ => None,
    }
}

pub(crate) fn parse_label_style(s: &str) -> Option<LabelStyle> {
    match s {
        "FILL" => Some(LabelStyle::Fill),
        "OUTLINE" => Some(LabelStyle::Outline),
        "FILL_AND_OUTLINE" => Some(LabelStyle::FillAndOutline),
        _ => None,
    }
}

pub(crate) fn parse_sensor_volume_portion(s: &str) -> Option<SensorVolumePortionToDisplay> {
    match s {
        "COMPLETE" => Some(SensorVolumePortionToDisplay::Complete),
        "BELOW_ELLIPSOID_HORIZON" => Some(SensorVolumePortionToDisplay::BelowEllipsoidHorizon),
        "ABOVE_ELLIPSOID_HORIZON" => Some(SensorVolumePortionToDisplay::AboveEllipsoidHorizon),
        _ => None,
    }
}

pub(crate) fn parse_shadow_mode(s: &str) -> Option<ShadowMode> {
    match s {
        "DISABLED" => Some(ShadowMode::Disabled),
        "ENABLED" => Some(ShadowMode::Enabled),
        "CAST_ONLY" => Some(ShadowMode::CastOnly),
        "RECEIVE_ONLY" => Some(ShadowMode::ReceiveOnly),
        _ => None,
    }
}

pub(crate) fn parse_stripe_orientation(s: &str) -> Option<StripeOrientation> {
    match s {
        "HORIZONTAL" => Some(StripeOrientation::Horizontal),
        "VERTICAL" => Some(StripeOrientation::Vertical),
        _ => None,
    }
}

pub(crate) fn parse_vertical_origin(s: &str) -> Option<VerticalOrigin> {
    match s {
        "CENTER" => Some(VerticalOrigin::Center),
        "BOTTOM" => Some(VerticalOrigin::Bottom),
        "BASELINE" => Some(VerticalOrigin::Baseline),
        "TOP" => Some(VerticalOrigin::Top),
        _ => None,
    }
}

pub(crate) fn parse_reference_frame(s: &str) -> Option<ReferenceFrame> {
    match s {
        "FIXED" => Some(ReferenceFrame::Fixed),
        "INERTIAL" => Some(ReferenceFrame::Inertial),
        _ => None,
    }
}

pub(crate) fn parse_clock_range(s: &str) -> Option<ClockRange> {
    match s {
        "UNBOUNDED" => Some(ClockRange::Unbounded),
        "CLAMPED" => Some(ClockRange::Clamped),
        "LOOP_STOP" => Some(ClockRange::LoopStop),
        _ => None,
    }
}

pub(crate) fn parse_clock_step(s: &str) -> Option<ClockStep> {
    match s {
        "TICK_DEPENDENT" => Some(ClockStep::TickDependent),
        "SYSTEM_CLOCK_MULTIPLIER" => Some(ClockStep::SystemClockMultiplier),
        "SYSTEM_CLOCK" => Some(ClockStep::SystemClock),
        _ => None,
    }
}

pub(crate) fn parse_extrapolation_type(s: &str) -> Option<ExtrapolationType> {
    match s {
        "NONE" => Some(ExtrapolationType::None),
        "HOLD" => Some(ExtrapolationType::Hold),
        "EXTRAPOLATE" => Some(ExtrapolationType::Extrapolate),
        _ => None,
    }
}

pub(crate) fn parse_interpolation_algorithm(s: &str) -> Option<Interpolation> {
    match s {
        "LINEAR" => Some(Interpolation::Linear),
        "LAGRANGE" => Some(Interpolation::Lagrange),
        "HERMITE" => Some(Interpolation::Hermite),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Property type inference for PropertyBag (`getPropertyType`)
// ---------------------------------------------------------------------------

pub(crate) fn get_property_type(czml_interval: &Value) -> CzmlValueType {
    match czml_interval {
        Value::Bool(_) => CzmlValueType::Boolean,
        Value::Number(_) => CzmlValueType::Number,
        Value::String(_) => CzmlValueType::String,
        Value::Object(map) => {
            if map.contains_key("array") {
                CzmlValueType::Array
            } else if map.contains_key("boolean") {
                CzmlValueType::Boolean
            } else if map.contains_key("boundingRectangle") {
                CzmlValueType::BoundingRectangle
            } else if map.contains_key("cartesian2") {
                CzmlValueType::Cartesian2
            } else if map.contains_key("cartesian")
                || map.contains_key("spherical")
                || map.contains_key("cartographicRadians")
                || map.contains_key("cartographicDegrees")
            {
                CzmlValueType::Cartesian3
            } else if map.contains_key("unitCartesian") || map.contains_key("unitSpherical") {
                CzmlValueType::UnitCartesian3
            } else if map.contains_key("rgba") || map.contains_key("rgbaf") {
                CzmlValueType::Color
            } else if map.contains_key("arcType") {
                CzmlValueType::ArcType
            } else if map.contains_key("classificationType") {
                CzmlValueType::ClassificationType
            } else if map.contains_key("colorBlendMode") {
                CzmlValueType::ColorBlendMode
            } else if map.contains_key("cornerType") {
                CzmlValueType::CornerType
            } else if map.contains_key("heightReference") {
                CzmlValueType::HeightReference
            } else if map.contains_key("horizontalOrigin") {
                CzmlValueType::HorizontalOrigin
            } else if map.contains_key("date") {
                CzmlValueType::JulianDate
            } else if map.contains_key("labelStyle") {
                CzmlValueType::LabelStyle
            } else if map.contains_key("number") {
                CzmlValueType::Number
            } else if map.contains_key("nearFarScalar") {
                CzmlValueType::NearFarScalar
            } else if map.contains_key("distanceDisplayCondition") {
                CzmlValueType::DistanceDisplayCondition
            } else if map.contains_key("object") || map.contains_key("value") {
                CzmlValueType::Object
            } else if map.contains_key("unitQuaternion") {
                CzmlValueType::Quaternion
            } else if map.contains_key("shadowMode") {
                CzmlValueType::ShadowMode
            } else if map.contains_key("string") {
                CzmlValueType::String
            } else if map.contains_key("stripeOrientation") {
                CzmlValueType::StripeOrientation
            } else if map.contains_key("wsen") || map.contains_key("wsenDegrees") {
                CzmlValueType::Rectangle
            } else if map.contains_key("uri") {
                CzmlValueType::Uri
            } else if map.contains_key("verticalOrigin") {
                CzmlValueType::VerticalOrigin
            } else {
                CzmlValueType::Object
            }
        }
        _ => CzmlValueType::Object,
    }
}

// ---------------------------------------------------------------------------
// Coordinate & interval unwrapping
// ---------------------------------------------------------------------------

pub(crate) fn unwrap_uri_interval(czml_interval: &Value, source_uri: Option<&Resource>) -> Option<DynamicValue> {
    let raw = get_defined(czml_interval, "uri").unwrap_or(czml_interval);
    let uri_str = raw.as_str()?;
    let resource = if let Some(base) = source_uri {
        base.get_derived_resource(DerivedResourceOptions { url: Some(uri_str.to_owned()), ..Default::default() })
            .or_else(|| Resource::new(uri_str))
    } else {
        Resource::new(uri_str)
    };
    resource.map(|r| DynamicValue::Resource(Rc::new(r)))
}

fn unwrap_color_interval(czml_interval: &Value) -> Result<UnwrappedInterval, RuntimeError> {
    if let Some(rgbaf) = get_defined(czml_interval, "rgbaf").and_then(Value::as_array) {
        if rgbaf.len() == Color::PACKED_LENGTH {
            let nums = parse_f64_slice(rgbaf)?;
            return Ok(UnwrappedInterval::Constant(DynamicValue::Color(Color::unpack(&nums, 0))));
        }
        return Ok(UnwrappedInterval::Sampled(parse_packed_numbers(rgbaf, Color::PACKED_LENGTH)?));
    }
    if let Some(rgba) = get_defined(czml_interval, "rgba").and_then(Value::as_array) {
        if rgba.len() == Color::PACKED_LENGTH {
            let nums = parse_f64_slice(rgba)?;
            return Ok(UnwrappedInterval::Constant(DynamicValue::Color(Color::from_bytes(
                nums[0], nums[1], nums[2], nums[3],
            ))));
        }
        if !rgba.len().is_multiple_of(5) {
            return Err(RuntimeError::new("Invalid rgba sample array length."));
        }
        let mut out = Vec::with_capacity(rgba.len());
        for chunk in rgba.chunks(5) {
            out.push(parse_time_tag(&chunk[0])?);
            out.push(PackedSampleValue::Number(Color::byte_to_float(as_f64_strict(&chunk[1])?)));
            out.push(PackedSampleValue::Number(Color::byte_to_float(as_f64_strict(&chunk[2])?)));
            out.push(PackedSampleValue::Number(Color::byte_to_float(as_f64_strict(&chunk[3])?)));
            out.push(PackedSampleValue::Number(Color::byte_to_float(as_f64_strict(&chunk[4])?)));
        }
        return Ok(UnwrappedInterval::Sampled(out));
    }
    Ok(UnwrappedInterval::None)
}

fn unwrap_rectangle_interval(czml_interval: &Value) -> Result<UnwrappedInterval, RuntimeError> {
    if let Some(wsen) = get_defined(czml_interval, "wsen").and_then(Value::as_array) {
        if wsen.len() == Rectangle::PACKED_LENGTH {
            let nums = parse_f64_slice(wsen)?;
            return Ok(UnwrappedInterval::Constant(rectangle_to_dynamic(Rectangle::unpack(&nums, 0))));
        }
        return Ok(UnwrappedInterval::Sampled(parse_packed_numbers(wsen, Rectangle::PACKED_LENGTH)?));
    }
    if let Some(wsen_deg) = get_defined(czml_interval, "wsenDegrees").and_then(Value::as_array) {
        if wsen_deg.len() == Rectangle::PACKED_LENGTH {
            let nums = parse_f64_slice(wsen_deg)?;
            return Ok(UnwrappedInterval::Constant(rectangle_to_dynamic(Rectangle::from_degrees(
                nums[0], nums[1], nums[2], nums[3],
            ))));
        }
        if !wsen_deg.len().is_multiple_of(5) {
            return Err(RuntimeError::new("Invalid wsenDegrees sample array length."));
        }
        let mut out = Vec::with_capacity(wsen_deg.len());
        for chunk in wsen_deg.chunks(5) {
            out.push(parse_time_tag(&chunk[0])?);
            out.push(PackedSampleValue::Number(to_radians(as_f64_strict(&chunk[1])?)));
            out.push(PackedSampleValue::Number(to_radians(as_f64_strict(&chunk[2])?)));
            out.push(PackedSampleValue::Number(to_radians(as_f64_strict(&chunk[3])?)));
            out.push(PackedSampleValue::Number(to_radians(as_f64_strict(&chunk[4])?)));
        }
        return Ok(UnwrappedInterval::Sampled(out));
    }
    Ok(UnwrappedInterval::None)
}

/// Unwraps a Cartesian3-valued interval (`cartesian`, `cartesianVelocity`,
/// `unitCartesian`, `unitSpherical`, `spherical`, `cartographicRadians`,
/// `cartographicDegrees`).
///
/// Returns `(Option<Cartesian3>, Option<Vec<PackedSampleValue>>)`:
/// - `Ok(UnwrappedCartesian::Constant(c))` for a single value (including 6-element `cartesianVelocity`),
/// - `Ok(UnwrappedCartesian::Sampled(packed))` for a time-tagged sampled array.
pub(crate) enum UnwrappedCartesian {
    Constant(Cartesian3),
    Sampled(Vec<PackedSampleValue>),
}

pub(crate) fn unwrap_cartesian_interval(
    czml_interval: &Value,
    normalize: bool,
) -> Result<UnwrappedCartesian, RuntimeError> {
    let maybe_norm = |c: Cartesian3| if normalize { c.normalize() } else { c };

    if let Some(arr) = get_defined(czml_interval, "cartesian").and_then(Value::as_array) {
        return unwrap_raw_cartesian_array(arr, 3, normalize);
    }
    if let Some(arr) = get_defined(czml_interval, "cartesianVelocity").and_then(Value::as_array) {
        return unwrap_raw_cartesian_array(arr, 6, normalize);
    }
    if let Some(arr) = get_defined(czml_interval, "unitCartesian").and_then(Value::as_array) {
        return unwrap_raw_cartesian_array(arr, 3, normalize);
    }
    if let Some(arr) = get_defined(czml_interval, "unitSpherical").and_then(Value::as_array) {
        if arr.len() == 2 {
            let clock = as_f64_strict(&arr[0])?;
            let cone = as_f64_strict(&arr[1])?;
            let c = maybe_norm(Cartesian3::from_spherical(&Spherical::new(clock, cone, 1.0)));
            return Ok(UnwrappedCartesian::Constant(c));
        }
        if !arr.len().is_multiple_of(3) {
            return Err(RuntimeError::new("Invalid unitSpherical sample array length."));
        }
        let mut out = Vec::with_capacity((arr.len() / 3) * 4);
        for chunk in arr.chunks(3) {
            let t = parse_time_tag(&chunk[0])?;
            let clock = as_f64_strict(&chunk[1])?;
            let cone = as_f64_strict(&chunk[2])?;
            let c = maybe_norm(Cartesian3::from_spherical(&Spherical::new(clock, cone, 1.0)));
            out.push(t);
            out.push(PackedSampleValue::Number(c.x));
            out.push(PackedSampleValue::Number(c.y));
            out.push(PackedSampleValue::Number(c.z));
        }
        return Ok(UnwrappedCartesian::Sampled(out));
    }
    if let Some(arr) = get_defined(czml_interval, "spherical").and_then(Value::as_array) {
        if arr.len() == 3 {
            let c = maybe_norm(Cartesian3::from_spherical(&Spherical::new(
                as_f64_strict(&arr[0])?,
                as_f64_strict(&arr[1])?,
                as_f64_strict(&arr[2])?,
            )));
            return Ok(UnwrappedCartesian::Constant(c));
        }
        if !arr.len().is_multiple_of(4) {
            return Err(RuntimeError::new("Invalid spherical sample array length."));
        }
        let mut out = Vec::with_capacity(arr.len());
        for chunk in arr.chunks(4) {
            let t = parse_time_tag(&chunk[0])?;
            let c = maybe_norm(Cartesian3::from_spherical(&Spherical::new(
                as_f64_strict(&chunk[1])?,
                as_f64_strict(&chunk[2])?,
                as_f64_strict(&chunk[3])?,
            )));
            out.push(t);
            out.push(PackedSampleValue::Number(c.x));
            out.push(PackedSampleValue::Number(c.y));
            out.push(PackedSampleValue::Number(c.z));
        }
        return Ok(UnwrappedCartesian::Sampled(out));
    }
    if let Some(arr) = get_defined(czml_interval, "cartographicRadians").and_then(Value::as_array) {
        if arr.len() == 3 {
            let c = maybe_norm(Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::new(
                as_f64_strict(&arr[0])?,
                as_f64_strict(&arr[1])?,
                as_f64_strict(&arr[2])?,
            )));
            return Ok(UnwrappedCartesian::Constant(c));
        }
        if !arr.len().is_multiple_of(4) {
            return Err(RuntimeError::new("Invalid cartographicRadians sample array length."));
        }
        let mut out = Vec::with_capacity(arr.len());
        for chunk in arr.chunks(4) {
            let t = parse_time_tag(&chunk[0])?;
            let c = maybe_norm(Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::new(
                as_f64_strict(&chunk[1])?,
                as_f64_strict(&chunk[2])?,
                as_f64_strict(&chunk[3])?,
            )));
            out.push(t);
            out.push(PackedSampleValue::Number(c.x));
            out.push(PackedSampleValue::Number(c.y));
            out.push(PackedSampleValue::Number(c.z));
        }
        return Ok(UnwrappedCartesian::Sampled(out));
    }
    if let Some(arr) = get_defined(czml_interval, "cartographicDegrees").and_then(Value::as_array) {
        if arr.len() == 3 {
            let c = maybe_norm(Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::new(
                to_radians(as_f64_strict(&arr[0])?),
                to_radians(as_f64_strict(&arr[1])?),
                as_f64_strict(&arr[2])?,
            )));
            return Ok(UnwrappedCartesian::Constant(c));
        }
        if !arr.len().is_multiple_of(4) {
            return Err(RuntimeError::new("Invalid cartographicDegrees sample array length."));
        }
        let mut out = Vec::with_capacity(arr.len());
        for chunk in arr.chunks(4) {
            let t = parse_time_tag(&chunk[0])?;
            let c = maybe_norm(Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::new(
                to_radians(as_f64_strict(&chunk[1])?),
                to_radians(as_f64_strict(&chunk[2])?),
                as_f64_strict(&chunk[3])?,
            )));
            out.push(t);
            out.push(PackedSampleValue::Number(c.x));
            out.push(PackedSampleValue::Number(c.y));
            out.push(PackedSampleValue::Number(c.z));
        }
        return Ok(UnwrappedCartesian::Sampled(out));
    }

    Err(RuntimeError::new(format!("{czml_interval} is not a valid CZML interval.")))
}

fn unwrap_raw_cartesian_array(
    arr: &[Value],
    packed_len: usize,
    normalize: bool,
) -> Result<UnwrappedCartesian, RuntimeError> {
    if arr.len() == packed_len {
        let x = as_f64_strict(&arr[0])?;
        let y = as_f64_strict(&arr[1])?;
        let z = as_f64_strict(&arr[2])?;
        let c = Cartesian3::new(x, y, z);
        return Ok(UnwrappedCartesian::Constant(if normalize { c.normalize() } else { c }));
    }
    let stride = packed_len + 1;
    if !arr.len().is_multiple_of(stride) {
        return Err(RuntimeError::new("Invalid cartesian sample array length."));
    }
    let mut out = Vec::with_capacity(arr.len());
    for chunk in arr.chunks(stride) {
        out.push(parse_time_tag(&chunk[0])?);
        let x = as_f64_strict(&chunk[1])?;
        let y = as_f64_strict(&chunk[2])?;
        let z = as_f64_strict(&chunk[3])?;
        let c = if normalize { Cartesian3::new(x, y, z).normalize() } else { Cartesian3::new(x, y, z) };
        out.push(PackedSampleValue::Number(c.x));
        out.push(PackedSampleValue::Number(c.y));
        out.push(PackedSampleValue::Number(c.z));
        for extra in &chunk[4..] {
            out.push(PackedSampleValue::Number(as_f64_strict(extra)?));
        }
    }
    Ok(UnwrappedCartesian::Sampled(out))
}

fn unwrap_quaternion_interval(czml_interval: &Value) -> Result<UnwrappedInterval, RuntimeError> {
    let Some(arr) = get_defined(czml_interval, "unitQuaternion").and_then(Value::as_array) else {
        return Ok(UnwrappedInterval::None);
    };
    if arr.len() == Quaternion::PACKED_LENGTH {
        let nums = parse_f64_slice(arr)?;
        let q = Quaternion::unpack(&nums, 0).normalize();
        return Ok(UnwrappedInterval::Constant(DynamicValue::Quaternion(q)));
    }
    if !arr.len().is_multiple_of(5) {
        return Err(RuntimeError::new("Invalid unitQuaternion sample array length."));
    }
    let mut out = Vec::with_capacity(arr.len());
    for chunk in arr.chunks(5) {
        out.push(parse_time_tag(&chunk[0])?);
        let q = Quaternion::new(
            as_f64_strict(&chunk[1])?,
            as_f64_strict(&chunk[2])?,
            as_f64_strict(&chunk[3])?,
            as_f64_strict(&chunk[4])?,
        )
        .normalize();
        out.push(PackedSampleValue::Number(q.x));
        out.push(PackedSampleValue::Number(q.y));
        out.push(PackedSampleValue::Number(q.z));
        out.push(PackedSampleValue::Number(q.w));
    }
    Ok(UnwrappedInterval::Sampled(out))
}

fn unwrap_enum_interval(czml_interval: &Value, keys: &[&str], parse: fn(&str) -> Option<f64>) -> UnwrappedInterval {
    let raw = keys.iter().find_map(|k| get_defined(czml_interval, k)).unwrap_or(czml_interval);
    match raw.as_str().and_then(parse) {
        Some(n) => UnwrappedInterval::Constant(DynamicValue::Number(n)),
        None => UnwrappedInterval::None,
    }
}

pub(crate) fn unwrap_interval(
    ty: CzmlValueType,
    czml_interval: &Value,
    source_uri: Option<&Resource>,
) -> Result<UnwrappedInterval, RuntimeError> {
    match ty {
        CzmlValueType::ArcType => {
            Ok(unwrap_enum_interval(czml_interval, &["arcType"], |s| parse_arc_type(s).map(|v| v as i32 as f64)))
        }
        CzmlValueType::Array => match get_defined(czml_interval, "array") {
            Some(Value::Array(items)) => {
                Ok(UnwrappedInterval::Constant(DynamicValue::Json(JsonReference::array(items.clone()))))
            }
            Some(other) => Ok(UnwrappedInterval::Constant(DynamicValue::from_json(other.clone()))),
            None => Ok(UnwrappedInterval::None),
        },
        CzmlValueType::Boolean => {
            let raw = get_defined(czml_interval, "boolean").unwrap_or(czml_interval);
            match raw.as_bool() {
                Some(b) => Ok(UnwrappedInterval::Constant(DynamicValue::Boolean(b))),
                None => Ok(UnwrappedInterval::None),
            }
        }
        CzmlValueType::BoundingRectangle => {
            let Some(arr) = get_defined(czml_interval, "boundingRectangle").and_then(Value::as_array) else {
                return Ok(UnwrappedInterval::None);
            };
            if arr.len() == BoundingRectangle::PACKED_LENGTH {
                let nums = parse_f64_slice(arr)?;
                Ok(UnwrappedInterval::Constant(bounding_rect_to_dynamic(BoundingRectangle::unpack(&nums, 0))))
            } else {
                Ok(UnwrappedInterval::Sampled(parse_packed_numbers(arr, BoundingRectangle::PACKED_LENGTH)?))
            }
        }
        CzmlValueType::Cartesian2 => {
            let Some(arr) = get_defined(czml_interval, "cartesian2").and_then(Value::as_array) else {
                return Ok(UnwrappedInterval::None);
            };
            if arr.len() == Cartesian2::PACKED_LENGTH {
                let nums = parse_f64_slice(arr)?;
                Ok(UnwrappedInterval::Constant(DynamicValue::Cartesian2(Cartesian2::unpack(&nums, 0))))
            } else {
                Ok(UnwrappedInterval::Sampled(parse_packed_numbers(arr, Cartesian2::PACKED_LENGTH)?))
            }
        }
        CzmlValueType::Cartesian3 => match unwrap_cartesian_interval(czml_interval, false)? {
            UnwrappedCartesian::Constant(c) => Ok(UnwrappedInterval::Constant(DynamicValue::Cartesian3(c))),
            UnwrappedCartesian::Sampled(s) => Ok(UnwrappedInterval::Sampled(s)),
        },
        CzmlValueType::UnitCartesian3 => match unwrap_cartesian_interval(czml_interval, true)? {
            UnwrappedCartesian::Constant(c) => Ok(UnwrappedInterval::Constant(DynamicValue::Cartesian3(c))),
            UnwrappedCartesian::Sampled(s) => Ok(UnwrappedInterval::Sampled(s)),
        },
        CzmlValueType::Color => unwrap_color_interval(czml_interval),
        CzmlValueType::ClassificationType => Ok(unwrap_enum_interval(czml_interval, &["classificationType"], |s| {
            parse_classification_type(s).map(|v| v as i32 as f64)
        })),
        CzmlValueType::ColorBlendMode => Ok(unwrap_enum_interval(czml_interval, &["colorBlendMode"], |s| {
            parse_color_blend_mode(s).map(|v| v as i32 as f64)
        })),
        CzmlValueType::CornerType => {
            Ok(unwrap_enum_interval(czml_interval, &["cornerType"], |s| parse_corner_type(s).map(|v| v as i32 as f64)))
        }
        CzmlValueType::HeightReference => Ok(unwrap_enum_interval(czml_interval, &["heightReference"], |s| {
            parse_height_reference(s).map(|v| v as i32 as f64)
        })),
        CzmlValueType::HorizontalOrigin => Ok(unwrap_enum_interval(czml_interval, &["horizontalOrigin"], |s| {
            parse_horizontal_origin(s).map(|v| v as i32 as f64)
        })),
        CzmlValueType::Image | CzmlValueType::Uri => match unwrap_uri_interval(czml_interval, source_uri) {
            Some(v) => Ok(UnwrappedInterval::Constant(v)),
            None => Ok(UnwrappedInterval::None),
        },
        CzmlValueType::JulianDate => {
            let raw = get_defined(czml_interval, "date").unwrap_or(czml_interval);
            let Some(s) = raw.as_str() else { return Ok(UnwrappedInterval::None) };
            let date = JulianDate::from_iso8601(s).map_err(|e| RuntimeError::new(e.to_string()))?;
            Ok(UnwrappedInterval::Constant(DynamicValue::Date(Rc::new(date))))
        }
        CzmlValueType::LabelStyle => {
            Ok(unwrap_enum_interval(czml_interval, &["labelStyle"], |s| parse_label_style(s).map(|v| v as i32 as f64)))
        }
        CzmlValueType::Number | CzmlValueType::Rotation => {
            let raw = get_defined(czml_interval, "number").unwrap_or(czml_interval);
            if let Some(n) = raw.as_f64() {
                Ok(UnwrappedInterval::Constant(DynamicValue::Number(n)))
            } else if let Some(arr) = raw.as_array() {
                if arr.len() == 1 {
                    Ok(UnwrappedInterval::Constant(DynamicValue::Number(as_f64_strict(&arr[0])?)))
                } else if arr.len() > 1 {
                    Ok(UnwrappedInterval::Sampled(parse_packed_numbers(arr, 1)?))
                } else {
                    Ok(UnwrappedInterval::None)
                }
            } else {
                Ok(UnwrappedInterval::None)
            }
        }
        CzmlValueType::NearFarScalar => {
            let Some(arr) = get_defined(czml_interval, "nearFarScalar").and_then(Value::as_array) else {
                return Ok(UnwrappedInterval::None);
            };
            if arr.len() == NearFarScalar::PACKED_LENGTH {
                let nums = parse_f64_slice(arr)?;
                Ok(UnwrappedInterval::Constant(near_far_to_dynamic(NearFarScalar::unpack(&nums, 0))))
            } else {
                Ok(UnwrappedInterval::Sampled(parse_packed_numbers(arr, NearFarScalar::PACKED_LENGTH)?))
            }
        }
        CzmlValueType::DistanceDisplayCondition => {
            let Some(arr) = get_defined(czml_interval, "distanceDisplayCondition").and_then(Value::as_array) else {
                return Ok(UnwrappedInterval::None);
            };
            if arr.len() == DistanceDisplayCondition::PACKED_LENGTH {
                let nums = parse_f64_slice(arr)?;
                Ok(UnwrappedInterval::Constant(distance_display_to_dynamic(DistanceDisplayCondition::unpack(&nums, 0))))
            } else {
                Ok(UnwrappedInterval::Sampled(parse_packed_numbers(arr, DistanceDisplayCondition::PACKED_LENGTH)?))
            }
        }
        CzmlValueType::Object => {
            let raw = get_defined(czml_interval, "object")
                .or_else(|| get_defined(czml_interval, "value"))
                .unwrap_or(czml_interval);
            Ok(UnwrappedInterval::Constant(DynamicValue::from_json(raw.clone())))
        }
        CzmlValueType::Quaternion => unwrap_quaternion_interval(czml_interval),
        CzmlValueType::SensorVolumePortionToDisplay => {
            Ok(unwrap_enum_interval(czml_interval, &["portionToDisplay"], |s| {
                parse_sensor_volume_portion(s).map(|v| v as i32 as f64)
            }))
        }
        CzmlValueType::ShadowMode => Ok(unwrap_enum_interval(czml_interval, &["shadowMode", "shadows"], |s| {
            parse_shadow_mode(s).map(|v| v as i32 as f64)
        })),
        CzmlValueType::String => {
            let raw = get_defined(czml_interval, "string").unwrap_or(czml_interval);
            match raw.as_str() {
                Some(s) => Ok(UnwrappedInterval::Constant(DynamicValue::String(s.to_owned()))),
                None => Ok(UnwrappedInterval::None),
            }
        }
        CzmlValueType::StripeOrientation => Ok(unwrap_enum_interval(czml_interval, &["stripeOrientation"], |s| {
            parse_stripe_orientation(s).map(|v| v as i32 as f64)
        })),
        CzmlValueType::Rectangle => unwrap_rectangle_interval(czml_interval),
        CzmlValueType::VerticalOrigin => Ok(unwrap_enum_interval(czml_interval, &["verticalOrigin"], |s| {
            parse_vertical_origin(s).map(|v| v as i32 as f64)
        })),
    }
}

pub(crate) fn update_interpolation_settings<T: SampledValue>(packet_data: &Value, property: &SampledProperty<T>) {
    let alg = get_defined(packet_data, "interpolationAlgorithm")
        .and_then(Value::as_str)
        .and_then(parse_interpolation_algorithm);
    let degree = get_defined(packet_data, "interpolationDegree").and_then(Value::as_f64);
    if alg.is_some() || degree.is_some() {
        property.set_interpolation_options(alg, degree);
    }

    if let Some(ext) =
        get_defined(packet_data, "forwardExtrapolationType").and_then(Value::as_str).and_then(parse_extrapolation_type)
    {
        property.set_forward_extrapolation_type(ext);
    }
    if let Some(dur) = get_defined(packet_data, "forwardExtrapolationDuration").and_then(Value::as_f64) {
        property.set_forward_extrapolation_duration(dur);
    }
    if let Some(ext) =
        get_defined(packet_data, "backwardExtrapolationType").and_then(Value::as_str).and_then(parse_extrapolation_type)
    {
        property.set_backward_extrapolation_type(ext);
    }
    if let Some(dur) = get_defined(packet_data, "backwardExtrapolationDuration").and_then(Value::as_f64) {
        property.set_backward_extrapolation_duration(dur);
    }
}

// ---------------------------------------------------------------------------
// Safe array unpackers (never panic on empty or malformed arrays)
// ---------------------------------------------------------------------------

pub(crate) fn unpack_cartesian2_values(arr: &[Value]) -> Result<Vec<Cartesian2>, RuntimeError> {
    if arr.is_empty() {
        return Ok(Vec::new());
    }
    if !arr.len().is_multiple_of(2) {
        return Err(RuntimeError::new("cartesian2 array length must be a multiple of 2."));
    }
    let nums = parse_f64_slice(arr)?;
    Ok(Cartesian2::unpack_array(&nums))
}

pub(crate) fn unpack_cartesian3_values(arr: &[Value]) -> Result<Vec<Cartesian3>, RuntimeError> {
    if arr.is_empty() {
        return Ok(Vec::new());
    }
    if !arr.len().is_multiple_of(3) {
        return Err(RuntimeError::new("cartesian array length must be a multiple of 3."));
    }
    let nums = parse_f64_slice(arr)?;
    Ok(Cartesian3::unpack_array(&nums))
}

pub(crate) fn unpack_cartographic_radians_values(arr: &[Value]) -> Result<Vec<Cartesian3>, RuntimeError> {
    if arr.is_empty() {
        return Ok(Vec::new());
    }
    if !arr.len().is_multiple_of(3) {
        return Err(RuntimeError::new("cartographicRadians array length must be a multiple of 3."));
    }
    let nums = parse_f64_slice(arr)?;
    Ok(Cartesian3::from_radians_array_heights(&nums, &Ellipsoid::WGS84))
}

pub(crate) fn unpack_cartographic_degrees_values(arr: &[Value]) -> Result<Vec<Cartesian3>, RuntimeError> {
    if arr.is_empty() {
        return Ok(Vec::new());
    }
    if !arr.len().is_multiple_of(3) {
        return Err(RuntimeError::new("cartographicDegrees array length must be a multiple of 3."));
    }
    let nums = parse_f64_slice(arr)?;
    Ok(Cartesian3::from_degrees_array_heights(&nums, &Ellipsoid::WGS84))
}
