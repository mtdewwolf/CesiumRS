//! CesiumJS `FrustumGeometry` and `FrustumOutlineGeometry`: the volume or
//! edges of a perspective or orthographic view frustum.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::cartesian4::Cartesian4;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::matrix3::Matrix3;
use super::matrix4::Matrix4;
use super::orthographic_frustum::OrthographicFrustum;
use super::perspective_frustum::PerspectiveFrustum;
use super::primitive_type::PrimitiveType;
use super::quaternion::Quaternion;
use super::typed_array::TypedArray;
use super::vertex_format::VertexFormat;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Frustum {
    Perspective(PerspectiveFrustum),
    Orthographic(OrthographicFrustum),
}

impl Frustum {
    /// What Cesium's geometries store: `PerspectiveFrustum.clone` does not
    /// copy `xOffset` and `yOffset`, so the copy has none.
    fn cesium_clone(self) -> Self {
        match self {
            Self::Perspective(mut f) => {
                f.x_offset = 0.0;
                f.y_offset = 0.0;
                Self::Perspective(f)
            }
            other => other,
        }
    }

    fn packed_length(&self) -> usize {
        match self {
            Self::Perspective(_) => PerspectiveFrustum::PACKED_LENGTH,
            Self::Orthographic(_) => OrthographicFrustum::PACKED_LENGTH,
        }
    }

    /// Writes the type (0 perspective, 1 orthographic) and the frustum;
    /// returns the next index.
    fn pack(&self, array: &mut [f64], i: usize) -> usize {
        match self {
            Self::Perspective(f) => {
                array[i] = 0.0;
                f.pack(array, i + 1);
            }
            Self::Orthographic(f) => {
                array[i] = 1.0;
                f.pack(array, i + 1);
            }
        }
        i + 1 + self.packed_length()
    }

    fn unpack(array: &[f64], i: usize) -> (Self, usize) {
        let frustum = if array[i] == 0.0 {
            Self::Perspective(PerspectiveFrustum::unpack(array, i + 1))
        } else {
            Self::Orthographic(OrthographicFrustum::unpack(array, i + 1))
        };
        (frustum, i + 1 + frustum.packed_length())
    }
}

/// The frustum's axes from `orientation`, as Cesium leaves them in its
/// scratch variables: x is the negated first column.
fn axes(orientation: &Quaternion) -> (Cartesian3, Cartesian3, Cartesian3) {
    let rotation_matrix = Matrix3::from_quaternion(orientation);
    let x = rotation_matrix.get_column(0).normalize().negate();
    let y = rotation_matrix.get_column(1).normalize();
    let z = rotation_matrix.get_column(2).normalize();
    (x, y, z)
}

const FRUSTUM_CORNERS_NDC: [Cartesian4; 4] = [
    Cartesian4::new(-1.0, -1.0, 1.0, 1.0),
    Cartesian4::new(1.0, -1.0, 1.0, 1.0),
    Cartesian4::new(1.0, 1.0, 1.0, 1.0),
    Cartesian4::new(-1.0, 1.0, 1.0, 1.0),
];

