//! CesiumJS `EllipsoidGeometry`: an ellipsoid, or a clock/cone section of
//! one, optionally hollow with an inner surface.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::index_datatype::IndexDatatype;
use super::math::{js_round, PI, TWO_PI};
use super::primitive_type::PrimitiveType;
use super::vertex_format::VertexFormat;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipsoidGeometry {
    pub radii: Cartesian3,
    pub inner_radii: Cartesian3,
    pub minimum_clock: f64,
    pub maximum_clock: f64,
    pub minimum_cone: f64,
    pub maximum_cone: f64,
    pub stack_partitions: f64,
    pub slice_partitions: f64,
    pub vertex_format: VertexFormat,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl Default for EllipsoidGeometry {
    /// A unit sphere with 64 × 64 partitions.
    fn default() -> Self {
        Self::new(Cartesian3::new(1.0, 1.0, 1.0), None, 0.0, TWO_PI, 0.0, PI, 64.0, 64.0, VertexFormat::DEFAULT, None)
            .expect("the defaults are valid")
    }
}

/// Whether a JavaScript number is truthy: not 0 and not NaN.
pub(crate) fn truthy(value: f64) -> bool {
    value != 0.0 && !value.is_nan()
}

impl EllipsoidGeometry {
    pub const PACKED_LENGTH: usize = 2 * 3 + VertexFormat::PACKED_LENGTH + 7;

    /// Partitions are rounded; `None` with fewer than three of either (Cesium
    /// throws). `inner_radii` defaults to `radii`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        radii: Cartesian3,
        inner_radii: Option<Cartesian3>,
        minimum_clock: f64,
        maximum_clock: f64,
        minimum_cone: f64,
        maximum_cone: f64,
        stack_partitions: f64,
        slice_partitions: f64,
        vertex_format: VertexFormat,
        offset_attribute: Option<GeometryOffsetAttribute>,
    ) -> Option<Self> {
        let stack_partitions = js_round(stack_partitions);
        let slice_partitions = js_round(slice_partitions);
        if slice_partitions < 3.0 || stack_partitions < 3.0 {
            return None;
        }
        Some(Self {
            radii,
            inner_radii: inner_radii.unwrap_or(radii),
            minimum_clock,
            maximum_clock,
            minimum_cone,
            maximum_cone,
            stack_partitions,
            slice_partitions,
            vertex_format,
            offset_attribute,
        })
    }

    pub fn pack(&self, array: &mut [f64], mut i: usize) {
        self.radii.pack(array, i);
        i += 3;
        self.inner_radii.pack(array, i);
        i += 3;
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        for v in [
            self.minimum_clock,
            self.maximum_clock,
            self.minimum_cone,
            self.maximum_cone,
            self.stack_partitions,
            self.slice_partitions,
            GeometryOffsetAttribute::pack(self.offset_attribute),
        ] {
            array[i] = v;
            i += 1;
        }
    }

    /// Like Cesium's `unpack` without a result: the values go through the constructor.
    pub fn unpack(array: &[f64], mut i: usize) -> Option<Self> {
        let radii = Cartesian3::unpack(array, i);
        i += 3;
        let inner_radii = Cartesian3::unpack(array, i);
        i += 3;
        let vertex_format = VertexFormat::unpack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        let a = &array[i..i + 7];
        Self::new(
            radii,
            Some(inner_radii),
            a[0],
            a[1],
            a[2],
            a[3],
            a[4],
            a[5],
            vertex_format,
            GeometryOffsetAttribute::unpack(a[6]),
        )
    }

    /// The triangles; `None` if any radius is not positive.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let radii = self.radii;
        let inner_radii = self.inner_radii;
        if radii.x <= 0.0 || radii.y <= 0.0 || radii.z <= 0.0 {
            return None;
        }
        if inner_radii.x <= 0.0 || inner_radii.y <= 0.0 || inner_radii.z <= 0.0 {
            return None;
        }
        let (minimum_clock, maximum_clock) = (self.minimum_clock, self.maximum_clock);
        let (minimum_cone, maximum_cone) = (self.minimum_cone, self.maximum_cone);
        let vertex_format = self.vertex_format;

        let mut slice_partitions = self.slice_partitions + 1.0;
        let mut stack_partitions = self.stack_partitions + 1.0;
        slice_partitions = js_round((slice_partitions * (maximum_clock - minimum_clock).abs()) / TWO_PI);
        stack_partitions = js_round((stack_partitions * (maximum_cone - minimum_cone).abs()) / PI);
        if slice_partitions < 2.0 {
            slice_partitions = 2.0;
        }
        if stack_partitions < 2.0 {
            stack_partitions = 2.0;
        }
        let (slice_partitions, stack_partitions) = (slice_partitions as usize, stack_partitions as usize);

        let mut phis = vec![minimum_cone];
        let mut thetas = vec![minimum_clock];
        for i in 0..stack_partitions {
            phis.push(minimum_cone + (i as f64 * (maximum_cone - minimum_cone)) / (stack_partitions as f64 - 1.0));
        }
        phis.push(maximum_cone);
        for j in 0..slice_partitions {
            thetas.push(minimum_clock + (j as f64 * (maximum_clock - minimum_clock)) / (slice_partitions as f64 - 1.0));
        }
        thetas.push(maximum_clock);
        let num_phis = phis.len();
        let num_thetas = thetas.len();

        let mut extra_indices = 0usize;
        let mut vertex_multiplier = 1usize;
        let has_inner_surface = inner_radii != radii;
        let mut is_top_open = false;
        let mut is_bot_open = false;
        let mut is_clock_open = false;
        if has_inner_surface {
            vertex_multiplier = 2;
            if minimum_cone > 0.0 {
                is_top_open = true;
                extra_indices += slice_partitions - 1;
            }
            if maximum_cone < PI {
                is_bot_open = true;
                extra_indices += slice_partitions - 1;
            }
            if truthy((maximum_clock - minimum_clock) % TWO_PI) {
                is_clock_open = true;
                extra_indices += (stack_partitions - 1) * 2 + 1;
            } else {
                extra_indices += 1;
            }
        }

        let vertex_count = num_thetas * num_phis * vertex_multiplier;
        let mut positions = vec![0.0; vertex_count * 3];
        let mut is_inner = vec![false; vertex_count];
        let mut negate_normal = vec![false; vertex_count];
        let index_count = slice_partitions * stack_partitions * vertex_multiplier;
        let num_indices = 6
            * (index_count as isize + extra_indices as isize + 1
                - ((slice_partitions + stack_partitions) * vertex_multiplier) as isize);

        let sin_phi: Vec<f64> = phis.iter().map(|p| p.sin()).collect();
        let cos_phi: Vec<f64> = phis.iter().map(|p| p.cos()).collect();
        let cos_theta: Vec<f64> = thetas.iter().map(|t| t.cos()).collect();
        let sin_theta: Vec<f64> = thetas.iter().map(|t| t.sin()).collect();

        let mut index = 0;
        for i in 0..num_phis {
            for j in 0..num_thetas {
                positions[index] = radii.x * sin_phi[i] * cos_theta[j];
                positions[index + 1] = radii.y * sin_phi[i] * sin_theta[j];
                positions[index + 2] = radii.z * cos_phi[i];
                index += 3;
            }
        }
        let mut vertex_index = vertex_count / 2;
        if has_inner_surface {
            for i in 0..num_phis {
                for j in 0..num_thetas {
                    positions[index] = inner_radii.x * sin_phi[i] * cos_theta[j];
                    positions[index + 1] = inner_radii.y * sin_phi[i] * sin_theta[j];
                    positions[index + 2] = inner_radii.z * cos_phi[i];
                    index += 3;
                    is_inner[vertex_index] = true;
                    if i > 0 && i != num_phis - 1 && j != 0 && j != num_thetas - 1 {
                        negate_normal[vertex_index] = true;
                    }
                    vertex_index += 1;
                }
            }
        }

        let mut indices: Vec<u32> = Vec::new();
        let mut push = |values: [usize; 6]| indices.extend(values.iter().map(|&v| v as u32));
        for i in 1..num_phis.saturating_sub(2) {
            let top_offset = i * num_thetas;
            let bottom_offset = (i + 1) * num_thetas;
            for j in 1..num_thetas.saturating_sub(2) {
                push([
                    bottom_offset + j,
                    bottom_offset + j + 1,
                    top_offset + j + 1,
                    bottom_offset + j,
                    top_offset + j + 1,
                    top_offset + j,
                ]);
            }
        }
        if has_inner_surface {
            let offset = num_phis * num_thetas;
            for i in 1..num_phis.saturating_sub(2) {
                let top_offset = offset + i * num_thetas;
                let bottom_offset = offset + (i + 1) * num_thetas;
                for j in 1..num_thetas.saturating_sub(2) {
                    push([
                        bottom_offset + j,
                        top_offset + j,
                        top_offset + j + 1,
                        bottom_offset + j,
                        top_offset + j + 1,
                        bottom_offset + j + 1,
                    ]);
                }
            }
            if is_top_open {
                let inner_offset = num_phis * num_thetas;
                for i in 1..num_thetas.saturating_sub(2) {
                    push([i, i + 1, inner_offset + i + 1, i, inner_offset + i + 1, inner_offset + i]);
                }
            }
            if is_bot_open {
                let outer_offset = num_phis * num_thetas - num_thetas;
                let inner_offset = num_phis * num_thetas * vertex_multiplier - num_thetas;
                for i in 1..num_thetas.saturating_sub(2) {
                    push([
                        outer_offset + i + 1,
                        outer_offset + i,
                        inner_offset + i,
                        outer_offset + i + 1,
                        inner_offset + i,
                        inner_offset + i + 1,
                    ]);
                }
            }
        }
        if is_clock_open {
            for i in 1..num_phis.saturating_sub(2) {
                let inner_offset = num_thetas * num_phis + num_thetas * i;
                let outer_offset = num_thetas * i;
                push([
                    inner_offset,
                    outer_offset + num_thetas,
                    outer_offset,
                    inner_offset,
                    inner_offset + num_thetas,
                    outer_offset + num_thetas,
                ]);
            }
            for i in 1..num_phis.saturating_sub(2) {
                let inner_offset = num_thetas * num_phis + num_thetas * (i + 1) - 1;
                let outer_offset = num_thetas * (i + 1) - 1;
                push([
                    outer_offset + num_thetas,
                    inner_offset,
                    outer_offset,
                    outer_offset + num_thetas,
                    inner_offset + num_thetas,
                    inner_offset,
                ]);
            }
        }
        // Cesium sizes the index array from these counts, but picks its width
        // from `index_count`, which can be under 64K while the vertex count is
        // not: past 254 partitions its 16-bit indices wrap. The width here
        // comes from the vertex count.
        indices.resize(num_indices.max(0) as usize, 0);
        let indices = IndexDatatype::create_typed_array(vertex_count, &indices);

        let mut attributes = GeometryAttributes::default();
        if vertex_format.position {
            attributes.position = Some(GeometryAttribute::doubles(3, positions.clone()));
        }

        let vertex_count_half = vertex_count / 2;
        let ellipsoid_outer = Ellipsoid::new(radii.x, radii.y, radii.z);
        let ellipsoid_inner = Ellipsoid::new(inner_radii.x, inner_radii.y, inner_radii.z);
        if vertex_format.st || vertex_format.normal || vertex_format.tangent || vertex_format.bitangent {
            let mut st = Vec::new();
            let mut normals = Vec::new();
            let mut tangents = Vec::new();
            let mut bitangents = Vec::new();
            for i in 0..vertex_count {
                let ellipsoid = if is_inner[i] { &ellipsoid_inner } else { &ellipsoid_outer };
                let position = Cartesian3::from_array(&positions, i * 3);
                let mut normal = ellipsoid.geodetic_surface_normal(position).unwrap_or_default();
                if negate_normal[i] {
                    normal = normal.negate();
                }
                if vertex_format.st {
                    let (nx, ny) = (-normal.x, -normal.y);
                    st.push(ny.atan2(nx) / TWO_PI + 0.5);
                    st.push(normal.z.asin() / PI + 0.5);
                }
                if vertex_format.normal {
                    normals.extend([normal.x, normal.y, normal.z]);
                }
                if vertex_format.tangent || vertex_format.bitangent {
                    let tangent_offset = if is_inner[i] { vertex_count_half } else { 0 };
                    let unit = if !is_top_open && i >= tangent_offset && i < tangent_offset + num_thetas * 2 {
                        Cartesian3::UNIT_X
                    } else {
                        Cartesian3::UNIT_Z
                    };
                    let tangent = unit.cross(normal).normalize();
                    if vertex_format.tangent {
                        tangents.extend([tangent.x, tangent.y, tangent.z]);
                    }
                    if vertex_format.bitangent {
                        let bitangent = normal.cross(tangent).normalize();
                        bitangents.extend([bitangent.x, bitangent.y, bitangent.z]);
                    }
                }
            }
            if vertex_format.st {
                attributes.st = Some(GeometryAttribute::floats(2, &st));
            }
            if vertex_format.normal {
                attributes.normal = Some(GeometryAttribute::floats(3, &normals));
            }
            if vertex_format.tangent {
                attributes.tangent = Some(GeometryAttribute::floats(3, &tangents));
            }
            if vertex_format.bitangent {
                attributes.bitangent = Some(GeometryAttribute::floats(3, &bitangents));
            }
        }

        if let Some(offset) = self.offset_attribute {
            attributes.set_other("applyOffset", GeometryAttribute::apply_offset(positions.len() / 3, offset));
        }
        let mut geometry = Geometry::new(attributes, Some(indices), PrimitiveType::Triangles);
        geometry.bounding_sphere = Some(BoundingSphere::from_ellipsoid(&ellipsoid_outer));
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }

    /// A position-only unit sphere.
    pub fn get_unit_ellipsoid() -> Geometry {
        let e = Self { vertex_format: VertexFormat::POSITION_ONLY, ..Self::default() };
        e.create_geometry().expect("the unit ellipsoid is valid")
    }
}
