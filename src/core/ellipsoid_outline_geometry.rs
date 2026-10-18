//! CesiumJS `EllipsoidOutlineGeometry`: latitude and longitude lines on an
//! ellipsoid or a clock/cone section of one.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::index_datatype::IndexDatatype;
use super::js_math::JsMath;
use super::math::{js_round, PI, TWO_PI};
use super::primitive_type::PrimitiveType;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipsoidOutlineGeometry {
    pub radii: Cartesian3,
    pub inner_radii: Cartesian3,
    pub minimum_clock: f64,
    pub maximum_clock: f64,
    pub minimum_cone: f64,
    pub maximum_cone: f64,
    pub stack_partitions: f64,
    pub slice_partitions: f64,
    pub subdivisions: f64,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl Default for EllipsoidOutlineGeometry {
    /// A unit sphere with 10 stacks, 8 slices, and 128 subdivisions.
    fn default() -> Self {
        Self::new(Cartesian3::new(1.0, 1.0, 1.0), None, 0.0, TWO_PI, 0.0, PI, 10.0, 8.0, 128.0, None)
            .expect("the defaults are valid")
    }
}

impl EllipsoidOutlineGeometry {
    pub const PACKED_LENGTH: usize = 2 * 3 + 8;

    /// Partitions and subdivisions are rounded. `None` for fewer than one
    /// stack, negative slices or subdivisions, or a
    /// [`GeometryOffsetAttribute::Top`] offset (Cesium throws).
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
        subdivisions: f64,
        offset_attribute: Option<GeometryOffsetAttribute>,
    ) -> Option<Self> {
        let stack_partitions = js_round(stack_partitions);
        let slice_partitions = js_round(slice_partitions);
        let subdivisions = js_round(subdivisions);
        if stack_partitions < 1.0
            || slice_partitions < 0.0
            || subdivisions < 0.0
            || offset_attribute == Some(GeometryOffsetAttribute::Top)
        {
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
            subdivisions,
            offset_attribute,
        })
    }

    pub fn pack(&self, array: &mut [f64], mut i: usize) {
        self.radii.pack(array, i);
        i += 3;
        self.inner_radii.pack(array, i);
        i += 3;
        for v in [
            self.minimum_clock,
            self.maximum_clock,
            self.minimum_cone,
            self.maximum_cone,
            self.stack_partitions,
            self.slice_partitions,
            self.subdivisions,
            GeometryOffsetAttribute::pack(self.offset_attribute),
        ] {
            array[i] = v;
            i += 1;
        }
    }

    pub fn unpack(array: &[f64], i: usize) -> Option<Self> {
        let radii = Cartesian3::unpack(array, i);
        let inner_radii = Cartesian3::unpack(array, i + 3);
        let a = &array[i + 6..i + 14];
        Self::new(
            radii,
            Some(inner_radii),
            a[0],
            a[1],
            a[2],
            a[3],
            a[4],
            a[5],
            a[6],
            GeometryOffsetAttribute::unpack(a[7]),
        )
    }

    /// The lines; `None` if any radius is not positive.
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
        let subdivisions = self.subdivisions as usize;
        let ellipsoid = Ellipsoid::new(radii.x, radii.y, radii.z);

        let mut slice_partitions =
            js_round(((self.slice_partitions + 1.0) * (maximum_clock - minimum_clock).abs()) / TWO_PI);
        let mut stack_partitions = js_round(((self.stack_partitions + 1.0) * (maximum_cone - minimum_cone).abs()) / PI);
        if slice_partitions < 2.0 {
            slice_partitions = 2.0;
        }
        if stack_partitions < 2.0 {
            stack_partitions = 2.0;
        }
        let (slice_partitions, stack_partitions) = (slice_partitions as usize, stack_partitions as usize);

        let mut extra_indices = 0;
        let mut vertex_multiplier = 1;
        let has_inner_surface = inner_radii != radii;
        let mut is_top_open = false;
        let mut is_bot_open = false;
        if has_inner_surface {
            vertex_multiplier = 2;
            if minimum_cone > 0.0 {
                is_top_open = true;
                extra_indices += slice_partitions;
            }
            if maximum_cone < PI {
                is_bot_open = true;
                extra_indices += slice_partitions;
            }
        }

        let vertex_count = subdivisions * vertex_multiplier * (stack_partitions + slice_partitions);
        let num_indices = 2
            * (vertex_count as isize + extra_indices as isize
                - ((slice_partitions + stack_partitions) * vertex_multiplier) as isize);
        let mut positions = Vec::with_capacity(vertex_count * 3);

        let angle = |min: f64, max: f64, i: usize, n: usize| min + (i as f64 * (max - min)) / (n as f64 - 1.0);
        let push_ring = |positions: &mut Vec<f64>,
                         r: Cartesian3,
                         sin_phi: &[f64],
                         cos_phi: &[f64],
                         sin_theta: &[f64],
                         cos_theta: &[f64]| {
            for i in 0..sin_phi.len() {
                for j in 0..sin_theta.len() {
                    positions.extend([
                        r.x * sin_phi[i] * cos_theta[j],
                        r.y * sin_phi[i] * sin_theta[j],
                        r.z * cos_phi[i],
                    ]);
                }
            }
        };

        // Lines of latitude.
        let phis: Vec<f64> =
            (0..stack_partitions).map(|i| angle(minimum_cone, maximum_cone, i, stack_partitions)).collect();
        let thetas: Vec<f64> =
            (0..subdivisions).map(|i| angle(minimum_clock, maximum_clock, i, subdivisions)).collect();
        let (sin_phi, cos_phi): (Vec<f64>, Vec<f64>) = phis.iter().map(|p| (p.js_sin(), p.js_cos())).unzip();
        let (sin_theta, cos_theta): (Vec<f64>, Vec<f64>) = thetas.iter().map(|t| (t.js_sin(), t.js_cos())).unzip();
        push_ring(&mut positions, radii, &sin_phi, &cos_phi, &sin_theta, &cos_theta);
        if has_inner_surface {
            push_ring(&mut positions, inner_radii, &sin_phi, &cos_phi, &sin_theta, &cos_theta);
        }

        // Lines of longitude.
        let phis: Vec<f64> = (0..subdivisions).map(|i| angle(minimum_cone, maximum_cone, i, subdivisions)).collect();
        let thetas: Vec<f64> =
            (0..slice_partitions).map(|i| angle(minimum_clock, maximum_clock, i, slice_partitions)).collect();
        let (sin_phi, cos_phi): (Vec<f64>, Vec<f64>) = phis.iter().map(|p| (p.js_sin(), p.js_cos())).unzip();
        let (sin_theta, cos_theta): (Vec<f64>, Vec<f64>) = thetas.iter().map(|t| (t.js_sin(), t.js_cos())).unzip();
        push_ring(&mut positions, radii, &sin_phi, &cos_phi, &sin_theta, &cos_theta);
        if has_inner_surface {
            push_ring(&mut positions, inner_radii, &sin_phi, &cos_phi, &sin_theta, &cos_theta);
        }

        let mut indices: Vec<u32> = Vec::new();
        for i in 0..stack_partitions * vertex_multiplier {
            let top_offset = i * subdivisions;
            for j in 0..subdivisions.saturating_sub(1) {
                indices.extend([(top_offset + j) as u32, (top_offset + j + 1) as u32]);
            }
        }
        let mut offset = stack_partitions * subdivisions * vertex_multiplier;
        let longitude_lines = |indices: &mut Vec<u32>, offset: usize| {
            for i in 0..slice_partitions {
                for j in 0..subdivisions.saturating_sub(1) {
                    indices.extend([
                        (offset + i + j * slice_partitions) as u32,
                        (offset + i + (j + 1) * slice_partitions) as u32,
                    ]);
                }
            }
        };
        longitude_lines(&mut indices, offset);
        if has_inner_surface {
            offset = stack_partitions * subdivisions * vertex_multiplier + slice_partitions * subdivisions;
            longitude_lines(&mut indices, offset);
            let mut outer_offset = stack_partitions * subdivisions * vertex_multiplier;
            let mut inner_offset = outer_offset + subdivisions * slice_partitions;
            if is_top_open {
                for i in 0..slice_partitions {
                    indices.extend([(outer_offset + i) as u32, (inner_offset + i) as u32]);
                }
            }
            if is_bot_open {
                outer_offset += subdivisions * slice_partitions - slice_partitions;
                inner_offset += subdivisions * slice_partitions - slice_partitions;
                for i in 0..slice_partitions {
                    indices.extend([(outer_offset + i) as u32, (inner_offset + i) as u32]);
                }
            }
        }
        indices.resize(num_indices.max(0) as usize, 0);
        let indices = IndexDatatype::create_typed_array(vertex_count, &indices);

        positions.resize(vertex_count * 3, 0.0);
        let mut attributes = GeometryAttributes::default();
        if let Some(offset) = self.offset_attribute {
            attributes.set_other("applyOffset", GeometryAttribute::apply_offset(positions.len() / 3, offset));
        }
        attributes.position = Some(GeometryAttribute::doubles(3, positions));
        let mut geometry = Geometry::new(attributes, Some(indices), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(BoundingSphere::from_ellipsoid(&ellipsoid));
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }
}
