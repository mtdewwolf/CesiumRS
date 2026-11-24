//! CesiumJS `Vector3DTilePoints`: decodes vector-tile points into billboards,
//! labels and anchor polylines.
//!
//! The pinned module builds real `BillboardCollection`, `LabelCollection` and
//! `PolylineCollection` objects. Those collections and their GPU resources are
//! separate modules; this port keeps the per-point records and the complete
//! `Cesium3DTilePointFeature` property surface (including defaults, side
//! effects and style evaluation) as CPU state. It therefore matches the point
//! decode, feature and styling behaviour, but does not render or account for
//! the collections' texture atlases.

use super::{
    cesium_3d_tile_style::Cesium3DTileStyle,
    expression::{StyleFeature, StyleValue},
    horizontal_origin::HorizontalOrigin,
    label_style::LabelStyle,
    scene::Result,
    vector_3d_tile_primitive::{BatchTableRef, FeatureList, Vector3DTileFeature},
    vertical_origin::VerticalOrigin,
};
use crate::{
    core::{
        math, Cartesian2, Cartesian3, Color, DistanceDisplayCondition, Ellipsoid, NearFarScalar, Rectangle,
        RuntimeError,
    },
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
            show: false,
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

/// A `Cesium3DTilePointFeature`: batched attributes plus mutable point styling.
#[derive(Clone)]
pub struct Vector3DTilePointFeature {
    pub batch_id: usize,
    pub index: usize,
    collections: Rc<RefCell<PointCollections>>,
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
        "[{}, {}, {}, {}]",
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
        ellipsoid: Ellipsoid,
    ) -> Self {
        let pick_ids = [
            Some(format!("{}_billboard", batch_id)),
            Some(format!("{}_label", batch_id)),
            Some(format!("{}_polyline", batch_id)),
        ];
        Self { batch_id, index, collections, ellipsoid, pick_ids }
    }

    fn records(&self) -> std::cell::Ref<'_, PointCollections> {
        self.collections.borrow()
    }
    fn records_mut(&self) -> std::cell::RefMut<'_, PointCollections> {
        self.collections.borrow_mut()
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
        if let Some(l) = self.records_mut().labels.get_mut(self.index) {
            l.fill_color = value;
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
    pub height_reference: super::HeightReference,
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
            height_reference: super::HeightReference::None,
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
    height_reference: super::HeightReference,
    ellipsoid: Ellipsoid,
    collections: Rc<RefCell<PointCollections>>,
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
            packed_buffer: None,
            pending_positions: None,
            ready: false,
            destroyed: false,
        }
    }

    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn points_length(&self) -> usize {
        self.collections.borrow().billboards.len()
    }
    pub fn textures_byte_length(&self) -> usize {
        // The pinned value is the billboard and label texture-atlas memory.
        // Atlases are owned by the unported collections; report the decoded
        // image payload instead of inventing atlas layout numbers.
        let c = self.collections.borrow();
        c.billboards
            .iter()
            .map(|b| {
                let length = b.size + 2.0 * b.outline_width;
                let side = if length.is_finite() && length > 0.0 { length.ceil() as usize } else { 0 };
                side * side * 4
            })
            .sum()
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
    pub fn height_reference(&self) -> super::HeightReference {
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

    fn install(&mut self) {
        let Some(positions) = self.pending_positions.take() else {
            return;
        };
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
        self.positions.clear();
        self.packed_buffer = None;
        self.ready = true;
    }

    /// Cesium's `createFeatures`; features share the point collections.
    pub fn create_features(&self, features: &mut FeatureList) {
        for (i, &batch_id) in self.batch_ids.iter().enumerate() {
            let index = batch_id as usize;
            if features.len() <= index {
                features.resize(index + 1, None);
            }
            let point = Vector3DTilePointFeature::new(index, i, self.collections.clone(), self.ellipsoid);
            features[index] = Some(Vector3DTileFeature {
                batch_id: index,
                show: true,
                color: Color::WHITE,
                properties: Default::default(),
                point: Some(point),
            });
        }
    }

    /// Cesium's `applyDebugSettings`.
    pub fn apply_debug_settings(&mut self, enabled: bool, color: Color) {
        self.collections.borrow_mut().highlight_color = if enabled { color } else { Color::WHITE };
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
        Ok(())
    }

    /// Cesium's `update`; the first call starts decoding, the next installs it.
    pub fn update(&mut self, _frame: &mut super::FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if !self.ready {
            if self.pending_positions.is_none() && !self.positions.is_empty() {
                self.decode()?;
                return Ok(());
            }
            if self.pending_positions.is_some() {
                self.install();
            }
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
        Vector3DTilePointFeature::new(0, 0, collections, Ellipsoid::WGS84)
    }

    #[test]
    fn defaults_match_cesium() {
        let f = feature();
        assert_eq!(f.point_size(), Some(8.0));
        assert_eq!(f.point_outline_width(), Some(0.0));
        assert_eq!(f.label_horizontal_origin(), HorizontalOrigin::Right);
        assert_eq!(f.label_vertical_origin(), VerticalOrigin::Baseline);
        assert_eq!(f.image(), None);
        assert!(f.image_texture_id_like().contains("8"));
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
        assert!(f.billboard().unwrap().image_texture_id.unwrap().contains("12"));
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
