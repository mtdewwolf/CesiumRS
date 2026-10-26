//! CesiumJS `ConditionsExpression`: an ordered if/else chain of style
//! expressions.

use super::expression::{Expression, ShaderState, StyleFeature, StyleValue};
use super::style_expression::StyleExpression;
use crate::core::{Color, RuntimeError};
use serde_json::Value as Json;
use std::collections::BTreeMap;

#[derive(Clone)]
struct Statement {
    condition: Expression,
    expression: Expression,
}

/// A conditions expression such as
/// `{ conditions: [["${Height} > 10", "color('red')"], ["true", "color('blue')"]] }`.
#[derive(Clone)]
pub struct ConditionsExpression {
    conditions_expression: Json,
    runtime_conditions: Option<Vec<Statement>>,
}

impl std::fmt::Debug for ConditionsExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConditionsExpression").field("conditions_expression", &self.conditions_expression).finish()
    }
}

impl ConditionsExpression {
    pub fn new(
        conditions: &[(String, String)],
        defines: Option<&BTreeMap<String, String>>,
    ) -> Result<Self, RuntimeError> {
        let json_conditions = Json::Array(
            conditions
                .iter()
                .map(|(condition, expression)| {
                    Json::Array(vec![Json::String(condition.clone()), Json::String(expression.clone())])
                })
                .collect(),
        );
        let conditions_expression = Json::Object([("conditions".to_string(), json_conditions)].into_iter().collect());
        Self::from_parts(conditions_expression, conditions, defines)
    }

    /// Builds from the style JSON object that upstream calls a conditions
    /// expression. Missing `conditions` yields the upstream no-op expression.
    pub fn from_json(
        conditions_expression: &Json,
        defines: Option<&BTreeMap<String, String>>,
    ) -> Result<Self, RuntimeError> {
        let conditions = conditions_expression
            .get("conditions")
            .and_then(Json::as_array)
            .map(|statements| {
                statements
                    .iter()
                    .map(|statement| {
                        let statement = statement
                            .as_array()
                            .ok_or_else(|| RuntimeError::new("conditions statements must be arrays"))?;
                        let condition = statement.first().map(js_string).unwrap_or_else(|| "true".into());
                        let expression = statement.get(1).map(js_string).unwrap_or_else(|| "undefined".into());
                        Ok((condition, expression))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        match conditions {
            Some(conditions) => Self::new(&conditions, defines),
            None => Ok(Self { conditions_expression: conditions_expression.clone(), runtime_conditions: None }),
        }
    }

    fn from_parts(
        conditions_expression: Json,
        conditions: &[(String, String)],
        defines: Option<&BTreeMap<String, String>>,
    ) -> Result<Self, RuntimeError> {
        let runtime_conditions = conditions
            .iter()
            .map(|(condition, expression)| {
                Ok(Statement {
                    condition: Expression::new(condition, defines)?,
                    expression: Expression::new(expression, defines)?,
                })
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        Ok(Self { conditions_expression, runtime_conditions: Some(runtime_conditions) })
    }

    pub fn conditions_expression(&self) -> &Json {
        &self.conditions_expression
    }
}

fn js_string(value: &Json) -> String {
    match value {
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => crate::core::math::js_number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Json::String(s) => s.clone(),
        Json::Array(a) => a.iter().map(js_string).collect::<Vec<_>>().join(","),
        Json::Object(_) => "[object Object]".into(),
    }
}

impl StyleExpression for ConditionsExpression {
    fn evaluate(&self, feature: Option<&dyn StyleFeature>) -> Result<StyleValue, RuntimeError> {
        let Some(conditions) = &self.runtime_conditions else {
            return Ok(StyleValue::Undefined);
        };
        for statement in conditions {
            if statement.condition.evaluate(feature)?.to_boolean() {
                return statement.expression.evaluate(feature);
            }
        }
        Ok(StyleValue::Undefined)
    }

    fn evaluate_color(&self, feature: Option<&dyn StyleFeature>) -> Result<Option<Color>, RuntimeError> {
        let Some(conditions) = &self.runtime_conditions else {
            return Ok(None);
        };
        for statement in conditions {
            if statement.condition.evaluate(feature)?.to_boolean() {
                return statement.expression.evaluate_color(feature);
            }
        }
        Ok(None)
    }

    fn get_shader_function(
        &self,
        function_signature: &str,
        variable_substitution_map: &BTreeMap<String, String>,
        shader_state: &mut ShaderState,
        return_type: &str,
    ) -> Result<Option<String>, RuntimeError> {
        let Some(conditions) = &self.runtime_conditions else {
            return Ok(None);
        };
        if conditions.is_empty() {
            return Ok(None);
        }

        let mut shader_function = String::new();
        for (index, statement) in conditions.iter().enumerate() {
            let condition = statement
                .condition
                .get_shader_expression(variable_substitution_map, shader_state)?
                .unwrap_or_else(|| "undefined".to_string());
            let expression = statement
                .expression
                .get_shader_expression(variable_substitution_map, shader_state)?
                .unwrap_or_else(|| "undefined".to_string());
            shader_function.push_str(&format!(
                "    {} ({condition})\n    {{\n        return {expression};\n    }}\n",
                if index == 0 { "if" } else { "else if" }
            ));
        }

        Ok(Some(format!(
            "{return_type} {function_signature}\n{{\n{shader_function}    return {return_type}(1.0);\n}}\n"
        )))
    }

    fn get_variables(&self) -> Vec<String> {
        let Some(conditions) = &self.runtime_conditions else {
            return Vec::new();
        };
        let mut variables = Vec::new();
        for statement in conditions {
            variables.extend(statement.condition.get_variables());
            variables.extend(statement.expression.get_variables());
        }
        dedupe(variables)
    }

    fn json_value(&self) -> Json {
        self.conditions_expression.clone()
    }
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        if !out.contains(&value) {
            out.push(value);
        }
    }
    out
}
