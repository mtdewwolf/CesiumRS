//! CesiumJS `EllipsoidPrimitive`: a renderable 3D ellipsoid evaluated via ray
//! casting against its bounding box geometry. Follows Cesium step for step:
//! translucency-driven render state, a shared unit-box vertex array, a
//! bounding sphere cached across radii/model-matrix/center changes, color and
//! pick shader recompilation on material/lighting/log-depth/translucency
//! changes, and per-pass command queuing. Checked against a SwiftShader
//! capture of the pinned engine in `tests/scene_single_renderables.rs`.

use std::{collections::BTreeMap, rc::Rc};

use super::{
    blending_state,
    frame_state::FrameState,
    material::{Material, MaterialValue, COLOR_TYPE},
    scene::{Result, SceneCommand, SceneDrawOptions, ScenePrimitive},
    scene_mode::SceneMode,
    CullFace,
};
use crate::{
    core::{BoundingSphere, BoxGeometry, Cartesian3, Matrix4, RuntimeError, VertexFormat},
    renderer::{
        render_state::DepthTest, BufferUsage, Context, DrawMetadata, GeometryUploadOptions, Gl, Pass, PickColorQualifier,
        PickId, RenderState, ShaderProgram, ShaderProgramOptions, ShaderSource, UniformValue, VertexArray,
    },
    shaders::root::{ELLIPSOID_FS, ELLIPSOID_VS},
};

/// Options for configuring an [`EllipsoidPrimitive`].
#[derive(Clone, Debug)]
pub struct EllipsoidPrimitiveOptions {
    pub center: Option<Cartesian3>,
    pub radii: Option<Cartesian3>,
    pub model_matrix: Option<Matrix4>,
    pub show: Option<bool>,
    pub material: Option<Material>,
    pub id: Option<String>,
    pub debug_show_bounding_volume: Option<bool>,
    pub only_sun_lighting: Option<bool>,
    pub depth_test_enabled: Option<bool>,
}

impl Default for EllipsoidPrimitiveOptions {
    fn default() -> Self {
        Self {
            center: Some(Cartesian3::ZERO),
            radii: None,
            model_matrix: Some(Matrix4::IDENTITY),
            show: Some(true),
            material: None,
            id: None,
            debug_show_bounding_volume: Some(false),
            only_sun_lighting: Some(false),
            depth_test_enabled: Some(true),
        }
    }
}

/// The resolved state of one queued draw command, for capture comparison.
#[derive(Clone, Debug, PartialEq)]
pub struct EllipsoidCommand {
    pub pass: Pass,
    pub pick_only: bool,
    pub render_state: RenderState,
    pub bounding_volume: BoundingSphere,
    pub model_matrix: Matrix4,
    pub debug_show_bounding_volume: bool,
    pub execute_in_closest_frustum: bool,
    /// Uniforms in submission order: `u_radii`, `u_oneOverEllipsoidRadiiSquared`,
    /// (for pick) `czm_pickColor`, then the material's generated uniforms.
    /// Values are kept at full `f64` precision, as the engine records them.
    pub uniforms: Vec<(String, Vec<f64>)>,
    pub vs_defines: Vec<String>,
    pub fs_defines: Vec<String>,
    pub pick_color_qualifier: Option<PickColorQualifier>,
}

#[allow(dead_code)]
struct PickObject {
    id: Option<String>,
}

/// A renderable 3D ellipsoid primitive that casts rays against an enclosing box.
pub struct EllipsoidPrimitive<G: Gl + 'static> {
    pub center: Cartesian3,
    pub radii: Option<Cartesian3>,
    pub model_matrix: Matrix4,
    pub show: bool,
    pub material: Material,
    pub id: Option<String>,
    pub debug_show_bounding_volume: bool,
    pub only_sun_lighting: bool,
    pub depth_test_enabled: bool,

    center_cached: Cartesian3,
    radii_cached: Cartesian3,
    model_matrix_cached: Matrix4,
    computed_model_matrix: Matrix4,
    one_over_ellipsoid_radii_squared: Cartesian3,
    bounding_sphere: BoundingSphere,

    translucent: Option<bool>,
    use_log_depth: bool,
    only_sun_lighting_cached: bool,
    id_cached: Option<String>,
    material_cached: Material,

    render_state: Option<RenderState>,
    vertex_array: Option<Rc<VertexArray<G>>>,
    shader_program: Option<Rc<ShaderProgram<G>>>,
    pick_shader_program: Option<Rc<ShaderProgram<G>>>,
    pick_id: Option<PickId>,

    commands: Vec<EllipsoidCommand>,
    destroyed: bool,
}

