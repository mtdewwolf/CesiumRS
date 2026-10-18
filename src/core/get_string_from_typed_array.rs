//! CesiumJS `getStringFromTypedArray`, `getMagic`, and `getJsonFromTypedArray`.

use super::runtime_error::RuntimeError;

/// Decodes UTF-8 like `TextDecoder`: a leading byte-order mark is dropped and
/// invalid sequences become U+FFFD. The range is clamped to the bytes, as
/// `subarray` clamps it.
pub fn get_string_from_typed_array(bytes: &[u8], byte_offset: usize, byte_length: Option<usize>) -> String {
    let len = bytes.len();
    let start = byte_offset.min(len);
    let end = match byte_length {
        Some(l) => byte_offset.saturating_add(l).min(len),
        None => len,
    };
    let view = if end > start { &bytes[start..end] } else { &[] };
    let view = view.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(view);
    String::from_utf8_lossy(view).into_owned()
}

/// The first four characters at `byte_offset`, such as `glTF` or `b3dm`.
pub fn get_magic(bytes: &[u8], byte_offset: usize) -> String {
    get_string_from_typed_array(bytes, byte_offset, Some(4.min(bytes.len())))
}

pub fn get_json_from_typed_array(
    bytes: &[u8],
    byte_offset: usize,
    byte_length: Option<usize>,
) -> Result<serde_json::Value, RuntimeError> {
    serde_json::from_str(&get_string_from_typed_array(bytes, byte_offset, byte_length))
        .map_err(|e| RuntimeError::new(e.to_string()))
}
