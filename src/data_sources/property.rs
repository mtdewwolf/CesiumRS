//! Typed Property contract and the pinned Property static helpers.
use super::{CloneablePropertyValue, DefinitionChanged, PropertyValue};
use crate::core::{JulianDate, RuntimeError};
use std::{any::Any, rc::Rc};
pub type PropertyRef<T> = Rc<dyn Property<T>>;
/// Supplies omitted times; use an injected clock for deterministic or bare-WASM hosts.
pub type PropertyClock = Rc<dyn Fn() -> JulianDate>;
pub fn system_clock() -> PropertyClock {
    #[cfg(all(feature = "browser", target_arch = "wasm32"))]
    {
        Rc::new(|| JulianDate::from_date(js_sys::Date::now()))
    }
    #[cfg(not(all(feature = "browser", target_arch = "wasm32")))]
    {
        Rc::new(JulianDate::now)
    }
}
/// A typed, optionally undefined value evaluated at a simulation time.
/// `Option<T>` as the value type additionally represents explicit JavaScript null.
pub trait Property<T>: Any {
    fn is_constant(&self) -> bool;
    fn definition_changed(&self) -> &DefinitionChanged;
    fn get_value(&self, time: Option<&JulianDate>, result: Option<&mut T>) -> Result<Option<T>, RuntimeError>;
    fn equals(&self, other: &dyn Property<T>) -> Result<bool, RuntimeError>;
    fn as_any(&self) -> &dyn Any;
}
pub fn equals<T: 'static>(left: Option<&PropertyRef<T>>, right: Option<&PropertyRef<T>>) -> Result<bool, RuntimeError> {
    match (left, right) {
        (None, None) => Ok(true),
        (Some(a), Some(b)) => {
            if Rc::ptr_eq(a, b) {
                Ok(true)
            } else {
                a.equals(&**b)
            }
        }
        _ => Ok(false),
    }
}
pub fn array_equals<T: 'static>(
    left: Option<&[PropertyRef<T>]>,
    right: Option<&[PropertyRef<T>]>,
) -> Result<bool, RuntimeError> {
    match (left, right) {
        (None, None) => Ok(true),
        (Some(a), Some(b)) => {
            if a.len() != b.len() {
                return Ok(false);
            }
            for (a, b) in a.iter().zip(b) {
                if !equals(Some(a), Some(b))? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}
pub fn is_constant<T: 'static>(property: Option<&dyn Property<T>>) -> bool {
    property.is_none_or(Property::is_constant)
}
pub fn get_value_or_undefined<T: 'static>(
    property: Option<&dyn Property<T>>,
    time: Option<&JulianDate>,
    result: Option<&mut T>,
) -> Result<Option<T>, RuntimeError> {
    property.map_or(Ok(None), |p| p.get_value(time, result))
}
pub fn get_value_or_default<T: PropertyValue>(
    property: Option<&dyn Property<T>>,
    time: Option<&JulianDate>,
    default: T,
    result: Option<&mut T>,
) -> Result<T, RuntimeError> {
    Ok(get_value_or_undefined(property, time, result)?.filter(PropertyValue::is_defined).unwrap_or(default))
}
pub fn get_value_or_cloned_default<T: CloneablePropertyValue>(
    property: Option<&dyn Property<T>>,
    time: Option<&JulianDate>,
    default: &T,
    result: Option<&mut T>,
) -> Result<T, RuntimeError> {
    // On fallback the JS helper clones into the undefined returned value,
    // rather than into the caller's original result object.
    Ok(get_value_or_undefined(property, time, result)?
        .filter(PropertyValue::is_defined)
        .unwrap_or_else(|| default.clone()))
}
pub(crate) fn write_result<T: Clone>(value: Option<T>, result: Option<&mut T>) -> Option<T> {
    if let (Some(v), Some(out)) = (&value, result) {
        out.clone_from(v);
    }
    value
}