impl<G: Gl + 'static> EllipsoidPrimitive<G> {
    /// Creates a new `EllipsoidPrimitive` from the given options.
    pub fn new(options: EllipsoidPrimitiveOptions) -> Self {
        let center = options.center.unwrap_or(Cartesian3::ZERO);
        let radii = options.radii;
        let model_matrix = options.model_matrix.unwrap_or(Matrix4::IDENTITY);
        let show = options.show.unwrap_or(true);
        let material = options.material.unwrap_or_else(|| Material::from_type(COLOR_TYPE));
        let id = options.id;
        let debug_show_bounding_volume = options.debug_show_bounding_volume.unwrap_or(false);
        let only_sun_lighting = options.only_sun_lighting.unwrap_or(false);
        let depth_test_enabled = options.depth_test_enabled.unwrap_or(true);
        let material_cached = material.clone();

        Self {
            center,
            radii,
            model_matrix,
            show,
            material,
            id,
            debug_show_bounding_volume,
            only_sun_lighting,
            depth_test_enabled,
            center_cached: Cartesian3::ZERO,
            radii_cached: Cartesian3::ZERO,
            model_matrix_cached: Matrix4::ZERO,
            computed_model_matrix: Matrix4::ZERO,
            one_over_ellipsoid_radii_squared: Cartesian3::ZERO,
            bounding_sphere: BoundingSphere::default(),
            translucent: None,
            use_log_depth: false,
            only_sun_lighting_cached: false,
            id_cached: None,
            material_cached,
            render_state: None,
            vertex_array: None,
            shader_program: None,
            pick_shader_program: None,
            pick_id: None,
            commands: Vec::new(),
            destroyed: false,
        }
    }

    /// The computed 4x4 matrix including the center offset.
    pub fn computed_model_matrix(&self) -> &Matrix4 {
        &self.computed_model_matrix
    }

    /// Vector containing 1 / (radii^2) per component.
    pub fn one_over_ellipsoid_radii_squared(&self) -> Cartesian3 {
        self.one_over_ellipsoid_radii_squared
    }

    /// The bounding sphere enclosing the primitive in world coordinates.
    pub fn bounding_sphere(&self) -> &BoundingSphere {
        &self.bounding_sphere
    }

    /// The commands queued by the last update, in queue order.
    pub fn commands(&self) -> &[EllipsoidCommand] {
        &self.commands
    }

    /// Clears the queued command snapshot without running an update.
    pub fn clear_commands(&mut self) {
        self.commands.clear();
    }

    pub fn vertex_array(&self) -> Option<&Rc<VertexArray<G>>> {
        self.vertex_array.as_ref()
    }

    pub fn shader_program(&self) -> Option<&Rc<ShaderProgram<G>>> {
        self.shader_program.as_ref()
    }

    pub fn pick_shader_program(&self) -> Option<&Rc<ShaderProgram<G>>> {
        self.pick_shader_program.as_ref()
    }

    /// The pick color, once a pick pass has allocated the pick id.
    pub fn pick_id(&self) -> Option<&PickId> {
        self.pick_id.as_ref()
    }

    /// Returns `true` if this primitive has been destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    fn check(&self) -> Result<()> {
        if self.destroyed {
            Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."))
        } else {
            Ok(())
        }
    }

    /// Cesium's `EllipsoidPrimitive.prototype.update`.
    pub fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.check()?;
        self.commands.clear();

        if !self.show || frame.mode != SceneMode::Scene3D || self.radii.is_none() {
            return Ok(());
        }

        let context = super::depth_plane::context_of::<G>(frame)?;
        let translucent = self.material.is_translucent();
        let translucency_changed = self.translucent != Some(translucent);

        if self.render_state.is_none() || translucency_changed {
            self.translucent = Some(translucent);
            let fragment_depth = context.capabilities()?.fragment_depth;
            self.render_state = Some(RenderState {
                // Cull front faces - not back faces - so the ellipsoid doesn't
                // disappear if the viewer enters the bounding box.
                cull: crate::renderer::render_state::Cull { enabled: true, face: CullFace::Front.value() },
                depth_test: DepthTest { enabled: self.depth_test_enabled, ..Default::default() },
                // Only write depth when EXT_frag_depth is supported since the
                // depth for the bounding box is wrong; it is not the true depth
                // of the ray casted ellipsoid.
                depth_mask: !translucent && fragment_depth,
                blending: if translucent { blending_state::alpha_blend() } else { Default::default() },
                ..Default::default()
            });
        }

        if self.vertex_array.is_none() {
            self.vertex_array = Some(Rc::new(get_vertex_array(&context)?));
        }

        let radii = self.radii.expect("checked above");
        let mut bounding_sphere_dirty = false;

        if self.radii_cached != radii {
            self.radii_cached = radii;
            let r = &mut self.one_over_ellipsoid_radii_squared;
            r.x = 1.0 / (radii.x * radii.x);
            r.y = 1.0 / (radii.y * radii.y);
            r.z = 1.0 / (radii.z * radii.z);
            bounding_sphere_dirty = true;
        }

        if self.model_matrix != self.model_matrix_cached || self.center != self.center_cached {
            self.model_matrix_cached = self.model_matrix;
            self.center_cached = self.center;
            // Translate model coordinates used for rendering such that the
            // origin is the center of the ellipsoid.
            self.computed_model_matrix = Matrix4::multiply(&self.model_matrix, &Matrix4::from_translation(self.center));
            bounding_sphere_dirty = true;
        }

        if bounding_sphere_dirty {
            let base = BoundingSphere::new(Cartesian3::ZERO, radii.maximum_component());
            self.bounding_sphere = BoundingSphere::transform(&base, &self.computed_model_matrix);
        }

        let material_changed = !same_material(&self.material, &self.material_cached);
        self.material_cached = self.material.clone();
        let lighting_changed = self.only_sun_lighting != self.only_sun_lighting_cached;
        self.only_sun_lighting_cached = self.only_sun_lighting;

        let use_log_depth = frame.use_log_depth;
        let use_log_depth_changed = self.use_log_depth != use_log_depth;
        self.use_log_depth = use_log_depth;

        let recompile =
            material_changed || lighting_changed || translucency_changed || use_log_depth_changed;

        if recompile {
            self.shader_program = Some(context.with_shader_cache(|c| {
                c.replace_shader_program(self.shader_program.clone(), &ellipsoid_shader_options(
                    &self.material,
                    self.only_sun_lighting,
                    translucent,
                    context.capabilities()?.fragment_depth,
                    self.use_log_depth,
                    false,
                ))
            })?);
        }

        let passes = frame.passes;

        if passes.render {
            let mut cmd_uniforms: Vec<(String, Vec<f64>)> = vec![
                ("u_radii".to_string(), radii_f64(radii)),
                ("u_oneOverEllipsoidRadiiSquared".to_string(), radii_f64(self.one_over_ellipsoid_radii_squared)),
            ];
            cmd_uniforms.extend(material_uniform_f64(&self.material));
            let mut renderer_uniforms: Vec<(String, UniformValue)> = vec![
                ("u_radii".to_string(), radii_uniform(radii)),
                ("u_oneOverEllipsoidRadiiSquared".to_string(), radii_uniform(self.one_over_ellipsoid_radii_squared)),
            ];
            renderer_uniforms.extend(material_uniform_values(&self.material));

            let mut metadata = DrawMetadata::default();
            metadata.bounding_volume = Some(self.bounding_sphere);
            metadata.model_matrix = Some(self.computed_model_matrix);
            metadata.debug_show_bounding_volume = self.debug_show_bounding_volume;
            metadata.execute_in_closest_frustum = translucent;

            let pass = if translucent { Pass::Translucent } else { Pass::Opaque };
            let program = self.shader_program.clone().expect("color shader compiled");
            self.commands.push(EllipsoidCommand {
                pass,
                pick_only: false,
                render_state: self.render_state.clone().expect("render state"),
                bounding_volume: self.bounding_sphere,
                model_matrix: self.computed_model_matrix,
                debug_show_bounding_volume: self.debug_show_bounding_volume,
                execute_in_closest_frustum: translucent,
                uniforms: cmd_uniforms,
                vs_defines: vs_defines(&program),
                fs_defines: fs_defines(&program),
                pick_color_qualifier: pick_qualifier(&program),
            });

            let mut options = SceneDrawOptions::new(self.vertex_array.clone().expect("vertex array"), program);
            options.state = self.render_state.clone().expect("render state");
            options.uniforms = renderer_uniforms;
            frame.command_list.push(SceneCommand::from_draw(
                "ellipsoid-primitive-color",
                context.clone(),
                pass,
                metadata,
                options,
            ));
        }

        if passes.pick {
            if self.pick_id.is_none() || self.id_cached != self.id {
                self.id_cached = self.id.clone();
                self.pick_id = None;
                let pick_id = context.create_pick_id(Rc::new(PickObject { id: self.id.clone() }))?;
                self.pick_id = Some(pick_id);
            }

            if material_changed || lighting_changed || self.pick_shader_program.is_none() || use_log_depth_changed {
                self.pick_shader_program = Some(context.with_shader_cache(|c| {
                    c.replace_shader_program(self.pick_shader_program.clone(), &ellipsoid_shader_options(
                        &self.material,
                        self.only_sun_lighting,
                        translucent,
                        context.capabilities()?.fragment_depth,
                        self.use_log_depth,
                        true,
                    ))
                })?);
            }

            let pick_color = self.pick_id.as_ref().expect("pick id").color;
            let mut cmd_uniforms: Vec<(String, Vec<f64>)> = vec![
                ("u_radii".to_string(), radii_f64(radii)),
                ("u_oneOverEllipsoidRadiiSquared".to_string(), radii_f64(self.one_over_ellipsoid_radii_squared)),
                ("czm_pickColor".to_string(), color_f64(pick_color)),
            ];
            cmd_uniforms.extend(material_uniform_f64(&self.material));
            let mut renderer_uniforms: Vec<(String, UniformValue)> = vec![
                ("u_radii".to_string(), radii_uniform(radii)),
                ("u_oneOverEllipsoidRadiiSquared".to_string(), radii_uniform(self.one_over_ellipsoid_radii_squared)),
                ("czm_pickColor".to_string(), color_uniform(pick_color)),
            ];
            renderer_uniforms.extend(material_uniform_values(&self.material));

            let mut metadata = DrawMetadata::default();
            metadata.bounding_volume = Some(self.bounding_sphere);
            metadata.model_matrix = Some(self.computed_model_matrix);
            metadata.pick_only = true;
            metadata.execute_in_closest_frustum = translucent;

            let pass = if translucent { Pass::Translucent } else { Pass::Opaque };
            let program = self.pick_shader_program.clone().expect("pick shader compiled");
            self.commands.push(EllipsoidCommand {
                pass,
                pick_only: true,
                render_state: self.render_state.clone().expect("render state"),
                bounding_volume: self.bounding_sphere,
                model_matrix: self.computed_model_matrix,
                debug_show_bounding_volume: false,
                execute_in_closest_frustum: translucent,
                uniforms: cmd_uniforms,
                vs_defines: vs_defines(&program),
                fs_defines: fs_defines(&program),
                pick_color_qualifier: pick_qualifier(&program),
            });

            let mut options = SceneDrawOptions::new(self.vertex_array.clone().expect("vertex array"), program);
            options.state = self.render_state.clone().expect("render state");
            options.uniforms = renderer_uniforms;
            frame.command_list.push(SceneCommand::from_draw(
                "ellipsoid-primitive-pick",
                context.clone(),
                pass,
                metadata,
                options,
            ));
        }

        Ok(())
    }

    /// Cesium's `EllipsoidPrimitive.prototype.destroy`.
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Ok(());
        }
        if let Some(va) = self.vertex_array.take() {
            va.destroy()?;
        }
        self.shader_program = None;
        self.pick_shader_program = None;
        self.pick_id = None;
        self.commands.clear();
        self.destroyed = true;
        Ok(())
    }
}

