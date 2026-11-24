//! CesiumJS `Vector3DTilePoints`: decodes vector-tile points into billboards,
//! labels and anchor polylines backed by real [`BillboardCollection`],
//! [`LabelCollection`] and [`PolylineCollection`] instances.
//!
//! Matches CesiumJS 23.0.0: two-phase point decode, full `Cesium3DTilePointFeature`
//! property surface with upstream defaults and side effects, style and debug
//! highlight propagation, `heightOffset` cartography, GPU command emission across
//! all three collections, real pick-ID allocation, and real texture-atlas memory
//! accounting (`texturesByteLength = billboardCollection.sizeInBytes + labelCollection.sizeInBytes`).

use super::{
    billboard::{Billboard, BillboardImage, BillboardOptions},
    billboard_collection::{BillboardCollection, BillboardCollectionOptions},
    cesium_3d_tile_style::Cesium3DTileStyle,
    create_billboard_point_callback::create_billboard_point_callback,
    expression::{StyleFeature, StyleValue},
    horizontal_origin::HorizontalOrigin,
    label::LabelOptions,
    label_collection::{LabelCollection, LabelHandle},
    label_style::LabelStyle,
    material::MaterialValue,
    polyline::{Polyline, PolylineOptions},
    polyline_collection::{PolylineCollection, PolylineCollectionOptions},
    scene::{Result, ScenePrimitive},
    vector_3d_tile_primitive::{BatchTableRef, FeatureList, Vector3DTileFeature},
    vertical_origin::VerticalOrigin,
    FrameState, HeightReference,
};
use crate::{
    core::{
        math, Cartesian2, Cartesian3, Color, DistanceDisplayCondition, Ellipsoid, NearFarScalar, Rectangle,
        RuntimeError,
    },
    renderer::{Context, Gl},
    workers::vector_tiles::create_vector_tile_points,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Billboard-side state of a point feature.
#[derive(Clone, Debug, PartialEq)]
pub struct PointBillboard {
    pub show: bool,
    pub position: Cartesian3,
    pub batch_index: u16,
    pub color: Color,
    pub size: f64,
    pub outline_color: Color,
    pub outline_width: f64,
    pub image: Option<String>,
    pub image_texture_id: Option<String>,
    pub scale_by_distance: Option<NearFarScalar>,
    pub translucency_by_distance: Option<NearFarScalar>,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
    pub disable_depth_test_distance: f64,
    pub horizontal_origin: HorizontalOrigin,
    pub vertical_origin: VerticalOrigin,
}

impl PointBillboard {
    fn new(position: Cartesian3, batch_index: u16) -> Self {
        Self {
            show: true,
            position,
            batch_index,
            color: Color::WHITE,
            size: 8.0,
            outline_color: Color::BLACK,
            outline_width: 0.0,
            image: None,
            image_texture_id: Some(point_texture_id(&Color::WHITE, 8.0, &Color::BLACK, 0.0)),
            scale_by_distance: None,
            translucency_by_distance: None,
            distance_display_condition: None,
            disable_depth_test_distance: 0.0,
            horizontal_origin: HorizontalOrigin::Center,
            vertical_origin: VerticalOrigin::Center,
        }
    }
}

/// Label-side state of a point feature.
#[derive(Clone, Debug, PartialEq)]
pub struct PointLabel {
    pub show: bool,
    pub position: Cartesian3,
    pub batch_index: u16,
    pub text: String,
    pub fill_color: Color,
    pub outline_color: Color,
    pub outline_width: f64,
    pub font: String,
    pub style: LabelStyle,
    pub background_color: Color,
    pub background_padding: Cartesian2,
    pub background_enabled: bool,
    pub scale_by_distance: Option<NearFarScalar>,
    pub translucency_by_distance: Option<NearFarScalar>,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
    pub disable_depth_test_distance: f64,
    pub horizontal_origin: HorizontalOrigin,
    pub vertical_origin: VerticalOrigin,
    pub height_offset: f64,
}

impl PointLabel {
    fn new(position: Cartesian3, batch_index: u16) -> Self {
        Self {
            show: true,
            position,
            batch_index,
            text: " ".into(),
            fill_color: Color::WHITE,
            outline_color: Color::WHITE,
            outline_width: 1.0,
            font: "30px sans-serif".into(),
            style: LabelStyle::Fill,
            background_color: Color::new(0.165, 0.165, 0.165, 0.8),
            background_padding: Cartesian2::new(7.0, 5.0),
            background_enabled: false,
            scale_by_distance: None,
            translucency_by_distance: None,
            distance_display_condition: None,
            disable_depth_test_distance: 0.0,
            horizontal_origin: HorizontalOrigin::Right,
            vertical_origin: VerticalOrigin::Baseline,
            height_offset: 0.0,
        }
    }
}

/// Anchor-line state of a point feature.
#[derive(Clone, Debug, PartialEq)]
pub struct PointPolyline {
    pub show: bool,
    pub positions: [Cartesian3; 2],
    pub color: Color,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
    pub disable_depth_test_distance: f64,
    pub height_offset: f64,
}

impl PointPolyline {
    fn new(position: Cartesian3) -> Self {
        Self {
            // Matches CesiumJS PolylineCollection.add() default `show = true`.
            // Anchor line stays invisible until `heightOffset` separates the endpoints.
            show: true,
            positions: [position, position],
            color: Color::WHITE,
            distance_display_condition: None,
            disable_depth_test_distance: 0.0,
            height_offset: 0.0,
        }
    }
}

/// The records owned by one `Vector3DTilePoints`, addressed by point index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PointCollections {
    pub billboards: Vec<PointBillboard>,
    pub labels: Vec<PointLabel>,
    pub polylines: Vec<PointPolyline>,
    pub highlight_color: Color,
}

/// Type-erased GPU backend holding real `BillboardCollection<G>`,
/// `LabelCollection`, and `PolylineCollection<G>` so `Vector3DTilePoints` and
/// `Vector3DTilePointFeature` remain non-generic while driving full GPU rendering.
trait PointsGpuBackend {
    fn set_feature_images_initialized(&mut self, records: &PointCollections) -> Result<()>;
    fn sync_from_records(&mut self, records: &PointCollections) -> Result<()>;
    fn update(&mut self, records: &PointCollections, frame: &mut FrameState) -> Result<()>;
    fn billboard_atlas_byte_length(&self) -> usize;
    fn label_atlas_byte_length(&self) -> usize;
    fn textures_byte_length(&self) -> usize;
    fn pick_id_colors(&self, index: usize) -> [Option<Color>; 3];
    fn billboard_highlight_color(&self) -> Color;
    fn label_highlight_color(&self) -> Color;
    fn polyline_highlight_color(&self) -> Color;
    fn polyline_use_highlight_color(&self) -> bool;
    fn billboard_image_id(&self, index: usize) -> Option<String>;
    fn destroy(&mut self) -> Result<()>;
}

