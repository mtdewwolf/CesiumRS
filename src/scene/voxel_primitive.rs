//! CesiumJS `VoxelPrimitive`: a renderable voxel shape backed by a
//! [`VoxelProvider`](super::VoxelProvider), with traversal, picking and GPU
//! megatextures.
use super::{
    build_voxel_draw_commands::build_voxel_draw_commands,
    process_voxel_properties::{process_voxel_properties, VoxelPropertyInfo},
    scene::ScenePrimitive,
    voxel_provider::VoxelProvider,
    voxel_render_resources::{VoxelRenderResources, VoxelRenderResourcesOptions},
    voxel_traversal::{VoxelTraversal, VoxelTraversalEvents, VoxelTraversalSettings},
    model::{CustomShader, CustomShaderOptions, UniformType},
    Cesium3DTilesetStatistics, ClippingPlaneCollection, FrameState, MetadataComponentType, MetadataType, SceneCommand,
    SceneDrawOptions, VoxelContent, VoxelMetadataOrder, VoxelRequest, VoxelShape, VoxelShapeType,
};
use crate::{
    core::{
        vertical_exaggeration::get_height, Cartesian3, Cartesian4, Clock, Event, JulianDate, Matrix3, Matrix4,
        OrientedBoundingBox, RuntimeError, TimeIntervalCollection,
    },
    renderer::{
        Context, Gl, PickId, SampledTexture, ShaderProgram, ShaderSource, UniformFrustum, UniformValue,
    },
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
type Result<T> = std::result::Result<T, RuntimeError>;

const DEFAULT_FRAGMENT_SHADER_TEXT: &str = "void fragmentMain(FragmentInput fsInput, inout czm_modelMaterial material)
{
    material.diffuse = vec3(1.0);
    material.alpha = 1.0;
}";

/// Constructor options. `provider` defaults to a 1x1 scalar box.
pub struct VoxelPrimitiveOptions<G: Gl> {
    pub provider: Option<Box<dyn VoxelProvider>>,
    pub model_matrix: Matrix4,
    pub custom_shader: Option<CustomShader<G>>,
    pub clock: Option<Clock>,
    pub calculate_statistics: bool,
}
impl<G: Gl> Default for VoxelPrimitiveOptions<G> {
    fn default() -> Self {
        Self {
            provider: None,
            model_matrix: Matrix4::IDENTITY,
            custom_shader: None,
            clock: None,
            calculate_statistics: false,
        }
    }
}

/// `DefaultVoxelProvider`: one scalar float box tile.
pub struct DefaultVoxelProvider {
    transform: Matrix4,
    dimensions: Cartesian3,
    names: Vec<String>,
    types: Vec<MetadataType>,
    component_types: Vec<MetadataComponentType>,
}
impl Default for DefaultVoxelProvider {
    fn default() -> Self {
        Self {
            transform: Matrix4::IDENTITY,
            dimensions: Cartesian3::new(1., 1., 1.),
            names: vec!["data".into()],
            types: vec![MetadataType::Scalar],
            component_types: vec![MetadataComponentType::Float32],
        }
    }
}
impl VoxelProvider for DefaultVoxelProvider {
    fn global_transform(&self) -> &Matrix4 {
        &self.transform
    }
    fn shape_transform(&self) -> &Matrix4 {
        &self.transform
    }
    fn shape(&self) -> VoxelShapeType {
        VoxelShapeType::Box
    }
    fn min_bounds(&self) -> Option<Cartesian3> {
        None
    }
    fn max_bounds(&self) -> Option<Cartesian3> {
        None
    }
    fn dimensions(&self) -> Cartesian3 {
        self.dimensions
    }
    fn padding_before(&self) -> Cartesian3 {
        Cartesian3::ZERO
    }
    fn padding_after(&self) -> Cartesian3 {
        Cartesian3::ZERO
    }
    fn names(&self) -> &[String] {
        &self.names
    }
    fn types(&self) -> &[MetadataType] {
        &self.types
    }
    fn component_types(&self) -> &[MetadataComponentType] {
        &self.component_types
    }
    fn minimum_values(&self) -> Option<&[Vec<f64>]> {
        None
    }
    fn maximum_values(&self) -> Option<&[Vec<f64>]> {
        None
    }
    fn maximum_tile_count(&self) -> Option<usize> {
        Some(1)
    }
    fn available_levels(&self) -> Option<u32> {
        Some(1)
    }
    fn keyframe_count(&self) -> Option<u32> {
        Some(1)
    }
    fn time_interval_collection(&self) -> Option<&TimeIntervalCollection<u32>> {
        None
    }
    fn request_data(&mut self, options: VoxelRequest) -> Option<super::voxel_provider::VoxelFuture> {
        if options.tile_level >= 1 {
            return None;
        }
        Some(Box::pin(async {
            Ok(VoxelContent::from_metadata_array(vec![crate::core::TypedArray::Float32(vec![1.0])]))
        }))
    }
}

/// Primitive-owned shared state read by the draw's uniform map.
#[derive(Default)]
struct VoxelUniforms {
    values: BTreeMap<String, UniformValue>,
}

struct VoxelPickObject {
    #[allow(dead_code)]
    id: u64,
}

static NEXT_PRIMITIVE_ID: AtomicU64 = AtomicU64::new(1);

/// A voxel renderable.
pub struct VoxelPrimitive<G: Gl + 'static> {
    id: u64,
    ready: bool,
    provider: Box<dyn VoxelProvider>,
    traversal: Option<VoxelTraversal<G>>,
    statistics: Cesium3DTilesetStatistics,
    calculate_statistics: bool,
    shape: Option<Box<dyn VoxelShape>>,
    shape_visible: bool,
    dimensions: Cartesian3,
    input_dimensions: Cartesian3,
    padding_before: Cartesian3,
    padding_after: Cartesian3,
    available_levels: u32,
    min_bounds: Cartesian3,
    min_bounds_old: Cartesian3,
    max_bounds: Cartesian3,
    max_bounds_old: Cartesian3,
    min_clipping_bounds: Cartesian3,
    min_clipping_bounds_old: Cartesian3,
    max_clipping_bounds: Cartesian3,
    max_clipping_bounds_old: Cartesian3,
    vertical_exaggeration: f64,
    vertical_exaggeration_relative_height: f64,
    clipping_planes: Option<Rc<RefCell<ClippingPlaneCollection>>>,
    clipping_planes_state: isize,
    clipping_planes_enabled: bool,
    model_matrix: Matrix4,
    model_matrix_old: Matrix4,
    custom_shader: CustomShader<G>,
    custom_shader_compilation_event: Event<()>,
    shader_dirty: bool,
    draw_command: Option<SceneCommand>,
    draw_command_pick: Option<SceneCommand>,
    draw_command_pick_voxel: Option<SceneCommand>,
    pick_id: Option<PickId>,
    clock: Option<Clock>,
    transform_position_local_to_world: Matrix4,
    transform_position_world_to_local: Matrix4,
    transform_plane_local_to_view: Matrix4,
    transform_direction_world_to_local: Matrix3,
    nearest_sampling: bool,
    level_blend_factor: f64,
    step_size_multiplier: f64,
    depth_test: bool,
    use_log_depth: Option<bool>,
    screen_space_error: f64,
    debug_draw: bool,
    disable_render: bool,
    disable_update: bool,
    uniforms: Rc<RefCell<VoxelUniforms>>,
    shape_defines_old: BTreeMap<String, String>,
    uniform_map: BTreeMap<String, UniformValue>,
    textures: Vec<(String, Rc<dyn SampledTexture<G>>)>,
    pub tile_load: Event<()>,
    pub tile_visible: Event<()>,
    pub tile_failed: Event<()>,
    pub tile_unload: Event<()>,
    pub load_progress: Event<(usize, usize)>,
    pub all_tiles_loaded: Event<()>,
    pub initial_tiles_loaded: Event<()>,
    destroyed: bool,
}

