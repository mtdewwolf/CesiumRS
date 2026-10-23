//! CesiumJS `ImplicitTileCoordinates`: the level and x/y(/z) of a tile in an
//! implicit quadtree or octree, with the subtree it belongs to.
//!
//! Cesium computes with doubles and 32-bit bitwise operators; the port keeps
//! that arithmetic (shift counts modulo 32, wrapping), so out-of-range
//! coordinates fail the same checks. Those checks are Cesium's
//! `DeveloperError`s, returned as [`MetadataError::Developer`].

use crate::core::math::log2;
use crate::core::MortonOrder;

use super::implicit_subdivision_scheme::ImplicitSubdivisionScheme;
use super::metadata_value::{developer, MetadataError};

type Result<T> = std::result::Result<T, MetadataError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImplicitTileCoordinates {
    pub subdivision_scheme: ImplicitSubdivisionScheme,
    /// Levels per subtree, at least 1.
    pub subtree_levels: u32,
    pub level: u32,
    pub x: u32,
    pub y: u32,
    /// Octrees only.
    pub z: Option<u32>,
}

/// JavaScript `a << b` on 32-bit integers.
fn shl(a: i64, b: i64) -> i64 {
    (a as i32).wrapping_shl((b as u32) & 31) as i64
}

/// JavaScript `a >> b`.
fn shr(a: i64, b: i64) -> i64 {
    ((a as i32) >> ((b as u32) & 31)) as i64
}

impl ImplicitTileCoordinates {
    /// `new ImplicitTileCoordinates(options)`. Octrees need `z`; a
    /// quadtree ignores it. Negative or out-of-range coordinates fail.
    pub fn new(
        subdivision_scheme: ImplicitSubdivisionScheme,
        subtree_levels: u32,
        level: i64,
        x: i64,
        y: i64,
        z: Option<i64>,
    ) -> Result<Self> {
        let octree = subdivision_scheme == ImplicitSubdivisionScheme::Octree;
        if subtree_levels == 0 {
            return Err(developer("subtreeLevels must be at least 1"));
        }
        if octree && z.is_none() {
            return Err(developer("Expected options.z to be typeof number, actual typeof was undefined"));
        }
        if level < 0 {
            return Err(developer("level must be non-negative"));
        }
        if x < 0 {
            return Err(developer("x must be non-negative"));
        }
        if y < 0 {
            return Err(developer("y must be non-negative"));
        }
        if octree && z.is_some_and(|z| z < 0) {
            return Err(developer("z must be non-negative"));
        }
        let dimension_at_level = shl(1, level);
        if x >= dimension_at_level {
            return Err(developer("x is out of range"));
        }
        if y >= dimension_at_level {
            return Err(developer("y is out of range"));
        }
        if octree && z.is_some_and(|z| z >= dimension_at_level) {
            return Err(developer("z is out of range"));
        }
        Ok(Self {
            subdivision_scheme,
            subtree_levels,
            level: level as u32,
            x: x as u32,
            y: y as u32,
            z: if octree { z.map(|z| z as u32) } else { None },
        })
    }

    fn octree(&self) -> bool {
        self.subdivision_scheme == ImplicitSubdivisionScheme::Octree
    }

    fn z(&self) -> i64 {
        self.z.unwrap_or(0) as i64
    }

    /// Coordinates of the same shape.
    fn with(&self, level: i64, x: i64, y: i64, z: i64) -> Result<Self> {
        Self::new(self.subdivision_scheme, self.subtree_levels, level, x, y, self.octree().then_some(z))
    }

    /// The tile's index among its siblings.
    pub fn child_index(&self) -> u32 {
        let mut child_index = self.x & 1;
        child_index |= (self.y & 1) << 1;
        if self.octree() {
            child_index |= (self.z() as u32 & 1) << 2;
        }
        child_index
    }

    /// The Morton index within the level; `None` past 16 bits per
    /// coordinate (10 for octrees), where Cesium's encoder fails a check.
    pub fn morton_index(&self) -> Option<u32> {
        if self.octree() {
            MortonOrder::encode_3d(self.x, self.y, self.z() as u32)
        } else {
            MortonOrder::encode_2d(self.x, self.y)
        }
    }

    /// The index of the tile in a breadth-first numbering of the tree: the
    /// tile count of the levels above plus the Morton index. Cesium's level
    /// offset uses a 32-bit shift, so it is only exact for small levels.
    pub fn tile_index(&self) -> Option<f64> {
        let level = self.level as i64;
        let level_offset =
            if self.octree() { (shl(1, 3 * level) - 1) as f64 / 7.0 } else { (shl(1, 2 * level) - 1) as f64 / 3.0 };
        Some(level_offset + self.morton_index()? as f64)
    }

    fn check_matching_subtree_shape(&self, other: &Self) -> Result<()> {
        if self.subdivision_scheme != other.subdivision_scheme {
            return Err(developer("coordinates must have same subdivisionScheme"));
        }
        if self.subtree_levels != other.subtree_levels {
            return Err(developer("coordinates must have same subtreeLevels"));
        }
        Ok(())
    }

    /// The tile `offset` below this one, `offset` being relative to it.
    pub fn get_descendant_coordinates(&self, offset: &Self) -> Result<Self> {
        self.check_matching_subtree_shape(offset)?;
        let level = offset.level as i64;
        self.with(
            self.level as i64 + level,
            shl(self.x as i64, level) + offset.x as i64,
            shl(self.y as i64, level) + offset.y as i64,
            shl(self.z(), level) + offset.z(),
        )
    }

