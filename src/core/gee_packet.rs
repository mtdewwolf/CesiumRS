//! Native GEE packet inflation, quadtree decoding and dbRoot protobuf fields.
use super::google_earth_enterprise_tile_information::GoogleEarthEnterpriseTileInformationOptions;
use super::network_terrain::Reader;
use super::{GoogleEarthEnterpriseTileInformation, RuntimeError};
use std::{collections::BTreeMap, io::Read};
pub fn inflate_packet(data: &[u8], key: &[u8]) -> Result<Vec<u8>, RuntimeError> {
    let mut data = data.to_vec();
    super::decode_google_earth_enterprise_data::decode_google_earth_enterprise_data(key, &mut data)?;
    let mut r = Reader::new(&data);
    let magic = r.u32()?;
    let length = match magic {
        0x7468dead => r.u32()?,
        0xadde6874 => u32::from_be_bytes(r.take(4)?.try_into().unwrap()),
        _ => return Err(RuntimeError::new("Invalid magic")),
    } as usize;
    let source = r.take(data.len() - r.pos)?;
    let mut out = vec![];
    if source.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(source).take(length as u64 + 1).read_to_end(&mut out)
    } else {
        flate2::read::ZlibDecoder::new(source).take(length as u64 + 1).read_to_end(&mut out)
    }
    .map_err(|e| RuntimeError::new(e.to_string()))?;
    if out.len() != length {
        return Err(RuntimeError::new("Size of packet doesn't match header"));
    }
    Ok(out)
}
pub fn terrain_meshes(data: &[u8]) -> Result<Vec<Vec<u8>>, RuntimeError> {
    let mut r = Reader::new(data);
    let mut out = vec![];
    for _ in 0..5 {
        let start = r.pos;
        for _ in 0..4 {
            let size = r.u32()? as usize;
            r.take(size)?;
        }
        out.push(data[start..r.pos].to_vec());
    }
    Ok(out)
}
pub fn metadata_packet(
    data: &[u8],
    quad_key: &str,
) -> Result<BTreeMap<String, Option<GoogleEarthEnterpriseTileInformation>>, RuntimeError> {
    let mut r = Reader::new(data);
    if r.u32()? != 32301 {
        return Err(RuntimeError::new("Invalid magic"));
    }
    if r.u32()? != 1 {
        return Err(RuntimeError::new("Invalid data type. Must be 1 for QuadTreePacket"));
    }
    if r.u32()? != 2 {
        return Err(RuntimeError::new("Invalid QuadTreePacket version. Only version 2 is supported."));
    }
    let count = r.u32()? as usize;
    if r.u32()? != 32 {
        return Err(RuntimeError::new("Invalid instance size."));
    }
    let offset = r.u32()? as usize;
    let data_size = r.u32()? as usize;
    let meta_size = r.u32()? as usize;
    if count.checked_mul(32).and_then(|n| n.checked_add(r.pos)) != Some(offset) {
        return Err(RuntimeError::new("Invalid dataBufferOffset"));
    }
    if offset.checked_add(data_size).and_then(|n| n.checked_add(meta_size)) != Some(data.len()) {
        return Err(RuntimeError::new("Invalid packet offsets"));
    }
    let mut entries = vec![];
    for _ in 0..count {
        let bits = r.u8()?;
        r.u8()?;
        let cnode_version = r.u16()?;
        let imagery_version = r.u16()?;
        let terrain_version = r.u16()?;
        r.take(20)?;
        let imagery_provider = r.u8()?;
        let terrain_provider = r.u8()?;
        r.take(2)?;
        entries.push(GoogleEarthEnterpriseTileInformation::new(GoogleEarthEnterpriseTileInformationOptions {
            bits,
            cnode_version,
            imagery_version,
            terrain_version,
            imagery_provider,
            terrain_provider,
        }));
    }
    let root = entries.first().ok_or_else(|| RuntimeError::new("QuadTreePacket has no root."))?;
    let mut out = BTreeMap::new();
    if !quad_key.is_empty() {
        out.insert(quad_key.into(), Some(root.clone()));
    }
    let mut index = 1;
    fn populate(
        key: &str,
        parent: &GoogleEarthEnterpriseTileInformation,
        level: u32,
        entries: &[GoogleEarthEnterpriseTileInformation],
        index: &mut usize,
        out: &mut BTreeMap<String, Option<GoogleEarthEnterpriseTileInformation>>,
    ) {
        if level == 4 && parent.has_subtree() {
            return;
        }
        for i in 0..4 {
            let child = format!("{key}{i}");
            if level == 4 || !parent.has_child(i) {
                out.insert(child, None);
            } else if let Some(info) = entries.get(*index) {
                *index += 1;
                out.insert(child.clone(), Some(info.clone()));
                populate(&child, info, level + 1, entries, index, out);
            } else {
                return;
            }
        }
    }
    populate(quad_key, root, if quad_key.is_empty() { 1 } else { 0 }, &entries, &mut index, &mut out);
    Ok(out)
}
#[derive(Clone, Copy)]
pub enum Wire<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}
fn varint(bytes: &[u8], pos: &mut usize) -> Result<u64, RuntimeError> {
    let mut value = 0u64;
    for shift in (0..70).step_by(7) {
        let b = *bytes.get(*pos).ok_or_else(|| RuntimeError::new("Protobuf field is truncated."))?;
        *pos += 1;
        if shift == 63 && b > 1 {
            return Err(RuntimeError::new("Protobuf varint overflow."));
        }
        value |= ((b & 127) as u64) << shift;
        if b < 128 {
            return Ok(value);
        }
    }
    Err(RuntimeError::new("Protobuf varint overflow."))
}
pub fn protobuf(data: &[u8]) -> Result<Vec<(u32, Wire<'_>)>, RuntimeError> {
    let mut r = Reader::new(data);
    let mut out = vec![];
    while r.pos < data.len() {
        let tag = varint(data, &mut r.pos)?;
        let field = (tag >> 3) as u32;
        if field == 0 {
            return Err(RuntimeError::new("Invalid protobuf field number."));
        }
        let wire = match tag & 7 {
            0 => Wire::Varint(varint(data, &mut r.pos)?),
            1 => Wire::Fixed64(u64::from_le_bytes(r.take(8)?.try_into().unwrap())),
            2 => {
                let n = varint(data, &mut r.pos)? as usize;
                Wire::Bytes(r.take(n)?)
            }
            5 => Wire::Fixed32(u32::from_le_bytes(r.take(4)?.try_into().unwrap())),
            _ => return Err(RuntimeError::new("Unsupported protobuf wire type.")),
        };
        out.push((field, wire));
    }
    Ok(out)
}
pub fn field<'a>(data: &[(u32, Wire<'a>)], id: u32) -> Option<Wire<'a>> {
    data.iter().rev().find(|(n, _)| *n == id).map(|(_, v)| *v)
}
impl<'a> Wire<'a> {
    pub fn bytes(self) -> Option<&'a [u8]> {
        if let Self::Bytes(b) = self {
            Some(b)
        } else {
            None
        }
    }
    pub fn uint(self) -> Option<u64> {
        if let Self::Varint(v) = self {
            Some(v)
        } else {
            None
        }
    }
    pub fn double(self) -> Option<f64> {
        if let Self::Fixed64(v) = self {
            Some(f64::from_bits(v))
        } else {
            None
        }
    }
}