impl<G: Gl> VoxelPrimitive<G> {
    pub fn new(options: VoxelPrimitiveOptions<G>) -> Result<Self> {
        let provider = options.provider.unwrap_or_else(|| Box::new(DefaultVoxelProvider::default()));
        let custom_shader = match options.custom_shader {
            Some(shader) => shader,
            None => CustomShader::new(CustomShaderOptions {
                fragment_shader_text: Some(DEFAULT_FRAGMENT_SHADER_TEXT.into()),
                ..Default::default()
            })?,
        };
        let mut primitive = Self {
            id: NEXT_PRIMITIVE_ID.fetch_add(1, Ordering::Relaxed),
            ready: false,
            provider,
            traversal: None,
            statistics: Cesium3DTilesetStatistics::new(),
            calculate_statistics: options.calculate_statistics,
            shape: None,
            shape_visible: false,
            dimensions: Cartesian3::ZERO,
            input_dimensions: Cartesian3::ZERO,
            padding_before: Cartesian3::ZERO,
            padding_after: Cartesian3::ZERO,
            available_levels: 1,
            min_bounds: Cartesian3::ZERO,
            min_bounds_old: Cartesian3::ZERO,
            max_bounds: Cartesian3::ZERO,
            max_bounds_old: Cartesian3::ZERO,
            min_clipping_bounds: Cartesian3::ZERO,
            min_clipping_bounds_old: Cartesian3::ZERO,
            max_clipping_bounds: Cartesian3::ZERO,
            max_clipping_bounds_old: Cartesian3::ZERO,
            vertical_exaggeration: 1.0,
            vertical_exaggeration_relative_height: 0.0,
            clipping_planes: None,
            clipping_planes_state: 0,
            clipping_planes_enabled: false,
            model_matrix: options.model_matrix,
            model_matrix_old: options.model_matrix,
            custom_shader,
            custom_shader_compilation_event: Event::new(),
            shader_dirty: true,
            draw_command: None,
            draw_command_pick: None,
            draw_command_pick_voxel: None,
            pick_id: None,
            clock: options.clock,
            transform_position_local_to_world: Matrix4::IDENTITY,
            transform_position_world_to_local: Matrix4::IDENTITY,
            transform_plane_local_to_view: Matrix4::IDENTITY,
            transform_direction_world_to_local: Matrix3::IDENTITY,
            nearest_sampling: false,
            level_blend_factor: 0.0,
            step_size_multiplier: 1.0,
            depth_test: true,
            use_log_depth: None,
            screen_space_error: 4.0,
            debug_draw: false,
            disable_render: false,
            disable_update: false,
            uniforms: Rc::new(RefCell::new(VoxelUniforms::default())),
            shape_defines_old: BTreeMap::new(),
            uniform_map: BTreeMap::new(),
            textures: Vec::new(),
            tile_load: Event::new(),
            tile_visible: Event::new(),
            tile_failed: Event::new(),
            tile_unload: Event::new(),
            load_progress: Event::new(),
            all_tiles_loaded: Event::new(),
            initial_tiles_loaded: Event::new(),
            destroyed: false,
        };
        primitive.initialize();
        Ok(primitive)
    }

