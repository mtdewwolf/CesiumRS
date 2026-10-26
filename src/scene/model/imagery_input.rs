//! CesiumJS `Scene/Model/ImageryInput`: uniform input data for draping imagery
//! over model primitives.

use crate::core::Cartesian4;
use crate::renderer::{Gl, Texture};
use std::rc::Rc;

/// Input information passed to shader uniforms for an imagery texture.
#[derive(Clone)]
pub struct ImageryInput<G: Gl> {
    pub layer_index: usize,
    pub texture: Option<Rc<Texture<G>>>,
    pub texture_translation_and_scale: Cartesian4,
    pub texture_coordinate_rectangle: Cartesian4,
    pub imagery_tex_coord_attribute_set_index: u32,
    pub alpha: f32,
    pub brightness: f32,
    pub contrast: f32,
    pub hue: f32,
    pub saturation: f32,
    pub one_over_gamma: f32,
    pub color_to_alpha: Option<Cartesian4>,
}

impl<G: Gl> ImageryInput<G> {
    pub fn new(
        layer_index: usize,
        texture: Option<Rc<Texture<G>>>,
        texture_translation_and_scale: Cartesian4,
        texture_coordinate_rectangle: Cartesian4,
        imagery_tex_coord_attribute_set_index: u32,
    ) -> Self {
        Self {
            layer_index,
            texture,
            texture_translation_and_scale,
            texture_coordinate_rectangle,
            imagery_tex_coord_attribute_set_index,
            alpha: 1.0,
            brightness: 1.0,
            contrast: 1.0,
            hue: 0.0,
            saturation: 1.0,
            one_over_gamma: 1.0,
            color_to_alpha: None,
        }
    }
}
