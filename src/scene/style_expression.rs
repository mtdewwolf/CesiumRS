//! CesiumJS `StyleExpression`: the interface implemented by `Expression` and
//! `ConditionsExpression`.

use super::expression::{Expression, ShaderState, StyleFeature, StyleValue};
use crate::core::{Color, RuntimeError};
use serde_json::Value as Json;
use std::collections::BTreeMap;

/// A style expression that can be evaluated against a feature and emitted as
/// a GLSL function.
///
/// Cesium's `StyleExpression` is an abstract constructor that throws when its
/// methods are called directly; a Rust trait carries the same contract without
/// an instantiable base value.
pub trait StyleExpression {
    fn evaluate(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError>;

    fn evaluate_color(&self, feature: Option<&dyn StyleFeature>) -> Result<Option<Color>, RuntimeError>;

    /// `Ok(None)` is Cesium's `undefined` shader expression, which callers
    /// cache and treat as "no shader function".
    fn get_shader_function(
        &self,
        function_signature: &str,
        variable_substitution_map: &BTreeMap<String, String>,
        shader_state: &mut ShaderState,
        return_type: &str,
    ) -> Result<Option<String>, RuntimeError>;

    fn get_variables(&self) -> Vec<String>;

    /// The JSON form stored back into `Cesium3DTileStyle.style`.
    fn json_value(&self) -> Json {
        Json::Null
    }
}

impl StyleExpression for Expression {
    fn evaluate(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        Expression::evaluate(self, feature)
    }

    fn evaluate_color(&self, feature: Option<&dyn StyleFeature>) -> Result<Option<Color>, RuntimeError> {
        Expression::evaluate_color(self, feature)
    }

    fn get_shader_function(
        &self,
        function_signature: &str,
        variable_substitution_map: &BTreeMap<String, String>,
        shader_state: &mut ShaderState,
        return_type: &str,
    ) -> Result<Option<String>, RuntimeError> {
        Expression::get_shader_function(self, function_signature, variable_substitution_map, shader_state, return_type)
            .map(Some)
    }

    fn get_variables(&self) -> Vec<String> {
        Expression::get_variables(self)
    }

    fn json_value(&self) -> Json {
        Json::String(self.expression().to_string())
    }
}