    fn initialize(&mut self) {
        let shape_type = self.provider.shape();
        self.min_bounds = self.provider.min_bounds().unwrap_or_else(|| shape_type.min_bounds());
        self.max_bounds = self.provider.max_bounds().unwrap_or_else(|| shape_type.max_bounds());
        self.min_clipping_bounds = self.min_bounds;
        self.max_clipping_bounds = self.max_bounds;
        self.check_transform_and_bounds();
        self.shape = Some(shape_type.create_shape());
        self.shape_visible = self.update_shape_and_transforms();
    }

    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn provider(&self) -> &dyn VoxelProvider {
        self.provider.as_ref()
    }
    pub fn shape(&self) -> VoxelShapeType {
        self.provider.shape()
    }
    pub fn bounding_sphere(&self) -> Option<crate::core::BoundingSphere> {
        self.shape.as_ref().map(|shape| *shape.bounding_sphere())
    }
    pub fn oriented_bounding_box(&self) -> Option<&OrientedBoundingBox> {
        self.shape.as_ref().map(|shape| shape.oriented_bounding_box())
    }
    pub fn model_matrix(&self) -> Matrix4 {
        self.model_matrix
    }
    pub fn set_model_matrix(&mut self, model_matrix: Matrix4) {
        self.model_matrix = model_matrix;
    }
    pub fn dimensions(&self) -> Cartesian3 {
        self.dimensions
    }
    pub fn input_dimensions(&self) -> Cartesian3 {
        self.input_dimensions
    }
    pub fn padding_before(&self) -> Cartesian3 {
        self.padding_before
    }
    pub fn padding_after(&self) -> Cartesian3 {
        self.padding_after
    }
    pub fn minimum_values(&self) -> Option<&[Vec<f64>]> {
        self.provider.minimum_values()
    }
    pub fn maximum_values(&self) -> Option<&[Vec<f64>]> {
        self.provider.maximum_values()
    }
    pub fn show(&self) -> bool {
        !self.disable_render
    }
    pub fn set_show(&mut self, show: bool) {
        self.disable_render = !show;
    }
    pub fn disable_update(&self) -> bool {
        self.disable_update
    }
    pub fn set_disable_update(&mut self, disable: bool) {
        self.disable_update = disable;
    }
    pub fn debug_draw(&self) -> bool {
        self.debug_draw
    }
    pub fn set_debug_draw(&mut self, debug: bool) {
        self.debug_draw = debug;
    }
    pub fn depth_test(&self) -> bool {
        self.depth_test
    }
    pub fn set_depth_test(&mut self, depth_test: bool) {
        if self.depth_test != depth_test {
            self.depth_test = depth_test;
            self.shader_dirty = true;
        }
    }
    pub fn nearest_sampling(&self) -> bool {
        self.nearest_sampling
    }
    pub fn set_nearest_sampling(&mut self, nearest: bool) {
        self.nearest_sampling = nearest;
    }
    pub fn level_blend_factor(&self) -> f64 {
        self.level_blend_factor
    }
    pub fn set_level_blend_factor(&mut self, value: f64) {
        self.level_blend_factor = value.clamp(0.0, 1.0);
    }
    pub fn screen_space_error(&self) -> f64 {
        self.screen_space_error
    }
    pub fn set_screen_space_error(&mut self, value: f64) {
        self.screen_space_error = value;
    }
    pub fn step_size(&self) -> f64 {
        self.step_size_multiplier
    }
    pub fn set_step_size(&mut self, value: f64) {
        self.step_size_multiplier = value;
    }
    pub fn min_bounds(&self) -> Cartesian3 {
        self.min_bounds
    }
    pub fn set_min_bounds(&mut self, value: Cartesian3) {
        self.min_bounds = value;
    }
    pub fn max_bounds(&self) -> Cartesian3 {
        self.max_bounds
    }
    pub fn set_max_bounds(&mut self, value: Cartesian3) {
        self.max_bounds = value;
    }
    pub fn min_clipping_bounds(&self) -> Cartesian3 {
        self.min_clipping_bounds
    }
    pub fn set_min_clipping_bounds(&mut self, value: Cartesian3) {
        self.min_clipping_bounds = value;
    }
    pub fn max_clipping_bounds(&self) -> Cartesian3 {
        self.max_clipping_bounds
    }
    pub fn set_max_clipping_bounds(&mut self, value: Cartesian3) {
        self.max_clipping_bounds = value;
    }
    pub fn clipping_planes(&self) -> Option<&Rc<RefCell<ClippingPlaneCollection>>> {
        self.clipping_planes.as_ref()
    }
    /// `ClippingPlaneCollection.setOwner`: assign this primitive, destroying
    /// any previous owner is the caller's responsibility.
    pub fn set_clipping_planes(&mut self, planes: Option<Rc<RefCell<ClippingPlaneCollection>>>) -> Result<()> {
        if let Some(planes) = &planes {
            planes.borrow_mut().assign_owner(self.id as usize)?;
        }
        self.clipping_planes = planes;
        Ok(())
    }
    pub fn custom_shader(&self) -> &CustomShader<G> {
        &self.custom_shader
    }
    pub fn set_custom_shader(&mut self, custom_shader: Option<CustomShader<G>>) -> Result<()> {
        let Some(custom_shader) = custom_shader else {
            self.custom_shader = CustomShader::new(CustomShaderOptions {
                fragment_shader_text: Some(DEFAULT_FRAGMENT_SHADER_TEXT.into()),
                ..Default::default()
            })?;
            self.shader_dirty = true;
            return Ok(());
        };
        for name in self.custom_shader.uniforms.keys() {
            self.uniform_map.remove(name);
        }
        self.custom_shader = custom_shader;
        self.shader_dirty = true;
        Ok(())
    }
    pub fn custom_shader_compilation_event(&mut self) -> &mut Event<()> {
        &mut self.custom_shader_compilation_event
    }
    pub fn statistics(&self) -> &Cesium3DTilesetStatistics {
        &self.statistics
    }
    pub fn traversal(&self) -> Option<&VoxelTraversal<G>> {
        self.traversal.as_ref()
    }

