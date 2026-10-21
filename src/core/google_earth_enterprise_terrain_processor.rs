//! CesiumJS `Workers/createVerticesFromGoogleEarthEnterpriseBuffer`: four-quadrant
//! GEE terrain tiles, shared quad-border vertices, skirts and terrain encoding.
//!
//! Reads follow the worker's little-endian `DataView` offsets. A read past the
//! buffer, a negative total point or face count and a quad whose stored size does
//! not match its contents return errors, as the worker throws. Face indices that
//! point past a quad's points become 0, as `undefined` does in a `Uint16Array`.
use super::math::{self, js_max, js_min};
use super::terrain_encoding::{TerrainEncodingOptions, TerrainVertex};
use super::terrain_provider::TerrainEdgeIndices;
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct GoogleEarthEnterpriseBufferRequest<'a> {
    pub buffer: &'a [u8],
    pub relative_to_center: Cartesian3,
    pub ellipsoid: Ellipsoid,
    /// Cesium falls back to the native rectangle only on a path that then throws
    /// on the missing geographic rectangle, so the geographic rectangle is required.
    pub rectangle: Rectangle,
    pub exaggeration: f64,
    pub exaggeration_relative_height: f64,
    pub skirt_height: f64,
    pub include_web_mercator_t: bool,
    pub negative_altitude_exponent_bias: f64,
    pub negative_elevation_threshold: f64,
}

/// The worker's result, with owned buffers in place of transferred `ArrayBuffer`s.
#[derive(Clone, Debug, PartialEq)]
pub struct GoogleEarthEnterpriseBufferResult {
    pub vertices: Vec<f32>,
    pub indices: Vec<u16>,
    /// Cesium's `numberOfAttributes`: the encoding's stride.
    pub number_of_attributes: usize,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub bounding_sphere_3d: BoundingSphere,
    pub oriented_bounding_box: Option<OrientedBoundingBox>,
    pub occludee_point_in_scaled_space: Option<Cartesian3>,
    pub encoding: TerrainEncoding,
    pub vertex_count_without_skirts: usize,
    pub index_count_without_skirts: usize,
    pub edges: TerrainEdgeIndices,
}

const EPSILON12: f64 = 1e-12;

fn index_of_epsilon(values: &[f64], value: f64) -> Option<usize> {
    values.iter().position(|&v| math::equals_epsilon(v, value, EPSILON12, EPSILON12))
}

struct View<'a>(&'a [u8]);
impl View<'_> {
    fn bytes<const N: usize>(&self, offset: usize) -> Result<[u8; N], RuntimeError> {
        offset
            .checked_add(N)
            .and_then(|end| self.0.get(offset..end))
            .map(|s| s.try_into().unwrap())
            .ok_or_else(|| RuntimeError::new("Offset is outside the bounds of the DataView"))
    }
    fn u8(&self, o: usize) -> Result<u8, RuntimeError> {
        Ok(self.bytes::<1>(o)?[0])
    }
    fn u16(&self, o: usize) -> Result<u16, RuntimeError> {
        Ok(u16::from_le_bytes(self.bytes(o)?))
    }
    fn u32(&self, o: usize) -> Result<u32, RuntimeError> {
        Ok(u32::from_le_bytes(self.bytes(o)?))
    }
    fn i32(&self, o: usize) -> Result<i32, RuntimeError> {
        Ok(i32::from_le_bytes(self.bytes(o)?))
    }
    fn f32(&self, o: usize) -> Result<f64, RuntimeError> {
        Ok(f32::from_le_bytes(self.bytes(o)?) as f64)
    }
    fn f64(&self, o: usize) -> Result<f64, RuntimeError> {
        Ok(f64::from_le_bytes(self.bytes(o)?))
    }
    /// Cesium converts the stored half-turns with `CesiumMath.toRadians(value * 180)`.
    fn angle(&self, o: usize) -> Result<f64, RuntimeError> {
        Ok(math::to_radians(self.f64(o)? * 180.0))
    }
}

#[derive(Clone, Copy)]
struct BorderPoint {
    index: u32,
    cartographic: Cartographic,
}

struct Skirts<'a> {
    positions: &'a mut Vec<Cartesian3>,
    heights: &'a mut Vec<f64>,
    uvs: &'a mut Vec<Cartesian2>,
    web_mercator_ts: &'a mut Vec<f64>,
    normals: &'a mut Vec<Option<Cartesian3>>,
    indices: &'a mut Vec<u32>,
    h_min: f64,
    last_border_point: Option<BorderPoint>,
    skirt_height: f64,
    to_enu: Matrix4,
    ellipsoid: Ellipsoid,
    minimum: Cartesian3,
    maximum: Cartesian3,
}

