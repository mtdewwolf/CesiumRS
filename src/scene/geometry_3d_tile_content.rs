//! Geometry tiles decode packed solids into a shared indexed mesh. Each batch
//! classifies terrain or 3D Tiles with a depth-fail stencil pass and a color pass.
use super::{
    cesium_3d_tile_content::{read_u32, tile_payload},
    cesium_3d_tileset::{array, TileContentModelState},
    scene::{Result, SceneDrawOptions},
    *,
};
use crate::{
    core::*,
    renderer::{
        Context, DrawMetadata, GeometryUploadOptions, Gl, Pass, ShaderProgram, ShaderProgramOptions, ShaderSource,
        UniformValue, VertexArray,
    },
    workers::vector_tiles::{create_vector_tile_geometries, VectorTileGeometries, VectorTileGeometriesParameters},
};
use serde_json::Value;
use std::rc::Rc;

struct GeometryResources<G: Gl> {
    vertices: Rc<VertexArray<G>>,
    program: Rc<ShaderProgram<G>>,
}

pub struct Geometry3DTileContent<G: Gl + 'static> {
    pub tile: TileId,
    pub resource: Resource,
    pub center: Cartesian3,
    pub batch_table: Rc<Cesium3DTileBatchTable<G>>,
    mesh: Option<VectorTileGeometries>,
    model_matrix: Matrix4,
    classification: ClassificationType,
    highlight: Color,
    gpu: Option<GeometryResources<G>>,
    ready: bool,
}

