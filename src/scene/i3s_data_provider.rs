//! Port of CesiumJS `Scene/I3SDataProvider`.

use crate::core::arcgis_tiled_elevation_terrain_provider::ArcGISTiledElevationTerrainProvider;
use crate::core::cartesian3::Cartesian3;
use crate::core::cartographic::Cartographic;
use crate::core::rectangle::Rectangle;
use crate::core::resource::Resource;
use crate::core::resource_client::ResourceClient;
use crate::core::runtime_error::RuntimeError;
use crate::scene::cesium_3d_tileset::{TilesetOptions, TilesetPassState};
use crate::scene::i3s_layer::{I3SLayer, LayerParent};
use crate::scene::i3s_statistics::I3SStatistics;
use crate::scene::i3s_sublayer::I3SSublayer;
use crate::scene::scene::{Result, ScenePrimitive};
use crate::scene::tileset_content::RenderableTileFactory;
use crate::core::network_terrain::NetworkTerrainData;
use crate::workers::decode_i3s::I3sGeoid;
use crate::core::query::QueryValue;
use crate::core::terrain_provider::TerrainProvider;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Port of `I3SDataProvider.ConstructorOptions`.
#[derive(Clone, Default)]
pub struct I3SDataProviderOptions {
    pub name: Option<String>,
    pub show: Option<bool>,
    pub geoid_tiled_terrain_provider: Option<Rc<ArcGISTiledElevationTerrainProvider>>,
    pub show_features: Option<bool>,
    pub adjust_material_alpha_mode: Option<bool>,
    pub apply_symbology: Option<bool>,
    pub calculate_normals: Option<bool>,
    pub cesium3d_tileset_options: Option<TilesetOptions>,
    pub content_factory: Option<Rc<dyn RenderableTileFactory>>,
}

/// A geoid heightmap the I3S transcoder samples when placing bounds.
#[derive(Clone, Debug)]
pub struct I3sGeoidHeightMap {
    pub native_extent: Rectangle,
    pub width: usize,
    pub height: usize,
    pub scale: f64,
    pub offset: f64,
    pub buffer: Vec<f64>,
}

impl I3sGeoidHeightMap {
    /// Samples the geoid at `(sample_x, sample_y)` in native coordinates:
    /// bilinear interpolation of `buffer` with a vertical flip, scaled by
    /// `scale` and offset by `offset`.
    pub fn sample(&self, sample_x: f64, sample_y: f64) -> f64 {
        let extent = self.native_extent;
        let mut x = ((sample_x - extent.west) / (extent.east - extent.west)) * (self.width as f64 - 1.0);
        let mut y = ((sample_y - extent.south) / (extent.north - extent.south)) * (self.height as f64 - 1.0);
        let xi = x.floor();
        let mut yi = y.floor();
        x -= xi;
        y -= yi;
        let x_next = if xi as usize >= self.width { xi } else { xi + 1.0 };
        let mut y_next = if yi as usize >= self.height { yi } else { yi + 1.0 };
        yi = self.height as f64 - 1.0 - yi;
        y_next = self.height as f64 - 1.0 - y_next;
        let h00 = sample_map(xi, yi, self.width as f64, &self.buffer);
        let h10 = sample_map(x_next, yi, self.width as f64, &self.buffer);
        let h01 = sample_map(xi, y_next, self.width as f64, &self.buffer);
        let h11 = sample_map(x_next, y_next, self.width as f64, &self.buffer);
        bilinear_interpolate(x, y, h00, h10, h01, h11) * self.scale + self.offset
    }

    /// The projected position of the geoid point in native coordinates.
    pub fn project(&self, cartographic: &Cartographic) -> Cartesian3 {
        Cartesian3::new(cartographic.longitude, cartographic.latitude, cartographic.height)
    }
}

/// JS array read on a map: missing or out-of-range indexes are `NaN`.
fn sample_map(u: f64, v: f64, width: f64, data: &[f64]) -> f64 {
    let address = u + v * width;
    if address.is_nan() {
        return f64::NAN;
    }
    data.get(address as usize).copied().unwrap_or(f64::NAN)
}

/// `bilinearInterpolate`.
fn bilinear_interpolate(tx: f64, ty: f64, h00: f64, h10: f64, h01: f64, h11: f64) -> f64 {
    let a = h00 * (1.0 - tx) + h10 * tx;
    let b = h01 * (1.0 - tx) + h11 * tx;
    a * (1.0 - ty) + b * ty
}

