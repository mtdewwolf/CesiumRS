//! Cesium heightmap/quantized-mesh terrain, parent layers and metadata availability.
use super::heightmap_terrain_data::{HeightmapTerrainDataOptions, HeightmapTerrainStructure};
use super::heightmap_tessellator::HeightmapStructure;
use super::network_terrain::{fetch_bytes, Reader, TerrainFuture};
use super::quantized_mesh_terrain_data::QuantizedMeshTerrainDataOptions;
use super::quantized_terrain_processor::QuantizedTerrainEdges;
use super::resource::DerivedResourceOptions;
use super::*;
use futures_util::{
    future::{LocalBoxFuture, Shared},
    FutureExt,
};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::HashMap,
    future::{poll_fn, Future},
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};
#[derive(Clone)]
pub struct CesiumTerrainOptions {
    pub request_vertex_normals: bool,
    pub request_water_mask: bool,
    pub request_metadata: bool,
    pub ellipsoid: Ellipsoid,
    pub credit: Option<Credit>,
}
impl Default for CesiumTerrainOptions {
    fn default() -> Self {
        Self {
            request_vertex_normals: false,
            request_water_mask: false,
            request_metadata: true,
            ellipsoid: Ellipsoid::WGS84,
            credit: None,
        }
    }
}
struct Layer {
    resource: Resource,
    version: String,
    templates: Vec<String>,
    available: Option<Rc<TileAvailability>>,
    loaded: Option<Rc<TileAvailability>>,
    availability_levels: Option<u32>,
    normals: bool,
    water: bool,
    metadata: bool,
    little_endian: bool,
    ion: bool,
}
type Availability = Shared<LocalBoxFuture<'static, Result<(), RuntimeError>>>;
struct State {
    client: ResourceClient,
    scheme: Rc<dyn TilingScheme>,
    scheme_name: String,
    layers: Vec<Layer>,
    options: CesiumTerrainOptions,
    heightmap: bool,
    water: bool,
    normals: bool,
    metadata: bool,
    available: Option<Rc<TileAvailability>>,
    tile_credits: Vec<Credit>,
    level_zero_error: f64,
    error: Arc<Mutex<Event<RuntimeError>>>,
    pending: Rc<RefCell<HashMap<String, Availability>>>,
}
#[derive(Clone)]
pub struct CesiumTerrainProvider {
    state: Rc<State>,
}
fn invalid(message: impl Into<String>) -> RuntimeError {
    RuntimeError::new(message)
}
fn ranges(value: &Value, start: u32, scheme: &dyn TilingScheme) -> Vec<(u32, [u32; 4])> {
    let mut out = vec![];
    if let Some(levels) = value.as_array() {
        for (i, level) in levels.iter().enumerate() {
            let l = start + i as u32;
            let n = scheme.get_number_of_y_tiles_at_level(l) as u32;
            for range in level.as_array().into_iter().flatten() {
                let v = |k| range[k].as_u64().unwrap_or(0) as u32;
                out.push((
                    l,
                    [
                        v("startX"),
                        n.wrapping_sub(v("endY")).wrapping_sub(1),
                        v("endX"),
                        n.wrapping_sub(v("startY")).wrapping_sub(1),
                    ],
                ));
            }
        }
    }
    out
}
fn add_ranges(a: &TileAvailability, rs: &[(u32, [u32; 4])]) {
    for (l, r) in rs {
        a.add_available_tile_range(*l, r[0], r[1], r[2], r[3]);
    }
}
impl CesiumTerrainProvider {
    pub async fn from_url(
        resource: Resource,
        options: CesiumTerrainOptions,
        client: ResourceClient,
    ) -> Result<Self, RuntimeError> {
        Self::load(resource, options, client, false).await
    }
    pub async fn from_ion_asset_id(
        id: u64,
        ion: &Ion,
        asset: IonAssetOptions,
        options: CesiumTerrainOptions,
        client: ResourceClient,
    ) -> Result<Self, RuntimeError> {
        let r = IonResource::from_asset_id(id, ion, asset, &client).await.map_err(super::network_terrain::net_error)?;
        let resource = r.as_resource().clone();
        resource.options_mut().credits = Some(r.credits());
        Self::load(resource, options, client, !r.is_external()).await
    }
    async fn load(
        mut resource: Resource,
        options: CesiumTerrainOptions,
        client: ResourceClient,
        ion: bool,
    ) -> Result<Self, RuntimeError> {
        resource.append_forward_slash();
        let mut layers = vec![];
        for _ in 0..128 {
            let r = resource
                .get_derived_resource(DerivedResourceOptions { url: Some("layer.json".into()), ..Default::default() })
                .unwrap();
            let metadata = match client
                .fetch_json(&r)
                .ok_or_else(|| invalid("Layer metadata request was throttled."))?
                .await
            {
                Ok(v) => v,
                Err(e) if e.status_code == Some(404) => {
                    json!({"format":"heightmap-1.0","version":"1.0.0","scheme":"tms","tiles":["{z}/{x}/{y}.terrain?v={version}"]})
                }
                Err(e) => return Err(invalid(format!("An error occurred while accessing {}.\n{e}", r.url()))),
            };
            let parent = metadata["parentUrl"]
                .as_str()
                .map(str::to_owned)
                .filter(|_| metadata["available"].is_array() || metadata["metadataAvailability"].is_number());
            layers.push((resource.clone(), metadata));
            if let Some(url) = parent {
                resource = resource
                    .get_derived_resource(DerivedResourceOptions { url: Some(url), ..Default::default() })
                    .ok_or_else(|| invalid("Invalid parent layer URL."))?;
                resource.append_forward_slash();
            } else {
                return Self::from_layers(layers, options, client, ion);
            }
        }
        Err(invalid("Terrain parent layer chain is cyclic or too deep."))
    }
    pub fn from_layers(
        input: Vec<(Resource, Value)>,
        mut options: CesiumTerrainOptions,
        client: ResourceClient,
        ion: bool,
    ) -> Result<Self, RuntimeError> {
        if input.is_empty() {
            return Err(invalid("The tile format is not specified in the layer.json file."));
        }
        let mut layers = vec![];
        let mut overall = vec![];
        let mut max_zoom = 0;
        let mut attribution = String::new();
        let mut scheme: Rc<dyn TilingScheme> = Rc::new(GeographicTilingScheme::default());
        let mut scheme_name = String::new();
        let (mut heightmap, mut water, mut normals, mut metadata) = (false, false, false, false);
        let mut last_credits = vec![];
        for (resource, data) in input {
            let format = data["format"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| invalid("The tile format is not specified in the layer.json file."))?;
            let templates = data["tiles"]
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or_else(|| invalid("The layer.json file does not specify any tile URL templates."))?
                .iter()
                .map(|v| v.as_str().unwrap_or("").to_owned())
                .collect();
            let is_heightmap = format == "heightmap-1.0";
            if !is_heightmap && !format.starts_with("quantized-mesh-1.") {
                return Err(invalid(format!("The tile format \"{format}\" is invalid or not supported.")));
            }
            if is_heightmap {
                heightmap = true;
                options.request_water_mask = true;
            }
            max_zoom = max_zoom.max(data["maxzoom"].as_u64().map(|v| v as u32).unwrap_or(u32::MAX));
            scheme = match data["projection"].as_str() {
                None | Some("EPSG:4326") => {
                    Rc::new(GeographicTilingScheme::new(options.ellipsoid, Rectangle::MAX_VALUE, 2, 1))
                }
                Some("EPSG:3857") => Rc::new(WebMercatorTilingScheme::new(options.ellipsoid, 1, 1, None)),
                Some(s) => return Err(invalid(format!("The projection \"{s}\" is invalid or not supported."))),
            };
            scheme_name = data["scheme"].as_str().unwrap_or("").to_owned();
            if !matches!(scheme_name.as_str(), "" | "tms" | "slippyMap") {
                return Err(invalid(format!("The scheme \"{scheme_name}\" is invalid or not supported.")));
            }
            let extension = |name: &str| data["extensions"].as_array().is_some_and(|a| a.iter().any(|v| v == name));
            let little_endian = !extension("vertexnormals") || extension("octvertexnormals");
            let has_normals = extension("vertexnormals") || extension("octvertexnormals");
            let has_water = is_heightmap || extension("watermask");
            let has_metadata = extension("metadata");
            let availability_levels = data["metadataAvailability"].as_u64().map(|v| v as u32);
            let (available, loaded) = if let Some(_) = availability_levels {
                let a = Rc::new(TileAvailability::new(scheme.clone(), max_zoom));
                a.add_available_tile_range(0, 0, 0, 1, 0);
                overall.retain(|(l, _)| *l != 0);
                overall.push((0, [0, 0, 1, 0]));
                (Some(a), Some(Rc::new(TileAvailability::new(scheme.clone(), max_zoom))))
            } else if let Some(levels) = data["available"].as_array() {
                let a = Rc::new(TileAvailability::new(scheme.clone(), levels.len() as u32));
                let rs = ranges(&data["available"], 0, scheme.as_ref());
                add_ranges(&a, &rs);
                overall.extend(rs);
                (Some(a), None)
            } else {
                (None, None)
            };
            water |= has_water;
            normals |= has_normals;
            metadata |= has_metadata;
            if let Some(s) = data["attribution"].as_str() {
                if !attribution.is_empty() {
                    attribution.push(' ');
                }
                attribution.push_str(s);
            }
            last_credits = resource.options().credits.clone().unwrap_or_default();
            layers.push(Layer {
                resource,
                version: data["version"].as_str().unwrap_or("undefined").into(),
                templates,
                available,
                loaded,
                availability_levels,
                normals: has_normals,
                water: has_water,
                metadata: has_metadata,
                little_endian,
                ion,
            });
        }
        let available = if overall.is_empty() {
            None
        } else {
            let a = Rc::new(TileAvailability::new(scheme.clone(), max_zoom));
            add_ranges(&a, &overall);
            Some(a)
        };
        if !attribution.is_empty() {
            for _ in 0..layers.len() {
                last_credits.push(Credit::new(attribution.clone(), false));
            }
        }
        let level_zero_error = super::terrain_provider::get_estimated_level_zero_geometric_error_for_a_heightmap(
            &options.ellipsoid,
            65.,
            scheme.get_number_of_x_tiles_at_level(0) as f64,
            None,
        );
        Ok(Self {
            state: Rc::new(State {
                client,
                scheme,
                scheme_name,
                layers,
                options,
                heightmap,
                water,
                normals,
                metadata,
                available,
                tile_credits: last_credits,
                level_zero_error,
                error: Arc::new(Mutex::new(Event::new())),
                pending: Default::default(),
            }),
        })
    }
    pub fn request_vertex_normals(&self) -> bool {
        self.state.options.request_vertex_normals
    }
    pub fn request_water_mask(&self) -> bool {
        self.state.options.request_water_mask
    }
    pub fn request_metadata(&self) -> bool {
        self.state.options.request_metadata
    }
    pub fn has_metadata(&self) -> bool {
        self.state.metadata && self.request_metadata()
    }
    pub fn tile_credits(&self) -> &[Credit] {
        &self.state.tile_credits
    }
    pub fn layer_count(&self) -> usize {
        self.state.layers.len()
    }
    pub fn availability_tile(levels: u32, x: u32, y: u32, level: u32) -> Option<(u32, u32, u32)> {
        if level == 0 || levels == 0 {
            return None;
        }
        let parent = if level % levels == 0 { level - levels } else { level / levels * levels };
        let div = 1u32.wrapping_shl(level - parent);
        if div == 0 {
            return None;
        }
        Some((x / div, y / div, parent))
    }
    pub fn tile_resource(
        &self,
        layer: usize,
        x: u32,
        y: u32,
        level: u32,
        request: Option<RequestHandle<HttpResponse>>,
    ) -> Option<Resource> {
        self.state.tile_resource(layer, x, y, level, request)
    }
    pub fn decode_tile(
        &self,
        layer: usize,
        x: u32,
        y: u32,
        level: u32,
        bytes: &[u8],
    ) -> Result<NetworkTerrainData, RuntimeError> {
        self.state.decode_tile(layer, x, y, level, bytes)
    }
}
impl State {
    fn tile_resource(
        &self,
        index: usize,
        x: u32,
        y: u32,
        l: u32,
        request: Option<RequestHandle<HttpResponse>>,
    ) -> Option<Resource> {
        let layer = self.layers.get(index)?;
        if layer.templates.is_empty() {
            return None;
        }
        let terrain_y = if self.scheme_name == "slippyMap" {
            y
        } else {
            (self.scheme.get_number_of_y_tiles_at_level(l) as u32).wrapping_sub(y).wrapping_sub(1)
        };
        let mut extensions = vec![];
        if self.options.request_vertex_normals && layer.normals {
            extensions.push(if layer.little_endian { "octvertexnormals" } else { "vertexnormals" });
        }
        if self.options.request_water_mask && layer.water {
            extensions.push("watermask");
        }
        if self.options.request_metadata && layer.metadata {
            extensions.push("metadata");
        }
        let mut query = vec![];
        let extension = if extensions.is_empty() || layer.ion {
            if layer.ion && !extensions.is_empty() {
                query.push(("extensions".into(), Some(super::query::QueryValue::Single(extensions.join("-")))));
            }
            String::new()
        } else {
            format!(";extensions={}", extensions.join("-"))
        };
        let url =
            layer.templates[((x as u64 + terrain_y as u64 + l as u64) % layer.templates.len() as u64) as usize].clone();
        layer.resource.get_derived_resource(DerivedResourceOptions {
            url: Some(url),
            template_values: vec![
                ("version".into(), layer.version.clone()),
                ("z".into(), l.to_string()),
                ("x".into(), x.to_string()),
                ("y".into(), terrain_y.to_string()),
            ],
            query_parameters: query,
            headers: vec![(
                "Accept".into(),
                format!("application/vnd.quantized-mesh{extension},application/octet-stream;q=0.9,*/*;q=0.01"),
            )],
            request,
            ..Default::default()
        })
    }
    fn request_layer(
        self: &Rc<Self>,
        layer: usize,
        x: u32,
        y: u32,
        l: u32,
        request: Option<RequestHandle<HttpResponse>>,
    ) -> Option<TerrainFuture<'static, NetworkTerrainData>> {
        let r = self.tile_resource(layer, x, y, l, request)?;
        let f = fetch_bytes(&self.client, &r)?;
        let weak = Rc::downgrade(self);
        Some(Box::pin(async move {
            let bytes = f.await?;
            let state = weak.upgrade().ok_or_else(|| invalid("Terrain provider was dropped."))?;
            state.decode_tile(layer, x, y, l, &bytes)
        }))
    }
    fn decode_tile(
        &self,
        index: usize,
        x: u32,
        y: u32,
        level: u32,
        bytes: &[u8],
    ) -> Result<NetworkTerrainData, RuntimeError> {
        let mut r = Reader::new(bytes);
        if self.heightmap {
            let values = (0..65 * 65).map(|_| r.u16()).collect::<Result<Vec<_>, _>>()?;
            let mask = r.u8()?;
            let water = r.take(bytes.len() - r.pos)?.to_vec();
            let mut o = HeightmapTerrainDataOptions::new(TypedArray::Uint16(values), 65, 65);
            o.child_tile_mask = mask;
            o.water_mask = Some(water);
            o.structure = HeightmapTerrainStructure {
                samples: HeightmapStructure { height_scale: 0.2, height_offset: -1000., ..Default::default() },
                lowest_encoded_height: Some(0.),
                highest_encoded_height: Some(65535.),
            };
            return HeightmapTerrainData::new(o)
                .map(|data| NetworkTerrainData::Heightmap { data, credits: Some(self.tile_credits.clone()) })
                .ok_or_else(|| invalid("Invalid terrain heightmap."));
        }
        let layer = &self.layers[index];
        let _center = r.c3()?;
        let min = r.f32()?;
        let max = r.f32()?;
        let sphere = BoundingSphere::new(r.c3()?, r.f64()?);
        let horizon = r.c3()?;
        let count = r.u32()? as usize;
        if count > bytes.len() / 6 {
            return Err(invalid("Terrain vertex count is out of bounds."));
        }
        let mut vertices = (0..count * 3).map(|_| r.u16()).collect::<Result<Vec<_>, _>>()?;
        let (u, rest) = vertices.split_at_mut(count);
        let (v, h) = rest.split_at_mut(count);
        AttributeCompression::zig_zag_delta_decode(u, v, Some(h))
            .ok_or_else(|| invalid("Invalid quantized vertices."))?;
        let wide = count > 65536;
        let alignment = if wide { 4 } else { 2 };
        if r.pos % alignment != 0 {
            r.take(alignment - r.pos % alignment)?;
        }
        let triangles = r.u32()? as usize;
        let mut indices = r.indices(
            triangles.checked_mul(3).ok_or_else(|| invalid("Terrain triangle count is out of bounds."))?,
            wide,
        )?;
        let mut highest = 0u32;
        for i in &mut indices {
            let code = *i;
            *i = highest.wrapping_sub(code);
            if code == 0 {
                highest = highest.wrapping_add(1);
            }
        }
        let mut edges = vec![];
        for _ in 0..4 {
            let n = r.u32()? as usize;
            edges.push(r.indices(n, wide)?);
        }
        let mut normals = None;
        let mut water = None;
        while r.pos < bytes.len() {
            let id = r.u8()?;
            let length = if layer.little_endian { r.u32()? } else { u32::from_be_bytes(r.take(4)?.try_into().unwrap()) }
                as usize;
            let payload = r.take(length)?;
            match id {
                1 if self.options.request_vertex_normals => {
                    normals = Some(
                        payload.get(..count * 2).ok_or_else(|| invalid("Terrain normals are truncated."))?.to_vec(),
                    );
                }
                2 if self.options.request_water_mask => water = Some(payload.to_vec()),
                4 if self.options.request_metadata => {
                    let mut m = Reader::new(payload);
                    let len = m.u32()? as usize;
                    if len > 0 {
                        let metadata: Value =
                            serde_json::from_slice(m.take(len)?).map_err(|e| invalid(e.to_string()))?;
                        let rs = ranges(&metadata["available"], level + 1, self.scheme.as_ref());
                        if let Some(a) = &self.available {
                            add_ranges(a, &rs);
                        }
                        if let Some(a) = &layer.available {
                            add_ranges(a, &rs);
                        }
                    }
                    if let Some(a) = &layer.loaded {
                        a.add_available_tile_range(level, x, y, x, y);
                    }
                }
                _ => {}
            }
        }
        let availability =
            self.available.as_ref().ok_or_else(|| invalid("Quantized terrain availability is missing."))?;
        let rectangle = self.scheme.tile_xy_to_rectangle(x as f64, y as f64, level);
        let mut o = QuantizedMeshTerrainDataOptions::new(
            vertices,
            if wide {
                TypedArray::Uint32(indices)
            } else {
                TypedArray::Uint16(indices.into_iter().map(|v| v as u16).collect())
            },
            [min, max],
            sphere,
            horizon,
        );
        o.edges = QuantizedTerrainEdges {
            west: edges.remove(0),
            south: edges.remove(0),
            east: edges.remove(0),
            north: edges.remove(0),
        };
        o.encoded_normals = normals;
        o.water_mask = water;
        o.credits = Some(self.tile_credits.clone());
        o.child_tile_mask = availability.compute_child_mask_for_tile(level, x, y);
        o.skirt_heights = [self.level_zero_error / 1i32.wrapping_shl(level) as f64 * 5.; 4];
        o.oriented_bounding_box = OrientedBoundingBox::from_rectangle(&rectangle, min, max, self.scheme.ellipsoid());
        QuantizedMeshTerrainData::new(o)
            .map(NetworkTerrainData::Quantized)
            .ok_or_else(|| invalid("Invalid quantized terrain tile."))
    }
    fn check_layer(self: &Rc<Self>, index: usize, x: u32, y: u32, level: u32) -> (bool, Option<Availability>) {
        let layer = &self.layers[index];
        let Some(interval) = layer.availability_levels else { return (false, None) };
        let mut tile = CesiumTerrainProvider::availability_tile(interval, x, y, level);
        while let Some((x, y, l)) = tile {
            if layer.available.as_ref().is_some_and(|a| a.is_tile_available(l, x, y))
                && !layer.loaded.as_ref().is_some_and(|a| a.is_tile_available(l, x, y))
            {
                if index == 0 {
                    return (true, None);
                }
                let key = format!("{index}-{l}-{x}-{y}");
                if let Some(f) = self.pending.borrow().get(&key) {
                    return (true, Some(f.clone()));
                }
                let request = Request::<HttpResponse>::new(RequestOptions {
                    throttle_by_server: true,
                    request_type: RequestType::Terrain,
                    ..Default::default()
                })
                .into_handle();
                let Some(job) = self.request_layer(index, x, y, l, Some(request)) else { return (true, None) };
                let weak = Rc::downgrade(&self.pending);
                let cache_key = key.clone();
                let future = async move {
                    let r = job.await.map(|_| ());
                    if let Some(cache) = weak.upgrade() {
                        cache.borrow_mut().remove(&cache_key);
                    }
                    r
                }
                .boxed_local()
                .shared();
                self.pending.borrow_mut().insert(key, future.clone());
                return (true, Some(future));
            }
            tile = CesiumTerrainProvider::availability_tile(interval, x, y, l);
        }
        (false, None)
    }
    fn pump(&self) {
        let jobs = self.pending.borrow().values().cloned().collect::<Vec<_>>();
        for mut f in jobs {
            let _ = Pin::new(&mut f).poll(&mut Context::from_waker(Waker::noop()));
        }
    }
}
impl TerrainProvider for CesiumTerrainProvider {
    type Error = RuntimeError;
    type Credit = Credit;
    type TerrainData = NetworkTerrainData;
    type Request = RequestHandle<HttpResponse>;
    type GeometryFuture<'a> = TerrainFuture<'a, NetworkTerrainData>;
    type AvailabilityFuture<'a> = TerrainFuture<'a, ()>;
    fn error_event(&self) -> Arc<Mutex<Event<RuntimeError>>> {
        self.state.error.clone()
    }
    fn credit(&self) -> Option<&Credit> {
        self.state.options.credit.as_ref()
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.state.scheme.as_ref()
    }
    fn has_water_mask(&self) -> bool {
        self.state.water && self.request_water_mask()
    }
    fn has_vertex_normals(&self) -> bool {
        self.state.normals && self.request_vertex_normals()
    }
    fn availability(&self) -> Option<&TileAvailability> {
        self.state.available.as_deref()
    }
    fn request_tile_geometry(
        &self,
        x: u32,
        y: u32,
        l: u32,
        request: Option<Self::Request>,
    ) -> Option<Self::GeometryFuture<'_>> {
        let s = self.state.clone();
        if s.layers.len() == 1 {
            return s.request_layer(0, x, y, l, request);
        }
        let mut pending = vec![];
        let mut unknown = false;
        for (i, layer) in s.layers.iter().enumerate() {
            if layer.available.as_ref().is_none_or(|a| a.is_tile_available(l, x, y)) {
                return s.request_layer(i, x, y, l, request);
            }
            let (u, p) = s.check_layer(i, x, y, l);
            unknown |= u;
            if let Some(p) = p {
                pending.push(p);
            }
        }
        if !unknown {
            return Some(Box::pin(async { Err(invalid("Terrain tile doesn't exist")) }));
        }
        Some(Box::pin(async move {
            for p in pending {
                p.await?;
            }
            let mut yielded = false;
            poll_fn(|cx| {
                if yielded {
                    Poll::Ready(())
                } else {
                    yielded = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
            self.request_tile_geometry(x, y, l, request).ok_or_else(|| invalid("Terrain request was throttled."))?.await
        }))
    }
    fn get_level_maximum_geometric_error(&self, l: u32) -> f64 {
        self.state.level_zero_error / 1i32.wrapping_shl(l) as f64
    }
    fn get_tile_data_available(&self, x: u32, y: u32, l: u32) -> Option<bool> {
        self.state.pump();
        let a = self.state.available.as_ref()?;
        if l > a.maximum_level() {
            return Some(false);
        }
        if a.is_tile_available(l, x, y) {
            return Some(true);
        }
        if !self.state.metadata {
            return Some(false);
        }
        for i in 0..self.state.layers.len() {
            if self.state.check_layer(i, x, y, l).0 {
                return None;
            }
        }
        Some(false)
    }
    fn load_tile_data_availability(&self, x: u32, y: u32, l: u32) -> Option<Self::AvailabilityFuture<'_>> {
        let a = self.state.available.as_ref()?;
        if l > a.maximum_level() || a.is_tile_available(l, x, y) || !self.state.metadata {
            return None;
        }
        for i in 0..self.state.layers.len() {
            if let Some(f) = self.state.check_layer(i, x, y, l).1 {
                return Some(Box::pin(f));
            }
        }
        None
    }
}
