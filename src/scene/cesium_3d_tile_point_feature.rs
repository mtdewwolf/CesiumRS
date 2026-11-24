//! A vector point feature controls its billboard, label and anchor polyline together.
use super::{material::MaterialValue, scene::Result, *};
use crate::{core::*, renderer::Gl};
use std::{ops::Deref, rc::Rc};

pub struct Cesium3DTilePointFeature<G: Gl + 'static> {
    feature: Rc<Cesium3DTileFeature<G>>,
    pub billboard: Billboard<G>,
    pub label: LabelHandle,
    pub anchor: Polyline,
    ellipsoid: Ellipsoid,
    color: Option<Color>,
    point_size: Option<f64>,
    outline_color: Option<Color>,
    outline_width: Option<f64>,
    image: Option<String>,
    height_offset: Option<f64>,
}
impl<G: Gl + 'static> Deref for Cesium3DTilePointFeature<G> {
    type Target = Cesium3DTileFeature<G>;
    fn deref(&self) -> &Self::Target {
        &self.feature
    }
}
impl<G: Gl + 'static> Cesium3DTilePointFeature<G> {
    pub const DEFAULT_COLOR: Color = Color::WHITE;
    pub const DEFAULT_POINT_OUTLINE_COLOR: Color = Color::BLACK;
    pub const DEFAULT_POINT_OUTLINE_WIDTH: f64 = 0.0;
    pub const DEFAULT_POINT_SIZE: f64 = 8.0;

    pub fn new(
        feature: Rc<Cesium3DTileFeature<G>>,
        billboard: Billboard<G>,
        label: LabelHandle,
        anchor: Polyline,
        ellipsoid: Ellipsoid,
    ) -> Self {
        let p = Self {
            feature,
            billboard,
            label,
            anchor,
            ellipsoid,
            color: None,
            point_size: None,
            outline_color: None,
            outline_width: None,
            image: None,
            height_offset: None,
        };
        p.refresh_image();
        p
    }
    pub fn show(&self) -> bool {
        self.label.borrow().show()
    }
    pub fn set_show(&self, v: bool) {
        self.label.borrow_mut().set_show(v);
        self.billboard.set_show(v);
        self.anchor.set_show(v);
    }
    pub fn color(&self) -> Option<Color> {
        self.color
    }
    pub fn set_color(&mut self, v: Option<Color>) {
        self.color = v;
        self.refresh_image();
    }
    pub fn point_size(&self) -> Option<f64> {
        self.point_size
    }
    pub fn set_point_size(&mut self, v: Option<f64>) {
        self.point_size = v;
        self.refresh_image();
    }
    pub fn point_outline_color(&self) -> Option<Color> {
        self.outline_color
    }
    pub fn set_point_outline_color(&mut self, v: Option<Color>) {
        self.outline_color = v;
        self.refresh_image();
    }
    pub fn point_outline_width(&self) -> Option<f64> {
        self.outline_width
    }
    pub fn set_point_outline_width(&mut self, v: Option<f64>) {
        self.outline_width = v;
        self.refresh_image();
    }
    pub fn image(&self) -> Option<&str> {
        self.image.as_deref()
    }
    pub fn set_image(&mut self, v: Option<(String, BillboardImage<G>)>) {
        if let Some((uri, image)) = v {
            self.billboard.set_image_with_id(uri.clone(), image);
            self.image = Some(uri);
        } else {
            self.image = None;
            self.refresh_image();
        }
    }
    fn refresh_image(&self) {
        if self.image.is_some() {
            return;
        }
        let color = self.color.unwrap_or(Self::DEFAULT_COLOR);
        let outline = self.outline_color.unwrap_or(Self::DEFAULT_POINT_OUTLINE_COLOR);
        let width = self.outline_width.unwrap_or(Self::DEFAULT_POINT_OUTLINE_WIDTH);
        let size = self.point_size.unwrap_or(Self::DEFAULT_POINT_SIZE);
        let id = format!("point-{:?}-{size}-{:?}-{width}", color.to_bytes(), outline.to_bytes());
        if self.billboard.image().as_ref() != Some(&id) {
            self.billboard.set_image_with_id(id, rasterize_billboard_point(color, outline, width, size));
        }
    }
    pub fn label_color(&self) -> Color {
        self.label.borrow().fill_color()
    }
    pub fn set_label_color(&self, v: Color) {
        self.label.borrow_mut().set_fill_color(v);
        self.anchor.set_show(self.show() && v.alpha > 0.);
    }
    pub fn label_text(&self) -> String {
        self.label.borrow().text().into()
    }
    pub fn set_label_text(&self, v: Option<&str>) {
        self.label.borrow_mut().set_text(v.unwrap_or_default().into());
    }
    pub fn height_offset(&self) -> Option<f64> {
        self.height_offset
    }
    pub fn set_height_offset(&mut self, value: f64) -> Result<()> {
        let mut cart = self
            .ellipsoid
            .cartesian_to_cartographic(self.billboard.position())
            .ok_or_else(|| RuntimeError::new("Point has no cartographic position"))?;
        cart.height += value - self.height_offset.unwrap_or(0.);
        let position = self.ellipsoid.cartographic_to_cartesian(&cart);
        let start = self.anchor.positions().first().copied().unwrap_or(self.billboard.position());
        self.billboard.set_position(position);
        self.label.borrow_mut().set_position(position);
        self.anchor.set_positions(vec![start, position]);
        self.height_offset = Some(value);
        Ok(())
    }
    pub fn anchor_line_enabled(&self) -> bool {
        self.anchor.show()
    }
    pub fn set_anchor_line_enabled(&self, v: bool) {
        self.anchor.set_show(v);
    }
    pub fn anchor_line_color(&self) -> Option<Color> {
        match self.anchor.material().borrow().uniform("color") {
            Some(MaterialValue::Vec4(v)) => Some(Color::new(v.x, v.y, v.z, v.w)),
            _ => None,
        }
    }
    pub fn set_anchor_line_color(&self, color: Color) -> Result<()> {
        self.anchor.material().borrow_mut().set_uniform("color", MaterialValue::from_color(color))
    }
    pub fn label_outline_color(&self) -> Color {
        self.label.borrow().outline_color()
    }
    pub fn set_label_outline_color(&self, value: Color) {
        self.label.borrow_mut().set_outline_color(value);
    }
    pub fn label_outline_width(&self) -> f64 {
        self.label.borrow().outline_width()
    }
    pub fn set_label_outline_width(&self, value: f64) {
        self.label.borrow_mut().set_outline_width(value);
    }
    pub fn label_style(&self) -> LabelStyle {
        self.label.borrow().style()
    }
    pub fn set_label_style(&self, value: LabelStyle) {
        self.label.borrow_mut().set_style(value);
    }
    pub fn background_color(&self) -> Color {
        self.label.borrow().background_color()
    }
    pub fn set_background_color(&self, value: Color) {
        self.label.borrow_mut().set_background_color(value);
    }
    pub fn background_padding(&self) -> Cartesian2 {
        self.label.borrow().background_padding()
    }
    pub fn set_background_padding(&self, value: Cartesian2) {
        self.label.borrow_mut().set_background_padding(value);
    }
    pub fn background_enabled(&self) -> bool {
        self.label.borrow().show_background()
    }
    pub fn set_background_enabled(&self, value: bool) {
        self.label.borrow_mut().set_show_background(value);
    }
    pub fn label_horizontal_origin(&self) -> HorizontalOrigin {
        self.label.borrow().horizontal_origin()
    }
    pub fn set_label_horizontal_origin(&self, value: HorizontalOrigin) {
        self.label.borrow_mut().set_horizontal_origin(value);
    }
    pub fn label_vertical_origin(&self) -> VerticalOrigin {
        self.label.borrow().vertical_origin()
    }
    pub fn set_label_vertical_origin(&self, value: VerticalOrigin) {
        self.label.borrow_mut().set_vertical_origin(value);
    }
    pub fn horizontal_origin(&self) -> HorizontalOrigin {
        self.billboard.horizontal_origin()
    }
    pub fn set_horizontal_origin(&self, value: HorizontalOrigin) {
        self.billboard.set_horizontal_origin(value);
    }
    pub fn vertical_origin(&self) -> VerticalOrigin {
        self.billboard.vertical_origin()
    }
    pub fn set_vertical_origin(&self, value: VerticalOrigin) {
        self.billboard.set_vertical_origin(value);
    }
    pub fn font(&self) -> String {
        self.label.borrow().font().into()
    }
    pub fn set_font(&self, value: String) {
        self.label.borrow_mut().set_font(value);
    }
    pub fn scale_by_distance(&self) -> Option<NearFarScalar> {
        self.label.borrow().scale_by_distance()
    }
    pub fn set_scale_by_distance(&self, value: Option<NearFarScalar>) -> Result<()> {
        self.label.borrow_mut().set_scale_by_distance(value).map_err(RuntimeError::new)?;
        self.billboard.set_scale_by_distance(value)
    }
    pub fn translucency_by_distance(&self) -> Option<NearFarScalar> {
        self.label.borrow().translucency_by_distance()
    }
    pub fn set_translucency_by_distance(&self, value: Option<NearFarScalar>) -> Result<()> {
        self.label.borrow_mut().set_translucency_by_distance(value).map_err(RuntimeError::new)?;
        self.billboard.set_translucency_by_distance(value)
    }
    pub fn distance_display_condition(&self) -> Option<DistanceDisplayCondition> {
        self.label.borrow().distance_display_condition()
    }
    pub fn set_distance_display_condition(&self, value: Option<DistanceDisplayCondition>) -> Result<()> {
        self.label.borrow_mut().set_distance_display_condition(value).map_err(RuntimeError::new)?;
        self.anchor.set_distance_display_condition(value)?;
        self.billboard.set_distance_display_condition(value)
    }
    pub fn disable_depth_test_distance(&self) -> Option<f64> {
        self.label.borrow().disable_depth_test_distance()
    }
    pub fn set_disable_depth_test_distance(&self, value: Option<f64>) -> Result<()> {
        self.label.borrow_mut().set_disable_depth_test_distance(value).map_err(RuntimeError::new)?;
        self.billboard.set_disable_depth_test_distance(value)
    }
}
/// Pure-Rust equivalent of CesiumJS `createBillboardPointCallback` rasterizing an RGBA circle + outline.
pub(crate) fn rasterize_billboard_point<G: Gl + 'static>(
    color: Color,
    outline_color: Color,
    outline_width: f64,
    pixel_size: f64,
) -> BillboardImage<G> {
    let length = (pixel_size + 2.0 * outline_width).max(1.0).ceil() as u32;
    let mut bytes = vec![0u8; (length * length * 4) as usize];
    let center = length as f64 * 0.5;
    let outer_r = center;
    let inner_r = pixel_size * 0.5;
    let fill = [
        (color.red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    ];
    let outline = [
        (outline_color.red.clamp(0.0, 1.0) * 255.0).round() as u8,
        (outline_color.green.clamp(0.0, 1.0) * 255.0).round() as u8,
        (outline_color.blue.clamp(0.0, 1.0) * 255.0).round() as u8,
        (outline_color.alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
    ];
    for y in 0..length {
        for x in 0..length {
            let dx = x as f64 + 0.5 - center;
            let dy = y as f64 + 0.5 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let px = if dist <= inner_r {
                fill
            } else if outline_width > 0.0 && dist <= outer_r {
                outline
            } else {
                [0, 0, 0, 0]
            };
            let idx = ((y * length + x) * 4) as usize;
            bytes[idx..idx + 4].copy_from_slice(&px);
        }
    }
    BillboardImage::pixels(length, length, bytes)
}