/// `_material !== this.material`: the material is replaced when its type or
/// uniform values change (the only observable difference in the owned value).
fn same_material(a: &Material, b: &Material) -> bool {
    a.type_name == b.type_name && a.uniforms == b.uniforms
}

fn radii_f64(v: Cartesian3) -> Vec<f64> {
    vec![v.x, v.y, v.z]
}

fn color_f64(c: crate::core::Color) -> Vec<f64> {
    vec![c.red, c.green, c.blue, c.alpha]
}

fn radii_uniform(v: Cartesian3) -> UniformValue {
    UniformValue::Float(vec![v.x as f32, v.y as f32, v.z as f32])
}

fn color_uniform(c: crate::core::Color) -> UniformValue {
    UniformValue::Float(vec![c.red as f32, c.green as f32, c.blue as f32, c.alpha as f32])
}

fn vs_defines<G: Gl>(program: &ShaderProgram<G>) -> Vec<String> {
    program.source_options().map_or_else(Vec::new, |o| o.vertex_shader_source.defines.clone())
}

fn fs_defines<G: Gl>(program: &ShaderProgram<G>) -> Vec<String> {
    program.source_options().map_or_else(Vec::new, |o| o.fragment_shader_source.defines.clone())
}

fn pick_qualifier<G: Gl>(program: &ShaderProgram<G>) -> Option<PickColorQualifier> {
    program.source_options().and_then(|o| o.fragment_shader_source.pick_color_qualifier)
}

