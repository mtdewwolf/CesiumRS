//! Geographic two-root implicit 3D Tiles terrain, subtree metadata and GLB loading.
use super::*;
use super::{
    network_terrain::{fetch_bytes, fetch_json, TerrainFuture},
    resource::DerivedResourceOptions,
};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Ready,
    rc::Rc,
    sync::{Arc, Mutex},
};
type WaterMaskDecoder = Rc<dyn Fn(Vec<u8>) -> TerrainFuture<'static, Vec<u8>>>;
#[derive(Clone, Default)]
pub struct TilesTerrainOptions {
    pub ellipsoid: Option<Ellipsoid>,
    pub credit: Option<Credit>,
    pub request_vertex_normals: bool,
    pub request_water_mask: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImplicitTerrainCoordinates {
    pub level: u32,
    pub x: u32,
    pub y: u32,
}
impl ImplicitTerrainCoordinates {
    fn subtree(self, n: u32) -> Self {
        let level = self.level / n * n;
        let shift = self.level - level;
        Self { level, x: self.x >> shift, y: self.y >> shift }
    }
    fn parent(self, n: u32) -> Self {
        Self { level: self.level - n, x: self.x >> n, y: self.y >> n }
    }
    fn morton(self) -> u64 {
        let mut m = 0;
        for i in 0..self.level.min(31) {
            m |= ((self.x >> i & 1) as u64) << (i * 2);
            m |= ((self.y >> i & 1) as u64) << (i * 2 + 1);
        }
        m
    }
    fn local(self, root: Self) -> Self {
        let level = self.level - root.level;
        let mask = (1u32 << level) - 1;
        Self { level, x: self.x & mask, y: self.y & mask }
    }
    fn tile_index(self) -> usize {
        (((1u64 << (self.level * 2)) - 1) / 3 + self.morton()) as usize
    }
}
#[derive(Clone)]
struct Tileset {
    subtree_levels: u32,
    available_levels: u32,
    subtree: String,
    content: String,
}
#[derive(Clone, Debug)]
pub struct ImplicitTerrainSubtree {
    pub coordinates: ImplicitTerrainCoordinates,
    pub json: Value,
    pub buffers: Vec<Vec<u8>>,
    stamp: u64,
}
fn err(s: impl Into<String>) -> RuntimeError {
    RuntimeError::new(s)
}
fn uint(v: &Value) -> Result<usize, RuntimeError> {
    v.as_u64().map(|n| n as usize).ok_or_else(|| err("Invalid implicit terrain metadata."))
}
impl ImplicitTerrainSubtree {
    pub fn from_binary(coordinates: ImplicitTerrainCoordinates, bytes: &[u8]) -> Result<Self, RuntimeError> {
        let (json, buffer): (Value, Vec<u8>) = if bytes.starts_with(b"subt") {
            let mut r = super::network_terrain::Reader::new(bytes);
            r.u32()?;
            if r.u32()? != 1 {
                return Err(err("Unsupported subtree version."));
            }
            let jl = u64::from_le_bytes(r.take(8)?.try_into().unwrap()) as usize;
            let bl = u64::from_le_bytes(r.take(8)?.try_into().unwrap()) as usize;
            let json = serde_json::from_slice(r.take(jl)?).map_err(|e| err(e.to_string()))?;
            (json, r.take(bl)?.to_vec())
        } else {
            (serde_json::from_slice(bytes).map_err(|e| err(e.to_string()))?, vec![])
        };
        let mut buffers = vec![];
        if let Some(list) = json["buffers"].as_array() {
            for b in list {
                if b["uri"].is_null() {
                    buffers.push(buffer.clone());
                } else {
                    buffers.push(vec![]);
                }
            }
        }
        Ok(Self { coordinates, json, buffers, stamp: 0 })
    }
    pub fn buffer_view(&self, id: usize) -> Result<&[u8], RuntimeError> {
        let v = self.json["bufferViews"].get(id).ok_or_else(|| err("Invalid subtree buffer view."))?;
        let b = self.buffers.get(uint(&v["buffer"])?).ok_or_else(|| err("Missing subtree buffer."))?;
        let start = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        let end = start.checked_add(uint(&v["byteLength"])?).ok_or_else(|| err("Subtree buffer overflow."))?;
        b.get(start..end).ok_or_else(|| err("Subtree buffer is truncated."))
    }
    fn available(&self, kind: &str, index: usize) -> bool {
        let mut availability = &self.json[kind];
        if availability.is_array() {
            availability = &availability[0];
        }
        if let Some(c) = availability["constant"].as_u64() {
            return c != 0;
        }
        let id = availability["bitstream"].as_u64().or_else(|| availability["bufferView"].as_u64());
        id.and_then(|id| self.buffer_view(id as usize).ok())
            .and_then(|b| b.get(index / 8))
            .is_some_and(|b| b & (1 << (index % 8)) != 0)
    }
    fn tile_available(&self, c: ImplicitTerrainCoordinates) -> bool {
        self.available("tileAvailability", c.local(self.coordinates).tile_index())
    }
    fn child_available(&self, c: ImplicitTerrainCoordinates) -> bool {
        self.available("childSubtreeAvailability", c.local(self.coordinates).morton() as usize)
    }
    fn semantic(
        &self,
        schema: &Value,
        tile: ImplicitTerrainCoordinates,
        semantic: &str,
    ) -> Result<Vec<f64>, RuntimeError> {
        let index = tile.local(self.coordinates).tile_index();
        if !self.available("tileAvailability", index) {
            return Err(err("Terrain tile metadata is unavailable."));
        }
        let row = (0..index).filter(|&i| self.available("tileAvailability", i)).count();
        let table = if self.json["tileMetadata"].is_number() {
            &self.json["propertyTables"][uint(&self.json["tileMetadata"])?]
        } else {
            &self.json["extensions"]["3DTILES_metadata"]
        };
        let class =
            &schema["classes"][table["class"].as_str().ok_or_else(|| err("Tile metadata class is missing."))?];
        let props = class["properties"].as_object().ok_or_else(|| err("Tile metadata properties are missing."))?;
        let (id, property) = props
            .iter()
            .find(|(_, p)| p["semantic"] == semantic)
            .ok_or_else(|| err(format!("Missing {semantic} metadata.")))?;
        let storage = &table["properties"][id];
        let fallback = property["default"].clone();
        if storage.is_null() {
            return numbers(&fallback);
        }
        let components = match property["type"].as_str().unwrap_or("SCALAR") {
            "VEC2" => 2,
            "VEC3" => 3,
            "VEC4" => 4,
            "MAT2" => 4,
            "MAT3" => 9,
            "MAT4" => 16,
            _ => 1,
        };
        let component = property["componentType"].as_str().unwrap_or("FLOAT64");
        let count = if property["array"] == true { property["count"].as_u64().unwrap_or(1) as usize } else { 1 };
        let mut first = row * count;
        let mut last = first + count;
        if property["array"] == true && property["count"].is_null() {
            let offsets = self.buffer_view(uint(&storage["arrayOffsets"])?)?;
            let ty = storage["arrayOffsetType"].as_str().unwrap_or("UINT32");
            first = scalar(offsets, row, ty)? as usize;
            last = scalar(offsets, row + 1, ty)? as usize;
        }
        let bytes = self.buffer_view(uint(&storage["values"])?)?;
        let mut out = vec![];
        for i in first * components..last * components {
            out.push(scalar(bytes, i, component)?);
        }
        if !property["noData"].is_null() && out == numbers(&property["noData"])? {
            return numbers(&fallback);
        }
        for (i, v) in out.iter_mut().enumerate() {
            if property["normalized"] == true {
                let max = match component {
                    "UINT8" => 255.,
                    "UINT16" => 65535.,
                    "UINT32" => 4294967295.,
                    "INT8" => 127.,
                    "INT16" => 32767.,
                    "INT32" => 2147483647.,
                    _ => 1.,
                };
                *v = (*v / max).max(if component.starts_with("UINT") { 0. } else { -1. });
            }
            let transform = |name: &str, default: f64| {
                storage[name]
                    .as_f64()
                    .or_else(|| storage[name][i % components].as_f64())
                    .or_else(|| property[name].as_f64())
                    .or_else(|| property[name][i % components].as_f64())
                    .unwrap_or(default)
            };
            *v = *v * transform("scale", 1.) + transform("offset", 0.);
        }
        Ok(out)
    }
}
fn numbers(v: &Value) -> Result<Vec<f64>, RuntimeError> {
    if let Some(n) = v.as_f64() {
        return Ok(vec![n]);
    }
    v.as_array()
        .ok_or_else(|| err("Invalid numeric metadata value."))?
        .iter()
        .map(|v| v.as_f64().ok_or_else(|| err("Invalid numeric metadata value.")))
        .collect()
}
fn scalar(bytes: &[u8], index: usize, ty: &str) -> Result<f64, RuntimeError> {
    let size = match ty {
        "INT8" | "UINT8" => 1,
        "INT16" | "UINT16" => 2,
        "INT32" | "UINT32" | "FLOAT32" => 4,
        "INT64" | "UINT64" | "FLOAT64" => 8,
        _ => return Err(err("Unsupported numeric metadata type.")),
    };
    let start = index.checked_mul(size).ok_or_else(|| err("Metadata offset overflow."))?;
    let b = bytes.get(start..start + size).ok_or_else(|| err("Metadata buffer is truncated."))?;
    Ok(match ty {
        "INT8" => b[0] as i8 as f64,
        "UINT8" => b[0] as f64,
        "INT16" => i16::from_le_bytes(b.try_into().unwrap()) as f64,
        "UINT16" => u16::from_le_bytes(b.try_into().unwrap()) as f64,
        "INT32" => i32::from_le_bytes(b.try_into().unwrap()) as f64,
        "UINT32" => u32::from_le_bytes(b.try_into().unwrap()) as f64,
        "INT64" => i64::from_le_bytes(b.try_into().unwrap()) as f64,
        "UINT64" => u64::from_le_bytes(b.try_into().unwrap()) as f64,
        "FLOAT32" => f32::from_le_bytes(b.try_into().unwrap()) as f64,
        _ => f64::from_le_bytes(b.try_into().unwrap()),
    })
}
pub struct Cesium3DTilesTerrainProvider {
    pub resource: Resource,
    scheme: GeographicTilingScheme,
    options: TilesTerrainOptions,
    schema: Value,
    tilesets: [Tileset; 2],
    client: ResourceClient,
    cache: RefCell<HashMap<(u8, ImplicitTerrainCoordinates), ImplicitTerrainSubtree>>,
    stamp: Cell<u64>,
    pub maximum_subtree_count: usize,
    available: TileAvailability,
    water_decoder: Option<WaterMaskDecoder>,
    error: Arc<Mutex<Event<RuntimeError>>>,
}
impl Cesium3DTilesTerrainProvider {
    pub async fn from_url(
        resource: Resource,
        options: TilesTerrainOptions,
        client: ResourceClient,
    ) -> Result<Self, RuntimeError> {
        let json = fetch_json(&client, &resource)
            .ok_or_else(|| err("Tileset request was throttled."))?
            .await
            .map_err(|_| err("Could not load tileset JSON"))?;
        Self::from_json(resource, &json, options, client)
    }
    pub async fn from_ion_asset_id(
        id: u64,
        ion: &Ion,
        asset: IonAssetOptions,
        options: TilesTerrainOptions,
        client: ResourceClient,
    ) -> Result<Self, RuntimeError> {
        let ion =
            IonResource::from_asset_id(id, ion, asset, &client).await.map_err(super::network_terrain::net_error)?;
        let resource = ion.as_resource().clone();
        resource.options_mut().credits = Some(ion.credits());
        Self::from_url(resource, options, client).await
    }
    pub fn from_json(
        resource: Resource,
        json: &Value,
        options: TilesTerrainOptions,
        client: ResourceClient,
    ) -> Result<Self, RuntimeError> {
        let make = |i| {
            let tile = &json["root"]["children"][i];
            let implicit = tile.get("implicitTiling").unwrap_or(&tile["extensions"]["3DTILES_implicit_tiling"]);
            if implicit["subdivisionScheme"] != "QUADTREE" {
                return Err(err("Terrain requires QUADTREE implicit tiling."));
            }
            let levels = uint(&implicit["subtreeLevels"])? as u32;
            if levels == 0 || levels > 15 {
                return Err(err("Invalid implicit subtree depth."));
            }
            Ok(Tileset {
                subtree_levels: levels,
                available_levels: uint(&implicit["availableLevels"])? as u32,
                subtree: implicit["subtrees"]["uri"].as_str().ok_or_else(|| err("Missing subtree template."))?.into(),
                content: tile["content"]["uri"]
                    .as_str()
                    .or_else(|| tile["contents"][0]["uri"].as_str())
                    .ok_or_else(|| err("Missing terrain content template."))?
                    .into(),
            })
        };
        let tilesets = [make(0)?, make(1)?];
        let scheme =
            GeographicTilingScheme::new(options.ellipsoid.unwrap_or(Ellipsoid::WGS84), Rectangle::MAX_VALUE, 2, 1);
        let available =
            TileAvailability::new(scheme.clone(), tilesets.iter().map(|s| s.available_levels).max().unwrap_or(0));
        available.add_available_tile_range(0, 0, 0, 1, 0);
        Ok(Self {
            resource,
            scheme,
            options,
            schema: json["schema"].clone(),
            tilesets,
            client,
            cache: Default::default(),
            stamp: Cell::new(0),
            maximum_subtree_count: 0,
            available,
            water_decoder: None,
            error: Arc::new(Mutex::new(Event::new())),
        })
    }
    pub fn set_image_host<H: ImageHost + 'static>(&mut self, host: Rc<H>) {
        self.water_decoder = Some(Rc::new(move |bytes| {
            let host = host.clone();
            Box::pin(async move {
                let image = host
                    .decode_image(
                        &bytes,
                        "image/png",
                        ImageDecodeOptions { flip_y: false, skip_color_space_conversion: true },
                    )
                    .ok_or_else(|| err("Water mask image decode was throttled."))?
                    .await?;
                super::image_helpers::get_image_pixels(host.as_ref(), &image, None, None)
            })
        }));
    }
    pub fn coordinates(x: u32, y: u32, l: u32) -> (u8, ImplicitTerrainCoordinates) {
        let n = 1u32.wrapping_shl(l);
        ((x / n) as u8, ImplicitTerrainCoordinates { level: l, x: x % n, y: n.wrapping_sub(y).wrapping_sub(1) })
    }
    pub fn tile_resource(&self, root: u8, c: ImplicitTerrainCoordinates, subtree: bool) -> Option<Resource> {
        let t = &self.tilesets[usize::from(root != 0)];
        self.resource.get_derived_resource(DerivedResourceOptions {
            url: Some(if subtree { t.subtree.clone() } else { t.content.clone() }),
            template_values: vec![
                ("level".into(), c.level.to_string()),
                ("x".into(), c.x.to_string()),
                ("y".into(), c.y.to_string()),
            ],
            ..Default::default()
        })
    }
    pub fn add_subtree(&self, root: u8, mut subtree: ImplicitTerrainSubtree) -> Result<(), RuntimeError> {
        let n = self.tilesets[usize::from(root != 0)].subtree_levels;
        let coord = subtree.coordinates;
        // Populate the native availability view as each subtree becomes known.
        for local in 0..=n {
            let dim = 1u32 << local;
            let level = coord.level + local;
            if level >= 31 {
                break;
            }
            let global_x = (root as u32 * (1u32 << coord.level) + coord.x) * dim;
            let global_y = coord.y * dim;
            let y_tiles = 1u32 << level;
            let kind = if local == n { "childSubtreeAvailability" } else { "tileAvailability" };
            let known = |c| if local == n { subtree.child_available(c) } else { subtree.tile_available(c) };
            let constant = subtree.json[kind]["constant"].as_u64();
            if constant == Some(1) {
                self.available.add_available_tile_range(
                    level,
                    global_x,
                    y_tiles - (global_y + dim),
                    global_x + dim - 1,
                    y_tiles - global_y - 1,
                );
            } else if constant != Some(0) {
                for row in 0..dim {
                    let mut x = 0;
                    while x < dim {
                        let c = ImplicitTerrainCoordinates { level, x: coord.x * dim + x, y: global_y + row };
                        if !known(c) {
                            x += 1;
                            continue;
                        }
                        let start = x;
                        while x < dim
                            && known(ImplicitTerrainCoordinates { level, x: coord.x * dim + x, y: global_y + row })
                        {
                            x += 1;
                        }
                        self.available.add_available_tile_range(
                            level,
                            global_x + start,
                            y_tiles - global_y - row - 1,
                            global_x + x - 1,
                            y_tiles - global_y - row - 1,
                        );
                    }
                }
            }
        }
        let mut cache = self.cache.borrow_mut();
        if coord.level > 0 && !cache.contains_key(&(root, coord.parent(n))) {
            return Err(err("parent node needs to exist"));
        }
        subtree.stamp = self.stamp.get();
        self.stamp.set(self.stamp.get() + 1);
        cache.insert((root, coord), subtree);
        while self.maximum_subtree_count > 0 && cache.len() > self.maximum_subtree_count {
            let victim = cache
                .iter()
                .filter(|(k, _)| **k != (root, coord))
                .min_by(|a, b| {
                    let ca = a.0 .1;
                    let cb = b.0 .1;
                    let ancestor = |a: ImplicitTerrainCoordinates, b: ImplicitTerrainCoordinates| {
                        a.level < b.level && b.x >> (b.level - a.level) == a.x && b.y >> (b.level - a.level) == a.y
                    };
                    if ancestor(ca, cb) {
                        std::cmp::Ordering::Greater
                    } else if ancestor(cb, ca) {
                        std::cmp::Ordering::Less
                    } else {
                        a.1.stamp.cmp(&b.1.stamp)
                    }
                })
                .map(|(k, _)| *k);
            if let Some(k) = victim {
                cache.remove(&k);
            } else {
                break;
            }
        }
        Ok(())
    }
    async fn subtree(&self, root: u8, c: ImplicitTerrainCoordinates) -> Result<ImplicitTerrainSubtree, RuntimeError> {
        if let Some(s) = self.cache.borrow().get(&(root, c)) {
            return Ok(s.clone());
        }
        let resource = self.tile_resource(root, c, true).ok_or_else(|| err("Invalid subtree URL."))?;
        let bytes = fetch_bytes(&self.client, &resource).ok_or_else(|| err("Subtree request was throttled."))?.await?;
        if let Some(s) = self.cache.borrow().get(&(root, c)) {
            return Ok(s.clone());
        }
        let mut subtree = ImplicitTerrainSubtree::from_binary(c, &bytes)?;
        for i in 0..subtree.buffers.len() {
            if let Some(uri) = subtree.json["buffers"][i]["uri"].as_str() {
                let r = resource
                    .get_derived_resource(DerivedResourceOptions { url: Some(uri.into()), ..Default::default() })
                    .ok_or_else(|| err("Invalid subtree buffer URL."))?;
                subtree.buffers[i] =
                    fetch_bytes(&self.client, &r).ok_or_else(|| err("Subtree buffer request was throttled."))?.await?;
            }
        }
        self.add_subtree(root, subtree.clone())?;
        Ok(subtree)
    }
    async fn load_gltf(&self, resource: &Resource) -> Result<TerrainGltf, RuntimeError> {
        let bytes =
            fetch_bytes(&self.client, resource).ok_or_else(|| err("Terrain content request was throttled."))?.await?;
        let mut g = TerrainGltf::from_glb(&bytes)?;
        let count = g.json["buffers"].as_array().map_or(0, Vec::len);
        g.buffers.resize_with(count, Vec::new);
        for i in 0..count {
            if let Some(uri) = g.json["buffers"][i]["uri"].as_str() {
                let r = resource
                    .get_derived_resource(DerivedResourceOptions { url: Some(uri.into()), ..Default::default() })
                    .ok_or_else(|| err("Invalid glTF buffer URL."))?;
                g.buffers[i] =
                    fetch_bytes(&self.client, &r).ok_or_else(|| err("glTF buffer request was throttled."))?.await?;
            }
        }
        Ok(g)
    }
    async fn water_mask(&self, g: &TerrainGltf) -> Result<Option<Vec<u8>>, RuntimeError> {
        let extension = &g.json["extensions"]["EXT_structural_metadata"];
        let Some(classes) = extension["schema"]["classes"].as_object() else { return Ok(None) };
        let Some((id, name)) = classes.iter().find_map(|(id, c)| {
            c["properties"].as_object()?.iter().find(|(_, p)| p["semantic"] == "WATERMASK").map(|(name, _)| (id, name))
        }) else {
            return Ok(None);
        };
        let data = extension["propertyTextures"]
            .as_array()
            .and_then(|a| a.iter().find(|p| p["class"] == *id))
            .ok_or_else(|| err(format!("Expected a propertyTexture with a class {id}")))?;
        let texture = &g.json["textures"][uint(&data["properties"][name]["index"])?];
        let image = &g.json["images"][uint(&texture["source"])?];
        let view = &g.json["bufferViews"][uint(&image["bufferView"])?];
        let bytes = g.buffers.get(uint(&view["buffer"])?).ok_or_else(|| err("Missing water mask buffer."))?;
        let start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
        let len = uint(&view["byteLength"])?;
        let bytes = bytes.get(start..start + len).ok_or_else(|| err("Truncated water mask image."))?.to_vec();
        let decode = self
            .water_decoder
            .as_ref()
            .ok_or_else(|| err("A terrain ImageHost is required for water mask PNG decoding."))?;
        decode(bytes).await.map(Some)
    }
    pub fn compute_maximum_level_at_position(&self, p: &Cartographic) -> u32 {
        let root = u8::from(p.longitude >= 0.);
        let t = &self.tilesets[root as usize];
        let mut c = ImplicitTerrainCoordinates { level: 0, x: 0, y: 0 };
        let cache = self.cache.borrow();
        let Some(mut s) = cache.get(&(root, c)) else { return 0 };
        loop {
            let dim = (1u64 << c.level) as f64;
            let lon0 = -std::f64::consts::PI + root as f64 * std::f64::consts::PI;
            let u = (p.longitude - lon0) / std::f64::consts::PI * dim - c.x as f64;
            let v = (p.latitude + std::f64::consts::FRAC_PI_2) / std::f64::consts::PI * dim - c.y as f64;
            let descendant = |offset: u32| {
                let d = 1u32 << offset;
                ImplicitTerrainCoordinates {
                    level: c.level + offset,
                    x: c.x * d + (u * d as f64).trunc().clamp(0., (d - 1) as f64) as u32,
                    y: c.y * d + (v * d as f64).trunc().clamp(0., (d - 1) as f64) as u32,
                }
            };
            let child = descendant(t.subtree_levels);
            if s.child_available(child) {
                if let Some(next) = cache.get(&(root, child)) {
                    c = child;
                    s = next;
                    continue;
                }
                return child.level;
            }
            let mut deepest = 0;
            for local in 0..t.subtree_levels {
                let tile = descendant(local);
                if !s.tile_available(tile) {
                    break;
                }
                deepest = tile.level;
            }
            return deepest;
        }
    }
}
impl TerrainProvider for Cesium3DTilesTerrainProvider {
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
        self.options.credit.as_ref()
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        &self.scheme
    }
    fn has_water_mask(&self) -> bool {
        self.options.request_water_mask
    }
    fn has_vertex_normals(&self) -> bool {
        self.options.request_vertex_normals
    }
    fn availability(&self) -> Option<&TileAvailability> {
        Some(&self.available)
    }
    fn get_level_maximum_geometric_error(&self, l: u32) -> f64 {
        super::terrain_provider::get_estimated_level_zero_geometric_error_for_a_heightmap(
            self.scheme.ellipsoid(),
            64.,
            2.,
            None,
        ) / 1i32.wrapping_shl(l) as f64
    }
    fn request_tile_geometry(
        &self,
        x: u32,
        y: u32,
        l: u32,
        _: Option<Self::Request>,
    ) -> Option<Self::GeometryFuture<'_>> {
        let (root, c) = Self::coordinates(x, y, l);
        let t = &self.tilesets[usize::from(root != 0)];
        let coord = c.subtree(t.subtree_levels);
        let resource = self.tile_resource(root, c, false)?;
        Some(Box::pin(async move {
            let subtree = self.subtree(root, coord);
            let gltf = self.load_gltf(&resource);
            let (subtree, gltf) = futures_util::future::try_join(subtree, gltf).await?;
            let water = if self.options.request_water_mask { self.water_mask(&gltf).await? } else { None };
            let prop = |semantic| subtree.semantic(&self.schema, c, semantic);
            let min = prop("TILE_MINIMUM_HEIGHT")?[0];
            let max = prop("TILE_MAXIMUM_HEIGHT")?[0];
            let bs = prop("TILE_BOUNDING_SPHERE")?;
            let h = prop("TILE_HORIZON_OCCLUSION_POINT")?;
            if bs.len() < 4 || h.len() < 3 {
                return Err(err("Invalid terrain bounds metadata."));
            }
            let r = self.scheme.tile_xy_to_rectangle(x as f64, y as f64, l);
            let obb = OrientedBoundingBox::from_rectangle(&r, min, max, self.scheme.ellipsoid())
                .ok_or_else(|| err("Invalid terrain bounds."))?;
            let mut mask = 0;
            for (dx, dy, bit) in [(0, 0, 1), (1, 0, 2), (0, 1, 4), (1, 1, 8)] {
                let child = ImplicitTerrainCoordinates { level: c.level + 1, x: c.x * 2 + dx, y: c.y * 2 + dy };
                let available = if child.level.is_multiple_of(t.subtree_levels) {
                    subtree.child_available(child)
                } else {
                    subtree.tile_available(child)
                };
                if available {
                    mask |= bit;
                }
            }
            Ok(NetworkTerrainData::Cesium3DTiles(Cesium3DTilesTerrainData::new(Cesium3DTilesTerrainDataOptions {
                gltf,
                minimum_height: min,
                maximum_height: max,
                bounding_sphere: BoundingSphere::new(Cartesian3::new(bs[0], bs[1], bs[2]), bs[3]),
                oriented_bounding_box: obb,
                horizon_occlusion_point: Cartesian3::new(h[0], h[1], h[2]),
                skirt_height: self.get_level_maximum_geometric_error(l) * 5.,
                request_vertex_normals: self.options.request_vertex_normals,
                child_tile_mask: mask,
                credits: self.resource.options().credits.clone(),
                water_mask: water,
            })))
        }))
    }
    fn get_tile_data_available(&self, x: u32, y: u32, l: u32) -> Option<bool> {
        let (root, c) = Self::coordinates(x, y, l);
        let n = self.tilesets[usize::from(root != 0)].subtree_levels;
        let coord = c.subtree(n);
        let cache = self.cache.borrow();
        if let Some(s) = cache.get(&(root, coord)) {
            return Some(s.tile_available(c));
        }
        if coord.level == 0 {
            return (c.level == 0).then_some(true);
        }
        let parent = cache.get(&(root, coord.parent(n)))?;
        if !parent.child_available(coord) {
            Some(false)
        } else {
            (c.level == coord.level).then_some(true)
        }
    }
    fn load_tile_data_availability(&self, _: u32, _: u32, _: u32) -> Option<Self::AvailabilityFuture<'_>> {
        None
    }
}
