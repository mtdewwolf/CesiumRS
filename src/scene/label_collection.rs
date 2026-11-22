//! CesiumJS `LabelCollection`: label collection management, layout,
//! pluggable SDF glyph rasterization, and GPU rendering backed by two
//! [`BillboardCollection`](super::BillboardCollection) instances (backgrounds + SDF glyphs).

use crate as _root;

use super::{
    billboard::{Billboard, BillboardImage, BillboardOptions, HeightSampler},
    billboard_collection::{BillboardCollection, BillboardCollectionOptions},
    frame_state::FrameState,
    label::*,
    scene::{Result as SceneResult, ScenePrimitive},
    Cesium3DTilePass,
};
use _root::{
    core::{BoundingSphere, Cartesian2, Cartesian3, Color, Matrix4, RuntimeError},
    renderer::{Context, Gl},
    scene::{BlendOption, HeightReference, HorizontalOrigin, LabelStyle, VerticalOrigin},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

/// Rasterized SDF glyph image and layout metrics.
#[derive(Clone, Debug)]
pub struct GlyphBitmap {
    pub width: u32,
    pub height: u32,
    pub advance: f64,
    pub ascent: f64,
    pub descent: f64,
    /// RGBA8 pixels where alpha stores the SDF distance (cutoff at 0.25 -> 191).
    pub rgba_sdf: Vec<u8>,
}

/// Pluggable glyph rasterizer supplying SDF glyph bitmaps and typography metrics
/// for [`LabelCollection`]. Hosts may plug in a browser Canvas2D or native font
/// rasterizer; [`DefaultSdfGlyphRasterizer`] provides a deterministic SDF bitmap fallback.
pub trait GlyphRasterizer {
    fn rasterize(&self, ch: char, font: &ParsedFont) -> GlyphBitmap;
}

/// Built-in deterministic SDF glyph rasterizer producing real Euclidean-distance
/// RGBA SDF textures matching `sdf_settings::PADDING` / `RADIUS` / `CUTOFF`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultSdfGlyphRasterizer;

impl GlyphRasterizer for DefaultSdfGlyphRasterizer {
    fn rasterize(&self, ch: char, _font: &ParsedFont) -> GlyphBitmap {
        let pad = 4usize;
        let gw = 8usize;
        let gh = 12usize;
        let w = gw + pad * 2;
        let h = gh + pad * 2;
        let seed = ch as u32;
        let mut inside = vec![false; w * h];
        if !ch.is_whitespace() {
            // Deterministic glyph silhouette inside the inner gw x gh box
            for y in 0..gh {
                for x in 0..gw {
                    let bit = ((seed.wrapping_mul(2654435761) ^ ((x as u32) * 37 + (y as u32) * 101)) >> 13) & 1;
                    let border = x == 0 || x == gw - 1 || y == 0 || y == gh - 1;
                    inside[(y + pad) * w + (x + pad)] = border || bit == 1;
                }
            }
        }
        let radius = sdf_settings::RADIUS;
        let cutoff = sdf_settings::CUTOFF;
        let mut rgba = vec![255u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let is_in = inside[y * w + x];
                let mut min_dist_sq = radius * radius;
                for ny in 0..h {
                    for nx in 0..w {
                        if inside[ny * w + nx] != is_in {
                            let dx = x as f64 - nx as f64;
                            let dy = y as f64 - ny as f64;
                            let d2 = dx * dx + dy * dy;
                            if d2 < min_dist_sq {
                                min_dist_sq = d2;
                            }
                        }
                    }
                }
                let dist = min_dist_sq.sqrt();
                let signed = if is_in { -dist } else { dist };
                let sdf = (1.0 - (signed / radius + cutoff)).clamp(0.0, 1.0);
                let idx = (y * w + x) * 4;
                rgba[idx] = 255;
                rgba[idx + 1] = 255;
                rgba[idx + 2] = 255;
                rgba[idx + 3] = (sdf * 255.0).round() as u8;
            }
        }
        GlyphBitmap {
            width: w as u32,
            height: h as u32,
            advance: 24.0,
            ascent: 36.0,
            descent: 12.0,
            rgba_sdf: rgba,
        }
    }
}

