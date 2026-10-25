//! CesiumJS `PrimitiveOutlineGenerator` (`CESIUM_primitive_outline`): per-vertex
//! outline coordinates that sample a lookup texture, copying vertices where
//! adjacent triangles need different edge orders.
//!
//! JavaScript semantics kept: vertex indices are numbers, a missing trailing
//! index being NaN; the coordinate array is sparse, so unreferenced vertices
//! read as NaN; index arrays widen when a copy overflows them; and
//! `updateAttribute` divides by the original vertex count even when that
//! leaves a fractional stride, where typed-array reads and writes at
//! non-integer indices are `undefined` and no-ops.
use crate::core::{IndexDatatype, PixelDatatype, PixelFormat, RuntimeError, TypedArray};
use crate::renderer::{
    Context, Gl, Sampler, Texture, TextureMagnificationFilter, TextureMinificationFilter, TextureOptions, TextureWrap,
};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// glTF reserves 65535 as the primitive restart value.
const MAX_GLTF_UINT16_INDEX: f64 = 65534.0;
const MAX_GLTF_UINT8_INDEX: f64 = 255.0;

/// `EdgeSet`: unordered vertex pairs, hashed as `small * vertexCount + big`
/// in JavaScript numbers.
struct EdgeSet {
    vertex_count: f64,
    edges: HashSet<u64>,
}

/// A `Set` member: SameValueZero, so -0 is 0 and every NaN is one value.
fn same_value_zero(value: f64) -> u64 {
    if value == 0.0 {
        0
    } else if value.is_nan() {
        f64::NAN.to_bits()
    } else {
        value.to_bits()
    }
}

impl EdgeSet {
    fn new(indices: &[f64], vertex_count: usize) -> Self {
        let vertex_count = vertex_count as f64;
        let mut edges = HashSet::new();
        // `for (i = 0; i < length; i += 2)`: an odd length pairs the last index with undefined.
        for pair in indices.chunks(2) {
            let (a, b) = (pair[0], pair.get(1).copied().unwrap_or(f64::NAN));
            edges.insert(same_value_zero(Self::hash(a, b, vertex_count)));
        }
        Self { vertex_count, edges }
    }
    fn hash(a: f64, b: f64, vertex_count: f64) -> f64 {
        // Math.min/max propagate NaN.
        let (small, big) = if a.is_nan() || b.is_nan() { (f64::NAN, f64::NAN) } else { (a.min(b), a.max(b)) };
        small * vertex_count + big
    }
    fn has_edge(&self, a: f64, b: f64) -> bool {
        self.edges.contains(&same_value_zero(Self::hash(a, b, self.vertex_count)))
    }
}

pub struct PrimitiveOutlineGenerator {
    triangle_indices: TypedArray,
    original_vertex_count: usize,
    outline_coordinates: Vec<f32>,
    /// The original vertex of each copy.
    extra_vertices: Vec<f64>,
}

impl PrimitiveOutlineGenerator {
    /// `outline_indices` are the extension's edge index pairs, as the numbers
    /// its accessor reads.
    pub fn new(triangle_indices: TypedArray, outline_indices: &[f64], original_vertex_count: usize) -> Self {
        let edges = EdgeSet::new(outline_indices, original_vertex_count);
        let mut generator = Self {
            triangle_indices,
            original_vertex_count,
            outline_coordinates: Vec::new(),
            extra_vertices: Vec::new(),
        };
        generator.initialize(&edges);
        generator
    }

    /// The triangle indices, widened to 16 or 32 bits if copies needed it.
    pub fn updated_triangle_indices(&self) -> &TypedArray {
        &self.triangle_indices
    }

    /// Three coordinates per vertex, original vertices first.
    pub fn outline_coordinates(&self) -> &[f32] {
        &self.outline_coordinates
    }

