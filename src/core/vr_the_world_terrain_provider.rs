//! VR-TheWorld XML metadata and RGB-encoded TIFF heightmaps through ImageHost.
use super::heightmap_terrain_data::{HeightmapTerrainDataOptions, HeightmapTerrainStructure};
use super::heightmap_tessellator::HeightmapStructure;
use super::network_terrain::TerrainFuture;
use super::resource::DerivedResourceOptions;
use super::*;
use std::{
    future::Ready,
    rc::Rc,
    sync::{Arc, Mutex},
};
#[derive(Default)]
pub struct VRTheWorldOptions {
    pub ellipsoid: Option<Ellipsoid>,
    pub credit: Option<Credit>,
}
pub struct VRTheWorldTerrainProvider<H: ImageHost> {
    pub resource: Resource,
    pub width: usize,
    pub height: usize,
    pub rectangles: Vec<(Rectangle, u32)>,
    scheme: GeographicTilingScheme,
    credit: Option<Credit>,
    client: ResourceClient,
    host: Rc<H>,
    level_zero_error: f64,
    error: Arc<Mutex<Event<RuntimeError>>>,
}
impl<H: ImageHost + 'static> VRTheWorldTerrainProvider<H> {
    pub async fn from_url(
        resource: Resource,
        options: VRTheWorldOptions,
        client: ResourceClient,
        host: Rc<H>,
    ) -> Result<Self, RuntimeError> {
        let f = client
            .fetch_xml(&resource)
            .ok_or_else(|| RuntimeError::new("VR-TheWorld metadata request was throttled."))?;
        let xml = f
            .await
            .map_err(|e| RuntimeError::new(format!("An error occurred while accessing {}: {e}", resource.url())))?;
        Self::from_xml(resource, xml.source(), options, client, host)
    }
    pub fn from_xml(
        resource: Resource,
        xml: &str,
        o: VRTheWorldOptions,
        client: ResourceClient,
        host: Rc<H>,
    ) -> Result<Self, RuntimeError> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| RuntimeError::new(e.to_string()))?;
        let srs = doc.descendants().find(|n| n.has_tag_name("SRS")).and_then(|n| n.text()).unwrap_or("");
        if srs != "EPSG:4326" {
            return Err(RuntimeError::new(format!("SRS {srs} is not supported")));
        }
        let format = doc
            .descendants()
            .find(|n| n.has_tag_name("TileFormat"))
            .ok_or_else(|| RuntimeError::new("TileFormat is required."))?;
        let integer = |v: Option<&str>| {
            v.and_then(|s| s.parse::<usize>().ok()).ok_or_else(|| RuntimeError::new("Invalid tile dimensions."))
        };
        let width = integer(format.attribute("width"))?;
        let height = integer(format.attribute("height"))?;
        let scheme = GeographicTilingScheme::new(o.ellipsoid.unwrap_or(Ellipsoid::WGS84), Rectangle::MAX_VALUE, 2, 1);
        let level_zero_error = super::terrain_provider::get_estimated_level_zero_geometric_error_for_a_heightmap(
            scheme.ellipsoid(),
            width.min(height) as f64,
            2.,
            None,
        );
        let mut rectangles = vec![];
        for n in doc.descendants().filter(|n| n.has_tag_name("DataExtent")) {
            let number = |k| n.attribute(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(f64::NAN);
            let level = integer(n.attribute("maxlevel"))? as u32;
            rectangles
                .push((Rectangle::from_degrees(number("minx"), number("miny"), number("maxx"), number("maxy")), level));
        }
        Ok(Self {
            resource,
            width,
            height,
            rectangles,
            scheme,
            credit: o.credit,
            client,
            host,
            level_zero_error,
            error: Arc::new(Mutex::new(Event::new())),
        })
    }
    pub fn structure(&self) -> HeightmapTerrainStructure {
        HeightmapTerrainStructure {
            samples: HeightmapStructure {
                height_scale: 0.001,
                height_offset: -1000.,
                elements_per_height: 3,
                stride: 4,
                element_multiplier: 256.,
                is_big_endian: true,
            },
            lowest_encoded_height: Some(0.),
            highest_encoded_height: Some(16777215.),
        }
    }
    pub fn child_mask(&self, x: u32, y: u32, level: u32) -> u8 {
        let parent = self.scheme.tile_xy_to_rectangle(x as f64, y as f64, level);
        let mut mask = 0;
        for (r, max) in &self.rectangles {
            if *max <= level || r.intersection(&parent).is_none() {
                continue;
            }
            for (dx, dy, bit) in [(0, 0, 4), (1, 0, 8), (0, 1, 1), (1, 1, 2)] {
                if self
                    .scheme
                    .tile_xy_to_rectangle((x * 2 + dx) as f64, (y * 2 + dy) as f64, level + 1)
                    .intersection(r)
                    .is_some()
                {
                    mask |= bit;
                }
            }
        }
        mask
    }
    pub fn tile_resource(
        &self,
        x: u32,
        y: u32,
        l: u32,
        request: Option<RequestHandle<HttpResponse>>,
    ) -> Option<Resource> {
        let y_tiles = self.scheme.get_number_of_y_tiles_at_level(l);
        self.resource.get_derived_resource(DerivedResourceOptions {
            url: Some(format!("{l}/{x}/{}.tif", y_tiles as i64 - y as i64 - 1)),
            query_parameters: vec![("cesium".into(), Some(super::query::QueryValue::Single("true".into())))],
            request,
            ..Default::default()
        })
    }
}
impl<H: ImageHost + 'static> TerrainProvider for VRTheWorldTerrainProvider<H> {
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
        &self.scheme
    }
    fn has_water_mask(&self) -> bool {
        false
    }
    fn has_vertex_normals(&self) -> bool {
        false
    }
    fn availability(&self) -> Option<&TileAvailability> {
        None
    }
    fn request_tile_geometry(
        &self,
        x: u32,
        y: u32,
        level: u32,
        request: Option<Self::Request>,
    ) -> Option<Self::GeometryFuture<'_>> {
        let r = self.tile_resource(x, y, level, request)?;
        let f = self.client.fetch_image(
            &r,
            self.host.clone(),
            FetchImageOptions { prefer_image_bitmap: true, ..Default::default() },
        )?;
        Some(Box::pin(async move {
            let image = f
                .await
                .map_err(super::network_terrain::net_error)?
                .ok_or_else(|| RuntimeError::new("Terrain image is empty."))?;
            let pixels = super::image_helpers::get_image_pixels(self.host.as_ref(), &image, None, None)?;
            let mut o = HeightmapTerrainDataOptions::new(TypedArray::Uint8(pixels), self.width, self.height);
            o.child_tile_mask = self.child_mask(x, y, level);
            o.structure = HeightmapTerrainStructure {
                samples: HeightmapStructure {
                    height_scale: 0.001,
                    height_offset: -1000.,
                    elements_per_height: 3,
                    stride: 4,
                    element_multiplier: 256.,
                    is_big_endian: true,
                },
                lowest_encoded_height: Some(0.),
                highest_encoded_height: Some(16777215.),
            };
            HeightmapTerrainData::new(o)
                .map(NetworkTerrainData::heightmap)
                .ok_or_else(|| RuntimeError::new("Invalid VR-TheWorld heightmap."))
        }))
    }
    fn get_level_maximum_geometric_error(&self, l: u32) -> f64 {
        self.level_zero_error / 1i32.wrapping_shl(l) as f64
    }
    fn get_tile_data_available(&self, _: u32, _: u32, _: u32) -> Option<bool> {
        None
    }
    fn load_tile_data_availability(&self, _: u32, _: u32, _: u32) -> Option<Self::AvailabilityFuture<'_>> {
        None
    }
}