    pub fn get_ancestor_coordinates(&self, offset_levels: i64) -> Result<Self> {
        if offset_levels < 0 {
            return Err(developer("offsetLevels must be non-negative"));
        }
        if offset_levels > self.level as i64 {
            return Err(developer("ancestor cannot be above the tileset root"));
        }
        let divisor = shl(1, offset_levels) as f64;
        let floor = |v: i64| (v as f64 / divisor).floor() as i64;
        self.with(self.level as i64 - offset_levels, floor(self.x as i64), floor(self.y as i64), floor(self.z()))
    }

    /// `descendant` relative to this tile.
    pub fn get_offset_coordinates(&self, descendant: &Self) -> Result<Self> {
        if !self.is_equal(descendant) && !self.is_ancestor(descendant)? {
            return Err(developer("this is not an ancestor of descendant"));
        }
        self.check_matching_subtree_shape(descendant)?;
        let offset_level = descendant.level as i64 - self.level as i64;
        let dimension = shl(1, offset_level);
        let rem = |v: i64| if dimension == 0 { 0 } else { v % dimension };
        self.with(offset_level, rem(descendant.x as i64), rem(descendant.y as i64), rem(descendant.z()))
    }

    /// Child `child_index`, numbered like [`Self::child_index`].
    pub fn get_child_coordinates(&self, child_index: i64) -> Result<Self> {
        let branching_factor = self.subdivision_scheme.get_branching_factor() as i64;
        if child_index < 0 || branching_factor <= child_index {
            return Err(developer(format!("childIndex must be at least 0 and less than {branching_factor}")));
        }
        self.with(
            self.level as i64 + 1,
            2 * self.x as i64 + child_index % 2,
            2 * self.y as i64 + (child_index / 2) % 2,
            2 * self.z() + (child_index / 4) % 2,
        )
    }

    /// The root of the subtree containing this tile.
    pub fn get_subtree_coordinates(&self) -> Result<Self> {
        self.get_ancestor_coordinates((self.level % self.subtree_levels) as i64)
    }

    /// The root of the parent subtree.
    pub fn get_parent_subtree_coordinates(&self) -> Result<Self> {
        self.get_ancestor_coordinates((self.level % self.subtree_levels + self.subtree_levels) as i64)
    }

    pub fn is_ancestor(&self, descendant: &Self) -> Result<bool> {
        self.check_matching_subtree_shape(descendant)?;
        let level_difference = descendant.level as i64 - self.level as i64;
        if level_difference <= 0 {
            return Ok(false);
        }
        let is_x = self.x as i64 == shr(descendant.x as i64, level_difference);
        let is_y = self.y as i64 == shr(descendant.y as i64, level_difference);
        if self.octree() {
            return Ok(is_x && is_y && self.z() == shr(descendant.z(), level_difference));
        }
        Ok(is_x && is_y)
    }

    pub fn is_equal(&self, other: &Self) -> bool {
        self.subdivision_scheme == other.subdivision_scheme
            && self.subtree_levels == other.subtree_levels
            && self.level == other.level
            && self.x == other.x
            && self.y == other.y
            && (!self.octree() || self.z == other.z)
    }

    pub fn is_implicit_tileset_root(&self) -> bool {
        self.level == 0
    }

    pub fn is_subtree_root(&self) -> bool {
        self.level.is_multiple_of(self.subtree_levels)
    }

    pub fn is_bottom_of_subtree(&self) -> bool {
        self.level % self.subtree_levels == self.subtree_levels - 1
    }

    /// The `{level}`, `{x}`, `{y}` (and `{z}`) values for URI templates, in
    /// that order.
    pub fn get_template_values(&self) -> Vec<(&'static str, u32)> {
        let mut values = vec![("level", self.level), ("x", self.x), ("y", self.y)];
        if let (true, Some(z)) = (self.octree(), self.z) {
            values.push(("z", z));
        }
        values
    }

    /// The tile at `morton_index` on `level`. Octree indices must fit in 30
    /// bits.
    pub fn from_morton_index(
        subdivision_scheme: ImplicitSubdivisionScheme,
        subtree_levels: u32,
        level: i64,
        morton_index: u32,
    ) -> Result<Self> {
        if subdivision_scheme == ImplicitSubdivisionScheme::Octree {
            let [x, y, z] =
                MortonOrder::decode_3d(morton_index).ok_or_else(|| developer("mortonIndex must be less than 2^30"))?;
            return Self::new(subdivision_scheme, subtree_levels, level, x as i64, y as i64, Some(z as i64));
        }
        let [x, y] = MortonOrder::decode_2d(morton_index);
        Self::new(subdivision_scheme, subtree_levels, level, x as i64, y as i64, None)
    }

    /// The tile at a breadth-first `tile_index` (see [`Self::tile_index`]).
    pub fn from_tile_index(
        subdivision_scheme: ImplicitSubdivisionScheme,
        subtree_levels: u32,
        tile_index: u32,
    ) -> Result<Self> {
        let t = tile_index as f64;
        let (level, level_offset) = if subdivision_scheme == ImplicitSubdivisionScheme::Octree {
            let level = (log2(7.0 * t + 1.0) / 3.0).floor() as i64;
            (level, (shl(1, 3 * level) - 1) as f64 / 7.0)
        } else {
            let level = (log2(3.0 * t + 1.0) / 2.0).floor() as i64;
            (level, (shl(1, 2 * level) - 1) as f64 / 3.0)
        };
        let morton_index = t - level_offset;
        if morton_index.fract() != 0.0 || !(0.0..=u32::MAX as f64).contains(&morton_index) {
            return Err(developer("mortonIndex must be a 32-bit unsigned integer"));
        }
        Self::from_morton_index(subdivision_scheme, subtree_levels, level, morton_index as u32)
    }
}
