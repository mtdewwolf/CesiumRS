//! ArcGIS tiled elevation, LERC tiles and dynamic tilemap availability.
use super::heightmap_terrain_data::{HeightmapTerrainDataOptions, HeightmapTerrainStructure};
use super::heightmap_tessellator::HeightmapStructure;
use super::network_terrain::{fetch_bytes, fetch_json, TerrainFuture};
use super::resource::DerivedResourceOptions;
use super::*;
use futures_util::{
    future::{LocalBoxFuture, Shared},
    FutureExt,
};
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::HashMap,
    future::{Future, Ready},
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context, Waker},
};
#[derive(Default)]
pub struct ArcGisTerrainOptions {
    pub ellipsoid: Option<Ellipsoid>,
    pub token: Option<String>,
}
type AvailabilityRequest = Shared<LocalBoxFuture<'static, Result<Option<bool>, RuntimeError>>>;
pub struct ArcGISTiledElevationTerrainProvider {
    pub resource: Resource,
    pub width: usize,
    pub height: usize,
    pub encoding: HeightmapEncoding,
    pub lod_count: u32,
    pub structure: HeightmapTerrainStructure,
    scheme: Rc<dyn TilingScheme>,
    credit: Option<Credit>,
    client: ResourceClient,
    available: Option<Rc<TileAvailability>>,
    loaded: Option<Rc<TileAvailability>>,
    pending: Rc<RefCell<HashMap<String, AvailabilityRequest>>>,
    error: Arc<Mutex<Event<RuntimeError>>>,
    level_zero_error: f64,
}
impl ArcGISTiledElevationTerrainProvider {
    pub async fn from_url(
        resource: Resource,
        options: ArcGisTerrainOptions,
        client: ResourceClient,
    ) -> Result<Self, RuntimeError> {
        resource.append_forward_slash();
        if let Some(token) = &options.token {
            resource.set_query_parameters(
                &vec![("token".into(), Some(super::query::QueryValue::Single(token.clone())))],
                false,
            );
        }
        let metadata = resource
            .get_derived_resource(DerivedResourceOptions {
                query_parameters: vec![("f".into(), Some(super::query::QueryValue::Single("pjson".into())))],
                ..Default::default()
            })
            .unwrap();
        let data =
            fetch_json(&client, &metadata).ok_or_else(|| RuntimeError::new("Metadata request was throttled."))?.await?;
        Self::from_metadata(resource, &data, options, client)
    }
    pub fn from_metadata(
        resource: Resource,
        m: &Value,
        options: ArcGisTerrainOptions,
        client: ResourceClient,
    ) -> Result<Self, RuntimeError> {
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let wkid = m["spatialReference"]["latestWkid"].as_i64().or_else(|| m["spatialReference"]["wkid"].as_i64());
        let extent = &m["extent"];
        let n = |k: &str| extent[k].as_f64().unwrap_or(f64::NAN);
        let scheme: Rc<dyn TilingScheme> = match wkid {
            Some(4326) => Rc::new(GeographicTilingScheme::new(
                ellipsoid,
                Rectangle::from_degrees(n("xmin"), n("ymin"), n("xmax"), n("ymax")),
                2,
                1,
            )),
            Some(3857) => {
                let max = std::f64::consts::PI * ellipsoid.maximum_radius;
                Rc::new(WebMercatorTilingScheme::new(
                    ellipsoid,
                    1,
                    1,
                    Some((
                        Cartesian2::new(n("xmin").max(-max), n("ymin").max(-max)),
                        Cartesian2::new(n("xmax").min(max), n("ymax").min(max)),
                    )),
                ))
            }
            _ => return Err(RuntimeError::new("Invalid spatial reference")),
        };
        let info = &m["tileInfo"];
        if info.is_null() {
            return Err(RuntimeError::new("tileInfo is required"));
        }
        let width = info["rows"].as_u64().ok_or_else(|| RuntimeError::new("Invalid tileInfo rows."))? as usize + 1;
        let height = info["cols"].as_u64().ok_or_else(|| RuntimeError::new("Invalid tileInfo cols."))? as usize + 1;
        let encoding = if info["format"] == "LERC" { HeightmapEncoding::Lerc } else { HeightmapEncoding::None };
        let lod_count = info["lods"].as_array().map_or(0, |a| a.len().saturating_sub(1) as u32);
        let has_availability = m["capabilities"].as_str().is_some_and(|s| s.contains("Tilemap"));
        let (available, loaded) = if has_availability {
            let a = Rc::new(TileAvailability::new(scheme.clone(), lod_count));
            a.add_available_tile_range(
                0,
                0,
                0,
                scheme.get_number_of_x_tiles_at_level(0) as u32,
                scheme.get_number_of_y_tiles_at_level(0) as u32,
            );
            (Some(a), Some(Rc::new(TileAvailability::new(scheme.clone(), lod_count))))
        } else {
            (None, None)
        };
        let structure = HeightmapTerrainStructure {
            samples: HeightmapStructure { element_multiplier: 1., ..Default::default() },
            lowest_encoded_height: m["minValues"][0].as_f64().filter(|_| m["maxValues"].is_array()),
            highest_encoded_height: m["maxValues"][0].as_f64().filter(|_| m["minValues"].is_array()),
        };
        let level_zero_error = super::terrain_provider::get_estimated_level_zero_geometric_error_for_a_heightmap(
            &ellipsoid,
            width as f64,
            scheme.get_number_of_x_tiles_at_level(0) as f64,
            None,
        );
        Ok(Self {
            resource,
            width,
            height,
            encoding,
            lod_count,
            structure,
            scheme,
            credit: m["copyrightText"].as_str().map(|s| Credit::new(s, false)),
            client,
            available,
            loaded,
            pending: Default::default(),
            error: Arc::new(Mutex::new(Event::new())),
            level_zero_error,
        })
    }
    fn known_available(&self, x: u32, y: u32, level: u32) -> Option<bool> {
        let a = self.available.as_ref()?;
        if level > self.lod_count {
            return Some(false);
        }
        if a.is_tile_available(level, x, y) {
            return Some(true);
        }
        if self.loaded.as_ref()?.is_tile_available(level, x, y) {
            return Some(false);
        }
        None
    }
    pub fn request_availability(&self, x: u32, y: u32, level: u32) -> Option<AvailabilityRequest> {
        let available = self.available.clone()?;
        let loaded = self.loaded.clone()?;
        let x_offset = x / 128 * 128;
        let y_offset = y / 128 * 128;
        let dim = 1u32.wrapping_shl(level).min(128);
        let path = format!("tilemap/{level}/{y_offset}/{x_offset}/{dim}/{dim}");
        if let Some(f) = self.pending.borrow().get(&path) {
            return Some(f.clone());
        }
        let request = Request::<HttpResponse>::new(RequestOptions {
            throttle_by_server: true,
            request_type: RequestType::Terrain,
            ..Default::default()
        })
        .into_handle();
        let r = self.resource.get_derived_resource(DerivedResourceOptions {
            url: Some(path.clone()),
            request: Some(request),
            ..Default::default()
        })?;
        let fetch = fetch_json(&self.client, &r)?;
        let weak = Rc::downgrade(&self.pending);
        let cache_key = path.clone();
        let future = async move {
            let result = fetch.await;
            if let Some(cache) = weak.upgrade() {
                cache.borrow_mut().remove(&cache_key);
            }
            let result = result?;
            let data = result["data"].as_array().ok_or_else(|| RuntimeError::new("Invalid ArcGIS tilemap."))?;
            let size = dim as usize;
            if size == 0 || data.len() < size * size {
                return Err(RuntimeError::new("ArcGIS tilemap is truncated."));
            }
            loaded.add_available_tile_range(level, x_offset, y_offset, x_offset + dim, y_offset + dim);
            for row in 0..size {
                let mut col = 0;
                while col < size {
                    if data[row * size + col] != 1 {
                        col += 1;
                        continue;
                    }
                    let start = col;
                    while col < size && data[row * size + col] == 1 {
                        col += 1;
                    }
                    available.add_available_tile_range(
                        level,
                        x_offset + start as u32,
                        y_offset + row as u32,
                        x_offset + col as u32 - 1,
                        y_offset + row as u32,
                    );
                }
            }
            Ok(Some(available.is_tile_available(level, x, y)))
        }
        .boxed_local()
        .shared();
        self.pending.borrow_mut().insert(path, future.clone());
        Some(future)
    }
    fn poll_availability(&self) {
        let jobs = self.pending.borrow().values().cloned().collect::<Vec<_>>();
        let mut cx = Context::from_waker(Waker::noop());
        for mut f in jobs {
            let _ = Pin::new(&mut f).poll(&mut cx);
        }
    }
    pub fn tile_resource(
        &self,
        x: u32,
        y: u32,
        level: u32,
        request: Option<RequestHandle<HttpResponse>>,
    ) -> Option<Resource> {
        self.resource.get_derived_resource(DerivedResourceOptions {
            url: Some(format!("tile/{level}/{y}/{x}")),
            request,
            ..Default::default()
        })
    }
}
impl TerrainProvider for ArcGISTiledElevationTerrainProvider {
    type Error = RuntimeError;
    type Credit = Credit;
    type TerrainData = NetworkTerrainData;
    type Request = RequestHandle<HttpResponse>;
    type GeometryFuture<'a> = TerrainFuture<'a, NetworkTerrainData>;
    type AvailabilityFuture<'a> = Ready<Result<(), RuntimeError>>;
    fn error_event(&self) -> Arc<Mutex<Event<RuntimeError>>> {
        self.error.clone()
    }
    fn credit(&self) -> Option<&Credit> {
        self.credit.as_ref()
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.scheme.as_ref()
    }
    fn has_water_mask(&self) -> bool {
        false
    }
    fn has_vertex_normals(&self) -> bool {
        false
    }
    fn availability(&self) -> Option<&TileAvailability> {
        self.available.as_deref()
    }
    fn request_tile_geometry(
        &self,
        x: u32,
        y: u32,
        level: u32,
        request: Option<Self::Request>,
    ) -> Option<Self::GeometryFuture<'_>> {
        let a = if self.available.is_some() && self.known_available(x * 2, y * 2, level + 1).is_none() {
            Some(self.request_availability(x * 2, y * 2, level + 1)?)
        } else {
            None
        };
        let r = self.tile_resource(x, y, level, request)?;
        let f = fetch_bytes(&self.client, &r)?;
        Some(Box::pin(async move {
            let bytes = f.await?;
            if let Some(a) = a {
                a.await?;
            }
            let mut o = HeightmapTerrainDataOptions::new(TypedArray::Uint8(bytes), self.width, self.height);
            o.encoding = self.encoding;
            o.structure = self.structure;
            o.child_tile_mask = self.available.as_ref().map_or(15, |a| a.compute_child_mask_for_tile(level, x, y));
            HeightmapTerrainData::new(o)
                .map(NetworkTerrainData::heightmap)
                .ok_or_else(|| RuntimeError::new("Invalid ArcGIS heightmap."))
        }))
    }
    fn get_level_maximum_geometric_error(&self, l: u32) -> f64 {
        self.level_zero_error / 1i32.wrapping_shl(l) as f64
    }
    fn get_tile_data_available(&self, x: u32, y: u32, l: u32) -> Option<bool> {
        self.poll_availability();
        let known = self.known_available(x, y, l);
        if known.is_none() {
            let _ = self.request_availability(x, y, l);
            self.poll_availability();
        }
        known
    }
    fn load_tile_data_availability(&self, _: u32, _: u32, _: u32) -> Option<Self::AvailabilityFuture<'_>> {
        None
    }
}
