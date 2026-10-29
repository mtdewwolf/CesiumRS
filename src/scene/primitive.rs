//! Partial CesiumJS `Primitive` scaffold for geometry combination and options.
//!
//! GPU resource creation, batch tables and executable draw commands are not
//! implemented. Updating a drawable primitive returns an error rather than
//! claiming readiness and queuing commands that do nothing.

use super::{
    appearance::Appearance,
    primitive_pipeline::{CombineGeometryParameters, PrimitivePipeline},
    primitive_state::PrimitiveState,
    scene::{Result, ScenePrimitive},
    FrameState, ShadowMode,
};
use crate::core::{BoundingSphere, Geometry, GeometryInstance, Matrix4, RuntimeError};

/// Options for creating a [`Primitive`].
pub struct PrimitiveOptions {
    pub geometry_instances: Vec<GeometryInstance>,
    pub appearance: Option<Box<dyn Appearance>>,
    pub depth_fail_appearance: Option<Box<dyn Appearance>>,
    pub show: Option<bool>,
    pub model_matrix: Option<Matrix4>,
    pub vertex_cache_optimize: Option<bool>,
    pub interleave: Option<bool>,
    pub compress_vertices: Option<bool>,
    pub release_geometry_instances: Option<bool>,
    pub allow_picking: Option<bool>,
    pub cull: Option<bool>,
    pub asynchronous: Option<bool>,
    pub debug_show_bounding_volume: Option<bool>,
    pub shadows: Option<ShadowMode>,
}

impl Default for PrimitiveOptions {
    fn default() -> Self {
        Self {
            geometry_instances: Vec::new(),
            appearance: None,
            depth_fail_appearance: None,
            show: Some(true),
            model_matrix: Some(Matrix4::IDENTITY),
            vertex_cache_optimize: Some(false),
            interleave: Some(false),
            compress_vertices: Some(true),
            release_geometry_instances: Some(true),
            allow_picking: Some(true),
            cull: Some(true),
            asynchronous: Some(true),
            debug_show_bounding_volume: Some(false),
            shadows: Some(ShadowMode::Disabled),
        }
    }
}

/// Partial primitive options and geometry preparation; rendering is unsupported.
pub struct Primitive {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub appearance: Option<Box<dyn Appearance>>,
    pub depth_fail_appearance: Option<Box<dyn Appearance>>,
    pub vertex_cache_optimize: bool,
    pub interleave: bool,
    pub compress_vertices: bool,
    pub release_geometry_instances: bool,
    pub allow_picking: bool,
    pub cull: bool,
    pub asynchronous: bool,
    pub debug_show_bounding_volume: bool,
    pub shadows: ShadowMode,

    geometry_instances: Option<Vec<GeometryInstance>>,
    geometries: Vec<Geometry>,
    bounding_spheres: Vec<Option<BoundingSphere>>,
    bounding_spheres_cv: Vec<Option<BoundingSphere>>,
    state: PrimitiveState,
    destroyed: bool,
}

impl Primitive {
    pub fn new(options: PrimitiveOptions) -> Self {
        Self {
            show: options.show.unwrap_or(true),
            model_matrix: options.model_matrix.unwrap_or(Matrix4::IDENTITY),
            appearance: options.appearance,
            depth_fail_appearance: options.depth_fail_appearance,
            vertex_cache_optimize: options.vertex_cache_optimize.unwrap_or(false),
            interleave: options.interleave.unwrap_or(false),
            compress_vertices: options.compress_vertices.unwrap_or(true),
            release_geometry_instances: options.release_geometry_instances.unwrap_or(true),
            allow_picking: options.allow_picking.unwrap_or(true),
            cull: options.cull.unwrap_or(true),
            asynchronous: options.asynchronous.unwrap_or(true),
            debug_show_bounding_volume: options.debug_show_bounding_volume.unwrap_or(false),
            shadows: options.shadows.unwrap_or(ShadowMode::Disabled),
            geometry_instances: Some(options.geometry_instances),
            geometries: Vec::new(),
            bounding_spheres: Vec::new(),
            bounding_spheres_cv: Vec::new(),
            state: PrimitiveState::Ready,
            destroyed: false,
        }
    }

    pub fn state(&self) -> PrimitiveState {
        self.state
    }

    pub fn is_ready(&self) -> bool {
        self.state == PrimitiveState::Complete
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn geometries(&self) -> &[Geometry] {
        &self.geometries
    }

    pub fn bounding_spheres(&self) -> &[Option<BoundingSphere>] {
        &self.bounding_spheres
    }

    fn combine(&mut self, frame: &FrameState) -> Result<()> {
        let instances = match self.geometry_instances.take() {
            Some(inst) => inst,
            None => return Ok(()),
        };

        let parameters = CombineGeometryParameters {
            instances,
            uint_index_support: frame.context_capabilities.element_index_uint,
            scene_3d_only: frame.scene_3d_only,
            vertex_cache_optimize: self.vertex_cache_optimize,
            compress_vertices: self.compress_vertices,
            model_matrix: self.model_matrix,
            create_pick_offsets: self.allow_picking,
            ..Default::default()
        };

        match PrimitivePipeline::combine_geometry(parameters) {
            Ok(results) => {
                self.model_matrix = results.model_matrix;
                self.geometries = results.geometries.unwrap_or_default();
                self.bounding_spheres = results.bounding_spheres;
                self.bounding_spheres_cv = results.bounding_spheres_cv;
                if !self.geometries.is_empty() {
                    self.state = PrimitiveState::Failed;
                    return Err(RuntimeError::new(
                        "Primitive rendering is not implemented: executable GPU command creation is required",
                    ));
                }
                self.state = PrimitiveState::Complete;
                Ok(())
            }
            Err(e) => {
                self.state = PrimitiveState::Failed;
                Err(e)
            }
        }
    }
}

impl ScenePrimitive for Primitive {
    fn pre_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("Primitive has been destroyed"));
        }
        if self.state == PrimitiveState::Failed {
            return Err(RuntimeError::new("Primitive creation previously failed"));
        }
        if !self.show || self.appearance.is_none() {
            return Ok(());
        }

        if self.state == PrimitiveState::Ready {
            self.combine(frame)?;
        }

        Ok(())
    }

    fn post_passes_update(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }

    fn update_for_pass(&mut self, _frame: &mut FrameState, _pass: super::Cesium3DTilePass) -> Result<()> {
        Ok(())
    }

    fn destroy(&mut self) -> Result<()> {
        self.destroyed = true;
        self.geometries.clear();
        self.bounding_spheres.clear();
        self.bounding_spheres_cv.clear();
        self.geometry_instances = None;
        Ok(())
    }
}