/// `getVertexArray`: the shared position-only unit box, cached on the context.
fn get_vertex_array<G: Gl + 'static>(context: &Context<G>) -> Result<VertexArray<G>> {
    let geometry = BoxGeometry::from_dimensions(Cartesian3::new(2.0, 2.0, 2.0), VertexFormat::POSITION_ONLY, None)
        .and_then(|b| b.create_geometry())
        .ok_or_else(|| RuntimeError::new("the unit box is valid"))?;
    VertexArray::from_geometry(
        context,
        &geometry,
        GeometryUploadOptions {
            buffer_usage: BufferUsage::StaticDraw,
            attribute_locations: [("position".to_string(), 0)].into_iter().collect(),
            interleave: true,
            vertex_array_attributes: vec![],
        },
    )
}

/// The color or pick shader, matching `EllipsoidPrimitive.js` verbatim.
fn ellipsoid_shader_options(
    material: &Material,
    only_sun_lighting: bool,
    translucent: bool,
    fragment_depth: bool,
    use_log_depth: bool,
    pick: bool,
) -> ShaderProgramOptions {
    let mut vs = ShaderSource { sources: vec![ELLIPSOID_VS.source.to_string()], ..Default::default() };
    let mut fs = ShaderSource {
        sources: vec![material.shader_source.clone(), ELLIPSOID_FS.source.to_string()],
        ..Default::default()
    };
    if pick {
        fs.pick_color_qualifier = Some(PickColorQualifier::Uniform);
    }
    if only_sun_lighting {
        fs.defines.push("ONLY_SUN_LIGHTING".into());
    }
    if !translucent && fragment_depth {
        fs.defines.push("WRITE_DEPTH".into());
    }
    if use_log_depth {
        vs.defines.push("LOG_DEPTH".into());
        fs.defines.push("LOG_DEPTH".into());
    }
    ShaderProgramOptions {
        vertex_shader_source: vs,
        fragment_shader_source: fs,
        attribute_locations: BTreeMap::from([("position".to_string(), 0)]),
    }
}

