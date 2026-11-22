//! Shared helpers for evaluating graphics property slots into concrete scene values.
use super::{DynamicValue, EntityComponent, PropertyInput};
use crate::{
    core::{
        BoundingRectangle, Cartesian2, Cartesian3, Color, DistanceDisplayCondition, JulianDate, NearFarScalar,
        Quaternion, Resource, RuntimeError, TranslationRotationScale,
    },
    scene::{
        ColorBlendMode, HeightReference, HorizontalOrigin, LabelStyle, ShadowMode, SplitDirection, VerticalOrigin,
    },
};
use serde_json::Value;
use std::rc::Rc;

pub(crate) fn eval_dynamic(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<DynamicValue>, RuntimeError> {
    let Some(slot) = slot else { return Ok(None) };
    let val = match slot {
        PropertyInput::Value(v) => Some(v.clone()),
        PropertyInput::Component(c) => c.get_value(time, None)?,
    };
    Ok(val.filter(|v| !matches!(v, DynamicValue::Null)))
}

pub(crate) fn is_slot_constant(slot: Option<&PropertyInput>) -> bool {
    match slot {
        None | Some(PropertyInput::Value(_)) => true,
        Some(PropertyInput::Component(c)) => c.is_constant(),
    }
}

pub(crate) fn eval_bool(slot: Option<&PropertyInput>, time: &JulianDate, default: bool) -> Result<bool, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Boolean(b)) => b,
        _ => default,
    })
}

pub(crate) fn eval_f64_opt(slot: Option<&PropertyInput>, time: &JulianDate) -> Result<Option<f64>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => Some(n),
        _ => None,
    })
}

pub(crate) fn eval_f64(slot: Option<&PropertyInput>, time: &JulianDate, default: f64) -> Result<f64, RuntimeError> {
    Ok(eval_f64_opt(slot, time)?.unwrap_or(default))
}

pub(crate) fn eval_string_opt(slot: Option<&PropertyInput>, time: &JulianDate) -> Result<Option<String>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::String(s)) => Some(s),
        _ => None,
    })
}

pub(crate) fn eval_string(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: &str,
) -> Result<String, RuntimeError> {
    Ok(eval_string_opt(slot, time)?.unwrap_or_else(|| default.to_owned()))
}

pub(crate) fn eval_color_opt(slot: Option<&PropertyInput>, time: &JulianDate) -> Result<Option<Color>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(v) => dynamic_to_color(&v),
        None => None,
    })
}

pub(crate) fn eval_color(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: Color,
) -> Result<Color, RuntimeError> {
    Ok(eval_color_opt(slot, time)?.unwrap_or(default))
}

pub(crate) fn dynamic_to_color(v: &DynamicValue) -> Option<Color> {
    match v {
        DynamicValue::Color(c) => Some(*c),
        DynamicValue::Json(j) => json_to_color(&j.value()),
        _ => None,
    }
}

fn json_to_color(v: &Value) -> Option<Color> {
    if let (Some(r), Some(g), Some(b)) = (
        v.get("red").and_then(Value::as_f64),
        v.get("green").and_then(Value::as_f64),
        v.get("blue").and_then(Value::as_f64),
    ) {
        let a = v.get("alpha").and_then(Value::as_f64).unwrap_or(1.0);
        return Some(Color::new(r, g, b, a));
    }
    None
}

pub(crate) fn eval_cartesian2_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<Cartesian2>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Cartesian2(v)) => Some(v),
        Some(DynamicValue::Json(j)) => {
            let v = j.value();
            match (v.get("x").and_then(Value::as_f64), v.get("y").and_then(Value::as_f64)) {
                (Some(x), Some(y)) => Some(Cartesian2::new(x, y)),
                _ => None,
            }
        }
        _ => None,
    })
}

pub(crate) fn eval_cartesian2(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: Cartesian2,
) -> Result<Cartesian2, RuntimeError> {
    Ok(eval_cartesian2_opt(slot, time)?.unwrap_or(default))
}

pub(crate) fn eval_cartesian3_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<Cartesian3>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(v) => dynamic_to_cartesian3(&v),
        None => None,
    })
}

pub(crate) fn eval_cartesian3(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: Cartesian3,
) -> Result<Cartesian3, RuntimeError> {
    Ok(eval_cartesian3_opt(slot, time)?.unwrap_or(default))
}