struct LabelBillboards<G: Gl + 'static> {
    background: Option<Billboard<G>>,
    glyphs: Vec<(bool, f64, f64, Billboard<G>)>,
}

struct LabelCollectionGpu<G: Gl + 'static> {
    backgrounds: BillboardCollection<G>,
    glyphs: BillboardCollection<G>,
    per_label: Vec<LabelBillboards<G>>,
    glyph_cache: HashMap<(char, String), GlyphBitmap>,
    rasterizer: Rc<dyn GlyphRasterizer>,
}

impl<G: Gl + 'static> LabelCollectionGpu<G> {
    fn new(rasterizer: Rc<dyn GlyphRasterizer>) -> Self {
        let backgrounds = BillboardCollection::new(BillboardCollectionOptions::default());
        let mut glyphs = BillboardCollection::new(BillboardCollectionOptions::default());
        glyphs.set_sdf(true);
        Self {
            backgrounds,
            glyphs,
            per_label: Vec::new(),
            glyph_cache: HashMap::new(),
            rasterizer,
        }
    }

    fn set_height_sampler(&mut self, sampler: Option<HeightSampler>) {
        self.backgrounds.set_height_sampler(sampler.clone());
        self.glyphs.set_height_sampler(sampler);
    }

    fn set_rasterizer(&mut self, rasterizer: Rc<dyn GlyphRasterizer>) {
        self.rasterizer = rasterizer;
        self.glyph_cache.clear();
    }

    #[allow(clippy::too_many_arguments)]
    fn sync_and_update(
        &mut self,
        labels: &[LabelHandle],
        show: bool,
        model_matrix: Matrix4,
        debug_show_bounding_volume: bool,
        blend_option: BlendOption,
        coarse_depth_test_distance: f64,
        three_point_depth_test_distance: f64,
        frame: &mut FrameState,
    ) -> SceneResult<()> {
        self.backgrounds.show = show;
        self.backgrounds.model_matrix = model_matrix;
        self.backgrounds.debug_show_bounding_volume = debug_show_bounding_volume;
        self.backgrounds.blend_option = blend_option;
        self.backgrounds.coarse_depth_test_distance = coarse_depth_test_distance;
        self.backgrounds.three_point_depth_test_distance = three_point_depth_test_distance;

        self.glyphs.show = show;
        self.glyphs.model_matrix = model_matrix;
        self.glyphs.debug_show_bounding_volume = debug_show_bounding_volume;
        self.glyphs.blend_option = blend_option;
        self.glyphs.coarse_depth_test_distance = coarse_depth_test_distance;
        self.glyphs.three_point_depth_test_distance = three_point_depth_test_distance;

        // Rebuild billboard backing when label count, glyphs, layout, or background presence changed
        let needs_rebuild = self.per_label.len() != labels.len()
            || labels.iter().zip(self.per_label.iter()).any(|(h, backing)| {
                let l = h.borrow();
                let wants_bg = l.is_attached() && l.show_background();
                l.rebind_all_glyphs() || l.reposition_all_glyphs() || (backing.background.is_some() != wants_bg)
            });

        if needs_rebuild {
            self.backgrounds.remove_all()?;
            self.glyphs.remove_all()?;
            self.per_label.clear();

            for handle in labels {
                let label = handle.borrow();
                if !label.is_attached() {
                    self.per_label.push(LabelBillboards { background: None, glyphs: Vec::new() });
                    continue;
                }
                let hr = if self.glyphs.is_destroyed() { HeightReference::None } else { label.height_reference() };
                let bg = if label.show_background() {
                    let dims = label.background_dimensions.unwrap_or(Cartesian2::new(1.0, 1.0));
                    let tr = label.background_translate.unwrap_or(Cartesian2::ZERO);
                    let opts = BillboardOptions {
                        show: label.show(),
                        position: label.position(),
                        pixel_offset: Cartesian2::new(label.pixel_offset().x + tr.x, label.pixel_offset().y + tr.y),
                        eye_offset: label.eye_offset(),
                        horizontal_origin: HorizontalOrigin::Left,
                        vertical_origin: VerticalOrigin::Bottom,
                        height_reference: hr,
                        scale: label.total_scale(),
                        color: label.background_color(),
                        width: Some(dims.x.max(1.0)),
                        height: Some(dims.y.max(1.0)),
                        scale_by_distance: label.scale_by_distance(),
                        translucency_by_distance: label.translucency_by_distance(),
                        pixel_offset_scale_by_distance: label.pixel_offset_scale_by_distance(),
                        distance_display_condition: label.distance_display_condition(),
                        disable_depth_test_distance: label.disable_depth_test_distance(),
                        id: label.id().map(str::to_string),
                        image_id: Some("czm_label_bg_white".into()),
                        ..Default::default()
                    };
                    let white = BillboardImage::pixels(1, 1, vec![255, 255, 255, 255]);
                    self.backgrounds.add(opts, Some(white)).ok()
                } else {
                    None
                };

                let mut glyph_bbs = Vec::new();
                let rendered = label.rendered_text().to_string();
                let font = label.parsed_font().clone();
                let font_key = format!("{}|{}|{}|{}", font.family, font.style, font.weight, font.size);
                let total_scale = label.total_scale();
                let fill = match label.style() {
                    LabelStyle::Outline => Color::new(0.0, 0.0, 0.0, 0.0),
                    _ => label.fill_color(),
                };
                let outline_width = match label.style() {
                    LabelStyle::Fill => 0.0,
                    _ => label.outline_width(),
                };
                let bg_pad = label.background_padding();
                let line_spacing = font.line_height.unwrap_or(1.2 * font.size) / label.relative_size();

                let lines: Vec<&str> = rendered.split('\n').collect();
                for (line_idx, line) in lines.iter().enumerate() {
                    let mut x_cursor = 0.0;
                    let mut bitmaps = Vec::with_capacity(line.chars().count());
                    for ch in line.chars() {
                        let bm = self
                            .glyph_cache
                            .entry((ch, font_key.clone()))
                            .or_insert_with(|| self.rasterizer.rasterize(ch, &font))
                            .clone();
                        x_cursor += bm.advance;
                        bitmaps.push((ch, bm));
                    }
                    let line_width = x_cursor;
                    let mut x_off = calculate_width_offset(line_width, label.horizontal_origin(), bg_pad);
                    let y_off = -(line_idx as f64) * line_spacing;

                    for (ch, bm) in bitmaps {
                        let img_id = format!("czm_glyph_{}_{}", ch as u32, font_key);
                        let is_ws = ch.is_whitespace();
                        let opts = BillboardOptions {
                            show: label.show() && !is_ws,
                            position: label.position(),
                            pixel_offset: Cartesian2::new(
                                label.pixel_offset().x + x_off * total_scale,
                                label.pixel_offset().y + y_off * total_scale,
                            ),
                            eye_offset: label.eye_offset(),
                            horizontal_origin: HorizontalOrigin::Left,
                            vertical_origin: label.vertical_origin(),
                            height_reference: hr,
                            scale: total_scale,
                            color: fill,
                            outline_color: label.outline_color(),
                            outline_width,
                            scale_by_distance: label.scale_by_distance(),
                            translucency_by_distance: label.translucency_by_distance(),
                            pixel_offset_scale_by_distance: label.pixel_offset_scale_by_distance(),
                            distance_display_condition: label.distance_display_condition(),
                            disable_depth_test_distance: label.disable_depth_test_distance(),
                            id: label.id().map(str::to_string),
                            image_id: Some(img_id),
                            ..Default::default()
                        };
                        let img = BillboardImage::pixels(bm.width, bm.height, bm.rgba_sdf);
                        if let Ok(bb) = self.glyphs.add(opts, Some(img)) {
                            glyph_bbs.push((is_ws, x_off, y_off, bb));
                        }
                        x_off += bm.advance;
                    }
                }

                self.per_label.push(LabelBillboards { background: bg, glyphs: glyph_bbs });
            }
        } else {
            // Synchronize ordinary dirty properties in place without rebuilding the glyph atlas
            for (handle, backing) in labels.iter().zip(self.per_label.iter()) {
                let label = handle.borrow();
                if !label.is_dirty() || !label.is_attached() {
                    continue;
                }
                let hr = if self.glyphs.is_destroyed() { HeightReference::None } else { label.height_reference() };
                let total_scale = label.total_scale();
                let fill = match label.style() {
                    LabelStyle::Outline => Color::new(0.0, 0.0, 0.0, 0.0),
                    _ => label.fill_color(),
                };
                let outline_width = match label.style() {
                    LabelStyle::Fill => 0.0,
                    _ => label.outline_width(),
                };
                if let Some(bg) = &backing.background {
                    let tr = label.background_translate.unwrap_or(Cartesian2::ZERO);
                    bg.set_show(label.show());
                    bg.set_position(label.position());
                    bg.set_pixel_offset(Cartesian2::new(label.pixel_offset().x + tr.x, label.pixel_offset().y + tr.y));
                    bg.set_eye_offset(label.eye_offset());
                    bg.set_scale(total_scale);
                    bg.set_color(label.background_color());
                    bg.set_height_reference(hr)?;
                    bg.set_scale_by_distance(label.scale_by_distance())?;
                    bg.set_translucency_by_distance(label.translucency_by_distance())?;
                    bg.set_pixel_offset_scale_by_distance(label.pixel_offset_scale_by_distance())?;
                    bg.set_distance_display_condition(label.distance_display_condition())?;
                    bg.set_disable_depth_test_distance(label.disable_depth_test_distance())?;
                    bg.set_id(label.id().map(str::to_string))?;
                }
                for (is_ws, x_off, y_off, bb) in &backing.glyphs {
                    bb.set_show(label.show() && !*is_ws);
                    bb.set_position(label.position());
                    bb.set_pixel_offset(Cartesian2::new(
                        label.pixel_offset().x + *x_off * total_scale,
                        label.pixel_offset().y + *y_off * total_scale,
                    ));
                    bb.set_eye_offset(label.eye_offset());
                    bb.set_scale(total_scale);
                    bb.set_color(fill);
                    bb.set_outline_color(label.outline_color());
                    bb.set_outline_width(outline_width);
                    bb.set_height_reference(hr)?;
                    bb.set_scale_by_distance(label.scale_by_distance())?;
                    bb.set_translucency_by_distance(label.translucency_by_distance())?;
                    bb.set_pixel_offset_scale_by_distance(label.pixel_offset_scale_by_distance())?;
                    bb.set_distance_display_condition(label.distance_display_condition())?;
                    bb.set_disable_depth_test_distance(label.disable_depth_test_distance())?;
                    bb.set_id(label.id().map(str::to_string))?;
                }
            }
        }

        self.backgrounds.update(frame)?;
        self.glyphs.update(frame)?;
        Ok(())
    }

    fn destroy(&mut self) -> SceneResult<()> {
        if !self.backgrounds.is_destroyed() {
            self.backgrounds.destroy()?;
        }
        if !self.glyphs.is_destroyed() {
            self.glyphs.destroy()?;
        }
        self.per_label.clear();
        self.glyph_cache.clear();
        Ok(())
    }
}