    fn check_transform_and_bounds(&mut self) -> bool {
        let mut changes = 0;
        if self.model_matrix != self.model_matrix_old {
            self.model_matrix_old = self.model_matrix;
            changes += 1;
        }
        for (value, old) in [
            (self.min_bounds, &mut self.min_bounds_old),
            (self.max_bounds, &mut self.max_bounds_old),
            (self.min_clipping_bounds, &mut self.min_clipping_bounds_old),
            (self.max_clipping_bounds, &mut self.max_clipping_bounds_old),
        ] {
            if value != *old {
                *old = value;
                changes += 1;
            }
        }
        changes > 0
    }

    fn update_shape_and_transforms(&mut self) -> bool {
        let exaggeration = self.vertical_exaggeration;
        let relative = self.vertical_exaggeration_relative_height;
        let mut min = self.min_bounds;
        let mut max = self.max_bounds;
        let mut clip_min = self.min_clipping_bounds;
        let mut clip_max = self.max_clipping_bounds;
        let mut model = self.model_matrix;
        if self.shape() == VoxelShapeType::Ellipsoid {
            min.z = get_height(min.z, exaggeration, relative);
            max.z = get_height(max.z, exaggeration, relative);
            clip_min.z = get_height(clip_min.z, exaggeration, relative);
            clip_max.z = get_height(clip_max.z, exaggeration, relative);
        } else {
            model = model.multiply_by_scale(Cartesian3::new(1.0, 1.0, exaggeration));
            model = model.multiply_by_translation(self.compute_box_exaggeration_translation());
        }
        let provider = &*self.provider;
        let shape_transform = *provider.shape_transform();
        let global_transform = *provider.global_transform();
        let compound = global_transform.multiply_transformation(&model).multiply_transformation(&shape_transform);
        let Some(shape) = &mut self.shape else { return false };
        if !VoxelShape::update(shape.as_mut(), compound, min, max, Some(clip_min), Some(clip_max)) {
            return false;
        }
        self.transform_position_local_to_world = *shape.shape_transform();
        let Ok(world_to_local) = self.transform_position_local_to_world.inverse() else {
            return false;
        };
        self.transform_position_world_to_local = world_to_local;
        self.transform_direction_world_to_local = world_to_local.get_matrix3();
        true
    }

    fn compute_box_exaggeration_translation(&self) -> Cartesian3 {
        let provider = &*self.provider;
        let initial_center = provider.shape_transform().get_translation();
        let intermediate = self.model_matrix.multiply_by_point(initial_center);
        let transformed = provider.global_transform().multiply_by_point(intermediate);
        let center_height = crate::core::Ellipsoid::WGS84
            .cartesian_to_cartographic(transformed)
            .map_or(0.0, |cartographic| cartographic.height);
        let exaggerated = get_height(
            center_height,
            self.vertical_exaggeration,
            self.vertical_exaggeration_relative_height,
        );
        Cartesian3::new(0.0, 0.0, (exaggerated - center_height) / self.vertical_exaggeration)
    }

    fn update_vertical_exaggeration(&mut self, frame: &FrameState) -> bool {
        if self.vertical_exaggeration == frame.vertical_exaggeration
            && self.vertical_exaggeration_relative_height == frame.vertical_exaggeration_relative_height
        {
            return false;
        }
        self.vertical_exaggeration = frame.vertical_exaggeration;
        self.vertical_exaggeration_relative_height = frame.vertical_exaggeration_relative_height;
        true
    }

    fn init_from_provider(&mut self, context: &Context<G>) -> Result<()> {
        let provider = &*self.provider;
        let shape = self.shape.as_ref().ok_or_else(|| RuntimeError::new("Voxel shape is not initialized"))?;
        self.shape_defines_old = shape.shader_defines();
        for (key, value) in shape.shader_uniforms() {
            self.uniform_map.insert(format!("u_{key}"), value);
        }

        self.dimensions = provider.dimensions();
        self.padding_before = provider.padding_before();
        self.padding_after = provider.padding_after();
        self.input_dimensions = self.dimensions + self.padding_before + self.padding_after;
        if provider.metadata_order() == VoxelMetadataOrder::YUp {
            self.input_dimensions = Cartesian3::new(
                self.input_dimensions.x,
                self.input_dimensions.z,
                self.input_dimensions.y,
            );
        }
        self.available_levels = provider.available_levels().unwrap_or(1);
        let keyframe_count = provider.keyframe_count().unwrap_or(1);
        let names_len = provider.names().len();
        let maximum_tile_count = provider
            .maximum_tile_count()
            .or_else(|| provider.available_levels().map(|levels| ((8u64.pow(levels) - 1) / 7) as usize));
        let traversal = VoxelTraversal::new(
            context,
            self.dimensions,
            self.input_dimensions,
            names_len,
            provider.types(),
            provider.component_types(),
            shape.as_ref(),
            keyframe_count,
            None,
            maximum_tile_count,
            self.calculate_statistics,
        )?;
        self.statistics.textures_byte_length = traversal.texture_memory_byte_length;
        let mut uniforms = self.uniforms.borrow_mut();
        uniforms.values.insert(
            "u_octreeInternalNodeTexture".into(),
            UniformValue::Float(vec![]),
        );
        uniforms.values.insert(
            "u_octreeInternalNodeTilesPerRow".into(),
            UniformValue::Float(vec![traversal.internal_node_tiles_per_row as f32]),
        );
        uniforms.values.insert(
            "u_octreeInternalNodeTexelSizeUv".into(),
            UniformValue::Float(vec![
                traversal.internal_node_texel_size_uv.x as f32,
                traversal.internal_node_texel_size_uv.y as f32,
            ]),
        );
        uniforms.values.insert(
            "u_megatextureTileCounts".into(),
            UniformValue::Float(vec![
                traversal.megatextures[0].tile_counts.x as f32,
                traversal.megatextures[0].tile_counts.y as f32,
                traversal.megatextures[0].tile_counts.z as f32,
            ]),
        );
        for (key, value) in [
            ("u_dimensions", self.dimensions),
            ("u_inputDimensions", self.input_dimensions),
            ("u_paddingBefore", self.padding_before),
            ("u_paddingAfter", self.padding_after),
        ] {
            uniforms.values.insert(
                key.into(),
                UniformValue::Float(vec![value.x as f32, value.y as f32, value.z as f32]),
            );
        }
        uniforms.values.insert(
            "u_stepSize".into(),
            UniformValue::Float(vec![self.step_size_multiplier as f32]),
        );
        drop(uniforms);
        self.textures = traversal
            .megatextures
            .iter()
            .enumerate()
            .map(|(index, megatexture)| {
                (
                    format!("u_megatextureTextures[{index}]"),
                    megatexture.texture.clone() as Rc<dyn SampledTexture<G>>,
                )
            })
            .collect();
        self.textures.push((
            "u_octreeInternalNodeTexture".into(),
            traversal.internal_node_texture.clone() as Rc<dyn SampledTexture<G>>,
        ));
        self.traversal = Some(traversal);
        Ok(())
    }

