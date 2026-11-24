//! CesiumJS `GaussianSplat3DTileContent`: glTF Gaussian-splat tile content.
//!
//! The pinned class wraps `GltfLoader` and exposes the first primitive's
//! position, rotation, scale and spherical-harmonics attributes. This port takes
//! the loaded primitive's attributes directly; the glTF loader and tileset
//! factory pass them in. Attribute extraction, spherical-harmonic degree and
//! coefficient detection, half-float packing, transforms, metrics and the
//! content contract are ported. `isDestroyed` returns the boolean property, as
//! the interface intends, rather than the pinned typo that returns the function.

use super::gaussian_splat_primitive::{transform_splat_attributes, GaussianSplatTileContent};
use crate::{
    core::{Cartesian3, Matrix4, RuntimeError},
    scene::{Axis, VertexAttributeSemantic},
};
use std::rc::Rc;

/// One glTF attribute as the Gaussian-splat content reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct GaussianSplatAttribute {
    pub name: String,
    /// Components in the attribute's order (three per SH coefficient vector).
    pub values: Vec<f32>,
    pub count: usize,
    pub byte_length: usize,
}

impl GaussianSplatAttribute {
    pub fn new(name: impl Into<String>, values: Vec<f32>, count: usize) -> Self {
        let byte_length = values.len() * 4;
        Self { name: name.into(), values, count, byte_length }
    }
}

/// The payload a glTF loader gives a Gaussian-splat primitive.
#[derive(Clone, Debug, PartialEq)]
pub struct GaussianSplatPrimitiveData {
    pub attributes: Vec<GaussianSplatAttribute>,
    /// `scene.nodes[0].matrix`.
    pub world_transform: Matrix4,
    /// `scene.nodes[0].primitives[0]` transforms' combined matrix. The pinned
    /// content reads the node matrix; primitive transforms are already applied
    /// by the loader's attribute data for splats.
    pub node_matrix: Matrix4,
}

impl Default for GaussianSplatPrimitiveData {
    fn default() -> Self {
        Self { attributes: Vec::new(), world_transform: Matrix4::IDENTITY, node_matrix: Matrix4::IDENTITY }
    }
}

fn attribute<'a>(attributes: &'a [GaussianSplatAttribute], name: &str) -> Option<&'a GaussianSplatAttribute> {
    attributes.iter().find(|a| a.name == name)
}

/// Cesium's `ModelUtility.getAttributeBySemantic` for the three splat
/// attributes, including the pre-ratification `_POSITION`/`_ROTATION`/`_SCALE`
/// names.
fn attribute_by_semantic(
    attributes: &[GaussianSplatAttribute],
    semantic: VertexAttributeSemantic,
) -> Result<&GaussianSplatAttribute, RuntimeError> {
    let names: &[&str] = match semantic {
        VertexAttributeSemantic::Position => &["POSITION", "_POSITION", "KHR_gaussian_splatting:POSITION"],
        VertexAttributeSemantic::Rotation => &["ROTATION", "_ROTATION", "KHR_gaussian_splatting:ROTATION"],
        VertexAttributeSemantic::Scale => &["SCALE", "_SCALE", "KHR_gaussian_splatting:SCALE"],
        VertexAttributeSemantic::Color => &["COLOR_0", "_COLOR_0", "KHR_gaussian_splatting:COLOR_0"],
        _ => &[],
    };
    names
        .iter()
        .find_map(|name| attribute(attributes, name))
        .ok_or_else(|| RuntimeError::new("Missing required Gaussian splat attribute"))
}

/// `GaussianSplat3DTileContent.degreeAndCoefFromAttributes`.
pub fn degree_and_coef_from_attributes(attributes: &[GaussianSplatAttribute]) -> (u32, usize) {
    let count = attributes.iter().filter(|a| a.name.contains("SH_DEGREE_")).count();
    match count {
        3 => (1, 9),
        8 => (2, 24),
        15 => (3, 45),
        _ => (0, 0),
    }
}

