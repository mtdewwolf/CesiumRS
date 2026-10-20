//! Shared native types and bounded binary reading for network terrain formats.
use super::terrain_data::{TerrainMeshRequest, TerrainTile};
use super::*;
use std::{future::Future, pin::Pin, rc::Rc, sync::Arc};
pub type TerrainFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + 'a>>;
#[derive(Clone, Debug)]
pub enum NetworkTerrainData {
    Cesium3DTiles(Cesium3DTilesTerrainData),
    GoogleEarthEnterprise(GoogleEarthEnterpriseTerrainData),
    Heightmap { data: HeightmapTerrainData, credits: Option<Vec<Credit>> },
    Quantized(QuantizedMeshTerrainData<Credit>),
}
impl NetworkTerrainData {
    pub fn heightmap(data: HeightmapTerrainData) -> Self {
        Self::Heightmap { data, credits: None }
    }
}
impl TerrainData for NetworkTerrainData {
    type Credit = Credit;
    type WaterMask = [u8];
    type MeshFuture<'a> = TerrainFuture<'a, Arc<TerrainMesh>>;
    type UpsampleFuture<'a> = TerrainFuture<'a, Self>;
    fn credits(&self) -> Option<&[Credit]> {
        match self {
            Self::Cesium3DTiles(d) => d.credits(),
            Self::GoogleEarthEnterprise(d) => d.credits(),
            Self::Heightmap { credits, .. } => credits.as_deref(),
            Self::Quantized(d) => d.credits(),
        }
    }
    fn water_mask(&self) -> Option<&[u8]> {
        match self {
            Self::Cesium3DTiles(d) => d.water_mask(),
            Self::GoogleEarthEnterprise(d) => d.water_mask(),
            Self::Heightmap { data, .. } => data.water_mask(),
            Self::Quantized(d) => d.water_mask(),
        }
    }
    fn interpolate_height(&self, r: &Rectangle, lon: f64, lat: f64) -> Option<f64> {
        match self {
            Self::Cesium3DTiles(d) => d.interpolate_height(r, lon, lat),
            Self::GoogleEarthEnterprise(d) => d.interpolate_height(r, lon, lat),
            Self::Heightmap { data, .. } => data.interpolate_height(r, lon, lat),
            Self::Quantized(d) => d.interpolate_height(r, lon, lat),
        }
    }
    fn is_child_available(&self, x: u32, y: u32, cx: u32, cy: u32) -> bool {
        match self {
            Self::Cesium3DTiles(d) => d.is_child_available(x, y, cx, cy),
            Self::GoogleEarthEnterprise(d) => d.is_child_available(x, y, cx, cy),
            Self::Heightmap { data, .. } => data.is_child_available(x, y, cx, cy),
            Self::Quantized(d) => d.is_child_available(x, y, cx, cy),
        }
    }
    fn create_mesh<'a>(&'a mut self, r: TerrainMeshRequest<'a>) -> Option<Self::MeshFuture<'a>> {
        match self {
            Self::Cesium3DTiles(d) => Some(Box::pin(d.create_mesh(r)?)),
            Self::GoogleEarthEnterprise(d) => Some(Box::pin(d.create_mesh(r)?)),
            Self::Heightmap { data, .. } => Some(Box::pin(data.create_mesh(r)?)),
            Self::Quantized(d) => Some(Box::pin(d.create_mesh(r)?)),
        }
    }
    fn upsample<'a>(
        &'a self,
        s: &'a dyn TilingScheme,
        a: TerrainTile,
        b: TerrainTile,
    ) -> Option<Self::UpsampleFuture<'a>> {
        match self {
            Self::Cesium3DTiles(d) => {
                let f = d.upsample(s, a, b)?;
                Some(Box::pin(async move { f.await.map(Self::Cesium3DTiles) }))
            }
            Self::GoogleEarthEnterprise(d) => {
                let c = d.upsample_quantized_sync(s, a, b)?;
                Some(Box::pin(async { Ok(Self::Quantized(c)) }))
            }
            Self::Heightmap { data, credits } => {
                let f = data.upsample(s, a, b)?;
                Some(Box::pin(async move { Ok(Self::Heightmap { data: f.await?, credits: credits.clone() }) }))
            }
            Self::Quantized(d) => {
                let f = d.upsample(s, a, b)?;
                Some(Box::pin(async move { f.await.map(Self::Quantized) }))
            }
        }
    }
    fn was_created_by_upsampling(&self) -> bool {
        match self {
            Self::Cesium3DTiles(d) => d.was_created_by_upsampling(),
            Self::GoogleEarthEnterprise(d) => d.was_created_by_upsampling(),
            Self::Heightmap { data, .. } => data.was_created_by_upsampling(),
            Self::Quantized(d) => d.was_created_by_upsampling(),
        }
    }
}
impl<T: TilingScheme + ?Sized> TilingScheme for Rc<T> {
    fn ellipsoid(&self) -> &Ellipsoid {
        (**self).ellipsoid()
    }
    fn rectangle(&self) -> &Rectangle {
        (**self).rectangle()
    }
    fn projection(&self) -> &dyn MapProjection {
        (**self).projection()
    }
    fn get_number_of_x_tiles_at_level(&self, l: u32) -> i32 {
        (**self).get_number_of_x_tiles_at_level(l)
    }
    fn get_number_of_y_tiles_at_level(&self, l: u32) -> i32 {
        (**self).get_number_of_y_tiles_at_level(l)
    }
    fn rectangle_to_native_rectangle(&self, r: &Rectangle) -> Rectangle {
        (**self).rectangle_to_native_rectangle(r)
    }
    fn tile_xy_to_native_rectangle(&self, x: f64, y: f64, l: u32) -> Rectangle {
        (**self).tile_xy_to_native_rectangle(x, y, l)
    }
    fn tile_xy_to_rectangle(&self, x: f64, y: f64, l: u32) -> Rectangle {
        (**self).tile_xy_to_rectangle(x, y, l)
    }
    fn position_to_tile_xy(&self, p: &Cartographic, l: u32) -> Option<Cartesian2> {
        (**self).position_to_tile_xy(p, l)
    }
}
pub(crate) struct Reader<'a> {
    pub bytes: &'a [u8],
    pub pos: usize,
}
impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], RuntimeError> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&v| v <= self.bytes.len())
            .ok_or_else(|| RuntimeError::new("Terrain packet is truncated."))?;
        let s = &self.bytes[self.pos..end];
        self.pos = end;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8, RuntimeError> {
        Ok(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, RuntimeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32, RuntimeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn f32(&mut self) -> Result<f64, RuntimeError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()) as f64)
    }
    pub fn f64(&mut self) -> Result<f64, RuntimeError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn c3(&mut self) -> Result<Cartesian3, RuntimeError> {
        Ok(Cartesian3::new(self.f64()?, self.f64()?, self.f64()?))
    }
    pub fn indices(&mut self, count: usize, wide: bool) -> Result<Vec<u32>, RuntimeError> {
        if count > self.bytes.len() / if wide { 4 } else { 2 } {
            return Err(RuntimeError::new("Terrain index count is out of bounds."));
        }
        (0..count).map(|_| if wide { self.u32() } else { self.u16().map(u32::from) }).collect()
    }
}
pub(crate) fn net_error(e: RequestErrorEvent) -> RuntimeError {
    RuntimeError::new(e.to_string())
}
pub(crate) fn fetch_json(client: &ResourceClient, r: &Resource) -> Option<TerrainFuture<'static, serde_json::Value>> {
    let f = client.fetch_json(r)?;
    Some(Box::pin(async move { f.await.map_err(net_error) }))
}
pub(crate) fn fetch_bytes(client: &ResourceClient, r: &Resource) -> Option<TerrainFuture<'static, Vec<u8>>> {
    let f = client.fetch_array_buffer(r)?;
    Some(Box::pin(async move { f.await.map_err(net_error) }))
}