impl From<I3sGeoidHeightMap> for I3sGeoid {
    fn from(height_map: I3sGeoidHeightMap) -> Self {
        Self {
            west: height_map.native_extent.west,
            south: height_map.native_extent.south,
            east: height_map.native_extent.east,
            north: height_map.native_extent.north,
            width: height_map.width as f64,
            height: height_map.height as f64,
            buffer: height_map.buffer,
            scale: height_map.scale,
            offset: height_map.offset,
            web_mercator: false,
            web_mercator_radii: None,
        }
    }
}

/// Port of `I3SDataProvider`: the main public class for I3S support.
pub struct I3SDataProvider {
    name: Option<String>,
    show: Cell<bool>,
    geoid_tiled_terrain_provider: Option<Rc<ArcGISTiledElevationTerrainProvider>>,
    show_features: Cell<bool>,
    adjust_material_alpha_mode: Cell<bool>,
    apply_symbology: Cell<bool>,
    calculate_normals: Cell<bool>,
    cesium3d_tileset_options: Option<TilesetOptions>,
    content_factory: Option<Rc<dyn RenderableTileFactory>>,
    options: I3SDataProviderOptions,
    resource: Resource,
    data: Option<Value>,
    layers: RefCell<Vec<Rc<I3SLayer>>>,
    sublayers: RefCell<Vec<Rc<I3SSublayer>>>,
    attribute_statistics: RefCell<Vec<I3SStatistics>>,
    layers_extent: RefCell<Vec<Rectangle>>,
    extent: RefCell<Option<Rectangle>>,
    geoid_data_list: RefCell<Option<Vec<I3sGeoidHeightMap>>>,
    client: Option<ResourceClient>,
}

impl I3SDataProvider {
    /// Port of `new I3SDataProvider(options)` (Cesium `fromUrl` constructs it).
    pub fn new(options: I3SDataProviderOptions) -> Self {
        Self {
            name: options.name.clone(),
            show: Cell::new(options.show.unwrap_or(true)),
            geoid_tiled_terrain_provider: options.geoid_tiled_terrain_provider.clone(),
            show_features: Cell::new(options.show_features.unwrap_or(false)),
            adjust_material_alpha_mode: Cell::new(options.adjust_material_alpha_mode.unwrap_or(false)),
            apply_symbology: Cell::new(options.apply_symbology.unwrap_or(false)),
            calculate_normals: Cell::new(options.calculate_normals.unwrap_or(false)),
            cesium3d_tileset_options: options.cesium3d_tileset_options.clone(),
            content_factory: options.content_factory.clone(),
            options,
            resource: Resource::new("").expect("I3S provider placeholder URI"),
            data: None,
            layers: RefCell::new(Vec::new()),
            sublayers: RefCell::new(Vec::new()),
            attribute_statistics: RefCell::new(Vec::new()),
            layers_extent: RefCell::new(Vec::new()),
            extent: RefCell::new(None),
            geoid_data_list: RefCell::new(None),
            client: None,
        }
    }

    /// Port of the `name` getter.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Port of the `show` getter and setter.
    pub fn show(&self) -> bool {
        self.show.get()
    }

    pub fn set_show(&self, value: bool) {
        if self.show.get() != value {
            self.show.set(value);
            for layer in self.layers.borrow().iter() {
                layer.update_visibility();
            }
        }
    }

    /// Port of the `geoidTiledTerrainProvider` getter.
    pub fn geoid_tiled_terrain_provider(&self) -> Option<&Rc<ArcGISTiledElevationTerrainProvider>> {
        self.geoid_tiled_terrain_provider.as_ref()
    }

    /// Port of the `layers` getter.
    pub fn layers(&self) -> Vec<Rc<I3SLayer>> {
        self.layers.borrow().clone()
    }

    /// Port of the `sublayers` getter.
    pub fn sublayers(&self) -> Vec<Rc<I3SSublayer>> {
        self.sublayers.borrow().clone()
    }

    /// The `I3SStatistics` documents loaded for this dataset.
    pub fn attribute_statistics(&self) -> Vec<I3SStatistics> {
        self.attribute_statistics.borrow().clone()
    }

