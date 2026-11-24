//! Temporary terrain geometry stitched to the meshes covering adjacent tiles.

use crate::core::{
    terrain_encoding::{TerrainEncoding, TerrainEncodingOptions, TerrainVertex},
    terrain_mesh::{TerrainMesh, TerrainMeshOptions},
    terrain_provider::TerrainEdgeIndices,
    AttributeCompression, BoundingSphere, Cartesian2, Cartesian3, Cartographic, Ellipsoid, EllipsoidalOccluder,
    IndexDatatype, OrientedBoundingBox, Rectangle, WebMercatorProjection,
};
use std::f64::consts::TAU;

const EDGE_EPSILON: f64 = 1.0e-10;

#[derive(Clone, Copy, Debug)]
pub struct TerrainFillOptions {
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub geometric_error: f64,
    pub exaggeration: f64,
    pub exaggeration_relative_height: f64,
}

impl Default for TerrainFillOptions {
    fn default() -> Self {
        Self {
            minimum_height: 0.0,
            maximum_height: 0.0,
            geometric_error: 0.0,
            exaggeration: 1.0,
            exaggeration_relative_height: 0.0,
        }
    }
}

/// A fill is reusable as an edge source for adjacent fills.
#[derive(Clone, Debug)]
pub struct TerrainFillMesh {
    pub mesh: TerrainMesh,
}

#[derive(Clone, Copy)]
struct Sample {
    uv: Cartesian2,
    height: f64,
    position: Option<Cartesian3>,
    normal: Option<Cartesian3>,
}

impl Sample {
    fn with_height(uv: Cartesian2, height: f64) -> Self {
        Self { uv, height, position: None, normal: None }
    }

    fn interpolate(self, other: Self, t: f64, uv: Cartesian2) -> Self {
        let lerp = |a: Cartesian3, b: Cartesian3| a.add(b.subtract(a).multiply_by_scalar(t));
        Self {
            uv,
            height: self.height + (other.height - self.height) * t,
            position: self.position.zip(other.position).map(|(a, b)| lerp(a, b)),
            normal: self.normal.zip(other.normal).map(|(a, b)| lerp(a, b).normalize()),
        }
    }
}

/// Perimeter walks counterclockwise from northwest, as in the source terrain edge lists.
#[derive(Clone, Copy)]
enum Edge {
    West,
    South,
    East,
    North,
}

impl Edge {
    const ALL: [Self; 4] = [Self::West, Self::South, Self::East, Self::North];
    const CORNERS: [Cartesian2; 4] =
        [Cartesian2::new(0.0, 1.0), Cartesian2::new(0.0, 0.0), Cartesian2::new(1.0, 0.0), Cartesian2::new(1.0, 1.0)];

    fn opposite(self) -> Self {
        Self::ALL[(self as usize + 2) % 4]
    }

    fn indices(self, mesh: &TerrainMesh) -> &[u32] {
        match self {
            Self::West => &mesh.edges.west_indices_south_to_north,
            Self::South => &mesh.edges.south_indices_east_to_west,
            Self::East => &mesh.edges.east_indices_north_to_south,
            Self::North => &mesh.edges.north_indices_west_to_east,
        }
    }

    fn uv(self, along: f64) -> Cartesian2 {
        match self {
            Self::West => Cartesian2::new(0.0, 1.0 - along),
            Self::South => Cartesian2::new(along, 0.0),
            Self::East => Cartesian2::new(1.0, along),
            Self::North => Cartesian2::new(1.0 - along, 1.0),
        }
    }

    fn along(self, uv: Cartesian2) -> f64 {
        match self {
            Self::West => 1.0 - uv.y,
            Self::South => uv.x,
            Self::East => uv.y,
            Self::North => 1.0 - uv.x,
        }
    }

    fn shared(self, tile: &Rectangle, neighbor: &Rectangle) -> bool {
        let longitude_equal = |a: f64, b: f64| {
            let delta = (a - b).abs();
            delta <= EDGE_EPSILON || (delta - TAU).abs() <= EDGE_EPSILON
        };
        match self {
            Self::West => {
                longitude_equal(tile.west, neighbor.east) && tile.south < neighbor.north && tile.north > neighbor.south
            }
            Self::East => {
                longitude_equal(tile.east, neighbor.west) && tile.south < neighbor.north && tile.north > neighbor.south
            }
            Self::South => (tile.south - neighbor.north).abs() <= EDGE_EPSILON && longitude_overlaps(tile, neighbor),
            Self::North => (tile.north - neighbor.south).abs() <= EDGE_EPSILON && longitude_overlaps(tile, neighbor),
        }
    }
}