fn section<'a>(rest: &mut &'a [u8], length: usize) -> Result<&'a [u8]> {
    let (data, tail) =
        rest.split_at_checked(length).ok_or_else(|| RuntimeError::new("Truncated geometry tile section"))?;
    *rest = tail;
    Ok(data)
}
fn section_json(bytes: &[u8]) -> Result<Value> {
    serde_json::from_slice(bytes).map_err(|e| RuntimeError::new(format!("Invalid geometry table JSON: {e}")))
}
fn components<'a, const N: usize>(body: &'a [u8], descriptor: &Value, count: usize) -> Result<&'a [[u8; N]]> {
    let offset = descriptor["byteOffset"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| RuntimeError::new("Geometry field needs a byteOffset"))?;
    let data = body
        .get(offset..)
        .and_then(|b| b.as_chunks::<N>().0.get(..count))
        .ok_or_else(|| RuntimeError::new("Geometry field exceeds feature table"))?;
    Ok(data)
}
impl<G: Gl + 'static> Geometry3DTileContent<G> {
    pub fn from_bytes(
        tile: TileId,
        resource: Resource,
        bytes: &[u8],
        offset: usize,
        max_texture_size: u32,
    ) -> Result<Self> {
        let data = tile_payload(bytes, offset, b"geom")?;
        let mut center = Cartesian3::ZERO;
        let mut count = 0usize;
        let mut mesh = None;
        let mut batch_json = None;
        let mut batch_binary = None;
        if !data.is_empty() {
            let mut rest = data.get(28..).ok_or_else(|| RuntimeError::new("Invalid geometry tile header"))?;
            let table = section_json(section(&mut rest, read_u32(data, 12)?)?)?;
            let body = section(&mut rest, read_u32(data, 16)?)?;
            let json = section(&mut rest, read_u32(data, 20)?)?;
            if !json.is_empty() {
                batch_json = Some(section_json(json)?);
            }
            let binary = section(&mut rest, read_u32(data, 24)?)?;
            if !binary.is_empty() {
                batch_binary = Some(binary);
            }
            if let Some(rtc) = table.get("RTC_CENTER") {
                let [x, y, z] = array(rtc)?;
                center = Cartesian3::new(x, y, z);
            }
            let mut solids: [Vec<f32>; 4] = Default::default();
            let mut ids: [Vec<u16>; 4] = Default::default();
            let mut explicit = None;
            for (i, (name, id_name, stride)) in [
                ("BOXES", "BOX_BATCH_IDS", 19),
                ("CYLINDERS", "CYLINDER_BATCH_IDS", 18),
                ("ELLIPSOIDS", "ELLIPSOID_BATCH_IDS", 19),
                ("SPHERES", "SPHERE_BATCH_IDS", 4),
            ]
            .into_iter()
            .enumerate()
            {
                let n = match table.get(format!("{name}_LENGTH")) {
                    None => 0,
                    Some(v) => v
                        .as_u64()
                        .and_then(|v| usize::try_from(v).ok())
                        .ok_or_else(|| RuntimeError::new("Invalid geometry count"))?,
                };
                if n == 0 {
                    continue;
                }
                let end = count
                    .checked_add(n)
                    .filter(|v| *v <= 65536)
                    .ok_or_else(|| RuntimeError::new("Geometry tile exceeds 16-bit batch ID range"))?;
                let manual = table.get(id_name).filter(|v| !v.is_null());
                if explicit.replace(manual.is_some()).is_some_and(|old| old != manual.is_some()) {
                    return Err(RuntimeError::new("If one group defines batch IDs, all groups must define them"));
                }
                solids[i] =
                    components::<4>(body, &table[name], n * stride)?.iter().copied().map(f32::from_le_bytes).collect();
                ids[i] = match manual {
                    Some(d) => components::<2>(body, d, n)?.iter().copied().map(u16::from_le_bytes).collect(),
                    None => (count..end).map(|id| id as u16).collect(),
                };
                count = end;
            }
            if ids.iter().flatten().any(|id| *id as usize >= count) {
                return Err(RuntimeError::new("Geometry batch ID out of range"));
            }
            if count > 0 {
                let mut packed = vec![center.x, center.y, center.z];
                packed.extend(Matrix4::IDENTITY.0);
                mesh = Some(create_vector_tile_geometries(VectorTileGeometriesParameters {
                    boxes: Some(&solids[0]),
                    box_batch_ids: Some(&ids[0]),
                    cylinders: Some(&solids[1]),
                    cylinder_batch_ids: Some(&ids[1]),
                    ellipsoids: Some(&solids[2]),
                    ellipsoid_batch_ids: Some(&ids[2]),
                    spheres: Some(&solids[3]),
                    sphere_batch_ids: Some(&ids[3]),
                    batch_table_colors: &vec![u32::MAX; count],
                    packed_buffer: &packed,
                })?);
            }
        }
        let batch_table = Cesium3DTileBatchTable::new(count, batch_json.as_ref(), batch_binary, max_texture_size)?;
        Ok(Self {
            tile,
            resource,
            center,
            batch_table,
            mesh,
            model_matrix: Matrix4::IDENTITY,
            classification: ClassificationType::Both,
            highlight: Color::WHITE,
            gpu: None,
            ready: data.is_empty(),
        })
    }
    pub fn mesh(&self) -> Option<&VectorTileGeometries> {
        self.mesh.as_ref()
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn get_feature(&self, id: usize) -> Result<Rc<Cesium3DTileFeature<G>>> {
        self.batch_table.get_feature(id)
    }
    fn prepare(&mut self, context: &Context<G>) -> Result<()> {
        if self.batch_table.is_destroyed() {
            return Err(RuntimeError::new("Geometry content was destroyed"));
        }
        if let Some(mesh) = self.mesh.as_ref().filter(|_| self.gpu.is_none()) {
            let geometry = Geometry::new(
                GeometryAttributes {
                    position: Some(GeometryAttribute::new(
                        ComponentDatatype::Float,
                        3,
                        TypedArray::Float32(mesh.positions.clone()),
                    )),
                    ..Default::default()
                },
                Some(mesh.indices.clone()),
                PrimitiveType::Triangles,
            );
            let attributes = std::collections::BTreeMap::from([("position".into(), 0)]);
            let vertices = VertexArray::from_geometry(
                context,
                &geometry,
                GeometryUploadOptions {
                    attribute_locations: attributes.clone().into_iter().collect(),
                    ..Default::default()
                },
            )?;
            let shader = |s: &str| ShaderSource { sources: vec![s.into()], ..Default::default() };
            let program = ShaderProgram::from_sources(
                context,
                &ShaderProgramOptions {
                    vertex_shader_source: shader(
                        "in vec3 position; void main(){gl_Position=czm_modelViewProjection*vec4(position,1.0);}",
                    ),
                    fragment_shader_source: shader("uniform vec4 u_color; void main(){out_FragColor=u_color;}"),
                    attribute_locations: attributes,
                },
            )?;
            self.gpu = Some(GeometryResources { vertices: Rc::new(vertices), program: Rc::new(program) });
        }
        self.ready = true;
        Ok(())
    }
}
impl<G: Gl + 'static> TilesetContent for Geometry3DTileContent<G> {
    fn get_feature(&self, id: usize) -> Result<Option<Rc<dyn TileFeature>>> {
        Ok(Some(self.batch_table.get_feature(id)?))
    }
    fn set_feature_metadata(&mut self, metadata: &[FeatureMetadata]) {
        *self.batch_table.metadata.borrow_mut() = metadata.first().cloned().unwrap_or_default();
    }
    fn configure_model(&mut self, state: &TileContentModelState) {
        self.model_matrix = state.model_matrix;
        self.classification = state.classification_type.unwrap_or(ClassificationType::Both);
    }
    fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>) -> Result<()> {
        self.batch_table.apply_style(style)
    }
    fn feature_properties_dirty(&self) -> bool {
        self.batch_table.feature_properties_dirty.get()
    }
    fn set_feature_properties_dirty(&mut self, dirty: bool) {
        self.batch_table.feature_properties_dirty.set(dirty);
    }
    fn apply_debug_settings(&mut self, enabled: bool, color: Color) -> Result<()> {
        self.highlight = if enabled { color } else { Color::WHITE };
        Ok(())
    }
    fn metrics(&self) -> TileContentMetrics {
        let mut m = TileContentMetrics {
            features: self.batch_table.features_length() as u64,
            batch_table_bytes: self.batch_table.byte_length() as u64,
            ..Default::default()
        };
        if let Some(mesh) = &self.mesh {
            m.triangles = (mesh.indices.len() / 3) as u64;
            m.geometry_bytes = (mesh.positions.len() * 4
                + mesh.vertex_batch_ids.len() * 2
                + mesh.indices.len() * mesh.indices.component_datatype().get_size_in_bytes())
                as u64;
        }
        m
    }
    fn process(&mut self, frame: &FrameState) -> Result<bool> {
        if self.batch_table.is_destroyed() {
            return Err(RuntimeError::new("Geometry content was destroyed"));
        }
        if self.mesh.is_none() {
            self.ready = true;
        } else if let Some(context) = frame.context.as_ref().and_then(|c| c.get::<Context<G>>()) {
            self.prepare(context)?;
        }
        Ok(self.ready)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if !self.process(frame)? {
            return Ok(());
        }
        let Some(gpu) = &self.gpu else { return Ok(()) };
        let context = frame
            .context
            .as_ref()
            .and_then(|c| c.get::<Context<G>>())
            .ok_or_else(|| RuntimeError::new("Geometry content requires its renderer context"))?;
        let pick = frame.passes.pick;
        self.batch_table.update(context, pick || frame.passes.post_process)?;
        if !frame.passes.render && !pick {
            return Ok(());
        }
        let mesh = self.mesh.as_ref().expect("uploaded mesh");
        let model = self.model_matrix.multiply(&Matrix4::from_translation(self.center));
        for (i, &id) in mesh.batch_ids.iter().enumerate() {
            let id = id as usize;
            if !self.batch_table.get_show(id)? {
                continue;
            }
            let c = self.batch_table.get_color(id)?;
            if c.alpha == 0. {
                continue;
            }
            let h = self.highlight;
            let color = if pick {
                self.get_feature(id)?.pick_color()?.ok_or_else(|| RuntimeError::new("Missing geometry pick color"))?
            } else {
                let alpha = c.alpha * h.alpha;
                Color::new(c.red * h.red * alpha, c.green * h.green * alpha, c.blue * h.blue * alpha, alpha)
            };
            for (class, pass) in [
                (ClassificationType::Terrain, Pass::TerrainClassification),
                (ClassificationType::Cesium3DTile, Pass::Cesium3DTileClassification),
            ] {
                if self.classification != ClassificationType::Both && self.classification != class {
                    continue;
                }
                for stencil in [true, false] {
                    let mut options = SceneDrawOptions::new(gpu.vertices.clone(), gpu.program.clone());
                    use model::classification_model_draw_command::{
                        classification_color_state, classification_depth_state,
                    };
                    options.state =
                        if stencil { classification_depth_state(pass) } else { classification_color_state(pick) };
                    options.offset = mesh.index_offsets[i] as usize;
                    options.count = Some(mesh.index_counts[i] as usize);
                    options.uniforms = vec![(
                        "u_color".into(),
                        UniformValue::Float(vec![
                            color.red as f32,
                            color.green as f32,
                            color.blue as f32,
                            color.alpha as f32,
                        ]),
                    )];
                    frame.command_list.push(SceneCommand::from_draw(
                        "geometry-tile",
                        context.clone(),
                        pass,
                        DrawMetadata {
                            model_matrix: Some(model),
                            cull: false,
                            pick_id: pick.then(|| "u_color".into()),
                            ..Default::default()
                        },
                        options,
                    ));
                }
            }
        }
        Ok(())
    }
    fn pick(&self, _: &Ray, _: &FrameState) -> Result<Option<Cartesian3>> {
        Ok(None)
    }
    fn destroy(&mut self) -> Result<()> {
        if let Some(gpu) = self.gpu.take() {
            gpu.vertices.destroy()?;
        }
        self.mesh = None;
        self.ready = false;
        self.batch_table.destroy()
    }
}