impl Skirts<'_> {
    fn add(&mut self, border: &[BorderPoint], fudge: f64, east_or_west: bool, corner_fudge: f64) {
        let count = border.len();
        for (j, point) in border.iter().enumerate() {
            let current = self.positions.len() as u32;
            let latitude = point.cartographic.latitude.clamp(-math::PI_OVER_TWO, math::PI_OVER_TWO);
            let height = point.cartographic.height - self.skirt_height;
            self.h_min = js_min(self.h_min, height);
            let mut c = Cartographic::new(point.cartographic.longitude, latitude, height);
            if east_or_west {
                c.longitude += fudge;
            }
            if !east_or_west {
                c.latitude += fudge;
            } else if j == count - 1 {
                c.latitude += corner_fudge;
            } else if j == 0 {
                c.latitude -= corner_fudge;
            }
            let position = self.ellipsoid.cartographic_to_cartesian(&c);
            let border_index = point.index as usize;
            self.positions.push(position);
            self.heights.push(height);
            self.uvs.push(self.uvs[border_index]);
            if !self.web_mercator_ts.is_empty() {
                self.web_mercator_ts.push(self.web_mercator_ts[border_index]);
            }
            if !self.normals.is_empty() {
                self.normals.push(self.normals[border_index]);
            }
            let enu = self.to_enu.multiply_by_point(position);
            self.minimum = self.minimum.minimum_by_component(enu);
            self.maximum = self.maximum.maximum_by_component(enu);
            if let Some(last) = self.last_border_point {
                self.indices.extend([last.index, current - 1, current, current, point.index, last.index]);
            }
            self.last_border_point = Some(*point);
        }
    }
}

fn by(order: impl Fn(&BorderPoint, &BorderPoint) -> f64) -> impl Fn(&BorderPoint, &BorderPoint) -> std::cmp::Ordering {
    // JavaScript comparators return a difference; NaN compares as equal.
    move |a, b| order(a, b).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
}

