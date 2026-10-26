//! CesiumJS `Cesium3DTileStyleEngine`: tracks style dirty state and applies style
//! updates across visible tiles.

use super::cesium_3d_tile_style::Cesium3DTileStyle;

/// Trait for a tile that can receive styles during tileset style updates.
pub trait StyleableTile {
    fn last_style_time(&self) -> u64;
    fn set_last_style_time(&mut self, time: u64);
    fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>);
    fn features_length(&self) -> usize;
}

/// Tracks style assignment, dirty states, and styling timestamps for a tileset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cesium3DTileStyleEngine {
    style: Option<Cesium3DTileStyle>,
    style_dirty: bool,
    last_style_time: u64,
}

impl Cesium3DTileStyleEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn style(&self) -> Option<&Cesium3DTileStyle> {
        self.style.as_ref()
    }

    pub fn set_style(&mut self, style: Option<Cesium3DTileStyle>) {
        if self.style == style {
            return;
        }
        self.style = style;
        self.style_dirty = true;
    }

    pub fn is_dirty(&self) -> bool {
        self.style_dirty
    }

    pub fn make_dirty(&mut self) {
        self.style_dirty = true;
    }

    pub fn reset_dirty(&mut self) {
        self.style_dirty = false;
    }

    pub fn last_style_time(&self) -> u64 {
        self.last_style_time
    }

    /// Applies the current style to candidate tiles (either all selected tiles if style changed,
    /// or newly selected tiles to style). Returns `(tiles_styled, features_styled)`.
    pub fn apply_style_to_tiles<T: StyleableTile>(&mut self, tiles: &mut [T]) -> (usize, usize) {
        if self.style_dirty {
            self.last_style_time += 1;
        }

        let last_time = self.last_style_time;
        let mut tiles_styled = 0;
        let mut features_styled = 0;

        for tile in tiles {
            if tile.last_style_time() != last_time {
                tile.set_last_style_time(last_time);
                tile.apply_style(self.style.as_ref());
                features_styled += tile.features_length();
                tiles_styled += 1;
            }
        }

        (tiles_styled, features_styled)
    }
}
