//! CesiumJS `ImageryLayer`: displays tiled image data from a single imagery provider
//! on a globe or 3D tileset.

use super::frame_state::FrameState;
use super::imagery::Imagery;
use super::imagery_provider::ImageryProvider;
use super::ion_world_imagery_style::IonWorldImageryStyle;
use super::model::ImageryLayerSettings;
use super::split_direction::SplitDirection;
use super::tile_imagery::TileImagery;
use crate::core::{Cartesian4, Color, Event, Rectangle, RuntimeError, TilingScheme};
use crate::renderer::{TextureMagnificationFilter, TextureMinificationFilter};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// A shared handle to a mutable [`ImageryLayer`].
pub type ImageryLayerHandle = Rc<RefCell<ImageryLayer>>;

/// Initialization options for an [`ImageryLayer`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImageryLayerOptions {
    pub rectangle: Option<Rectangle>,
    pub alpha: Option<f64>,
    pub night_alpha: Option<f64>,
    pub day_alpha: Option<f64>,
    pub brightness: Option<f64>,
    pub contrast: Option<f64>,
    pub hue: Option<f64>,
    pub saturation: Option<f64>,
    pub gamma: Option<f64>,
    pub split_direction: Option<SplitDirection>,
    pub minification_filter: Option<TextureMinificationFilter>,
    pub magnification_filter: Option<TextureMagnificationFilter>,
    pub show: Option<bool>,
    pub maximum_anisotropy: Option<f64>,
    pub minimum_terrain_level: Option<u32>,
    pub maximum_terrain_level: Option<u32>,
    pub cutout_rectangle: Option<Rectangle>,
    pub color_to_alpha: Option<Color>,
    pub color_to_alpha_threshold: Option<f64>,
}


/// An imagery layer that displays tiled image data from a single imagery provider.
pub struct ImageryLayer {
    imagery_provider: Option<Arc<dyn ImageryProvider>>,
    rectangle: Rectangle,
    pub alpha: f64,
    pub night_alpha: f64,
    pub day_alpha: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub hue: f64,
    pub saturation: f64,
    pub gamma: f64,
    pub split_direction: SplitDirection,
    pub minification_filter: TextureMinificationFilter,
    pub magnification_filter: TextureMagnificationFilter,
    pub show: bool,
    pub maximum_anisotropy: Option<f64>,
    pub minimum_terrain_level: Option<u32>,
    pub maximum_terrain_level: Option<u32>,
    pub cutout_rectangle: Option<Rectangle>,
    pub color_to_alpha: Option<Color>,
    pub color_to_alpha_threshold: f64,

    // Internal state
    pub(crate) layer_index: isize,
    pub(crate) is_base_layer: bool,
    pub(crate) show_last: Option<bool>,
    destroyed: bool,

    // Events
    pub ready_event: Event<()>,
    pub error_event: Event<RuntimeError>,

    // Cache
    imagery_cache: HashMap<(i32, i32, u32), Rc<RefCell<Imagery>>>,
}

impl std::fmt::Debug for ImageryLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageryLayer")
            .field("rectangle", &self.rectangle)
            .field("alpha", &self.alpha)
            .field("show", &self.show)
            .field("layer_index", &self.layer_index)
            .field("is_base_layer", &self.is_base_layer)
            .field("destroyed", &self.destroyed)
            .finish()
    }
}

impl Default for ImageryLayer {
    fn default() -> Self {
        Self::new(None, ImageryLayerOptions::default())
    }
}

impl ImageryLayer {
    pub const DEFAULT_BRIGHTNESS: f64 = 1.0;
    pub const DEFAULT_CONTRAST: f64 = 1.0;
    pub const DEFAULT_HUE: f64 = 0.0;
    pub const DEFAULT_SATURATION: f64 = 1.0;
    pub const DEFAULT_GAMMA: f64 = 1.0;
    pub const DEFAULT_SPLIT: SplitDirection = SplitDirection::None;
    pub const DEFAULT_MINIFICATION_FILTER: TextureMinificationFilter = TextureMinificationFilter::Linear;
    pub const DEFAULT_MAGNIFICATION_FILTER: TextureMagnificationFilter = TextureMagnificationFilter::Linear;
    pub const DEFAULT_APPLY_COLOR_TO_ALPHA_THRESHOLD: f64 = 0.004;