    /// Port of the `data` getter.
    pub fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }

    /// Port of the `extent` getter: the union of layer extents.
    pub fn extent(&self) -> Option<Rectangle> {
        *self.extent.borrow()
    }

    /// Port of the `resource` getter.
    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    /// Port of the `showFeatures` getter.
    pub fn show_features(&self) -> bool {
        self.show_features.get()
    }

    /// Port of the `adjustMaterialAlphaMode` getter.
    pub fn adjust_material_alpha_mode(&self) -> bool {
        self.adjust_material_alpha_mode.get()
    }

    /// Port of the `applySymbology` getter.
    pub fn apply_symbology(&self) -> bool {
        self.apply_symbology.get()
    }

    /// Port of the `calculateNormals` getter.
    pub fn calculate_normals(&self) -> bool {
        self.calculate_normals.get()
    }

    /// Optional renderable tile content factory forwarded to each layer's tileset loader.
    pub fn content_factory(&self) -> Option<Rc<dyn RenderableTileFactory>> {
        self.content_factory.clone()
    }

    /// Port of `I3SDataProvider.fromUrl`.
    pub async fn from_url(resource: Resource, options: I3SDataProviderOptions, client: &ResourceClient) -> Result<Rc<I3SDataProvider>> {
        // Set a query parameter for json to avoid failure on html pages.
        resource.set_query_parameters(&vec![("f".to_string(), Some(QueryValue::Single("pjson".to_string())))], true);
        let data = I3SDataProvider::load_json(&resource, client).await?;
        let mut provider = I3SDataProvider::new(options);
        provider.resource = resource;
        provider.data = Some(data.clone());
        provider.client = Some(client.clone());
        let provider_rc = Rc::new(provider);
        let layers_data = if let Some(layers) = data.get("layers").and_then(Value::as_array) {
            layers.clone()
        } else {
            vec![data.clone()]
        };
        for layer_data in layers_data {
            Self::add_layers(&provider_rc, &layer_data, client).await?;
        }
        provider_rc.compute_extent();
        // Start loading all of the tiles.
        for layer in provider_rc.layers() {
            layer.load(provider_rc.cesium3d_tileset_options.clone(), client).await?;
        }
        Ok(provider_rc)
    }

    /// Port of `addLayers` (Cesium's private helper).
    async fn add_layers(provider_rc: &Rc<I3SDataProvider>, data: &Value, client: &ResourceClient) -> Result<()> {
        let building_layer_url = match data.get("layerType").and_then(Value::as_str) {
            Some("Building") => {
                // The Building Scene Layer requires features to be shown to support
                // filtering; it enables transparency and symbology and calculates
                // flat normals by default.
                if provider_rc.options.show_features.is_none() {
                    provider_rc.show_features.set(true);
                }
                if provider_rc.options.adjust_material_alpha_mode.is_none() {
                    provider_rc.adjust_material_alpha_mode.set(true);
                }
                if provider_rc.options.apply_symbology.is_none() {
                    provider_rc.apply_symbology.set(true);
                }
                if provider_rc.options.calculate_normals.is_none() {
                    provider_rc.calculate_normals.set(true);
                }
                Some(build_layer_url(&provider_rc.resource().clone(), data.get("id").and_then(Value::as_u64).unwrap_or_default()))
            }
            Some("3DObject") | Some("IntegratedMesh") => {
                if !provider_rc.calculate_normals.get() && data.get("textureSetDefinitions").is_none() {
                    // I3S layers without textures calculate flat normals by default.
                    provider_rc.calculate_normals.set(true);
                }
                let layer = I3SLayer::new(provider_rc, LayerParent::Provider(Rc::downgrade(provider_rc)), data.clone(), provider_rc.client())?;
                if let Some(extent) = layer.extent() {
                    provider_rc.layers_extent.borrow_mut().push(*extent);
                }
                provider_rc.layers.borrow_mut().push(layer);
                None
            }
            _ => {
                eprintln!(
                    "{} layer {} is skipped as not supported.",
                    data.get("layerType").and_then(Value::as_str).unwrap_or("undefined"),
                    data.get("name").and_then(Value::as_str).unwrap_or("undefined")
                );
                None
            }
        };
        if let Some(building_layer_url) = building_layer_url {
            if let Some(sublayers_data) = data.get("sublayers").and_then(Value::as_array) {
                for sublayer_data in sublayers_data.clone() {
                    let sublayer =
                        I3SSublayer::from_data(provider_rc, &building_layer_url, sublayer_data, LayerParent::Provider(Rc::downgrade(provider_rc)), client)
                            .await?;
                    provider_rc.sublayers.borrow_mut().push(sublayer.clone());
                    provider_rc.layers.borrow_mut().extend(sublayer.i3s_layers());
                }
            }
            if let Some(statistics_href) = data.get("statisticsHRef").and_then(Value::as_str) {
                let uri = format!("{building_layer_url}/{statistics_href}");
                let mut statistics = I3SStatistics::new(provider_rc, &uri);
                statistics.load(client).await?;
                provider_rc.attribute_statistics.borrow_mut().push(statistics);
            }
            if let Some(full_extent) = data.get("fullExtent") {
                let extent = Rectangle::from_degrees(
                    full_extent.get("xmin").and_then(Value::as_f64).unwrap_or_default(),
                    full_extent.get("ymin").and_then(Value::as_f64).unwrap_or_default(),
                    full_extent.get("xmax").and_then(Value::as_f64).unwrap_or_default(),
                    full_extent.get("ymax").and_then(Value::as_f64).unwrap_or_default(),
                );
                provider_rc.layers_extent.borrow_mut().push(extent);
            }
        }
        Ok(())
    }

    /// Port of `I3SDataProvider.loadJson`.
    pub async fn load_json(resource: &Resource, client: &ResourceClient) -> Result<Value> {
        let data = client
            .fetch_json(resource)
            .ok_or_else(|| RuntimeError::new("I3S request was throttled"))?
            .await
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        if let Some(error) = data.get("error") {
            eprintln!("Failed to fetch I3S {}", resource.url());
            if let Some(message) = error.get("message") {
                eprintln!("{}", message.as_str().unwrap_or_default());
            }
            if let Some(details) = error.get("details").and_then(Value::as_array) {
                for detail in details {
                    eprintln!("{detail}");
                }
            }
            return Err(RuntimeError::new(error.to_string()));
        }
        Ok(data)
    }

    /// Port of `I3SDataProvider.prototype._loadBinary`.
    pub async fn load_binary(resource: &Resource, client: &ResourceClient) -> Result<Vec<u8>> {
        let buffer = client
            .fetch_array_buffer(resource)
            .ok_or_else(|| RuntimeError::new("I3S request was throttled"))?
            .await
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        if let Some(first) = buffer.first() {
            if *first == b'{' {
                let str = String::from_utf8_lossy(&buffer);
                if str.contains("404") {
                    return Err(RuntimeError::new(format!("Failed to load binary: {}", resource.url())));
                }
            }
        }
        Ok(buffer)
    }

    /// Port of `I3SDataProvider.prototype._binarizeGltf`: the binary glTF
    /// (GLB) document of an in-memory glTF (4-byte aligned JSON chunk).
    pub fn binarize_gltf(raw_gltf: &Value) -> Vec<u8> {
        let mut json_bytes = serde_json::to_vec(raw_gltf).unwrap_or_default();
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }
        let length = 20u32 + json_bytes.len() as u32;
        let mut buffer = Vec::with_capacity(length as usize);
        buffer.extend_from_slice(b"glTF");
        buffer.extend_from_slice(&2u32.to_le_bytes());
        buffer.extend_from_slice(&length.to_le_bytes());
        buffer.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        buffer.extend_from_slice(&0x4E4F534Au32.to_le_bytes()); // JSON chunk type
        buffer.extend_from_slice(&json_bytes);
        buffer
    }

    /// Port of `I3SDataProvider.prototype.loadGeoidData`.
    pub async fn load_geoid_data(&self) {
        if self.geoid_data_list.borrow().is_some() {
            return;
        }
        let Some(geoid_terrain_provider) = self.geoid_tiled_terrain_provider.clone() else { return };
        let height_maps = get_covered_tiles(&geoid_terrain_provider, self.extent()).await;
        *self.geoid_data_list.borrow_mut() = Some(height_maps);
    }

    /// Port of `I3SDataProvider.prototype._computeExtent`.
    fn compute_extent(&self) {
        let mut rectangle: Option<Rectangle> = None;
        for layer_extent in self.layers_extent.borrow().iter() {
            rectangle = Some(match rectangle {
                Some(r) => Rectangle::union(&r, layer_extent),
                None => *layer_extent,
            });
        }
        *self.extent.borrow_mut() = rectangle;
    }

    /// Port of `I3SDataProvider.prototype.getAttributeNames`.
    pub fn get_attribute_names(&self) -> Vec<String> {
        self.attribute_statistics.borrow().iter().flat_map(|s| s.names()).collect()
    }

    /// Port of `I3SDataProvider.prototype.getAttributeValues`.
    pub fn get_attribute_values(&self, name: &str) -> Vec<Value> {
        for statistics in self.attribute_statistics.borrow().iter() {
            if let Some(values) = statistics.get_values(name) {
                return values;
            }
        }
        Vec::new()
    }

    /// Returns the loaded geoid height maps (the provider's `_geoidDataList`).
    pub fn geoid_data_list(&self) -> Option<Vec<I3sGeoidHeightMap>> {
        self.geoid_data_list.borrow().clone()
    }

    /// The shared [`ResourceClient`] used by this provider.
    pub fn client(&self) -> Option<ResourceClient> {
        self.client.clone()
    }

    /// Port of `I3SDataProvider.prototype.filterByAttributes`.
    pub async fn filter_by_attributes(&self, filters: Option<Vec<Value>>, client: &ResourceClient) -> Result<()> {
        for layer in self.layers() {
            layer.filter_by_attributes(filters.clone(), client).await?;
        }
        Ok(())
    }
}

