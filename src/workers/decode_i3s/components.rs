//! Typed attribute buffers and JavaScript object-key ordering.
//!
//! Cesium's I3S worker keeps each attribute in the typed array Draco or the
//! binary schema produced, and later writes transformed values back into that
//! same array. Stores use JavaScript's `ToInt*` / `ToUint*` / `Float32`
//! conversions. Outline welding uses the same object-key rules: integer keys
//! in `0..2^32-1` sort numerically ahead of every other key, and other keys
//! keep insertion order. `+0` and `-0` are one key.

use std::collections::BTreeMap;

/// One I3S attribute channel, in the typed array Cesium keeps it in.
#[derive(Clone, Debug)]
pub(super) enum Values {
    F32(Vec<f32>),
    I8(Vec<i8>),
    U8(Vec<u8>),
    I16(Vec<i16>),
    U16(Vec<u16>),
    I32(Vec<i32>),
    U32(Vec<u32>),
}

impl Values {
    pub(super) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(super) fn len(&self) -> usize {
        match self {
            Self::F32(v) => v.len(),
            Self::I8(v) => v.len(),
            Self::U8(v) => v.len(),
            Self::I16(v) => v.len(),
            Self::U16(v) => v.len(),
            Self::I32(v) => v.len(),
            Self::U32(v) => v.len(),
        }
    }

    /// Element `index` as a JavaScript number, or `None` past the end
    /// (`undefined` for a typed-array read).
    pub(super) fn get(&self, index: usize) -> Option<f64> {
        match self {
            Self::F32(v) => v.get(index).copied().map(f64::from),
            Self::I8(v) => v.get(index).copied().map(f64::from),
            Self::U8(v) => v.get(index).copied().map(f64::from),
            Self::I16(v) => v.get(index).copied().map(f64::from),
            Self::U16(v) => v.get(index).copied().map(f64::from),
            Self::I32(v) => v.get(index).copied().map(f64::from),
            Self::U32(v) => v.get(index).copied().map(f64::from),
        }
    }

    /// Writes `value` the way a JavaScript typed array would. An index past
    /// the end is ignored.
    pub(super) fn set(&mut self, index: usize, value: f64) {
        match self {
            Self::F32(v) => {
                if let Some(slot) = v.get_mut(index) {
                    *slot = value as f32;
                }
            }
            Self::I8(v) => {
                if let Some(slot) = v.get_mut(index) {
                    *slot = to_int8(value);
                }
            }
            Self::U8(v) => {
                if let Some(slot) = v.get_mut(index) {
                    *slot = to_uint8(value);
                }
            }
            Self::I16(v) => {
                if let Some(slot) = v.get_mut(index) {
                    *slot = to_int16(value);
                }
            }
            Self::U16(v) => {
                if let Some(slot) = v.get_mut(index) {
                    *slot = to_uint16(value);
                }
            }
            Self::I32(v) => {
                if let Some(slot) = v.get_mut(index) {
                    *slot = to_int32(value);
                }
            }
            Self::U32(v) => {
                if let Some(slot) = v.get_mut(index) {
                    *slot = to_uint32(value);
                }
            }
        }
    }

    /// `subarray(0, count)` bytes, clamping `count` to the element length.
    pub(super) fn prefix_bytes(&self, count: usize) -> Vec<u8> {
        let count = count.min(self.len());
        let mut out = Vec::with_capacity(count * self.element_size());
        match self {
            Self::F32(v) => push_bytes(&mut out, &v[..count], |x| x.to_le_bytes()),
            Self::I8(v) => out.extend(v[..count].iter().map(|x| *x as u8)),
            Self::U8(v) => out.extend_from_slice(&v[..count]),
            Self::I16(v) => push_bytes(&mut out, &v[..count], |x| x.to_le_bytes()),
            Self::U16(v) => push_bytes(&mut out, &v[..count], |x| x.to_le_bytes()),
            Self::I32(v) => push_bytes(&mut out, &v[..count], |x| x.to_le_bytes()),
            Self::U32(v) => push_bytes(&mut out, &v[..count], |x| x.to_le_bytes()),
        }
        out
    }

