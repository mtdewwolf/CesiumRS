//! CesiumJS `Scene/Model/ModelArticulationStage`: one stage of an
//! `AGI_articulations` articulation.
//!
//! The value is clamped to the stage's minimum and maximum, and only a change
//! larger than `Math.EPSILON16` marks the owning articulation dirty, so a
//! value that is set every frame does not rebuild transforms needlessly.

use crate::{
    core::{math, ArticulationStageType, Cartesian3, Matrix3, Matrix4, RuntimeError},
    scene::model_components as mc,
};

/// An in-memory representation of an articulation stage.
#[derive(Clone)]
pub struct ModelArticulationStage {
    name: Option<String>,
    stage_type: ArticulationStageType,
    minimum_value: f64,
    maximum_value: f64,
    current_value: f64,
}

impl ModelArticulationStage {
    /// Builds a stage and starts at the model's initial value. A stage without
    /// a type is an error, since Cesium only checks that the components exist.
    pub fn new(stage: &mc::ArticulationStage) -> Result<Self, RuntimeError> {
        let stage_type = stage.stage_type.ok_or_else(|| RuntimeError::new("Articulation stage is missing a type"))?;
        Ok(Self {
            name: stage.name.clone(),
            stage_type,
            minimum_value: stage.minimum_value.unwrap_or(f64::NEG_INFINITY),
            maximum_value: stage.maximum_value.unwrap_or(f64::INFINITY),
            current_value: stage.initial_value.unwrap_or(0.0),
        })
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn stage_type(&self) -> ArticulationStageType {
        self.stage_type
    }

    pub fn minimum_value(&self) -> f64 {
        self.minimum_value
    }

    pub fn maximum_value(&self) -> f64 {
        self.maximum_value
    }

    pub fn current_value(&self) -> f64 {
        self.current_value
    }

    /// Sets the value, clamped to the stage's range.
    ///
    /// Returns `true` when the change was large enough to mark the owning
    /// articulation dirty.
    pub fn set_current_value(&mut self, value: f64) -> bool {
        let value = math::clamp(value, self.minimum_value, self.maximum_value);
        if math::equals_epsilon(self.current_value, value, math::EPSILON16, math::EPSILON16) {
            return false;
        }
        self.current_value = value;
        true
    }

    /// Multiplies a stage transform into `result`.
    ///
    /// The incoming value of `result` participates in the computation: several
    /// stages of one articulation compose into a single matrix.
    pub fn apply_stage_to_matrix(&self, result: &Matrix4) -> Matrix4 {
        let value = self.current_value;
        match self.stage_type {
            ArticulationStageType::XRotate => {
                result.multiply_by_matrix3(&Matrix3::from_rotation_x(math::to_radians(value)))
            }
            ArticulationStageType::YRotate => {
                result.multiply_by_matrix3(&Matrix3::from_rotation_y(math::to_radians(value)))
            }
            ArticulationStageType::ZRotate => {
                result.multiply_by_matrix3(&Matrix3::from_rotation_z(math::to_radians(value)))
            }
            ArticulationStageType::XTranslate => result.multiply_by_translation(Cartesian3::new(value, 0.0, 0.0)),
            ArticulationStageType::YTranslate => result.multiply_by_translation(Cartesian3::new(0.0, value, 0.0)),
            ArticulationStageType::ZTranslate => result.multiply_by_translation(Cartesian3::new(0.0, 0.0, value)),
            ArticulationStageType::XScale => result.multiply_by_scale(Cartesian3::new(value, 1.0, 1.0)),
            ArticulationStageType::YScale => result.multiply_by_scale(Cartesian3::new(1.0, value, 1.0)),
            ArticulationStageType::ZScale => result.multiply_by_scale(Cartesian3::new(1.0, 1.0, value)),
            ArticulationStageType::UniformScale => result.multiply_by_uniform_scale(value),
        }
    }
}

impl std::fmt::Debug for ModelArticulationStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelArticulationStage")
            .field("name", &self.name)
            .field("stage_type", &self.stage_type)
            .field("current_value", &self.current_value)
            .finish()
    }
}
