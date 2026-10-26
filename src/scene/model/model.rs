//! Frame-driven model loading, rendering, animation, styling and ownership.
use super::*;
use crate::{
    core::{
        BoundingSphere, Cartesian3, Color, Credit, DistanceDisplayCondition, Event, Matrix4, Ray, Resource,
        RuntimeError,
    },
    renderer::{Buffer, Context, Gl, Pass, UniformFrustum},
    scene::{
        gltf_loader::{GltfLoader, GltfLoaderOptions},
        model_components as mc,
        resource_loader::ResourceLoader,
        ClassificationType, ColorBlendMode, FrameState, GltfJsonInput, HeightReference, ModelImageHost, ResourceCache,
        SceneMode, ScenePrimitive, ShadowMode, SplitDirection, StructuralMetadata,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::Arc,
};
type Result<T> = std::result::Result<T, RuntimeError>;

/// The loader retains cache leases while the graph owns a component snapshot.
/// GPU objects in snapshots have shared ownership; incremental textures refresh
/// the snapshot once their loaders have finished.
pub trait ModelLoader<G: Gl>: ResourceLoader {
    fn model_components(&self) -> Option<mc::Components<G>>;
    fn textures_loaded(&self) -> bool {
        true
    }
    fn incrementally_load_textures(&self) -> bool {
        false
    }
}
macro_rules! image_loader {
    ($ty:ident) => {
        impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> ModelLoader<G> for $ty<G, H>
        where
            H::Image: 'static,
        {
            fn model_components(&self) -> Option<mc::Components<G>> {
                self.components().map(|c| c.clone())
            }
            fn textures_loaded(&self) -> bool {
                self.textures_loaded()
            }
            fn incrementally_load_textures(&self) -> bool {
                self.incrementally_load_textures()
            }
        }
    };
}
image_loader!(GltfLoader);
image_loader!(B3dmLoader);
image_loader!(I3dmLoader);
impl<G: Gl + 'static> ModelLoader<G> for PntsLoader<G> {
    fn model_components(&self) -> Option<mc::Components<G>> {
        self.components().map(|c| c.clone())
    }
}
impl<G: Gl + 'static> ModelLoader<G> for GeoJsonLoader<G> {
    fn model_components(&self) -> Option<mc::Components<G>> {
        self.components().map(|c| c.clone())
    }
}