/// The pinned `float32ToFloat16`: sign, exponent and truncated mantissa, with
/// overflow to infinity and subnormals flushed to signed zero.
pub fn float32_to_float16(float32: f32) -> u16 {
    let bits = float32.to_bits();
    let sign = (bits >> 31) & 0x1;
    let exponent = (bits >> 23) & 0xff;
    let mantissa = bits & 0x7f_ffff;
    let half = if exponent == 0xff {
        (sign << 15) | (0x1f << 10) | (u32::from(mantissa != 0) * 0x200)
    } else if exponent == 0 {
        sign << 15
    } else {
        let new_exponent = exponent as i32 - 127 + 15;
        if new_exponent >= 31 {
            (sign << 15) | (0x1f << 10)
        } else if new_exponent <= 0 {
            sign << 15
        } else {
            (sign << 15) | ((new_exponent as u32) << 10) | (mantissa >> 13)
        }
    };
    half as u16
}

fn get_sh_attribute_prefix(attribute: &str) -> &'static str {
    if attribute.starts_with("KHR_gaussian_splatting:") {
        "KHR_gaussian_splatting:SH_DEGREE_"
    } else {
        "_SH_DEGREE_"
    }
}

/// `extractSHDegreeAndCoef`. JavaScript's `parseInt` yields `NaN` when the
/// degree or coefficient text is missing, so the result is floating point.
pub fn extract_sh_degree_and_coef(attribute: &str) -> (f64, f64) {
    let prefix = get_sh_attribute_prefix(attribute);
    let separator = "_COEF_";
    let l_start = prefix.len();
    let coef_index = attribute[l_start..].find(separator).map(|i| i + l_start);
    let Some(coef_index) = coef_index else {
        // `slice(lStart, -1)` and `slice(coefIndex + 6)` never parse.
        return (f64::NAN, f64::NAN);
    };
    let l = attribute.get(l_start..coef_index).and_then(parse_js_int).unwrap_or(f64::NAN);
    let n = attribute[coef_index + separator.len()..].parse::<f64>().unwrap_or(f64::NAN);
    (l, n)
}

fn parse_js_int(text: &str) -> Option<f64> {
    let trimmed = text.trim_start();
    let mut end = 0;
    let bytes = trimmed.as_bytes();
    if bytes.first() == Some(&b'-') || bytes.first() == Some(&b'+') {
        end = 1;
    }
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == 0 || (end == 1 && (bytes[0] == b'-' || bytes[0] == b'+')) {
        return None;
    }
    trimmed[..end].parse::<f64>().ok()
}

/// `GaussianSplat3DTileContent.packSphericalHarmonicsData`.
pub fn pack_spherical_harmonics_data(
    points_length: usize,
    degree: u32,
    coefficient_count: usize,
    attributes: &[GaussianSplatAttribute],
) -> Vec<u32> {
    let total_length = (points_length as f64 * (coefficient_count as f64 * (2.0 / 3.0))) as usize;
    let mut packed = vec![0u32; total_length];

    let mut sh_attributes: Vec<&GaussianSplatAttribute> =
        attributes.iter().filter(|a| a.name.contains("SH_DEGREE_")).collect();
    let stride = match degree {
        1 => 9,
        2 => 24,
        3 => 45,
        _ => 0,
    };
    sh_attributes.sort_by(|a, b| a.name.cmp(&b.name));
    let packed_stride = (stride as f64 * (2.0 / 3.0)) as usize;
    let base = [0usize, 9, 24];
    for attribute in sh_attributes {
        let (l, n) = extract_sh_degree_and_coef(&attribute.name);
        if !l.is_finite() || l < 1.0 || !n.is_finite() || n < 0.0 {
            continue;
        }
        let l = l as usize;
        let n = n as usize;
        if l > base.len() {
            continue;
        }
        for j in 0..points_length {
            let packed_base = (base[l - 1] * 2) / 3;
            let index = j * packed_stride + packed_base + n * 2;
            let src = j * 3;
            if index + 1 >= packed.len() || src + 2 >= attribute.values.len() {
                continue;
            }
            packed[index] = u32::from(float32_to_float16(attribute.values[src]))
                | (u32::from(float32_to_float16(attribute.values[src + 1])) << 16);
            packed[index + 1] = u32::from(float32_to_float16(attribute.values[src + 2]));
        }
    }
    packed
}