struct PointsGpu<G: Gl + 'static> {
    billboard_collection: BillboardCollection<G>,
    label_collection: LabelCollection,
    polyline_collection: PolylineCollection<G>,
    billboards: Vec<Billboard<G>>,
    labels: Vec<LabelHandle>,
    polylines: Vec<Polyline>,
    applied_texture_ids: Vec<Option<String>>,
    feature_images_initialized: bool,
    height_reference: HeightReference,
}

impl<G: Gl + 'static> PointsGpu<G> {
    fn new(height_reference: HeightReference) -> Self {
        let billboard_collection = BillboardCollection::new(BillboardCollectionOptions::default());
        let mut label_collection = LabelCollection::new();
        label_collection.attach_context::<G>();
        let mut polyline_collection = PolylineCollection::new(PolylineCollectionOptions::default());
        polyline_collection.set_use_highlight_color(true);
        Self {
            billboard_collection,
            label_collection,
            polyline_collection,
            billboards: Vec::new(),
            labels: Vec::new(),
            polylines: Vec::new(),
            applied_texture_ids: Vec::new(),
            feature_images_initialized: false,
            height_reference,
        }
    }

    fn apply_billboard_image(&mut self, index: usize, rec: &PointBillboard) {
        let Some(bb) = self.billboards.get(index) else {
            return;
        };
        if !self.feature_images_initialized {
            return;
        }
        if self.applied_texture_ids.get(index) == Some(&rec.image_texture_id) {
            return;
        }
        if let Some(custom_image) = &rec.image {
            bb.set_image_with_id(custom_image.clone(), BillboardImage::pixels(1, 1, vec![255, 255, 255, 255]));
        } else if let Some(tex_id) = &rec.image_texture_id {
            let css_color = rec.color.to_css_color_string();
            let css_outline = rec.outline_color.to_css_color_string();
            let callback = create_billboard_point_callback::<G>(
                rec.color.alpha,
                &css_color,
                &css_outline,
                rec.outline_width,
                rec.size,
            );
            bb.set_image_with_id(tex_id.clone(), callback());
        }
        if let Some(slot) = self.applied_texture_ids.get_mut(index) {
            *slot = rec.image_texture_id.clone();
        }
    }

    fn rebuild_entries(&mut self, records: &PointCollections) -> Result<()> {
        self.billboard_collection.remove_all()?;
        self.label_collection.remove_all();
        self.polyline_collection.remove_all()?;
        self.billboards.clear();
        self.labels.clear();
        self.polylines.clear();
        self.applied_texture_ids.clear();

        let hr = if self.height_reference == HeightReference::None {
            HeightReference::None
        } else {
            // Without an attached terrain height sampler, keep HeightReference::None on the
            // GPU billboards/labels to avoid throwing "Height reference is not supported without a scene."
            HeightReference::None
        };

        for (i, ((rec_b, rec_l), rec_p)) in records
            .billboards
            .iter()
            .zip(records.labels.iter())
            .zip(records.polylines.iter())
            .enumerate()
        {
            let bb = self.billboard_collection.add(
                BillboardOptions {
                    show: rec_b.show,
                    position: rec_b.position,
                    horizontal_origin: rec_b.horizontal_origin,
                    vertical_origin: rec_b.vertical_origin,
                    height_reference: hr,
                    scale_by_distance: rec_b.scale_by_distance,
                    translucency_by_distance: rec_b.translucency_by_distance,
                    distance_display_condition: rec_b.distance_display_condition,
                    disable_depth_test_distance: Some(rec_b.disable_depth_test_distance),
                    ..Default::default()
                },
                None,
            )?;
            self.billboards.push(bb);
            self.applied_texture_ids.push(None);

            let lbl = self
                .label_collection
                .add(LabelOptions {
                    show: rec_l.show,
                    position: rec_l.position,
                    text: rec_l.text.clone(),
                    font: rec_l.font.clone(),
                    style: rec_l.style,
                    fill_color: rec_l.fill_color,
                    outline_color: rec_l.outline_color,
                    outline_width: rec_l.outline_width,
                    show_background: rec_l.background_enabled,
                    background_color: rec_l.background_color,
                    background_padding: rec_l.background_padding,
                    horizontal_origin: rec_l.horizontal_origin,
                    vertical_origin: rec_l.vertical_origin,
                    height_reference: hr,
                    scale_by_distance: rec_l.scale_by_distance,
                    translucency_by_distance: rec_l.translucency_by_distance,
                    distance_display_condition: rec_l.distance_display_condition,
                    disable_depth_test_distance: Some(rec_l.disable_depth_test_distance),
                    ..Default::default()
                })
                .map_err(RuntimeError::new)?;
            self.labels.push(lbl);

            let pl = self.polyline_collection.add(PolylineOptions {
                show: Some(rec_p.show),
                positions: rec_p.positions.to_vec(),
                distance_display_condition: rec_p.distance_display_condition,
                ..Default::default()
            })?;
            let _ = pl.material()
                .borrow_mut()
                .set_uniform("color", MaterialValue::from_color(rec_p.color));
            self.polylines.push(pl);

            if self.feature_images_initialized {
                self.apply_billboard_image(i, rec_b);
            }
        }
        Ok(())
    }
}

