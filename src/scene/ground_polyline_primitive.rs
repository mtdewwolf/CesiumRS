//! CesiumJS `GroundPolylinePrimitive`: terrain- and 3D-Tiles-clamped polylines
//! rendered as screen-space shadow volumes.
//!
//! The wrapper prepares [`GroundPolylineGeometry`](crate::core::GroundPolylineGeometry)
//! descriptions for the frame projection, then builds one draw command per
//! uploaded geometry with Cesium's inverted-winding, stencil-masked render
//! states.

use super::{
    blending_state,
    classification_primitive::ClassificationPrimitiveOptions,
    material::material_uniform_values,
    primitive::{Primitive, PrimitiveOptions},
    primitive_shader::ShaderRewrites,
    primitive_state::PrimitiveState,
    scene::{Result, SceneCommand, SceneDrawOptions, ScenePrimitive},
    stencil_constants::CESIUM_3D_TILE_MASK,
    Cesium3DTilePass, ClassificationType, FrameState, SceneMode,
};
use crate::{
    core::{
        webgl_constants as gl, ApproximateTerrainHeights, ComponentDatatype, GeometryInstanceAttribute, Matrix4,
        RuntimeError, TypedArray,
    },
    renderer::{
        render_state::{Cull, StencilTest},
        Context, DrawMetadata, Gl, Pass, RenderState, ShaderProgram, ShaderProgramOptions, ShaderSource, UniformValue,
    },
    shaders::root::{
        POLYLINE_SHADOW_VOLUME_FS, POLYLINE_SHADOW_VOLUME_MORPH_FS, POLYLINE_SHADOW_VOLUME_MORPH_VS,
        POLYLINE_SHADOW_VOLUME_VS,
    },
    workers::GeometryDescription,
};
use std::{collections::BTreeMap, rc::Rc};

pub type GroundPolylinePrimitiveOptions = ClassificationPrimitiveOptions;

struct PolylineShaders<G: Gl> {
    program: Rc<ShaderProgram<G>>,
    attribute_locations: BTreeMap<String, u32>,
}

/// A ground polyline primitive.
pub struct GroundPolylinePrimitive<G: Gl + 'static> {
    pub classification_type: ClassificationType,
    pub debug_show_shadow_volume: bool,
    pending: Option<PrimitiveOptions>,
    primitive: Option<Primitive<G>>,
    shaders: Option<PolylineShaders<G>>,
    has_per_instance_colors: bool,
    ready: bool,
    destroyed: bool,
}