fn longitude_overlaps(a: &Rectangle, b: &Rectangle) -> bool {
    [-TAU, 0.0, TAU].into_iter().any(|offset| {
        let b_west = b.west + offset;
        a.west < b_west + b.width() && a.west + a.width() > b_west
    })
}

fn to_tile_uv(tile: &Rectangle, mesh: &TerrainMesh, uv: Cartesian2) -> Cartesian2 {
    let longitude = mesh.rectangle.west + uv.x * mesh.rectangle.width();
    let tile_middle = tile.west + tile.width() * 0.5;
    let longitude = if longitude >= tile.west - EDGE_EPSILON && longitude <= tile.west + tile.width() + EDGE_EPSILON {
        longitude
    } else {
        longitude + TAU * ((tile_middle - longitude) / TAU).round()
    };
    let latitude = mesh.rectangle.south + uv.y * mesh.rectangle.height();
    Cartesian2::new((longitude - tile.west) / tile.width(), (latitude - tile.south) / tile.height())
}

fn read_sample(tile: &Rectangle, mesh: &TerrainMesh, index: u32) -> Option<Sample> {
    let index = index as usize;
    let encoding = &mesh.encoding;
    let uv = to_tile_uv(tile, mesh, encoding.decode_texture_coordinates(&mesh.vertices, index)?);
    Some(Sample {
        uv,
        height: encoding.decode_height(&mesh.vertices, index)?,
        position: encoding.decode_position(&mesh.vertices, index),
        normal: encoding.decode_normal(&mesh.vertices, index),
    })
}

fn edge_samples(tile: &Rectangle, edge: Edge, neighbors: &[&TerrainMesh]) -> Vec<Sample> {
    let mut samples = Vec::new();
    for &mesh in neighbors.iter().filter(|mesh| edge.shared(tile, &mesh.rectangle)) {
        let mut source: Vec<_> = edge
            .opposite()
            .indices(mesh)
            .iter()
            .filter_map(|&index| read_sample(tile, mesh, index))
            .map(|sample| (edge.along(sample.uv), sample))
            .collect();
        source.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(t, mut sample) in &source {
            if (-EDGE_EPSILON..=1.0 + EDGE_EPSILON).contains(&t) {
                sample.uv = edge.uv(t.clamp(0.0, 1.0));
                samples.push(sample);
            }
        }
        // A fine fill can end in the middle of a coarse source segment. Keep
        // the interpolated Cartesian point on the source segment itself.
        for end in [0.0, 1.0] {
            if let Some(pair) =
                source.windows(2).find(|pair| pair[0].0 < end - EDGE_EPSILON && pair[1].0 > end + EDGE_EPSILON)
            {
                let [(a, sample_a), (b, sample_b)] = [pair[0], pair[1]];
                samples.push(sample_a.interpolate(sample_b, (end - a) / (b - a), edge.uv(end)));
            }
        }
    }
    samples.sort_by(|a, b| edge.along(a.uv).total_cmp(&edge.along(b.uv)));
    samples.dedup_by(|a, b| (edge.along(a.uv) - edge.along(b.uv)).abs() <= EDGE_EPSILON);
    samples
}

fn corner_sample(
    tile: &Rectangle,
    index: usize,
    edges: &[Vec<Sample>; 4],
    neighbors: &[&TerrainMesh],
) -> Option<Sample> {
    let uv = Edge::CORNERS[index];
    let at_corner =
        |sample: &Sample| (sample.uv.x - uv.x).abs() <= EDGE_EPSILON && (sample.uv.y - uv.y).abs() <= EDGE_EPSILON;
    edges[index]
        .first()
        .filter(|s| at_corner(s))
        .copied()
        .or_else(|| edges[(index + 3) % 4].last().filter(|s| at_corner(s)).copied())
        .or_else(|| {
            neighbors
                .iter()
                .filter_map(|mesh| {
                    let point =
                        Cartographic::new(tile.west + uv.x * tile.width(), tile.south + uv.y * tile.height(), 0.0);
                    if !mesh.rectangle.contains(&point) {
                        return None;
                    }
                    Edge::ALL
                        .into_iter()
                        .flat_map(|edge| edge.indices(mesh))
                        .filter_map(|&i| read_sample(tile, mesh, i))
                        .find(at_corner)
                })
                .next()
        })
}

impl TerrainFillMesh {
    pub(crate) fn shares_edge(tile: &Rectangle, neighbor: &Rectangle) -> bool {
        Edge::ALL.into_iter().any(|edge| edge.shared(tile, neighbor))
    }

