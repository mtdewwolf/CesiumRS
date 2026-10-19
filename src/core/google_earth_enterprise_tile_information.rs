//! CesiumJS `GoogleEarthEnterpriseTileInformation`.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoogleEarthEnterpriseTileInformation {
    pub bits: u8,
    pub cnode_version: u16,
    pub imagery_version: u16,
    pub terrain_version: u16,
    pub imagery_provider: u8,
    pub terrain_provider: u8,
    pub ancestor_has_terrain: bool,
    pub terrain_state: Option<i32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GoogleEarthEnterpriseTileInformationOptions {
    pub bits: u8,
    pub cnode_version: u16,
    pub imagery_version: u16,
    pub terrain_version: u16,
    pub imagery_provider: u8,
    pub terrain_provider: u8,
}

impl GoogleEarthEnterpriseTileInformation {
    pub fn new(options: GoogleEarthEnterpriseTileInformationOptions) -> Self {
        Self {
            bits: options.bits,
            cnode_version: options.cnode_version,
            imagery_version: options.imagery_version,
            terrain_version: options.terrain_version,
            imagery_provider: options.imagery_provider,
            terrain_provider: options.terrain_provider,
            ancestor_has_terrain: false,
            terrain_state: None,
        }
    }

    pub fn set_parent(&mut self, parent: &Self) {
        // Cesium deliberately tests this tile's terrain flag, not the parent's.
        self.ancestor_has_terrain = parent.ancestor_has_terrain || self.has_terrain();
    }
    pub fn has_subtree(&self) -> bool {
        self.bits & 0x10 != 0
    }
    pub fn has_imagery(&self) -> bool {
        self.bits & 0x40 != 0
    }
    pub fn has_terrain(&self) -> bool {
        self.bits & 0x80 != 0
    }
    pub fn has_children(&self) -> bool {
        self.bits & 0x0f != 0
    }
    pub fn has_child(&self, index: usize) -> bool {
        index < 4 && self.bits & (1 << index) != 0
    }
    pub fn get_child_bitmask(&self) -> u8 {
        self.bits & 0x0f
    }
}
