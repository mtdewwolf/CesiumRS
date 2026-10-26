//! CesiumJS `Cesium3DTileStyle`: the `show`, `color`, `pointSize` and label
//! expressions applied to a 3D Tiles tileset.
//!
//! The style JSON is kept and updated by the setters exactly as upstream does,
//! including normalizing booleans and numbers to expression strings.

use super::conditions_expression::ConditionsExpression;
use super::expression::{Expression, ShaderState};
use super::style_expression::StyleExpression;
use crate::core::RuntimeError;
use serde_json::{Map as JsonMap, Value as Json};
use std::collections::BTreeMap;
use std::rc::Rc;

/// A value assigned to a style property. Cesium also accepts a custom object
/// with an `evaluate` method; use [`StyleInput::Expression`] for that.
#[derive(Clone)]
pub enum StyleInput {
    Boolean(bool),
    Number(f64),
    String(String),
    Conditions(Vec<(String, String)>),
    Expression(Rc<dyn StyleExpression>),
    Custom(Json),
}

#[derive(Clone)]
struct CustomExpression(Json);

impl StyleExpression for CustomExpression {
    fn evaluate(
        &self,
        _feature: Option<&dyn super::expression::StyleFeature>,
    ) -> Result<super::expression::StyleValue, RuntimeError> {
        Ok(super::expression::StyleValue::Undefined)
    }

    fn evaluate_color(
        &self,
        _feature: Option<&dyn super::expression::StyleFeature>,
    ) -> Result<Option<crate::core::Color>, RuntimeError> {
        Ok(None)
    }

    fn get_shader_function(
        &self,
        _function_signature: &str,
        _variable_substitution_map: &BTreeMap<String, String>,
        _shader_state: &mut ShaderState,
        _return_type: &str,
    ) -> Result<Option<String>, RuntimeError> {
        Ok(None)
    }

    fn get_variables(&self) -> Vec<String> {
        Vec::new()
    }

    fn json_value(&self) -> Json {
        self.0.clone()
    }
}

