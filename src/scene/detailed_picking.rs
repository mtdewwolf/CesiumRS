//! Most-detailed ray preload requests and completion after a subsequent
//! post-render event. Requests own their inputs and can outlive a frame call.
use super::*;
use crate::{core::*, renderer::Gl};
use futures_channel::oneshot;
use std::{
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};
type Result<T> = std::result::Result<T, RuntimeError>;
pub struct MostDetailedPickRequest {
    receiver: oneshot::Receiver<Result<Vec<RayPickResult>>>,
}
impl Future for MostDetailedPickRequest {
    type Output = Result<Vec<RayPickResult>>;
    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.receiver).poll(context).map(|result| {
            result.unwrap_or_else(|_| Err(RuntimeError::new("Most-detailed pick was cancelled or Scene destroyed")))
        })
    }
}
pub(crate) struct DetailedPickJob<B: SceneBackend> {
    pub camera: Camera,
    pub targets: Vec<PickedObject>,
    pub ready: bool,
    pub complete_after: u64,
    pub result: Option<Result<Vec<RayPickResult>>>,
    pub execute: Option<PickExecutor<B>>,
    pub sender: oneshot::Sender<Result<Vec<RayPickResult>>>,
}
type PickCallback<B> = dyn FnOnce(&mut Scene<B>) -> Result<Vec<RayPickResult>>;
pub(crate) struct PickExecutor<B: SceneBackend>(Box<PickCallback<B>>);
impl<B: SceneBackend> Scene<B> {
    pub(crate) fn update_most_detailed_ray_picks(&mut self) -> Result<()> {
        let mut retained = Vec::new();
        for mut job in std::mem::take(&mut self.detailed_picks) {
            if job.sender.is_canceled() {
                continue;
            }
            if job.ready || job.result.is_some() {
                retained.push(job);
                continue;
            }
            let camera = job.camera.uniform_camera();
            let mut pass = TilesetPassState::new(Cesium3DTilePass::MostDetailedPreload);
            pass.culling_volume =
                Some(job.camera.frustum.compute_culling_volume(camera.position, camera.direction, camera.up));
            pass.camera = Some(camera);
            let ready = (|| {
                let mut ready = true;
                for primitive in &mut self.primitives {
                    let requested = primitive
                        .most_detailed_pick_targets()
                        .into_iter()
                        .filter(|id| job.targets.iter().any(|target| Rc::ptr_eq(target, id)))
                        .collect::<Vec<_>>();
                    if !requested.is_empty() {
                        ready &= primitive.update_most_detailed_pick(&mut self.frame_state, &mut pass, &requested)?;
                    }
                }
                Ok(ready)
            })();
            match ready {
                Ok(ready) => {
                    job.ready = ready;
                    retained.push(job);
                }
                Err(error) => {
                    let _ = job.sender.send(Err(error));
                }
            }
        }
        self.detailed_picks.extend(retained);
        Ok(())
    }
    pub(crate) fn complete_most_detailed_ray_picks(&mut self) {
        let mut retained = Vec::new();
        for mut job in std::mem::take(&mut self.detailed_picks) {
            if job.sender.is_canceled() {
                continue;
            }
            if job.result.is_some() && self.post_render_serial >= job.complete_after {
                let _ = job.sender.send(job.result.take().expect("checked"));
                continue;
            }
            if job.ready && job.result.is_none() {
                if let Some(execute) = job.execute.take() {
                    match (execute.0)(self) {
                        Ok(result) => {
                            job.result = Some(Ok(result));
                            job.complete_after = self.post_render_serial + 1;
                            self.request_render();
                        }
                        Err(error) => {
                            let _ = job.sender.send(Err(error));
                            continue;
                        }
                    }
                }
            }
            retained.push(job);
        }
        self.detailed_picks.extend(retained);
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampledHeight {
    pub longitude: f64,
    pub latitude: f64,
    pub height: Option<f64>,
}
impl<G: Gl + 'static> Scene<RendererSceneBackend<G>> {
    fn empty_pick_post_render_barrier(&mut self) -> MostDetailedPickRequest {
        let (sender, receiver) = oneshot::channel();
        self.detailed_picks.push(DetailedPickJob {
            camera: self.camera.clone_camera(),
            targets: Vec::new(),
            ready: true,
            complete_after: self.post_render_serial + 1,
            result: Some(Ok(Vec::new())),
            execute: None,
            sender,
        });
        self.request_render();
        MostDetailedPickRequest { receiver }
    }
    fn enqueue_detailed_pick(
        &mut self,
        ray: Ray,
        limit: Option<usize>,
        excluded: Vec<PickExclusion>,
        width: Option<f64>,
        require_position: bool,
    ) -> Result<MostDetailedPickRequest> {
        if self.camera.mode() != SceneMode::Scene3D {
            return Err(RuntimeError::new("Ray intersections are only supported in 3D mode."));
        }
        let targets = self
            .primitives
            .iter()
            .flat_map(|p| p.most_detailed_pick_targets())
            .filter(|target| {
                !excluded.iter().any(|e| match e {
                    PickExclusion::Object(o) | PickExclusion::Primitive(o) => Rc::ptr_eq(o, target),
                    PickExclusion::Id(_) => false,
                })
            })
            .collect::<Vec<_>>();
        let mut options = CameraOptions::new(self.camera.drawing_buffer_width, self.camera.drawing_buffer_height);
        options.client_width = self.camera.client_width;
        options.client_height = self.camera.client_height;
        options.map_projection = self.frame_state.uniform.map_projection.clone();
        options.ellipsoid = self.camera.ellipsoid;
        let mut camera = Camera::new(options);
        let mut frustum = OrthographicFrustum::new(0.1, 1.);
        frustum.near = 0.1;
        camera.frustum = CameraFrustum::Orthographic(frustum);
        Picking::update_offscreen_camera_from_ray(&ray, width, &mut camera)?;
        let (sender, receiver) = oneshot::channel();
        let mut job = DetailedPickJob {
            camera,
            ready: targets.is_empty(),
            targets,
            complete_after: 0,
            result: None,
            execute: Some(PickExecutor(Box::new(move |scene| {
                scene.drill_ray(ray, limit, &excluded, width, require_position, true)
            }))),
            sender,
        };
        if job.ready {
            let execute = job.execute.take().expect("initialized");
            job.result = Some(Ok((execute.0)(self)?));
            job.complete_after = self.post_render_serial + 1;
        }
        self.detailed_picks.push(job);
        self.request_render();
        Ok(MostDetailedPickRequest { receiver })
    }
    pub fn pick_from_ray_most_detailed(
        &mut self,
        ray: Ray,
        excluded: Vec<PickExclusion>,
        width: Option<f64>,
    ) -> Result<impl Future<Output = Result<Option<RayPickResult>>> + 'static> {
        let request = self.enqueue_detailed_pick(ray, Some(1), excluded, width, false)?;
        Ok(async move { Ok(request.await?.into_iter().next()) })
    }
    pub fn drill_pick_from_ray_most_detailed(
        &mut self,
        ray: Ray,
        limit: Option<usize>,
        excluded: Vec<PickExclusion>,
        width: Option<f64>,
    ) -> Result<MostDetailedPickRequest> {
        self.enqueue_detailed_pick(ray, limit, excluded, width, false)
    }
    pub fn sample_height_most_detailed(
        &mut self,
        positions: Vec<Cartographic>,
        excluded: Vec<PickExclusion>,
        width: Option<f64>,
    ) -> Result<impl Future<Output = Result<Vec<SampledHeight>>> + 'static> {
        if self.camera.mode() != SceneMode::Scene3D {
            return Err(RuntimeError::new("sampleHeightMostDetailed is only supported in 3D mode."));
        }
        if !self.sample_height_supported() {
            return Err(RuntimeError::new(
                "sampleHeightMostDetailed requires depth texture support. Check sampleHeightSupported.",
            ));
        }
        let ellipsoid = self.camera.ellipsoid;
        let requests = positions
            .iter()
            .map(|p| {
                self.enqueue_detailed_pick(
                    Picking::ray_for_sample_height(&ellipsoid, p),
                    Some(1),
                    excluded.clone(),
                    width,
                    true,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let barrier = requests.is_empty().then(|| self.empty_pick_post_render_barrier());
        Ok(async move {
            if let Some(barrier) = barrier {
                barrier.await?;
            }
            let results = futures_util::future::join_all(requests).await;
            positions
                .into_iter()
                .zip(results)
                .map(|(position, result)| {
                    Ok(SampledHeight {
                        longitude: position.longitude,
                        latitude: position.latitude,
                        height: result?
                            .into_iter()
                            .next()
                            .and_then(|r| r.position)
                            .and_then(|p| ellipsoid.cartesian_to_cartographic(p))
                            .map(|p| p.height),
                    })
                })
                .collect()
        })
    }
    pub fn clamp_to_height_most_detailed(
        &mut self,
        positions: Vec<Cartesian3>,
        excluded: Vec<PickExclusion>,
        width: Option<f64>,
    ) -> Result<impl Future<Output = Result<Vec<Option<Cartesian3>>>> + 'static> {
        if self.camera.mode() != SceneMode::Scene3D {
            return Err(RuntimeError::new("clampToHeightMostDetailed is only supported in 3D mode."));
        }
        if !self.clamp_to_height_supported() {
            return Err(RuntimeError::new(
                "clampToHeightMostDetailed requires depth texture support. Check clampToHeightSupported.",
            ));
        }
        let ellipsoid = self.camera.ellipsoid;
        let requests = positions
            .iter()
            .map(|p| {
                let p = ellipsoid
                    .cartesian_to_cartographic(*p)
                    .ok_or_else(|| RuntimeError::new("Cartesian position has no cartographic coordinates"))?;
                self.enqueue_detailed_pick(
                    Picking::ray_for_sample_height(&ellipsoid, &p),
                    Some(1),
                    excluded.clone(),
                    width,
                    true,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        let barrier = requests.is_empty().then(|| self.empty_pick_post_render_barrier());
        Ok(async move {
            if let Some(barrier) = barrier {
                barrier.await?;
            }
            futures_util::future::join_all(requests)
                .await
                .into_iter()
                .map(|result| Ok(result?.into_iter().next().and_then(|r| r.position)))
                .collect()
        })
    }
}