    fn check_shape_defines(&mut self) -> bool {
        let Some(shape) = &self.shape else { return false };
        let defines = shape.shader_defines();
        let changed = defines != self.shape_defines_old;
        if changed {
            self.shape_defines_old = defines;
        }
        changed
    }

    fn rebuild_draw_commands(&mut self, context: &Context<G>) -> Result<()> {
        let Some(traversal) = &self.traversal else { return Ok(()) };
        let Some(shape) = &self.shape else { return Ok(()) };
        let clipping_resolution = self
            .clipping_planes
            .as_ref()
            .filter(|planes| planes.borrow().enabled)
            .map_or([0, 0], |planes| {
                planes
                    .borrow()
                    .texture_resolution(false, context.limits().map_or(2048, |l| l.maximum_texture_size))
            });
        let render_bound_resolution = shape
            .render_bound_plane_handles()
            .first()
            .map(|_| [0, 0])
            .unwrap_or([0, 0]);
        let custom_uniforms: Vec<(String, UniformType)> = self
            .custom_shader
            .uniforms
            .iter()
            .map(|(name, uniform)| (name.clone(), uniform.uniform_type))
            .collect();
        let uniform_names: Vec<String> = self.uniform_map.keys().cloned().collect();
        let mut resources = VoxelRenderResources::new(VoxelRenderResourcesOptions {
            custom_fragment_shader_text: Some(
                self.custom_shader
                    .fragment_shader_text
                    .as_deref()
                    .unwrap_or(DEFAULT_FRAGMENT_SHADER_TEXT),
            ),
            custom_uniforms: &custom_uniforms,
            uniform_names: &uniform_names,
            clipping_planes: self.clipping_planes.clone(),
            shape: shape.as_ref(),
            shape_type: self.provider.shape(),
            metadata_order: self.provider.metadata_order(),
            depth_test: self.depth_test,
            padding_before: self.padding_before,
            padding_after: self.padding_after,
            use_log_depth: self.use_log_depth.unwrap_or(false),
            sample_count: traversal.sample_count.unwrap_or(1),
        });
        process_voxel_properties(
            &mut resources,
            &VoxelPropertyInfo {
                names: self.provider.names().to_vec(),
                types: self.provider.types().to_vec(),
                component_types: self.provider.component_types().to_vec(),
                minimum_values: self.provider.minimum_values().map(<[Vec<f64>]>::to_vec),
                maximum_values: self.provider.maximum_values().map(<[Vec<f64>]>::to_vec),
            },
        )?;
        let commands = build_voxel_draw_commands(&mut resources, clipping_resolution, render_bound_resolution)?;

        let context_clone = context.clone();
        let quad = context.get_viewport_quad_vertex_array()?;
        let make = |sources: (ShaderSource, ShaderSource), pick_only: bool| -> Result<SceneCommand> {
            let program = Rc::new(ShaderProgram::from_sources(
                &context_clone,
                &crate::renderer::ShaderProgramOptions {
                    vertex_shader_source: sources.0,
                    fragment_shader_source: sources.1,
                    attribute_locations: [("position".to_string(), 0u32)].into_iter().collect(),
                },
            )?);
            let mut draw = SceneDrawOptions::new(quad.clone(), program);
            draw.state = commands.render_state.clone();
            draw.primitive_type = crate::core::PrimitiveType::Triangles;
            draw.uniform_map = Some(Rc::new({
                let uniforms = self.uniforms.clone();
                move || Ok(uniforms.borrow().values.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            }));
            draw.textures = self.textures.clone();
            let mut metadata = crate::renderer::DrawMetadata {
                bounding_volume: self.shape.as_ref().map(|shape| *shape.bounding_sphere()),
                model_matrix: Some(self.transform_position_local_to_world),
                cull: self.depth_test,
                occlude: self.depth_test,
                execute_in_closest_frustum: true,
                pick_only,
                owner: Some(format!("VoxelPrimitive({})", self.id)),
                ..Default::default()
            };
            if let Some(pick_id) = &self.pick_id {
                metadata.pick_id = Some(format!("{:?}", pick_id.color));
            }
            Ok(SceneCommand::from_draw(
                format!("VoxelPrimitive({})", self.id),
                context_clone.clone(),
                crate::renderer::Pass::Voxels,
                metadata,
                draw,
            ))
        };
        self.draw_command = Some(make(commands.shader, false)?);
        self.draw_command_pick = Some(make(commands.shader_pick, true)?);
        self.draw_command_pick_voxel = Some(make(commands.shader_pick_voxel, true)?);
        Ok(())
    }

    fn update_nearest_sampling(&mut self) {
        let Some(traversal) = &mut self.traversal else { return };
        for megatexture in &mut traversal.megatextures {
            let _ = megatexture.set_nearest_sampling(self.nearest_sampling);
        }
    }

    fn update_clipping_planes(&mut self, context: &Context<G>) -> Result<bool> {
        let Some(planes) = &self.clipping_planes else { return Ok(false) };
        planes.borrow_mut().update(context)?;
        let (state, enabled) = {
            let planes = planes.borrow();
            (planes.state(), planes.enabled)
        };
        if enabled {
            let Some(shape) = &self.shape else { return Ok(false) };
            let model_inverse = planes.borrow().model_matrix.inverse()?;
            self.transform_plane_local_to_view = model_inverse
                .multiply_transformation(shape.shape_transform())
                .transpose();
        }
        if self.clipping_planes_state == state && self.clipping_planes_enabled == enabled {
            return Ok(false);
        }
        self.clipping_planes_state = state;
        self.clipping_planes_enabled = enabled;
        Ok(true)
    }

    fn update_uniforms(&mut self, frame: &FrameState, context: &Context<G>) -> Result<()> {
        let state = context.uniform_state();
        let view_projection = state.projection.multiply_transformation(&state.view);
        let Some(shape) = &self.shape else { return Ok(()) };
        let ndc = oriented_bounding_box_to_ndc_aabb(shape.oriented_bounding_box(), &view_projection);
        let local = self.transform_position_world_to_local.multiply_transformation(&state.inverse_view);
        self.transform_plane_local_to_view = local.transpose();
        let direction = self.transform_direction_world_to_local.multiply(&state.inverse_view.get_matrix3());
        let camera_position_local = self.transform_position_world_to_local.multiply_by_point(frame.uniform.camera.position);
        let camera_direction_local = self.transform_direction_world_to_local.multiply_by_vector(frame.uniform.camera.direction);
        let camera_tile = get_tile_coordinates(self, camera_position_local, self.available_levels);
        let mut uniforms = self.uniforms.borrow_mut();
        uniforms.values.insert(
            "u_ndcSpaceAxisAlignedBoundingBox".into(),
            UniformValue::Float(vec![ndc.x as f32, ndc.y as f32, ndc.z as f32, ndc.w as f32]),
        );
        uniforms.values.insert(
            "u_transformPositionViewToLocal".into(),
            UniformValue::Matrix {
                dimension: 4,
                values: local.0.iter().map(|v| *v as f32).collect(),
            },
        );
        uniforms.values.insert(
            "u_transformDirectionViewToLocal".into(),
            UniformValue::Matrix {
                dimension: 3,
                values: direction.0.iter().map(|v| *v as f32).collect(),
            },
        );
        uniforms.values.insert(
            "u_cameraPositionLocal".into(),
            UniformValue::Float(vec![
                camera_position_local.x as f32,
                camera_position_local.y as f32,
                camera_position_local.z as f32,
            ]),
        );
        uniforms.values.insert(
            "u_cameraDirectionLocal".into(),
            UniformValue::Float(vec![
                camera_direction_local.x as f32,
                camera_direction_local.y as f32,
                camera_direction_local.z as f32,
            ]),
        );
        uniforms.values.insert(
            "u_cameraTileCoordinates".into(),
            UniformValue::Float(vec![
                camera_tile.x.floor() as f32,
                camera_tile.y.floor() as f32,
                camera_tile.z.floor() as f32,
                camera_tile.w as f32,
            ]),
        );
        uniforms.values.insert(
            "u_cameraTileUv".into(),
            UniformValue::Float(vec![
                (camera_tile.x - camera_tile.x.floor()) as f32,
                (camera_tile.y - camera_tile.y.floor()) as f32,
                (camera_tile.z - camera_tile.z.floor()) as f32,
            ]),
        );
        uniforms.values.insert(
            "u_stepSize".into(),
            UniformValue::Float(vec![self.step_size_multiplier as f32]),
        );
        if let Some(leaf) = self.traversal.as_ref().and_then(|t| t.leaf_node_texture.as_ref()) {
            let traversal = self.traversal.as_ref().unwrap();
            uniforms.values.insert(
                "u_octreeLeafNodeTexelSizeUv".into(),
                UniformValue::Float(vec![
                    traversal.leaf_node_texel_size_uv.x as f32,
                    traversal.leaf_node_texel_size_uv.y as f32,
                ]),
            );
            uniforms.values.insert(
                "u_octreeLeafNodeTilesPerRow".into(),
                UniformValue::Float(vec![traversal.leaf_node_tiles_per_row.unwrap_or(0) as f32]),
            );
            if !self
                .textures
                .iter()
                .any(|(name, _)| name == "u_octreeLeafNodeTexture")
            {
                self.textures.push((
                    "u_octreeLeafNodeTexture".into(),
                    leaf.clone() as Rc<dyn SampledTexture<G>>,
                ));
            }
        }
        if let Some(shape) = &self.shape {
            for (key, value) in shape.shader_uniforms() {
                uniforms.values.insert(format!("u_{key}"), value);
            }
        }
        for (name, specifier) in &self.custom_shader.uniforms {
            if let Some(value) = specifier.value.borrow().to_renderer(specifier.uniform_type) {
                uniforms.values.insert(name.clone(), value);
            }
        }
        Ok(())
    }

    fn choose_command(&self, frame: &FrameState) -> Option<&SceneCommand> {
        if frame.passes.pick {
            self.draw_command_pick.as_ref()
        } else if frame.passes.pick_voxel {
            self.draw_command_pick_voxel.as_ref()
        } else {
            self.draw_command.as_ref()
        }
    }

    /// `getKeyframeLocation`: a possibly fractional interval index.
    pub fn get_keyframe_location(
        time_interval_collection: Option<&TimeIntervalCollection<u32>>,
        clock: Option<&Clock>,
    ) -> f64 {
        let (Some(collection), Some(clock)) = (time_interval_collection, clock) else {
            return 0.0;
        };
        if collection.is_empty() {
            return 0.0;
        }
        let mut date = clock.current_time();
        let index = collection.index_of(&date);
        let (mut interval_index, mut interval) = if index >= 0 {
            (index as usize, collection.get(index as usize).unwrap().clone())
        } else {
            let insertion = (!index) as usize;
            if insertion == collection.len() {
                let interval = collection.get(collection.len() - 1).unwrap().clone();
                date = interval.stop;
                (collection.len() - 1, interval)
            } else {
                let interval = collection.get(insertion).unwrap().clone();
                date = interval.start;
                (insertion, interval)
            }
        };
        let total_seconds = JulianDate::seconds_difference(&interval.stop, &interval.start);
        let seconds_from_start = JulianDate::seconds_difference(&date, &interval.start);
        let t = if total_seconds == 0.0 { 0.0 } else { seconds_from_start / total_seconds };
        if interval_index >= collection.len() {
            interval_index = collection.len() - 1;
        }
        let _ = &mut interval;
        interval_index as f64 + t
    }

    fn emit_selected_command(&self, frame: &mut FrameState) {
        let Some(command) = self.choose_command(frame) else { return };
        frame.command_list.push(command.clone());
    }
}

impl<G: Gl> ScenePrimitive for VoxelPrimitive<G> {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("VoxelPrimitive was destroyed"));
        }
        let context = frame
            .context
            .as_ref()
            .and_then(|resource| resource.get::<Context<G>>())
            .cloned()
            .ok_or_else(|| RuntimeError::new("VoxelPrimitive requires the frame's renderer context"))?;
        if !self.ready {
            if self.pick_id.is_none() {
                let object = Rc::new(VoxelPickObject { id: self.id });
                self.pick_id = Some(context.create_pick_id(object)?);
            }
            self.init_from_provider(&context)?;
            self.ready = true;
            return Ok(());
        }

        let shape_dirty = self.check_transform_and_bounds();
        let exaggeration_changed = self.update_vertical_exaggeration(frame);
        if shape_dirty || exaggeration_changed {
            self.shape_visible = self.update_shape_and_transforms();
            if self.check_shape_defines() {
                self.shader_dirty = true;
            }
        }
        if !self.shape_visible {
            return Ok(());
        }

        let camera_position = frame.uniform.camera.position;
        let camera_frustum = frame.uniform.camera.frustum;
        let inverse_view_rotation = context.uniform_state().inverse_view.get_matrix3();
        let shape = self.shape.as_mut().ok_or_else(|| RuntimeError::new("Voxel shape is missing"))?;
        shape.update_view_transforms_with_camera(camera_position, inverse_view_rotation)?;

        let keyframe_location = Self::get_keyframe_location(
            self.provider.time_interval_collection(),
            self.clock.as_ref(),
        );
        let sample_count_old = self.traversal.as_ref().and_then(|traversal| traversal.sample_count);
        let sse_denominator = match camera_frustum {
            UniformFrustum::Perspective(frustum) => frustum.sse_denominator(),
            _ => f64::NAN,
        };
        let settings = VoxelTraversalSettings {
            level_blend_factor: self.level_blend_factor,
            screen_space_error: self.screen_space_error,
            calculate_statistics: self.calculate_statistics,
            pause_update: self.disable_update,
            recompute_bounding_volumes: shape_dirty,
            frame_number: frame.frame_number,
            camera_position,
            drawing_buffer_height: frame.drawing_buffer_height as f64,
            pixel_ratio: frame.pixel_ratio,
            sse_denominator,
        };
        let culling_volume = frame.culling_volume.clone();
        let shape_ref = self.shape.as_deref().ok_or_else(|| RuntimeError::new("Voxel shape is missing"))?;
        let mut events = VoxelTraversalEvents {
            tile_load: &mut self.tile_load,
            tile_visible: &mut self.tile_visible,
            tile_failed: &mut self.tile_failed,
            tile_unload: &mut self.tile_unload,
            load_progress: &mut self.load_progress,
            all_tiles_loaded: &mut self.all_tiles_loaded,
            initial_tiles_loaded: &mut self.initial_tiles_loaded,
        };
        let waker = frame.render_request.as_ref().map(|request| request.waker());
        let traversal = self
            .traversal
            .as_mut()
            .ok_or_else(|| RuntimeError::new("Voxel traversal is missing"))?;
        traversal.set_keyframe_location(keyframe_location);
        traversal.update(
            frame,
            &culling_volume,
            &mut *self.provider,
            shape_ref,
            settings,
            &mut self.statistics,
            &mut events,
            waker,
        )?;
        if sample_count_old != traversal.sample_count {
            self.shader_dirty = true;
        }
        if !traversal.is_renderable(&traversal.root_node) {
            return Ok(());
        }
        if self.disable_render {
            return Ok(());
        }
        if self.use_log_depth != Some(frame.use_log_depth) {
            self.use_log_depth = Some(frame.use_log_depth);
            self.shader_dirty = true;
        }
        if self.update_clipping_planes(&context)? {
            self.shader_dirty = true;
        }
        if self.shader_dirty {
            self.rebuild_draw_commands(&context)?;
            self.shader_dirty = false;
        }
        self.update_uniforms(frame, &context)?;
        self.update_nearest_sampling();

        let view_projection = context
            .uniform_state()
            .projection
            .multiply_transformation(&context.uniform_state().view);
        let ndc = self
            .shape
            .as_ref()
            .map(|shape| oriented_bounding_box_to_ndc_aabb(shape.oriented_bounding_box(), &view_projection));
        if let Some(ndc) = ndc {
            if ndc.x == 1.0 || ndc.y == 1.0 || ndc.z == -1.0 || ndc.w == -1.0 {
                return Ok(());
            }
        }
        self.emit_selected_command(frame);
        Ok(())
    }

    fn post_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update_for_pass(&mut self, frame: &mut FrameState, _pass: super::Cesium3DTilePass) -> Result<()> {
        self.update(frame)
    }

    fn destroy(&mut self) -> Result<()> {
        self.draw_command = None;
        self.draw_command_pick = None;
        self.draw_command_pick_voxel = None;
        if let Some(pick_id) = self.pick_id.take() {
            pick_id.destroy();
        }
        if let Some(mut traversal) = self.traversal.take() {
            traversal.destroy()?;
        }
        self.statistics.textures_byte_length = 0;
        if let Some(planes) = self.clipping_planes.take() {
            planes.borrow_mut().destroy()?;
        }
        self.textures.clear();
        self.destroyed = true;
        Ok(())
    }
}