trait LabelCollectionBackend {
    fn set_height_sampler(&mut self, sampler: Option<HeightSampler>);
    fn set_rasterizer(&mut self, rasterizer: Rc<dyn GlyphRasterizer>);
    #[allow(clippy::too_many_arguments)]
    fn sync_and_update(
        &mut self,
        labels: &[LabelHandle],
        show: bool,
        model_matrix: Matrix4,
        debug_show_bounding_volume: bool,
        blend_option: BlendOption,
        coarse_depth_test_distance: f64,
        three_point_depth_test_distance: f64,
        frame: &mut FrameState,
    ) -> SceneResult<()>;
    fn destroy(&mut self) -> SceneResult<()>;
}

impl<G: Gl + 'static> LabelCollectionBackend for LabelCollectionGpu<G> {
    fn set_height_sampler(&mut self, sampler: Option<HeightSampler>) {
        LabelCollectionGpu::set_height_sampler(self, sampler);
    }
    fn set_rasterizer(&mut self, rasterizer: Rc<dyn GlyphRasterizer>) {
        LabelCollectionGpu::set_rasterizer(self, rasterizer);
    }
    #[allow(clippy::too_many_arguments)]
    fn sync_and_update(
        &mut self,
        labels: &[LabelHandle],
        show: bool,
        model_matrix: Matrix4,
        debug_show_bounding_volume: bool,
        blend_option: BlendOption,
        coarse_depth_test_distance: f64,
        three_point_depth_test_distance: f64,
        frame: &mut FrameState,
    ) -> SceneResult<()> {
        LabelCollectionGpu::sync_and_update(
            self,
            labels,
            show,
            model_matrix,
            debug_show_bounding_volume,
            blend_option,
            coarse_depth_test_distance,
            three_point_depth_test_distance,
            frame,
        )
    }
    fn destroy(&mut self) -> SceneResult<()> {
        LabelCollectionGpu::destroy(self)
    }
}