/// glTF Gaussian-splat tile content.
#[derive(Clone, Debug, PartialEq)]
pub struct GaussianSplat3DTileContent {
    url: String,
    primitive: GaussianSplatPrimitiveData,
    attributes: Vec<GaussianSplatAttribute>,
    positions: Vec<f32>,
    rotations: Vec<f32>,
    scales: Vec<f32>,
    colors: Vec<u8>,
    spherical_harmonics_degree: u32,
    spherical_harmonics_coefficient_count: usize,
    packed_spherical_harmonics_data: Option<Vec<u32>>,
    ready: bool,
    transformed: bool,
    feature_properties_dirty: bool,
    metadata: Option<serde_json::Value>,
    group: Option<serde_json::Value>,
    resources_loaded: bool,
    destroyed: bool,
}

impl GaussianSplat3DTileContent {
    /// Builds content from a loaded glTF `ModelComponents::Components` object
    /// (matching `GaussianSplat3DTileContent.fromGltf`'s attribute extraction).
    pub fn from_gltf_components<G: crate::renderer::Gl>(
        url: impl Into<String>,
        components: &super::model_components::Components<G>,
    ) -> Result<Self, RuntimeError> {
        let node = components
            .scene
            .as_ref()
            .and_then(|scene| scene.nodes.first().copied())
            .and_then(|idx| components.nodes.get(idx))
            .or_else(|| components.nodes.first())
            .ok_or_else(|| RuntimeError::new("glTF Gaussian splat document has no nodes"))?;
        let primitive = node
            .primitives
            .first()
            .ok_or_else(|| RuntimeError::new("glTF Gaussian splat node has no primitives"))?;
        let mut attributes = Vec::with_capacity(primitive.attributes.len());
        for attr in &primitive.attributes {
            let name = attr
                .name
                .clone()
                .or_else(|| attr.semantic.map(|s| s.as_str().to_string()))
                .unwrap_or_default();
            let Some(typed) = &attr.typed_array else {
                continue;
            };
            let values: Vec<f32> = typed.to_f64_vec().into_iter().map(|v| v as f32).collect();
            let count = attr.count.unwrap_or(0);
            let mut gs_attr = GaussianSplatAttribute::new(name, values, count);
            gs_attr.byte_length = typed.len() * typed.component_datatype().get_size_in_bytes();
            attributes.push(gs_attr);
        }
        let node_matrix = node.matrix.unwrap_or(Matrix4::IDENTITY);
        Self::from_primitive(
            url,
            GaussianSplatPrimitiveData {
                attributes,
                world_transform: components.transform,
                node_matrix,
            },
        )
    }

    /// Builds content from a loaded glTF primitive's attributes.
    pub fn from_primitive(url: impl Into<String>, primitive: GaussianSplatPrimitiveData) -> Result<Self, RuntimeError> {
        let attributes = primitive.attributes.clone();
        let positions = attribute_by_semantic(&attributes, VertexAttributeSemantic::Position)?.values.clone();
        let rotations = attribute_by_semantic(&attributes, VertexAttributeSemantic::Rotation)?.values.clone();
        let scales = attribute_by_semantic(&attributes, VertexAttributeSemantic::Scale)?.values.clone();
        let colors_attribute = attribute(&attributes, "COLOR_0")
            .or_else(|| attribute(&attributes, "_COLOR_0"))
            .or_else(|| attribute(&attributes, "KHR_gaussian_splatting:COLOR_0"));
        let colors = colors_attribute
            .map(|attribute| attribute.values.iter().map(|&value| value.clamp(0.0, 255.0).round() as u8).collect())
            .unwrap_or_default();
        let (degree, coefficient_count) = degree_and_coef_from_attributes(&attributes);
        let mut content = Self {
            url: url.into(),
            primitive,
            attributes,
            positions,
            rotations,
            scales,
            colors,
            spherical_harmonics_degree: degree,
            spherical_harmonics_coefficient_count: coefficient_count,
            packed_spherical_harmonics_data: None,
            ready: false,
            transformed: false,
            feature_properties_dirty: false,
            metadata: None,
            group: None,
            resources_loaded: true,
            destroyed: false,
        };
        content.packed_spherical_harmonics_data = Some(pack_spherical_harmonics_data(
            content.points_length(),
            content.spherical_harmonics_degree,
            content.spherical_harmonics_coefficient_count,
            &content.attributes,
        ));
        Ok(content)
    }