pub(crate) fn dynamic_to_cartesian3(v: &DynamicValue) -> Option<Cartesian3> {
    match v {
        DynamicValue::Cartesian3(c) => Some(*c),
        DynamicValue::Json(j) => {
            let v = j.value();
            match (
                v.get("x").and_then(Value::as_f64),
                v.get("y").and_then(Value::as_f64),
                v.get("z").and_then(Value::as_f64),
            ) {
                (Some(x), Some(y), Some(z)) => Some(Cartesian3::new(x, y, z)),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(crate) fn dynamic_to_quaternion(v: &DynamicValue) -> Option<Quaternion> {
    match v {
        DynamicValue::Quaternion(q) => Some(*q),
        DynamicValue::Json(j) => {
            let v = j.value();
            match (
                v.get("x").and_then(Value::as_f64),
                v.get("y").and_then(Value::as_f64),
                v.get("z").and_then(Value::as_f64),
                v.get("w").and_then(Value::as_f64),
            ) {
                (Some(x), Some(y), Some(z), Some(w)) => Some(Quaternion::new(x, y, z, w)),
                _ => None,
            }
        }
        _ => None,
    }
}

pub(crate) fn dynamic_to_trs(v: &DynamicValue) -> Option<TranslationRotationScale> {
    match v {
        DynamicValue::Record(fields) => {
            let mut trs = TranslationRotationScale::default();
            for (k, val) in fields {
                let Some(val) = val else { continue };
                match k.as_str() {
                    "translation" => {
                        if let Some(c) = dynamic_to_cartesian3(val) {
                            trs.translation = c;
                        }
                    }
                    "rotation" => {
                        if let Some(q) = dynamic_to_quaternion(val) {
                            trs.rotation = q;
                        }
                    }
                    "scale" => {
                        if let Some(c) = dynamic_to_cartesian3(val) {
                            trs.scale = c;
                        }
                    }
                    _ => {}
                }
            }
            Some(trs)
        }
        DynamicValue::Json(j) => {
            let val = j.value();
            let obj = val.as_object()?;
            let mut trs = TranslationRotationScale::default();
            if let Some(t) = obj.get("translation") {
                if let Some(c) = dynamic_to_cartesian3(&DynamicValue::from_json(t.clone())) {
                    trs.translation = c;
                }
            }
            if let Some(r) = obj.get("rotation") {
                if let Some(q) = dynamic_to_quaternion(&DynamicValue::from_json(r.clone())) {
                    trs.rotation = q;
                }
            }
            if let Some(s) = obj.get("scale") {
                if let Some(c) = dynamic_to_cartesian3(&DynamicValue::from_json(s.clone())) {
                    trs.scale = c;
                }
            }
            Some(trs)
        }
        _ => None,
    }
}

pub(crate) fn eval_near_far_scalar_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<NearFarScalar>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Json(j)) => {
            let v = j.value();
            match (
                v.get("near").and_then(Value::as_f64),
                v.get("nearValue").and_then(Value::as_f64),
                v.get("far").and_then(Value::as_f64),
                v.get("farValue").and_then(Value::as_f64),
            ) {
                (Some(near), Some(near_value), Some(far), Some(far_value)) => {
                    Some(NearFarScalar::new(near, near_value, far, far_value))
                }
                _ => None,
            }
        }
        _ => None,
    })
}

pub(crate) fn eval_distance_display_condition_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<DistanceDisplayCondition>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Json(j)) => {
            let v = j.value();
            match (v.get("near").and_then(Value::as_f64), v.get("far").and_then(Value::as_f64)) {
                (Some(near), Some(far)) => Some(DistanceDisplayCondition::new(near, far)),
                _ => None,
            }
        }
        _ => None,
    })
}

pub(crate) fn eval_bounding_rectangle_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<BoundingRectangle>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Json(j)) => {
            let v = j.value();
            match (
                v.get("x").and_then(Value::as_f64),
                v.get("y").and_then(Value::as_f64),
                v.get("width").and_then(Value::as_f64),
                v.get("height").and_then(Value::as_f64),
            ) {
                (Some(x), Some(y), Some(width), Some(height)) => Some(BoundingRectangle::new(x, y, width, height)),
                _ => None,
            }
        }
        _ => None,
    })
}

pub(crate) fn eval_resource_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<Resource>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Resource(r)) => Some((*r).clone()),
        Some(DynamicValue::String(s)) => Resource::new(s),
        _ => None,
    })
}