    /// Generates a skirtless fill. Neighbors are examined in priority order,
    /// with real terrain meshes preceding fills generated earlier in the frame.
    pub fn new(
        rectangle: Rectangle,
        ellipsoid: &Ellipsoid,
        options: TerrainFillOptions,
        neighbors: &[&TerrainMesh],
    ) -> Self {
        let edges = Edge::ALL.map(|edge| edge_samples(&rectangle, edge, neighbors));
        let mut corners = std::array::from_fn(|i| corner_sample(&rectangle, i, &edges, neighbors));
        for i in 0..4 {
            if corners[i].is_some() {
                continue;
            }
            let next = (i + 1) % 4;
            let previous = (i + 3) % 4;
            let a = edges[i].first().copied().or(corners[next]);
            let b = edges[previous].last().copied().or(corners[previous]);
            let fallback =
                corners[(i + 2) % 4].map_or((options.minimum_height + options.maximum_height) * 0.5, |v| v.height);
            let height = match (a, b) {
                (Some(a), Some(b)) => (a.height + b.height) * 0.5,
                (Some(a), None) | (None, Some(a)) => a.height,
                (None, None) => fallback,
            };
            corners[i] = Some(Sample::with_height(Edge::CORNERS[i], height));
        }
        let corners = corners.map(Option::unwrap);
        let min = corners.iter().map(|s| s.height).fold(f64::INFINITY, f64::min);
        let max = corners.iter().map(|s| s.height).fold(f64::NEG_INFINITY, f64::max);
        let max_width = 6.0 * (1.0 - options.geometric_error / ellipsoid.maximum_radius).clamp(-1.0, 1.0).acos();
        let (samples, indices, edges) = if rectangle.width() > max_width
            && max - min <= options.geometric_error
            && edges.iter().all(Vec::is_empty)
            && corners.iter().all(|c| c.position.is_none())
        {
            grid(max)
        } else {
            fan(corners, &edges, (min + max) * 0.5)
        };
        let mesh = encode_mesh(rectangle, ellipsoid, options, samples, indices, edges);
        Self { mesh }
    }

    /// Interpolates height on the actual fill triangles in tile coordinates.
    pub fn height_at(&self, position: &Cartographic) -> Option<f64> {
        let mesh = &self.mesh;
        let rectangle = mesh.rectangle;
        if !rectangle.contains(position) {
            return None;
        }
        let u = if position.longitude < rectangle.west { position.longitude + TAU } else { position.longitude };
        let uv = Cartesian2::new(
            (u - rectangle.west) / rectangle.width(),
            (position.latitude - rectangle.south) / rectangle.height(),
        );
        let encoding = &mesh.encoding;
        for offset in (0..mesh.indices.len()).step_by(3) {
            let indices = [offset, offset + 1, offset + 2].map(|i| mesh.indices.get(i).unwrap() as usize);
            let [a, b, c] = indices.map(|i| encoding.decode_texture_coordinates(&mesh.vertices, i).unwrap());
            let det = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
            let s = ((b.y - c.y) * (uv.x - c.x) + (c.x - b.x) * (uv.y - c.y)) / det;
            let t = ((c.y - a.y) * (uv.x - c.x) + (a.x - c.x) * (uv.y - c.y)) / det;
            let r = 1.0 - s - t;
            if [s, t, r].into_iter().all(|w| w >= -EDGE_EPSILON) {
                let [ha, hb, hc] = indices.map(|i| encoding.decode_height(&mesh.vertices, i).unwrap());
                return Some(s * ha + t * hb + r * hc);
            }
        }
        None
    }
}

fn fan(corners: [Sample; 4], edges: &[Vec<Sample>; 4], center: f64) -> (Vec<Sample>, Vec<u32>, TerrainEdgeIndices) {
    let mut samples = Vec::new();
    let starts = std::array::from_fn::<_, 4, _>(|i| {
        let start = samples.len() as u32;
        samples.push(corners[i]);
        samples.extend(edges[i].iter().filter(|sample| {
            let t = Edge::ALL[i].along(sample.uv);
            t > EDGE_EPSILON && t < 1.0 - EDGE_EPSILON
        }));
        start
    });
    let end = samples.len() as u32;
    let side = |i: usize| {
        let next = starts.get(i + 1).copied().unwrap_or(end);
        std::iter::once(next % end).chain((starts[i]..next).rev()).collect()
    };
    let indices = (0..end).flat_map(|i| [end, i, (i + 1) % end]).collect();
    samples.push(Sample::with_height(Cartesian2::new(0.5, 0.5), center));
    let edges = TerrainEdgeIndices {
        west_indices_south_to_north: side(0),
        south_indices_east_to_west: side(1),
        east_indices_north_to_south: side(2),
        north_indices_west_to_east: side(3),
    };
    (samples, indices, edges)
}