    fn initialize(&mut self, edges: &EdgeSet) {
        let vertex_count = self.original_vertex_count as f64;
        let mut coordinates: Vec<Option<f64>> = Vec::new();
        // Unmatchable vertex index -> its copy, so each vertex is copied once.
        let mut copies: HashMap<u64, f64> = HashMap::new();
        let indices = &mut self.triangle_indices;
        let index = |indices: &TypedArray, i: usize| indices.get(i).unwrap_or(f64::NAN);
        let mut i = 0;
        while i < indices.len() {
            let (mut i0, mut i1, mut i2) = (index(indices, i), index(indices, i + 1), index(indices, i + 2));
            let edge01 = edges.has_edge(i0, i1);
            let edge12 = edges.has_edge(i1, i2);
            let edge20 = edges.has_edge(i2, i0);
            let mut unmatchable = match_and_store(&mut coordinates, i0, i1, i2, edge01, edge12, edge20);
            while let Some(vertex) = unmatchable {
                let copy = *copies.entry(same_value_zero(vertex)).or_insert_with(|| {
                    let copy = vertex_count + self.extra_vertices.len() as f64;
                    // A copied vertex may itself be a copy; store the original.
                    let mut original = vertex;
                    while original >= vertex_count {
                        original = self.extra_vertices[(original - vertex_count) as usize];
                    }
                    self.extra_vertices.push(original);
                    copy
                });
                // A copy can overflow an 8- or 16-bit index buffer.
                let widen = match indices {
                    TypedArray::Uint8(_) | TypedArray::Uint16(_) if copy > MAX_GLTF_UINT16_INDEX => {
                        Some(TypedArray::Uint32(indices.to_f64_vec().into_iter().map(|v| v as u32).collect()))
                    }
                    TypedArray::Uint8(v) if copy > MAX_GLTF_UINT8_INDEX => {
                        Some(TypedArray::Uint16(v.iter().map(|&v| u16::from(v)).collect()))
                    }
                    _ => None,
                };
                if let Some(widened) = widen {
                    *indices = widened;
                }
                if vertex == i0 {
                    i0 = copy;
                    indices.set(i, copy);
                } else if vertex == i1 {
                    i1 = copy;
                    indices.set(i + 1, copy);
                } else {
                    i2 = copy;
                    indices.set(i + 2, copy);
                }
                unmatchable = match_and_store(&mut coordinates, i0, i1, i2, edge01, edge12, edge20);
            }
            i += 3;
        }
        // `new Float32Array(sparseArray)`: holes are undefined, stored as NaN.
        self.outline_coordinates = coordinates.into_iter().map(|c| c.map_or(f32::NAN, |c| c as f32)).collect();
    }

    /// The attribute with values appended for each copied vertex, as
    /// `updateAttribute` builds it.
    pub fn update_attribute(&self, attribute: &TypedArray) -> Result<TypedArray, RuntimeError> {
        let original_length = attribute.len() as f64;
        let stride = original_length / self.original_vertex_count as f64;
        let extra = self.extra_vertices.len() as f64;
        let length = original_length + extra * stride;
        if !length.is_finite() && !length.is_nan() {
            return Err(RuntimeError::new(format!("Invalid typed array length: {length}")));
        }
        // ToIndex truncates; NaN is 0.
        let length = if length.is_nan() { 0 } else { length.trunc() as usize };
        if length < attribute.len() {
            return Err(RuntimeError::new("offset is out of bounds"));
        }
        let mut result = TypedArray::zeros(attribute.component_datatype(), length);
        for (i, value) in attribute.to_f64_vec().into_iter().enumerate() {
            result.set(i, value);
        }
        // A typed array element exists only at integer indices below its length.
        let element = |index: f64| (index.fract() == 0.0 && index >= 0.0).then_some(index as usize);
        for (i, &vertex) in self.extra_vertices.iter().enumerate() {
            let source = vertex * stride;
            let target = original_length + i as f64 * stride;
            let mut j = 0.0;
            while j < stride {
                let value = element(source + j).and_then(|k| result.get(k)).unwrap_or(f64::NAN);
                if let Some(k) = element(target + j) {
                    result.set(k, value);
                }
                j += 1.0;
            }
        }
        Ok(result)
    }

    /// The outline lookup texture, shared through the context's cache.
    pub fn create_texture<G: Gl>(context: &Context<G>) -> Result<Rc<Texture<G>>, RuntimeError> {
        let cache = context.cache()?;
        if let Some(texture) = cache.model_outline_texture.borrow().as_ref() {
            return Ok(texture.clone());
        }
        let max_size = 4096.min(context.limits()?.maximum_texture_size);
        let mut size = max_size;
        let level_zero = create_mip_level(size);
        let mut levels = Vec::new();
        while size > 1 {
            size >>= 1;
            levels.push(create_mip_level(size));
        }
        let options = TextureOptions {
            width: max_size,
            height: 1,
            // The shader samples .r. Use the equivalent single-channel format
            // on WebGL2, where the renderer disallows legacy LUMINANCE.
            pixel_format: if context.webgl2() { PixelFormat::Red } else { PixelFormat::Luminance },
            pixel_datatype: PixelDatatype::UnsignedByte,
            sampler: Sampler {
                wrap_s: TextureWrap::ClampToEdge,
                wrap_t: TextureWrap::ClampToEdge,
                minification_filter: TextureMinificationFilter::LinearMipmapLinear,
                magnification_filter: TextureMagnificationFilter::Linear,
                ..Sampler::default()
            },
            ..TextureOptions::rgba(max_size, 1)
        };
        let texture = Rc::new(Texture::with_mipmaps(context, options, &level_zero, &levels)?);
        *cache.model_outline_texture.borrow_mut() = Some(texture.clone());
        Ok(texture)
    }
}