#[derive(Clone, Debug)]
pub struct ModelOptions {
    pub id: Option<String>,
    pub model_type: ModelType,
    pub model_matrix: Matrix4,
    pub scale: f64,
    pub minimum_pixel_size: f64,
    pub maximum_scale: Option<f64>,
    pub show: bool,
    pub color: Option<Color>,
    pub color_blend_mode: ColorBlendMode,
    pub color_blend_amount: f64,
    pub silhouette_color: Color,
    pub silhouette_size: f64,
    pub cull: bool,
    pub opaque_pass: Pass,
    pub allow_picking: bool,
    pub enable_pick: bool,
    pub project_to_2d: bool,
    pub feature_id_label: String,
    pub instance_feature_id_label: String,
    pub back_face_culling: bool,
    pub shadows: ShadowMode,
    pub debug_show_bounding_volume: bool,
    pub debug_wireframe: bool,
    pub enable_debug_wireframe: bool,
    pub enable_show_outline: bool,
    pub show_outline: bool,
    pub outline_color: Color,
    pub classification_type: Option<ClassificationType>,
    pub split_direction: SplitDirection,
    pub clamp_animations: bool,
    pub enable_vertical_exaggeration: bool,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
    pub height_reference: HeightReference,
    pub ignore_commands: bool,
    pub show_credits_on_screen: bool,
    pub credit: Option<Credit>,
}
impl Default for ModelOptions {
    fn default() -> Self {
        Self {
            id: None,
            model_type: ModelType::Gltf,
            model_matrix: Matrix4::IDENTITY,
            scale: 1.,
            minimum_pixel_size: 0.,
            maximum_scale: None,
            show: true,
            color: None,
            color_blend_mode: ColorBlendMode::Highlight,
            color_blend_amount: 0.5,
            silhouette_color: Color::RED,
            silhouette_size: 0.,
            cull: true,
            opaque_pass: Pass::Opaque,
            allow_picking: true,
            enable_pick: false,
            project_to_2d: false,
            feature_id_label: "featureId_0".into(),
            instance_feature_id_label: "instanceFeatureId_0".into(),
            back_face_culling: true,
            shadows: ShadowMode::Enabled,
            debug_show_bounding_volume: false,
            debug_wireframe: false,
            enable_debug_wireframe: false,
            enable_show_outline: true,
            show_outline: true,
            outline_color: Color::BLACK,
            classification_type: None,
            split_direction: SplitDirection::None,
            clamp_animations: true,
            enable_vertical_exaggeration: true,
            distance_display_condition: None,
            height_reference: HeightReference::None,
            ignore_commands: false,
            show_credits_on_screen: false,
            credit: None,
        }
    }
}
impl ModelOptions {
    pub fn validate(&self) -> Result<()> {
        if self.minimum_pixel_size < 0. {
            return Err(RuntimeError::new("minimumPixelSize must be greater than or equal to zero"));
        }
        if let Some(d) = self.distance_display_condition {
            if d.far <= d.near {
                return Err(RuntimeError::new(
                    "distanceDisplayCondition.far must be greater than distanceDisplayCondition.near",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
struct PipelineKey {
    properties: ModelPipelineProperties,
    alpha: u8,
    opaque_pass: Pass,
    mode: SceneMode,
    fog: bool,
    skip_lod: bool,
    clipping_state: isize,
    custom_shader: usize,
    ibl: ImageBasedLightingOptions,
    point: PointCloudStylingOptions,
}
type ImageUpdate<G> = Rc<dyn Fn(&CustomShader<G>, u32) -> Result<()>>;
type MatrixValues = Rc<RefCell<Vec<Matrix4>>>;
type MorphValues = Rc<RefCell<Vec<f64>>>;

/// Host-owned terrain and environment systems can refresh their model inputs
/// before building commands. Clamping supplies a world-space matrix each frame.
pub trait ModelUpdateServices<G: Gl> {
    fn update(&mut self, _model: &mut Model<G>, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn clamped_matrix(
        &mut self,
        _matrix: Matrix4,
        _reference: HeightReference,
        _frame: &FrameState,
    ) -> Result<Option<Matrix4>> {
        Ok(None)
    }
}

pub struct Model<G: Gl + 'static> {
    pub options: ModelOptions,
    pub resource: Option<Resource>,
    pub scene_graph: Option<ModelSceneGraph<G, ModelDrawCommandKind<G>>>,
    pub active_animations: Rc<RefCell<ModelAnimationCollection>>,
    pub feature_tables: Vec<ModelFeatureTable<G>>,
    pub feature_table_id: Option<usize>,
    pub statistics: ModelStatistics,
    pub custom_shader: Option<Rc<CustomShader<G>>>,
    pub style: Option<Rc<RefCell<crate::scene::Cesium3DTileStyle>>>,
    pub image_based_lighting: ImageBasedLightingOptions,
    pub point_cloud_styling: PointCloudStylingOptions,
    pub clipping_planes: Option<model_clipping_planes_pipeline_stage::ModelClippingPlanesState<G>>,
    pub clipping_polygons: Option<model_clipping_polygons_pipeline_stage::ModelClippingPolygonsState<G>>,
    pub imagery_inputs: BTreeMap<(usize, usize), Vec<ImageryInput<G>>>,
    pub imagery_flags: ImageryFlags,
    pub services: Option<Box<dyn ModelUpdateServices<G>>>,
    pub command_selection: ModelCommandSelection,
    pub ready_event: Rc<RefCell<Event<Option<String>>>>,
    pub textures_ready_event: Event<Option<String>>,
    pub error_event: Event<RuntimeError>,
    context: Context<G>,
    loader: Option<Rc<dyn ModelLoader<G>>>,
    pending_components: Option<mc::Components<G>>,
    ready: Rc<Cell<bool>>,
    ready_queued: bool,
    destroyed: Rc<Cell<bool>>,
    textures_loaded: bool,
    commands_built: bool,
    pipeline_key: Option<PipelineKey>,
    bounding_sphere: Option<BoundingSphere>,
    initial_radius: f64,
    computed_scale: f64,
    uniform_state: Rc<RefCell<ModelUniformState>>,
    pipeline_buffers: Vec<Buffer<G>>,
    joint_values: BTreeMap<usize, MatrixValues>,
    morph_values: BTreeMap<usize, MorphValues>,
    view_matrix: Rc<RefCell<Matrix4>>,
    view_3d_matrix: Rc<RefCell<Matrix4>>,
    previous_matrix: Matrix4,
    custom_warnings: CustomShaderWarnings,
    style_dirty: bool,
    image_update: Option<ImageUpdate<G>>,
}
impl<G: Gl + 'static> Model<G> {
    pub fn new(
        context: Context<G>,
        loader: Rc<dyn ModelLoader<G>>,
        resource: Option<Resource>,
        options: ModelOptions,
    ) -> Result<Self> {
        let mut model = Self::empty(context, options)?;
        model.loader = Some(loader);
        model.resource = resource;
        Ok(model)
    }
    pub fn from_components(context: Context<G>, components: mc::Components<G>, options: ModelOptions) -> Result<Self> {
        let mut model = Self::empty(context, options)?;
        model.pending_components = Some(components);
        model.textures_loaded = true;
        Ok(model)
    }
    fn empty(context: Context<G>, options: ModelOptions) -> Result<Self> {
        options.validate()?;
        Ok(Self {
            previous_matrix: options.model_matrix,
            computed_scale: options.scale,
            options,
            resource: None,
            scene_graph: None,
            active_animations: Rc::new(RefCell::new(ModelAnimationCollection::new())),
            feature_tables: vec![],
            feature_table_id: None,
            statistics: ModelStatistics::new(),
            custom_shader: None,
            style: None,
            style_dirty: false,
            image_update: None,
            image_based_lighting: Default::default(),
            point_cloud_styling: Default::default(),
            clipping_planes: None,
            clipping_polygons: None,
            imagery_inputs: BTreeMap::new(),
            imagery_flags: Default::default(),
            services: None,
            command_selection: Default::default(),
            ready_event: Rc::new(RefCell::new(Event::new())),
            textures_ready_event: Event::new(),
            error_event: Event::new(),
            context,
            loader: None,
            pending_components: None,
            ready: Rc::new(Cell::new(false)),
            ready_queued: false,
            destroyed: Rc::new(Cell::new(false)),
            textures_loaded: false,
            commands_built: false,
            pipeline_key: None,
            bounding_sphere: None,
            initial_radius: 0.,
            uniform_state: Rc::new(RefCell::new(ModelUniformState::default())),
            pipeline_buffers: vec![],
            joint_values: BTreeMap::new(),
            morph_values: BTreeMap::new(),
            view_matrix: Rc::new(RefCell::new(Matrix4::IDENTITY)),
            view_3d_matrix: Rc::new(RefCell::new(Matrix4::IDENTITY)),
            custom_warnings: CustomShaderWarnings::default(),
        })
    }
    pub fn ready(&self) -> bool {
        self.ready.get() && !self.destroyed.get()
    }
    pub fn textures_loaded(&self) -> bool {
        self.textures_loaded
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }
    pub fn bounding_sphere(&self) -> Result<BoundingSphere> {
        if !self.ready() {
            return Err(RuntimeError::new(
                "The model is not loaded. Use readyEvent or wait for ready before accessing boundingSphere.",
            ));
        }
        self.bounding_sphere.ok_or_else(|| RuntimeError::new("Model has no bounding sphere"))
    }
    pub fn computed_scale(&self) -> f64 {
        self.computed_scale
    }
    pub fn is_translucent(&self) -> bool {
        self.options.color.is_some_and(|c| c.alpha > 0. && c.alpha < 1.)
    }
    pub fn is_invisible(&self) -> bool {
        self.options.color.is_some_and(|c| c.alpha == 0.)
    }
    pub fn has_silhouette(&self, frame: &FrameState) -> bool {
        frame.stencil_buffer
            && self.options.silhouette_size > 0.
            && self.options.silhouette_color.alpha > 0.
            && self.options.classification_type.is_none()
    }
    pub fn reset_draw_commands(&mut self) {
        self.commands_built = false;
    }
    pub fn set_image_host<H: crate::core::ImageHost + 'static>(&mut self, host: Rc<H>) {
        let context = self.context.clone();
        self.image_update = Some(Rc::new(move |shader, frame| {
            shader.update(&context, frame, Some(context.default_texture()?), host.as_ref())?;
            Ok(())
        }));
    }
    pub fn set_style(&mut self, style: Option<Rc<RefCell<crate::scene::Cesium3DTileStyle>>>) {
        self.style = style;
        self.make_style_dirty();
    }
    pub fn make_style_dirty(&mut self) {
        self.style_dirty = true;
    }
    pub fn get_node(&self, name: &str) -> Result<Option<&ModelNode>> {
        if !self.ready() {
            return Err(RuntimeError::new(
                "The model is not loaded. Use readyEvent or wait for ready before calling getNode.",
            ));
        }
        Ok(self.scene_graph.as_ref().and_then(|g| g.get_node(name)))
    }
    pub fn get_extension(&self, name: &str) -> Result<Option<&serde_json::Value>> {
        if !self.ready() {
            return Err(RuntimeError::new("The model is not loaded."));
        }
        Ok(self.scene_graph.as_ref().and_then(|g| g.components().extensions.get(name)))
    }
    pub fn set_articulation_stage(&mut self, key: &str, value: f64) -> Result<()> {
        if !self.ready() {
            return Err(RuntimeError::new("The model is not loaded."));
        }
        if let Some(g) = &mut self.scene_graph {
            g.set_articulation_stage(key, value);
        }
        Ok(())
    }
    pub fn apply_articulations(&mut self) -> Result<()> {
        if !self.ready() {
            return Err(RuntimeError::new("The model is not loaded."));
        }
        if let Some(g) = &mut self.scene_graph {
            g.apply_articulations();
        }
        Ok(())
    }
    pub fn apply_style(&mut self, style: Option<&ModelFeatureStyle<'_, G>>) -> Result<()> {
        if let Some(table) = self.feature_table_id.and_then(|i| self.feature_tables.get(i)) {
            table.apply_style(style)?;
        }
        self.reset_draw_commands();
        Ok(())
    }
    pub fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.is_destroyed() {
            return Err(RuntimeError::new("Model was destroyed"));
        }
        self.options.validate()?;
        if let (Some(shader), Some(update)) = (&self.custom_shader, &self.image_update) {
            update(shader, frame.frame_number)?;
        }
        if let Some(planes) = &self.clipping_planes {
            planes.clipping_planes.borrow_mut().update(&self.context)?;
        }
        if let Some(mut services) = self.services.take() {
            let result = services.update(self, frame);
            self.services = Some(services);
            result?;
        }
        if let Some(loader) = &self.loader {
            match loader.process(frame) {
                Ok(processed) => {
                    if processed && self.scene_graph.is_none() {
                        self.pending_components = loader.model_components();
                    }
                    if processed && loader.textures_loaded() && !self.textures_loaded {
                        if let Some(graph) = &mut self.scene_graph {
                            if let Some(components) = loader.model_components() {
                                graph.components_mut().nodes = components.nodes;
                            }
                        }
                        self.textures_loaded = true;
                        self.commands_built = false;
                        if self.ready.get() && loader.incrementally_load_textures() {
                            self.textures_ready_event.raise_event(&self.options.id);
                        }
                    }
                }
                Err(error) => {
                    self.error_event.raise_event(&error);
                    return Err(error);
                }
            }
        }
        if self.scene_graph.is_none() {
            if let Some(components) = self.pending_components.take() {
                let metadata = components.structural_metadata.as_ref().and_then(|m| m.get::<StructuralMetadata<G>>());
                if let Some(metadata) = metadata {
                    let max_texture_size = self.context.limits()?.maximum_texture_size;
                    self.feature_tables = metadata
                        .property_tables()
                        .iter()
                        .map(|table| ModelFeatureTable::new(table.clone(), None, max_texture_size))
                        .collect::<Result<Vec<_>>>()?;
                }
                self.scene_graph = Some(ModelSceneGraph::new(
                    components,
                    ModelSceneGraphOptions {
                        model_matrix: self.options.model_matrix,
                        computed_scale: self.computed_scale,
                        clamped_scale: self.clamped_scale(),
                        ..Default::default()
                    },
                )?);
            }
        }
        if frame.mode == SceneMode::Morphing {
            return Ok(());
        }
        let Some(mut graph) = self.scene_graph.take() else {
            return Ok(());
        };
        let result = self.update_graph(&mut graph, frame);
        self.scene_graph = Some(graph);
        if let Err(ref error) = result {
            self.error_event.raise_event(error);
        }
        result
    }
    fn clamped_scale(&self) -> f64 {
        self.options.maximum_scale.map_or(self.options.scale, |m| m.min(self.options.scale))
    }
    fn properties(&self, frame: &FrameState) -> ModelPipelineProperties {
        ModelPipelineProperties {
            use_webgl2: self.context.webgl2(),
            project_to_2d: self.options.project_to_2d,
            has_vertical_exaggeration: self.options.enable_vertical_exaggeration && frame.vertical_exaggeration != 1.,
            has_imagery_layers: !self.imagery_inputs.is_empty(),
            custom_shader: self.custom_shader.as_ref().map(|s| CustomShaderPipelineProperties {
                has_fragment_shader: s.fragment_shader_text.is_some(),
                replace_material: s.mode == CustomShaderMode::ReplaceMaterial,
            }),
            has_style: self.point_cloud_styling.style_functions.is_some(),
            debug_wireframe: self.options.debug_wireframe,
            enable_debug_wireframe: self.options.enable_debug_wireframe,
            point_cloud_shading: Some(PointCloudShadingProperties {
                attenuation: self.point_cloud_styling.shading.attenuation,
                back_face_culling: self.point_cloud_styling.shading.back_face_culling,
            }),
            enable_show_outline: self.options.enable_show_outline,
            classification_type: self.options.classification_type,
            allow_picking: self.options.allow_picking,
            feature_id_label: self.options.feature_id_label.clone(),
            instance_feature_id_label: self.options.instance_feature_id_label.clone(),
            has_color: self.options.color.is_some(),
            image_based_lighting_enabled: self.image_based_lighting.enabled,
            clipping_enabled: self.clipping_planes.as_ref().is_some_and(|s| {
                let p = s.clipping_planes.borrow();
                p.enabled && !p.is_empty()
            }),
            clipping_polygons_enabled: self
                .clipping_polygons
                .as_ref()
                .is_some_and(|p| p.compute_polygon_bounds_length > 0),
            has_silhouette: self.has_silhouette(frame),
            split_direction: self.options.split_direction,
            model_type: self.options.model_type,
        }
    }
    fn update_graph(
        &mut self,
        graph: &mut ModelSceneGraph<G, ModelDrawCommandKind<G>>,
        frame: &mut FrameState,
    ) -> Result<()> {
        if self.options.model_matrix != self.previous_matrix
            && frame.mode != SceneMode::Scene3D
            && self.options.project_to_2d
        {
            return Err(RuntimeError::new(
                "Model.modelMatrix cannot be changed in 2D or Columbus View if projectTo2D is true.",
            ));
        }
        self.previous_matrix = self.options.model_matrix;
        let mut matrix = self.options.model_matrix;
        if self.options.height_reference != HeightReference::None {
            let services = self
                .services
                .as_mut()
                .ok_or_else(|| RuntimeError::new("Height reference is not supported without a scene."))?;
            if let Some(clamped) = services.clamped_matrix(matrix, self.options.height_reference, frame)? {
                matrix = clamped;
            }
        }
        self.computed_scale = self.clamped_scale();
        graph.compute_model_matrix(&matrix, self.computed_scale);
        if frame.mode != SceneMode::Scene3D && !self.options.project_to_2d {
            graph.compute_model_matrix_2d(frame.uniform.map_projection.as_ref());
        }
        *self.view_matrix.borrow_mut() = frame.uniform.camera.view;
        *self.view_3d_matrix.borrow_mut() = frame.uniform.camera.view;
        self.sync_uniforms(graph);
        let previous_table = self.feature_table_id;
        self.select_feature_table(graph);
        if previous_table != self.feature_table_id && self.style.is_some() {
            self.style_dirty = true;
        }
        if self.style_dirty {
            self.apply_current_style(graph)?;
            self.style_dirty = false;
            self.sync_uniforms(graph);
        }
        for table in &self.feature_tables {
            table.update(&self.context, frame.passes.pick || frame.passes.post_process)?;
        }
        self.command_selection.style =
            self.feature_table_id.and_then(|i| self.feature_tables.get(i)).map(|t| t.style_commands_needed());
        self.command_selection.project_to_2d = self.options.project_to_2d;
        let properties = self.properties(frame);
        let alpha = self.options.color.map_or(2, |c| {
            if c.alpha == 0. {
                0
            } else if c.alpha < 1. {
                1
            } else {
                2
            }
        });
        let key = PipelineKey {
            properties: properties.clone(),
            alpha,
            opaque_pass: self.options.opaque_pass,
            mode: frame.mode,
            fog: frame.fog.renderable,
            skip_lod: self.command_selection.mixed_content
                && frame.stencil_buffer
                && self.options.model_type.is_3d_tiles(),
            clipping_state: self.clipping_planes.as_ref().map_or(0, |s| s.clipping_planes.borrow().state()),
            custom_shader: self.custom_shader.as_ref().map_or(0, |s| Rc::as_ptr(s) as usize),
            ibl: self.image_based_lighting.clone(),
            point: self.point_cloud_styling.clone(),
        };
        if self.pipeline_key.as_ref() != Some(&key) {
            self.commands_built = false;
        }
        if !self.commands_built {
            graph.configure_pipeline(frame, &properties);
            self.build_commands(graph, frame, &properties, key.skip_lod)?;
            self.pipeline_key = Some(key);
            self.commands_built = true;
        }
        if let Some(sphere) = graph.bounding_sphere() {
            let mut world = sphere.transform(&matrix);
            self.initial_radius = world.radius;
            world.radius *= self.clamped_scale();
            self.bounding_sphere = Some(world);
            self.computed_scale = self.compute_scale(frame, world);
            graph.compute_model_matrix(&matrix, self.computed_scale);
            graph.update_model_matrix(&matrix, frame, Some(frame.uniform.map_projection.as_ref()));
        }
        if !self.ready.get() {
            if !self.ready_queued {
                let ready = self.ready.clone();
                let destroyed = self.destroyed.clone();
                let event = self.ready_event.clone();
                let id = self.options.id.clone();
                frame.after_render.push(Box::new(move || {
                    if !destroyed.get() {
                        ready.set(true);
                        event.borrow_mut().raise_event(&id);
                    }
                    Ok(true)
                }));
                self.ready_queued = true;
            }
            return Ok(());
        }
        let animated = if self.options.classification_type.is_none() {
            self.active_animations.borrow_mut().update(
                &frame.time,
                self.options.clamp_animations,
                graph.runtime_nodes_mut(),
            )?
        } else {
            false
        };
        graph.update(frame, animated || graph.user_animation_dirty);
        for node in graph.runtime_nodes_mut().iter_mut().flatten() {
            if let Some(values) = self.joint_values.get(&node.node_index()) {
                *values.borrow_mut() = node.computed_joint_matrices().to_vec();
            }
            if let Some(values) = self.morph_values.get(&node.node_index()) {
                *values.borrow_mut() = node.morph_weights().to_vec();
            }
            for primitive in &mut node.runtime_primitives {
                if let Some(draw) = &mut primitive.draw_command {
                    draw.set_back_face_culling(self.options.back_face_culling);
                    draw.set_shadow_mode(self.options.shadows);
                    draw.set_debug_show_bounding_volume(self.options.debug_show_bounding_volume);
                    draw.command_mut().metadata.owner = self.options.id.clone();
                    draw.command_mut().metadata.cull = self.options.cull;
                    if let ModelDrawCommandKind::Model(draw) = draw {
                        draw.options.silhouette_alpha = self.options.silhouette_color.alpha;
                        draw.options.invisible = self.is_invisible();
                    }
                }
            }
        }
        let animations = self.active_animations.clone();
        frame.after_render.push(Box::new(move || {
            animations.borrow_mut().raise_pending_events();
            Ok(false)
        }));
        self.sync_uniforms(graph);
        if !self.options.show
            || self.computed_scale == 0.
            || self.options.ignore_commands
            || !self.passes_distance_display_condition(frame)
            || self.is_invisible() && !self.has_silhouette(frame)
            || !(frame.passes.render || frame.passes.pick && self.options.allow_picking)
        {
            return Ok(());
        }
        let mut silhouettes = vec![];
        let mut edges = vec![];
        self.command_selection.bounding_sphere_2d = graph.bounding_sphere_2d().copied();
        for (node, primitive) in graph.runtime_primitive_order(true) {
            if let Some(draw) = graph.runtime_primitive(node, primitive).and_then(|p| p.draw_command.as_ref()) {
                let lists = draw.commands(frame, &self.command_selection);
                for command in lists.backfaces.into_iter().chain(lists.commands) {
                    frame.command_list.push(command.into_scene_command(
                        self.context.clone(),
                        self.options.id.clone().unwrap_or_else(|| "model".into()),
                    )?);
                }
                silhouettes.extend(lists.silhouettes);
                edges.extend(lists.edges);
            }
        }
        for command in silhouettes.into_iter().chain(edges) {
            frame.command_list.push(
                command.into_scene_command(
                    self.context.clone(),
                    self.options.id.clone().unwrap_or_else(|| "model".into()),
                )?,
            );
        }
        let mut credits = graph.components().asset.credits.clone();
        if let Some(credit) = &self.options.credit {
            credits.push(credit.clone());
        }
        for mut credit in credits {
            credit.show_on_screen |= self.options.show_credits_on_screen;
            frame.credits.push(credit);
        }
        Ok(())
    }
    fn apply_current_style(&mut self, graph: &ModelSceneGraph<G, ModelDrawCommandKind<G>>) -> Result<()> {
        let structural = graph.components().structural_metadata.as_ref().and_then(|m| m.get::<StructuralMetadata<G>>());
        let table = self.feature_table_id.and_then(|i| self.feature_tables.get(i)).filter(|t| t.features_length() > 0);
        if self.options.model_type == ModelType::TilePnts
            && (table.is_none() || structural.is_some_and(|m| !m.property_attributes().is_empty()))
        {
            self.point_cloud_styling.style_functions = if let Some(style) = &self.style {
                let mut style = style.borrow_mut();
                let mut substitutions: BTreeMap<String, String> = BTreeMap::from([
                    ("POSITION".into(), "attributes.positionMC".into()),
                    ("POSITION_ABSOLUTE".into(), "v_positionWC".into()),
                    ("COLOR".into(), "attributes.color_0".into()),
                    ("NORMAL".into(), "attributes.normalMC".into()),
                ]);
                if let Some(metadata) = structural {
                    for attribute in metadata.property_attributes() {
                        for (name, _) in attribute.properties().iter() {
                            substitutions.insert(name.to_owned(), format!("metadata.{name}"));
                        }
                    }
                }
                let parameters = "ProcessedAttributes attributes, Metadata metadata, float tiles3d_tileset_time";
                let mut state = crate::scene::expression::ShaderState::default();
                let color = style.get_color_shader_function(
                    &format!("getColorFromStyle({parameters})"),
                    &substitutions,
                    &mut state,
                )?;
                let show = style.get_show_shader_function(
                    &format!("getShowFromStyle({parameters})"),
                    &substitutions,
                    &mut state,
                )?;
                let size = style.get_point_size_shader_function(
                    &format!("getPointSizeFromStyle({parameters})"),
                    &substitutions,
                    &mut state,
                )?;
                Some(PointCloudStyleFunctions {
                    color_style_function: color,
                    show_style_function: show,
                    point_size_style_function: size,
                    style_translucent: state.translucent,
                })
            } else {
                None
            };
        } else if let Some(table) = table {
            let style = self.style.as_ref().map(|s| s.borrow());
            let color = |feature: &ModelFeature<G>| -> Result<Option<Color>> {
                match style.as_ref().and_then(|s| s.color()) {
                    Some(e) => e.evaluate_color(Some(feature)),
                    None => Ok(None),
                }
            };
            let show = |feature: &ModelFeature<G>| -> Result<Option<bool>> {
                match style.as_ref().and_then(|s| s.show()) {
                    Some(e) => Ok(Some(e.evaluate(Some(feature))?.to_boolean())),
                    None => Ok(None),
                }
            };
            table.apply_style(Some(&ModelFeatureStyle { color: Some(&color), show: Some(&show) }))?;
        } else {
            let style = self.style.as_ref().map(|s| s.borrow());
            self.options.color = match style.as_ref().and_then(|s| s.color()) {
                Some(e) => e.evaluate_color(None)?,
                None => Some(Color::WHITE),
            };
            self.options.show = match style.as_ref().and_then(|s| s.show()) {
                Some(e) => e.evaluate(None)?.to_boolean(),
                None => true,
            };
        }
        self.commands_built = false;
        Ok(())
    }
    fn build_commands(
        &mut self,
        graph: &mut ModelSceneGraph<G, ModelDrawCommandKind<G>>,
        frame: &mut FrameState,
        properties: &ModelPipelineProperties,
        skip_lod: bool,
    ) -> Result<()> {
        self.pipeline_buffers.clear();
        self.joint_values.clear();
        self.morph_values.clear();
        self.statistics.clear();
        let mut resources = ModelRenderResources::new(self.uniform_state.clone());
        resources.custom_shader = self.custom_shader.clone();
        for stage in graph.model_pipeline_stages() {
            match stage {
                ModelPipelineStage::Color => {
                    model_color_pipeline_stage::process(&mut resources.pipeline, properties.has_silhouette)
                }
                ModelPipelineStage::ImageBasedLighting => {
                    image_based_lighting_pipeline_stage::process(&mut resources, &self.image_based_lighting)
                }
                ModelPipelineStage::ClippingPlanes => {
                    if let Some(state) = &self.clipping_planes {
                        model_clipping_planes_pipeline_stage::process(&mut resources, state);
                    }
                }
                ModelPipelineStage::ClippingPolygons => {
                    if let Some(state) = &self.clipping_polygons {
                        model_clipping_polygons_pipeline_stage::process(&mut resources, state);
                    }
                }
                ModelPipelineStage::Silhouette => {} // The stage's Rust API operates on primitive resources.
                ModelPipelineStage::Splitter => model_splitter_pipeline_stage::process(&mut resources.pipeline),
                ModelPipelineStage::Tileset => tileset_pipeline_stage::process(&mut resources.pipeline, skip_lod),
                ModelPipelineStage::Atmosphere => atmosphere_pipeline_stage::process(&mut resources, frame),
            }
        }
        resources.node_render_resources = (0..graph.components().nodes.len()).map(|_| None).collect();
        let structural = graph.components().structural_metadata.clone();
        let metadata = structural.as_ref().and_then(|m| m.get::<StructuralMetadata<G>>());
        let pick_sources =
            self.feature_tables.iter().map(|t| t.picking_pipeline_source().map(Some)).collect::<Result<Vec<_>>>()?;
        let batch_statistics: Vec<_> = self.feature_tables.iter().map(|t| Some(t.batch_statistics())).collect();
        let mut usage = metadata_pipeline_stage::MetadataUsage::default();
        if let Some(shader) = &self.custom_shader {
            usage.vertex.extend(shader.used_variables_vertex.metadata_set.iter().map(str::to_owned));
            usage.fragment.extend(shader.used_variables_fragment.metadata_set.iter().map(str::to_owned));
        }
        if self.point_cloud_styling.style_functions.is_some() {
            if let Some(style) = &self.style {
                let style = style.borrow();
                let variables = |s: Option<&Rc<dyn crate::scene::style_expression::StyleExpression>>| {
                    s.map_or_else(Vec::new, |s| s.get_variables())
                };
                usage.add_point_cloud_variables(
                    &variables(style.color()),
                    &variables(style.show()),
                    &variables(style.point_size()),
                );
            }
        }
        let node_indices: Vec<_> =
            graph.runtime_nodes().iter().enumerate().filter_map(|(i, n)| n.as_ref().map(|_| i)).collect();
        for node_index in node_indices {
            let mut node = graph.components().nodes[node_index].clone();
            let mut node_resources = NodeRenderResources::new(&resources, node_index);
            let components_transform = graph.components().transform;
            let axis_correction_matrix = *graph.axis_correction_matrix();
            let runtime = graph.runtime_node_mut(node_index).expect("runtime node");
            runtime.configure_pipeline(&node);
            let node_transform = *runtime.computed_transform();
            let joint_values = Rc::new(RefCell::new(runtime.computed_joint_matrices().to_vec()));
            let morph_values = Rc::new(RefCell::new(runtime.morph_weights().to_vec()));
            self.joint_values.insert(node_index, joint_values.clone());
            self.morph_values.insert(node_index, morph_values.clone());
            if node.instances.is_some() {
                runtime.instancing.computed_transform = node_transform;
                runtime.instancing.components_transform = components_transform;
                runtime.instancing.axis_correction_matrix = axis_correction_matrix;
                instancing_pipeline_stage::process(
                    &mut node_resources,
                    &mut node,
                    &mut runtime.instancing,
                    frame.mode,
                    frame.scene_3d_only,
                    frame.morph_time,
                    frame.uniform.map_projection.clone(),
                    &self.context,
                    self.view_matrix.clone(),
                    self.view_3d_matrix.clone(),
                    &mut self.pipeline_buffers,
                )?;
            }
            node_statistics_pipeline_stage::process(&mut self.statistics, &node, &runtime.statistics_buffers);
            let translation_min = runtime.instancing.instancing_translation_min;
            let translation_max = runtime.instancing.instancing_translation_max;
            for primitive_index in 0..node.primitives.len() {
                let mut primitive = node.primitives[primitive_index].clone();
                let mut r = PrimitiveRenderResources::new(
                    &node_resources,
                    primitive_index,
                    &primitive,
                    translation_min,
                    translation_max,
                )?;
                if properties.has_silhouette {
                    model_silhouette_pipeline_stage::process(&mut r);
                }
                let runtime = graph.runtime_primitive_mut(node_index, primitive_index).expect("runtime primitive");
                runtime.configure_pipeline(
                    properties,
                    &node,
                    &primitive,
                    frame.mode,
                    frame.scene_3d_only,
                    frame.vertical_exaggeration,
                    &mut frame.edge_visibility_requested,
                );
                for stage in runtime.pipeline_stages.clone() {
                    match stage {
                        PrimitivePipelineStage::SceneMode2D => {
                            let mut state = scene_mode_2d_pipeline_stage::RuntimePrimitive2DState {
                                bounding_sphere_2d: runtime.bounding_sphere_2d,
                                position_buffer_2d: runtime.position_buffer_2d.clone(),
                            };
                            scene_mode_2d_pipeline_stage::process(
                                &mut r,
                                &mut primitive,
                                &mut state,
                                &node_transform,
                                frame.mode,
                                frame.morph_time,
                                frame.uniform.map_projection.as_ref(),
                                &self.context,
                                self.view_matrix.clone(),
                                &mut self.pipeline_buffers,
                            )?;
                            runtime.bounding_sphere_2d = state.bounding_sphere_2d;
                            runtime.position_buffer_2d = state.position_buffer_2d;
                        }
                        PrimitivePipelineStage::Geometry => geometry_pipeline_stage::process(
                            &mut r,
                            &primitive,
                            frame.mode,
                            frame.scene_3d_only,
                            self.context.webgl2(),
                        )?,
                        PrimitivePipelineStage::Wireframe => {
                            wireframe_pipeline_stage::process(&mut r, &primitive, &self.context, &mut self.statistics)?
                        }
                        PrimitivePipelineStage::Classification => {
                            classification_pipeline_stage::process(&mut r, &primitive)?
                        }
                        PrimitivePipelineStage::MorphTargets => {
                            morph_targets_pipeline_stage::process(&mut r, &primitive, morph_values.clone())?
                        }
                        PrimitivePipelineStage::Skinning => {
                            skinning_pipeline_stage::process::<G>(&mut r.pipeline, &primitive, joint_values.clone())?
                        }
                        PrimitivePipelineStage::PointCloudStyling => {
                            let mut options = self.point_cloud_styling.clone();
                            options.node_transform = node_transform;
                            point_cloud_styling_pipeline_stage::process(&mut r, &primitive, &options, frame)?;
                        }
                        PrimitivePipelineStage::Dequantization => {
                            dequantization_pipeline_stage::process(&mut r, &primitive)?
                        }
                        PrimitivePipelineStage::Imagery => {
                            if let Some(inputs) = self.imagery_inputs.get(&(node_index, primitive_index)) {
                                let count = inputs
                                    .iter()
                                    .map(|i| i.imagery_tex_coord_attribute_set_index as usize + 1)
                                    .max()
                                    .unwrap_or(0);
                                ImageryPipelineStage::process_imagery_inputs(
                                    &mut r,
                                    inputs,
                                    count,
                                    &self.imagery_flags,
                                )?;
                            }
                        }
                        PrimitivePipelineStage::Material => {
                            material_pipeline_stage::process(&mut r, &primitive, &self.context, frame.pixel_ratio)
                        }
                        PrimitivePipelineStage::FeatureId => feature_id_pipeline_stage::process(
                            &mut r,
                            &node,
                            &primitive,
                            &self.context,
                            &mut self.statistics,
                            &mut self.pipeline_buffers,
                        )?,
                        PrimitivePipelineStage::Metadata => {
                            metadata_pipeline_stage::process(
                                &mut r,
                                &primitive,
                                &node,
                                metadata,
                                metadata_pipeline_stage::MetadataStageOptions {
                                    webgl2: self.context.webgl2(),
                                    statistics: None,
                                    usage: &usage,
                                },
                            )?;
                        }
                        PrimitivePipelineStage::MetadataPicking => {
                            metadata_picking_pipeline_stage::process(&mut r.pipeline)?
                        }
                        PrimitivePipelineStage::SelectedFeatureId => {
                            selected_feature_id_pipeline_stage::process(&mut r, &node, &primitive)?
                        }
                        PrimitivePipelineStage::BatchTexture => {
                            if let Some(table) = self.feature_table_id.and_then(|i| self.feature_tables.get(i)) {
                                batch_texture_pipeline_stage::process(&mut r, table.batch_pipeline_source()?);
                            }
                        }
                        PrimitivePipelineStage::CpuStyling => cpu_styling_pipeline_stage::process(&mut r.pipeline),
                        PrimitivePipelineStage::VerticalExaggeration => {
                            vertical_exaggeration_pipeline_stage::process(&mut r, &primitive, frame)
                        }
                        PrimitivePipelineStage::CustomShader => {
                            custom_shader_pipeline_stage::process(
                                &mut r,
                                &primitive,
                                metadata,
                                &mut self.custom_warnings,
                            )?;
                        }
                        PrimitivePipelineStage::Lighting => lighting_pipeline_stage::process(&mut r, &primitive),
                        PrimitivePipelineStage::Picking => picking_pipeline_stage::process(
                            &mut r,
                            &node,
                            &primitive,
                            &self.context,
                            &mut self.statistics,
                            self.options.model_type.is_3d_tiles(),
                            &pick_sources,
                        )?,
                        PrimitivePipelineStage::PrimitiveOutline => {
                            primitive_outline_pipeline_stage::process(&mut r, &primitive, &self.context)?
                        }
                        PrimitivePipelineStage::EdgeVisibility => {
                            edge_visibility_pipeline_stage::process(&mut r, &primitive, frame, &self.context)?
                        }
                        PrimitivePipelineStage::EdgeDetection => edge_detection_pipeline_stage::process(&mut r),
                        PrimitivePipelineStage::Alpha => alpha_pipeline_stage::process(&mut r.pipeline),
                        PrimitivePipelineStage::PrimitiveStatistics => primitive_statistics_pipeline_stage::process(
                            &mut self.statistics,
                            &r,
                            &primitive,
                            metadata,
                            &batch_statistics,
                        ),
                    }
                }
                runtime.bounding_sphere = Some(r.bounding_sphere);
                runtime.batch_lengths = r.batch_lengths.clone();
                runtime.batch_offsets = r.batch_offsets.clone();
                let computed = if frame.mode == SceneMode::Scene3D || self.options.project_to_2d {
                    *graph.computed_model_matrix()
                } else {
                    *graph.computed_model_matrix_2d()
                };
                let options = model_draw_commands::ModelDrawBuildOptions {
                    model_matrix: computed,
                    node_transform,
                    bounding_sphere_2d: graph
                        .runtime_primitive(node_index, primitive_index)
                        .and_then(|p| p.bounding_sphere_2d),
                    project_to_2d: self.options.project_to_2d,
                    owner: self.options.id.clone(),
                    cull: self.options.cull,
                    debug_show_bounding_volume: self.options.debug_show_bounding_volume,
                    allow_picking: self.options.allow_picking,
                    enable_debug_wireframe: self.options.enable_debug_wireframe,
                    debug_wireframe: self.options.debug_wireframe,
                    derived: ModelDrawCommandOptions {
                        double_sided: primitive.material.as_ref().is_some_and(|m| m.double_sided),
                        has_silhouette: properties.has_silhouette,
                        has_skip_level_of_detail: skip_lod,
                        silhouette_id: self.uniform_state.borrow().silhouette_id.unwrap_or(0),
                        silhouette_alpha: self.options.silhouette_color.alpha,
                        invisible: self.is_invisible(),
                        shadows: self.options.shadows,
                    },
                };
                let draw = model_draw_commands::build_model_draw_command(&mut r, &self.context, frame, options)?;
                graph.set_draw_command(node_index, primitive_index, draw);
                node_resources.primitive_render_resources.push(r);
            }
            resources.node_render_resources[node_index] = Some(node_resources);
        }
        if !graph.runtime_primitive_order(false).is_empty() {
            let bounds =
                graph.compute_bounding_volumes(&resources, &self.options.model_matrix, self.clamped_scale())?;
            self.bounding_sphere = Some(bounds.model_bounding_sphere);
            self.initial_radius = bounds.initial_radius;
        } else {
            self.bounding_sphere = Some(BoundingSphere::default());
        }
        Ok(())
    }
    fn sync_uniforms(&self, graph: &ModelSceneGraph<G, ModelDrawCommandKind<G>>) {
        let mut u = self.uniform_state.borrow_mut();
        u.opaque_pass = self.options.opaque_pass;
        u.color = self.options.color.unwrap_or(Color::WHITE);
        u.has_color = self.options.color.is_some();
        u.color_blend_mode = self.options.color_blend_mode;
        u.color_blend_amount = self.options.color_blend_amount;
        u.model_type = self.options.model_type;
        u.project_to_2d = self.options.project_to_2d;
        u.enable_pick = self.options.enable_pick;
        u.classification_type = self.options.classification_type;
        u.silhouette_color = self.options.silhouette_color;
        u.silhouette_size = self.options.silhouette_size;
        u.outline_color = self.options.outline_color;
        u.show_outline = self.options.show_outline;
        u.split_direction = self.options.split_direction.value() as f64;
        u.back_face_culling = self.options.back_face_culling;
        u.model_matrix = self.options.model_matrix;
        u.scene_graph_computed_model_matrix = *graph.computed_model_matrix();
        u.bounding_sphere = self.bounding_sphere.unwrap_or_default();
        u.feature_id_label = self.options.feature_id_label.clone();
        u.instance_feature_id_label = self.options.instance_feature_id_label.clone();
        u.point_cloud_shading_attenuation = self.point_cloud_styling.shading.attenuation;
        u.point_cloud_shading_normal_shading = Some(self.point_cloud_styling.shading.normal_shading);
    }
    fn select_feature_table(&mut self, graph: &ModelSceneGraph<G, ModelDrawCommandKind<G>>) {
        self.feature_table_id = None;
        for node in &graph.components().nodes {
            if let Some(instances) = &node.instances {
                if let Some(id) = model_utility::get_feature_ids_by_label(
                    &instances.feature_ids,
                    &self.options.instance_feature_id_label,
                )
                .and_then(|f| f.property_table_id())
                {
                    self.feature_table_id = Some(id);
                    return;
                }
            }
        }
        for node in &graph.components().nodes {
            for primitive in &node.primitives {
                if let Some(id) =
                    model_utility::get_feature_ids_by_label(&primitive.feature_ids, &self.options.feature_id_label)
                        .and_then(|f| f.property_table_id())
                {
                    self.feature_table_id = Some(id);
                    return;
                }
            }
        }
    }
    fn compute_scale(&self, frame: &FrameState, sphere: BoundingSphere) -> f64 {
        let mut scale = self.options.scale;
        if self.options.minimum_pixel_size != 0. && !self.options.project_to_2d && self.initial_radius > 0. {
            let distance = sphere.center.subtract(frame.uniform.camera.position).dot(frame.uniform.camera.direction)
                - sphere.radius;
            let w = frame.drawing_buffer_width as f64;
            let h = frame.drawing_buffer_height as f64;
            let pixel = match frame.uniform.camera.frustum {
                UniformFrustum::Perspective(f) => f.get_pixel_dimensions(w, h, distance, 1.),
                UniformFrustum::PerspectiveOffCenter(f) => f.get_pixel_dimensions(w, h, distance, 1.),
                UniformFrustum::Orthographic(f) => f.get_pixel_dimensions(w, h, distance, 1.),
                UniformFrustum::OrthographicOffCenter(f) => f.get_pixel_dimensions(w, h, distance, 1.),
            };
            let meters = pixel.x.max(pixel.y);
            if (2. * sphere.radius / meters).min(w.max(h)) < self.options.minimum_pixel_size {
                scale = self.options.minimum_pixel_size * meters / (2. * self.initial_radius);
            }
        }
        self.options.maximum_scale.map_or(scale, |m| m.min(scale))
    }
    pub fn passes_distance_display_condition(&self, frame: &FrameState) -> bool {
        let Some(condition) = self.options.distance_display_condition else {
            return true;
        };
        let distance_squared = if frame.mode == SceneMode::Scene2D {
            let (_, _, _, planes) = frame.uniform.camera.frustum.parameters();
            ((planes[3] - planes[2]) * 0.5).powi(2)
        } else {
            let p = self.options.model_matrix.get_translation();
            let p = if frame.mode == SceneMode::Scene3D {
                p
            } else {
                let state = crate::scene::SceneTransformState::from_frame(frame, crate::core::Cartesian2::ONE);
                crate::scene::SceneTransforms::compute_actual_ellipsoid_position(&state, p).unwrap_or(p)
            };
            p.subtract(frame.uniform.camera.position).magnitude_squared()
        };
        distance_squared >= condition.near * condition.near && distance_squared <= condition.far * condition.far
    }
    pub fn pick(&self, ray: &Ray, frame: &FrameState) -> Result<Option<Cartesian3>> {
        if !self.context.webgl2() && !self.options.enable_pick {
            return Ok(None);
        }
        let Some(graph) = &self.scene_graph else {
            return Ok(None);
        };
        pick_model(
            graph,
            ray,
            frame,
            ModelPickOptions {
                ready: self.ready(),
                webgl2: self.context.webgl2(),
                back_face_culling: self.options.back_face_culling,
                model_matrix: self.options.model_matrix,
                vertical_exaggeration: if self.options.enable_vertical_exaggeration {
                    frame.vertical_exaggeration
                } else {
                    1.
                },
                relative_height: frame.vertical_exaggeration_relative_height,
            },
        )
    }
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed.replace(true) {
            return Ok(());
        }
        self.ready.set(false);
        self.scene_graph.take();
        self.pending_components.take();
        self.pipeline_buffers.clear();
        self.joint_values.clear();
        self.morph_values.clear();
        for table in &self.feature_tables {
            table.destroy()?;
        }
        self.feature_tables.clear();
        if let Some(loader) = self.loader.take() {
            loader.destroy();
        }
        Ok(())
    }
    pub async fn from_gltf<H: ModelImageHost<G> + 'static>(
        cache: ResourceCache,
        resource: Resource,
        base: Option<Resource>,
        input: Option<GltfJsonInput>,
        context: Context<G>,
        host: Rc<H>,
        options: ModelOptions,
    ) -> Result<Self>
    where
        H::Image: 'static,
    {
        let loader_options = GltfLoaderOptions {
            enable_pick: options.enable_pick,
            load_attributes_for_2d: options.project_to_2d,
            load_indices_for_wireframe: options.enable_debug_wireframe,
            load_primitive_outline: options.enable_show_outline,
            load_for_classification: options.classification_type.is_some(),
            ..Default::default()
        };
        let loader = Rc::new(GltfLoader::new(
            cache,
            resource.clone(),
            base,
            input,
            context.clone(),
            host.clone(),
            loader_options,
        ));
        loader.load().await?;
        let mut model = Self::new(context, loader, Some(resource), options)?;
        model.set_image_host(host);
        Ok(model)
    }
    // Match the underlying tile loader constructor.
    #[allow(clippy::too_many_arguments)]
    pub async fn from_b3dm<H: ModelImageHost<G> + 'static>(
        cache: ResourceCache,
        resource: Resource,
        base: Option<Resource>,
        data: Arc<Vec<u8>>,
        offset: usize,
        context: Context<G>,
        host: Rc<H>,
        mut options: ModelOptions,
    ) -> Result<Self>
    where
        H::Image: 'static,
    {
        options.model_type = ModelType::TileB3dm;
        let loader_options = B3dmLoaderOptions {
            enable_pick: options.enable_pick,
            load_attributes_for_2d: options.project_to_2d,
            load_indices_for_wireframe: options.enable_debug_wireframe,
            load_primitive_outline: options.enable_show_outline,
            load_for_classification: options.classification_type.is_some(),
            ..Default::default()
        };
        let loader = Rc::new(B3dmLoader::new(
            cache,
            resource.clone(),
            base,
            data,
            Some(offset),
            context.clone(),
            host.clone(),
            loader_options,
        ));
        loader.load().await?;
        let mut model = Self::new(context, loader, Some(resource), options)?;
        model.set_image_host(host);
        Ok(model)
    }
    // Match the underlying tile loader constructor.
    #[allow(clippy::too_many_arguments)]
    pub async fn from_i3dm<H: ModelImageHost<G> + 'static>(
        cache: ResourceCache,
        resource: Resource,
        base: Option<Resource>,
        data: Arc<Vec<u8>>,
        offset: usize,
        context: Context<G>,
        host: Rc<H>,
        mut options: ModelOptions,
    ) -> Result<Self>
    where
        H::Image: 'static,
    {
        options.model_type = ModelType::TileI3dm;
        let loader_options = I3dmLoaderOptions {
            enable_pick: options.enable_pick,
            load_attributes_for_2d: options.project_to_2d,
            load_indices_for_wireframe: options.enable_debug_wireframe,
            load_primitive_outline: options.enable_show_outline,
            ..Default::default()
        };
        let loader = Rc::new(I3dmLoader::new(
            cache,
            resource.clone(),
            base,
            data,
            Some(offset),
            context.clone(),
            host.clone(),
            loader_options,
        ));
        loader.load().await?;
        let mut model = Self::new(context, loader, Some(resource), options)?;
        model.set_image_host(host);
        Ok(model)
    }
    pub async fn from_pnts(
        resource: Resource,
        data: Arc<Vec<u8>>,
        offset: usize,
        context: Context<G>,
        mut options: ModelOptions,
    ) -> Result<Self> {
        options.model_type = ModelType::TilePnts;
        let loader = Rc::new(PntsLoader::new(
            data,
            Some(offset),
            context.clone(),
            PntsLoaderOptions { load_attributes_for_2d: options.project_to_2d, ..Default::default() },
        ));
        loader.load().await?;
        Self::new(context, loader, Some(resource), options)
    }
    pub async fn from_geo_json(
        resource: Resource,
        data: serde_json::Value,
        context: Context<G>,
        mut options: ModelOptions,
    ) -> Result<Self> {
        options.model_type = ModelType::TileGeojson;
        let loader = Rc::new(GeoJsonLoader::new(data, context.clone()));
        loader.load().await?;
        Self::new(context, loader, Some(resource), options)
    }
}
impl<G: Gl + 'static> ScenePrimitive for Model<G> {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        Model::update(self, frame)
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, _: crate::scene::Cesium3DTilePass) -> Result<()> {
        Model::update(self, frame)
    }
    fn destroy(&mut self) -> Result<()> {
        Model::destroy(self)
    }
}