/// Shared handle modeling CesiumJS label mutable reference semantics.
pub type LabelHandle = Rc<RefCell<Label>>;

/// Calculates the horizontal offset for text lines matching CesiumJS `calculateWidthOffset`.
pub fn calculate_width_offset(
    line_width: f64,
    horizontal_origin: HorizontalOrigin,
    background_padding: Cartesian2,
) -> f64 {
    match horizontal_origin {
        HorizontalOrigin::Center => -line_width / 2.0,
        HorizontalOrigin::Right => -(line_width + background_padding.x),
        HorizontalOrigin::Left => background_padding.x,
    }
}

/// CesiumJS `LabelCollection`: manages text labels, SDF glyph layout, and GPU
/// rendering via paired background and SDF glyph [`BillboardCollection`]s.
pub struct LabelCollection {
    labels: Vec<LabelHandle>,
    labels_to_update: Vec<LabelHandle>,
    show: bool,
    model_matrix: Matrix4,
    debug_show_bounding_volume: bool,
    blend_option: BlendOption,
    coarse_depth_test_distance: f64,
    three_point_depth_test_distance: f64,
    enable_right_to_left_detection: bool,
    total_glyph_count: usize,
    base_volume_cached: Option<BoundingSphere>,
    base_volume_wc_cached: Option<BoundingSphere>,
    rasterizer: Rc<dyn GlyphRasterizer>,
    height_sampler: Option<HeightSampler>,
    gpu: Option<Box<dyn LabelCollectionBackend>>,
    destroyed: bool,
}

