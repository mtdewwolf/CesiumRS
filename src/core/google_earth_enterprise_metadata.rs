//! Google Earth Enterprise dbRoot, encrypted quadtree packets and subtree sharing.
use super::*;
use super::{
    gee_packet as packet,
    network_terrain::{fetch_bytes, TerrainFuture},
    resource::DerivedResourceOptions,
};
use futures_util::{
    future::{LocalBoxFuture, Shared},
    FutureExt,
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Waker},
};
type Info = GoogleEarthEnterpriseTileInformation;
type Pending = Shared<LocalBoxFuture<'static, Result<(), RuntimeError>>>;
pub struct GoogleEarthEnterpriseMetadata {
    pub resource: Resource,
    pub imagery_present: Cell<bool>,
    pub proto_imagery: Cell<Option<bool>>,
    pub terrain_present: Cell<bool>,
    pub negative_altitude_exponent_bias: Cell<f64>,
    pub negative_altitude_threshold: Cell<f64>,
    pub providers: RefCell<BTreeMap<u32, Credit>>,
    pub key: RefCell<Vec<u8>>,
    pub quad_packet_version: Cell<u32>,
    pub tile_info: RefCell<BTreeMap<String, Option<Info>>>,
    client: ResourceClient,
    pending: RefCell<HashMap<String, Pending>>,
}
impl GoogleEarthEnterpriseMetadata {
    pub fn new(resource: Resource, client: ResourceClient) -> Rc<Self> {
        resource.append_forward_slash();
        Rc::new(Self {
            resource,
            imagery_present: Cell::new(true),
            proto_imagery: Cell::new(None),
            terrain_present: Cell::new(true),
            negative_altitude_exponent_bias: Cell::new(32.),
            negative_altitude_threshold: Cell::new(1e-12),
            providers: Default::default(),
            key: RefCell::new(include_bytes!("../../assets/gee-default-key.bin").to_vec()),
            quad_packet_version: Cell::new(1),
            tile_info: Default::default(),
            client,
            pending: Default::default(),
        })
    }
    pub async fn from_url(resource: Resource, client: ResourceClient) -> Result<Rc<Self>, RuntimeError> {
        let m = Self::new(resource, client);
        let db = m
            .resource
            .get_derived_resource(DerivedResourceOptions {
                url: Some("dbRoot.v5?output=proto".into()),
                ..Default::default()
            })
            .ok_or_else(|| RuntimeError::new("Invalid dbRoot resource."))?;
        let db_result = if let Some(f) = fetch_bytes(&m.client, &db) {
            match f.await {
                Ok(b) => m.apply_db_root(&b),
                Err(e) => Err(e),
            }
        } else {
            Err(RuntimeError::new("dbRoot request was throttled."))
        };
        if db_result.is_err() {
            *m.key.borrow_mut() = include_bytes!("../../assets/gee-default-key.bin").to_vec();
        }
        let result = m
            .get_quad_tree_packet("", m.quad_packet_version.get(), None)
            .ok_or_else(|| RuntimeError::new("Metadata request was throttled."))?
            .await;
        if let Err(e) = result {
            let url = m.metadata_resource("", 1, None).map(|r| r.url()).unwrap_or_default();
            return Err(RuntimeError::new(format!("An error occurred while accessing {url}: {e}")));
        }
        Ok(m)
    }
    pub fn apply_db_root(&self, bytes: &[u8]) -> Result<(), RuntimeError> {
        let encrypted = packet::protobuf(bytes)?;
        let key = packet::field(&encrypted, 2).and_then(packet::Wire::bytes).unwrap_or(&[]);
        *self.key.borrow_mut() = key.to_vec();
        let data = packet::field(&encrypted, 3).and_then(packet::Wire::bytes).unwrap_or(&[]);
        let root = packet::inflate_packet(data, key)?;
        self.apply_decoded_db_root(&root)
    }
    pub fn apply_decoded_db_root(&self, bytes: &[u8]) -> Result<(), RuntimeError> {
        let root = packet::protobuf(bytes)?;
        let uint = |id, default| packet::field(&root, id).and_then(packet::Wire::uint).unwrap_or(default);
        self.imagery_present.set(uint(1, 1) != 0);
        self.terrain_present.set(uint(2, 0) != 0);
        self.proto_imagery.set(Some(uint(14, 0) != 0));
        if let Some(b) = packet::field(&root, 13).and_then(packet::Wire::bytes) {
            let v = packet::protobuf(b)?;
            self.quad_packet_version.set(packet::field(&v, 1).and_then(packet::Wire::uint).unwrap_or(0) as u32);
        }
        if let Some(b) = packet::field(&root, 7).and_then(packet::Wire::bytes) {
            let e = packet::protobuf(b)?;
            if let Some(b) = packet::field(&e, 1).and_then(packet::Wire::bytes) {
                let model = packet::protobuf(b)?;
                self.negative_altitude_exponent_bias
                    .set(packet::field(&model, 5).and_then(packet::Wire::uint).unwrap_or(0) as i32 as f64);
                self.negative_altitude_threshold
                    .set(packet::field(&model, 6).and_then(packet::Wire::double).unwrap_or(0.));
            }
        }
        for (_, wire) in root.iter().filter(|(id, _)| *id == 3) {
            if let Some(b) = wire.bytes() {
                let p = packet::protobuf(b)?;
                let id = packet::field(&p, 1).and_then(packet::Wire::uint).unwrap_or(0) as u32;
                if let Some(b) = packet::field(&p, 2).and_then(packet::Wire::bytes) {
                    let copyright = packet::protobuf(b)?;
                    let text = packet::field(&copyright, 2).and_then(packet::Wire::bytes).unwrap_or(&[]);
                    self.providers
                        .borrow_mut()
                        .insert(id, Credit::new(String::from_utf8_lossy(text).into_owned(), false));
                }
            }
        }
        Ok(())
    }
    pub fn tile_xy_to_quad_key(x: u32, y: u32, level: u32) -> String {
        let mut key = String::new();
        for i in (0..=level).rev() {
            let mask = 1u32.wrapping_shl(i);
            let d = if y & mask == 0 {
                if x & mask == 0 {
                    3
                } else {
                    2
                }
            } else if x & mask != 0 {
                1
            } else {
                0
            };
            key.push(char::from(b'0' + d));
        }
        key
    }
    pub fn quad_key_to_tile_xy(key: &str) -> (u32, u32, i32) {
        let level = key.len() as i32 - 1;
        let (mut x, mut y) = (0, 0);
        for (i, b) in key.bytes().enumerate() {
            let mask = 1u32.wrapping_shl((level - i as i32) as u32);
            let digit = b.wrapping_sub(b'0');
            if digit & 2 != 0 {
                if digit & 1 == 0 {
                    x |= mask;
                }
            } else {
                y |= mask;
                if digit & 1 != 0 {
                    x |= mask;
                }
            }
        }
        (x, y, level)
    }
    pub fn get_tile_information(&self, x: u32, y: u32, l: u32) -> Option<Option<Info>> {
        self.get_tile_information_from_quad_key(&Self::tile_xy_to_quad_key(x, y, l))
    }
    pub fn get_tile_information_from_quad_key(&self, q: &str) -> Option<Option<Info>> {
        self.tile_info.borrow().get(q).cloned()
    }
    pub fn is_valid(&self, q: &str) -> bool {
        if let Some(Some(_)) = self.get_tile_information_from_quad_key(q) {
            return true;
        }
        let mut end = q.len();
        while end > 1 {
            let digit = (q.as_bytes()[end - 1] - b'0') as usize;
            end -= 1;
            match self.get_tile_information_from_quad_key(&q[..end]) {
                Some(Some(info)) => return info.has_subtree() || info.has_child(digit),
                Some(None) => return false,
                None => {}
            }
        }
        true
    }
    pub fn metadata_resource(
        &self,
        q: &str,
        version: u32,
        request: Option<RequestHandle<HttpResponse>>,
    ) -> Option<Resource> {
        self.resource.get_derived_resource(DerivedResourceOptions {
            url: Some(format!("flatfile?q2-0{q}-q.{version}")),
            request,
            ..Default::default()
        })
    }
    pub fn apply_metadata_packet(&self, decoded: &[u8], q: &str) -> Result<(), RuntimeError> {
        let mut result = packet::metadata_packet(decoded, q)?;
        let mut tiles = self.tile_info.borrow_mut();
        if !q.is_empty() {
            let bits = result.remove(q).flatten().ok_or_else(|| RuntimeError::new("Metadata root is missing."))?.bits;
            let root = tiles
                .get_mut(q)
                .and_then(Option::as_mut)
                .ok_or_else(|| RuntimeError::new("Metadata parent is missing."))?;
            root.bits |= bits;
        }
        let mut keys = result.keys().cloned().collect::<Vec<_>>();
        keys.sort_by_key(String::len);
        for key in keys {
            let Some(mut info) = result.remove(&key) else { continue };
            if let Some(info) = info.as_mut() {
                if key.len() > 1 {
                    let parent = tiles
                        .get(&key[..key.len() - 1])
                        .and_then(Option::as_ref)
                        .ok_or_else(|| RuntimeError::new("Metadata ancestor is missing."))?;
                    info.set_parent(parent);
                }
            }
            tiles.insert(key, info);
        }
        Ok(())
    }
    pub fn get_quad_tree_packet(
        self: &Rc<Self>,
        q: &str,
        version: u32,
        request: Option<RequestHandle<HttpResponse>>,
    ) -> Option<TerrainFuture<'static, ()>> {
        let r = self.metadata_resource(q, version, request)?;
        let f = fetch_bytes(&self.client, &r)?;
        let key = self.key.borrow().clone();
        let weak = Rc::downgrade(self);
        let q = q.to_owned();
        Some(Box::pin(async move {
            let bytes = f.await?;
            let decoded = packet::inflate_packet(&bytes, &key)?;
            weak.upgrade()
                .ok_or_else(|| RuntimeError::new("Metadata was dropped."))?
                .apply_metadata_packet(&decoded, &q)
        }))
    }
    pub fn populate_subtree(
        self: &Rc<Self>,
        x: u32,
        y: u32,
        l: u32,
        request: Option<RequestHandle<HttpResponse>>,
    ) -> Option<TerrainFuture<'static, Info>> {
        self.populate_key(Self::tile_xy_to_quad_key(x, y, l), request)
    }
    fn populate_key(
        self: &Rc<Self>,
        target: String,
        request: Option<RequestHandle<HttpResponse>>,
    ) -> Option<TerrainFuture<'static, Info>> {
        if let Some(Some(info)) = self.get_tile_information_from_quad_key(&target) {
            if !info.has_subtree() || info.has_children() {
                return Some(Box::pin(async { Ok(info) }));
            }
        }
        let mut q = target.clone();
        let mut entry = self.get_tile_information_from_quad_key(&q);
        while entry.is_none() && q.len() > 1 {
            q.pop();
            entry = self.get_tile_information_from_quad_key(&q);
        }
        let existing = self.pending.borrow().get(&q).cloned();
        let shared = if let Some(f) = existing {
            f
        } else {
            let Some(Some(info)) = entry.filter(|v| v.as_ref().is_some_and(Info::has_subtree)) else {
                return Some(Box::pin(async move {
                    Err(RuntimeError::new(format!("Couldn't load metadata for tile {target}")))
                }));
            };
            let f = self.get_quad_tree_packet(&q, info.cnode_version as u32, request.clone())?;
            let weak = Rc::downgrade(self);
            let cache_key = q.clone();
            let shared = async move {
                let result = f.await;
                if let Some(s) = weak.upgrade() {
                    s.pending.borrow_mut().remove(&cache_key);
                }
                result
            }
            .boxed_local()
            .shared();
            self.pending.borrow_mut().insert(q, shared.clone());
            shared
        };
        let weak = Rc::downgrade(self);
        Some(Box::pin(async move {
            shared.await?;
            let this = weak.upgrade().ok_or_else(|| RuntimeError::new("Metadata was dropped."))?;
            let next = request.map(|r| {
                let r = r.borrow();
                Request::<HttpResponse>::new(RequestOptions {
                    throttle: r.options.throttle,
                    throttle_by_server: r.options.throttle_by_server,
                    request_type: r.options.request_type,
                    priority_function: r.options.priority_function.clone(),
                    ..Default::default()
                })
                .into_handle()
            });
            this.populate_key(target, next).ok_or_else(|| RuntimeError::new("Metadata request was throttled."))?.await
        }))
    }
    pub fn pump(&self) {
        let jobs = self.pending.borrow().values().cloned().collect::<Vec<_>>();
        for mut f in jobs {
            let _ = Pin::new(&mut f).poll(&mut Context::from_waker(Waker::noop()));
        }
    }
}
