//! Pure-Rust ZIP reader/writer over `flate2` for KMZ archives, plus MIME type lookup.
use crate::core::{uri_helpers::get_extension_from_uri, RuntimeError};
use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use std::io::{Read, Write};

const LOCAL_FILE_HEADER_SIG: u32 = 0x0403_4b50;
const CENTRAL_DIR_HEADER_SIG: u32 = 0x0201_4b50;
const END_OF_CENTRAL_DIR_SIG: u32 = 0x0605_4b50;
const ZIP_MAGIC: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const METHOD_STORE: u16 = 0;
const METHOD_DEFLATE: u16 = 8;

/// One extracted entry from a KMZ/ZIP archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipEntry {
    pub filename: String,
    pub is_dir: bool,
    pub data: Vec<u8>,
}

/// Returns true if `bytes` starts with the PKZIP local file header signature.
pub fn is_zip_bytes(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4] == ZIP_MAGIC
}

/// Computes standard IEEE 802.3 CRC-32 (PKZIP polynomial `0xEDB88320`).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Result<u16, RuntimeError> {
    let slice = bytes.get(offset..offset + 2).ok_or_else(|| RuntimeError::new("Unexpected end of ZIP archive."))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, RuntimeError> {
    let slice = bytes.get(offset..offset + 4).ok_or_else(|| RuntimeError::new("Unexpected end of ZIP archive."))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn find_eocd(bytes: &[u8]) -> Result<usize, RuntimeError> {
    if bytes.len() < 22 {
        return Err(RuntimeError::new("ZIP archive is too short."));
    }
    let min_start = bytes.len().saturating_sub(22 + 65_535);
    let mut pos = bytes.len() - 22;
    loop {
        if read_u32_le(bytes, pos)? == END_OF_CENTRAL_DIR_SIG {
            let comment_len = usize::from(read_u16_le(bytes, pos + 20)?);
            if pos + 22 + comment_len == bytes.len() {
                return Ok(pos);
            }
        }
        if pos == min_start {
            break;
        }
        pos -= 1;
    }
    Err(RuntimeError::new("Invalid ZIP archive: missing End of Central Directory."))
}

/// Reads all entries from a ZIP/KMZ archive (Store and Deflate methods).
pub fn read_zip(bytes: &[u8]) -> Result<Vec<ZipEntry>, RuntimeError> {
    let eocd_pos = find_eocd(bytes)?;
    let entry_count = usize::from(read_u16_le(bytes, eocd_pos + 10)?);
    let cd_size = read_u32_le(bytes, eocd_pos + 12)? as usize;
    let cd_offset = read_u32_le(bytes, eocd_pos + 16)? as usize;
    let cd_end = cd_offset.checked_add(cd_size).ok_or_else(|| RuntimeError::new("ZIP central directory overflow."))?;
    if cd_end > bytes.len() {
        return Err(RuntimeError::new("ZIP central directory extends past end of archive."));
    }

    let mut entries = Vec::with_capacity(entry_count);
    let mut cursor = cd_offset;
    for _ in 0..entry_count {
        if cursor + 46 > cd_end {
            return Err(RuntimeError::new("Truncated ZIP central directory entry."));
        }
        if read_u32_le(bytes, cursor)? != CENTRAL_DIR_HEADER_SIG {
            return Err(RuntimeError::new("Invalid ZIP central directory signature."));
        }
        let method = read_u16_le(bytes, cursor + 10)?;
        let expected_crc = read_u32_le(bytes, cursor + 16)?;
        let compressed_size = read_u32_le(bytes, cursor + 20)? as usize;
        let uncompressed_size = read_u32_le(bytes, cursor + 24)? as usize;
        let name_len = usize::from(read_u16_le(bytes, cursor + 28)?);
        let extra_len = usize::from(read_u16_le(bytes, cursor + 30)?);
        let comment_len = usize::from(read_u16_le(bytes, cursor + 32)?);
        let external_attr = read_u32_le(bytes, cursor + 38)?;
        let local_offset = read_u32_le(bytes, cursor + 42)? as usize;

        let name_start = cursor + 46;
        let name_end = name_start + name_len;
        let name_bytes =
            bytes.get(name_start..name_end).ok_or_else(|| RuntimeError::new("Truncated ZIP entry filename."))?;
        let filename = String::from_utf8_lossy(name_bytes).into_owned();

        cursor = name_end + extra_len + comment_len;

        let is_dir = filename.ends_with('/') || (external_attr & 0x10) != 0;
        if is_dir {
            entries.push(ZipEntry { filename, is_dir: true, data: Vec::new() });
            continue;
        }

        if local_offset + 30 > bytes.len() {
            return Err(RuntimeError::new("Invalid ZIP local file header offset."));
        }
        if read_u32_le(bytes, local_offset)? != LOCAL_FILE_HEADER_SIG {
            return Err(RuntimeError::new("Invalid ZIP local file header signature."));
        }
        let local_name_len = usize::from(read_u16_le(bytes, local_offset + 26)?);
        let local_extra_len = usize::from(read_u16_le(bytes, local_offset + 28)?);
        let data_start = local_offset + 30 + local_name_len + local_extra_len;
        let data_end =
            data_start.checked_add(compressed_size).ok_or_else(|| RuntimeError::new("ZIP entry payload overflow."))?;
        let compressed =
            bytes.get(data_start..data_end).ok_or_else(|| RuntimeError::new("Truncated ZIP entry payload."))?;

        let data = match method {
            METHOD_STORE => compressed.to_vec(),
            METHOD_DEFLATE => {
                let mut decoder = DeflateDecoder::new(compressed);
                let mut out = Vec::with_capacity(uncompressed_size);
                decoder
                    .read_to_end(&mut out)
                    .map_err(|e| RuntimeError::new(format!("ZIP deflate decompression failed: {e}")))?;
                out
            }
            other => {
                return Err(RuntimeError::new(format!("Unsupported ZIP compression method: {other}")));
            }
        };

        if crc32(&data) != expected_crc {
            return Err(RuntimeError::new(format!("ZIP CRC mismatch for entry '{filename}'.")));
        }

        entries.push(ZipEntry { filename, is_dir: false, data });
    }

    Ok(entries)
}

/// Writes a standard PKZIP 2.0 archive containing the given `(filename, bytes)` entries.
pub fn write_zip(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, RuntimeError> {
    let mut out = Vec::new();
    let mut central_dir = Vec::new();

    for &(filename, raw_data) in entries {
        let local_offset = out.len() as u32;
        let checksum = crc32(raw_data);
        let uncompressed_size = raw_data.len() as u32;

        let (method, payload) = if raw_data.is_empty() {
            (METHOD_STORE, Vec::new())
        } else {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(raw_data)
                .map_err(|e| RuntimeError::new(format!("ZIP deflate compression failed: {e}")))?;
            let compressed =
                encoder.finish().map_err(|e| RuntimeError::new(format!("ZIP deflate finish failed: {e}")))?;
            if compressed.len() < raw_data.len() {
                (METHOD_DEFLATE, compressed)
            } else {
                (METHOD_STORE, raw_data.to_vec())
            }
        };

        let compressed_size = payload.len() as u32;
        let name_bytes = filename.as_bytes();
        let name_len =
            u16::try_from(name_bytes.len()).map_err(|_| RuntimeError::new("ZIP filename exceeds 65535 bytes."))?;
        let flags: u16 = 0x0800; // UTF-8 filename encoding
        let version: u16 = 20;

        // Local file header
        out.extend_from_slice(&LOCAL_FILE_HEADER_SIG.to_le_bytes());
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // mod time
        out.extend_from_slice(&0u16.to_le_bytes()); // mod date
        out.extend_from_slice(&checksum.to_le_bytes());
        out.extend_from_slice(&compressed_size.to_le_bytes());
        out.extend_from_slice(&uncompressed_size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&payload);

        // Central directory header
        central_dir.extend_from_slice(&CENTRAL_DIR_HEADER_SIG.to_le_bytes());
        central_dir.extend_from_slice(&version.to_le_bytes()); // version made by
        central_dir.extend_from_slice(&version.to_le_bytes()); // version needed
        central_dir.extend_from_slice(&flags.to_le_bytes());
        central_dir.extend_from_slice(&method.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // mod time
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // mod date
        central_dir.extend_from_slice(&checksum.to_le_bytes());
        central_dir.extend_from_slice(&compressed_size.to_le_bytes());
        central_dir.extend_from_slice(&uncompressed_size.to_le_bytes());
        central_dir.extend_from_slice(&name_len.to_le_bytes());
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // extra len
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // comment len
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // disk number
        central_dir.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        central_dir.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        central_dir.extend_from_slice(&local_offset.to_le_bytes());
        central_dir.extend_from_slice(name_bytes);
    }

    let cd_offset = out.len() as u32;
    let cd_size = central_dir.len() as u32;
    let count = u16::try_from(entries.len()).map_err(|_| RuntimeError::new("ZIP entry count exceeds 65535."))?;

    out.extend_from_slice(&central_dir);

    // End of central directory record
    out.extend_from_slice(&END_OF_CENTRAL_DIR_SIG.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // disk number
    out.extend_from_slice(&0u16.to_le_bytes()); // cd start disk
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment len

    Ok(out)
}

/// Detects MIME type from a filename using CesiumJS's KML `MimeTypes` table.
pub fn detect_mime_type(filename: &str) -> &'static str {
    let ext = get_extension_from_uri(&filename.to_ascii_lowercase());
    match ext.as_str() {
        "avi" => "video/x-msvideo",
        "bmp" => "image/bmp",
        "bz2" => "application/x-bzip2",
        "chm" => "application/vnd.ms-htmlhelp",
        "css" => "text/css",
        "csv" => "text/csv",
        "doc" => "application/msword",
        "dvi" => "application/x-dvi",
        "eps" | "ps" => "application/postscript",
        "flv" => "video/x-flv",
        "gif" => "image/gif",
        "gz" => "application/x-gzip",
        "htm" | "html" => "text/html",
        "ico" => "image/vnd.microsoft.icon",
        "jnlp" => "application/x-java-jnlp-file",
        "jpeg" | "jpg" => "image/jpeg",
        "m3u" => "audio/x-mpegurl",
        "m4v" | "mp4" | "mp4v" => "video/mp4",
        "mathml" => "application/mathml+xml",
        "mid" | "midi" => "audio/midi",
        "mov" | "qt" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "mpeg" | "mpg" => "video/mpeg",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ogg" => "application/ogg",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "pps" | "ppt" => "application/vnd.ms-powerpoint",
        "rdf" => "application/rdf+xml",
        "rss" => "application/rss+xml",
        "rtf" => "application/rtf",
        "svg" => "image/svg+xml",
        "swf" => "application/x-shockwave-flash",
        "text" | "txt" => "text/plain",
        "tif" | "tiff" => "image/tiff",
        "wav" => "audio/x-wav",
        "wma" => "audio/x-ms-wma",
        "wmv" => "video/x-ms-wmv",
        "xml" | "kml" => "application/xml",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}
