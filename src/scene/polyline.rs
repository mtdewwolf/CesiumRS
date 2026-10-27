//! CPU-side state and geometry generation for CesiumJS `Polyline`.
//!
//! Rendering, GPU resource lifecycle, pick-id allocation, and scene frame
//! integration remain to be implemented; see [`PolylineCollection`](super::polyline_collection::PolylineCollection).

use crate::{
    core::{
        array_remove_duplicates::array_remove_duplicates,
        polyline_pipeline::wrap_longitude,
        BoundingSphere, Cartesian3, Color, DistanceDisplayCondition, Matrix4,
    },
    scene::material::{Material, MaterialValue, COLOR_TYPE},
};

pub const POSITION_INDEX: usize = 0;
pub const SHOW_INDEX: usize = 1;
pub const WIDTH_INDEX: usize = 2;
pub const MATERIAL_INDEX: usize = 3;
pub const POSITION_SIZE_INDEX: usize = 4;
pub const DISTANCE_DISPLAY_CONDITION_INDEX: usize = 5;
pub const NUMBER_OF_PROPERTIES: usize = 6;

/// Configuration options for creating or mutating a [`Polyline`].
#[derive(Clone, Debug)]
pub struct PolylineOptions {
    pub show: bool,
    pub width: f64,
    pub r#loop: bool,
    pub material: Material,
    pub positions: Vec<Cartesian3>,
    pub id: Option<String>,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
}

impl Default for PolylineOptions {
    fn default() -> Self {
        let mut material = Material::from_type(COLOR_TYPE);
        material.set_uniform("color", MaterialValue::from_color(Color::WHITE)).expect("Color material has a color uniform");
        Self {
            show: true,
            width: 1.0,
            r#loop: false,
            material,
            positions: Vec::new(),
            id: None,
            distance_display_condition: None,
        }
    }
}

impl PartialEq for PolylineOptions {
    fn eq(&self, other: &Self) -> bool {
        self.show == other.show
            && self.width == other.width
            && self.r#loop == other.r#loop
            && self.material.type_name == other.material.type_name
            && self.material.uniforms == other.material.uniforms
            && self.positions == other.positions
            && self.id == other.id
            && self.distance_display_condition == other.distance_display_condition
    }
}

impl PolylineOptions {
    /// Rust options are typed; Cesium permits negative and non-finite width at construction.
    pub fn validate(&self) -> Result<(), &'static str> {
        Ok(())
    }
}

/// A CPU polyline primitive tracking state, duplicates, antimeridian wrapping,
/// and bounding spheres.
#[derive(Clone, Debug)]
pub struct Polyline {
    options: PolylineOptions,
    actual_positions: Vec<Cartesian3>,
    segments: (Vec<Cartesian3>, Vec<usize>),
    bounding_volume: BoundingSphere,
    bounding_volume_wc: BoundingSphere,
    model_matrix: Matrix4,
    properties_changed: [u32; NUMBER_OF_PROPERTIES],
    dirty: bool,
    attached: bool,
    index: usize,
}

impl Polyline {
    /// Creates a new `Polyline`. Called by `PolylineCollection::add`.
    pub(crate) fn new(options: PolylineOptions, model_matrix: Option<&Matrix4>) -> Result<Self, &'static str> {
        options.validate()?;