    /// Cesium's `tilesetRequiresGaussianSplattingExt`, without the deprecation
    /// warning for the removed original extension.
    pub fn tileset_requires_gaussian_splat_extensions(required: &[String]) -> bool {
        required.iter().any(|ext| ext == "KHR_gaussian_splatting")
            && required.iter().any(|ext| ext == "KHR_gaussian_splatting_compression_spz_2")
    }

    pub fn features_length(&self) -> usize {
        0
    }
    pub fn points_length(&self) -> usize {
        self.primitive.attributes.first().map_or(0, |attribute| attribute.count)
    }
    pub fn triangles_length(&self) -> usize {
        0
    }
    pub fn geometry_byte_length(&self) -> usize {
        self.primitive.attributes.iter().map(|a| a.byte_length).sum()
    }
    pub fn textures_byte_length(&self) -> usize {
        0
    }
    pub fn batch_table_byte_length(&self) -> usize {
        0
    }
    pub fn inner_contents(&self) -> Option<&[()]> {
        None
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn transformed(&self) -> bool {
        self.transformed
    }
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn batch_table(&self) -> Option<()> {
        None
    }
    pub fn metadata(&self) -> Option<&serde_json::Value> {
        self.metadata.as_ref()
    }
    pub fn set_metadata(&mut self, value: Option<serde_json::Value>) {
        self.metadata = value;
    }
    pub fn group(&self) -> Option<&serde_json::Value> {
        self.group.as_ref()
    }
    pub fn set_group(&mut self, value: Option<serde_json::Value>) {
        self.group = value;
    }
    pub fn positions(&self) -> &[f32] {
        &self.positions
    }
    pub fn rotations(&self) -> &[f32] {
        &self.rotations
    }
    pub fn scales(&self) -> &[f32] {
        &self.scales
    }
    pub fn colors(&self) -> &[u8] {
        &self.colors
    }
    pub fn spherical_harmonics_degree(&self) -> u32 {
        self.spherical_harmonics_degree
    }
    pub fn spherical_harmonics_coefficient_count(&self) -> usize {
        self.spherical_harmonics_coefficient_count
    }
    pub fn packed_spherical_harmonics_data(&self) -> Option<&[u32]> {
        self.packed_spherical_harmonics_data.as_deref()
    }
    pub fn has_property(&self, _batch_id: usize, _name: &str) -> bool {
        false
    }
    pub fn get_feature(&self, _batch_id: usize) -> Option<()> {
        None
    }
    pub fn apply_debug_settings(&mut self, _enabled: bool, _color: crate::core::Color) {}
    pub fn apply_style(&mut self, _style: Option<&super::Cesium3DTileStyle>) {}
    pub fn pick(&self, _ray: &crate::core::Ray, _frame: &crate::scene::FrameState) -> Option<Cartesian3> {
        None
    }

    /// Cesium's `update`; the first call loads resources, then marks ready.
    pub fn update(&mut self, _frame: &mut crate::scene::FrameState) -> bool {
        if self.destroyed {
            return false;
        }
        if self.ready {
            return true;
        }
        if !self.resources_loaded {
            self.resources_loaded = true;
            return false;
        }
        self.ready = true;
        true
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        self.attributes.clear();
        self.positions.clear();
        self.rotations.clear();
        self.scales.clear();
        self.colors.clear();
        self.packed_spherical_harmonics_data = None;
        self.ready = false;
        self.destroyed = true;
        Ok(())
    }

    /// Transforms the splat attributes to world space, as
    /// `GaussianSplatPrimitive.transformTile`.
    pub fn transform(
        &mut self,
        computed_transform: &Matrix4,
        world_transform: &Matrix4,
        tileset_model_matrix: &Matrix4,
        root_transform: &Matrix4,
        axis_correction: &Matrix4,
    ) {
        if self.destroyed {
            return;
        }
        let mut positions = attribute_by_semantic(&self.attributes, VertexAttributeSemantic::Position)
            .expect("validated splat positions")
            .values
            .clone();
        let mut scales = attribute_by_semantic(&self.attributes, VertexAttributeSemantic::Scale)
            .expect("validated splat scales")
            .values
            .clone();
        if transform_splat_attributes(
            &mut positions,
            &self.rotations,
            &mut scales,
            computed_transform,
            world_transform,
            tileset_model_matrix,
            root_transform,
            axis_correction,
        ) {
            self.positions = positions;
            self.scales = scales;
            self.transformed = true;
        }
    }
}

impl GaussianSplatTileContent for GaussianSplat3DTileContent {
    fn points_length(&self) -> usize {
        GaussianSplat3DTileContent::points_length(self)
    }
    fn positions(&self) -> &[f32] {
        &self.positions
    }
    fn rotations(&self) -> &[f32] {
        &self.rotations
    }
    fn scales(&self) -> &[f32] {
        &self.scales
    }
    fn colors(&self) -> &[u8] {
        &self.colors
    }
    fn spherical_harmonics_degree(&self) -> u32 {
        self.spherical_harmonics_degree
    }
    fn spherical_harmonics_coefficient_count(&self) -> usize {
        self.spherical_harmonics_coefficient_count
    }
    fn packed_spherical_harmonics_data(&self) -> Option<&[u32]> {
        self.packed_spherical_harmonics_data.as_deref()
    }
    fn is_ready(&self) -> bool {
        self.ready
    }
}

/// `Axis` is re-exported here because the pinned content constructs loaders with
/// `upAxis: Axis.Y, forwardAxis: Axis.Z`.
pub const GLTF_UP_AXIS: Axis = Axis::Y;
pub const GLTF_FORWARD_AXIS: Axis = Axis::Z;

/// A shared reference to content, as tiles hold their content.
pub type GaussianSplatContentRef = Rc<dyn GaussianSplatTileContent>;

/// The attribute names a splat primitive must provide for a content position.
pub fn required_semantics() -> [&'static str; 3] {
    ["POSITION", "ROTATION", "SCALE"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_float_matches_pinned_conversion() {
        assert_eq!(float32_to_float16(1.0), 0x3c00);
        assert_eq!(float32_to_float16(-2.0), 0xc000);
        assert_eq!(float32_to_float16(65536.0), 0x7c00);
        assert_eq!(float32_to_float16(0.0), 0);
        assert_eq!(float32_to_float16(-0.0), 0x8000);
        assert_eq!(float32_to_float16(f32::NAN) & 0x7c00, 0x7c00);
    }

    #[test]
    fn degree_detection_matches_attribute_counts() {
        let names = |count: usize| {
            (0..count)
                .map(|i| GaussianSplatAttribute::new(format!("_SH_DEGREE_1_COEF_{i}"), vec![0.0; 3], 1))
                .collect::<Vec<_>>()
        };
        assert_eq!(degree_and_coef_from_attributes(&names(0)), (0, 0));
        assert_eq!(degree_and_coef_from_attributes(&names(3)), (1, 9));
        assert_eq!(degree_and_coef_from_attributes(&names(8)), (2, 24));
        assert_eq!(degree_and_coef_from_attributes(&names(15)), (3, 45));
        assert_eq!(degree_and_coef_from_attributes(&names(7)), (0, 0));
    }

    #[test]
    fn degree_one_packing_interleaves_two_half_floats() {
        let attributes = vec![
            GaussianSplatAttribute::new("_SH_DEGREE_1_COEF_0", vec![1.0, 2.0, 3.0], 1),
            GaussianSplatAttribute::new("_SH_DEGREE_1_COEF_1", vec![4.0, 5.0, 6.0], 1),
            GaussianSplatAttribute::new("_SH_DEGREE_1_COEF_2", vec![7.0, 8.0, 9.0], 1),
        ];
        let packed = pack_spherical_harmonics_data(1, 1, 9, &attributes);
        assert_eq!(packed.len(), 6);
        assert_eq!(packed[0], u32::from(float32_to_float16(1.0)) | (u32::from(float32_to_float16(2.0)) << 16));
        assert_eq!(packed[1], u32::from(float32_to_float16(3.0)));
    }
}
