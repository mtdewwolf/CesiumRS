//! Positions with explicit reference frames and host-owned time/transform data.
use super::{
    property::{system_clock, write_result},
    DefinitionChanged, Property, PropertyClock, Subscription,
};
use crate::core::*;
use std::{cell::RefCell, rc::Rc};
pub type PositionPropertyRef = Rc<dyn PositionProperty>;
pub type FrameTransform = Rc<dyn Fn(&JulianDate) -> Result<Matrix3, RuntimeError>>;
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameConversion {
    pub input: ReferenceFrame,
    pub output: ReferenceFrame,
}
/// Shared host context. The default matches Cesium's Earth/TEME fallback while
/// IAU data is unavailable. Supply Earth data or a central-body transform for ICRF.
#[derive(Clone)]
pub struct PositionContext {
    pub clock: PropertyClock,
    pub frame_transform: FrameTransform,
    pub ellipsoid: Rc<dyn Fn() -> Ellipsoid>,
}
impl Default for PositionContext {
    fn default() -> Self {
        Self {
            clock: system_clock(),
            frame_transform: Rc::new(|time| Ok(Transforms::compute_teme_to_pseudo_fixed_matrix(time))),
            ellipsoid: Rc::new(|| Ellipsoid::WGS84),
        }
    }
}
impl PositionContext {
    pub fn with_earth_data(eop: Rc<EarthOrientationParameters>, xys: Rc<RefCell<Iau2006XysData>>) -> Self {
        Self {
            frame_transform: Rc::new(move |time| {
                Ok(Transforms::compute_icrf_to_central_body_fixed_matrix(time, &eop, &xys.borrow()))
            }),
            ..Self::default()
        }
    }
    pub fn time(&self, time: Option<&JulianDate>) -> JulianDate {
        time.copied().unwrap_or_else(|| (self.clock)())
    }
    pub fn convert(
        &self,
        time: &JulianDate,
        value: Option<Cartesian3>,
        frames: FrameConversion,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        let Some(value) = value else { return Ok(None) };
        let value = if frames.input == frames.output {
            value
        } else {
            let matrix = (self.frame_transform)(time)?;
            if frames.input == ReferenceFrame::Inertial {
                matrix.multiply_by_vector(value)
            } else {
                matrix.transpose().multiply_by_vector(value)
            }
        };
        Ok(write_result(Some(value), result))
    }
}
pub fn convert_to_reference_frame(
    time: &JulianDate,
    value: Option<Cartesian3>,
    frames: FrameConversion,
    result: Option<&mut Cartesian3>,
) -> Result<Option<Cartesian3>, RuntimeError> {
    PositionContext::default().convert(time, value, frames, result)
}
pub trait PositionProperty: Property<Cartesian3> {
    fn reference_frame(&self) -> ReferenceFrame;
    fn get_value_in_reference_frame(
        &self,
        time: &JulianDate,
        reference_frame: ReferenceFrame,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError>;
}
pub(crate) fn forward(event: &DefinitionChanged, changed: &DefinitionChanged) -> Subscription {
    let weak = changed.downgrade();
    event.add_listener(move |_| {
        if let Some(event) = weak.upgrade() {
            event.raise();
        }
    })
}
pub(crate) fn same_ref<T: ?Sized>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}
pub(crate) fn position_equals(
    a: Option<&PositionPropertyRef>,
    b: Option<&PositionPropertyRef>,
) -> Result<bool, RuntimeError> {
    match (a, b) {
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
