//! CesiumJS S2Cell: 64-bit IDs, hexadecimal tokens, ancestry and geodetic
//! centers/vertices. Native u64 replaces JavaScript BigInt for S2's fixed range.
use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;

const MAX_LEVEL: u32 = 30;
const POSITION_TO_IJ: [[u32; 4]; 4] = [[0, 1, 3, 2], [0, 2, 3, 1], [3, 2, 0, 1], [3, 1, 0, 2]];
const ORIENTATION_MASK: [u32; 4] = [1, 0, 0, 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct S2Cell {
    cell_id: u64,
    level: u32,
}

impl S2Cell {
    pub fn new(cell_id: u64) -> Option<Self> {
        Some(Self { cell_id, level: Self::get_level(cell_id)? })
    }
    pub fn cell_id(&self) -> u64 {
        self.cell_id
    }
    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn is_valid_id(id: u64) -> bool {
        id > 0 && (id >> 61) <= 5 && (id.isolate_lowest_one() & 0x1555_5555_5555_5555) != 0
    }
    pub fn is_valid_token(token: &str) -> bool {
        Self::get_id_from_token(token).is_some_and(Self::is_valid_id)
    }
    pub fn from_token(token: &str) -> Option<Self> {
        Self::new(Self::get_id_from_token(token)?)
    }
    pub fn get_id_from_token(token: &str) -> Option<u64> {
        if token.is_empty() || token.len() > 16 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        Some(u64::from_str_radix(token, 16).ok()? << ((16 - token.len()) * 4))
    }
    pub fn get_token_from_id(id: u64) -> String {
        format!("{id:016x}").trim_end_matches('0').to_owned()
    }
    pub fn get_level(id: u64) -> Option<u32> {
        Self::is_valid_id(id).then(|| MAX_LEVEL - id.trailing_zeros() / 2)
    }

    pub fn get_child(&self, index: u8) -> Option<Self> {
        if self.level == MAX_LEVEL || index > 3 {
            return None;
        }
        let lsb = (self.cell_id.isolate_lowest_one()) >> 2;
        Self::new((self.cell_id as i128 + (2 * index as i128 - 3) * lsb as i128) as u64)
    }
    pub fn get_parent(&self) -> Option<Self> {
        self.get_parent_at_level(self.level.checked_sub(1)?)
    }
    pub fn get_parent_at_level(&self, level: u32) -> Option<Self> {
        if self.level == 0 || level > self.level {
            return None;
        }
        let lsb = 1u64 << (2 * (MAX_LEVEL - level));
        Self::new((self.cell_id & lsb.wrapping_neg()) | lsb)
    }

    /// Preserves Cesium's extra position zero at level zero: faces 1 and 2
    /// produce roots on faces 2 and 4, while faces 3–5 yield invalid IDs.
    pub fn from_face_position_level(face: u8, position: u64, level: u32) -> Option<Self> {
        if face > 5 || level > MAX_LEVEL || position >= 1u64 << (2 * level) {
            return None;
        }
        let face_shift = if level == 0 { 62 } else { 61 };
        let id = ((face as u128) << face_shift) | (((position as u128) * 2 + 1) << (2 * (MAX_LEVEL - level)));
        Self::new(u64::try_from(id).ok()?)
    }

    pub fn get_center(&self, ellipsoid: &Ellipsoid) -> Cartesian3 {
        let (face, i, j) = self.face_ij();
        let correction = if self.level == MAX_LEVEL {
            1
        } else if (i as u64 ^ (self.cell_id >> 2)) & 1 != 0 {
            2
        } else {
            0
        };
        let s = (i as f64 * 2.0 + correction as f64) / (1u64 << 31) as f64;
        let t = (j as f64 * 2.0 + correction as f64) / (1u64 << 31) as f64;
        geodetic_point(face, st_to_uv(s), st_to_uv(t), ellipsoid)
    }
    pub fn get_vertex(&self, index: u8, ellipsoid: &Ellipsoid) -> Option<Cartesian3> {
        if index > 3 {
            return None;
        }
        let (face, i, j) = self.face_ij();
        let size = 1u32 << (MAX_LEVEL - self.level);
        let bounds = |coordinate: u32| {
            let low = coordinate & size.wrapping_neg();
            [st_to_uv(low as f64 / (1u64 << 30) as f64), st_to_uv((low + size) as f64 / (1u64 << 30) as f64)]
        };
        let u = bounds(i);
        let v = bounds(j);
        let y = (index >> 1) & 1;
        Some(geodetic_point(face, u[(y ^ (index & 1)) as usize], v[y as usize], ellipsoid))
    }

    fn face_ij(&self) -> (u8, u32, u32) {
        let face = (self.cell_id >> 61) as u8;
        let mut orientation = (face & 1) as u32;
        let (mut i, mut j) = (0, 0);
        // The lookup table in Cesium groups four steps. Walking the same
        // orientation transitions directly produces the identical 30-bit IJ.
        for k in (0..30).rev() {
            let position = ((self.cell_id >> (k * 2 + 1)) & 3) as usize;
            let ij = POSITION_TO_IJ[orientation as usize][position];
            i = (i << 1) | (ij >> 1);
            j = (j << 1) | (ij & 1);
            orientation ^= ORIENTATION_MASK[position];
        }
        (face, i, j)
    }
}

fn st_to_uv(s: f64) -> f64 {
    if s >= 0.5 {
        (1.0 / 3.0) * (4.0 * s * s - 1.0)
    } else {
        (1.0 / 3.0) * (1.0 - 4.0 * (1.0 - s) * (1.0 - s))
    }
}
fn geodetic_point(face: u8, u: f64, v: f64, ellipsoid: &Ellipsoid) -> Cartesian3 {
    let xyz = match face {
        0 => Cartesian3::new(1.0, u, v),
        1 => Cartesian3::new(-u, 1.0, v),
        2 => Cartesian3::new(-u, -v, 1.0),
        3 => Cartesian3::new(-1.0, -v, -u),
        4 => Cartesian3::new(v, -1.0, -u),
        _ => Cartesian3::new(v, u, -1.0),
    }
    .normalize();
    let c = Ellipsoid::UNIT_SPHERE.cartesian_to_cartographic(xyz).unwrap();
    ellipsoid.cartographic_to_cartesian(&c)
}
