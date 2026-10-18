//! CesiumJS `HeightmapTessellator`: height samples to encoded terrain
//! vertices, including edge skirts, bounds and horizon-culling metadata.

use super::axis_aligned_bounding_box::AxisAlignedBoundingBox;
use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::ellipsoidal_occluder::EllipsoidalOccluder;
use super::js_math::JsMath;
use super::math::{clamp, js_max, js_min, to_radians, PI_OVER_TWO};
use super::oriented_bounding_box::OrientedBoundingBox;
use super::rectangle::Rectangle;
use super::terrain_encoding::{TerrainEncoding, TerrainEncodingOptions, TerrainVertex};
use super::transforms::Transforms;
use super::typed_array::TypedArray;
use super::web_mercator_projection::WebMercatorProjection;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightmapStructure {
    pub height_scale: f64,
    pub height_offset: f64,
    pub elements_per_height: usize,
    pub stride: usize,
    pub element_multiplier: f64,
    pub is_big_endian: bool,
}

impl HeightmapStructure {
    pub const DEFAULT: Self = Self {
        height_scale: 1.0,
        height_offset: 0.0,
        elements_per_height: 1,
        stride: 1,
        element_multiplier: 256.0,
        is_big_endian: false,
    };
}

impl Default for HeightmapStructure {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Debug)]
pub struct HeightmapTessellatorOptions<'a> {
    pub heightmap: &'a TypedArray,
    pub width: usize,
    pub height: usize,
    pub skirt_height: f64,
    /// Degrees for geographic data, meters for Web Mercator data.
    pub native_rectangle: Rectangle,
    pub rectangle: Option<Rectangle>,
    pub is_geographic: bool,
    pub relative_to_center: Option<Cartesian3>,
    pub ellipsoid: Ellipsoid,
    pub include_web_mercator_t: bool,
    pub exaggeration: f64,
    pub exaggeration_relative_height: f64,
    pub structure: HeightmapStructure,
}