pub fn get_tile_coordinates(
    primitive: &VoxelPrimitive<impl Gl>,
    position_local: Cartesian3,
    available_levels: u32,
) -> Cartesian4 {
    let Some(shape) = &primitive.shape else { return Cartesian4::ZERO };
    let shape_uv = shape.convert_local_to_shape_uv_space(position_local);
    let num_tiles = 2f64.powi(available_levels as i32 - 1);
    Cartesian4::new(
        shape_uv.x * num_tiles,
        shape_uv.y * num_tiles,
        shape_uv.z * num_tiles,
        available_levels as f64 - 1.0,
    )
}

const CORNERS: [Cartesian4; 8] = [
    Cartesian4::new(-1.0, -1.0, -1.0, 1.0),
    Cartesian4::new(1.0, -1.0, -1.0, 1.0),
    Cartesian4::new(-1.0, 1.0, -1.0, 1.0),
    Cartesian4::new(1.0, 1.0, -1.0, 1.0),
    Cartesian4::new(-1.0, -1.0, 1.0, 1.0),
    Cartesian4::new(1.0, -1.0, 1.0, 1.0),
    Cartesian4::new(-1.0, 1.0, 1.0, 1.0),
    Cartesian4::new(1.0, 1.0, 1.0, 1.0),
];
const VERTEX_NEIGHBOR_INDICES: [usize; 24] = [
    1, 2, 4, 0, 3, 5, 0, 3, 6, 1, 2, 7, 0, 5, 6, 1, 4, 7, 2, 4, 7, 3, 5, 6,
];