/// The material's numeric uniforms by generated name, at full `f64` precision.
fn material_uniform_f64(material: &Material) -> Vec<(String, Vec<f64>)> {
    material
        .uniform_bindings()
        .into_iter()
        .filter_map(|(name, value)| value.numbers().map(|numbers| (name, numbers)))
        .collect()
}

/// The material's numeric uniforms by generated name.
fn material_uniform_values(material: &Material) -> Vec<(String, UniformValue)> {
    material
        .uniform_bindings()
        .into_iter()
        .filter_map(|(name, value)| {
            let numbers = || value.numbers().unwrap_or_default().into_iter().map(|v| v as f32).collect();
            let uniform = match &value {
                MaterialValue::Bool(v) => UniformValue::Int(vec![i32::from(*v)]),
                MaterialValue::IntVec3(v) => UniformValue::Int(vec![v.x as i32, v.y as i32, v.z as i32]),
                MaterialValue::Mat2(_) => UniformValue::Matrix { dimension: 2, values: numbers() },
                MaterialValue::Mat3(_) => UniformValue::Matrix { dimension: 3, values: numbers() },
                MaterialValue::Mat4(_) => UniformValue::Matrix { dimension: 4, values: numbers() },
                MaterialValue::Channels(_) | MaterialValue::Sampler2D(_) | MaterialValue::SamplerCube(_) => return None,
                _ => UniformValue::Float(numbers()),
            };
            Some((name, uniform))
        })
        .collect()
}

impl<G: Gl + 'static> ScenePrimitive for EllipsoidPrimitive<G> {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        EllipsoidPrimitive::update(self, frame)
    }

    fn post_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update_for_pass(&mut self, frame: &mut FrameState, _pass: super::Cesium3DTilePass) -> Result<()> {
        EllipsoidPrimitive::update(self, frame)
    }

    fn destroy(&mut self) -> Result<()> {
        EllipsoidPrimitive::destroy(self)
    }
}