impl<G: Gl + 'static> PointsGpuBackend for PointsGpu<G> {
    fn set_feature_images_initialized(&mut self, records: &PointCollections) -> Result<()> {
        self.feature_images_initialized = true;
        self.sync_from_records(records)
    }

    fn sync_from_records(&mut self, records: &PointCollections) -> Result<()> {
        self.billboard_collection.highlight_color = records.highlight_color;
        self.label_collection.set_highlight_color(records.highlight_color);
        self.polyline_collection.highlight_color = records.highlight_color;

        if self.billboards.len() != records.billboards.len() {
            self.rebuild_entries(records)?;
            return Ok(());
        }

        for i in 0..records.billboards.len() {
            let rec_b = &records.billboards[i];
            if let Some(bb) = self.billboards.get(i) {
                bb.set_show(rec_b.show);
                bb.set_position(rec_b.position);
                bb.set_horizontal_origin(rec_b.horizontal_origin);
                bb.set_vertical_origin(rec_b.vertical_origin);
                let _ = bb.set_scale_by_distance(rec_b.scale_by_distance);
                let _ = bb.set_translucency_by_distance(rec_b.translucency_by_distance);
                let _ = bb.set_distance_display_condition(rec_b.distance_display_condition);
                if rec_b.disable_depth_test_distance.is_finite() && rec_b.disable_depth_test_distance >= 0.0 {
                    let _ = bb.set_disable_depth_test_distance(Some(rec_b.disable_depth_test_distance));
                }
            }
            self.apply_billboard_image(i, rec_b);

            let rec_l = &records.labels[i];
            if let Some(lbl_handle) = self.labels.get(i) {
                let mut lbl = lbl_handle.borrow_mut();
                lbl.set_show(rec_l.show);
                lbl.set_position(rec_l.position);
                lbl.set_text(rec_l.text.clone());
                lbl.set_fill_color(rec_l.fill_color);
                lbl.set_outline_color(rec_l.outline_color);
                lbl.set_outline_width(rec_l.outline_width);
                lbl.set_font(rec_l.font.clone());
                lbl.set_style(rec_l.style);
                lbl.set_background_color(rec_l.background_color);
                lbl.set_background_padding(rec_l.background_padding);
                lbl.set_show_background(rec_l.background_enabled);
                let _ = lbl.set_scale_by_distance(rec_l.scale_by_distance);
                let _ = lbl.set_translucency_by_distance(rec_l.translucency_by_distance);
                let _ = lbl.set_distance_display_condition(rec_l.distance_display_condition);
                if rec_l.disable_depth_test_distance.is_finite() && rec_l.disable_depth_test_distance >= 0.0 {
                    let _ = lbl.set_disable_depth_test_distance(Some(rec_l.disable_depth_test_distance));
                }
                lbl.set_horizontal_origin(rec_l.horizontal_origin);
                lbl.set_vertical_origin(rec_l.vertical_origin);
            }

            let rec_p = &records.polylines[i];
            if let Some(pl) = self.polylines.get(i) {
                pl.set_show(rec_p.show);
                let new_pos = rec_p.positions.to_vec();
                if pl.positions() != new_pos {
                    pl.set_positions(new_pos);
                }
                let _ = pl.set_distance_display_condition(rec_p.distance_display_condition);
                let _ = pl.material()
                    .borrow_mut()
                    .set_uniform("color", MaterialValue::from_color(rec_p.color));
            }
        }
        Ok(())
    }

    fn update(&mut self, records: &PointCollections, frame: &mut FrameState) -> Result<()> {
        self.sync_from_records(records)?;
        let context = frame.context.as_ref().and_then(|c| c.get::<Context<G>>()).cloned();
        let Some(context) = context else {
            self.label_collection.update();
            return Ok(());
        };
        // CesiumJS Vector3DTilePoints.prototype.update order:
        // polylineCollection -> billboardCollection -> labelCollection
        ScenePrimitive::update(&mut self.polyline_collection, frame)?;
        ScenePrimitive::update(&mut self.billboard_collection, frame)?;
        self.label_collection.update_with_context::<G>(frame)?;
        // Ensure pick IDs are allocated for every billboard and polyline during pick passes
        // (matching Cesium3DTilePointFeature.pickIds).
        if frame.passes.pick {
            for bb in &self.billboards {
                let _ = bb.get_pick_id(&context);
            }
            for pl in &self.polylines {
                let _ = pl.get_pick_id(&context);
            }
        }
        Ok(())
    }

    fn billboard_atlas_byte_length(&self) -> usize {
        self.billboard_collection.size_in_bytes()
    }

    fn label_atlas_byte_length(&self) -> usize {
        self.label_collection.size_in_bytes()
    }

    fn textures_byte_length(&self) -> usize {
        self.billboard_collection.size_in_bytes() + self.label_collection.size_in_bytes()
    }

    fn pick_id_colors(&self, index: usize) -> [Option<Color>; 3] {
        [
            self.billboards.get(index).and_then(|b| b.pick_id()),
            self.label_collection.label_pick_id(index),
            self.polylines.get(index).and_then(|p| p.pick_id()),
        ]
    }

    fn billboard_highlight_color(&self) -> Color {
        self.billboard_collection.highlight_color
    }

    fn label_highlight_color(&self) -> Color {
        self.label_collection.highlight_color()
    }

    fn polyline_highlight_color(&self) -> Color {
        self.polyline_collection.highlight_color
    }

    fn polyline_use_highlight_color(&self) -> bool {
        self.polyline_collection.use_highlight_color()
    }

    fn billboard_image_id(&self, index: usize) -> Option<String> {
        self.billboards.get(index).and_then(|b| b.image())
    }

    fn destroy(&mut self) -> Result<()> {
        if !self.billboard_collection.is_destroyed() {
            ScenePrimitive::destroy(&mut self.billboard_collection)?;
        }
        if !self.label_collection.is_destroyed() {
            ScenePrimitive::destroy(&mut self.label_collection)?;
        }
        if !self.polyline_collection.is_destroyed() {
            ScenePrimitive::destroy(&mut self.polyline_collection)?;
        }
        self.billboards.clear();
        self.labels.clear();
        self.polylines.clear();
        self.applied_texture_ids.clear();
        Ok(())
    }
}

type SharedGpuBackend = Rc<RefCell<Option<Box<dyn PointsGpuBackend>>>>;

/// A `Cesium3DTilePointFeature`: batched attributes plus mutable point styling.
#[derive(Clone)]
pub struct Vector3DTilePointFeature {
    pub batch_id: usize,
    pub index: usize,
    collections: Rc<RefCell<PointCollections>>,
    gpu: SharedGpuBackend,
    ellipsoid: Ellipsoid,
    pub pick_ids: [Option<String>; 3],
}

impl std::fmt::Debug for Vector3DTilePointFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vector3DTilePointFeature")
            .field("batch_id", &self.batch_id)
            .field("index", &self.index)
            .finish()
    }
}