impl<'a> HeightmapTessellatorOptions<'a> {
    pub fn new(
        heightmap: &'a TypedArray,
        width: usize,
        height: usize,
        native_rectangle: Rectangle,
        skirt_height: f64,
    ) -> Self {
        Self {
            heightmap,
            width,
            height,
            native_rectangle,
            skirt_height,
            rectangle: None,
            is_geographic: true,
            relative_to_center: None,
            ellipsoid: Ellipsoid::WGS84,
            include_web_mercator_t: false,
            exaggeration: 1.0,
            exaggeration_relative_height: 0.0,
            structure: HeightmapStructure::DEFAULT,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeightmapVertices {
    pub vertices: Vec<f32>,
    /// The surface sample range, excluding skirt lowering. Cesium starts
    /// these extrema at 65536 and -65536; those sentinel limits are retained.
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub encoding: TerrainEncoding,
    pub bounding_sphere_3d: BoundingSphere,
    pub oriented_bounding_box: Option<OrientedBoundingBox>,
    pub occludee_point_in_scaled_space: Option<Cartesian3>,
}

/// Returns None for undersized grids/buffers, invalid sample layouts, or
/// unsupported bounds. Input height samples are never changed.
pub fn compute_vertices(options: &HeightmapTessellatorOptions<'_>) -> Option<HeightmapVertices> {
    let (width, height) = (options.width, options.height);
    let structure = options.structure;
    if width < 2 || height < 2 || structure.stride == 0 || structure.elements_per_height == 0 {
        return None;
    }
    let grid_count = width.checked_mul(height)?;
    let last = (grid_count - 1).checked_mul(structure.stride)?.checked_add(structure.elements_per_height)?;
    if last > options.heightmap.len() {
        return None;
    }
    let skirts = options.skirt_height > 0.0;
    let edge_count = if skirts { width.checked_add(height)?.checked_mul(2)? } else { 0 };
    let count = grid_count.checked_add(edge_count)?;
    let native = options.native_rectangle;
    let ellipsoid = &options.ellipsoid;
    let inverse_radius = 1.0 / ellipsoid.maximum_radius;
    let latitude_of = |v: f64| {
        if options.is_geographic {
            to_radians(v)
        } else {
            PI_OVER_TWO - 2.0 * (-v * inverse_radius).js_exp().js_atan()
        }
    };
    let longitude_of = |v: f64| if options.is_geographic { to_radians(v) } else { v * inverse_radius };
    let geographic = options.rectangle.unwrap_or_else(|| {
        Rectangle::new(
            longitude_of(native.west),
            latitude_of(native.south),
            longitude_of(native.east),
            latitude_of(native.north),
        )
    });
    let center = options.relative_to_center.unwrap_or(Cartesian3::ZERO);
    let from_enu = Transforms::east_north_up_to_fixed_frame(center, ellipsoid);
    let to_enu = from_enu.inverse_transformation();
    let mercator_angle = WebMercatorProjection::geodetic_latitude_to_mercator_angle;
    let south_mercator = mercator_angle(geographic.south);
    let inverse_mercator_height = 1.0 / (mercator_angle(geographic.north) - south_mercator);
    let granularity_x = native.width() / (width - 1) as f64;
    let granularity_y = native.height() / (height - 1) as f64;
    let rectangle_width = native.width() * if options.is_geographic { 1.0 } else { inverse_radius };
    let rectangle_height = native.height() * if options.is_geographic { 1.0 } else { inverse_radius };
    let mut minimum = Cartesian3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut maximum = Cartesian3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut min_height = 65536.0;
    let mut max_height = -65536.0;
    let mut h_min = f64::INFINITY;
    let mut positions = vec![Cartesian3::ZERO; count];
    let mut heights = vec![0.0; count];
    let mut uvs = vec![Cartesian2::ZERO; count];
    let mut mercator_ts = options.include_web_mercator_t.then(|| vec![0.0; count]);
    let mut normals = (options.exaggeration != 1.0).then(|| vec![Cartesian3::ZERO; count]);
    let skirt = isize::from(skirts);
    let width_i = isize::try_from(width).ok()?;
    let height_i = isize::try_from(height).ok()?;
    let end_row = height_i.checked_add(skirt)?;
    let end_col = width_i.checked_add(skirt)?;
    for row_index in -skirt..end_row {
        let row = row_index.clamp(0, height_i - 1) as usize;
        let mut latitude = latitude_of(native.north - granularity_y * row as f64);
        let v = clamp((latitude - geographic.south) / (geographic.north - geographic.south), 0.0, 1.0);
        let north_edge = row_index == -skirt;
        let south_edge = row_index == end_row - 1;
        if skirts {
            if north_edge {
                latitude += 0.00001 * rectangle_height;
            } else if south_edge {
                latitude -= 0.00001 * rectangle_height;
            }
        }
        let cos_latitude = latitude.js_cos();
        let nz = latitude.js_sin();
        let kz = ellipsoid.radii_squared.z * nz;
        let mercator_t = (mercator_angle(latitude) - south_mercator) * inverse_mercator_height;
        for col_index in -skirt..end_col {
            let col = col_index.clamp(0, width_i - 1) as usize;
            let sample = (row * width + col) * structure.stride;
            let mut h = if structure.elements_per_height == 1 {
                options.heightmap.get(sample)?
            } else {
                let mut h = 0.0;
                for j in 0..structure.elements_per_height {
                    let offset = if structure.is_big_endian { j } else { structure.elements_per_height - 1 - j };
                    h = h * structure.element_multiplier + options.heightmap.get(sample + offset)?;
                }
                h
            };
            h = h * structure.height_scale + structure.height_offset;
            max_height = js_max(max_height, h);
            min_height = js_min(min_height, h);
            let mut longitude = longitude_of(native.west + granularity_x * col as f64);
            let u = clamp((longitude - geographic.west) / (geographic.east - geographic.west), 0.0, 1.0);
            let mut index = row * width + col;
            if skirts {
                let west_edge = col_index == -skirt;
                let east_edge = col_index == end_col - 1;
                if (north_edge || south_edge) && (west_edge || east_edge) {
                    continue;
                }
                if north_edge || south_edge || west_edge || east_edge {
                    h -= options.skirt_height;
                    if west_edge {
                        index = grid_count + height - row - 1;
                        longitude -= 0.00001 * rectangle_width;
                    } else if south_edge {
                        index = grid_count + height + width - col - 1;
                    } else if east_edge {
                        index = grid_count + height + width + row;
                        longitude += 0.00001 * rectangle_width;
                    } else {
                        index = grid_count + height * 2 + width + col;
                    }
                }
            }
            let nx = cos_latitude * longitude.js_cos();
            let ny = cos_latitude * longitude.js_sin();
            let kx = ellipsoid.radii_squared.x * nx;
            let ky = ellipsoid.radii_squared.y * ny;
            let inverse_gamma = 1.0 / (kx * nx + ky * ny + kz * nz).sqrt();
            let p =
                Cartesian3::new(kx * inverse_gamma + nx * h, ky * inverse_gamma + ny * h, kz * inverse_gamma + nz * h);
            let local = to_enu.multiply_by_point(p);
            minimum = local.minimum_by_component(minimum);
            maximum = local.maximum_by_component(maximum);
            h_min = js_min(h_min, h);
            positions[index] = p;
            heights[index] = h;
            uvs[index] = Cartesian2::new(u, v);
            if let Some(values) = &mut mercator_ts {
                values[index] = mercator_t;
            }
            if let Some(values) = &mut normals {
                values[index] = ellipsoid.geodetic_surface_normal(p)?;
            }
        }
    }
    let bounding_sphere_3d = BoundingSphere::from_points(&positions);
    let oriented_bounding_box = match options.rectangle {
        Some(r) => Some(OrientedBoundingBox::from_rectangle(&r, min_height, max_height, ellipsoid)?),
        None => None,
    };
    let occludee_point_in_scaled_space = options.relative_to_center.and_then(|center| {
        EllipsoidalOccluder::new(ellipsoid, None).compute_horizon_culling_point_possibly_under_ellipsoid(
            center,
            &positions,
            Some(min_height),
        )
    });
    let mut encoding_options = TerrainEncodingOptions::new(center);
    encoding_options.axis_aligned_bounding_box = Some(AxisAlignedBoundingBox::new(minimum, maximum, Some(center)));
    encoding_options.minimum_height = Some(h_min);
    encoding_options.maximum_height = Some(max_height);
    encoding_options.from_enu = Some(from_enu);
    encoding_options.has_web_mercator_t = options.include_web_mercator_t;
    encoding_options.has_geodetic_surface_normals = options.exaggeration != 1.0;
    encoding_options.exaggeration = options.exaggeration;
    encoding_options.exaggeration_relative_height = options.exaggeration_relative_height;
    let encoding = TerrainEncoding::new(encoding_options)?;
    let mut vertices = vec![0.0; count.checked_mul(encoding.stride)?];
    let mut buffer_index = 0;
    for i in 0..count {
        let mut vertex = TerrainVertex::new(positions[i], uvs[i], heights[i]);
        vertex.web_mercator_t = mercator_ts.as_ref().map(|values| values[i]);
        vertex.geodetic_surface_normal = normals.as_ref().map(|values| values[i]);
        buffer_index = encoding.encode(&mut vertices, buffer_index, &vertex)?;
    }
    Some(HeightmapVertices {
        vertices,
        minimum_height: min_height,
        maximum_height: max_height,
        encoding,
        bounding_sphere_3d,
        oriented_bounding_box,
        occludee_point_in_scaled_space,
    })
}