pub fn create_vertices_from_google_earth_enterprise_buffer(
    p: GoogleEarthEnterpriseBufferRequest<'_>,
) -> Result<GoogleEarthEnterpriseBufferResult, RuntimeError> {
    let r = p.rectangle;
    let (west, south, east, north) = (r.west, r.south, r.east, r.north);
    let (rectangle_width, rectangle_height) = (r.width(), r.height());
    let mut border_latitudes = vec![south, north];
    let mut border_longitudes = vec![west, east];
    let from_enu = Transforms::east_north_up_to_fixed_frame(p.relative_to_center, &p.ellipsoid);
    let to_enu = from_enu.inverse_transformation();
    let (south_mercator_y, one_over_mercator_height) = if p.include_web_mercator_t {
        let s = WebMercatorProjection::geodetic_latitude_to_mercator_angle(south);
        (s, 1.0 / (WebMercatorProjection::geodetic_latitude_to_mercator_angle(north) - s))
    } else {
        (0.0, 0.0)
    };
    let include_normals = p.exaggeration != 1.0;
    let dv = View(p.buffer);
    let (mut min_height, mut max_height) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut minimum = Cartesian3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut maximum = Cartesian3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);

    // First pass: quad borders and allocation sizes.
    let (mut offset, mut size, mut indices_size) = (0usize, 0i64, 0i64);
    for _ in 0..4 {
        let mut o = offset;
        let quad_size = dv.u32(o)? as usize;
        o += 4;
        let x = dv.angle(o)?;
        o += 8;
        if index_of_epsilon(&border_longitudes, x).is_none() {
            border_longitudes.push(x);
        }
        let y = dv.angle(o)?;
        o += 8;
        if index_of_epsilon(&border_latitudes, y).is_none() {
            border_latitudes.push(y);
        }
        o += 16;
        size += dv.i32(o)? as i64;
        o += 4;
        indices_size += dv.i32(o)? as i64 * 3;
        offset += quad_size + 4;
    }
    if size < 0 || indices_size < 0 {
        return Err(RuntimeError::new("Invalid array length"));
    }

    let mut border_points: Vec<Cartographic> = vec![];
    let mut border_indices: Vec<u32> = vec![];
    let mut positions = vec![];
    let mut uvs = vec![];
    let mut heights = vec![];
    let mut web_mercator_ts = vec![];
    let mut normals = vec![];
    let mut indices = vec![];
    let (mut west_border, mut south_border, mut east_border, mut north_border) = (vec![], vec![], vec![], vec![]);

    offset = 0;
    for _ in 0..4 {
        let quad_size = dv.u32(offset)? as usize;
        offset += 4;
        let start = offset;
        let origin_x = dv.angle(offset)?;
        let origin_y = dv.angle(offset + 8)?;
        let step_x = dv.angle(offset + 16)?;
        let step_y = dv.angle(offset + 24)?;
        let (half_step_x, half_step_y) = (step_x * 0.5, step_y * 0.5);
        offset += 32;
        let point_count = dv.i32(offset)?;
        let face_count = dv.i32(offset + 4)?;
        offset += 12;
        let mut mapping = Vec::new();
        for _ in 0..point_count.max(0) {
            let longitude = origin_x + dv.u8(offset)? as f64 * step_x;
            let latitude = origin_y + dv.u8(offset + 1)? as f64 * step_y;
            let mut height = dv.f32(offset + 2)?;
            offset += 6;
            // Old clients read negative altitudes as height / -2^32.
            if height != 0.0 && height < p.negative_elevation_threshold {
                height *= -(2f64.powf(p.negative_altitude_exponent_bias));
            }
            // Heights are stored in units of 1 / 6371010 m.
            height *= 6371010.0;
            let c = Cartographic::new(longitude, latitude, height);
            if index_of_epsilon(&border_longitudes, longitude).is_some()
                || index_of_epsilon(&border_latitudes, latitude).is_some()
            {
                match border_points.iter().position(|v| v.equals_epsilon(&c, EPSILON12)) {
                    None => {
                        border_points.push(c);
                        border_indices.push(positions.len() as u32);
                    }
                    Some(index) => {
                        mapping.push(border_indices[index]);
                        continue;
                    }
                }
            }
            let point_offset = positions.len() as u32;
            mapping.push(point_offset);
            let border = BorderPoint { index: point_offset, cartographic: c };
            if (longitude - west).abs() < half_step_x {
                west_border.push(border);
            } else if (longitude - east).abs() < half_step_x {
                east_border.push(border);
            } else if (latitude - south).abs() < half_step_y {
                south_border.push(border);
            } else if (latitude - north).abs() < half_step_y {
                north_border.push(border);
            }
            min_height = js_min(height, min_height);
            max_height = js_max(height, max_height);
            heights.push(height);
            let position = p.ellipsoid.cartographic_to_cartesian(&c);
            positions.push(position);
            if p.include_web_mercator_t {
                web_mercator_ts.push(
                    (WebMercatorProjection::geodetic_latitude_to_mercator_angle(latitude) - south_mercator_y)
                        * one_over_mercator_height,
                );
            }
            if include_normals {
                normals.push(p.ellipsoid.geodetic_surface_normal(position));
            }
            let enu = to_enu.multiply_by_point(position);
            minimum = minimum.minimum_by_component(enu);
            maximum = maximum.maximum_by_component(enu);
            let u = math::clamp((longitude - west) / (east - west), 0.0, 1.0);
            let v = math::clamp((latitude - south) / (north - south), 0.0, 1.0);
            uvs.push(Cartesian2::new(u, v));
        }
        for _ in 0..(face_count.max(0) as usize * 3) {
            let index = dv.u16(offset)? as usize;
            indices.push(mapping.get(index).copied().unwrap_or(0));
            offset += 2;
        }
        if quad_size != offset - start {
            return Err(RuntimeError::new("Invalid terrain tile."));
        }
    }

    let vertex_count_without_skirts = positions.len();
    let index_count_without_skirts = indices.len();
    west_border.sort_by(by(|a, b| b.cartographic.latitude - a.cartographic.latitude));
    south_border.sort_by(by(|a, b| a.cartographic.longitude - b.cartographic.longitude));
    east_border.sort_by(by(|a, b| a.cartographic.latitude - b.cartographic.latitude));
    north_border.sort_by(by(|a, b| b.cartographic.longitude - a.cartographic.longitude));

    let percentage = 0.00001;
    let mut skirts = Skirts {
        positions: &mut positions,
        heights: &mut heights,
        uvs: &mut uvs,
        web_mercator_ts: &mut web_mercator_ts,
        normals: &mut normals,
        indices: &mut indices,
        h_min: min_height,
        last_border_point: None,
        skirt_height: p.skirt_height,
        to_enu,
        ellipsoid: p.ellipsoid,
        minimum,
        maximum,
    };
    skirts.add(&west_border, -percentage * rectangle_width, true, -percentage * rectangle_height);
    skirts.add(&south_border, -percentage * rectangle_height, false, 0.0);
    skirts.add(&east_border, percentage * rectangle_width, true, percentage * rectangle_height);
    skirts.add(&north_border, percentage * rectangle_height, false, 0.0);
    let (h_min, minimum, maximum) = (skirts.h_min, skirts.minimum, skirts.maximum);

    // The north-west corner is in the west array: close the skirt between the
    // last north vertex and the first west vertex.
    if let (Some(first), Some(last)) = (west_border.first(), north_border.last()) {
        let first_skirt = vertex_count_without_skirts as u32;
        let last_skirt = positions.len() as u32 - 1;
        indices.extend([last.index, last_skirt, first_skirt, first_skirt, first.index, last.index]);
    }

    let bounding_sphere_3d = BoundingSphere::from_points(&positions);
    let oriented_bounding_box = OrientedBoundingBox::from_rectangle(&r, min_height, max_height, &p.ellipsoid);
    let occludee_point_in_scaled_space = EllipsoidalOccluder::new(&p.ellipsoid, None)
        .compute_horizon_culling_point_possibly_under_ellipsoid(p.relative_to_center, &positions, Some(min_height));
    let encoding = TerrainEncoding::new(TerrainEncodingOptions {
        center: Some(p.relative_to_center),
        axis_aligned_bounding_box: Some(AxisAlignedBoundingBox::new(minimum, maximum, Some(p.relative_to_center))),
        minimum_height: Some(h_min),
        maximum_height: Some(max_height),
        from_enu: Some(from_enu),
        has_vertex_normals: false,
        has_web_mercator_t: p.include_web_mercator_t,
        has_geodetic_surface_normals: include_normals,
        exaggeration: p.exaggeration,
        exaggeration_relative_height: p.exaggeration_relative_height,
    })
    .ok_or_else(|| RuntimeError::new("Invalid terrain tile."))?;
    let mut vertices = vec![0.0; positions.len() * encoding.stride];
    let mut buffer_index = 0;
    for k in 0..positions.len() {
        let mut vertex = TerrainVertex::new(positions[k], uvs[k], heights[k]);
        vertex.web_mercator_t = web_mercator_ts.get(k).copied();
        vertex.geodetic_surface_normal = normals.get(k).copied().flatten();
        buffer_index = encoding
            .encode(&mut vertices, buffer_index, &vertex)
            .ok_or_else(|| RuntimeError::new("Invalid terrain tile."))?;
    }

    let reversed = |border: &[BorderPoint]| border.iter().rev().map(|v| v.index).collect::<Vec<_>>();
    let west_indices = reversed(&west_border);
    let mut south_indices = reversed(&south_border);
    let east_indices = reversed(&east_border);
    let mut north_indices = reversed(&north_border);
    // Cesium inserts these corners unconditionally; a tile without east or west
    // border points would add `undefined`, which has no index to represent here.
    if let Some(&corner) = east_indices.last() {
        south_indices.insert(0, corner);
    }
    if let Some(&corner) = west_indices.first() {
        south_indices.push(corner);
    }
    if let Some(&corner) = west_indices.last() {
        north_indices.insert(0, corner);
    }
    if let Some(&corner) = east_indices.first() {
        north_indices.push(corner);
    }

    Ok(GoogleEarthEnterpriseBufferResult {
        vertices,
        // `new Uint16Array(indices)` wraps each index modulo 2^16.
        indices: indices.into_iter().map(|i| i as u16).collect(),
        number_of_attributes: encoding.stride,
        minimum_height: min_height,
        maximum_height: max_height,
        bounding_sphere_3d,
        oriented_bounding_box,
        occludee_point_in_scaled_space,
        encoding,
        vertex_count_without_skirts,
        index_count_without_skirts,
        edges: TerrainEdgeIndices {
            west_indices_south_to_north: west_indices,
            south_indices_east_to_west: south_indices,
            east_indices_north_to_south: east_indices,
            north_indices_west_to_east: north_indices,
        },
    })
}