    pub(super) fn to_f64(&self) -> Vec<f64> {
        (0..self.len()).filter_map(|i| self.get(i)).collect()
    }

    fn element_size(&self) -> usize {
        match self {
            Self::F32(_) | Self::I32(_) | Self::U32(_) => 4,
            Self::I16(_) | Self::U16(_) => 2,
            Self::I8(_) | Self::U8(_) => 1,
        }
    }
}

fn push_bytes<T, const N: usize>(out: &mut Vec<u8>, values: &[T], bytes: impl Fn(&T) -> [u8; N]) {
    for value in values {
        out.extend_from_slice(&bytes(value));
    }
}

/// JavaScript `ToUint32`.
pub(super) fn to_uint32(n: f64) -> u32 {
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    let mut n = n.trunc() % 4_294_967_296.0;
    if n < 0.0 {
        n += 4_294_967_296.0;
    }
    n as u32
}

pub(super) fn to_int32(n: f64) -> i32 {
    to_uint32(n) as i32
}

pub(super) fn to_uint16(n: f64) -> u16 {
    to_uint32(n) as u16
}

pub(super) fn to_int16(n: f64) -> i16 {
    to_uint16(n) as i16
}

pub(super) fn to_uint8(n: f64) -> u8 {
    to_uint32(n) as u8
}

pub(super) fn to_int8(n: f64) -> i8 {
    to_uint8(n) as i8
}

/// `new Float32Array(length)` length conversion (`ToLength` of a non-negative number).
pub(super) fn to_length(n: f64) -> usize {
    if !n.is_finite() || n <= 0.0 {
        return 0;
    }
    let n = n.trunc();
    if n >= usize::MAX as f64 {
        usize::MAX
    } else {
        n as usize
    }
}

/// Feature ids, either a typed array (dense) or the plain array built from `faceRange`.
#[derive(Clone, Debug)]
pub(super) enum FeatureIndex {
    Dense(Vec<f64>),
    /// `None` is a hole (`undefined`), which `Float32Array` stores as NaN.
    Sparse(Vec<Option<f64>>),
}

impl FeatureIndex {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Dense(v) => v.len(),
            Self::Sparse(v) => v.len(),
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<f64> {
        match self {
            Self::Dense(v) => v.get(index).copied(),
            Self::Sparse(v) => v.get(index).copied().flatten(),
        }
    }

    pub(super) fn from_values(values: &Values) -> Self {
        Self::Dense(values.to_f64())
    }