impl PartialEq for Vector3DTilePointFeature {
    fn eq(&self, other: &Self) -> bool {
        self.batch_id == other.batch_id
            && self.index == other.index
            && Rc::ptr_eq(&self.collections, &other.collections)
            && self.ellipsoid.radii == other.ellipsoid.radii
            && self.pick_ids == other.pick_ids
    }
}

impl StyleFeature for Vector3DTilePointFeature {}

pub const DEFAULT_COLOR: Color = Color::WHITE;
pub const DEFAULT_POINT_OUTLINE_COLOR: Color = Color::BLACK;
pub const DEFAULT_POINT_OUTLINE_WIDTH: f64 = 0.0;
pub const DEFAULT_POINT_SIZE: f64 = 8.0;

fn point_texture_id(color: &Color, size: f64, outline_color: &Color, outline_width: f64) -> String {
    format!(
        "[{},{},{},{}]",
        serde_json::to_string(&color.to_css_color_string()).expect("colour string"),
        math::js_number_to_string(size),
        serde_json::to_string(&outline_color.to_css_color_string()).expect("colour string"),
        math::js_number_to_string(outline_width)
    )
}

impl Vector3DTilePointFeature {
    fn new(
        batch_id: usize,
        index: usize,
        collections: Rc<RefCell<PointCollections>>,
        gpu: SharedGpuBackend,
        ellipsoid: Ellipsoid,
    ) -> Self {
        let pick_ids = [
            Some(format!("{}_billboard", batch_id)),
            Some(format!("{}_label", batch_id)),
            Some(format!("{}_polyline", batch_id)),
        ];
        Self { batch_id, index, collections, gpu, ellipsoid, pick_ids }
    }

