//! CesiumJS terrain sampling. Missing heights use NaN because `Cartographic`
//! stores a numeric height; positions outside the provider rectangle are kept.

use super::terrain_data::{TerrainData, TerrainMeshRequest, TerrainTile};
use super::{Cartographic, RuntimeError, TerrainProvider};
use std::{
    collections::VecDeque,
    future::{poll_fn, Future},
    pin::Pin,
    task::Poll,
};

/// Supplies the asynchronous 100ms backoff for a throttled tile request.
/// The delay's waker must notify the executor; no blocking sleep is used.
pub trait TerrainSamplingHost {
    type Delay<'a>: Future<Output = ()>
    where
        Self: 'a;
    fn delay(&self, milliseconds: u32) -> Self::Delay<'_>;
}

#[derive(Debug)]
pub enum TerrainSamplingError<E> {
    Provider(E),
    Mesh(RuntimeError),
    MeshDeferred,
    MissingAvailability,
}

type SamplingResult<E> = Result<(), TerrainSamplingError<E>>;
type SamplingFuture<'a, E> = Pin<Box<dyn Future<Output = SamplingResult<E>> + 'a>>;

struct TileGroup {
    x: u32,
    y: u32,
    positions: Vec<(usize, Cartographic)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SampleTerrainOptions {
    pub level: u32,
    pub reject_on_tile_fail: bool,
}

/// Groups points into tiles, requests each tile once, and interpolates against
/// its actual surface triangles. Mesh creation is retried without throttling
/// when compressed data cannot interpolate directly.
pub async fn sample_terrain<P, H>(
    provider: &P,
    positions: &mut [Cartographic],
    options: SampleTerrainOptions,
    host: &H,
) -> Result<(), TerrainSamplingError<P::Error>>
where
    P: TerrainProvider,
    P::TerrainData: TerrainData,
    H: TerrainSamplingHost,
{
    let SampleTerrainOptions { level, reject_on_tile_fail } = options;
    let scheme = provider.tiling_scheme();
    let mut groups: VecDeque<TileGroup> = VecDeque::new();
    // Tile -> index into `groups`, which keeps first-seen order as Cesium's
    // object keys do.
    let mut by_tile: std::collections::HashMap<(u32, u32), usize> = std::collections::HashMap::new();
    for (index, position) in positions.iter().enumerate() {
        let Some(xy) = scheme.position_to_tile_xy(position, level) else {
            continue;
        };
        let (x, y) = (xy.x as u32, xy.y as u32);
        match by_tile.entry((x, y)) {
            std::collections::hash_map::Entry::Occupied(e) => groups[*e.get()].positions.push((index, *position)),
            std::collections::hash_map::Entry::Vacant(e) => {
                e.insert(groups.len());
                groups.push_back(TileGroup { x, y, positions: vec![(index, *position)] });
            }
        }
    }
    let working = std::rc::Rc::new(std::cell::RefCell::new(positions.to_vec()));
    let mut active: Vec<(Vec<usize>, SamplingFuture<'_, P::Error>)> = Vec::new();
    let mut delay: Option<Pin<Box<H::Delay<'_>>>> = None;
    poll_fn(|cx| {
        if let Some(wait) = &mut delay {
            if wait.as_mut().poll(cx).is_ready() {
                delay = None;
            }
        }
        if delay.is_none() {
            while let Some(group) = groups.pop_front() {
                let Some(request) = provider.request_tile_geometry(group.x, group.y, level, None) else {
                    groups.push_front(group);
                    delay = Some(Box::pin(host.delay(100)));
                    // Register the timer's waker on this poll.
                    if let Some(wait) = delay.as_mut() {
                        if wait.as_mut().poll(cx).is_ready() {
                            delay = None;
                            cx.waker().wake_by_ref();
                        }
                    }
                    break;
                };
                let indices = group.positions.iter().map(|(index, _)| *index).collect();
                let working = working.clone();
                let task = async move {
                    let mut data = request.await.map_err(TerrainSamplingError::Provider)?;
                    let rectangle = scheme.tile_xy_to_rectangle(group.x as f64, group.y as f64, level);
                    let mut mesh_required = false;
                    for (index, p) in &group.positions {
                        let height = data.interpolate_height(&rectangle, p.longitude, p.latitude);
                        if height.is_none() {
                            mesh_required = true;
                            break;
                        }
                        if let Some(height) = height {
                            working.borrow_mut()[*index].height = height;
                        }
                    }
                    if mesh_required {
                        let mut options =
                            TerrainMeshRequest::new(scheme, TerrainTile { x: group.x, y: group.y, level });
                        options.throttle = false;
                        data.create_mesh(options)
                            .ok_or(TerrainSamplingError::MeshDeferred)?
                            .await
                            .map_err(TerrainSamplingError::Mesh)?;
                        for (index, p) in &group.positions {
                            if let Some(height) = data.interpolate_height(&rectangle, p.longitude, p.latitude) {
                                working.borrow_mut()[*index].height = height;
                            }
                        }
                    }
                    Ok(())
                };
                active.push((indices, Box::pin(task)));
            }
        }
        let mut index = 0;
        let mut first_error = None;
        while index < active.len() {
            let result = active[index].1.as_mut().poll(cx);
            match result {
                Poll::Pending => index += 1,
                Poll::Ready(result) => {
                    let (indices, _) = active.remove(index);
                    match result {
                        Ok(()) => {}
                        Err(error) => {
                            if reject_on_tile_fail {
                                // Promise callbacks for other ready tiles run
                                // even when one tile rejects. Poll the complete
                                // batch before returning its first error.
                                if first_error.is_none() {
                                    first_error = Some(error);
                                }
                                continue;
                            }
                            for index in indices {
                                working.borrow_mut()[index].height = f64::NAN;
                            }
                        }
                    }
                }
            }
        }
        positions.copy_from_slice(&working.borrow());
        if let Some(error) = first_error {
            Poll::Ready(Err(error))
        } else if groups.is_empty() && active.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    })
    .await
}

/// Samples the currently deepest available level, refreshing availability and
/// repeating for points whose maximum level changed while loading terrain.
pub async fn sample_terrain_most_detailed<P, H>(
    provider: &P,
    positions: &mut [Cartographic],
    reject_on_tile_fail: bool,
    host: &H,
) -> Result<(), TerrainSamplingError<P::Error>>
where
    P: TerrainProvider,
    P::TerrainData: TerrainData,
    H: TerrainSamplingHost,
{
    if provider.availability().is_none() {
        return Err(TerrainSamplingError::MissingAvailability);
    }
    let mut pending: Vec<usize> = (0..positions.len()).collect();
    while !pending.is_empty() {
        let mut levels = Vec::with_capacity(pending.len());
        let mut by_level: std::collections::BTreeMap<u32, Vec<usize>> = Default::default();
        let mut availability_requests = Vec::new();
        for &index in &pending {
            // A level-0 tile loads availability before the next position is
            // measured, so later positions in this pass can see the new range.
            let level = {
                let Some(availability) = provider.availability() else {
                    return Err(TerrainSamplingError::MissingAvailability);
                };
                availability.compute_maximum_level_at_position(&positions[index])
            };
            levels.push(level);
            if level == 0 {
                if let Some(xy) = provider.tiling_scheme().position_to_tile_xy(&positions[index], 1) {
                    if let Some(request) = provider.load_tile_data_availability(xy.x as u32, xy.y as u32, 1) {
                        availability_requests.push(Box::pin(request));
                    }
                }
            }
            if level >= 0 {
                by_level.entry(level as u32).or_default().push(index);
            }
        }
        poll_fn(|cx| {
            let mut index = 0;
            while index < availability_requests.len() {
                match availability_requests[index].as_mut().poll(cx) {
                    Poll::Pending => index += 1,
                    Poll::Ready(Ok(())) => {
                        drop(availability_requests.remove(index));
                    }
                    Poll::Ready(Err(error)) => return Poll::Ready(Err(TerrainSamplingError::Provider(error))),
                }
            }
            if availability_requests.is_empty() {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
        .await?;
        let working = std::rc::Rc::new(std::cell::RefCell::new(positions.to_vec()));
        let jobs = by_level.into_iter().map(|(level, indices)| {
            let working = working.clone();
            let mut subset: Vec<_> = indices.iter().map(|&i| working.borrow()[i]).collect();
            async move {
                let result =
                    sample_terrain(provider, &mut subset, SampleTerrainOptions { level, reject_on_tile_fail }, host)
                        .await;
                for (index, position) in indices.into_iter().zip(subset) {
                    working.borrow_mut()[index] = position;
                }
                result
            }
        });
        let mut samples: Vec<Option<SamplingFuture<'_, P::Error>>> =
            jobs.map(|job| Some(Box::pin(job) as SamplingFuture<'_, P::Error>)).collect();
        poll_fn(|cx| {
            let mut first_error = None;
            for job in &mut samples {
                if let Some(future) = job {
                    if let Poll::Ready(result) = future.as_mut().poll(cx) {
                        *job = None;
                        if let Err(error) = result {
                            if first_error.is_none() {
                                first_error = Some(error);
                            }
                        }
                    }
                }
            }
            positions.copy_from_slice(&working.borrow());
            if let Some(error) = first_error {
                Poll::Ready(Err(error))
            } else if samples.iter().all(Option::is_none) {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
        .await?;
        let Some(availability) = provider.availability() else {
            return Err(TerrainSamplingError::MissingAvailability);
        };
        pending = pending
            .into_iter()
            .zip(levels)
            .filter_map(|(index, previous)| {
                (availability.compute_maximum_level_at_position(&positions[index]) != previous).then_some(index)
            })
            .collect();
    }
    Ok(())
}
