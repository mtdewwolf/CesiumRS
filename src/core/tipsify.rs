//! CesiumJS `Tipsify`: vertex-cache-aware triangle reordering (Sander,
//! Nehab and Barczak, "Fast Triangle Reordering for Vertex Locality and
//! Reduced Overdraw").

fn maximum_index_of(indices: &[u32]) -> u32 {
    indices.iter().copied().max().unwrap_or(0)
}

/// Average cache miss ratio of `indices` for a FIFO cache of `cache_size`
/// (Cesium's default is 24). `None` unless there are whole triangles and the
/// cache holds at least three vertices.
pub fn calculate_acmr(indices: &[u32], maximum_index: Option<u32>, cache_size: usize) -> Option<f64> {
    let num_indices = indices.len();
    if num_indices < 3 || !num_indices.is_multiple_of(3) || cache_size < 3 {
        return None;
    }
    let maximum_index = maximum_index.unwrap_or_else(|| maximum_index_of(indices));
    let mut vertex_time_stamps = vec![0usize; maximum_index as usize + 1];
    let mut s = cache_size + 1;
    for &index in indices {
        if s - vertex_time_stamps[index as usize] > cache_size {
            vertex_time_stamps[index as usize] = s;
            s += 1;
        }
    }
    Some((s as f64 - cache_size as f64 + 1.0) / (num_indices as f64 / 3.0))
}

struct Vertex {
    num_live_triangles: usize,
    time_stamp: usize,
    vertex_triangles: Vec<usize>,
}

/// Reorders triangles for the post-transform vertex cache. `None` unless
/// there are whole triangles and the cache holds at least three vertices.
pub fn tipsify(indices: &[u32], maximum_index: Option<u32>, cache_size: usize) -> Option<Vec<u32>> {
    let num_indices = indices.len();
    if num_indices < 3 || !num_indices.is_multiple_of(3) || cache_size < 3 {
        return None;
    }
    let maximum_index_plus_one = match maximum_index {
        Some(m) => m as usize + 1,
        None => maximum_index_of(indices) as usize + 1,
    };

    let mut vertices: Vec<Vertex> = (0..maximum_index_plus_one)
        .map(|_| Vertex { num_live_triangles: 0, time_stamp: 0, vertex_triangles: Vec::new() })
        .collect();
    for (triangle, tri) in indices.as_chunks::<3>().0.iter().enumerate() {
        for &index in tri {
            let v = &mut vertices[index as usize];
            v.vertex_triangles.push(triangle);
            v.num_live_triangles += 1;
        }
    }

    let mut f: isize = 0;
    let mut s = cache_size + 1;
    let mut cursor = 1;
    let mut dead_end: Vec<u32> = Vec::new();
    let mut output_indices = Vec::with_capacity(num_indices);
    let mut triangle_emitted = vec![false; num_indices / 3];

    while f != -1 {
        let mut one_ring = Vec::new();
        let vertex_triangles = vertices[f as usize].vertex_triangles.clone();
        for triangle in vertex_triangles {
            if !triangle_emitted[triangle] {
                triangle_emitted[triangle] = true;
                for j in 0..3 {
                    let index = indices[triangle * 3 + j];
                    one_ring.push(index);
                    dead_end.push(index);
                    output_indices.push(index);
                    let vertex = &mut vertices[index as usize];
                    vertex.num_live_triangles -= 1;
                    if s - vertex.time_stamp > cache_size {
                        vertex.time_stamp = s;
                        s += 1;
                    }
                }
            }
        }

        // getNextVertex: the one-ring vertex still in cache that was cached
        // earliest, else skip the dead end.
        let mut n: isize = -1;
        let mut m: isize = -1;
        for &index in &one_ring {
            let v = &vertices[index as usize];
            if v.num_live_triangles > 0 {
                let mut p = 0;
                if s - v.time_stamp + 2 * v.num_live_triangles <= cache_size {
                    p = (s - v.time_stamp) as isize;
                }
                if p > m || m == -1 {
                    m = p;
                    n = index as isize;
                }
            }
        }
        f = if n == -1 { skip_dead_end(&vertices, &mut dead_end, &mut cursor, maximum_index_plus_one) } else { n };
    }
    Some(output_indices)
}

fn skip_dead_end(
    vertices: &[Vertex],
    dead_end: &mut Vec<u32>,
    cursor: &mut usize,
    maximum_index_plus_one: usize,
) -> isize {
    while let Some(d) = dead_end.pop() {
        if vertices[d as usize].num_live_triangles > 0 {
            return d as isize;
        }
    }
    while *cursor < maximum_index_plus_one {
        if vertices[*cursor].num_live_triangles > 0 {
            *cursor += 1;
            return *cursor as isize - 1;
        }
        *cursor += 1;
    }
    -1
}