/// Projects the eight OBB corners to NDC, clipping edge intersections at the
/// near plane, and returns the clamped NDC axis-aligned bounds.
pub fn oriented_bounding_box_to_ndc_aabb(
    oriented_bounding_box: &OrientedBoundingBox,
    world_to_projection: &Matrix4,
) -> Cartesian4 {
    let local_to_world = Matrix4::from_rotation_translation(
        &oriented_bounding_box.half_axes,
        oriented_bounding_box.center,
    );
    let local_to_projection = world_to_projection.multiply_transformation(&local_to_world);
    let clips: Vec<Cartesian4> = CORNERS.iter().map(|corner| local_to_projection.multiply_by_vector(*corner)).collect();
    let mut ndc_min_x = f64::MAX;
    let mut ndc_max_x = -f64::MAX;
    let mut ndc_min_y = f64::MAX;
    let mut ndc_max_y = -f64::MAX;
    for (corner_index, position) in clips.iter().enumerate() {
        if position.z >= -position.w {
            let x = position.x / position.w;
            let y = position.y / position.w;
            ndc_min_x = ndc_min_x.min(x);
            ndc_max_x = ndc_max_x.max(x);
            ndc_min_y = ndc_min_y.min(y);
            ndc_max_y = ndc_max_y.max(y);
        } else {
            for neighbor in 0..3 {
                let neighbor_index = VERTEX_NEIGHBOR_INDICES[corner_index * 3 + neighbor];
                let neighbor_position = &clips[neighbor_index];
                if neighbor_position.z >= -neighbor_position.w {
                    let distance_from_position = position.z + position.w;
                    let distance_from_neighbor = neighbor_position.z + neighbor_position.w;
                    let t = distance_from_position / (distance_from_position - distance_from_neighbor);
                    let intersect = Cartesian4::new(
                        position.x + (neighbor_position.x - position.x) * t,
                        position.y + (neighbor_position.y - position.y) * t,
                        position.z + (neighbor_position.z - position.z) * t,
                        position.w + (neighbor_position.w - position.w) * t,
                    );
                    let x = intersect.x / intersect.w;
                    let y = intersect.y / intersect.w;
                    ndc_min_x = ndc_min_x.min(x);
                    ndc_max_x = ndc_max_x.max(x);
                    ndc_min_y = ndc_min_y.min(y);
                    ndc_max_y = ndc_max_y.max(y);
                }
            }
        }
    }
    Cartesian4::new(
        ndc_min_x.clamp(-1.0, 1.0),
        ndc_min_y.clamp(-1.0, 1.0),
        ndc_max_x.clamp(-1.0, 1.0),
        ndc_max_y.clamp(-1.0, 1.0),
    )
}