impl ScenePrimitive for I3SDataProvider {
    fn update(&mut self, _frame: &mut crate::scene::FrameState) -> Result<()> {
        for layer in self.layers() {
            if let Some(tileset) = layer.tileset_mut().as_mut() {
                tileset.update(_frame)?;
            }
        }
        Ok(())
    }

    fn pre_passes_update(&mut self, frame: &mut crate::scene::FrameState) -> Result<()> {
        for layer in self.layers() {
            if let Some(tileset) = layer.tileset_mut().as_mut() {
                tileset.pre_passes_update(frame)?;
            }
        }
        Ok(())
    }

    fn post_passes_update(&mut self, frame: &mut crate::scene::FrameState) -> Result<()> {
        for layer in self.layers() {
            if let Some(tileset) = layer.tileset_mut().as_mut() {
                tileset.post_passes_update(frame)?;
            }
        }
        Ok(())
    }

    fn update_for_pass(&mut self, frame: &mut crate::scene::FrameState, pass: crate::scene::Cesium3DTilePass) -> Result<()> {
        for layer in self.layers() {
            if let Some(tileset) = layer.tileset_mut().as_mut() {
                tileset.update_for_pass(frame, &mut TilesetPassState::new(pass))?;
            }
        }
        Ok(())
    }

    fn destroy(&mut self) -> Result<()> {
        for layer in self.layers() {
            if let Some(tileset) = layer.tileset_mut().as_mut() {
                tileset.destroy()?;
            }
        }
        self.layers.borrow_mut().clear();
        self.sublayers.borrow_mut().clear();
        Ok(())
    }
}

