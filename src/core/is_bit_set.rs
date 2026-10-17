//! CesiumJS `isBitSet`.

/// Whether any bit of `mask` is set in `bits`.
pub fn is_bit_set(bits: u32, mask: u32) -> bool {
    bits & mask != 0
}

#[cfg(test)]
mod tests {
    #[test]
    fn bits() {
        assert!(super::is_bit_set(0b101, 0b100));
        assert!(!super::is_bit_set(0b101, 0b010));
        assert!(!super::is_bit_set(0b101, 0));
    }
}
