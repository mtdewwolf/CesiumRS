//! CesiumJS `Geometry`, `GeometryAttribute`, `GeometryAttributes`,
//! `GeometryType`, and `GeometryOffsetAttribute`: vertex data ready for the
//! renderer.

use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::component_datatype::ComponentDatatype;
use super::ellipsoid::Ellipsoid;
use super::matrix2::Matrix2;
use super::matrix3::Matrix3;
use super::primitive_type::PrimitiveType;
use super::quaternion::Quaternion;
use super::rectangle::Rectangle;
use super::transforms::Transforms;
use super::typed_array::TypedArray;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GeometryType {
    #[default]
    None = 0,
    Triangles = 1,
    Lines = 2,
    Polylines = 3,
}

/// Which vertices move when a geometry is extruded or offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GeometryOffsetAttribute {
    None = 0,
    Top = 1,
    All = 2,
}

impl GeometryOffsetAttribute {
    /// The value Cesium packs, with -1 for no attribute.
    pub fn pack(value: Option<Self>) -> f64 {
        value.map_or(-1.0, |v| v as i32 as f64)
    }

    pub fn unpack(value: f64) -> Option<Self> {
        match value as i32 {
            0 => Some(Self::None),
            1 => Some(Self::Top),
            2 => Some(Self::All),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeometryAttribute {
    pub component_datatype: ComponentDatatype,
    /// 1 to 4.
    pub components_per_attribute: u8,
    pub normalize: bool,
    pub values: TypedArray,
}

impl GeometryAttribute {
    pub fn new(component_datatype: ComponentDatatype, components_per_attribute: u8, values: TypedArray) -> Self {
        Self { component_datatype, components_per_attribute, normalize: false, values }
    }

    pub fn doubles(components_per_attribute: u8, values: Vec<f64>) -> Self {
        Self::new(ComponentDatatype::Double, components_per_attribute, TypedArray::Float64(values))
    }

    /// Stores `values` as 32-bit floats, as Cesium's `Float32Array`s do.
    pub fn floats(components_per_attribute: u8, values: &[f64]) -> Self {
        Self::new(
            ComponentDatatype::Float,
            components_per_attribute,
            TypedArray::Float32(values.iter().map(|&v| v as f32).collect()),
        )
    }

    /// Cesium's per-vertex `applyOffset` attribute: 0 for
    /// [`GeometryOffsetAttribute::None`], else 1.
    pub fn apply_offset(vertex_count: usize, offset: GeometryOffsetAttribute) -> Self {
        let value = if offset == GeometryOffsetAttribute::None { 0 } else { 1 };
        Self::new(ComponentDatatype::UnsignedByte, 1, TypedArray::Uint8(vec![value; vertex_count]))
    }
}

/// The standard attributes, plus any others by name (such as `applyOffset`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeometryAttributes {
    pub position: Option<GeometryAttribute>,
    pub normal: Option<GeometryAttribute>,
    pub st: Option<GeometryAttribute>,
    pub bitangent: Option<GeometryAttribute>,
    pub tangent: Option<GeometryAttribute>,
    pub color: Option<GeometryAttribute>,
    pub others: Vec<(String, GeometryAttribute)>,
}

impl GeometryAttributes {
    /// Every attribute with its name, standard ones first in Cesium's order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &GeometryAttribute)> {
        [
            ("position", &self.position),
            ("normal", &self.normal),
            ("st", &self.st),
            ("bitangent", &self.bitangent),
            ("tangent", &self.tangent),
            ("color", &self.color),
        ]
        .into_iter()
        .filter_map(|(name, a)| a.as_ref().map(|a| (name, a)))
        .chain(self.others.iter().map(|(name, a)| (name.as_str(), a)))
    }

    pub fn get(&self, name: &str) -> Option<&GeometryAttribute> {
        self.iter().find(|(n, _)| *n == name).map(|(_, a)| a)
    }

    fn is_standard(name: &str) -> bool {
        matches!(name, "position" | "normal" | "st" | "bitangent" | "tangent" | "color")
    }

    fn standard_mut(&mut self, name: &str) -> Option<&mut Option<GeometryAttribute>> {
        Some(match name {
            "position" => &mut self.position,
            "normal" => &mut self.normal,
            "st" => &mut self.st,
            "bitangent" => &mut self.bitangent,
            "tangent" => &mut self.tangent,
            "color" => &mut self.color,
            _ => return None,
        })
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut GeometryAttribute> {
        if Self::is_standard(name) {
            return self.standard_mut(name).and_then(|slot| slot.as_mut());
        }
        self.others.iter_mut().find(|(n, _)| n == name).map(|(_, a)| a)
    }

    /// Sets an attribute by name, standard or not (`attributes[name] = ...`).
    pub fn set(&mut self, name: &str, attribute: GeometryAttribute) {
        match self.standard_mut(name) {
            Some(slot) => *slot = Some(attribute),
            None => self.set_other(name, attribute),
        }
    }

    /// Removes an attribute by name (`delete attributes[name]`).
    pub fn remove(&mut self, name: &str) -> Option<GeometryAttribute> {
        if let Some(slot) = self.standard_mut(name) {
            return slot.take();
        }
        let i = self.others.iter().position(|(n, _)| n == name)?;
        Some(self.others.remove(i).1)
    }

    /// Attribute names in iteration order.
    pub fn names(&self) -> Vec<String> {
        self.iter().map(|(n, _)| n.to_string()).collect()
    }

    pub fn set_other(&mut self, name: &str, attribute: GeometryAttribute) {
        match self.others.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = attribute,
            None => self.others.push((name.to_string(), attribute)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub attributes: GeometryAttributes,
    pub indices: Option<TypedArray>,
    pub primitive_type: PrimitiveType,
    pub bounding_sphere: Option<BoundingSphere>,
    pub geometry_type: GeometryType,
    pub bounding_sphere_cv: Option<BoundingSphere>,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl Geometry {
    pub fn new(attributes: GeometryAttributes, indices: Option<TypedArray>, primitive_type: PrimitiveType) -> Self {
        Self {
            attributes,
            indices,
            primitive_type,
            bounding_sphere: None,
            geometry_type: GeometryType::None,
            bounding_sphere_cv: None,
            offset_attribute: None,
        }
    }

    /// Vertices per attribute, or `None` if the attributes disagree (Cesium
    /// throws). -1 when there are no attributes, as in Cesium.
    pub fn compute_number_of_vertices(&self) -> Option<f64> {
        let mut number_of_vertices = -1.0;
        for (_, attribute) in self.attributes.iter() {
            let num = attribute.values.len() as f64 / f64::from(attribute.components_per_attribute);
            if number_of_vertices != num && number_of_vertices != -1.0 {
                return None;
            }
            number_of_vertices = num;
        }
        Some(number_of_vertices)
    }

    /// Cesium's `Geometry._textureCoordinateRotationPoints`: three corners of
    /// the rotated texture space, as six numbers.
    pub fn texture_coordinate_rotation_points(
        positions: &[Cartesian3],
        st_rotation: f64,
        ellipsoid: &Ellipsoid,
        bounding_rectangle: &Rectangle,
    ) -> [f64; 6] {
        let rectangle_center = bounding_rectangle.center();
        let enu_center = ellipsoid.cartographic_to_cartesian(&rectangle_center);
        let enu_to_fixed_frame = Transforms::east_north_up_to_fixed_frame(enu_center, ellipsoid);
        // The ENU frame is rigid, so the inverse always exists.
        let fixed_frame_to_enu = enu_to_fixed_frame.inverse().unwrap_or(enu_to_fixed_frame);

        let corners = [
            (bounding_rectangle.west, bounding_rectangle.south),
            (bounding_rectangle.west, bounding_rectangle.north),
            (bounding_rectangle.east, bounding_rectangle.south),
        ];
        let bounding_points_enu: Vec<Cartesian2> = corners
            .iter()
            .map(|&(lon, lat)| {
                let p = ellipsoid.cartographic_to_cartesian(&Cartographic::new(lon, lat, 0.0));
                let e = fixed_frame_to_enu.multiply_by_point_as_vector(p);
                Cartesian2::new(e.x, e.y)
            })
            .collect();

        let rotation = Quaternion::from_axis_angle(Cartesian3::UNIT_Z, -st_rotation);
        let texture_matrix = Matrix3::from_quaternion(&rotation);
        let (mut enu_min_x, mut enu_min_y) = (f64::INFINITY, f64::INFINITY);
        let (mut enu_max_x, mut enu_max_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in positions {
            let pos_enu = texture_matrix.multiply_by_vector(fixed_frame_to_enu.multiply_by_point_as_vector(*p));
            enu_min_x = super::math::js_min(enu_min_x, pos_enu.x);
            enu_min_y = super::math::js_min(enu_min_y, pos_enu.y);
            enu_max_x = super::math::js_max(enu_max_x, pos_enu.x);
            enu_max_y = super::math::js_max(enu_max_y, pos_enu.y);
        }

        let to_desired_in_computed = Matrix2::from_rotation(st_rotation);
        let bounding_enu_min = bounding_points_enu[0];
        let bounding_points_width = bounding_points_enu[2].x - bounding_enu_min.x;
        let bounding_points_height = bounding_points_enu[1].y - bounding_enu_min.y;
        let mut result = [0.0; 6];
        let points = [(enu_min_x, enu_min_y), (enu_min_x, enu_max_y), (enu_max_x, enu_min_y)];
        for (i, (x, y)) in points.into_iter().enumerate() {
            let p = to_desired_in_computed.multiply_by_vector(Cartesian2::new(x, y));
            result[2 * i] = (p.x - bounding_enu_min.x) / bounding_points_width;
            result[2 * i + 1] = (p.y - bounding_enu_min.y) / bounding_points_height;
        }
        result
    }
}
