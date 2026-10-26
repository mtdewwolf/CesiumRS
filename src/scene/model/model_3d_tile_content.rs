//! Renderable glTF, b3dm, i3dm, pnts and GeoJSON tiles backed by Model.
use super::{Model, ModelAnimationOptions, ModelFeature, ModelFeatureStyle, ModelOptions};
use crate::{
    core::{Cartesian3, Color, Ray, Resource, RuntimeError},
    renderer::{Context, Gl, Pass},
    scene::{
        cesium_3d_tileset::TileContentModelState, gltf_pipeline::BufferSource, Cesium3DTileContentType, FrameResource,
        FrameState, GltfJsonInput, ModelAnimationLoop, ModelImageHost, RenderableTileFactory, ResourceCache,
        TileContentFuture, TileContentMetrics, TileContentRequest, TileId, TilesetContent,
    },
};
use std::{rc::Rc, sync::Arc};
type Result<T> = std::result::Result<T, RuntimeError>;

pub struct Model3DTileContent<G: Gl + 'static> {
    pub model: Model<G>,
    pub tile: TileId,
    pub resource: Resource,
    pub metadata: Option<FrameResource>,
    pub group: Option<FrameResource>,
    ready: bool,
}
impl<G: Gl + 'static> Model3DTileContent<G> {
    pub fn new(mut model: Model<G>, tile: TileId, resource: Resource) -> Self {
        model.options.cull = false;
        model.options.opaque_pass = Pass::Cesium3DTile;
        Self { model, tile, resource, metadata: None, group: None, ready: false }
    }
    pub fn ready(&self) -> bool {
        self.ready && !self.model.is_destroyed()
    }
    pub fn is_destroyed(&self) -> bool {
        self.model.is_destroyed()
    }
    pub fn features_length(&self) -> usize {
        self.model.feature_table_id.and_then(|i| self.model.feature_tables.get(i)).map_or(0, |t| t.features_length())
    }
    pub fn get_feature(&self, id: usize) -> Result<Rc<ModelFeature<G>>> {
        let selected = self.model.feature_table_id.ok_or_else(|| {
            RuntimeError::new(
                "No feature ID set is selected. Make sure featureIdLabel or instanceFeatureIdLabel is defined",
            )
        })?;
        self.model
            .feature_tables
            .get(selected)
            .ok_or_else(|| RuntimeError::new("No feature table found for the selected feature ID set"))?
            .get_feature(id)
    }
    pub fn has_property(&self, id: usize, name: &str) -> Result<bool> {
        match self.model.feature_table_id.and_then(|i| self.model.feature_tables.get(i)) {
            Some(table) => table.has_property(id, name),
            None => Ok(false),
        }
    }
    pub fn apply_debug_settings(&mut self, enabled: bool, color: Color) -> Result<()> {
        let color = if enabled { color } else { Color::WHITE };
        if self.features_length() == 0 {
            self.model.options.color = Some(color);
        } else if let Some(table) = self.model.feature_table_id.and_then(|i| self.model.feature_tables.get(i)) {
            table.set_all_color(color)?;
        }
        Ok(())
    }
    pub fn apply_style(&mut self, style: Option<&ModelFeatureStyle<'_, G>>) -> Result<()> {
        self.model.apply_style(style)
    }
    fn update_ready(&mut self) -> Result<()> {
        if !self.ready && self.model.ready() {
            if let Some(graph) = &self.model.scene_graph {
                self.model.active_animations.borrow_mut().add_all(
                    &graph.components().animations,
                    graph.runtime_nodes(),
                    &ModelAnimationOptions { loop_mode: Some(ModelAnimationLoop::Repeat), ..Default::default() },
                )?;
            }
            self.ready = true;
        }
        Ok(())
    }
}
impl<G: Gl + 'static> TilesetContent for Model3DTileContent<G> {
    fn configure_model(&mut self, state: &TileContentModelState) {
        self.model.options.model_matrix = state.model_matrix;
        self.model.options.classification_type = state.classification_type;
        self.model.options.feature_id_label = state.feature_id_label.clone();
        self.model.options.instance_feature_id_label = state.instance_feature_id_label.clone();
        self.model.options.enable_pick = state.enable_pick;
        self.model.options.show_credits_on_screen = state.show_credits_on_screen;
    }
    fn metrics(&self) -> TileContentMetrics {
        let s = &self.model.statistics;
        let batch: usize = self.model.feature_tables.iter().map(|t| t.batch_texture().byte_length()).sum();
        TileContentMetrics {
            features: self.features_length() as u64,
            points: s.points_length as u64,
            triangles: s.triangles_length as u64,
            geometry_bytes: s.geometry_byte_length as u64,
            texture_bytes: s.textures_byte_length().saturating_sub(batch) as u64,
            batch_table_bytes: (s.property_tables_byte_length + batch) as u64,
        }
    }
    fn texture_allocations(&self) -> Vec<(Option<String>, u64)> {
        self.model
            .statistics
            .get_texture_ids()
            .iter()
            .map(|id| (Some(id.clone()), self.model.statistics.get_texture_byte_length_by_id(id).unwrap_or(0) as u64))
            .collect()
    }
    fn process(&mut self, _: &FrameState) -> Result<bool> {
        self.update_ready()?;
        Ok(self.ready())
    }
    fn process_with_frame(&mut self, frame: &mut FrameState) -> Result<bool> {
        let ignore = self.model.options.ignore_commands;
        self.model.options.ignore_commands = true;
        let result = self.model.update(frame);
        self.model.options.ignore_commands = ignore;
        result?;
        self.update_ready()?;
        Ok(self.ready())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.model.update(frame)?;
        self.update_ready()
    }
    fn pick(&self, ray: &Ray, frame: &FrameState) -> Result<Option<Cartesian3>> {
        if !self.ready() {
            Ok(None)
        } else {
            self.model.pick(ray, frame)
        }
    }
    fn destroy(&mut self) -> Result<()> {
        self.ready = false;
        self.model.destroy()
    }
}

