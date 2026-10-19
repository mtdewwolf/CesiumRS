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

type SamplingResult<E> = Result<Vec<(usize, Option<f64>)>, TerrainSamplingError<E>>;
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
    for (index, position) in positions.iter().enumerate() {
        let Some(xy) = scheme.position_to_tile_xy(position, level) else {
            continue;
        };
        let (x, y) = (xy.x as u32, xy.y as u32);
        if let Some(group) = groups.iter_mut().find(|g| g.x == x && g.y == y) {
            group.positions.push((index, *position));
        } else {
            groups.push_back(TileGroup { x, y, positions: vec![(index, *position)] });
        }
    }
    let mut active: Vec<(Vec<usize>, SamplingFuture<'_, P::Error>)> = Vec::new();
    let mut delay: Option<Pin<Box<H::Delay<'_>>>> = None;
    poll_fn(|cx| {
        if let Some(wait) = &mut delay {
            if wait.as_mut().poll(cx).is_ready() {
                delay = None;
            }
        }
        if delay.is_none() {
            while let Some(group) = groups.front() {
                let Some(request) = provider.request_tile_geometry(group.x, group.y, level, None) else {
                    delay = Some(Box::pin(host.delay(100)));
                    // Register the timer's waker on this poll.
                    if delay.as_mut().unwrap().as_mut().poll(cx).is_ready() {
                        delay = None;
                        cx.waker().wake_by_ref();
                    }
                    break;
                };
                let group = groups.pop_front().unwrap();
                let indices = group.positions.iter().map(|(index, _)| *index).collect();
                let task = async move {
                    let mut data = request.await.map_err(TerrainSamplingError::Provider)?;
                    let rectangle = scheme.tile_xy_to_rectangle(group.x as f64, group.y as f64, level);
                    let mut heights = Vec::with_capacity(group.positions.len());
                    let mut mesh_required = false;
                    for (index, p) in &group.positions {
                        let height = data.interpolate_height(&rectangle, p.longitude, p.latitude);
                        if height.is_none() {
                            mesh_required = true;
                            break;
                        }
                        heights.push((*index, height));
                    }
                    if mesh_required {
                        let mut options =
                            TerrainMeshRequest::new(scheme, TerrainTile { x: group.x, y: group.y, level });
                        options.throttle = false;
                        data.create_mesh(options)
                            .ok_or(TerrainSamplingError::MeshDeferred)?
                            .await
                            .map_err(TerrainSamplingError::Mesh)?;
                        heights.clear();
                        for (index, p) in &group.positions {
                            heights.push((*index, data.interpolate_height(&rectangle, p.longitude, p.latitude)));
                        }
                    }
                    Ok(heights)
                };
                active.push((indices, Box::pin(task)));
            }
        }
        let mut index = 0;
        while index < active.len() {
            let result = active[index].1.as_mut().poll(cx);
            match result {
                Poll::Pending => index += 1,
                Poll::Ready(result) => {
                    let (indices, _) = active.remove(index);
                    match result {
                        Ok(heights) => {
                            for (index, height) in heights {
                                if let Some(height) = height {
                                    positions[index].height = height;
                                }
                            }
                        }
                        Err(error) => {
                            if reject_on_tile_fail {
                                return Poll::Ready(Err(error));
                            }
                            for index in indices {
                                positions[index].height = f64::NAN;
                            }
                        }
                    }
                }
            }
        }
        if groups.is_empty() && active.is_empty() {
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
        for &index in &pending {
            let level = provider.availability().unwrap().compute_maximum_level_at_position(&positions[index]);
            levels.push(level);
            if level == 0 {
                if let Some(xy) = provider.tiling_scheme().position_to_tile_xy(&positions[index], 1) {
                    if let Some(request) = provider.load_tile_data_availability(xy.x as u32, xy.y as u32, 1) {
                        request.await.map_err(TerrainSamplingError::Provider)?;
                    }
                }
            }
            if level >= 0 {
                by_level.entry(level as u32).or_default().push(index);
            }
        }
        for (level, indices) in by_level {
            let mut subset: Vec<_> = indices.iter().map(|&i| positions[i]).collect();
            sample_terrain(provider, &mut subset, SampleTerrainOptions { level, reject_on_tile_fail }, host).await?;
            for (index, position) in indices.into_iter().zip(subset) {
                positions[index] = position;
            }
        }
        pending = pending
            .into_iter()
            .zip(levels)
            .filter_map(|(index, previous)| {
                (provider.availability().unwrap().compute_maximum_level_at_position(&positions[index]) != previous)
                    .then_some(index)
            })
            .collect();
    }
    Ok(())
}