impl<G: Gl + 'static> GroundPolylinePrimitive<G> {
    pub fn new(options: GroundPolylinePrimitiveOptions) -> Self {
        let mut primitive = options.primitive;
        primitive.compress_vertices = Some(false);
        Self {
            classification_type: options.classification_type,
            debug_show_shadow_volume: options.debug_show_shadow_volume,
            pending: Some(primitive),
            primitive: None,
            shaders: None,
            has_per_instance_colors: true,
            ready: false,
            destroyed: false,
        }
    }

    pub fn is_supported(frame: &FrameState) -> bool {
        frame.context_capabilities.depth_texture
    }

    pub fn initialize_terrain_heights() -> &'static ApproximateTerrainHeights {
        ApproximateTerrainHeights::bundled()
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn set_show(&mut self, show: bool) {
        if let Some(options) = &mut self.pending {
            options.show = Some(show);
        }
        if let Some(primitive) = &mut self.primitive {
            primitive.show = show;
        }
    }

    pub fn set_debug_show_bounding_volume(&mut self, show: bool) {
        if let Some(options) = &mut self.pending {
            options.debug_show_bounding_volume = Some(show);
        }
        if let Some(primitive) = &mut self.primitive {
            primitive.debug_show_bounding_volume = show;
        }
    }

    pub fn set_appearance(&mut self, appearance: Box<dyn super::Appearance>) {
        if let Some(options) = &mut self.pending {
            options.appearance = Some(appearance);
        } else if let Some(primitive) = &mut self.primitive {
            primitive.appearance = Some(appearance);
            self.shaders = None;
        }
    }

    pub fn get_geometry_instance_attributes(
        &mut self,
        id: &str,
    ) -> Result<Option<super::GeometryInstanceAttributes<'_, G>>> {
        self.primitive
            .as_mut()
            .ok_or_else(|| RuntimeError::new("must call update before calling getGeometryInstanceAttributes"))?
            .get_geometry_instance_attributes(id)
    }

    fn initialize(&mut self, frame: &FrameState) -> Result<()> {
        let mut options = self.pending.take().expect("pending polyline options");
        prepare_polylines(&mut options, frame)?;
        if options.appearance.is_none() {
            options.appearance = Some(Box::new(super::PolylineMaterialAppearance::new(Default::default())));
        }
        self.primitive = Some(Primitive::new(options));
        Ok(())
    }

    fn build_shaders(&mut self, context: &Context<G>, frame: &FrameState, primitive: &mut Primitive<G>) -> Result<()> {
        if self.shaders.is_some() {
            return Ok(());
        }
        let appearance = primitive
            .appearance
            .as_deref()
            .ok_or_else(|| RuntimeError::new("GroundPolylinePrimitive requires an appearance"))?;
        let material_shader = appearance.material().map(|material| material.shader_source.clone());
        let morphing = frame.mode == SceneMode::Morphing;
        let columbus = frame.mode != SceneMode::Scene3D && !morphing;
        let mut defines =
            vec![format!("GLOBE_MINIMUM_ALTITUDE {:.1}", frame.uniform.map_projection.ellipsoid().minimum_radius)];
        let mut color_define = String::new();
        if let Some(source) = &material_shader {
            if source.contains("in float v_polylineAngle;") {
                defines.push("ANGLE_VARYING".into());
            }
            if source.contains("in float v_width;") {
                defines.push("WIDTH_VARYING".into());
            }
        } else {
            color_define = "PER_INSTANCE_COLOR".into();
            defines.push(color_define.clone());
        }
        if columbus {
            defines.push("COLUMBUS_VIEW_2D".into());
        }
        if morphing {
            defines.push(format!(
                "MAX_TERRAIN_HEIGHT {:.1}",
                crate::core::approximate_terrain_heights::DEFAULT_MAX_TERRAIN_HEIGHT
            ));
        }
        let mut fragment_defines = vec![color_define];
        if self.debug_show_shadow_volume {
            fragment_defines.push("DEBUG_SHOW_VOLUME".into());
        }
        let (vertex_source, fragment_source) = if morphing {
            (POLYLINE_SHADOW_VOLUME_MORPH_VS.source, POLYLINE_SHADOW_VOLUME_MORPH_FS.source)
        } else {
            (POLYLINE_SHADOW_VOLUME_VS.source, POLYLINE_SHADOW_VOLUME_FS.source)
        };
        let vertex = primitive.batch_vertex_shader(vertex_source);
        let rewrites = ShaderRewrites {
            offset: false,
            show: primitive.has_batch_attribute("show"),
            distance_display_condition: primitive.has_batch_attribute("distanceDisplayCondition"),
            color: false,
            depth_fail_color: false,
            compress_vertices: false,
            scene_3d_only: frame.scene_3d_only,
            rtc: false,
        };
        let (vertex, _) = rewrites.apply(vertex, String::new(), false)?;
        let mut fragment = ShaderSource {
            sources: material_shader.into_iter().chain(std::iter::once(fragment_source.to_string())).collect(),
            defines: fragment_defines,
            ..Default::default()
        };
        // The batch callback's pick-color helper is only needed when the
        // fragment stage reads it; keep it declared for polyline picking.
        fragment.sources.insert(0, "in vec4 v_pickColor;".into());
        let program = ShaderProgram::from_sources(
            context,
            &ShaderProgramOptions {
                vertex_shader_source: ShaderSource { sources: vec![vertex], defines, ..Default::default() },
                fragment_shader_source: fragment,
                attribute_locations: primitive.uploaded_attribute_locations().clone(),
            },
        )?;
        self.shaders = Some(PolylineShaders {
            program: Rc::new(program),
            attribute_locations: primitive.uploaded_attribute_locations().clone(),
        });
        let _ = appearance;
        Ok(())
    }

    fn queue_commands(&mut self, frame: &mut FrameState) -> Result<()> {
        let Some(mut primitive) = self.primitive.take() else { return Ok(()) };
        let result = self.queue_commands_with(frame, &mut primitive);
        self.primitive = Some(primitive);
        result
    }

    fn queue_commands_with(&mut self, frame: &mut FrameState, primitive: &mut Primitive<G>) -> Result<()> {
        if primitive.state() != PrimitiveState::Complete
            || !primitive.show
            || (!frame.passes.render && !frame.passes.pick)
        {
            return Ok(());
        }
        let context = primitive
            .upload_context()
            .cloned()
            .ok_or_else(|| RuntimeError::new("GroundPolylinePrimitive requires a renderer context"))?;
        self.build_shaders(&context, frame, primitive)?;
        let shaders = self.shaders.as_ref().expect("built above");
        let vertices = primitive.uploaded_vertex_arrays().to_vec();
        let spheres = primitive.bounding_spheres().to_vec();
        let render_state = polyline_render_state(frame.mode, false);
        let render_state_tiles = polyline_render_state(frame.mode, true);
        let queue_terrain = self.classification_type != ClassificationType::Cesium3DTile;
        let queue_tiles = self.classification_type != ClassificationType::Terrain && frame.mode != SceneMode::Morphing;
        let uniforms = polyline_uniforms(primitive);
        for (index, vertex_array) in vertices.iter().enumerate() {
            let volume = spheres.get(index).copied().flatten().map(|sphere| sphere.transform(&primitive.model_matrix));
            if frame.passes.render {
                if queue_terrain {
                    self.push_command(
                        frame,
                        &context,
                        primitive,
                        shaders,
                        vertex_array.clone(),
                        render_state.clone(),
                        Pass::TerrainClassification,
                        volume,
                        uniforms.clone(),
                        index,
                        frame.passes.pick,
                    );
                }
                if queue_tiles {
                    self.push_command(
                        frame,
                        &context,
                        primitive,
                        shaders,
                        vertex_array.clone(),
                        render_state_tiles.clone(),
                        Pass::Cesium3DTileClassification,
                        volume,
                        uniforms.clone(),
                        index,
                        frame.passes.pick,
                    );
                }
            } else if frame.passes.pick {
                self.push_command(
                    frame,
                    &context,
                    primitive,
                    shaders,
                    vertex_array.clone(),
                    render_state.clone(),
                    Pass::TerrainClassification,
                    volume,
                    uniforms.clone(),
                    index,
                    true,
                );
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn push_command(
        &self,
        frame: &mut FrameState,
        context: &Context<G>,
        primitive: &Primitive<G>,
        shaders: &PolylineShaders<G>,
        vertices: Rc<crate::renderer::VertexArray<G>>,
        state: RenderState,
        pass: Pass,
        volume: Option<crate::core::BoundingSphere>,
        uniforms: Vec<(String, UniformValue)>,
        index: usize,
        pick: bool,
    ) {
        let mut options = SceneDrawOptions::new(vertices, shaders.program.clone());
        options.primitive_type = primitive.command_primitive_type();
        options.state = state;
        options.uniforms = uniforms;
        if let Some(texture) = primitive.batch_texture() {
            options.textures.push(("batchTexture".into(), texture));
        }
        let metadata = DrawMetadata {
            bounding_volume: volume,
            model_matrix: Some(primitive.model_matrix),
            owner: Some("GroundPolylinePrimitive".into()),
            cull: primitive.cull,
            debug_show_bounding_volume: primitive.debug_show_bounding_volume,
            pick_id: pick.then(|| "czm_batchTable_pickColor(v_endPlaneNormalEcAndBatchId.w)".into()),
            ..Default::default()
        };
        let _ = &shaders.attribute_locations;
        frame.command_list.push(SceneCommand::from_draw(
            format!("GroundPolylinePrimitive-{index}"),
            context.clone(),
            pass,
            metadata,
            options,
        ));
    }

    pub fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if self.pending.as_ref().is_some_and(|options| options.geometry_instances.is_empty())
            && self.primitive.is_none()
        {
            return Ok(());
        }
        if !ApproximateTerrainHeights::bundled().initialized() {
            if self.pending.as_ref().is_some_and(|options| options.asynchronous == Some(false)) {
                return Err(RuntimeError::new(
                    "For synchronous GroundPolylinePrimitives, you must call GroundPolylinePrimitives.initializeTerrainHeights() and wait for the returned promise to resolve.",
                ));
            }
            return Ok(());
        }
        if self.primitive.is_none() {
            self.initialize(frame)?;
        }
        if let Some(primitive) = &self.primitive {
            let color_appearance =
                primitive.appearance.as_deref().is_some_and(|appearance| appearance.material().is_none());
            if color_appearance && !self.has_per_instance_colors {
                return Err(RuntimeError::new(
                    "All GeometryInstances must have color attributes to use PolylineColorAppearance with GroundPolylinePrimitive.",
                ));
            }
        }
        let start = frame.command_list.len();
        if let Some(primitive) = self.primitive.as_mut() {
            ScenePrimitive::update(primitive, frame)?;
        }
        frame.command_list.truncate(start);
        if !self.ready && self.primitive.as_ref().is_some_and(|primitive| primitive.state() == PrimitiveState::Complete)
        {
            self.ready = true;
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
        self.primitive.take().map_or(Ok(()), |mut primitive| ScenePrimitive::destroy(&mut primitive))
    }
}

impl<G: Gl + 'static> ScenePrimitive for GroundPolylinePrimitive<G> {
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

fn polyline_uniforms(primitive: &Primitive<impl Gl>) -> Vec<(String, UniformValue)> {
    let mut uniforms = Vec::new();
    if let Some(appearance) = primitive.appearance.as_deref() {
        uniforms.extend(appearance.uniforms());
        if let Some(material) = appearance.material() {
            uniforms.extend(material_uniform_values(material));
        }
    }
    uniforms.extend(primitive.batch_uniform_values());
    uniforms
}

pub(crate) fn prepare_polylines(options: &mut PrimitiveOptions, frame: &FrameState) -> Result<()> {
    let projection = frame.uniform.map_projection.as_ref();
    let mut has_colors = true;
    for (index, instance) in options.geometry_instances.iter_mut().enumerate() {
        let width = match options.geometry_descriptions.get_mut(&index) {
            Some(GeometryDescription::GroundPolylineGeometry(geometry)) => {
                geometry.scene_3d_only = frame.scene_3d_only;
                geometry.set_projection_and_ellipsoid(!projection.is_geographic(), *projection.ellipsoid());
                geometry.width
            }
            None if instance
                .geometry
                .as_ref()
                .is_some_and(|geometry| geometry.attributes.get("startHiAndForwardOffsetX").is_some()) =>
            {
                1.0
            }
            _ => return Err(RuntimeError::new("Ground polyline instances require GroundPolylineGeometry.")),
        };
        if !instance.attributes.iter().any(|(name, _)| name == "width") {
            instance.attributes.push((
                "width".into(),
                GeometryInstanceAttribute::new(
                    ComponentDatatype::UnsignedByte,
                    1,
                    Some(false),
                    TypedArray::Uint8(vec![width.trunc().rem_euclid(256.0) as u8]),
                ),
            ));
        }
        if !instance.attributes.iter().any(|(name, _)| name == "color") {
            has_colors = false;
        }
        instance.model_matrix = Matrix4::IDENTITY;
    }
    options.model_matrix = Some(Matrix4::IDENTITY);
    options.compress_vertices = Some(false);
    let _ = has_colors;
    Ok(())
}

pub fn polyline_render_state(mode: SceneMode, tiles: bool) -> RenderState {
    let morph = mode == SceneMode::Morphing;
    RenderState {
        cull: Cull { enabled: true, face: if morph { gl::FRONT } else { gl::BACK } },
        depth_test: crate::renderer::render_state::DepthTest { enabled: morph, ..Default::default() },
        depth_mask: false,
        blending: blending_state::pre_multiplied_alpha_blend(),
        stencil_test: if morph {
            StencilTest::default()
        } else {
            StencilTest {
                enabled: tiles,
                front_function: gl::EQUAL,
                back_function: gl::EQUAL,
                reference: CESIUM_3D_TILE_MASK,
                mask: i64::from(CESIUM_3D_TILE_MASK),
                ..Default::default()
            }
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::GlValue;

    struct TestGl;
    impl Gl for TestGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2]> {
            Ok([1, 1])
        }
        fn invoke(&self, _: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>> {
            Ok(GlValue::Null)
        }
    }

    #[test]
    fn replacement_commands_include_material_uniforms() {
        let options = PrimitiveOptions {
            appearance: Some(Box::new(super::super::PolylineMaterialAppearance::new(Default::default()))),
            ..Default::default()
        };
        let primitive = Primitive::<TestGl>::new(options);
        let uniforms = polyline_uniforms(&primitive);
        assert!(uniforms.iter().any(|(name, _)| name.starts_with("color")), "{uniforms:?}");
    }
}
