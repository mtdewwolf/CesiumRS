//! CesiumJS `EllipseGeometry`: a filled ellipse on the ellipsoid, or the
//! volume it extrudes, with texture coordinates and normal frames.

use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::component_datatype::ComponentDatatype;
use super::ellipse_geometry_library::{compute_ellipse_positions, raise_positions_to_height, EllipseOptions};
use super::ellipsoid::Ellipsoid;
use super::geographic_projection::GeographicProjection;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::geometry_instance::GeometryInstance;
use super::geometry_pipeline::combine_instances;
use super::index_datatype::IndexDatatype;
use super::map_projection::MapProjection;
use super::math::{equals_epsilon, js_max, js_min, EPSILON2, EPSILON7, PI, PI_OVER_TWO, RADIANS_PER_DEGREE};
use super::matrix3::Matrix3;
use super::primitive_type::PrimitiveType;
use super::quaternion::Quaternion;
use super::rectangle::Rectangle;
use super::typed_array::TypedArray;
use super::vertex_format::VertexFormat;

/// Construction options, with Cesium's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseGeometryOptions {
    pub center: Cartesian3,
    pub semi_major_axis: f64,
    pub semi_minor_axis: f64,
    pub ellipsoid: Ellipsoid,
    pub height: f64,
    /// Defaults to `height`.
    pub extruded_height: Option<f64>,
    pub rotation: f64,
    pub st_rotation: f64,
    pub granularity: f64,
    pub vertex_format: VertexFormat,
    pub shadow_volume: bool,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl EllipseGeometryOptions {
    pub fn new(center: Cartesian3, semi_major_axis: f64, semi_minor_axis: f64) -> Self {
        Self {
            center,
            semi_major_axis,
            semi_minor_axis,
            ellipsoid: Ellipsoid::WGS84,
            height: 0.0,
            extruded_height: None,
            rotation: 0.0,
            st_rotation: 0.0,
            granularity: RADIANS_PER_DEGREE,
            vertex_format: VertexFormat::DEFAULT,
            shadow_volume: false,
            offset_attribute: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipseGeometry {
    pub center: Cartesian3,
    pub semi_major_axis: f64,
    pub semi_minor_axis: f64,
    pub ellipsoid: Ellipsoid,
    pub rotation: f64,
    pub st_rotation: f64,
    /// The higher of the two heights.
    pub height: f64,
    pub granularity: f64,
    pub vertex_format: VertexFormat,
    /// The lower of the two heights.
    pub extruded_height: f64,
    pub shadow_volume: bool,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

/// The per-call settings `computeTopBottomAttributes` and
/// `computeWallAttributes` read.
struct Settings {
    center: Cartesian3,
    semi_major_axis: f64,
    semi_minor_axis: f64,
    ellipsoid: Ellipsoid,
    st_rotation: f64,
    height: f64,
    extruded_height: f64,
    vertex_format: VertexFormat,
    shadow_volume: bool,
    offset_attribute: Option<GeometryOffsetAttribute>,
}

/// Running bounds of the texture coordinates.
struct TexBounds {
    min: Cartesian2,
    max: Cartesian2,
}

impl TexBounds {
    fn new() -> Self {
        Self {
            min: Cartesian2::new(f64::INFINITY, f64::INFINITY),
            max: Cartesian2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }

    fn add(&mut self, t: Cartesian2) {
        self.min.x = js_min(t.x, self.min.x);
        self.min.y = js_min(t.y, self.min.y);
        self.max.x = js_max(t.x, self.max.x);
        self.max.y = js_max(t.y, self.max.y);
    }

    /// Rescales `Float32Array` texture coordinates to [0, 1], in place.
    fn normalize(&self, st: &mut [f32]) {
        for k in (0..st.len()).step_by(2) {
            st[k] = ((f64::from(st[k]) - self.min.x) / (self.max.x - self.min.x)) as f32;
            st[k + 1] = ((f64::from(st[k + 1]) - self.min.y) / (self.max.y - self.min.y)) as f32;
        }
    }
}

impl Settings {
    /// The texture coordinate of `position`: rotate about the center's
    /// normal, project, and scale by the axes.
    fn tex_coord(
        &self,
        position: Cartesian3,
        texture_matrix: &Matrix3,
        projection: &GeographicProjection,
        projected_center: Cartesian3,
    ) -> Option<Cartesian2> {
        let rotated = texture_matrix.multiply_by_vector(position);
        let projected = projection.project(&self.ellipsoid.cartesian_to_cartographic(rotated)?).subtract(projected_center);
        Some(Cartesian2::new(
            (projected.x + self.semi_major_axis) / (2.0 * self.semi_major_axis),
            (projected.y + self.semi_minor_axis) / (2.0 * self.semi_minor_axis),
        ))
    }

    fn frame(&self) -> Option<(GeographicProjection, Cartesian3, Cartesian3)> {
        let projection = GeographicProjection::new(self.ellipsoid);
        let projected_center = projection.project(&self.ellipsoid.cartesian_to_cartographic(self.center)?);
        let geodetic_normal = self
            .ellipsoid
            .geodetic_surface_normal(self.ellipsoid.scale_to_geodetic_surface(self.center)?)?;
        Some((projection, projected_center, geodetic_normal))
    }

    fn apply_offset(&self, attributes: &mut GeometryAttributes, size: usize) {
        let Some(offset) = self.offset_attribute else { return };
        let mut values = vec![0u8; size];
        match offset {
            GeometryOffsetAttribute::Top => values[..size / 2].fill(1),
            GeometryOffsetAttribute::None => {}
            GeometryOffsetAttribute::All => values.fill(1),
        }
        attributes.set_other(
            "applyOffset",
            GeometryAttribute::new(ComponentDatatype::UnsignedByte, 1, TypedArray::Uint8(values)),
        );
    }
}

fn set3(values: &mut Option<Vec<f64>>, i: usize, v: Cartesian3) {
    if let Some(values) = values {
        values[i..i + 3].copy_from_slice(&[v.x, v.y, v.z]);
    }
}

fn float_attribute(components: u8, values: Option<Vec<f64>>) -> Option<GeometryAttribute> {
    values.map(|v| GeometryAttribute::floats(components, &v))
}

fn st_attribute(values: Option<Vec<f32>>) -> Option<GeometryAttribute> {
    values.map(|v| GeometryAttribute::new(ComponentDatatype::Float, 2, TypedArray::Float32(v)))
}

fn compute_top_bottom_attributes(positions: &[f64], s: &Settings, extrude: bool) -> Option<GeometryAttributes> {
    let vf = s.vertex_format;
    let size = if extrude { (positions.len() / 3) * 2 } else { positions.len() / 3 };
    let mut st = vf.st.then(|| vec![0f32; size * 2]);
    let mut normals = vf.normal.then(|| vec![0.0; size * 3]);
    let mut tangents = vf.tangent.then(|| vec![0.0; size * 3]);
    let mut bitangents = vf.bitangent.then(|| vec![0.0; size * 3]);
    let mut extrude_normals = s.shadow_volume.then(|| vec![0.0; size * 3]);

    let (projection, projected_center, geodetic_normal) = s.frame()?;
    let (texture_matrix, tangent_matrix) = if s.st_rotation != 0.0 {
        (
            Matrix3::from_quaternion(&Quaternion::from_axis_angle(geodetic_normal, s.st_rotation)),
            Matrix3::from_quaternion(&Quaternion::from_axis_angle(geodetic_normal, -s.st_rotation)),
        )
    } else {
        (Matrix3::IDENTITY, Matrix3::IDENTITY)
    };
    let mut bounds = TexBounds::new();
    let length = positions.len();
    let bottom_offset = if extrude { length } else { 0 };
    let st_offset = (bottom_offset / 3) * 2;
    let mut tangent = Cartesian3::ZERO;
    let mut tex_index = 0;
    for i in (0..length).step_by(3) {
        let position = Cartesian3::from_array(positions, i);
        if let Some(st) = st.as_mut() {
            let t = s.tex_coord(position, &texture_matrix, &projection, projected_center)?;
            bounds.add(t);
            if extrude {
                st[tex_index + st_offset] = t.x as f32;
                st[tex_index + 1 + st_offset] = t.y as f32;
            }
            st[tex_index] = t.x as f32;
            st[tex_index + 1] = t.y as f32;
            tex_index += 2;
        }
        if vf.normal || vf.tangent || vf.bitangent || s.shadow_volume {
            let normal = s.ellipsoid.geodetic_surface_normal(position).unwrap_or(Cartesian3::new(f64::NAN, f64::NAN, f64::NAN));
            if s.shadow_volume {
                set3(&mut extrude_normals, i + bottom_offset, normal.negate());
            }
            if vf.normal || vf.tangent || vf.bitangent {
                if vf.tangent || vf.bitangent {
                    tangent = tangent_matrix.multiply_by_vector(Cartesian3::UNIT_Z.cross(normal).normalize());
                }
                if vf.normal {
                    set3(&mut normals, i, normal);
                    if extrude {
                        set3(&mut normals, i + bottom_offset, normal.negate());
                    }
                }
                if vf.tangent {
                    set3(&mut tangents, i, tangent);
                    if extrude {
                        set3(&mut tangents, i + bottom_offset, tangent.negate());
                    }
                }
                if vf.bitangent {
                    let bitangent = normal.cross(tangent).normalize();
                    set3(&mut bitangents, i, bitangent);
                    if extrude {
                        set3(&mut bitangents, i + bottom_offset, bitangent);
                    }
                }
            }
        }
    }
    if let Some(st) = st.as_mut() {
        bounds.normalize(st);
    }

    let mut attributes = GeometryAttributes::default();
    if vf.position {
        let raised = raise_positions_to_height(positions, &s.ellipsoid, s.height, s.extruded_height, extrude);
        attributes.position = Some(GeometryAttribute::doubles(3, raised));
    }
    attributes.st = st_attribute(st);
    attributes.normal = float_attribute(3, normals);
    attributes.tangent = float_attribute(3, tangents);
    attributes.bitangent = float_attribute(3, bitangents);
    if let Some(e) = float_attribute(3, extrude_normals) {
        attributes.set_other("extrudeDirection", e);
    }
    if extrude {
        s.apply_offset(&mut attributes, size);
    }
    Some(attributes)
}

/// Triangle indices for the rows of fill points, `numPts` per quarter.
fn top_indices(num_pts: usize) -> Vec<u32> {
    let declared = (12 * (num_pts * (num_pts + 1))).saturating_sub(6);
    let mut indices: Vec<u32> = Vec::with_capacity(declared);
    let mut push = |a: usize, b: usize, c: usize| indices.extend([a as u32, b as u32, c as u32]);
    let mut prev_index = 0;
    let mut position_index = 1;
    for _ in 0..3 {
        push(position_index, prev_index, position_index + 1);
        position_index += 1;
    }
    for i in 2..num_pts + 1 {
        position_index = i * (i + 1) - 1;
        prev_index = (i - 1) * i - 1;
        push(position_index, prev_index, position_index + 1);
        position_index += 1;
        let num_interior = 2 * i;
        for _ in 0..num_interior - 1 {
            push(position_index, prev_index, prev_index + 1);
            prev_index += 1;
            push(position_index, prev_index, position_index + 1);
            position_index += 1;
        }
        push(position_index, prev_index, position_index + 1);
        position_index += 1;
    }
    let num_interior = num_pts * 2;
    position_index += 1;
    prev_index += 1;
    for _ in 0..num_interior.saturating_sub(1) {
        push(position_index, prev_index, prev_index + 1);
        prev_index += 1;
        push(position_index, prev_index, position_index + 1);
        position_index += 1;
    }
    push(position_index, prev_index, prev_index + 1);
    prev_index += 1;
    push(position_index, prev_index, prev_index + 1);
    position_index += 1;
    prev_index += 1;
    prev_index += 1;
    let mut i = num_pts as isize - 1;
    while i > 1 {
        push(prev_index, prev_index + 1, position_index);
        prev_index += 1;
        let num_interior = 2 * i as usize;
        for _ in 0..num_interior - 1 {
            push(position_index, prev_index, prev_index + 1);
            prev_index += 1;
            push(position_index, prev_index, position_index + 1);
            position_index += 1;
        }
        push(prev_index, prev_index + 1, position_index);
        prev_index += 2;
        position_index += 1;
        i -= 1;
    }
    for _ in 0..3 {
        push(prev_index, prev_index + 1, position_index);
        prev_index += 1;
    }
    if indices.len() < declared {
        indices.resize(declared, 0);
    }
    indices
}

fn compute_wall_attributes(positions: &[f64], s: &Settings) -> Option<GeometryAttributes> {
    let vf = s.vertex_format;
    let size = (positions.len() / 3) * 2;
    let mut final_positions = vec![0.0; size * 3];
    let mut st = vf.st.then(|| vec![0f32; size * 2]);
    let mut normals = vf.normal.then(|| vec![0.0; size * 3]);
    let mut tangents = vf.tangent.then(|| vec![0.0; size * 3]);
    let mut bitangents = vf.bitangent.then(|| vec![0.0; size * 3]);
    let mut extrude_normals = s.shadow_volume.then(|| vec![0.0; size * 3]);

    let (projection, projected_center, geodetic_normal) = s.frame()?;
    let texture_matrix = Matrix3::from_quaternion(&Quaternion::from_axis_angle(geodetic_normal, s.st_rotation));
    let mut bounds = TexBounds::new();
    let length = positions.len();
    let st_offset = (length / 3) * 2;
    let mut tex_index = 0;
    for i in (0..length).step_by(3) {
        let original = Cartesian3::from_array(positions, i);
        if let Some(st) = st.as_mut() {
            let t = s.tex_coord(original, &texture_matrix, &projection, projected_center)?;
            bounds.add(t);
            st[tex_index + st_offset] = t.x as f32;
            st[tex_index + 1 + st_offset] = t.y as f32;
            st[tex_index] = t.x as f32;
            st[tex_index + 1] = t.y as f32;
            tex_index += 2;
        }
        let surface = s.ellipsoid.scale_to_geodetic_surface(original)?;
        let normal = s.ellipsoid.geodetic_surface_normal(surface)?;
        if s.shadow_volume {
            set3(&mut extrude_normals, i + length, normal.negate());
        }
        let position = surface.add(normal.multiply_by_scalar(s.height));
        let extruded = surface.add(normal.multiply_by_scalar(s.extruded_height));
        if vf.position {
            final_positions[i + length..i + length + 3].copy_from_slice(&[extruded.x, extruded.y, extruded.z]);
            final_positions[i..i + 3].copy_from_slice(&[position.x, position.y, position.z]);
        }
        if vf.normal || vf.tangent || vf.bitangent {
            let bitangent = normal;
            let next = Cartesian3::from_array(positions, (i + 3) % length).subtract(position);
            let bottom = extruded.subtract(position);
            let wall_normal = bottom.cross(next).normalize();
            if vf.normal {
                set3(&mut normals, i, wall_normal);
                set3(&mut normals, i + length, wall_normal);
            }
            if vf.tangent {
                let tangent = bitangent.cross(wall_normal).normalize();
                set3(&mut tangents, i, tangent);
                set3(&mut tangents, i + length, tangent);
            }
            if vf.bitangent {
                set3(&mut bitangents, i, bitangent);
                set3(&mut bitangents, i + length, bitangent);
            }
        }
    }
    if let Some(st) = st.as_mut() {
        bounds.normalize(st);
    }

    let mut attributes = GeometryAttributes::default();
    if vf.position {
        attributes.position = Some(GeometryAttribute::doubles(3, final_positions));
    }
    attributes.st = st_attribute(st);
    attributes.normal = float_attribute(3, normals);
    attributes.tangent = float_attribute(3, tangents);
    attributes.bitangent = float_attribute(3, bitangents);
    if let Some(e) = float_attribute(3, extrude_normals) {
        attributes.set_other("extrudeDirection", e);
    }
    s.apply_offset(&mut attributes, size);
    Some(attributes)
}

fn compute_wall_indices(positions: &[f64]) -> Vec<u32> {
    let length = positions.len() / 3;
    let mut indices = Vec::with_capacity(length * 6);
    for i in 0..length {
        let ul = i;
        let ll = i + length;
        let ur = (ul + 1) % length;
        let lr = ur + length;
        indices.extend([ul, ll, ur, ur, ll, lr].map(|v| v as u32));
    }
    indices
}

/// `IndexDatatype.createTypedArray` with Cesium's (sometimes fractional)
/// vertex count.
fn index_array(number_of_vertices: f64, indices: &[u32]) -> TypedArray {
    IndexDatatype::create_typed_array(if number_of_vertices >= 65536.0 { 65536 } else { 0 }, indices)
}

impl EllipseGeometry {
    pub const PACKED_LENGTH: usize = 3 + Ellipsoid::PACKED_LENGTH + VertexFormat::PACKED_LENGTH + 9;

    /// `None` if the semi-major axis is shorter than the semi-minor axis or
    /// the granularity is not positive (Cesium throws).
    pub fn new(options: EllipseGeometryOptions) -> Option<Self> {
        if options.semi_major_axis < options.semi_minor_axis || options.granularity <= 0.0 {
            return None;
        }
        let height = options.height;
        let extruded_height = options.extruded_height.unwrap_or(height);
        Some(Self {
            center: options.center,
            semi_major_axis: options.semi_major_axis,
            semi_minor_axis: options.semi_minor_axis,
            ellipsoid: options.ellipsoid,
            rotation: options.rotation,
            st_rotation: options.st_rotation,
            height: js_max(extruded_height, height),
            granularity: options.granularity,
            vertex_format: options.vertex_format,
            extruded_height: js_min(extruded_height, height),
            shadow_volume: options.shadow_volume,
            offset_attribute: options.offset_attribute,
        })
    }

    pub fn pack(&self, array: &mut [f64], mut i: usize) {
        self.center.pack(array, i);
        i += 3;
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        let values = [
            self.semi_major_axis,
            self.semi_minor_axis,
            self.rotation,
            self.st_rotation,
            self.height,
            self.granularity,
            self.extruded_height,
            if self.shadow_volume { 1.0 } else { 0.0 },
            GeometryOffsetAttribute::pack(self.offset_attribute),
        ];
        array[i..i + 9].copy_from_slice(&values);
    }

    pub fn unpack(array: &[f64], i: usize) -> Option<Self> {
        let center = Cartesian3::unpack(array, i);
        let ellipsoid = Ellipsoid::unpack(array, i + 3);
        let vertex_format = VertexFormat::unpack(array, i + 3 + Ellipsoid::PACKED_LENGTH);
        let o = i + 3 + Ellipsoid::PACKED_LENGTH + VertexFormat::PACKED_LENGTH;
        let a = &array[o..o + 9];
        Self::new(EllipseGeometryOptions {
            center,
            semi_major_axis: a[0],
            semi_minor_axis: a[1],
            ellipsoid,
            height: a[4],
            extruded_height: Some(a[6]),
            rotation: a[2],
            st_rotation: a[3],
            granularity: a[5],
            vertex_format,
            shadow_volume: a[7] == 1.0,
            offset_attribute: GeometryOffsetAttribute::unpack(a[8]),
        })
    }

    /// The cartographic rectangle around the ellipse; one that spans more
    /// than half the globe becomes a band from -π to π.
    pub fn compute_rectangle(
        center: Cartesian3,
        semi_major_axis: f64,
        semi_minor_axis: f64,
        rotation: f64,
        granularity: f64,
        ellipsoid: &Ellipsoid,
    ) -> Rectangle {
        let options = EllipseOptions { center, semi_major_axis, semi_minor_axis, rotation, granularity };
        let flat = compute_ellipse_positions(&options, false, true).outer_positions.unwrap_or_default();
        let positions: Vec<Cartesian3> = (0..flat.len() / 3).map(|i| Cartesian3::from_array(&flat, i * 3)).collect();
        let mut rectangle = Rectangle::from_cartesian_array(&positions, ellipsoid);
        if rectangle.width() > PI {
            rectangle.north = if rectangle.north > 0.0 { PI_OVER_TWO - EPSILON7 } else { rectangle.north };
            rectangle.south = if rectangle.south < 0.0 { EPSILON7 - PI_OVER_TWO } else { rectangle.south };
            rectangle.east = PI;
            rectangle.west = -PI;
        }
        rectangle
    }

    pub fn rectangle(&self) -> Rectangle {
        Self::compute_rectangle(
            self.center,
            self.semi_major_axis,
            self.semi_minor_axis,
            self.rotation,
            self.granularity,
            &self.ellipsoid,
        )
    }

    /// Three corners of the rotated texture space, as six numbers.
    pub fn texture_coordinate_rotation_points(&self) -> [f64; 6] {
        let st_rotation = -self.st_rotation;
        if st_rotation == 0.0 {
            return [0.0, 0.0, 0.0, 1.0, 1.0, 0.0];
        }
        let options = EllipseOptions {
            center: self.center,
            semi_major_axis: self.semi_major_axis,
            semi_minor_axis: self.semi_minor_axis,
            rotation: self.rotation,
            granularity: self.granularity,
        };
        let flat = compute_ellipse_positions(&options, false, true).outer_positions.unwrap_or_default();
        let positions: Vec<Cartesian3> = (0..flat.len() / 3).map(|i| Cartesian3::from_array(&flat, i * 3)).collect();
        Geometry::texture_coordinate_rotation_points(&positions, st_rotation, &self.ellipsoid, &self.rectangle())
    }

    /// A shadow volume between the heights the two functions return for
    /// this granularity and ellipsoid.
    pub fn create_shadow_volume(
        &self,
        min_height: impl Fn(f64, &Ellipsoid) -> f64,
        max_height: impl Fn(f64, &Ellipsoid) -> f64,
    ) -> Option<Self> {
        let mut options = EllipseGeometryOptions::new(self.center, self.semi_major_axis, self.semi_minor_axis);
        options.ellipsoid = self.ellipsoid;
        options.rotation = self.rotation;
        options.st_rotation = self.st_rotation;
        options.granularity = self.granularity;
        options.extruded_height = Some(min_height(self.granularity, &self.ellipsoid));
        options.height = max_height(self.granularity, &self.ellipsoid);
        options.vertex_format = VertexFormat::POSITION_ONLY;
        options.shadow_volume = true;
        Self::new(options)
    }

    /// The filled ellipse; `None` for a non-positive axis or a center at the
    /// ellipsoid's center.
    pub fn create_geometry(&self) -> Option<Geometry> {
        if self.semi_major_axis <= 0.0 || self.semi_minor_axis <= 0.0 {
            return None;
        }
        let height = self.height;
        let extruded_height = self.extruded_height;
        let extrude = !equals_epsilon(height, extruded_height, 0.0, EPSILON2);
        let center = self.ellipsoid.scale_to_geodetic_surface(self.center)?;
        let ellipse = EllipseOptions {
            center,
            semi_major_axis: self.semi_major_axis,
            semi_minor_axis: self.semi_minor_axis,
            rotation: self.rotation,
            granularity: self.granularity,
        };
        let settings = Settings {
            center,
            semi_major_axis: self.semi_major_axis,
            semi_minor_axis: self.semi_minor_axis,
            ellipsoid: self.ellipsoid,
            st_rotation: self.st_rotation,
            height,
            extruded_height: if extrude { extruded_height } else { height },
            vertex_format: self.vertex_format,
            shadow_volume: extrude && self.shadow_volume,
            offset_attribute: if extrude { self.offset_attribute } else { None },
        };
        let normal = self.ellipsoid.geodetic_surface_normal(center)?;

        let (attributes, indices, bounding_sphere) = if extrude {
            let top = BoundingSphere::new(center.add(normal.multiply_by_scalar(height)), self.semi_major_axis);
            let bottom = BoundingSphere::new(center.add(normal.multiply_by_scalar(extruded_height)), self.semi_major_axis);
            let cep = compute_ellipse_positions(&ellipse, true, true);
            let positions = cep.positions.unwrap_or_default();
            let outer = cep.outer_positions.unwrap_or_default();
            let top_bottom_attributes = compute_top_bottom_attributes(&positions, &settings, true)?;
            let mut indices = top_indices(cep.num_pts.unwrap_or(0));
            let length = indices.len();
            let pos_length = (positions.len() / 3) as u32;
            indices.resize(length * 2, 0);
            for i in (0..length).step_by(3) {
                indices[i + length] = indices[i + 2] + pos_length;
                indices[i + 1 + length] = indices[i + 1] + pos_length;
                indices[i + 2 + length] = indices[i] + pos_length;
            }
            let top_bottom = Geometry::new(
                top_bottom_attributes,
                Some(index_array(f64::from(pos_length) * 2.0 / 3.0, &indices)),
                PrimitiveType::Triangles,
            );
            let wall = Geometry::new(
                compute_wall_attributes(&outer, &settings)?,
                Some(index_array(outer.len() as f64 * 2.0 / 3.0, &compute_wall_indices(&outer))),
                PrimitiveType::Triangles,
            );
            let combined = combine_instances(&[GeometryInstance::new(top_bottom, None), GeometryInstance::new(wall, None)])?
                .into_iter()
                .next()?;
            (combined.attributes, combined.indices, top.union(&bottom))
        } else {
            let bounding_sphere = BoundingSphere::new(center.add(normal.multiply_by_scalar(height)), self.semi_major_axis);
            let cep = compute_ellipse_positions(&ellipse, true, false);
            let positions = cep.positions.unwrap_or_default();
            let mut attributes = compute_top_bottom_attributes(&positions, &settings, false)?;
            let indices =
                IndexDatatype::create_typed_array(positions.len() / 3, &top_indices(cep.num_pts.unwrap_or(0)));
            if let Some(offset) = self.offset_attribute {
                let count = attributes.position.as_ref().map_or(0, |p| p.values.len() / 3);
                attributes.set_other("applyOffset", GeometryAttribute::apply_offset(count, offset));
            }
            (attributes, Some(indices), bounding_sphere)
        };

        let mut geometry = Geometry::new(attributes, indices, PrimitiveType::Triangles);
        geometry.bounding_sphere = Some(bounding_sphere);
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }
}