    /// Every element, with holes and out-of-range reads as `None`.
    pub(super) fn to_options(&self) -> Vec<Option<f64>> {
        (0..self.len()).map(|i| self.get(i)).collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum OtherKey {
    NegInf,
    Finite(u64),
    PosInf,
    Nan,
}

fn other_key(n: f64) -> OtherKey {
    if n.is_nan() {
        OtherKey::Nan
    } else if n == f64::INFINITY {
        OtherKey::PosInf
    } else if n == f64::NEG_INFINITY {
        OtherKey::NegInf
    } else {
        OtherKey::Finite(n.to_bits())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ExtraKey {
    /// Object key `"undefined"`, which is not a number and not `"NaN"`.
    Missing,
    Number(OtherKey),
}

/// A JavaScript object used as a map from numbers, plus the `"undefined"` key.
#[derive(Clone, Debug)]
pub(super) struct NumMap<T> {
    indices: BTreeMap<u32, T>,
    others: Vec<(ExtraKey, T)>,
}

impl<T> NumMap<T> {
    pub(super) fn new() -> Self {
        Self { indices: BTreeMap::new(), others: Vec::new() }
    }

    pub(super) fn entry<F: FnOnce() -> T>(&mut self, key: f64, make: F) -> &mut T {
        if let Some(index) = array_index(key) {
            return self.indices.entry(index).or_insert_with(make);
        }
        self.extra(ExtraKey::Number(other_key(key)), make)
    }

    /// The property key produced by indexing an object with `undefined`.
    pub(super) fn entry_missing<F: FnOnce() -> T>(&mut self, make: F) -> &mut T {
        self.extra(ExtraKey::Missing, make)
    }

    fn extra<F: FnOnce() -> T>(&mut self, key: ExtraKey, make: F) -> &mut T {
        if let Some(position) = self.others.iter().position(|(k, _)| *k == key) {
            return &mut self.others[position].1;
        }
        self.others.push((key, make()));
        let last = self.others.len() - 1;
        &mut self.others[last].1
    }

    /// Integer keys in ascending order, then every other key in insertion order.
    pub(super) fn values_in_key_order(&self) -> Vec<&T> {
        self.indices.values().chain(self.others.iter().map(|(_, v)| v)).collect()
    }
}

impl<T> Default for NumMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// `0..2^32-1` as an array index. `-0` is `0`.
pub(super) fn array_index(n: f64) -> Option<u32> {
    if n.is_nan() || !(0.0..4_294_967_295.0).contains(&n) || n.fract() != 0.0 {
        return None;
    }
    Some(n as u32)
}

/// Property name Cesium uses when a feature id is read out of an array.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum FeatureKey {
    Index(u32),
    Undefined,
    Nan,
    PosInf,
    NegInf,
    Finite(u64),
    Default,
}

impl FeatureKey {
    pub(super) fn from_value(value: Option<f64>) -> Self {
        match value {
            None => Self::Undefined,
            Some(n) if n.is_nan() => Self::Nan,
            Some(n) if n == f64::INFINITY => Self::PosInf,
            Some(n) if n == f64::NEG_INFINITY => Self::NegInf,
            Some(n) => array_index(n).map(Self::Index).unwrap_or(Self::Finite(n.to_bits())),
        }
    }

    /// Parses a symbology property name the way an object key compares to a feature id.
    pub(super) fn parse_property(name: &str) -> Self {
        if name == "default" {
            return Self::Default;
        }
        if name == "undefined" {
            return Self::Undefined;
        }
        if name == "NaN" {
            return Self::Nan;
        }
        if name == "Infinity" {
            return Self::PosInf;
        }
        if name == "-Infinity" {
            return Self::NegInf;
        }
        if let Ok(n) = name.parse::<f64>() {
            let key = Self::from_value(Some(n));
            // `"1.0"` is not the property `"1"`. Only the canonical key matches.
            let canonical = match key {
                Self::Index(index) => index.to_string(),
                Self::PosInf => "Infinity".to_string(),
                Self::NegInf => "-Infinity".to_string(),
                Self::Nan => "NaN".to_string(),
                Self::Finite(bits) => {
                    return if f64::from_bits(bits).to_string() == name { key } else { Self::Finite(u64::MAX) }
                }
                Self::Undefined | Self::Default => return key,
            };
            if name == canonical {
                return key;
            }
        }
        // A non-numeric property can only match the literal strings above.
        Self::Finite(u64::MAX)
    }
}

#[derive(Clone, Debug)]
pub(super) struct FeatureMap<T> {
    indices: BTreeMap<u32, T>,
    others: Vec<(FeatureKey, T)>,
}

impl<T> FeatureMap<T> {
    pub(super) fn new() -> Self {
        Self { indices: BTreeMap::new(), others: Vec::new() }
    }

    pub(super) fn entry<F: FnOnce() -> T>(&mut self, key: FeatureKey, make: F) -> &mut T {
        if let FeatureKey::Index(index) = key {
            return self.indices.entry(index).or_insert_with(make);
        }
        if let Some(position) = self.others.iter().position(|(k, _)| *k == key) {
            return &mut self.others[position].1;
        }
        self.others.push((key, make()));
        let last = self.others.len() - 1;
        &mut self.others[last].1
    }

    pub(super) fn values_in_key_order(&self) -> Vec<&T> {
        self.indices.values().chain(self.others.iter().map(|(_, v)| v)).collect()
    }
}