impl std::fmt::Debug for LabelCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LabelCollection")
            .field("len", &self.labels.len())
            .field("show", &self.show)
            .field("model_matrix", &self.model_matrix)
            .field("total_glyph_count", &self.total_glyph_count)
            .field("destroyed", &self.destroyed)
            .finish()
    }
}

impl Default for LabelCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl LabelCollection {
    /// Creates an empty label collection with default settings.
    pub fn new() -> Self {
        Self {
            labels: Vec::new(),
            labels_to_update: Vec::new(),
            show: true,
            model_matrix: Matrix4::IDENTITY,
            debug_show_bounding_volume: false,
            blend_option: BlendOption::OpaqueAndTranslucent,
            coarse_depth_test_distance: 0.0,
            three_point_depth_test_distance: 0.0,
            enable_right_to_left_detection: false,
            total_glyph_count: 0,
            base_volume_cached: None,
            base_volume_wc_cached: None,
            rasterizer: Rc::new(DefaultSdfGlyphRasterizer),
            height_sampler: None,
            gpu: None,
            destroyed: false,
        }
    }

    /// Creates a `LabelCollection` attached to a renderer `Context<G>`.
    pub fn new_with_context<G: Gl + 'static>(_context: Context<G>) -> Self {
        let mut coll = Self::new();
        coll.attach_context::<G>();
        coll
    }

    /// Enables GPU billboard-backed rendering for this label collection on context `G`.
    pub fn attach_context<G: Gl + 'static>(&mut self) {
        let mut gpu = LabelCollectionGpu::<G>::new(self.rasterizer.clone());
        gpu.set_height_sampler(self.height_sampler.clone());
        self.gpu = Some(Box::new(gpu));
    }

    /// Installs a custom SDF glyph rasterizer.
    pub fn set_glyph_rasterizer(&mut self, rasterizer: Rc<dyn GlyphRasterizer>) {
        self.rasterizer = rasterizer.clone();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.set_rasterizer(rasterizer);
        }
    }

    /// Installs a scene height sampler for label height clamping.
    pub fn set_height_sampler(&mut self, sampler: Option<HeightSampler>) {
        self.height_sampler = sampler.clone();
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.set_height_sampler(sampler);
        }
    }

    /// Updates CPU layout and emits GPU background + SDF glyph draw commands.
    pub fn update_with_context<G: Gl + 'static>(&mut self, frame: &mut FrameState) -> SceneResult<()> {
        if self.destroyed {
            return Err(RuntimeError::new("LabelCollection has been destroyed"));
        }
        if self.gpu.is_none() {
            self.attach_context::<G>();
        }
        ScenePrimitive::update(self, frame)
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// Computes a culling-safe bounding sphere accounting for label screen/world offsets.
    pub fn expanded_bounding_sphere(&mut self) -> BoundingSphere {
        let mut bs = self.base_volume_wc();
        let mut max_extent = 0.0_f64;
        for handle in &self.labels {
            let l = handle.borrow();
            let eye = l.eye_offset().magnitude();
            let dims = l.background_dimensions.map_or(0.0, |d| d.x.hypot(d.y) * l.total_scale());
            max_extent = max_extent.max(eye + dims);
        }
        bs.radius += max_extent;
        bs
    }

    pub fn show(&self) -> bool {
        self.show
    }

    pub fn set_show(&mut self, show: bool) {
        self.show = show;
    }

    pub fn model_matrix(&self) -> &Matrix4 {
        &self.model_matrix
    }

    pub fn set_model_matrix(&mut self, matrix: Matrix4) {
        self.model_matrix = matrix;
        self.base_volume_wc_cached = None;
    }

    pub fn debug_show_bounding_volume(&self) -> bool {
        self.debug_show_bounding_volume
    }

    pub fn set_debug_show_bounding_volume(&mut self, debug: bool) {
        self.debug_show_bounding_volume = debug;
    }

    pub fn blend_option(&self) -> BlendOption {
        self.blend_option
    }

    pub fn set_blend_option(&mut self, blend: BlendOption) {
        self.blend_option = blend;
    }

    pub fn coarse_depth_test_distance(&self) -> f64 {
        self.coarse_depth_test_distance
    }

    pub fn set_coarse_depth_test_distance(&mut self, distance: f64) {
        self.coarse_depth_test_distance = distance;
    }

    pub fn three_point_depth_test_distance(&self) -> f64 {
        self.three_point_depth_test_distance
    }

    pub fn set_three_point_depth_test_distance(&mut self, distance: f64) {
        self.three_point_depth_test_distance = distance;
    }

    pub fn enable_right_to_left_detection(&self) -> bool {
        self.enable_right_to_left_detection
    }

    pub fn set_enable_right_to_left_detection(&mut self, enable: bool) {
        for label in &self.labels {
            assert!(label.try_borrow_mut().is_ok(), "cannot change RTL mode while a label is borrowed");
        }
        self.enable_right_to_left_detection = enable;
        for label in &self.labels {
            label.borrow_mut().set_rtl_detection(enable);
        }
    }

    pub fn len(&self) -> usize {
        self.labels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    pub fn total_glyph_count(&self) -> usize {
        self.total_glyph_count
    }

    /// Adds a label with the specified options to the collection.
    pub fn add(&mut self, options: LabelOptions) -> Result<LabelHandle, &'static str> {
        let index = self.labels.len();
        let label = Label::new(options, index, self.enable_right_to_left_detection)?;
        let handle = Rc::new(RefCell::new(label));
        self.labels.push(Rc::clone(&handle));
        self.labels_to_update.push(Rc::clone(&handle));
        self.invalidate_bounds();
        Ok(handle)
    }

    /// Removes a label from the collection.
    ///
    /// Detaches the label, compacts remaining indices, and invalidates bounding volumes.
    pub fn remove(&mut self, handle: &LabelHandle) -> bool {
        let index = self.labels.iter().position(|item| Rc::ptr_eq(item, handle));
        if let Some(pos) = index {
            if self.labels[pos..].iter().any(|label| label.try_borrow_mut().is_err()) {
                return false;
            }
            let removed = self.labels.remove(pos);
            removed.borrow_mut().detach();

            // Re-index remaining labels
            for (idx, lbl) in self.labels.iter().enumerate().skip(pos) {
                lbl.borrow_mut().set_collection_index(idx);
            }

            self.labels_to_update.retain(|item| !Rc::ptr_eq(item, handle));
            self.invalidate_bounds();
            true
        } else {
            false
        }
    }

    /// Removes all labels from the collection.
    pub fn remove_all(&mut self) {
        for label in &self.labels {
            assert!(label.try_borrow_mut().is_ok(), "cannot remove labels while a handle is borrowed");
        }
        for handle in self.labels.drain(..) {
            handle.borrow_mut().detach();
        }
        self.labels_to_update.clear();
        self.total_glyph_count = 0;
        self.invalidate_bounds();
    }

    /// Checks if the collection contains the specified label.
    pub fn contains(&self, handle: &LabelHandle) -> bool {
        self.labels.iter().any(|item| Rc::ptr_eq(item, handle))
    }

    /// Gets the label handle at the specified index.
    pub fn get(&self, index: usize) -> Option<LabelHandle> {
        self.labels.get(index).cloned()
    }

    fn invalidate_bounds(&mut self) {
        self.base_volume_cached = None;
        self.base_volume_wc_cached = None;
    }

    /// Computes the bounding sphere enclosing all label positions in model space.
    pub fn base_volume(&mut self) -> BoundingSphere {
        if self.labels.is_empty() {
            let sphere = BoundingSphere::new(Cartesian3::ZERO, 0.0);
            self.base_volume_cached = Some(sphere);
            return sphere;
        }

        let points: Vec<Cartesian3> = self
            .labels
            .iter()
            .map(|handle| handle.borrow().actual_position())
            .collect();

        let sphere = BoundingSphere::from_points(&points);
        self.base_volume_cached = Some(sphere);
        sphere
    }

    /// Computes the bounding sphere transformed to world coordinates via `model_matrix`.
    pub fn base_volume_wc(&mut self) -> BoundingSphere {
        let base = self.base_volume();
        let sphere = base.transform(&self.model_matrix);
        self.base_volume_wc_cached = Some(sphere);
        sphere
    }

    /// Flushes dirty state and updates CPU glyph / background layout metrics.
    pub fn update(&mut self) {
        self.update_layout_internal(true);
    }

    fn update_layout_internal(&mut self, clear_dirty: bool) {
        if !self.show {
            return;
        }

        self.labels_to_update.clear();
        self.total_glyph_count = 0;
        for handle in &self.labels {
            let mut label = handle.borrow_mut();
            if !label.is_attached() {
                continue;
            }

            let rendered_text = label.rendered_text().to_string();
            let text_len = rendered_text.chars().count();
            self.total_glyph_count += text_len;

            if label.reposition_all_glyphs() || label.rebind_all_glyphs() {
                let lines: Vec<&str> = rendered_text.split('\n').collect();
                let num_lines = lines.len();

                let relative_size = label.relative_size();
                let mut bg_pad = label.background_padding();
                bg_pad.x /= relative_size;
                bg_pad.y /= relative_size;

                const GLYPH_WIDTH: f64 = 24.0;
                const GLYPH_HEIGHT: f64 = 48.0;
                const GLYPH_DESCENT: f64 = 12.0;
                const GLYPH_Y: f64 = 36.0;

                let mut max_line_width = 0.0f64;
                for line in &lines {
                    let char_count = line.chars().count();
                    let line_width = char_count as f64 * GLYPH_WIDTH;
                    if line_width > max_line_width {
                        max_line_width = line_width;
                    }
                }

                let line_spacing = label
                    .parsed_font()
                    .line_height
                    .unwrap_or(1.2 * label.parsed_font().size)
                    / relative_size;
                let other_lines_height = line_spacing * (num_lines.saturating_sub(1) as f64);
                let max_line_height = GLYPH_HEIGHT;

                let mut total_line_width = max_line_width;
                let mut total_line_height = max_line_height + other_lines_height;

                if label.show_background() {
                    total_line_width += bg_pad.x * 2.0;
                    total_line_height += bg_pad.y * 2.0;
                }

                let total_scale = label.total_scale();
                let horizontal_origin = label.horizontal_origin();
                let vertical_origin = label.vertical_origin();

                let width_offset = match horizontal_origin {
                    HorizontalOrigin::Center => -max_line_width / 2.0 - bg_pad.x,
                    HorizontalOrigin::Right => -(max_line_width + bg_pad.x * 2.0),
                    HorizontalOrigin::Left => 0.0,
                };

                let y_offset = match vertical_origin {
                    VerticalOrigin::Top => max_line_height - GLYPH_Y - GLYPH_DESCENT,
                    VerticalOrigin::Center => (max_line_height - GLYPH_Y) / 2.0 - GLYPH_DESCENT,
                    VerticalOrigin::Baseline => -bg_pad.y - GLYPH_DESCENT,
                    VerticalOrigin::Bottom => 0.0,
                };

                let translate = Cartesian2::new(
                    width_offset * total_scale,
                    y_offset * total_scale,
                );

                label.background_dimensions = Some(Cartesian2::new(total_line_width, total_line_height));
                label.background_translate = Some(translate);
            }

            if clear_dirty {
                label.clear_dirty();
            }
        }
    }
}

