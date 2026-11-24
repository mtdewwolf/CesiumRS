//! CesiumJS `Composite3DTileContent`: a legacy `cmpt` envelope owns child
//! contents and gives each a distinct `compositeIndex` cache URL.
use super::{
    cesium_3d_tile_content::{read_u32, tile_payload},
    cesium_3d_tileset::TileContentModelState,
    scene::Result,
    *,
};
use crate::core::{query::QueryValue, resource::DerivedResourceOptions, *};

pub struct Composite3DTileContent {
    pub resource: Resource,
    children: Multiple3DTileContent,
    metadata: Option<std::rc::Rc<dyn MetadataEntity>>,
    group: Option<std::rc::Rc<dyn MetadataEntity>>,
}
impl Composite3DTileContent {
    pub fn new(resource: Resource, contents: Vec<Box<dyn TilesetContent>>) -> Self {
        Self {
            resource,
            children: Multiple3DTileContent::new(contents),
            metadata: None,
            group: None,
        }
    }
    /// Splits the composite payload into `(resource, child bytes)` pairs.
    /// Nested composites get `compositeIndex` suffixes joined with `_`.
    pub fn unpack<'a>(
        resource: &Resource,
        bytes: &'a [u8],
        offset: usize,
    ) -> Result<Vec<(Resource, &'a [u8])>> {
        let slice = bytes.get(offset..).filter(|b| b.len() >= 16).ok_or_else(|| {
            RuntimeError::new("Invalid Composite Tile payload")
        })?;
        if &slice[..4] != b"cmpt" {
            return Err(RuntimeError::new("Invalid Composite Tile magic"));
        }
        let version = read_u32(slice, 4)?;
        if version != 1 {
            return Err(RuntimeError::new(format!(
                "Only Composite Tile version 1 is supported. Version {version} is not."
            )));
        }
        let bytes = tile_payload(bytes, offset, b"cmpt")?;
        let count = read_u32(bytes, 12)?;
        let mut rest = &bytes[16..];
        let prefix = resource
            .options()
            .query_parameters
            .iter()
            .find(|(k, _)| k == "compositeIndex")
            .and_then(|(_, v)| v.as_ref())
            .map(|v| match v {
                QueryValue::Single(s) => s.clone(),
                QueryValue::Multiple(a) => a.join(","),
            });
        let mut children = Vec::new();
        for index in 0..count {
            let length = read_u32(rest, 8)?;
            let (child, tail) = rest
                .split_at_checked(length)
                .filter(|(c, _)| c.len() >= 12)
                .ok_or_else(|| RuntimeError::new("Invalid composite child length"))?;
            let magic = std::str::from_utf8(&child[..4]).unwrap_or("");
            if Cesium3DTileContentFactory::dispatch(magic, &[]).is_none() {
                return Err(RuntimeError::new(format!("Unknown tile content type, {magic}, inside Composite tile")));
            }
            let index = prefix.as_ref().map_or_else(|| index.to_string(), |p| format!("{p}_{index}"));
            let child_resource = resource
                .get_derived_resource(DerivedResourceOptions {
                    query_parameters: vec![("compositeIndex".into(), Some(QueryValue::Single(index)))],
                    ..Default::default()
                })
                .ok_or_else(|| RuntimeError::new("Invalid composite child resource"))?;
            children.push((child_resource, child));
            rest = tail;
        }
        Ok(children)
    }
    /// Parses the child offsets and builds each content with the caller's
    /// factory. Child futures run concurrently and all are destroyed if any
    /// child creation fails.
    pub async fn from_tile_type(
        resource: Resource,
        bytes: &[u8],
        offset: usize,
        mut create: impl FnMut(Resource, &[u8]) -> TileContentFuture,
    ) -> Result<Self> {
        let futures = Self::unpack(&resource, bytes, offset)?.into_iter().map(|(r, b)| create(r, b));
        let mut contents = Vec::new();
        let mut failure = None;
        for result in futures_util::future::join_all(futures).await {
            match result {
                Ok(c) => contents.push(c),
                Err(e) => {
                    failure.get_or_insert(e);
                }
            }
        }
        if let Some(e) = failure {
            for c in &mut contents {
                let _ = c.destroy();
            }
            return Err(e);
        }
        Ok(Self::new(resource, contents))
    }
    pub fn url(&self) -> String {
        self.resource.get_url_component(true, false)
    }
    pub fn ready(&self) -> bool {
        self.children.ready()
    }
    pub fn inner_contents(&self) -> &[Box<dyn TilesetContent>] {
        self.children.contents()
    }
    pub fn is_destroyed(&self) -> bool {
        self.children.is_destroyed()
    }
}
impl TilesetContent for Composite3DTileContent {
    fn set_feature_metadata(&mut self, metadata: &[FeatureMetadata]) {
        self.metadata = metadata.first().and_then(|m| m.content.clone());
        self.group = metadata.first().and_then(|m| m.group.clone());
        self.children.broadcast_feature_metadata(metadata);
    }
    fn inner_contents(&self) -> Option<&[Box<dyn TilesetContent>]> {
        Some(self.children.contents())
    }
    fn feature_properties_dirty(&self) -> bool {
        self.children.feature_properties_dirty()
    }
    fn set_feature_properties_dirty(&mut self, dirty: bool) {
        self.children.set_feature_properties_dirty(dirty);
    }
    fn configure_model(&mut self, state: &TileContentModelState) {
        self.children.configure_model(state);
    }
    fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>) -> Result<()> {
        self.children.apply_style(style)
    }
    fn apply_debug_settings(&mut self, enabled: bool, color: Color) -> Result<()> {
        self.children.apply_debug_settings(enabled, color)
    }
    fn process(&mut self, frame: &FrameState) -> Result<bool> {
        self.children.process(frame)
    }
    fn process_with_frame(&mut self, frame: &mut FrameState) -> Result<bool> {
        self.children.process_with_frame(frame)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.children.update(frame)
    }
    fn is_renderable(&self) -> bool {
        self.children.is_renderable()
    }
    fn metrics(&self) -> TileContentMetrics {
        self.children.metrics()
    }
    fn texture_allocations(&self) -> Vec<(Option<String>, u64)> {
        self.children.texture_allocations()
    }
    fn pick(&self, ray: &Ray, frame: &FrameState) -> Result<Option<Cartesian3>> {
        self.children.pick(ray, frame)
    }
    fn take_expansions(&mut self) -> Vec<TileExpansion> {
        self.children.take_expansions()
    }
    fn take_failures(&mut self) -> Vec<TileContentFailure> {
        self.children.take_failures()
    }
    fn destroy(&mut self) -> Result<()> {
        self.children.destroy()
    }
}