impl StyleInput {
    pub fn from_json(value: &Json) -> Result<Option<Self>, RuntimeError> {
        match value {
            Json::Null => Ok(None),
            Json::Bool(b) => Ok(Some(Self::Boolean(*b))),
            Json::Number(n) => Ok(Some(Self::Number(n.as_f64().unwrap_or(f64::NAN)))),
            Json::String(s) => Ok(Some(Self::String(s.clone()))),
            Json::Object(_) => {
                if let Some(conditions) = value.get("conditions").and_then(Json::as_array) {
                    let conditions = conditions
                        .iter()
                        .map(|statement| {
                            let statement = statement
                                .as_array()
                                .ok_or_else(|| RuntimeError::new("conditions statements must be arrays"))?;
                            let condition = statement.first().map(js_string).unwrap_or_else(|| "true".into());
                            let expression = statement.get(1).map(js_string).unwrap_or_else(|| "undefined".into());
                            Ok((condition, expression))
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(Some(Self::Conditions(conditions)))
                } else {
                    Ok(Some(Self::Custom(value.clone())))
                }
            }
            Json::Array(_) => Ok(Some(Self::Custom(value.clone()))),
        }
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

/// CesiumJS `Cesium3DTileStyle`.
#[derive(Clone)]
pub struct Cesium3DTileStyle {
    style: Json,
    ready: bool,

    show: Option<Rc<dyn StyleExpression>>,
    color: Option<Rc<dyn StyleExpression>>,
    point_size: Option<Rc<dyn StyleExpression>>,
    point_outline_color: Option<Rc<dyn StyleExpression>>,
    point_outline_width: Option<Rc<dyn StyleExpression>>,
    label_color: Option<Rc<dyn StyleExpression>>,
    label_outline_color: Option<Rc<dyn StyleExpression>>,
    label_outline_width: Option<Rc<dyn StyleExpression>>,
    font: Option<Rc<dyn StyleExpression>>,
    label_style: Option<Rc<dyn StyleExpression>>,
    label_text: Option<Rc<dyn StyleExpression>>,
    background_color: Option<Rc<dyn StyleExpression>>,
    background_padding: Option<Rc<dyn StyleExpression>>,
    background_enabled: Option<Rc<dyn StyleExpression>>,
    scale_by_distance: Option<Rc<dyn StyleExpression>>,
    translucency_by_distance: Option<Rc<dyn StyleExpression>>,
    distance_display_condition: Option<Rc<dyn StyleExpression>>,
    height_offset: Option<Rc<dyn StyleExpression>>,
    anchor_line_enabled: Option<Rc<dyn StyleExpression>>,
    anchor_line_color: Option<Rc<dyn StyleExpression>>,
    image: Option<Rc<dyn StyleExpression>>,
    disable_depth_test_distance: Option<Rc<dyn StyleExpression>>,
    horizontal_origin: Option<Rc<dyn StyleExpression>>,
    vertical_origin: Option<Rc<dyn StyleExpression>>,
    label_horizontal_origin: Option<Rc<dyn StyleExpression>>,
    label_vertical_origin: Option<Rc<dyn StyleExpression>>,
    meta: BTreeMap<String, Expression>,

    color_shader_function: Option<String>,
    show_shader_function: Option<String>,
    point_size_shader_function: Option<String>,
    color_shader_function_ready: bool,
    show_shader_function_ready: bool,
    point_size_shader_function_ready: bool,
    color_shader_translucent: bool,
}

impl std::fmt::Debug for Cesium3DTileStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cesium3DTileStyle").field("style", &self.style).finish()
    }
}

macro_rules! style_field {
    ($get:ident, $set:ident, $key:literal, $field:ident) => {
        pub fn $get(&self) -> Option<&Rc<dyn StyleExpression>> {
            self.$field.as_ref()
        }
        pub fn $set(&mut self, value: Option<StyleInput>) -> Result<(), RuntimeError> {
            let expression = self.make_expression(value.as_ref())?;
            self.$field = expression;
            let expression = self.$field.clone();
            self.set_style_value($key, expression.as_ref());
            Ok(())
        }
    };
}

impl PartialEq for Cesium3DTileStyle {
    fn eq(&self, other: &Self) -> bool {
        self.style == other.style
    }
}

impl Default for Cesium3DTileStyle {
    fn default() -> Self {
        Self::new(None).expect("empty style never fails")
    }
}

impl Cesium3DTileStyle {
    pub fn new(style: Option<&Json>) -> Result<Self, RuntimeError> {
        let style = match style {
            Some(Json::Object(object)) => Json::Object(object.clone()),
            Some(_) | None => Json::Object(JsonMap::new()),
        };
        let mut result = Self {
            style,
            ready: false,
            show: None,
            color: None,
            point_size: None,
            point_outline_color: None,
            point_outline_width: None,
            label_color: None,
            label_outline_color: None,
            label_outline_width: None,
            font: None,
            label_style: None,
            label_text: None,
            background_color: None,
            background_padding: None,
            background_enabled: None,
            scale_by_distance: None,
            translucency_by_distance: None,
            distance_display_condition: None,
            height_offset: None,
            anchor_line_enabled: None,
            anchor_line_color: None,
            image: None,
            disable_depth_test_distance: None,
            horizontal_origin: None,
            vertical_origin: None,
            label_horizontal_origin: None,
            label_vertical_origin: None,
            meta: BTreeMap::new(),
            color_shader_function: None,
            show_shader_function: None,
            point_size_shader_function: None,
            color_shader_function_ready: false,
            show_shader_function_ready: false,
            point_size_shader_function_ready: false,
            color_shader_translucent: false,
        };
        result.setup()?;
        Ok(result)
    }

    fn setup(&mut self) -> Result<(), RuntimeError> {
        let style = self.style.clone();
        self.set_show(StyleInput::from_json(style.get("show").unwrap_or(&Json::Null))?)?;
        self.set_color(StyleInput::from_json(style.get("color").unwrap_or(&Json::Null))?)?;
        self.set_point_size(StyleInput::from_json(style.get("pointSize").unwrap_or(&Json::Null))?)?;
        self.set_point_outline_color(StyleInput::from_json(style.get("pointOutlineColor").unwrap_or(&Json::Null))?)?;
        self.set_point_outline_width(StyleInput::from_json(style.get("pointOutlineWidth").unwrap_or(&Json::Null))?)?;
        self.set_label_color(StyleInput::from_json(style.get("labelColor").unwrap_or(&Json::Null))?)?;
        self.set_label_outline_color(StyleInput::from_json(style.get("labelOutlineColor").unwrap_or(&Json::Null))?)?;
        self.set_label_outline_width(StyleInput::from_json(style.get("labelOutlineWidth").unwrap_or(&Json::Null))?)?;
        self.set_label_style(StyleInput::from_json(style.get("labelStyle").unwrap_or(&Json::Null))?)?;
        self.set_font(StyleInput::from_json(style.get("font").unwrap_or(&Json::Null))?)?;
        self.set_label_text(StyleInput::from_json(style.get("labelText").unwrap_or(&Json::Null))?)?;
        self.set_background_color(StyleInput::from_json(style.get("backgroundColor").unwrap_or(&Json::Null))?)?;
        self.set_background_padding(StyleInput::from_json(style.get("backgroundPadding").unwrap_or(&Json::Null))?)?;
        self.set_background_enabled(StyleInput::from_json(style.get("backgroundEnabled").unwrap_or(&Json::Null))?)?;
        self.set_scale_by_distance(StyleInput::from_json(style.get("scaleByDistance").unwrap_or(&Json::Null))?)?;
        self.set_translucency_by_distance(StyleInput::from_json(
            style.get("translucencyByDistance").unwrap_or(&Json::Null),
        )?)?;
        self.set_distance_display_condition(StyleInput::from_json(
            style.get("distanceDisplayCondition").unwrap_or(&Json::Null),
        )?)?;
        self.set_height_offset(StyleInput::from_json(style.get("heightOffset").unwrap_or(&Json::Null))?)?;
        self.set_anchor_line_enabled(StyleInput::from_json(style.get("anchorLineEnabled").unwrap_or(&Json::Null))?)?;
        self.set_anchor_line_color(StyleInput::from_json(style.get("anchorLineColor").unwrap_or(&Json::Null))?)?;
        self.set_image(StyleInput::from_json(style.get("image").unwrap_or(&Json::Null))?)?;
        self.set_disable_depth_test_distance(StyleInput::from_json(
            style.get("disableDepthTestDistance").unwrap_or(&Json::Null),
        )?)?;
        self.set_horizontal_origin(StyleInput::from_json(style.get("horizontalOrigin").unwrap_or(&Json::Null))?)?;
        self.set_vertical_origin(StyleInput::from_json(style.get("verticalOrigin").unwrap_or(&Json::Null))?)?;
        self.set_label_horizontal_origin(StyleInput::from_json(
            style.get("labelHorizontalOrigin").unwrap_or(&Json::Null),
        )?)?;
        self.set_label_vertical_origin(StyleInput::from_json(
            style.get("labelVerticalOrigin").unwrap_or(&Json::Null),
        )?)?;

        let mut meta = BTreeMap::new();
        if let Some(Json::Object(meta_json)) = style.get("meta") {
            let defines = self.defines();
            for (property, value) in meta_json {
                let text = value.as_str().ok_or_else(|| RuntimeError::new("meta style values must be strings"))?;
                meta.insert(property.clone(), Expression::new(text, defines.as_ref())?);
            }
        }
        self.meta = meta;
        self.ready = true;
        Ok(())
    }

    pub fn style(&self) -> &Json {
        &self.style
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    fn defines(&self) -> Option<BTreeMap<String, String>> {
        let defines = self.style.get("defines")?.as_object()?;
        Some(defines.iter().map(|(key, value)| (key.clone(), js_string(value))).collect())
    }

    fn make_expression(&self, value: Option<&StyleInput>) -> Result<Option<Rc<dyn StyleExpression>>, RuntimeError> {
        let Some(value) = value else {
            return Ok(None);
        };
        let defines = self.defines();
        match value {
            StyleInput::Boolean(value) => Ok(Some(Rc::new(Expression::new(&value.to_string(), None)?))),
            StyleInput::Number(value) => {
                Ok(Some(Rc::new(Expression::new(&crate::core::math::js_number_to_string(*value), None)?)))
            }
            StyleInput::String(value) => Ok(Some(Rc::new(Expression::new(value, defines.as_ref())?))),
            StyleInput::Conditions(conditions) => {
                Ok(Some(Rc::new(ConditionsExpression::new(conditions, defines.as_ref())?)))
            }
            StyleInput::Expression(expression) => Ok(Some(Rc::clone(expression))),
            StyleInput::Custom(json) => Ok(Some(Rc::new(CustomExpression(json.clone())))),
        }
    }

    fn set_style_value(&mut self, key: &str, expression: Option<&Rc<dyn StyleExpression>>) {
        if let Json::Object(object) = &mut self.style {
            if let Some(expression) = expression {
                object.insert(key.to_string(), expression.json_value());
            } else {
                object.remove(key);
            }
        }
        match key {
            "show" => self.show_shader_function_ready = false,
            "color" => self.color_shader_function_ready = false,
            "pointSize" => self.point_size_shader_function_ready = false,
            _ => {}
        }
    }

    style_field!(show, set_show, "show", show);
    style_field!(color, set_color, "color", color);
    style_field!(point_size, set_point_size, "pointSize", point_size);
    style_field!(point_outline_color, set_point_outline_color, "pointOutlineColor", point_outline_color);
    style_field!(point_outline_width, set_point_outline_width, "pointOutlineWidth", point_outline_width);
    style_field!(label_color, set_label_color, "labelColor", label_color);
    style_field!(label_outline_color, set_label_outline_color, "labelOutlineColor", label_outline_color);
    style_field!(label_outline_width, set_label_outline_width, "labelOutlineWidth", label_outline_width);
    style_field!(font, set_font, "font", font);
    style_field!(label_style, set_label_style, "labelStyle", label_style);
    style_field!(label_text, set_label_text, "labelText", label_text);
    style_field!(background_color, set_background_color, "backgroundColor", background_color);
    style_field!(background_padding, set_background_padding, "backgroundPadding", background_padding);
    style_field!(background_enabled, set_background_enabled, "backgroundEnabled", background_enabled);
    style_field!(scale_by_distance, set_scale_by_distance, "scaleByDistance", scale_by_distance);
    style_field!(
        translucency_by_distance,
        set_translucency_by_distance,
        "translucencyByDistance",
        translucency_by_distance
    );
    style_field!(
        distance_display_condition,
        set_distance_display_condition,
        "distanceDisplayCondition",
        distance_display_condition
    );
    style_field!(height_offset, set_height_offset, "heightOffset", height_offset);
    style_field!(anchor_line_enabled, set_anchor_line_enabled, "anchorLineEnabled", anchor_line_enabled);
    style_field!(anchor_line_color, set_anchor_line_color, "anchorLineColor", anchor_line_color);
    style_field!(image, set_image, "image", image);
    style_field!(
        disable_depth_test_distance,
        set_disable_depth_test_distance,
        "disableDepthTestDistance",
        disable_depth_test_distance
    );
    style_field!(horizontal_origin, set_horizontal_origin, "horizontalOrigin", horizontal_origin);
    style_field!(vertical_origin, set_vertical_origin, "verticalOrigin", vertical_origin);
    style_field!(
        label_horizontal_origin,
        set_label_horizontal_origin,
        "labelHorizontalOrigin",
        label_horizontal_origin
    );
    style_field!(label_vertical_origin, set_label_vertical_origin, "labelVerticalOrigin", label_vertical_origin);

    pub fn meta(&self) -> &BTreeMap<String, Expression> {
        &self.meta
    }

    pub fn set_meta(&mut self, meta: BTreeMap<String, Expression>) {
        self.meta = meta;
    }

    /// Cesium's `getColorShaderFunction`. The cache and the cached
    /// translucency flag match upstream.
    pub fn get_color_shader_function(
        &mut self,
        function_signature: &str,
        variable_substitution_map: &BTreeMap<String, String>,
        shader_state: &mut ShaderState,
    ) -> Result<Option<String>, RuntimeError> {
        if self.color_shader_function_ready {
            shader_state.translucent = self.color_shader_translucent;
            return Ok(self.color_shader_function.clone());
        }
        self.color_shader_function_ready = true;
        self.color_shader_function = match &self.color {
            Some(color) => {
                color.get_shader_function(function_signature, variable_substitution_map, shader_state, "vec4")?
            }
            None => None,
        };
        self.color_shader_translucent = shader_state.translucent;
        Ok(self.color_shader_function.clone())
    }

    pub fn get_show_shader_function(
        &mut self,
        function_signature: &str,
        variable_substitution_map: &BTreeMap<String, String>,
        shader_state: &mut ShaderState,
    ) -> Result<Option<String>, RuntimeError> {
        if self.show_shader_function_ready {
            return Ok(self.show_shader_function.clone());
        }
        self.show_shader_function_ready = true;
        self.show_shader_function = match &self.show {
            Some(show) => {
                show.get_shader_function(function_signature, variable_substitution_map, shader_state, "bool")?
            }
            None => None,
        };
        Ok(self.show_shader_function.clone())
    }

    pub fn get_point_size_shader_function(
        &mut self,
        function_signature: &str,
        variable_substitution_map: &BTreeMap<String, String>,
        shader_state: &mut ShaderState,
    ) -> Result<Option<String>, RuntimeError> {
        if self.point_size_shader_function_ready {
            return Ok(self.point_size_shader_function.clone());
        }
        self.point_size_shader_function_ready = true;
        self.point_size_shader_function = match &self.point_size {
            Some(point_size) => {
                point_size.get_shader_function(function_signature, variable_substitution_map, shader_state, "float")?
            }
            None => None,
        };
        Ok(self.point_size_shader_function.clone())
    }

    pub fn get_variables(&self) -> Vec<String> {
        let mut variables = Vec::new();
        if let Some(color) = &self.color {
            variables.extend(color.get_variables());
        }
        if let Some(show) = &self.show {
            variables.extend(show.get_variables());
        }
        if let Some(point_size) = &self.point_size {
            variables.extend(point_size.get_variables());
        }
        let mut unique = Vec::new();
        for variable in variables {
            if !unique.contains(&variable) {
                unique.push(variable);
            }
        }
        unique
    }
}
