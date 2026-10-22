//! CesiumJS `Cesium3DTilePass`: the traversal passes a tileset runs, and the
//! options each pass uses.

numeric_enum!(Cesium3DTilePass: i32 {
    Render = 0 => "RENDER",
    Pick = 1 => "PICK",
    Shadow = 2 => "SHADOW",
    Preload = 3 => "PRELOAD",
    PreloadFlight = 4 => "PRELOAD_FLIGHT",
    RequestRenderModeDeferCheck = 5 => "REQUEST_RENDER_MODE_DEFER_CHECK",
    MostDetailedPreload = 6 => "MOST_DETAILED_PRELOAD",
    MostDetailedPick = 7 => "MOST_DETAILED_PICK",
});

/// What a pass does with the tiles it visits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassOptions {
    pub pass: Cesium3DTilePass,
    pub is_render: bool,
    pub request_tiles: bool,
    pub ignore_commands: bool,
}

impl Cesium3DTilePass {
    pub const NUMBER_OF_PASSES: i32 = 8;

    pub fn get_pass_options(self) -> PassOptions {
        let (is_render, request_tiles, ignore_commands) = match self {
            Self::Render => (true, true, false),
            Self::Pick | Self::MostDetailedPick => (false, false, false),
            Self::Shadow => (false, true, false),
            Self::Preload | Self::PreloadFlight | Self::RequestRenderModeDeferCheck | Self::MostDetailedPreload => {
                (false, true, true)
            }
        };
        PassOptions { pass: self, is_render, request_tiles, ignore_commands }
    }
}