/// Cesium's `FrustumGeometry._computeNearFarPlanes`: the near and far
/// quads, 24 numbers. `None` if the view-projection matrix is singular.
pub fn compute_near_far_planes(origin: Cartesian3, orientation: &Quaternion, frustum: &Frustum) -> Option<[f64; 24]> {
    let (x, y, z) = axes(orientation);
    let view = Matrix4::compute_view(origin, z, y, x);
    let mut positions = [0.0; 24];
    match frustum {
        Frustum::Perspective(f) => {
            let inverse_view_projection = f.projection_matrix().multiply(&view).inverse().ok()?;
            let splits = [f.near, f.far];
            for i in 0..2 {
                for (j, ndc) in FRUSTUM_CORNERS_NDC.iter().enumerate() {
                    let corner = inverse_view_projection.multiply_by_vector(*ndc);
                    let w = 1.0 / corner.w;
                    let c = Cartesian3::new(corner.x, corner.y, corner.z).multiply_by_scalar(w);
                    let c = c.subtract(origin).normalize();
                    let fac = z.dot(c);
                    let c = c.multiply_by_scalar(splits[i] / fac).add(origin);
                    positions[12 * i + j * 3..12 * i + j * 3 + 3].copy_from_slice(&[c.x, c.y, c.z]);
                }
            }
        }
        Frustum::Orthographic(f) => {
            let inverse_view = view.inverse_transformation();
            let off_center = f.off_center_frustum();
            let splits = [0.0, f.near, f.far];
            for i in 0..2 {
                let (near, far) = (splits[i], splits[i + 1]);
                for (j, ndc) in FRUSTUM_CORNERS_NDC.iter().enumerate() {
                    let corner = Cartesian4::new(
                        (ndc.x * (off_center.right - off_center.left) + off_center.left + off_center.right) * 0.5,
                        (ndc.y * (off_center.top - off_center.bottom) + off_center.bottom + off_center.top) * 0.5,
                        (ndc.z * (near - far) - near - far) * 0.5,
                        1.0,
                    );
                    let c = inverse_view.multiply_by_vector(corner);
                    positions[12 * i + j * 3..12 * i + j * 3 + 3].copy_from_slice(&[c.x, c.y, c.z]);
                }
            }
        }
    }
    Some(positions)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrustumGeometry {
    pub frustum: Frustum,
    pub origin: Cartesian3,
    pub orientation: Quaternion,
    pub vertex_format: VertexFormat,
    pub draw_near_plane: bool,
}

impl FrustumGeometry {
    /// A perspective frustum's x and y offsets are dropped, as in Cesium.
    pub fn new(frustum: Frustum, origin: Cartesian3, orientation: Quaternion, vertex_format: VertexFormat) -> Self {
        Self { frustum: frustum.cesium_clone(), origin, orientation, vertex_format, draw_near_plane: true }
    }

    pub fn packed_length(&self) -> usize {
        2 + self.frustum.packed_length() + 3 + 4 + VertexFormat::PACKED_LENGTH
    }

    pub fn pack(&self, array: &mut [f64], i: usize) {
        let mut i = self.frustum.pack(array, i);
        self.origin.pack(array, i);
        i += 3;
        self.orientation.pack(array, i);
        i += 4;
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        array[i] = if self.draw_near_plane { 1.0 } else { 0.0 };
    }

    pub fn unpack(array: &[f64], i: usize) -> Self {
        let (frustum, mut i) = Frustum::unpack(array, i);
        let origin = Cartesian3::unpack(array, i);
        i += 3;
        let orientation = Quaternion::unpack(array, i);
        i += 4;
        let vertex_format = VertexFormat::unpack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        Self { frustum: frustum.cesium_clone(), origin, orientation, vertex_format, draw_near_plane: array[i] == 1.0 }
    }

    /// The faces (the near one optional). Cesium tests whether the vertex
    /// format's flags are defined, not whether they are set, so normals,
    /// tangents, bitangents and texture coordinates are always present.
    /// `None` if the perspective projection cannot be inverted.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let draw_near_plane = self.draw_near_plane;
        let number_of_planes = if draw_near_plane { 6 } else { 5 };
        let planes = compute_near_far_planes(self.origin, &self.orientation, &self.frustum)?;
        let mut positions = vec![0.0; 72];
        positions[..24].copy_from_slice(&planes);
        let corner = |k: usize| [planes[3 * k], planes[3 * k + 1], planes[3 * k + 2]];
        // Side faces, from the near (0–3) and far (4–7) corners.
        let faces = [[4, 0, 3, 7], [5, 1, 0, 4], [1, 5, 6, 2], [2, 6, 7, 3]];
        for (f, face) in faces.iter().enumerate() {
            for (k, &c) in face.iter().enumerate() {
                let o = 24 + 12 * f + 3 * k;
                positions[o..o + 3].copy_from_slice(&corner(c));
            }
        }
        if !draw_near_plane {
            positions.drain(..12);
        }

        let (x, y, z) = axes(&self.orientation);
        let (nx, ny, nz) = (x.negate(), y.negate(), z.negate());
        let mut planes_attributes = Vec::new();
        if draw_near_plane {
            planes_attributes.push((nz, x, y));
        }
        planes_attributes.extend([(z, nx, y), (nx, nz, y), (ny, nz, nx), (x, z, y), (y, z, nx)]);
        let mut normals = Vec::new();
        let mut tangents = Vec::new();
        let mut bitangents = Vec::new();
        let mut st = Vec::new();
        for (normal, tangent, bitangent) in planes_attributes {
            for _ in 0..4 {
                normals.extend([normal.x, normal.y, normal.z]);
                tangents.extend([tangent.x, tangent.y, tangent.z]);
                bitangents.extend([bitangent.x, bitangent.y, bitangent.z]);
            }
            st.extend([0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]);
        }

        let mut indices = Vec::with_capacity(6 * number_of_planes);
        for i in 0..number_of_planes as u16 {
            let index = i * 4;
            indices.extend([index, index + 1, index + 2, index, index + 2, index + 3]);
        }

        let bounding_sphere = BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3);
        let attributes = GeometryAttributes {
            position: Some(GeometryAttribute::doubles(3, positions)),
            normal: Some(GeometryAttribute::floats(3, &normals)),
            tangent: Some(GeometryAttribute::floats(3, &tangents)),
            bitangent: Some(GeometryAttribute::floats(3, &bitangents)),
            st: Some(GeometryAttribute::floats(2, &st)),
            ..Default::default()
        };
        let mut geometry = Geometry::new(attributes, Some(TypedArray::Uint16(indices)), PrimitiveType::Triangles);
        geometry.bounding_sphere = Some(bounding_sphere);
        Some(geometry)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrustumOutlineGeometry {
    pub frustum: Frustum,
    pub origin: Cartesian3,
    pub orientation: Quaternion,
    pub draw_near_plane: bool,
}

impl FrustumOutlineGeometry {
    /// A perspective frustum's x and y offsets are dropped, as in Cesium.
    pub fn new(frustum: Frustum, origin: Cartesian3, orientation: Quaternion) -> Self {
        Self { frustum: frustum.cesium_clone(), origin, orientation, draw_near_plane: true }
    }

    pub fn packed_length(&self) -> usize {
        2 + self.frustum.packed_length() + 3 + 4
    }

    pub fn pack(&self, array: &mut [f64], i: usize) {
        let mut i = self.frustum.pack(array, i);
        self.origin.pack(array, i);
        i += 3;
        self.orientation.pack(array, i);
        i += 4;
        array[i] = if self.draw_near_plane { 1.0 } else { 0.0 };
    }

    pub fn unpack(array: &[f64], i: usize) -> Self {
        let (frustum, mut i) = Frustum::unpack(array, i);
        let origin = Cartesian3::unpack(array, i);
        i += 3;
        let orientation = Quaternion::unpack(array, i);
        i += 4;
        Self { frustum: frustum.cesium_clone(), origin, orientation, draw_near_plane: array[i] == 1.0 }
    }

    /// The edges; `None` if the perspective projection cannot be inverted.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let positions = compute_near_far_planes(self.origin, &self.orientation, &self.frustum)?.to_vec();
        let draw_near_plane = self.draw_near_plane;
        let number_of_planes = if draw_near_plane { 2 } else { 1 };
        // Cesium's index array is one plane's worth too short for the
        // connecting edges it writes; JavaScript drops the writes past the
        // end, so the last four edges are missing there too.
        let mut indices = vec![0u16; 8 * (number_of_planes + 1)];
        let mut write = |offset: usize, values: [u16; 8]| {
            for (k, v) in values.into_iter().enumerate() {
                if let Some(slot) = indices.get_mut(offset + k) {
                    *slot = v;
                }
            }
        };
        for i in (if draw_near_plane { 0 } else { 1 })..2 {
            let offset = if draw_near_plane { i * 8 } else { 0 };
            let index = (i * 4) as u16;
            write(offset, [index, index + 1, index + 1, index + 2, index + 2, index + 3, index + 3, index]);
        }
        for i in 0..2 {
            let offset = (number_of_planes + i) * 8;
            let index = (i * 4) as u16;
            write(offset, [index, index + 4, index + 1, index + 5, index + 2, index + 6, index + 3, index + 7]);
        }
        let bounding_sphere = BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3);
        let attributes =
            GeometryAttributes { position: Some(GeometryAttribute::doubles(3, positions)), ..Default::default() };
        let mut geometry = Geometry::new(attributes, Some(TypedArray::Uint16(indices)), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(bounding_sphere);
        Some(geometry)
    }
}
