//! CesiumJS Google Earth Enterprise packet XOR decoding, in place.

use super::runtime_error::RuntimeError;

pub fn decode_google_earth_enterprise_data(key: &[u8], data: &mut [u8]) -> Result<(), RuntimeError> {
    if key.is_empty() || !key.len().is_multiple_of(4) {
        return Err(RuntimeError::new("The length of key must be greater than 0 and a multiple of 4."));
    }
    let header: [u8; 4] =
        data.get(..4).ok_or_else(|| RuntimeError::new("Packet header is truncated."))?.try_into().unwrap();
    if matches!(u32::from_le_bytes(header), 0x7468dead | 0xadde6874) {
        return Ok(());
    }
    // With no 64-bit block, Cesium leaves kp undefined. DataView coerces
    // undefined (then NaN after kp++) to zero on every trailing-byte read.
    if data.len() < 8 {
        for byte in data {
            *byte ^= key[0];
        }
        return Ok(());
    }
    let end64 = data.len() - data.len() % 8;
    let mut dp = 0;
    let mut kp = 0;
    let mut offset = 8;
    // Validate every read before modifying input, including short/misaligned keys.
    let mut ranges = Vec::new();
    while dp < end64 {
        offset = (offset + 8) % 24;
        kp = offset;
        while dp < end64 && kp < key.len() {
            if kp + 8 > key.len() {
                return Err(RuntimeError::new("Key read is out of bounds."));
            }
            ranges.push((dp, kp, 8));
            dp += 8;
            kp += 24;
        }
    }
    if dp < data.len() {
        if kp >= key.len() {
            offset = (offset + 8) % 24;
            kp = offset;
        }
        let length = data.len() - dp;
        if kp + length > key.len() {
            return Err(RuntimeError::new("Key read is out of bounds."));
        }
        ranges.push((dp, kp, length));
    }
    for (dp, kp, length) in ranges {
        for i in 0..length {
            data[dp + i] ^= key[kp + i];
        }
    }
    Ok(())
}
