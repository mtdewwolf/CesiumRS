//! CesiumJS `ClassificationPrimitive`: batched shadow volumes that classify
//! terrain and 3D Tiles.
//!
//! Classification reuses the ported [`Primitive`] for geometry combination,
//! upload, pick offsets and per-instance attributes, then builds its own
//! stencil-depth and color commands with [`ShadowVolumeAppearance`]. Ground
//! and polyline variants live in their own modules.

use super::{
    appearance::Appearance,
    blending_state,
    material::material_uniform_values,
    per_instance_color_appearance::{PerInstanceColorAppearance, PerInstanceColorAppearanceOptions},
    primitive::{Primitive, PrimitiveOptions},
    primitive_shader::{self, ShaderRewrites},
    primitive_state::PrimitiveState,
    scene::{Result, SceneCommand, SceneDrawOptions, ScenePrimitive},
    stencil_constants::{CESIUM_3D_TILE_MASK, CLASSIFICATION_MASK},
    Cesium3DTilePass, ClassificationType, FrameState, GeometryInstanceAttributes, SceneMode, ShadowVolumeAppearance,
};
use crate::{
    core::{webgl_constants as gl, BoundingSphere, GeometryInstance, Matrix4, RuntimeError},
    renderer::{
        render_state::{ColorMask, DepthTest, StencilOperation, StencilTest},
        Context, DrawMetadata, Gl, Pass, RenderState, ShaderProgram, ShaderProgramOptions, ShaderSource, UniformValue,
    },
    shaders::root::{SHADOW_VOLUME_APPEARANCE_VS, SHADOW_VOLUME_FS},
};
use std::rc::Rc;

/// Options for [`ClassificationPrimitive`].
pub struct ClassificationPrimitiveOptions {
    pub primitive: PrimitiveOptions,
    pub classification_type: ClassificationType,
    pub debug_show_shadow_volume: bool,
}

