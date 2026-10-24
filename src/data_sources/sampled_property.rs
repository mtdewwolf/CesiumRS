//! SampledProperty's packed sample ordering, interpolation and extrapolation.
//! Sample codecs reuse Core packing and quaternion interpolation transforms.
use super::{
    property::{system_clock, write_result},
    DefinitionChanged, Property, PropertyClock, PropertyValue,
};
use crate::core::{
    interpolation_algorithm::{
        HermitePolynomialApproximation, InterpolationAlgorithm, InterpolationSamples, LagrangePolynomialApproximation,
        LinearApproximation,
    },
    *,
};
use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    cmp::Ordering,
    rc::Rc,
};
type Result<T> = std::result::Result<T, RuntimeError>;
/// A packed value codec. Derivative samples use the matching packed numeric rows.
pub trait SampledValue: PropertyValue {
    const PACKED_LENGTH: usize;
    const PACKED_INTERPOLATION_LENGTH: usize = Self::PACKED_LENGTH;
    const TYPE_NAME: &'static str;
    fn pack(&self, out: &mut [f64], offset: usize);
    fn unpack(array: &[f64], offset: usize) -> Self;
    fn convert_for_interpolation(_values: &[f64], _: usize, _: usize) -> Option<Vec<f64>> {
        None
    }
    fn unpack_interpolated(array: &[f64], _: &[f64], _: usize, _: usize) -> Self {
        Self::unpack(array, 0)
    }
}
impl SampledValue for f64 {
    const PACKED_LENGTH: usize = 1;
    const TYPE_NAME: &'static str = "Number";
    fn pack(&self, a: &mut [f64], i: usize) {
        a[i] = *self;
    }
    fn unpack(a: &[f64], i: usize) -> Self {
        a[i]
    }
}
macro_rules! sampled {
    ($($t:ty),*) => {
        $(impl SampledValue for $t {
            const PACKED_LENGTH: usize = <$t as Packable>::PACKED_LENGTH;
            const TYPE_NAME: &'static str = stringify!($t);
            fn pack(&self, array: &mut [f64], offset: usize) {
                Packable::pack(self, array, offset);
            }
            fn unpack(array: &[f64], offset: usize) -> Self {
                Packable::unpack(array, offset)
            }
        })*
    };
}
sampled!(
    Cartesian2,
    Cartesian3,
    Cartesian4,
    Color,
    Matrix2,
    Matrix3,
    Matrix4,
    Rectangle,
    BoundingRectangle,
    BoundingSphere,
    Ellipsoid,
    NearFarScalar,
    DistanceDisplayCondition,
    VertexFormat
);
impl SampledValue for Quaternion {
    const PACKED_LENGTH: usize = 4;
    const PACKED_INTERPOLATION_LENGTH: usize = 3;
    const TYPE_NAME: &'static str = "Quaternion";
    fn pack(&self, a: &mut [f64], i: usize) {
        Packable::pack(self, a, i);
    }
    fn unpack(a: &[f64], i: usize) -> Self {
        Packable::unpack(a, i)
    }
    fn convert_for_interpolation(v: &[f64], a: usize, b: usize) -> Option<Vec<f64>> {
        Some(Self::convert_packed_array_for_interpolation(v, a, b))
    }
    fn unpack_interpolated(a: &[f64], v: &[f64], first: usize, last: usize) -> Self {
        Self::unpack_interpolation_result(a, v, first, last)
    }
}
/// Identity and packing metadata for a sample or derivative type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleType {
    id: TypeId,
    pub name: &'static str,
    pub packed_length: usize,
    pub interpolation_length: usize,
}
impl SampleType {
    pub fn of<T: SampledValue>() -> Self {
        Self {
            id: TypeId::of::<T>(),
            name: T::TYPE_NAME,
            packed_length: T::PACKED_LENGTH,
            interpolation_length: T::PACKED_INTERPOLATION_LENGTH,
        }
    }
}
#[derive(Clone)]
pub enum Interpolation {
    Linear,
    Lagrange,
    Hermite,
    Custom(Rc<dyn InterpolationAlgorithm>),
}
impl Interpolation {
    fn algorithm(&self) -> &dyn InterpolationAlgorithm {
        match self {
            Self::Linear => &LinearApproximation,
            Self::Lagrange => &LagrangePolynomialApproximation,
            Self::Hermite => &HermitePolynomialApproximation,
            Self::Custom(a) => &**a,
        }
    }
    pub fn algorithm_type(&self) -> &str {
        self.algorithm().algorithm_type()
    }
    fn same(&self, o: &Self) -> bool {
        match (self, o) {
            (Self::Linear, Self::Linear) | (Self::Lagrange, Self::Lagrange) | (Self::Hermite, Self::Hermite) => true,
            (Self::Custom(a), Self::Custom(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}
#[derive(Clone, Debug)]
pub enum PackedSampleValue {
    Number(f64),
    Date(JulianDate),
    Iso8601(String),
}
impl From<f64> for PackedSampleValue {
    fn from(v: f64) -> Self {
        Self::Number(v)
    }
}
impl From<JulianDate> for PackedSampleValue {
    fn from(v: JulianDate) -> Self {
        Self::Date(v)
    }
}
struct State {
    points: Cell<Option<usize>>,
    times: Vec<JulianDate>,
    values: Vec<f64>,
    algorithm: Interpolation,
    degree: f64,
    forward: ExtrapolationType,
    backward: ExtrapolationType,
    forward_duration: f64,
    backward_duration: f64,
}
/// Sorted samples with Core interpolation algorithms and explicit extrapolation policies.
pub struct SampledProperty<T: SampledValue> {
    state: RefCell<State>,
    derivative_types: Option<Vec<SampleType>>,
    stride: usize,
    interpolation_stride: usize,
    changed: DefinitionChanged,
    clock: PropertyClock,
    marker: std::marker::PhantomData<T>,
}
impl<T: SampledValue> SampledProperty<T> {
    pub fn new(derivative_types: Option<Vec<SampleType>>) -> Self {
        Self::with_clock(derivative_types, system_clock())
    }
    pub fn with_clock(derivative_types: Option<Vec<SampleType>>, clock: PropertyClock) -> Self {
        let stride = T::PACKED_LENGTH + derivative_types.iter().flatten().map(|t| t.packed_length).sum::<usize>();
        let interpolation_stride = T::PACKED_INTERPOLATION_LENGTH
            + derivative_types.iter().flatten().map(|t| t.interpolation_length).sum::<usize>();
        Self {
            state: RefCell::new(State {
                points: Cell::new(None),
                times: vec![],
                values: vec![],
                algorithm: Interpolation::Linear,
                degree: 1.,
                forward: ExtrapolationType::None,
                backward: ExtrapolationType::None,
                forward_duration: 0.,
                backward_duration: 0.,
            }),
            derivative_types,
            stride,
            interpolation_stride,
            changed: DefinitionChanged::new(),
            clock,
            marker: std::marker::PhantomData,
        }
    }
    pub fn sample_type(&self) -> SampleType {
        SampleType::of::<T>()
    }
    pub fn derivative_types(&self) -> Option<&[SampleType]> {
        self.derivative_types.as_deref()
    }
    pub fn interpolation_degree(&self) -> f64 {
        self.state.borrow().degree
    }
    pub fn interpolation_algorithm(&self) -> Interpolation {
        self.state.borrow().algorithm.clone()
    }
    pub fn forward_extrapolation_type(&self) -> ExtrapolationType {
        self.state.borrow().forward
    }
    pub fn backward_extrapolation_type(&self) -> ExtrapolationType {
        self.state.borrow().backward
    }
    pub fn forward_extrapolation_duration(&self) -> f64 {
        self.state.borrow().forward_duration
    }
    pub fn backward_extrapolation_duration(&self) -> f64 {
        self.state.borrow().backward_duration
    }
    pub fn set_forward_extrapolation_type(&self, v: ExtrapolationType) {
        let c = self.state.borrow().forward != v;
        if c {
            self.state.borrow_mut().forward = v;
            self.changed.raise();
        }
    }
    pub fn set_backward_extrapolation_type(&self, v: ExtrapolationType) {
        let c = self.state.borrow().backward != v;
        if c {
            self.state.borrow_mut().backward = v;
            self.changed.raise();
        }
    }
    pub fn set_forward_extrapolation_duration(&self, v: f64) {
        let c = self.state.borrow().forward_duration != v;
        if c {
            self.state.borrow_mut().forward_duration = v;
            self.changed.raise();
        }
    }
    pub fn set_backward_extrapolation_duration(&self, v: f64) {
        let c = self.state.borrow().backward_duration != v;
        if c {
            self.state.borrow_mut().backward_duration = v;
            self.changed.raise();
        }
    }
    pub fn set_interpolation_options(&self, algorithm: Option<Interpolation>, degree: Option<f64>) {
        let changed = {
            let mut s = self.state.borrow_mut();
            let mut changed = false;
            if let Some(a) = algorithm {
                if !s.algorithm.same(&a) {
                    s.algorithm = a;
                    changed = true;
                }
            }
            if let Some(d) = degree {
                if s.degree != d {
                    s.degree = d;
                    changed = true;
                }
            }
            changed
        };
        if changed {
            self.state.borrow().points.set(None);
            self.changed.raise();
        }
    }
    fn packed(&self, value: &T, derivatives: Option<&[Vec<f64>]>) -> Result<Vec<f64>> {
        let mut row = vec![f64::NAN; self.stride];
        value.pack(&mut row, 0);
        if let Some(types) = self.derivative_types.as_ref().filter(|types| !types.is_empty()) {
            let d = derivatives.ok_or_else(|| RuntimeError::new("derivatives is required."))?;
            if d.len() != types.len() {
                return Err(RuntimeError::new("derivatives must match derivativeTypes."));
            }
            let mut offset = T::PACKED_LENGTH;
            for (t, v) in types.iter().zip(d) {
                if v.len() != t.packed_length {
                    return Err(RuntimeError::new("Invalid derivative packed length"));
                }
                row[offset..offset + v.len()].copy_from_slice(v);
                offset += v.len();
            }
        }
        Ok(row)
    }
    fn index(times: &[JulianDate], date: &JulianDate) -> std::result::Result<usize, usize> {
        times.binary_search_by(|t| JulianDate::compare(t, date).partial_cmp(&0.).unwrap_or(Ordering::Equal))
    }
    fn merge(&self, rows: impl IntoIterator<Item = (JulianDate, Vec<f64>)>) {
        {
            let mut s = self.state.borrow_mut();
            s.points.set(None);
            for (time, row) in rows {
                match Self::index(&s.times, &time) {
                    Ok(i) => s.values[i * self.stride..(i + 1) * self.stride].copy_from_slice(&row),
                    Err(i) => {
                        s.times.insert(i, time);
                        s.values.splice(i * self.stride..i * self.stride, row);
                    }
                }
            }
        }
        self.changed.raise();
    }
    pub fn add_sample(&self, time: JulianDate, value: &T, derivatives: Option<&[Vec<f64>]>) -> Result<()> {
        let row = self.packed(value, derivatives)?;
        self.merge([(time, row)]);
        Ok(())
    }
    pub fn add_samples(&self, times: &[JulianDate], values: &[T], derivatives: Option<&[Vec<Vec<f64>>]>) -> Result<()> {
        if times.len() != values.len() {
            return Err(RuntimeError::new("times and values must be the same length."));
        }
        if self.derivative_types.as_ref().is_some_and(|types| !types.is_empty())
            && derivatives.is_none_or(|d| d.len() != times.len())
        {
            return Err(RuntimeError::new("times and derivativeValues must be the same length."));
        }
        let rows = times
            .iter()
            .zip(values)
            .enumerate()
            .map(|(i, (t, v))| self.packed(v, derivatives.map(|d| d[i].as_slice())).map(|r| (*t, r)))
            .collect::<Result<Vec<_>>>()?;
        self.merge(rows);
        Ok(())
    }
    pub fn add_samples_packed_array(&self, data: &[PackedSampleValue], epoch: Option<JulianDate>) -> Result<()> {
        if !data.len().is_multiple_of(self.stride + 1) {
            return Err(RuntimeError::new("Invalid packed sample length"));
        }
        let mut rows = vec![];
        for row in data.chunks(self.stride + 1) {
            let time = match &row[0] {
                PackedSampleValue::Date(d) => *d,
                PackedSampleValue::Iso8601(s) => {
                    JulianDate::from_iso8601(s).map_err(|e| RuntimeError::new(e.to_string()))?
                }
                PackedSampleValue::Number(n) => epoch
                    .ok_or_else(|| RuntimeError::new("epoch is required for numeric sample times"))?
                    .add_seconds(*n),
            };
            let values = row[1..]
                .iter()
                .map(|v| match v {
                    PackedSampleValue::Number(n) => Ok(*n),
                    _ => Err(RuntimeError::new("Sample values must be numeric")),
                })
                .collect::<Result<Vec<_>>>()?;
            rows.push((time, values));
        }
        self.merge(rows);
        Ok(())
    }
    pub fn get_sample(&self, index: isize) -> Option<JulianDate> {
        let s = self.state.borrow();
        let index = if index < 0 { index.checked_add(s.times.len() as isize)? } else { index };
        usize::try_from(index).ok().and_then(|i| s.times.get(i)).copied()
    }
    pub fn remove_sample(&self, time: &JulianDate) -> bool {
        let removed = {
            let mut s = self.state.borrow_mut();
            if let Ok(i) = Self::index(&s.times, time) {
                s.times.remove(i);
                s.values.drain(i * self.stride..(i + 1) * self.stride);
                true
            } else {
                false
            }
        };
        if removed {
            self.state.borrow().points.set(None);
            self.changed.raise();
        }
        removed
    }
    pub fn remove_samples<D>(&self, interval: &TimeInterval<D>) {
        self.state.borrow().points.set(None);
        {
            let mut s = self.state.borrow_mut();
            let first = match Self::index(&s.times, &interval.start) {
                Ok(i) => i + usize::from(!interval.is_start_included),
                Err(i) => i,
            };
            let last = match Self::index(&s.times, &interval.stop) {
                Ok(i) => i + usize::from(interval.is_stop_included),
                Err(i) => i,
            };
            if last > first {
                s.times.drain(first..last);
                s.values.drain(first * self.stride..last * self.stride);
            }
        }
        self.changed.raise();
    }
    fn evaluate(&self, time: &JulianDate) -> Result<Option<T>> {
        let s = self.state.borrow();
        let n = s.times.len();
        if n == 0 {
            return Ok(None);
        }
        let mut index = match Self::index(&s.times, time) {
            Ok(i) => return Ok(Some(T::unpack(&s.values, i * self.stride))),
            Err(i) => i,
        };
        if index == 0 {
            if s.backward == ExtrapolationType::None
                || s.backward_duration != 0. && JulianDate::seconds_difference(&s.times[0], time) > s.backward_duration
            {
                return Ok(None);
            }
            if s.backward == ExtrapolationType::Hold {
                return Ok(Some(T::unpack(&s.values, 0)));
            }
        }
        if index >= n {
            index = n - 1;
            if s.forward == ExtrapolationType::None
                || s.forward_duration != 0.
                    && JulianDate::seconds_difference(time, &s.times[index]) > s.forward_duration
            {
                return Ok(None);
            }
            // Preserve the pinned forward-HOLD offset even with derivatives.
            if s.forward == ExtrapolationType::Hold {
                return Ok(Some(T::unpack(&s.values, index * T::PACKED_LENGTH)));
            }
        }
        let algorithm = s.algorithm.algorithm();
        let order = self.derivative_types.as_ref().map_or(0, Vec::len);
        let points = if let Some(points) = s.points.get() {
            points
        } else {
            let requested = algorithm.get_required_data_points(s.degree, order as f64);
            let requested = if requested.is_nan() { requested } else { requested.min(n as f64) };
            if !requested.is_finite() || requested < 0. || requested.fract() != 0. {
                return Err(RuntimeError::new("Invalid array length"));
            }
            let points = (requested as usize).min(n);
            s.points.set(Some(points));
            points
        };
        if points < 2 {
            return Ok(None);
        }
        let degree = points - 1;
        let first = index.saturating_sub(degree / 2 + 1).min(n - points);
        let last = first + degree;
        let x_table =
            s.times[first..=last].iter().map(|t| JulianDate::seconds_difference(t, &s.times[last])).collect::<Vec<_>>();
        let mut y = T::convert_for_interpolation(&s.values, first, last)
            .unwrap_or_else(|| s.values[first * self.stride..(last + 1) * self.stride].to_vec());
        if y.len() < points * self.interpolation_stride {
            y.resize(points * self.interpolation_stride, f64::NAN);
        }
        let x = JulianDate::seconds_difference(time, &s.times[last]);
        let samples = InterpolationSamples {
            x,
            x_table: &x_table,
            y_table: &y,
            y_stride: self.interpolation_stride / (order + 1),
        };
        let interpolated = if order > 0 { algorithm.interpolate(&samples, order, order) } else { None };
        let out = interpolated
            .or_else(|| {
                algorithm
                    .interpolate_order_zero(&InterpolationSamples { y_stride: self.interpolation_stride, ..samples })
            })
            .ok_or_else(|| RuntimeError::new("Interpolation failed"))?;
        Ok(Some(T::unpack_interpolated(&out, &s.values, first, last)))
    }
}
impl<T: SampledValue> Property<T> for SampledProperty<T> {
    fn is_constant(&self) -> bool {
        self.state.borrow().values.is_empty()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(&self, time: Option<&JulianDate>, result: Option<&mut T>) -> Result<Option<T>> {
        let time = time.copied().unwrap_or_else(|| (self.clock)());
        Ok(write_result(self.evaluate(&time)?, result))
    }
    fn equals(&self, other: &dyn Property<T>) -> Result<bool> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        if std::ptr::eq(self, o) {
            return Ok(true);
        }
        let (a, b) = (self.state.borrow(), o.state.borrow());
        Ok(self.derivative_types == o.derivative_types
            && a.degree == b.degree
            && a.algorithm.same(&b.algorithm)
            && a.times == b.times
            && a.values == b.values)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