/// Install this on `TilesetResourceLoader::factory` to load the five built-in
/// model content formats. Structural/external tiles remain handled by the loader.
pub struct ModelTileFactory<G: Gl + 'static, H: ModelImageHost<G> + 'static>
where
    H::Image: 'static,
{
    pub context: Context<G>,
    pub cache: ResourceCache,
    pub host: Rc<H>,
    pub options: ModelOptions,
}
impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> RenderableTileFactory for ModelTileFactory<G, H>
where
    H::Image: 'static,
{
    fn create(&self, request: TileContentRequest, kind: Cesium3DTileContentType, bytes: Vec<u8>) -> TileContentFuture {
        let context = self.context.clone();
        let cache = self.cache.clone();
        let host = self.host.clone();
        let mut options = self.options.clone();
        options.opaque_pass = Pass::Cesium3DTile;
        options.cull = false;
        Box::pin(async move {
            if request.cancelled.get() {
                return Err(RuntimeError::new("tile request cancelled"));
            }
            let mut model = match kind {
                Cesium3DTileContentType::Gltf | Cesium3DTileContentType::GltfBinary => {
                    options.model_type = super::ModelType::TileGltf;
                    Model::from_gltf(
                        cache,
                        request.resource.clone(),
                        Some(request.base_resource),
                        Some(GltfJsonInput::Bytes(BufferSource::new(bytes))),
                        context,
                        host,
                        options,
                    )
                    .await?
                }
                Cesium3DTileContentType::Batched3DModel => {
                    Model::from_b3dm(
                        cache,
                        request.resource.clone(),
                        Some(request.base_resource),
                        Arc::new(bytes),
                        0,
                        context,
                        host,
                        options,
                    )
                    .await?
                }
                Cesium3DTileContentType::Instanced3DModel => {
                    Model::from_i3dm(
                        cache,
                        request.resource.clone(),
                        Some(request.base_resource),
                        Arc::new(bytes),
                        0,
                        context,
                        host,
                        options,
                    )
                    .await?
                }
                Cesium3DTileContentType::PointCloud => {
                    Model::from_pnts(request.resource.clone(), Arc::new(bytes), 0, context, options).await?
                }
                Cesium3DTileContentType::GeoJson => {
                    Model::from_geo_json(
                        request.resource.clone(),
                        serde_json::from_slice(&bytes).map_err(|e| RuntimeError::new(e.to_string()))?,
                        context,
                        options,
                    )
                    .await?
                }
                _ => return Err(RuntimeError::new(format!("Unsupported model tile content: {kind:?}"))),
            };
            if request.cancelled.get() {
                model.destroy()?;
                return Err(RuntimeError::new("tile request cancelled"));
            }
            Ok(Box::new(Model3DTileContent::new(model, request.tile, request.resource)) as Box<dyn TilesetContent>)
        })
    }
}