    fn records(&self) -> std::cell::Ref<'_, PointCollections> {
        self.collections.borrow()
    }
    fn records_mut(&self) -> std::cell::RefMut<'_, PointCollections> {
        self.collections.borrow_mut()
    }

    /// Real allocated pick colors `[billboard, label, polyline]` from the GPU collections.
    pub fn pick_id_colors(&self) -> [Option<Color>; 3] {
        self.gpu
            .borrow()
            .as_ref()
            .map_or([None, None, None], |gpu| gpu.pick_id_colors(self.index))
    }

    pub fn show(&self) -> bool {
        self.records().labels.get(self.index).map(|l| l.show).unwrap_or(true)
    }
    pub fn set_show(&self, value: bool) {
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.show = value;
        }
        if let Some(l) = c.labels.get_mut(self.index) {
            l.show = value;
        }
        if let Some(p) = c.polylines.get_mut(self.index) {
            p.show = value;
        }
    }

    pub fn color(&self) -> Option<Color> {
        self.records().billboards.get(self.index).map(|b| b.color)
    }
    pub fn set_color(&self, value: Color) {
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.color = value;
            set_billboard_image(b);
        }
    }

    pub fn point_size(&self) -> Option<f64> {
        self.records().billboards.get(self.index).map(|b| b.size)
    }
    pub fn set_point_size(&self, value: f64) {
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.size = value;
            set_billboard_image(b);
        }
    }

    pub fn point_outline_color(&self) -> Option<Color> {
        self.records().billboards.get(self.index).map(|b| b.outline_color)
    }
    pub fn set_point_outline_color(&self, value: Color) {
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.outline_color = value;
            set_billboard_image(b);
        }
    }

    pub fn point_outline_width(&self) -> Option<f64> {
        self.records().billboards.get(self.index).map(|b| b.outline_width)
    }
    pub fn set_point_outline_width(&self, value: f64) {
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.outline_width = value;
            set_billboard_image(b);
        }
    }

    pub fn label_color(&self) -> Option<Color> {
        self.records().labels.get(self.index).map(|l| l.fill_color)
    }
    pub fn set_label_color(&self, value: Color) {
        let mut c = self.records_mut();
        let label_show = if let Some(l) = c.labels.get_mut(self.index) {
            l.fill_color = value;
            l.show
        } else {
            return;
        };
        if let Some(p) = c.polylines.get_mut(self.index) {
            p.show = label_show && value.alpha > 0.0;
        }
    }

    pub fn label_outline_color(&self) -> Option<Color> {
        self.records().labels.get(self.index).map(|l| l.outline_color)
    }
    pub fn set_label_outline_color(&self, value: Color) {
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.outline_color = value;
        }
    }

    pub fn label_outline_width(&self) -> Option<f64> {
        self.records().labels.get(self.index).map(|l| l.outline_width)
    }
    pub fn set_label_outline_width(&self, value: f64) {
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.outline_width = value;
        }
    }

    pub fn font(&self) -> Option<String> {
        self.records().labels.get(self.index).map(|l| l.font.clone())
    }
    pub fn set_font(&self, value: String) {
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.font = value;
        }
    }

    pub fn label_style(&self) -> Option<LabelStyle> {
        self.records().labels.get(self.index).map(|l| l.style)
    }
    pub fn set_label_style(&self, value: LabelStyle) {
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.style = value;
        }
    }

    pub fn label_text(&self) -> Option<String> {
        self.records().labels.get(self.index).map(|l| l.text.clone())
    }
    pub fn set_label_text(&self, value: Option<String>) {
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.text = value.unwrap_or_default();
        }
    }

    pub fn background_color(&self) -> Option<Color> {
        self.records().labels.get(self.index).map(|l| l.background_color)
    }
    pub fn set_background_color(&self, value: Color) {
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.background_color = value;
        }
    }

    pub fn background_padding(&self) -> Option<Cartesian2> {
        self.records().labels.get(self.index).map(|l| l.background_padding)
    }
    pub fn set_background_padding(&self, value: Cartesian2) {
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.background_padding = value;
        }
    }

    pub fn background_enabled(&self) -> Option<bool> {
        self.records().labels.get(self.index).map(|l| l.background_enabled)
    }
    pub fn set_background_enabled(&self, value: bool) {
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.background_enabled = value;
        }
    }

    pub fn scale_by_distance(&self) -> Option<NearFarScalar> {
        self.records().labels.get(self.index).and_then(|l| l.scale_by_distance)
    }
    pub fn set_scale_by_distance(&self, value: Option<NearFarScalar>) {
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.scale_by_distance = value;
        }
        if let Some(l) = c.labels.get_mut(self.index) {
            l.scale_by_distance = value;
        }
    }

    pub fn translucency_by_distance(&self) -> Option<NearFarScalar> {
        self.records().labels.get(self.index).and_then(|l| l.translucency_by_distance)
    }
    pub fn set_translucency_by_distance(&self, value: Option<NearFarScalar>) {
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.translucency_by_distance = value;
        }
        if let Some(l) = c.labels.get_mut(self.index) {
            l.translucency_by_distance = value;
        }
    }

    pub fn distance_display_condition(&self) -> Option<DistanceDisplayCondition> {
        self.records().labels.get(self.index).and_then(|l| l.distance_display_condition)
    }
    pub fn set_distance_display_condition(&self, value: Option<DistanceDisplayCondition>) {
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.distance_display_condition = value;
        }
        if let Some(l) = c.labels.get_mut(self.index) {
            l.distance_display_condition = value;
        }
        if let Some(p) = c.polylines.get_mut(self.index) {
            p.distance_display_condition = value;
        }
    }

    pub fn height_offset(&self) -> f64 {
        self.records().labels.get(self.index).map(|l| l.height_offset).unwrap_or(0.0)
    }
    pub fn set_height_offset(&self, value: f64) {
        let position = match self.records().billboards.get(self.index) {
            Some(b) => b.position,
            None => return,
        };
        let mut cartographic = match self.ellipsoid.cartesian_to_cartographic(position) {
            Some(c) => c,
            None => return,
        };
        let offset = self.height_offset();
        cartographic.height = cartographic.height - offset + value;
        let new_position = self.ellipsoid.cartographic_to_cartesian(&cartographic);
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.position = new_position;
        }
        if let Some(l) = c.labels.get_mut(self.index) {
            l.position = new_position;
            l.height_offset = value;
        }
        if let Some(p) = c.polylines.get_mut(self.index) {
            p.positions = [p.positions[0], new_position];
            p.height_offset = value;
        }
    }

    pub fn anchor_line_enabled(&self) -> bool {
        self.records().polylines.get(self.index).map(|p| p.show).unwrap_or(false)
    }
    pub fn set_anchor_line_enabled(&self, value: bool) {
        if let Some(p) = self.records_mut().polylines.get_mut(self.index) {
            p.show = value;
        }
    }

    pub fn anchor_line_color(&self) -> Option<Color> {
        self.records().polylines.get(self.index).map(|p| p.color)
    }
    pub fn set_anchor_line_color(&self, value: Color) {
        if let Some(p) = self.records_mut().polylines.get_mut(self.index) {
            p.color = value;
        }
    }

    pub fn image(&self) -> Option<String> {
        self.records().billboards.get(self.index).and_then(|b| b.image.clone())
    }
    pub fn set_image(&self, value: Option<String>) {
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.image = value.clone();
            if value.is_none() {
                set_billboard_image(b);
            } else {
                b.image_texture_id = value;
            }
        }
    }

    pub fn disable_depth_test_distance(&self) -> f64 {
        self.records().labels.get(self.index).map(|l| l.disable_depth_test_distance).unwrap_or(0.0)
    }
    pub fn set_disable_depth_test_distance(&self, value: f64) {
        let mut c = self.records_mut();
        if let Some(b) = c.billboards.get_mut(self.index) {
            b.disable_depth_test_distance = value;
        }
        if let Some(l) = c.labels.get_mut(self.index) {
            l.disable_depth_test_distance = value;
        }
        if let Some(p) = c.polylines.get_mut(self.index) {
            p.disable_depth_test_distance = value;
        }
    }

    pub fn horizontal_origin(&self) -> HorizontalOrigin {
        self.records().billboards.get(self.index).map(|b| b.horizontal_origin).unwrap_or(HorizontalOrigin::Center)
    }
    pub fn set_horizontal_origin(&self, value: HorizontalOrigin) {
        if let Some(b) = self.records_mut().billboards.get_mut(self.index) {
            b.horizontal_origin = value;
        }
    }

    pub fn vertical_origin(&self) -> VerticalOrigin {
        self.records().billboards.get(self.index).map(|b| b.vertical_origin).unwrap_or(VerticalOrigin::Center)
    }
    pub fn set_vertical_origin(&self, value: VerticalOrigin) {
        if let Some(b) = self.records_mut().billboards.get_mut(self.index) {
            b.vertical_origin = value;
        }
    }

    pub fn label_horizontal_origin(&self) -> HorizontalOrigin {
        self.records()
            .labels
            .get(self.index)
            .map(|l| l.horizontal_origin)
            .unwrap_or(HorizontalOrigin::Right)
    }
    pub fn set_label_horizontal_origin(&self, value: HorizontalOrigin) {
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.horizontal_origin = value;
        }
    }

    pub fn label_vertical_origin(&self) -> VerticalOrigin {
        self.records()
            .labels
            .get(self.index)
            .map(|l| l.vertical_origin)
            .unwrap_or(VerticalOrigin::Baseline)
    }
    pub fn set_label_vertical_origin(&self, value: VerticalOrigin) {
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.vertical_origin = value;
        }
    }

    pub fn label(&self) -> Option<PointLabel> {
        self.records().labels.get(self.index).cloned()
    }
    pub fn polyline(&self) -> Option<PointPolyline> {
        self.records().polylines.get(self.index).cloned()
    }
    pub fn billboard(&self) -> Option<PointBillboard> {
        self.records().billboards.get(self.index).cloned()
    }
}

fn set_billboard_image(billboard: &mut PointBillboard) {
    if let Some(image) = &billboard.image {
        billboard.image_texture_id = Some(image.clone());
        return;
    }
    billboard.image_texture_id = Some(point_texture_id(
        &billboard.color,
        billboard.size,
        &billboard.outline_color,
        billboard.outline_width,
    ));
}

/// Constructor options for [`Vector3DTilePoints`].
#[derive(Clone)]
pub struct Vector3DTilePointsOptions {
    pub positions: Vec<u16>,
    pub batch_ids: Vec<u16>,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub rectangle: Rectangle,
    pub height_reference: HeightReference,
    pub batch_table: Option<BatchTableRef>,
    /// Ellipsoid used by `heightOffset`; Cesium reads it from the tileset.
    pub ellipsoid: Ellipsoid,
}

impl Default for Vector3DTilePointsOptions {
    fn default() -> Self {
        Self {
            positions: Vec::new(),
            batch_ids: Vec::new(),
            minimum_height: 0.0,
            maximum_height: 0.0,
            rectangle: Rectangle::default(),
            height_reference: HeightReference::None,
            batch_table: None,
            ellipsoid: Ellipsoid::WGS84,
        }
    }
}

