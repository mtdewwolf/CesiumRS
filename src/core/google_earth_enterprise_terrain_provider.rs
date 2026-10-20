//! GEE terrain requests, parent packet fan-out and expiring one-shot tile cache.
use super::*;
use super::{
    heightmap_terrain_data::HeightmapTerrainDataOptions,
    network_terrain::{fetch_bytes, TerrainFuture},
    resource::DerivedResourceOptions,
};
use futures_util::{
    future::{LocalBoxFuture, Shared},
    FutureExt,
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::{Future, Ready},
    pin::Pin,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context, Waker},
};
type Pending = Shared<LocalBoxFuture<'static, Result<(), RuntimeError>>>;
#[derive(Default)]
pub struct GeeTerrainOptions {
    pub ellipsoid: Option<Ellipsoid>,
    pub credit: Option<Credit>,
    pub now: Option<Rc<dyn Fn() -> f64>>,
}
struct State {
    metadata: Rc<GoogleEarthEnterpriseMetadata>,
    client: ResourceClient,
    scheme: GeographicTilingScheme,
    credit: Option<Credit>,
    cache: RefCell<HashMap<String, (Vec<u8>, f64)>>,
    pending: RefCell<HashMap<String, (Pending, Option<RequestHandle<HttpResponse>>)>>,
    background: RefCell<Vec<TerrainFuture<'static, GoogleEarthEnterpriseTileInformation>>>,
    last_tidy: Cell<f64>,
    now: Rc<dyn Fn() -> f64>,
    error: Arc<Mutex<Event<RuntimeError>>>,
}
#[derive(Clone)]
pub struct GoogleEarthEnterpriseTerrainProvider {
    state: Rc<State>,
}
impl GoogleEarthEnterpriseTerrainProvider {
    pub fn from_metadata(
        metadata: Rc<GoogleEarthEnterpriseMetadata>,
        o: GeeTerrainOptions,
        client: ResourceClient,
    ) -> Result<Self, RuntimeError> {
        if !metadata.terrain_present.get() {
            return Err(RuntimeError::new(format!("The server {} doesn't have terrain", metadata.resource.url())));
        }
        let now = o.now.unwrap_or_else(|| Rc::new(default_now));
        let timestamp = now();
        Ok(Self {
            state: Rc::new(State {
                metadata,
                client,
                scheme: GeographicTilingScheme::new(
                    o.ellipsoid.unwrap_or(Ellipsoid::WGS84),
                    Rectangle::new(
                        -std::f64::consts::PI,
                        -std::f64::consts::PI,
                        std::f64::consts::PI,
                        std::f64::consts::PI,
                    ),
                    2,
                    2,
                ),
                credit: o.credit,
                cache: Default::default(),
                pending: Default::default(),
                background: Default::default(),
                last_tidy: Cell::new(timestamp),
                now,
                error: Arc::new(Mutex::new(Event::new())),
            }),
        })
    }
    pub fn metadata(&self) -> &Rc<GoogleEarthEnterpriseMetadata> {
        &self.state.metadata
    }
    pub fn terrain_resource(
        &self,
        q: &str,
        version: u16,
        request: Option<RequestHandle<HttpResponse>>,
    ) -> Option<Resource> {
        self.state.metadata.resource.get_derived_resource(DerivedResourceOptions {
            url: Some(format!("flatfile?f1c-0{q}-t.{}", version.max(1))),
            request,
            ..Default::default()
        })
    }
    pub fn seed_cache(&self, key: String, buffer: Vec<u8>) {
        self.state.cache.borrow_mut().insert(key, (buffer, (self.state.now)()));
    }
}
fn default_now() -> f64 {
    #[cfg(all(target_arch = "wasm32", feature = "browser"))]
    {
        js_sys::Date::now()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        super::clock::get_timestamp()
    }
    #[cfg(all(target_arch = "wasm32", not(feature = "browser")))]
    {
        0.
    }
}
impl State {
    fn cached(&self, q: &str) -> Option<NetworkTerrainData> {
        let (buffer, _) = self.cache.borrow_mut().remove(q)?;
        let info = self.metadata.get_tile_information_from_quad_key(q).flatten()?;
        let mut mask = info.get_child_bitmask();
        if info.terrain_state == Some(3) {
            mask = 0;
            for i in 0..4 {
                if self
                    .metadata
                    .get_tile_information_from_quad_key(&format!("{q}{i}"))
                    .flatten()
                    .is_some_and(|i| i.has_terrain())
                {
                    mask |= 1 << i;
                }
            }
        }
        let credits = self.metadata.providers.borrow().get(&(info.terrain_provider as u32)).cloned().map(|c| vec![c]);
        Some(NetworkTerrainData::GoogleEarthEnterprise(GoogleEarthEnterpriseTerrainData::new(
            buffer,
            self.metadata.negative_altitude_exponent_bias.get(),
            self.metadata.negative_altitude_threshold.get(),
            mask,
            credits,
        )))
    }
    fn tidy(&self) {
        let now = (self.now)();
        if now - self.last_tidy.get() > 10000. {
            self.cache.borrow_mut().retain(|_, (_, t)| now - *t <= 10000.);
            self.last_tidy.set(now);
        }
    }
    fn pump(&self) {
        self.metadata.pump();
        let mut jobs = std::mem::take(&mut *self.background.borrow_mut());
        jobs.retain_mut(|f| f.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending());
        self.background.borrow_mut().extend(jobs);
        let jobs = self.pending.borrow().values().map(|(f, _)| f.clone()).collect::<Vec<_>>();
        for mut f in jobs {
            let _ = Pin::new(&mut f).poll(&mut Context::from_waker(Waker::noop()));
        }
    }
    fn set_state(&self, q: &str, state: i32) {
        if let Some(Some(info)) = self.metadata.tile_info.borrow_mut().get_mut(q) {
            info.terrain_state = Some(state);
        }
    }
}
impl TerrainProvider for GoogleEarthEnterpriseTerrainProvider {
    type Error = RuntimeError;
    type Credit = Credit;
    type TerrainData = NetworkTerrainData;
    type Request = RequestHandle<HttpResponse>;
    type GeometryFuture<'a> = TerrainFuture<'a, NetworkTerrainData>;
    type AvailabilityFuture<'a> = Ready<Result<(), RuntimeError>>;
    fn error_event(&self) -> Arc<Mutex<Event<RuntimeError>>> {
        self.state.error.clone()
    }
    fn credit(&self) -> Option<&Credit> {
        self.state.credit.as_ref()
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        &self.state.scheme
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
    fn get_level_maximum_geometric_error(&self, l: u32) -> f64 {
        40075.16 / 1i32.wrapping_shl(l) as f64
    }
    fn request_tile_geometry(
        &self,
        x: u32,
        y: u32,
        l: u32,
        request: Option<Self::Request>,
    ) -> Option<Self::GeometryFuture<'_>> {
        let state = &self.state;
        let m = &state.metadata;
        let target = GoogleEarthEnterpriseMetadata::tile_xy_to_quad_key(x, y, l);
        let nonexistent = || {
            Some(Box::pin(async { Err(RuntimeError::new("Terrain tile doesn't exist")) })
                as TerrainFuture<'_, NetworkTerrainData>)
        };
        let Some(info) = m.get_tile_information_from_quad_key(&target).flatten() else { return nonexistent() };
        if info.terrain_state.is_none() {
            state.set_state(&target, 0);
        }
        if let Some(data) = state.cached(&target) {
            return Some(Box::pin(async { Ok(data) }));
        }
        state.tidy();
        if !info.ancestor_has_terrain {
            let data =
                HeightmapTerrainData::new(HeightmapTerrainDataOptions::new(TypedArray::Uint8(vec![0; 256]), 16, 16))
                    .unwrap();
            return Some(Box::pin(async { Ok(NetworkTerrainData::heightmap(data)) }));
        }
        if info.terrain_state == Some(1) {
            return nonexistent();
        }
        let mut q = target.clone();
        let version = match info.terrain_state.unwrap_or(0) {
            2 => Some(info.terrain_version),
            3 => {
                q.pop();
                m.get_tile_information_from_quad_key(&q).flatten().map(|i| i.terrain_version)
            }
            _ => {
                if info.has_terrain() {
                    Some(info.terrain_version)
                } else {
                    q.pop();
                    m.get_tile_information_from_quad_key(&q)
                        .flatten()
                        .filter(|i| i.has_terrain())
                        .map(|i| i.terrain_version)
                }
            }
        };
        let Some(version) = version else { return nonexistent() };
        let existing = state.pending.borrow().get(&q).cloned();
        let (shared, shared_request) = if let Some(p) = existing {
            p
        } else {
            let r = self.terrain_resource(&q, version, request.clone())?;
            let fetch = fetch_bytes(&state.client, &r)?;
            let weak = Rc::downgrade(state);
            let key = q.clone();
            let future = async move {
                let result = fetch.await.and_then(|b| {
                    let s = weak.upgrade().ok_or_else(|| RuntimeError::new("Terrain provider was dropped."))?;
                    let bytes = super::gee_packet::inflate_packet(&b, &s.metadata.key.borrow())?;
                    super::gee_packet::terrain_meshes(&bytes)
                });
                let s = weak.upgrade().ok_or_else(|| RuntimeError::new("Terrain provider was dropped."))?;
                s.pending.borrow_mut().remove(&key);
                let meshes = result?;
                let mut tiles = s.metadata.tile_info.borrow_mut();
                let requested = tiles
                    .get_mut(&key)
                    .and_then(Option::as_mut)
                    .ok_or_else(|| RuntimeError::new("Terrain metadata is missing."))?;
                requested.terrain_state = Some(2);
                let provider = requested.terrain_provider;
                let now = (s.now)();
                let mut cache = s.cache.borrow_mut();
                for (i, mesh) in meshes.into_iter().enumerate() {
                    let child = if i == 0 { key.clone() } else { format!("{key}{}", i - 1) };
                    if i == 0 {
                        cache.insert(child, (mesh, now));
                    } else if let Some(Some(info)) = tiles.get_mut(&child) {
                        info.terrain_state = Some(3);
                        if info.terrain_provider == 0 {
                            info.terrain_provider = provider;
                        }
                        cache.insert(child, (mesh, now));
                    }
                }
                Ok(())
            }
            .boxed_local()
            .shared();
            state.pending.borrow_mut().insert(q, (future.clone(), request.clone()));
            (future, request.clone())
        };
        let weak = Rc::downgrade(state);
        Some(Box::pin(async move {
            let result = shared.await;
            let state = weak.upgrade().ok_or_else(|| RuntimeError::new("Terrain provider was dropped."))?;
            match result.and_then(|_| state.cached(&target).ok_or_else(|| RuntimeError::new("Failed to load terrain.")))
            {
                Ok(d) => Ok(d),
                Err(e) => {
                    if shared_request.as_ref().is_some_and(|r| r.borrow().state == RequestState::Cancelled) {
                        if let Some(r) = request {
                            r.borrow_mut().state = RequestState::Cancelled;
                        }
                    } else {
                        state.set_state(&target, 1);
                    }
                    Err(e)
                }
            }
        }))
    }
    fn get_tile_data_available(&self, x: u32, y: u32, l: u32) -> Option<bool> {
        let s = &self.state;
        s.pump();
        let q = GoogleEarthEnterpriseMetadata::tile_xy_to_quad_key(x, y, l);
        match s.metadata.get_tile_information_from_quad_key(&q) {
            Some(None) => Some(false),
            Some(Some(info)) => {
                if !info.ancestor_has_terrain {
                    return Some(true);
                }
                if info.terrain_state == Some(1) {
                    return Some(false);
                }
                if info.terrain_state.is_none() || info.terrain_state == Some(0) {
                    s.set_state(&q, 0);
                    if !info.has_terrain() {
                        return Some(
                            s.metadata
                                .get_tile_information_from_quad_key(&q[..q.len() - 1])
                                .flatten()
                                .is_some_and(|i| i.has_terrain()),
                        );
                    }
                }
                Some(true)
            }
            None => {
                if s.metadata.is_valid(&q) {
                    let request = Request::<HttpResponse>::new(RequestOptions {
                        throttle_by_server: true,
                        request_type: RequestType::Terrain,
                        ..Default::default()
                    })
                    .into_handle();
                    if let Some(f) = s.metadata.populate_subtree(x, y, l, Some(request)) {
                        s.background.borrow_mut().push(f);
                        s.pump();
                    }
                }
                Some(false)
            }
        }
    }
    fn load_tile_data_availability(&self, _: u32, _: u32, _: u32) -> Option<Self::AvailabilityFuture<'_>> {
        None
    }
}