fn grid(height: f64) -> (Vec<Sample>, Vec<u32>, TerrainEdgeIndices) {
    const SIZE: usize = 9;
    let grid = crate::core::terrain_provider::get_regular_grid_indices_and_edge_indices(SIZE, SIZE).unwrap();
    let samples = (0..SIZE)
        .flat_map(|y| {
            (0..SIZE).map(move |x| Sample::with_height(Cartesian2::new(x as f64 / 8.0, 1.0 - y as f64 / 8.0), height))
        })
        .collect();
    let indices = (0..grid.indices.len()).map(|i| grid.indices.get(i).unwrap() as u32).collect();
    (samples, indices, grid.edges.clone())
}

fn encode_mesh(
    rectangle: Rectangle,
    ellipsoid: &Ellipsoid,
    options: TerrainFillOptions,
    samples: Vec<Sample>,
    indices: Vec<u32>,
    edges: TerrainEdgeIndices,
) -> TerrainMesh {
    let mercator = WebMercatorProjection::geodetic_latitude_to_mercator_angle;
    let south = mercator(rectangle.south);
    let mercator_height = mercator(rectangle.north) - south;
    let mut minimum_height = f64::INFINITY;
    let mut maximum_height = f64::NEG_INFINITY;
    let vertices: Vec<_> = samples
        .into_iter()
        .map(|sample| {
            let cartographic = Cartographic::new(
                rectangle.west + sample.uv.x * rectangle.width(),
                rectangle.south + sample.uv.y * rectangle.height(),
                sample.height,
            );
            let position = sample.position.unwrap_or_else(|| ellipsoid.cartographic_to_cartesian(&cartographic));
            let normal = ellipsoid.geodetic_surface_normal_cartographic(&cartographic);
            minimum_height = minimum_height.min(sample.height);
            maximum_height = maximum_height.max(sample.height);
            TerrainVertex {
                position,
                texture_coordinates: sample.uv,
                height: sample.height,
                normal_to_pack: Some(AttributeCompression::oct_encode(sample.normal.unwrap_or(normal))),
                web_mercator_t: Some(if mercator_height == 0.0 {
                    sample.uv.y
                } else {
                    (mercator(cartographic.latitude) - south) / mercator_height
                }),
                geodetic_surface_normal: Some(normal),
            }
        })
        .collect();
    let mut middle = rectangle.center();
    middle.height = (minimum_height + maximum_height) * 0.5;
    let center = ellipsoid.cartographic_to_cartesian(&middle);
    let encoding = TerrainEncoding::new(TerrainEncodingOptions {
        center: Some(center),
        has_vertex_normals: true,
        has_web_mercator_t: true,
        has_geodetic_surface_normals: options.exaggeration != 1.0,
        exaggeration: options.exaggeration,
        exaggeration_relative_height: options.exaggeration_relative_height,
        ..TerrainEncodingOptions::default()
    })
    .unwrap();
    let stride = encoding.stride;
    let mut buffer = vec![0.0; vertices.len() * stride];
    for (i, vertex) in vertices.iter().enumerate() {
        encoding.encode(&mut buffer, i * stride, vertex).unwrap();
    }
    let positions: Vec<_> = vertices.iter().map(|v| v.position).collect();
    let oriented_bounding_box =
        OrientedBoundingBox::from_rectangle(&rectangle, minimum_height, maximum_height, ellipsoid);
    let vertex_count = vertices.len();
    TerrainMesh::new(TerrainMeshOptions {
        center,
        vertices: buffer,
        indices: IndexDatatype::create_typed_array(vertex_count, &indices),
        index_count_without_skirts: Some(indices.len()),
        vertex_count_without_skirts: vertex_count,
        minimum_height,
        maximum_height,
        rectangle,
        bounding_sphere_3d: BoundingSphere::from_points(&positions),
        occludee_point_in_scaled_space: EllipsoidalOccluder::new(ellipsoid, None)
            .compute_horizon_culling_point_possibly_under_ellipsoid(center, &positions, Some(minimum_height)),
        vertex_stride: Some(stride),
        oriented_bounding_box,
        encoding,
        edges,
    })
}