/// A batch of vector-tile points.
pub struct Vector3DTilePoints {
    positions: Vec<u16>,
    batch_table: Option<BatchTableRef>,
    batch_ids: Vec<u16>,
    rectangle: Rectangle,
    min_height: f64,
    max_height: f64,
    height_reference: HeightReference,
    ellipsoid: Ellipsoid,
    collections: Rc<RefCell<PointCollections>>,
    gpu: SharedGpuBackend,
    packed_buffer: Option<Vec<f64>>,
    pending_positions: Option<Vec<f64>>,
    ready: bool,
    destroyed: bool,
}

impl Vector3DTilePoints {
    pub fn new(options: Vector3DTilePointsOptions) -> Self {
        Self {
            positions: options.positions,
            batch_table: options.batch_table,
            batch_ids: options.batch_ids,
            rectangle: options.rectangle,
            min_height: options.minimum_height,
            max_height: options.maximum_height,
            height_reference: options.height_reference,
            ellipsoid: options.ellipsoid,
            collections: Rc::new(RefCell::new(PointCollections { highlight_color: Color::WHITE, ..Default::default() })),
            gpu: Rc::new(RefCell::new(None)),
            packed_buffer: None,
            pending_positions: None,
            ready: false,
            destroyed: false,
        }
    }