impl Default for ClassificationPrimitiveOptions {
    fn default() -> Self {
        Self {
            primitive: PrimitiveOptions::default(),
            classification_type: ClassificationType::Both,
            debug_show_shadow_volume: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClassificationKind {
    Volume,
    Ground,
    Polyline,
}

struct ShaderSet<G: Gl> {
    stencil: Rc<ShaderProgram<G>>,
    stencil_no_show: Rc<ShaderProgram<G>>,
    color: Rc<ShaderProgram<G>>,
    pick: Rc<ShaderProgram<G>>,
}

/// A batched classification primitive.
pub struct ClassificationPrimitive<G: Gl + 'static> {
    pub classification_type: ClassificationType,
    pub debug_show_shadow_volume: bool,
    kind: ClassificationKind,
    pending: Option<PrimitiveOptions>,
    primitive: Option<Primitive<G>>,
    planar_extents: bool,
    spherical_extents: bool,
    has_colors: bool,
    use_pick_offsets: bool,
    shaders: Option<ShaderSet<G>>,
    ready: bool,
    ground_bounding_volumes: Vec<Option<BoundingSphere>>,
    ground_bounding_volumes_2d: Vec<Option<BoundingSphere>>,
    ground_heights: Option<[f64; 2]>,
    destroyed: bool,
}

impl<G: Gl + 'static> ClassificationPrimitive<G> {
    pub fn new(options: ClassificationPrimitiveOptions) -> Self {
        Self::with_kind(options, ClassificationKind::Volume)
    }

    pub(crate) fn with_kind(options: ClassificationPrimitiveOptions, kind: ClassificationKind) -> Self {
        Self {
            classification_type: options.classification_type,
            debug_show_shadow_volume: options.debug_show_shadow_volume,
            kind,
            pending: Some(options.primitive),
            primitive: None,
            planar_extents: false,
            spherical_extents: false,
            use_pick_offsets: false,
            has_colors: false,
            shaders: None,
            ready: false,
            ground_bounding_volumes: Vec::new(),
            ground_bounding_volumes_2d: Vec::new(),
            ground_heights: None,
            destroyed: false,
        }
    }

    pub fn is_supported(frame: &FrameState) -> bool {
        frame.stencil_buffer
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn show(&self) -> bool {
        self.primitive.as_ref().is_none_or(|primitive| primitive.show)
    }

    pub fn set_show(&mut self, show: bool) {
        if let Some(options) = &mut self.pending {
            options.show = Some(show);
        }
        if let Some(primitive) = &mut self.primitive {
            primitive.show = show;
        }
    }

    pub fn debug_show_bounding_volume(&self) -> bool {
        self.primitive.as_ref().is_some_and(|primitive| primitive.debug_show_bounding_volume)
    }

    pub fn set_debug_show_bounding_volume(&mut self, show: bool) {
        if let Some(options) = &mut self.pending {
            options.debug_show_bounding_volume = Some(show);
        }
        if let Some(primitive) = &mut self.primitive {
            primitive.debug_show_bounding_volume = show;
        }
    }

    pub fn appearance(&self) -> Option<&dyn Appearance> {
        self.primitive.as_ref().and_then(|primitive| primitive.appearance.as_deref())
    }

    pub fn set_appearance(&mut self, appearance: Box<dyn Appearance>) {
        if let Some(options) = &mut self.pending {
            options.appearance = Some(appearance);
        } else if let Some(primitive) = &mut self.primitive {
            primitive.appearance = Some(appearance);
            self.shaders = None;
        }
    }

    pub fn geometry_instances(&self) -> Option<&[GeometryInstance]> {
        self.pending
            .as_ref()
            .map(|options| options.geometry_instances.as_slice())
            .or_else(|| self.primitive.as_ref()?.geometry_instances())
    }

    pub fn get_geometry_instance_attributes(&mut self, id: &str) -> Result<Option<GeometryInstanceAttributes<'_, G>>> {
        self.primitive
            .as_mut()
            .ok_or_else(|| RuntimeError::new("must call update before calling getGeometryInstanceAttributes"))?
            .get_geometry_instance_attributes(id)
    }

    pub(crate) fn ground_heights_value(&self) -> Option<[f64; 2]> {
        self.ground_heights
    }

    pub(crate) fn ground_bounding_sphere(&self, id: &str) -> Option<BoundingSphere> {
        let primitive = self.primitive.as_ref()?;
        let index = primitive
            .geometry_instances()
            .and_then(|instances| instances.iter().position(|instance| instance.id.as_deref() == Some(id)))?;
        self.ground_bounding_volumes.get(index).copied().flatten()
    }

    pub(crate) fn set_ground_bounds(
        &mut self,
        volumes: Vec<Option<BoundingSphere>>,
        volumes_2d: Vec<Option<BoundingSphere>>,
        heights: Option<[f64; 2]>,
    ) {
        self.ground_bounding_volumes = volumes;
        self.ground_bounding_volumes_2d = volumes_2d;
        self.ground_heights = heights;
    }

    fn initialize(&mut self, frame: &FrameState) -> Result<()> {
        let mut options = self.pending.take().expect("pending classification options");
        let instances = options.geometry_instances.clone();
        if let Some(first) = instances.first() {
            self.planar_extents =
                ShadowVolumeAppearance::has_attributes_for_texture_coordinate_planes(&first.attributes);
            self.spherical_extents = ShadowVolumeAppearance::has_attributes_for_spherical_extents(&first.attributes);
        }
        let colors: Vec<_> = instances
            .iter()
            .map(|instance| instance.attributes.iter().find(|(name, _)| name == "color").map(|(_, value)| value))
            .collect();
        self.has_colors = !colors.is_empty() && colors.iter().all(Option::is_some);
        let extents = self.planar_extents || self.spherical_extents;
        let polyline = self.kind == ClassificationKind::Polyline;
        if self.kind == ClassificationKind::Volume && !extents {
            if colors.iter().any(Option::is_none) && colors.iter().any(Option::is_some) {
                return Err(RuntimeError::new(
                    "All GeometryInstances must have color attributes to use per-instance color.",
                ));
            }
            if !self.has_colors || colors.windows(2).any(|pair| pair[0] != pair[1]) {
                return Err(RuntimeError::new(
                    "All GeometryInstances must have the same color attribute except via GroundPrimitives",
                ));
            }
        }
        if let Some(appearance) = options.appearance.as_deref() {
            if appearance.material().is_none() && !self.has_colors {
                return Err(RuntimeError::new(
                    "PerInstanceColorAppearance requires color GeometryInstanceAttributes on all GeometryInstances",
                ));
            }
            if appearance.material().is_some() && !polyline && !extents {
                return Err(RuntimeError::new(
                    "Materials on ClassificationPrimitives are not supported except via GroundPrimitives",
                ));
            }
        } else {
            options.appearance = Some(if polyline {
                Box::new(super::PolylineMaterialAppearance::new(Default::default()))
            } else {
                Box::new(PerInstanceColorAppearance::new(PerInstanceColorAppearanceOptions {
                    flat: Some(true),
                    ..Default::default()
                }))
            });
        }
        self.use_pick_offsets = !polyline && !extents;
        options.create_pick_offsets = self.use_pick_offsets;
        if self.kind != ClassificationKind::Volume {
            options.model_matrix = Some(Matrix4::IDENTITY);
        }
        let _ = frame;
        self.primitive = Some(Primitive::new(options));
        Ok(())
    }

    fn validate_appearance(&self) -> Result<()> {
        let Some(appearance) = self.appearance() else { return Ok(()) };
        let extents = self.planar_extents || self.spherical_extents;
        if appearance.material().is_none() && !self.has_colors {
            return Err(RuntimeError::new("PerInstanceColorAppearance requires color GeometryInstanceAttribute"));
        }
        if appearance.material().is_some() && self.kind == ClassificationKind::Volume && !extents {
            return Err(RuntimeError::new(
                "Materials on ClassificationPrimitives are not supported except via GroundPrimitive",
            ));
        }
        Ok(())
    }

    fn build_shaders(&mut self, context: &Context<G>, frame: &FrameState, primitive: &mut Primitive<G>) -> Result<()> {
        if self.shaders.is_some() {
            return Ok(());
        }
        let attribute_locations = primitive.uploaded_attribute_locations().clone();
        let scene_3d_only = frame.scene_3d_only;
        let compress_vertices = primitive.compress_vertices;
        let has_show = primitive.has_batch_attribute("show");
        let has_distance = primitive.has_batch_attribute("distanceDisplayCondition");
        let has_color = primitive.has_batch_attribute("color");
        let extruded =
            if self.kind == ClassificationKind::Ground { vec!["EXTRUDED_GEOMETRY".to_string()] } else { vec![] };
        let projection = frame.uniform.map_projection.as_ref();

        let base_vertex = primitive.batch_vertex_shader(SHADOW_VOLUME_APPEARANCE_VS.source);
        let stencil_rewrites = ShaderRewrites {
            offset: false,
            show: has_show,
            distance_display_condition: has_distance,
            color: has_color,
            depth_fail_color: false,
            compress_vertices,
            scene_3d_only,
            rtc: false,
        };
        let (stencil_vertex, stencil_fragment) =
            stencil_rewrites.apply(base_vertex.clone(), SHADOW_VOLUME_FS.source.to_string(), false)?;
        let no_show_rewrites = ShaderRewrites { show: false, ..stencil_rewrites };
        let (stencil_no_show_vertex, stencil_no_show_fragment) =
            no_show_rewrites.apply(base_vertex.clone(), SHADOW_VOLUME_FS.source.to_string(), false)?;

        let appearance =
            primitive.appearance.take().ok_or_else(|| RuntimeError::new("Classification requires an appearance"))?;
        let shadow =
            ShadowVolumeAppearance::new(self.planar_extents || self.spherical_extents, self.planar_extents, appearance);
        let color_vertex = shadow.create_vertex_shader(&extruded, &stencil_vertex, false, projection);
        let color_fragment = shadow.create_fragment_shader(false);
        let pick_vertex_base = primitive.batch_vertex_shader(SHADOW_VOLUME_APPEARANCE_VS.source);
        let (pick_vertex, _) = stencil_rewrites.apply(pick_vertex_base, String::new(), false)?;
        let pick_vertex = primitive_shader::update_pick_color_attribute(&pick_vertex);
        let pick_vertex = shadow.create_pick_vertex_shader(&extruded, &pick_vertex, false, projection);
        let pick_fragment = shadow.create_pick_fragment_shader(false);
        primitive.appearance = Some(shadow.appearance);

        let make = |vertex: ShaderSource, fragment: ShaderSource| {
            Ok::<_, RuntimeError>(Rc::new(ShaderProgram::from_sources(
                context,
                &ShaderProgramOptions {
                    vertex_shader_source: vertex,
                    fragment_shader_source: fragment,
                    attribute_locations: attribute_locations.clone(),
                },
            )?))
        };
        let stencil = make(
            ShaderSource { sources: vec![stencil_vertex], defines: extruded.clone(), ..Default::default() },
            ShaderSource { sources: vec![stencil_fragment], defines: extruded.clone(), ..Default::default() },
        )?;
        let stencil_no_show = make(
            ShaderSource { sources: vec![stencil_no_show_vertex], defines: extruded.clone(), ..Default::default() },
            ShaderSource { sources: vec![stencil_no_show_fragment], defines: extruded.clone(), ..Default::default() },
        )?;
        let color = make(color_vertex, color_fragment)?;
        let pick = make(pick_vertex, pick_fragment)?;
        self.shaders = Some(ShaderSet { stencil, stencil_no_show, color, pick });
        Ok(())
    }

    fn uniforms(&self, primitive: &Primitive<G>, material: bool) -> Vec<(String, UniformValue)> {
        let mut uniforms = Vec::new();
        if let Some(appearance) = primitive.appearance.as_deref() {
            uniforms.extend(appearance.uniforms());
            if material {
                if let Some(material) = appearance.material() {
                    uniforms.extend(material_uniform_values(material));
                }
            }
        }
        uniforms.extend(primitive.batch_uniform_values());
        if self.kind == ClassificationKind::Ground {
            uniforms.push(("u_globeMinimumAltitude".into(), UniformValue::Float(vec![55000.0])));
        }
        uniforms
    }

    fn bounding_volumes(&self, primitive: &Primitive<G>, frame: &FrameState) -> Vec<Option<BoundingSphere>> {
        if self.kind == ClassificationKind::Ground
            && frame.mode != SceneMode::Scene3D
            && self.ground_bounding_volumes_2d.len() == primitive.uploaded_vertex_arrays().len()
        {
            return self.ground_bounding_volumes_2d.clone();
        }
        if self.kind != ClassificationKind::Volume && !self.ground_bounding_volumes.is_empty() {
            return self.ground_bounding_volumes.clone();
        }
        primitive
            .bounding_spheres()
            .iter()
            .map(|sphere| sphere.map(|sphere| sphere.transform(&primitive.model_matrix)))
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    fn push_classification_command(
        &self,
        frame: &mut FrameState,
        context: &Context<G>,
        primitive: &Primitive<G>,
        shaders: &ShaderSet<G>,
        vertices: Rc<crate::renderer::VertexArray<G>>,
        program: Rc<ShaderProgram<G>>,
        state: RenderState,
        pass: Pass,
        bounding_volume: Option<BoundingSphere>,
        uniforms: Vec<(String, UniformValue)>,
        pick_only: bool,
        id: &str,
    ) {
        let mut options = SceneDrawOptions::new(vertices, program);
        options.primitive_type = primitive.command_primitive_type();
        options.state = state;
        options.uniforms = uniforms;
        if let Some(texture) = primitive.batch_texture() {
            options.textures.push(("batchTexture".into(), texture));
        }
        let _ = shaders;
        let metadata = DrawMetadata {
            bounding_volume,
            model_matrix: Some(primitive.model_matrix),
            owner: Some("ClassificationPrimitive".into()),
            cull: primitive.cull,
            debug_show_bounding_volume: primitive.debug_show_bounding_volume,
            pick_id: None,
            ..Default::default()
        };
        let command = SceneCommand::from_draw(id, context.clone(), pass, metadata, options);
        if pick_only {
            // Pick commands are already pick shaders; Scene must not derive again.
            frame.command_list.push(command);
        } else {
            frame.command_list.push(command);
        }
    }

    fn queue_commands(&mut self, frame: &mut FrameState) -> Result<()> {
        let Some(mut primitive) = self.primitive.take() else { return Ok(()) };
        let result = self.queue_commands_with(frame, &mut primitive);
        self.primitive = Some(primitive);
        result
    }

    fn queue_commands_with(&mut self, frame: &mut FrameState, primitive: &mut Primitive<G>) -> Result<()> {
        if primitive.state() != PrimitiveState::Complete || !primitive.show {
            return Ok(());
        }
        let context = primitive
            .upload_context()
            .cloned()
            .ok_or_else(|| RuntimeError::new("ClassificationPrimitive requires a renderer context"))?;
        self.build_shaders(&context, frame, primitive)?;
        let shaders = self.shaders.as_ref().expect("built above");
        let vertices = primitive.uploaded_vertex_arrays().to_vec();
        let volumes = self.bounding_volumes(primitive, frame);
        let model_matrix = primitive.model_matrix;
        let cull = primitive.cull;
        let stencil_state_terrain = if self.debug_show_shadow_volume {
            stencil_depth_state(false, false)
        } else {
            stencil_depth_state(true, false)
        };
        let stencil_state_tiles = if self.debug_show_shadow_volume {
            stencil_depth_state(false, true)
        } else {
            stencil_depth_state(true, true)
        };
        let color_state = classification_color_state(!self.debug_show_shadow_volume);
        let pick_state = classification_pick_state();
        let render_uniforms = self.uniforms(primitive, false);
        let color_uniforms = self.uniforms(primitive, true);
        let queue_terrain = self.classification_type != ClassificationType::Cesium3DTile;
        let queue_tiles = self.classification_type != ClassificationType::Terrain;
        let mut command_index = 0usize;
        let passes = frame.passes;

        if passes.render {
            for (index, vertex_array) in vertices.iter().enumerate() {
                let volume = volumes.get(index).copied().flatten();
                if queue_terrain {
                    self.push_classification_command(
                        frame,
                        &context,
                        primitive,
                        shaders,
                        vertex_array.clone(),
                        shaders.stencil.clone(),
                        stencil_state_terrain.clone(),
                        Pass::TerrainClassification,
                        volume,
                        render_uniforms.clone(),
                        false,
                        &format!("ClassificationPrimitive-stencil-{command_index}"),
                    );
                }
                if queue_tiles {
                    self.push_classification_command(
                        frame,
                        &context,
                        primitive,
                        shaders,
                        vertex_array.clone(),
                        shaders.stencil.clone(),
                        stencil_state_tiles.clone(),
                        Pass::Cesium3DTileClassification,
                        volume,
                        render_uniforms.clone(),
                        false,
                        &format!("ClassificationPrimitive-stencil-tiles-{command_index}"),
                    );
                }
                if queue_terrain {
                    self.push_classification_command(
                        frame,
                        &context,
                        primitive,
                        shaders,
                        vertex_array.clone(),
                        shaders.color.clone(),
                        color_state.clone(),
                        Pass::TerrainClassification,
                        volume,
                        color_uniforms.clone(),
                        false,
                        &format!("ClassificationPrimitive-color-{command_index}"),
                    );
                }
                if queue_tiles {
                    self.push_classification_command(
                        frame,
                        &context,
                        primitive,
                        shaders,
                        vertex_array.clone(),
                        shaders.color.clone(),
                        color_state.clone(),
                        Pass::Cesium3DTileClassification,
                        volume,
                        color_uniforms.clone(),
                        false,
                        &format!("ClassificationPrimitive-color-tiles-{command_index}"),
                    );
                }
                command_index += 2;
                let _ = model_matrix;
                let _ = cull;
            }
            if frame.invert_classification {
                for (index, vertex_array) in vertices.iter().enumerate() {
                    let volume = volumes.get(index).copied().flatten();
                    self.push_classification_command(
                        frame,
                        &context,
                        primitive,
                        shaders,
                        vertex_array.clone(),
                        shaders.stencil_no_show.clone(),
                        stencil_state_terrain.clone(),
                        Pass::Cesium3DTileClassificationIgnoreShow,
                        volume,
                        render_uniforms.clone(),
                        false,
                        &format!("ClassificationPrimitive-ignore-show-{index}"),
                    );
                }
            }
        }

        if passes.pick {
            if self.use_pick_offsets {
                let offsets = primitive.pick_offsets().to_vec();
                for (pick_index, offset) in offsets.iter().enumerate() {
                    let vertex_array = vertices.get(offset.index).cloned();
                    let volume = volumes.get(offset.index).copied().flatten();
                    let Some(vertex_array) = vertex_array else { continue };
                    if queue_terrain {
                        self.push_pick_command(
                            frame,
                            &context,
                            primitive,
                            shaders,
                            vertex_array.clone(),
                            pick_state.clone(),
                            volume,
                            render_uniforms.clone(),
                            offset,
                            pick_index,
                            false,
                        );
                        self.push_pick_command(
                            frame,
                            &context,
                            primitive,
                            shaders,
                            vertex_array,
                            pick_state.clone(),
                            volume,
                            render_uniforms.clone(),
                            offset,
                            pick_index,
                            true,
                        );
                    }
                }
            } else {
                for (index, vertex_array) in vertices.iter().enumerate() {
                    let volume = volumes.get(index).copied().flatten();
                    if queue_terrain {
                        self.push_pick_plain(
                            frame,
                            &context,
                            primitive,
                            shaders,
                            vertex_array.clone(),
                            pick_state.clone(),
                            volume,
                            render_uniforms.clone(),
                            index,
                            false,
                        );
                        self.push_pick_plain(
                            frame,
                            &context,
                            primitive,
                            shaders,
                            vertex_array.clone(),
                            pick_state.clone(),
                            volume,
                            render_uniforms.clone(),
                            index,
                            true,
                        );
                    }
                    if queue_tiles {
                        self.push_pick_plain(
                            frame,
                            &context,
                            primitive,
                            shaders,
                            vertex_array.clone(),
                            pick_state.clone(),
                            volume,
                            render_uniforms.clone(),
                            index,
                            false,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn push_pick_command(
        &self,
        frame: &mut FrameState,
        context: &Context<G>,
        primitive: &Primitive<G>,
        shaders: &ShaderSet<G>,
        vertices: Rc<crate::renderer::VertexArray<G>>,
        state: RenderState,
        volume: Option<BoundingSphere>,
        uniforms: Vec<(String, UniformValue)>,
        offset: &super::primitive_pipeline::PickOffset,
        pick_index: usize,
        color: bool,
    ) {
        let mut options =
            SceneDrawOptions::new(vertices, if color { shaders.pick.clone() } else { shaders.stencil.clone() });
        options.primitive_type = primitive.command_primitive_type();
        options.state = if color { state } else { stencil_depth_state(true, false) };
        options.uniforms = uniforms;
        options.offset = offset.offset;
        options.count = Some(offset.count);
        if let Some(texture) = primitive.batch_texture() {
            options.textures.push(("batchTexture".into(), texture));
        }
        let metadata = DrawMetadata {
            bounding_volume: volume,
            model_matrix: Some(primitive.model_matrix),
            owner: Some("ClassificationPrimitive".into()),
            cull: primitive.cull,
            pick_id: None,
            ..Default::default()
        };
        frame.command_list.push(SceneCommand::from_draw(
            format!("ClassificationPrimitive-pick-{pick_index}-{color}"),
            context.clone(),
            Pass::TerrainClassification,
            metadata,
            options,
        ));
    }

    #[allow(clippy::too_many_arguments)]
    fn push_pick_plain(
        &self,
        frame: &mut FrameState,
        context: &Context<G>,
        primitive: &Primitive<G>,
        shaders: &ShaderSet<G>,
        vertices: Rc<crate::renderer::VertexArray<G>>,
        state: RenderState,
        volume: Option<BoundingSphere>,
        uniforms: Vec<(String, UniformValue)>,
        index: usize,
        color: bool,
    ) {
        let mut options =
            SceneDrawOptions::new(vertices, if color { shaders.pick.clone() } else { shaders.stencil.clone() });
        options.primitive_type = primitive.command_primitive_type();
        options.state = if color { state } else { stencil_depth_state(true, false) };
        options.uniforms = uniforms;
        if let Some(texture) = primitive.batch_texture() {
            options.textures.push(("batchTexture".into(), texture));
        }
        let metadata = DrawMetadata {
            bounding_volume: volume,
            model_matrix: Some(primitive.model_matrix),
            owner: Some("ClassificationPrimitive".into()),
            cull: primitive.cull,
            pick_id: None,
            ..Default::default()
        };
        frame.command_list.push(SceneCommand::from_draw(
            format!("ClassificationPrimitive-pick-plain-{index}-{color}"),
            context.clone(),
            Pass::TerrainClassification,
            metadata,
            options,
        ));
    }

    pub fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if self.pending.as_ref().is_some_and(|options| options.geometry_instances.is_empty())
            || (!frame.passes.render && !frame.passes.pick)
            || (frame.scene_3d_only && frame.mode != SceneMode::Scene3D)
        {
            return Ok(());
        }
        if self.primitive.is_none() {
            self.initialize(frame)?;
        }
        self.validate_appearance()?;
        let ready_before = self.ready;
        let start = frame.command_list.len();
        if let Some(primitive) = self.primitive.as_mut() {
            ScenePrimitive::update(primitive, frame)?;
        }
        frame.command_list.truncate(start);
        if !self.ready && self.primitive.as_ref().is_some_and(|primitive| primitive.state() == PrimitiveState::Complete)
        {
            self.ready = true;
        }
        if !ready_before && self.ready {
            frame.after_render.push(Box::new(|| Ok(true)));
        }
        self.queue_commands(frame)
    }

    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        self.destroyed = true;
        self.pending = None;
        self.shaders = None;
        self.ground_bounding_volumes.clear();
        self.ground_bounding_volumes_2d.clear();
        self.primitive.take().map_or(Ok(()), |mut primitive| ScenePrimitive::destroy(&mut primitive))
    }
}

impl<G: Gl + 'static> ScenePrimitive for ClassificationPrimitive<G> {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        Self::update(self, frame)
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<()> {
        Ok(())
    }
    fn destroy(&mut self) -> Result<()> {
        Self::destroy(self)
    }
}

pub fn stencil_depth_state(enabled: bool, mask_tiles: bool) -> RenderState {
    let function = if mask_tiles { gl::EQUAL } else { gl::ALWAYS };
    RenderState {
        color_mask: ColorMask { red: false, green: false, blue: false, alpha: false },
        depth_test: DepthTest { enabled: true, func: gl::LEQUAL },
        depth_mask: false,
        stencil_mask: i64::from(CLASSIFICATION_MASK),
        stencil_test: StencilTest {
            enabled,
            front_function: function,
            back_function: function,
            reference: CESIUM_3D_TILE_MASK,
            mask: i64::from(CESIUM_3D_TILE_MASK),
            front_operation: StencilOperation { z_fail: gl::DECR_WRAP, ..Default::default() },
            back_operation: StencilOperation { z_fail: gl::INCR_WRAP, ..Default::default() },
        },
        ..Default::default()
    }
}

pub fn classification_color_state(enabled: bool) -> RenderState {
    let clear = StencilOperation { fail: gl::ZERO, z_fail: gl::ZERO, z_pass: gl::ZERO };
    RenderState {
        depth_mask: false,
        stencil_mask: i64::from(CLASSIFICATION_MASK),
        stencil_test: StencilTest {
            enabled,
            front_function: gl::NOTEQUAL,
            back_function: gl::NOTEQUAL,
            front_operation: clear.clone(),
            back_operation: clear,
            reference: 0,
            mask: i64::from(CLASSIFICATION_MASK),
        },
        blending: blending_state::pre_multiplied_alpha_blend(),
        ..Default::default()
    }
}

pub fn classification_pick_state() -> RenderState {
    let mut state = classification_color_state(true);
    state.blending = Default::default();
    state
}

/// `PrimitiveOptions` re-export used by ground wrappers.
pub type ClassificationPrimitivePrimitiveOptions = PrimitiveOptions;