    pub fn new(
        imagery_provider: Option<Arc<dyn ImageryProvider>>,
        options: ImageryLayerOptions,
    ) -> Self {
        let rectangle = options.rectangle.unwrap_or(Rectangle::MAX_VALUE);
        let alpha = options.alpha.unwrap_or(1.0);
        let night_alpha = options.night_alpha.unwrap_or(1.0);
        let day_alpha = options.day_alpha.unwrap_or(1.0);
        let brightness = options.brightness.unwrap_or(Self::DEFAULT_BRIGHTNESS);
        let contrast = options.contrast.unwrap_or(Self::DEFAULT_CONTRAST);
        let hue = options.hue.unwrap_or(Self::DEFAULT_HUE);
        let saturation = options.saturation.unwrap_or(Self::DEFAULT_SATURATION);
        let gamma = options.gamma.unwrap_or(Self::DEFAULT_GAMMA);
        let split_direction = options.split_direction.unwrap_or(Self::DEFAULT_SPLIT);
        let minification_filter = options.minification_filter.unwrap_or(Self::DEFAULT_MINIFICATION_FILTER);
        let magnification_filter = options.magnification_filter.unwrap_or(Self::DEFAULT_MAGNIFICATION_FILTER);
        let show = options.show.unwrap_or(true);
        let color_to_alpha_threshold = options
            .color_to_alpha_threshold
            .unwrap_or(Self::DEFAULT_APPLY_COLOR_TO_ALPHA_THRESHOLD);

        Self {
            imagery_provider,
            rectangle,
            alpha,
            night_alpha,
            day_alpha,
            brightness,
            contrast,
            hue,
            saturation,
            gamma,
            split_direction,
            minification_filter,
            magnification_filter,
            show,
            maximum_anisotropy: options.maximum_anisotropy,
            minimum_terrain_level: options.minimum_terrain_level,
            maximum_terrain_level: options.maximum_terrain_level,
            cutout_rectangle: options.cutout_rectangle,
            color_to_alpha: options.color_to_alpha,
            color_to_alpha_threshold,
            layer_index: -1,
            is_base_layer: false,
            show_last: Some(show),
            destroyed: false,
            ready_event: Event::new(),
            error_event: Event::new(),
            imagery_cache: HashMap::new(),
        }
    }

    /// Wraps a new [`ImageryLayer`] in an [`ImageryLayerHandle`] (`Rc<RefCell<ImageryLayer>>`).
    pub fn handle(
        imagery_provider: Option<Arc<dyn ImageryProvider>>,
        options: ImageryLayerOptions,
    ) -> ImageryLayerHandle {
        Rc::new(RefCell::new(Self::new(imagery_provider, options)))
    }

    /// Converts this layer into an [].
    pub fn into_handle(self) -> ImageryLayerHandle {
        Rc::new(RefCell::new(self))
    }

    pub fn from_world_imagery(
        _style: Option<IonWorldImageryStyle>,
        layer_options: ImageryLayerOptions,
    ) -> Self {
        Self::new(None, layer_options)
    }

    pub fn imagery_provider(&self) -> Option<&Arc<dyn ImageryProvider>> {
        self.imagery_provider.as_ref()
    }

    pub fn set_imagery_provider(&mut self, provider: Option<Arc<dyn ImageryProvider>>) {
        self.imagery_provider = provider;
        if self.imagery_provider.is_some() {
            self.ready_event.raise_event(&());
        }
    }

    pub fn ready(&self) -> bool {
        self.imagery_provider.is_some()
    }

    pub fn rectangle(&self) -> &Rectangle {
        &self.rectangle
    }

    /// Computes the intersection of this layer's rectangle with the imagery provider's
    /// availability rectangle.
    pub fn get_imagery_rectangle(&self) -> Option<Rectangle> {
        let provider = self.imagery_provider.as_ref()?;
        provider.rectangle().intersection(&self.rectangle)
    }