/// Returns the vertex to copy, or `None` once the triangle's coordinates are stored.
fn match_and_store(
    coordinates: &mut Vec<Option<f64>>,
    i0: f64,
    i1: f64,
    i2: f64,
    edge01: bool,
    edge12: bool,
    edge20: bool,
) -> Option<f64> {
    let flag = |b: bool| if b { 1.0 } else { 0.0 };
    let v0 = [flag(edge20), flag(edge01), 0.0];
    let mask0 = order_mask(coordinates, i0, v0);
    if mask0 == 0 {
        return Some(i0);
    }
    let v1 = [0.0, flag(edge01), flag(edge12)];
    let mask1 = order_mask(coordinates, i1, v1);
    if mask1 == 0 {
        return Some(i1);
    }
    let v2 = [flag(edge20), 0.0, flag(edge12)];
    let mask2 = order_mask(coordinates, i2, v2);
    if mask2 == 0 {
        return Some(i2);
    }
    let working = mask0 & mask1 & mask2;
    // Positions of a, b and c for the first working order: abc, acb, bac, bca, cab, cba.
    const ORDERS: [[usize; 3]; 6] = [[0, 1, 2], [0, 2, 1], [1, 0, 2], [2, 0, 1], [1, 2, 0], [2, 1, 0]];
    let Some(order) = (0..6).find(|bit| working & (1 << bit) != 0).map(|bit| ORDERS[bit]) else {
        // Copy the most constrained vertex: the one with the fewest valid orders.
        let (c0, c1, c2) = (mask0.count_ones(), mask1.count_ones(), mask2.count_ones());
        return Some(if c0 < c1 && c0 < c2 {
            i0
        } else if c1 < c2 {
            i1
        } else {
            i2
        });
    };
    for (vertex, values) in [(i0, v0), (i1, v1), (i2, v2)] {
        // A NaN vertex names a non-index property, not an element.
        let Some(start) = element(vertex).map(|v| v * 3) else {
            continue;
        };
        if coordinates.len() < start + 3 {
            coordinates.resize(start + 3, None);
        }
        for (slot, value) in order.iter().zip(values) {
            coordinates[start + slot] = Some(value);
        }
    }
    None
}

fn element(vertex: f64) -> Option<usize> {
    (vertex >= 0.0 && vertex.fract() == 0.0).then_some(vertex as usize)
}

fn order_mask(coordinates: &[Option<f64>], vertex: f64, [a, b, c]: [f64; 3]) -> u32 {
    let Some(start) = element(vertex).map(|v| v * 3) else {
        return 0b111111;
    };
    let get = |i: usize| coordinates.get(start + i).copied().flatten();
    let Some(first) = get(0) else {
        return 0b111111;
    };
    let (second, third) = (get(1).unwrap_or(f64::NAN), get(2).unwrap_or(f64::NAN));
    let is = |x: f64, y: f64, z: f64| first == x && second == y && third == z;
    u32::from(is(a, b, c))
        | u32::from(is(a, c, b)) << 1
        | u32::from(is(b, a, c)) << 2
        | u32::from(is(b, c, a)) << 3
        | u32::from(is(c, a, b)) << 4
        | u32::from(is(c, b, a)) << 5
}

fn create_mip_level(size: u32) -> Vec<u8> {
    let mut texture = vec![0u8; size as usize];
    // A 0.75 px outline, attenuated near the top of the mip pyramid.
    texture[size as usize - 1] = match size {
        8 => 96,
        4 => 48,
        2 => 24,
        1 => 12,
        _ => 192,
    };
    texture
}

/// `IndexDatatype.fromTypedArray` for updated triangle indices.
pub fn index_datatype_of(indices: &TypedArray) -> Option<IndexDatatype> {
    match indices {
        TypedArray::Uint8(_) => Some(IndexDatatype::UnsignedByte),
        TypedArray::Uint16(_) => Some(IndexDatatype::UnsignedShort),
        TypedArray::Uint32(_) => Some(IndexDatatype::UnsignedInt),
        _ => None,
    }
}
