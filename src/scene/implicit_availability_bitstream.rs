//! CesiumJS `ImplicitAvailabilityBitstream`: which tiles, contents or child
//! subtrees of an implicit subtree exist, as a constant or one bit each.

use crate::core::RuntimeError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImplicitAvailabilityBitstream {
    length_bits: usize,
    available_count: Option<usize>,
    constant: Option<bool>,
    bitstream: Option<Vec<u8>>,
}

/// `new ImplicitAvailabilityBitstream(options)`: a constant or a bitstream.
#[derive(Clone, Debug, Default)]
pub struct ImplicitAvailabilityBitstreamOptions {
    pub length_bits: usize,
    pub available_count: Option<usize>,
    /// Every bit has this value.
    pub constant: Option<bool>,
    /// Bits in little-endian bit order within each byte.
    pub bitstream: Option<Vec<u8>>,
    /// Count the set bits when `available_count` is not given. Defaults to
    /// false.
    pub compute_available_count_enabled: bool,
}

impl ImplicitAvailabilityBitstream {
    /// A constant counts all `length_bits` as available, whatever its value.
    /// A bitstream must hold exactly `ceil(length_bits / 8)` bytes.
    pub fn new(options: ImplicitAvailabilityBitstreamOptions) -> Result<Self, RuntimeError> {
        let length_bits = options.length_bits;
        let mut available_count = options.available_count;
        if options.constant.is_some() {
            available_count = Some(length_bits);
        } else {
            let bitstream = options.bitstream.as_deref().unwrap_or_default();
            let expected_length = length_bits.div_ceil(8);
            if bitstream.len() != expected_length {
                return Err(RuntimeError::new(format!(
                    "Availability bitstream must be exactly {expected_length} bytes long to store {length_bits} bits. Actual bitstream was {} bytes long.",
                    bitstream.len()
                )));
            }
            if available_count.is_none() && options.compute_available_count_enabled {
                available_count = Some(count_1_bits(bitstream, length_bits));
            }
        }
        Ok(Self { length_bits, available_count, constant: options.constant, bitstream: options.bitstream })
    }

    pub fn length_bits(&self) -> usize {
        self.length_bits
    }

    /// The number of available bits: given, counted, or `None` when neither.
    pub fn available_count(&self) -> Option<usize> {
        self.available_count
    }

    /// Bit `index`; `None` out of bounds, where Cesium throws a
    /// `DeveloperError`.
    pub fn get_bit(&self, index: usize) -> Option<bool> {
        if index >= self.length_bits {
            return None;
        }
        if let Some(constant) = self.constant {
            return Some(constant);
        }
        let bitstream = self.bitstream.as_deref().unwrap_or_default();
        Some((bitstream[index >> 3] >> (index % 8)) & 1 == 1)
    }
}

fn count_1_bits(bitstream: &[u8], length_bits: usize) -> usize {
    (0..length_bits).map(|i| ((bitstream[i >> 3] >> (i % 8)) & 1) as usize).sum()
}