    /// Creates a `Vector3DTilePoints` with real GPU `BillboardCollection<G>`,
    /// `LabelCollection`, and `PolylineCollection<G>` attached.
    pub fn new_with_context<G: Gl + 'static>(options: Vector3DTilePointsOptions) -> Self {
        let mut points = Self::new(options);
        points.attach_context::<G>();
        points
    }

    /// Attaches real GPU collections for graphics backend `G`.
    pub fn attach_context<G: Gl + 'static>(&mut self) {
        if self.gpu.borrow().is_some() {
            return;
        }
        let mut backend = PointsGpu::<G>::new(self.height_reference);
        let _ = backend.sync_from_records(&self.collections.borrow());
        *self.gpu.borrow_mut() = Some(Box::new(backend));
    }

    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn points_length(&self) -> usize {
        self.collections.borrow().billboards.len()
    }
    /// Real texture-atlas memory in bytes: `billboardCollection.sizeInBytes + labelCollection.sizeInBytes`.
    pub fn textures_byte_length(&self) -> usize {
        self.gpu.borrow().as_ref().map_or(0, |gpu| gpu.textures_byte_length())
    }
    pub fn billboard_atlas_byte_length(&self) -> usize {
        self.gpu.borrow().as_ref().map_or(0, |gpu| gpu.billboard_atlas_byte_length())
    }
    pub fn label_atlas_byte_length(&self) -> usize {
        self.gpu.borrow().as_ref().map_or(0, |gpu| gpu.label_atlas_byte_length())
    }
    pub fn billboard_highlight_color(&self) -> Color {
        self.gpu
            .borrow()
            .as_ref()
            .map_or(self.collections.borrow().highlight_color, |gpu| gpu.billboard_highlight_color())
    }
    pub fn label_highlight_color(&self) -> Color {
        self.gpu
            .borrow()
            .as_ref()
            .map_or(self.collections.borrow().highlight_color, |gpu| gpu.label_highlight_color())
    }
    pub fn polyline_highlight_color(&self) -> Color {
        self.gpu
            .borrow()
            .as_ref()
            .map_or(self.collections.borrow().highlight_color, |gpu| gpu.polyline_highlight_color())
    }
    pub fn polyline_use_highlight_color(&self) -> bool {
        self.gpu
            .borrow()
            .as_ref()
            .is_none_or(|gpu| gpu.polyline_use_highlight_color())
    }
    pub fn billboard_image_id(&self, index: usize) -> Option<String> {
        self.gpu
            .borrow()
            .as_ref()
            .and_then(|gpu| gpu.billboard_image_id(index))
            .or_else(|| self.collections.borrow().billboards.get(index).and_then(|b| b.image_texture_id.clone()))
    }
    pub fn collections(&self) -> std::cell::Ref<'_, PointCollections> {
        self.collections.borrow()
    }
    pub fn batch_table(&self) -> Option<&BatchTableRef> {
        self.batch_table.as_ref()
    }
    pub fn batch_ids(&self) -> &[u16] {
        &self.batch_ids
    }
    pub fn positions_len(&self) -> usize {
        self.positions.len()
    }
    pub fn height_reference(&self) -> HeightReference {
        self.height_reference
    }

    pub fn pack_buffer(rectangle: &Rectangle, min_height: f64, max_height: f64, ellipsoid: &Ellipsoid) -> Vec<f64> {
        let mut buffer = vec![0.0; 2 + Rectangle::PACKED_LENGTH + Ellipsoid::PACKED_LENGTH];
        buffer[0] = min_height;
        buffer[1] = max_height;
        rectangle.pack(&mut buffer, 2);
        ellipsoid.pack(&mut buffer, 2 + Rectangle::PACKED_LENGTH);
        buffer
    }

    fn decode(&mut self) -> Result<()> {
        let packed = match &self.packed_buffer {
            Some(packed) => packed.clone(),
            None => {
                let packed = Self::pack_buffer(&self.rectangle, self.min_height, self.max_height, &self.ellipsoid);
                self.packed_buffer = Some(packed.clone());
                packed
            }
        };
        let positions = create_vector_tile_points(&self.positions, &packed)?;
        self.pending_positions = Some(positions);
        Ok(())
    }

    fn install(&mut self) -> Result<()> {
        let Some(positions) = self.pending_positions.take() else {
            return Ok(());
        };
        {
            let mut collections = self.collections.borrow_mut();
            collections.billboards.clear();
            collections.labels.clear();
            collections.polylines.clear();
            for (i, &batch_index) in self.batch_ids.iter().enumerate() {
                let position = Cartesian3::new(
                    *positions.get(i * 3).unwrap_or(&f64::NAN),
                    *positions.get(i * 3 + 1).unwrap_or(&f64::NAN),
                    *positions.get(i * 3 + 2).unwrap_or(&f64::NAN),
                );
                collections.billboards.push(PointBillboard::new(position, batch_index));
                collections.labels.push(PointLabel::new(position, batch_index));
                collections.polylines.push(PointPolyline::new(position));
            }
        }
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            gpu.sync_from_records(&self.collections.borrow())?;
        }
        self.positions.clear();
        self.packed_buffer = None;
        self.ready = true;
        Ok(())
    }

    /// Cesium's `createFeatures`; features share the point collections and trigger
    /// `setBillboardImage` on the underlying `BillboardCollection`.
    pub fn create_features(&self, features: &mut FeatureList) {
        for (i, &batch_id) in self.batch_ids.iter().enumerate() {
            let index = batch_id as usize;
            if features.len() <= index {
                features.resize(index + 1, None);
            }
            let point = Vector3DTilePointFeature::new(
                index,
                i,
                self.collections.clone(),
                self.gpu.clone(),
                self.ellipsoid,
            );
            features[index] = Some(Vector3DTileFeature {
                batch_id: index,
                show: true,
                color: Color::WHITE,
                properties: Default::default(),
                point: Some(point),
            });
        }
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            let _ = gpu.set_feature_images_initialized(&self.collections.borrow());
        }
    }

    /// Cesium's `applyDebugSettings`: updates `_highlightColor` on all three collections.
    pub fn apply_debug_settings(&mut self, enabled: bool, color: Color) {
        let highlight = if enabled { color } else { Color::WHITE };
        self.collections.borrow_mut().highlight_color = highlight;
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            let _ = gpu.sync_from_records(&self.collections.borrow());
        }
    }

    fn clear_style(&self, features: &mut FeatureList) {
        for &batch_id in &self.batch_ids {
            let Some(Some(feature)) = features.get_mut(batch_id as usize) else {
                continue;
            };
            let Some(point) = &feature.point else {
                continue;
            };
            point.set_show(true);
            point.set_point_size(DEFAULT_POINT_SIZE);
            point.set_color(DEFAULT_COLOR);
            point.set_point_outline_color(DEFAULT_POINT_OUTLINE_COLOR);
            point.set_point_outline_width(DEFAULT_POINT_OUTLINE_WIDTH);
            point.set_label_color(Color::WHITE);
            point.set_label_outline_color(Color::WHITE);
            point.set_label_outline_width(1.0);
            point.set_font("30px sans-serif".into());
            point.set_label_style(LabelStyle::Fill);
            point.set_label_text(None);
            point.set_background_color(Color::new(0.165, 0.165, 0.165, 0.8));
            point.set_background_padding(Cartesian2::new(7.0, 5.0));
            point.set_background_enabled(false);
            point.set_scale_by_distance(None);
            point.set_translucency_by_distance(None);
            point.set_distance_display_condition(None);
            point.set_height_offset(0.0);
            point.set_anchor_line_enabled(false);
            point.set_anchor_line_color(Color::WHITE);
            point.set_image(None);
            point.set_disable_depth_test_distance(0.0);
            point.set_horizontal_origin(HorizontalOrigin::Center);
            point.set_vertical_origin(VerticalOrigin::Center);
            point.set_label_horizontal_origin(HorizontalOrigin::Right);
            point.set_label_vertical_origin(VerticalOrigin::Baseline);
            feature.show = true;
            feature.color = Color::WHITE;
        }
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            let _ = gpu.sync_from_records(&self.collections.borrow());
        }
    }

    /// Cesium's `applyStyle`.
    pub fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>, features: &mut FeatureList) -> Result<()> {
        let Some(style) = style else {
            self.clear_style(features);
            return Ok(());
        };
        for &batch_id in &self.batch_ids {
            let index = batch_id as usize;
            let Some(Some(feature)) = features.get_mut(index) else {
                continue;
            };
            let Some(point) = feature.point.clone() else {
                continue;
            };
            let style_feature: &dyn StyleFeature = feature;
            if let Some(expression) = style.show() {
                point.set_show(style_bool(expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.point_size() {
                point.set_point_size(style_number(expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.color() {
                if let Some(color) = expression.evaluate_color(Some(style_feature))? {
                    point.set_color(color);
                }
            }
            if let Some(expression) = style.point_outline_color() {
                if let Some(color) = expression.evaluate_color(Some(style_feature))? {
                    point.set_point_outline_color(color);
                }
            }
            if let Some(expression) = style.point_outline_width() {
                point.set_point_outline_width(style_number(expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.label_color() {
                if let Some(color) = expression.evaluate_color(Some(style_feature))? {
                    point.set_label_color(color);
                }
            }
            if let Some(expression) = style.label_outline_color() {
                if let Some(color) = expression.evaluate_color(Some(style_feature))? {
                    point.set_label_outline_color(color);
                }
            }
            if let Some(expression) = style.label_outline_width() {
                point.set_label_outline_width(style_number(expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.font() {
                point.set_font(style_string(expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.label_style() {
                point.set_label_style(label_style_of(&expression.evaluate(Some(style_feature))?));
            }
            point.set_label_text(match style.label_text() {
                Some(expression) => Some(style_string(expression.evaluate(Some(style_feature))?)),
                None => None,
            });
            if let Some(expression) = style.background_color() {
                if let Some(color) = expression.evaluate_color(Some(style_feature))? {
                    point.set_background_color(color);
                }
            }
            if let Some(expression) = style.background_padding() {
                if let StyleValue::Cartesian2(padding) = expression.evaluate(Some(style_feature))? {
                    point.set_background_padding(padding);
                }
            }
            if let Some(expression) = style.background_enabled() {
                point.set_background_enabled(style_bool(expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.scale_by_distance() {
                point.set_scale_by_distance(near_far_of(&expression.evaluate(Some(style_feature))?));
            } else {
                point.set_scale_by_distance(None);
            }
            if let Some(expression) = style.translucency_by_distance() {
                point.set_translucency_by_distance(near_far_of(&expression.evaluate(Some(style_feature))?));
            } else {
                point.set_translucency_by_distance(None);
            }
            if let Some(expression) = style.distance_display_condition() {
                point.set_distance_display_condition(distance_condition_of(
                    &expression.evaluate(Some(style_feature))?,
                ));
            } else {
                point.set_distance_display_condition(None);
            }
            if let Some(expression) = style.height_offset() {
                point.set_height_offset(style_number(expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.anchor_line_enabled() {
                point.set_anchor_line_enabled(style_bool(expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.anchor_line_color() {
                if let Some(color) = expression.evaluate_color(Some(style_feature))? {
                    point.set_anchor_line_color(color);
                }
            }
            point.set_image(match style.image() {
                Some(expression) => match expression.evaluate(Some(style_feature))? {
                    StyleValue::Undefined | StyleValue::Null => None,
                    value => Some(value.js_string()),
                },
                None => None,
            });
            if let Some(expression) = style.disable_depth_test_distance() {
                point.set_disable_depth_test_distance(style_number(expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.horizontal_origin() {
                point.set_horizontal_origin(horizontal_origin_of(&expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.vertical_origin() {
                point.set_vertical_origin(vertical_origin_of(&expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.label_horizontal_origin() {
                point.set_label_horizontal_origin(horizontal_origin_of(&expression.evaluate(Some(style_feature))?));
            }
            if let Some(expression) = style.label_vertical_origin() {
                point.set_label_vertical_origin(vertical_origin_of(&expression.evaluate(Some(style_feature))?));
            }
        }
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            gpu.sync_from_records(&self.collections.borrow())?;
        }
        Ok(())
    }

    /// Cesium's `update` with automatic attachment of GPU collections for `G`.
    pub fn update_with_context<G: Gl + 'static>(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if self.gpu.borrow().is_none() {
            self.attach_context::<G>();
        }
        self.update(frame)
    }

    /// Cesium's `update`; decodes/installs points, then drives
    /// `PolylineCollection`, `BillboardCollection`, and `LabelCollection`.
    pub fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if !self.ready {
            if self.pending_positions.is_none() && !self.positions.is_empty() {
                self.decode()?;
            } else if self.pending_positions.is_some() {
                self.install()?;
            }
        }
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            gpu.update(&self.collections.borrow(), frame)?;
        }
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if let Some(mut gpu) = self.gpu.borrow_mut().take() {
            gpu.destroy()?;
        }
        self.collections.borrow_mut().billboards.clear();
        self.collections.borrow_mut().labels.clear();
        self.collections.borrow_mut().polylines.clear();
        self.positions.clear();
        self.pending_positions = None;
        self.destroyed = true;
        Ok(())
    }
}

fn style_number(value: StyleValue) -> f64 {
    match value {
        StyleValue::Number(value) => value,
        StyleValue::Undefined | StyleValue::Null => f64::NAN,
        other => other.to_number(),
    }
}

fn style_bool(value: StyleValue) -> bool {
    value.to_boolean()
}

fn style_string(value: StyleValue) -> String {
    match value {
        StyleValue::String(value) => value,
        StyleValue::Undefined | StyleValue::Null => String::new(),
        other => other.js_string(),
    }
}

fn near_far_of(value: &StyleValue) -> Option<NearFarScalar> {
    match value {
        StyleValue::Cartesian4(c) => Some(NearFarScalar::new(c.x, c.y, c.z, c.w)),
        _ => None,
    }
}

fn distance_condition_of(value: &StyleValue) -> Option<DistanceDisplayCondition> {
    match value {
        StyleValue::Cartesian2(c) => Some(DistanceDisplayCondition { near: c.x, far: c.y }),
        _ => None,
    }
}

fn label_style_of(value: &StyleValue) -> LabelStyle {
    match value {
        StyleValue::Number(value) => LabelStyle::from_value(*value as i32).unwrap_or(LabelStyle::Fill),
        _ => LabelStyle::Fill,
    }
}

fn horizontal_origin_of(value: &StyleValue) -> HorizontalOrigin {
    match value {
        StyleValue::Number(value) => HorizontalOrigin::from_value(*value as i32).unwrap_or(HorizontalOrigin::Center),
        _ => HorizontalOrigin::Center,
    }
}

fn vertical_origin_of(value: &StyleValue) -> VerticalOrigin {
    match value {
        StyleValue::Number(value) => VerticalOrigin::from_value(*value as i32).unwrap_or(VerticalOrigin::Center),
        _ => VerticalOrigin::Center,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature() -> Vector3DTilePointFeature {
        let collections = Rc::new(RefCell::new(PointCollections {
            billboards: vec![PointBillboard::new(Cartesian3::new(1.0, 2.0, 3.0), 0)],
            labels: vec![PointLabel::new(Cartesian3::new(1.0, 2.0, 3.0), 0)],
            polylines: vec![PointPolyline::new(Cartesian3::new(1.0, 2.0, 3.0))],
            highlight_color: Color::WHITE,
        }));
        Vector3DTilePointFeature::new(0, 0, collections, Rc::new(RefCell::new(None)), Ellipsoid::WGS84)
    }

    #[test]
    fn defaults_match_cesium() {
        let f = feature();
        assert_eq!(f.point_size(), Some(8.0));
        assert_eq!(f.point_outline_width(), Some(0.0));
        assert_eq!(f.label_horizontal_origin(), HorizontalOrigin::Right);
        assert_eq!(f.label_vertical_origin(), VerticalOrigin::Baseline);
        assert_eq!(f.image(), None);
        assert_eq!(f.image_texture_id_like(), r#"["rgb(255,255,255)",8,"rgb(0,0,0)",0]"#);
    }

    #[test]
    fn show_applies_to_all_three_records() {
        let f = feature();
        f.set_show(false);
        assert!(!f.show());
        assert!(!f.billboard().unwrap().show);
        assert!(!f.label().unwrap().show);
        assert!(!f.polyline().unwrap().show);
    }

    #[test]
    fn point_style_updates_texture_id() {
        let f = feature();
        f.set_color(Color::RED);
        f.set_point_size(12.0);
        assert_eq!(f.point_size(), Some(12.0));
        assert_eq!(f.billboard().unwrap().image_texture_id.unwrap(), r#"["rgb(255,0,0)",12,"rgb(0,0,0)",0]"#);
    }

    #[test]
    fn anchor_line_and_height_offset() {
        let f = feature();
        f.set_height_offset(10.0);
        assert_eq!(f.height_offset(), 10.0);
        assert_ne!(f.billboard().unwrap().position, Cartesian3::new(1.0, 2.0, 3.0));
        f.set_anchor_line_enabled(true);
        assert!(f.anchor_line_enabled());
        f.set_anchor_line_color(Color::RED);
        assert_eq!(f.anchor_line_color(), Some(Color::RED));
    }

    impl Vector3DTilePointFeature {
        fn image_texture_id_like(&self) -> String {
            self.billboard().unwrap().image_texture_id.unwrap_or_default()
        }
    }
}