    pub fn is_base_layer(&self) -> bool {
        self.is_base_layer
    }

    pub fn set_base_layer(&mut self, is_base: bool) {
        self.is_base_layer = is_base;
    }

    pub fn layer_index(&self) -> isize {
        self.layer_index
    }

    pub fn set_layer_index(&mut self, index: isize) {
        self.layer_index = index;
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.destroyed = true;
        self.imagery_cache.clear();
        Ok(())
    }

    /// Computes the intersection of an imagery provider's rectangle and a layer's rectangle.
    pub fn get_imagery_rectangle_for_rectangles(
        provider_rectangle: &Rectangle,
        layer_rectangle: &Rectangle,
    ) -> Option<Rectangle> {
        provider_rectangle.intersection(layer_rectangle)
    }

    /// Computes the level of detail with the specified world coordinate spacing between texels
    /// given a tiling scheme and tile width.
    pub fn compute_level_with_maximum_texel_spacing(
        tiling_scheme: &dyn TilingScheme,
        tile_width: f64,
        texel_spacing: f64,
        latitude_closest_to_equator: f64,
    ) -> i32 {
        let ellipsoid = tiling_scheme.ellipsoid();
        let latitude_factor = if tiling_scheme.projection().is_geographic() {
            1.0
        } else {
            latitude_closest_to_equator.cos()
        };

        let tiling_scheme_rect = tiling_scheme.rectangle();
        let level_zero_max_spacing = (ellipsoid.maximum_radius * tiling_scheme_rect.width() * latitude_factor)
            / (tile_width * (tiling_scheme.get_number_of_x_tiles_at_level(0) as f64));

        let two_to_the_level = level_zero_max_spacing / texel_spacing;
        let level = two_to_the_level.ln() / std::f64::consts::LN_2;
        level.round() as i32
    }

    /// Computes the level of detail with the specified world coordinate spacing between texels.
    pub fn get_level_with_maximum_texel_spacing(
        &self,
        texel_spacing: f64,
        latitude_closest_to_equator: f64,
    ) -> Option<i32> {
        let provider = self.imagery_provider.as_ref()?;
        Some(Self::compute_level_with_maximum_texel_spacing(
            provider.tiling_scheme(),
            provider.tile_width(),
            texel_spacing,
            latitude_closest_to_equator,
        ))
    }

    /// Calculates the translation and scale for texture mapping between a terrain tile
    /// and an imagery tile.
    pub fn calculate_texture_translation_and_scale(
        terrain_rectangle: &Rectangle,
        imagery_rectangle: &Rectangle,
    ) -> Cartesian4 {
        let terrain_width = terrain_rectangle.width();
        let terrain_height = terrain_rectangle.height();
        let scale_x = terrain_width / imagery_rectangle.width();
        let scale_y = terrain_height / imagery_rectangle.height();
        Cartesian4::new(
            (scale_x * (terrain_rectangle.west - imagery_rectangle.west)) / terrain_width,
            (scale_y * (terrain_rectangle.south - imagery_rectangle.south)) / terrain_height,
            scale_x,
            scale_y,
        )
    }

    /// Retrieves or allocates an imagery tile in cache.
    pub fn get_imagery_from_cache(
        &mut self,
        x: i32,
        y: i32,
        level: u32,
        rectangle: Option<Rectangle>,
    ) -> Rc<RefCell<Imagery>> {
        let key = (x, y, level);
        if let Some(imagery) = self.imagery_cache.get(&key) {
            let img = imagery.clone();
            img.borrow_mut().add_reference();
            return img;
        }

        let rect = rectangle.unwrap_or_else(|| {
            if let Some(provider) = &self.imagery_provider {
                provider.tiling_scheme().tile_xy_to_rectangle(x as f64, y as f64, level)
            } else {
                Rectangle::MAX_VALUE
            }
        });

        let parent = if level > 0 {
            let parent_x = x / 2;
            let parent_y = y / 2;
            let parent_level = level - 1;
            Some(self.get_imagery_from_cache(parent_x, parent_y, parent_level, None))
        } else {
            None
        };

        let mut imagery = Imagery::new(x, y, level, rect, parent);
        imagery.add_reference();
        let rc = Rc::new(RefCell::new(imagery));
        self.imagery_cache.insert(key, rc.clone());
        rc
    }