        let matrix = model_matrix.copied().unwrap_or(Matrix4::IDENTITY);
        let mut actual = array_remove_duplicates(&options.positions, |a, b, eps| a.equals_epsilon(*b, eps), false, None);
        if options.r#loop && actual.len() > 2 {
            actual.push(actual[0]);
        }

        let segments = wrap_longitude(&actual, Some(&matrix));
        let bounding_volume = BoundingSphere::from_points(&actual);
        let bounding_volume_wc = bounding_volume.transform(&matrix);

        Ok(Self {
            options,
            actual_positions: actual,
            segments,
            bounding_volume,
            bounding_volume_wc,
            model_matrix: matrix,
            properties_changed: [0; NUMBER_OF_PROPERTIES],
            dirty: false,
            attached: true,
            index: 0,
        })
    }

    pub fn options(&self) -> &PolylineOptions {
        &self.options
    }

    pub fn show(&self) -> bool {
        self.options.show
    }

    pub fn set_show(&mut self, show: bool) {
        if self.options.show != show {
            self.options.show = show;
            self.properties_changed[SHOW_INDEX] += 1;
            self.dirty = true;
        }
    }

    pub fn width(&self) -> f64 {
        self.options.width
    }

    pub fn set_width(&mut self, width: f64) -> Result<(), &'static str> {
        if self.options.width != width {
            self.options.width = width;
            self.properties_changed[WIDTH_INDEX] += 1;
            self.dirty = true;
        }
        Ok(())
    }

    pub fn r#loop(&self) -> bool {
        self.options.r#loop
    }

    pub fn set_loop(&mut self, is_loop: bool) {
        if self.options.r#loop != is_loop {
            if is_loop {
                if self.actual_positions.len() > 2
                    && self.actual_positions[0] != self.actual_positions[self.actual_positions.len() - 1]
                {
                    self.actual_positions.push(self.actual_positions[0]);
                }
            } else if self.actual_positions.len() > 2
                && self.actual_positions[0] == self.actual_positions[self.actual_positions.len() - 1]
            {
                self.actual_positions.pop();
            }

            self.options.r#loop = is_loop;
            self.properties_changed[POSITION_SIZE_INDEX] += 1;
            self.dirty = true;
        }
    }

    pub fn material(&self) -> &Material {
        &self.options.material
    }

    pub fn set_material(&mut self, material: Material) {
        self.options.material = material;
        self.properties_changed[MATERIAL_INDEX] += 1;
        self.dirty = true;
    }

    pub fn positions(&self) -> &[Cartesian3] {
        &self.options.positions
    }

    pub fn actual_positions(&self) -> &[Cartesian3] {
        &self.actual_positions
    }

    pub fn set_positions(&mut self, positions: Vec<Cartesian3>) {
        let mut actual = array_remove_duplicates(&positions, |a, b, eps| a.equals_epsilon(*b, eps), false, None);
        if self.options.r#loop && actual.len() > 2 {
            actual.push(actual[0]);
        }

        if self.actual_positions.len() != actual.len() {
            self.properties_changed[POSITION_SIZE_INDEX] += 1;
        }

        self.options.positions = positions;
        self.actual_positions = actual;
        self.bounding_volume = BoundingSphere::from_points(&self.actual_positions);
        self.bounding_volume_wc = self.bounding_volume.transform(&self.model_matrix);
        self.properties_changed[POSITION_INDEX] += 1;
        self.dirty = true;
        let matrix = self.model_matrix;
        self.update(&matrix);
    }

    pub fn id(&self) -> Option<&str> {
        self.options.id.as_deref()
    }

    pub fn set_id(&mut self, id: Option<String>) {
        self.options.id = id;
    }

    pub fn distance_display_condition(&self) -> Option<DistanceDisplayCondition> {
        self.options.distance_display_condition
    }

    pub fn set_distance_display_condition(
        &mut self,
        distance_display_condition: Option<DistanceDisplayCondition>,
    ) -> Result<(), &'static str> {
        if distance_display_condition.is_some_and(|d| d.far <= d.near) {
            return Err("far distance must be greater than near distance.");
        }
        if self.options.distance_display_condition != distance_display_condition {
            self.options.distance_display_condition = distance_display_condition;
            self.properties_changed[DISTANCE_DISPLAY_CONDITION_INDEX] += 1;
            self.dirty = true;
        }
        Ok(())
    }

    pub fn bounding_volume(&self) -> &BoundingSphere {
        &self.bounding_volume
    }

    pub fn bounding_volume_wc(&self) -> &BoundingSphere {
        &self.bounding_volume_wc
    }

    pub fn segments(&self) -> &(Vec<Cartesian3>, Vec<usize>) {
        &self.segments
    }

    pub fn model_matrix(&self) -> &Matrix4 {
        &self.model_matrix
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub(crate) fn set_index(&mut self, index: usize) {
        self.index = index;
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    pub(crate) fn detach(&mut self) {
        self.attached = false;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn properties_changed(&self) -> &[u32; NUMBER_OF_PROPERTIES] {
        &self.properties_changed
    }

    pub fn clean(&mut self) {
        self.dirty = false;
        for i in 0..NUMBER_OF_PROPERTIES - 1 {
            self.properties_changed[i] = 0;
        }
    }

    /// Re-evaluates segments and world bounding sphere when model matrix or positions change.
    pub fn update(&mut self, model_matrix: &Matrix4) {
        let positions_changed = self.properties_changed[POSITION_INDEX] > 0
            || self.properties_changed[POSITION_SIZE_INDEX] > 0;
        if *model_matrix != self.model_matrix || positions_changed {
            let segments = wrap_longitude(&self.actual_positions, Some(model_matrix));
            if segments.0.len() != self.segments.0.len() || segments.1 != self.segments.1 {
                self.properties_changed[POSITION_SIZE_INDEX] += 1;
                self.dirty = true;
            }
            self.segments = segments;
            self.bounding_volume_wc = self.bounding_volume.transform(model_matrix);
        }
        self.model_matrix = *model_matrix;
    }
}

impl PartialEq for Polyline {
    fn eq(&self, other: &Self) -> bool {
        self.options == other.options
    }
}