/// `buildLayerUrl`: the tileset URL of a layer in a Building Scene Layer.
fn build_layer_url(provider_resource: &Resource, layer_id: u64) -> String {
    let data_provider_url = provider_resource.get_url_component(false, false);
    if is_layers_url(&data_provider_url) {
        data_provider_url.trim_end_matches('/').to_string()
    } else {
        let base = if data_provider_url.ends_with('/') { data_provider_url } else { format!("{data_provider_url}/") };
        format!("{base}layers/{layer_id}")
    }
}

/// `/layers\/\d/` test on a URL.
fn is_layers_url(url: &str) -> bool {
    let mut rest = url;
    while let Some(idx) = rest.find("layers/") {
        rest = &rest[idx + "layers/".len()..];
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// Port of `getCoveredTiles`: all the tiles the geoid provider covers.
async fn get_covered_tiles(terrain_provider: &ArcGISTiledElevationTerrainProvider, extent: Option<Rectangle>) -> Vec<I3sGeoidHeightMap> {
    let Some(extent) = extent else { return Vec::new() };
    let tiling_scheme = terrain_provider.tiling_scheme();
    let top_left_corner = Cartographic::from_radians(extent.west, extent.north, 0.0);
    let bottom_right_corner = Cartographic::from_radians(extent.east, extent.south, 0.0);
    let Some(min_corner_xy) = tiling_scheme.position_to_tile_xy(&top_left_corner, terrain_provider.lod_count) else { return Vec::new() };
    let Some(max_corner_xy) = tiling_scheme.position_to_tile_xy(&bottom_right_corner, terrain_provider.lod_count) else { return Vec::new() };
    let mut height_maps = Vec::new();
    let mut x = min_corner_xy.x;
    while x <= max_corner_xy.x {
        let mut y = min_corner_xy.y;
        while y <= max_corner_xy.y {
            let Some(future) = terrain_provider.request_tile_geometry(x as u32, y as u32, terrain_provider.lod_count, None) else {
                y += 1.0;
                continue;
            };
            let Ok(network_terrain_data) = future.await else {
                y += 1.0;
                continue;
            };
            let NetworkTerrainData::Heightmap { data: height_map, .. } = network_terrain_data else {
                y += 1.0;
                continue;
            };
            let geoid_buffer: Vec<f64> = height_map.buffer().map(|b| b.to_f64_vec()).unwrap_or_default();
            height_maps.push(I3sGeoidHeightMap {
                native_extent: tiling_scheme.tile_xy_to_native_rectangle(x, y, terrain_provider.lod_count),
                width: height_map.width(),
                height: height_map.height(),
                scale: height_map.structure().samples.height_scale,
                offset: height_map.structure().samples.height_offset,
                buffer: geoid_buffer,
            });
            y += 1.0;
        }
        x += 1.0;
    }
    height_maps
}