    pub fn remove_imagery_from_cache(&mut self, x: i32, y: i32, level: u32) {
        self.imagery_cache.remove(&(x, y, level));
    }

    /// Creates skeletons for imagery tiles overlapping a terrain tile.
    pub fn create_tile_imagery_skeletons(
        &mut self,
        tile_rectangle: &Rectangle,
        tile_level: u32,
        target_geometric_error: f64,
    ) -> Vec<TileImagery> {
        let provider = match self.imagery_provider.as_ref() {
            Some(p) => p,
            None => return Vec::new(),
        };

        if let Some(min_level) = self.minimum_terrain_level {
            if tile_level < min_level {
                return Vec::new();
            }
        }
        if let Some(max_level) = self.maximum_terrain_level {
            if tile_level > max_level {
                return Vec::new();
            }
        }

        let imagery_bounds = match provider.rectangle().intersection(&self.rectangle) {
            Some(b) => b,
            None => return Vec::new(),
        };

        let overlap = match tile_rectangle.intersection(&imagery_bounds) {
            Some(r) => r,
            None => {
                if !self.is_base_layer {
                    return Vec::new();
                }
                imagery_bounds
            }
        };

        let latitude_closest_to_equator = if overlap.south > 0.0 {
            overlap.south
        } else if overlap.north < 0.0 {
            overlap.north
        } else {
            0.0
        };

        let imagery_level = self
            .get_level_with_maximum_texel_spacing(target_geometric_error, latitude_closest_to_equator)
            .unwrap_or(0)
            .max(0) as u32;

        let max_lvl = provider.maximum_level().unwrap_or(imagery_level);
        let imagery_level = imagery_level.clamp(provider.minimum_level(), max_lvl);

        let tiling_scheme = provider.tiling_scheme();
        let nw = match tiling_scheme.position_to_tile_xy(&overlap.northwest(), imagery_level) {
            Some(c) => c,
            None => return Vec::new(),
        };
        let se = match tiling_scheme.position_to_tile_xy(&overlap.southeast(), imagery_level) {
            Some(c) => c,
            None => return Vec::new(),
        };

        let mut results = Vec::new();
        for x in (nw.x as i32)..=(se.x as i32) {
            for y in (nw.y as i32)..=(se.y as i32) {
                let img = self.get_imagery_from_cache(x, y, imagery_level, None);
                let tex_coords = Cartesian4::new(0.0, 0.0, 1.0, 1.0);
                results.push(TileImagery::new(img, tex_coords, false));
            }
        }
        results
    }

    pub fn queue_reprojection_commands(&self, _frame_state: &mut FrameState) {}

    pub fn cancel_reprojections(&self) {}
}

impl ImageryLayerSettings for ImageryLayer {
    fn show(&self) -> bool {
        self.show
    }
    fn alpha(&self) -> f64 {
        self.alpha
    }
    fn brightness(&self) -> f64 {
        self.brightness
    }
    fn contrast(&self) -> f64 {
        self.contrast
    }
    fn hue(&self) -> f64 {
        self.hue
    }
    fn saturation(&self) -> f64 {
        self.saturation
    }
    fn gamma(&self) -> f64 {
        self.gamma
    }
    fn color_to_alpha(&self) -> Option<Color> {
        self.color_to_alpha
    }
}

impl ImageryLayerSettings for ImageryLayerHandle {
    fn show(&self) -> bool {
        self.borrow().show()
    }
    fn alpha(&self) -> f64 {
        self.borrow().alpha()
    }
    fn brightness(&self) -> f64 {
        self.borrow().brightness()
    }
    fn contrast(&self) -> f64 {
        self.borrow().contrast()
    }
    fn hue(&self) -> f64 {
        self.borrow().hue()
    }
    fn saturation(&self) -> f64 {
        self.borrow().saturation()
    }
    fn gamma(&self) -> f64 {
        self.borrow().gamma()
    }
    fn color_to_alpha(&self) -> Option<Color> {
        self.borrow().color_to_alpha()
    }
}
