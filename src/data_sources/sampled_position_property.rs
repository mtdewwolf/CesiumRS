use super::{position_property::*, DefinitionChanged, Property, SampleType, SampledProperty};
use crate::core::*;
use std::{any::Any, ops::Deref};
pub struct SampledPositionProperty {
    value: SampledProperty<Cartesian3>,
    frame: ReferenceFrame,
    context: PositionContext,
    derivatives: usize,
}
impl SampledPositionProperty {
    pub fn new(frame: ReferenceFrame, derivatives: usize) -> Self {
        Self::with_context(frame, derivatives, PositionContext::default())
    }
    pub fn with_context(frame: ReferenceFrame, derivatives: usize, context: PositionContext) -> Self {
        Self {
            value: SampledProperty::with_clock(
                (derivatives > 0).then(|| vec![SampleType::of::<Cartesian3>(); derivatives]),
                context.clock.clone(),
            ),
            frame,
            context,
            derivatives,
        }
    }
    pub fn number_of_derivatives(&self) -> usize {
        self.derivatives
    }
    pub fn add_sample(
        &self,
        time: JulianDate,
        position: &Cartesian3,
        derivatives: Option<&[Cartesian3]>,
    ) -> Result<(), RuntimeError> {
        let packed = derivatives.map(|d| d.iter().map(|v| v.to_array().to_vec()).collect::<Vec<_>>());
        self.value.add_sample(time, position, packed.as_deref())
    }
    pub fn add_samples(
        &self,
        times: &[JulianDate],
        positions: &[Cartesian3],
        derivatives: Option<&[Vec<Cartesian3>]>,
    ) -> Result<(), RuntimeError> {
        let packed = derivatives.map(|d| {
            d.iter().map(|row| row.iter().map(|v| v.to_array().to_vec()).collect::<Vec<_>>()).collect::<Vec<_>>()
        });
        self.value.add_samples(times, positions, packed.as_deref())
    }
}
/// Sample management, interpolation and extrapolation settings delegate to SampledProperty.
impl Deref for SampledPositionProperty {
    type Target = SampledProperty<Cartesian3>;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
impl PositionProperty for SampledPositionProperty {
    fn reference_frame(&self) -> ReferenceFrame {
        self.frame
    }
    fn get_value_in_reference_frame(
        &self,
        time: &JulianDate,
        frame: ReferenceFrame,
        mut result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        let value = self.value.get_value(Some(time), result.as_deref_mut())?;
        self.context.convert(time, value, FrameConversion { input: self.frame, output: frame }, result)
    }
}
impl Property<Cartesian3> for SampledPositionProperty {
    fn is_constant(&self) -> bool {
        self.value.is_constant()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.value.definition_changed()
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        self.get_value_in_reference_frame(&self.context.time(time), ReferenceFrame::Fixed, result)
    }
    fn equals(&self, other: &dyn Property<Cartesian3>) -> Result<bool, RuntimeError> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        Ok(self.frame == o.frame && self.value.equals(&o.value)?)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for SampledPositionProperty {
    fn default() -> Self {
        Self::new(ReferenceFrame::Fixed, 0)
    }
}