impl ScenePrimitive for LabelCollection {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> SceneResult<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> SceneResult<()> {
        if self.destroyed {
            return Err(RuntimeError::new("LabelCollection has been destroyed"));
        }
        if !self.show {
            return Ok(());
        }
        self.update_layout_internal(false);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.sync_and_update(
                &self.labels,
                self.show,
                self.model_matrix,
                self.debug_show_bounding_volume,
                self.blend_option,
                self.coarse_depth_test_distance,
                self.three_point_depth_test_distance,
                frame,
            )?;
        }
        for handle in &self.labels {
            handle.borrow_mut().clear_dirty();
        }
        Ok(())
    }

    fn post_passes_update(&mut self, _frame: &mut FrameState) -> SceneResult<()> {
        Ok(())
    }

    fn update_for_pass(&mut self, _frame: &mut FrameState, _pass: Cesium3DTilePass) -> SceneResult<()> {
        Ok(())
    }

    fn destroy(&mut self) -> SceneResult<()> {
        if !self.destroyed {
            if let Some(mut gpu) = self.gpu.take() {
                gpu.destroy()?;
            }
            self.remove_all();
            self.destroyed = true;
        }
        Ok(())
    }
}

impl Drop for LabelCollection {
    fn drop(&mut self) {
        for label in &self.labels {
            if let Ok(mut label) = label.try_borrow_mut() { label.detach(); }
        }
    }
}