pub(crate) fn dynamic_to_height_reference(v: &DynamicValue) -> Option<HeightReference> {
    let n = match v {
        DynamicValue::Number(n) => *n as i32,
        _ => return None,
    };
    match n {
        x if x == HeightReference::None as i32 => Some(HeightReference::None),
        x if x == HeightReference::ClampToGround as i32 => Some(HeightReference::ClampToGround),
        x if x == HeightReference::RelativeToGround as i32 => Some(HeightReference::RelativeToGround),
        x if x == HeightReference::ClampToTerrain as i32 => Some(HeightReference::ClampToTerrain),
        x if x == HeightReference::RelativeToTerrain as i32 => Some(HeightReference::RelativeToTerrain),
        x if x == HeightReference::ClampTo3DTile as i32 => Some(HeightReference::ClampTo3DTile),
        x if x == HeightReference::RelativeTo3DTile as i32 => Some(HeightReference::RelativeTo3DTile),
        _ => None,
    }
}

pub(crate) fn eval_height_reference(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: HeightReference,
) -> Result<HeightReference, RuntimeError> {
    Ok(eval_dynamic(slot, time)?.as_ref().and_then(dynamic_to_height_reference).unwrap_or(default))
}

pub(crate) fn eval_horizontal_origin(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: HorizontalOrigin,
) -> Result<HorizontalOrigin, RuntimeError> {
    let n = match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => n as i32,
        _ => return Ok(default),
    };
    Ok(match n {
        x if x == HorizontalOrigin::Center as i32 => HorizontalOrigin::Center,
        x if x == HorizontalOrigin::Left as i32 => HorizontalOrigin::Left,
        x if x == HorizontalOrigin::Right as i32 => HorizontalOrigin::Right,
        _ => default,
    })
}

pub(crate) fn eval_vertical_origin(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: VerticalOrigin,
) -> Result<VerticalOrigin, RuntimeError> {
    let n = match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => n as i32,
        _ => return Ok(default),
    };
    Ok(match n {
        x if x == VerticalOrigin::Center as i32 => VerticalOrigin::Center,
        x if x == VerticalOrigin::Bottom as i32 => VerticalOrigin::Bottom,
        x if x == VerticalOrigin::Baseline as i32 => VerticalOrigin::Baseline,
        x if x == VerticalOrigin::Top as i32 => VerticalOrigin::Top,
        _ => default,
    })
}

pub(crate) fn eval_label_style(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: LabelStyle,
) -> Result<LabelStyle, RuntimeError> {
    let n = match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => n as i32,
        _ => return Ok(default),
    };
    Ok(match n {
        x if x == LabelStyle::Fill as i32 => LabelStyle::Fill,
        x if x == LabelStyle::Outline as i32 => LabelStyle::Outline,
        x if x == LabelStyle::FillAndOutline as i32 => LabelStyle::FillAndOutline,
        _ => default,
    })
}

pub(crate) fn eval_split_direction(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: SplitDirection,
) -> Result<SplitDirection, RuntimeError> {
    let n = match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => n as i32,
        _ => return Ok(default),
    };
    Ok(match n {
        x if x == SplitDirection::Left as i32 => SplitDirection::Left,
        x if x == SplitDirection::None as i32 => SplitDirection::None,
        x if x == SplitDirection::Right as i32 => SplitDirection::Right,
        _ => default,
    })
}

pub(crate) fn eval_shadow_mode(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: ShadowMode,
) -> Result<ShadowMode, RuntimeError> {
    let n = match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => n as i32,
        _ => return Ok(default),
    };
    Ok(match n {
        x if x == ShadowMode::Disabled as i32 => ShadowMode::Disabled,
        x if x == ShadowMode::Enabled as i32 => ShadowMode::Enabled,
        x if x == ShadowMode::CastOnly as i32 => ShadowMode::CastOnly,
        x if x == ShadowMode::ReceiveOnly as i32 => ShadowMode::ReceiveOnly,
        _ => default,
    })
}

pub(crate) fn eval_color_blend_mode(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: ColorBlendMode,
) -> Result<ColorBlendMode, RuntimeError> {
    let n = match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => n as i32,
        _ => return Ok(default),
    };
    Ok(match n {
        x if x == ColorBlendMode::Highlight as i32 => ColorBlendMode::Highlight,
        x if x == ColorBlendMode::Replace as i32 => ColorBlendMode::Replace,
        x if x == ColorBlendMode::Mix as i32 => ColorBlendMode::Mix,
        _ => default,
    })
}

pub(crate) fn slot_component(slot: Option<PropertyInput>) -> Option<Rc<dyn EntityComponent>> {
    match slot {
        Some(PropertyInput::Component(c)) => Some(c),
        _ => None,
    }
}
