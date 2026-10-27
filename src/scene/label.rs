//! CPU-side state and screen-space geometry for CesiumJS `Label`.
//!
//! # Architecture and Parity Status
//! This module provides partial CPU-side behavior for CesiumJS `Label`:
//! - Complete configuration options, defaults, and parameter range validation matching
//!   CesiumJS `DeveloperError` preconditions.
//! - Font parsing (extracting family, size in px, style, weight, line-height) and
//!   SDF relative size and total scale computations.
//! - Bidirectional text processing:
//!   - `filter_unsupported_characters` matching upstream regex filter for control and
//!     invisible Unicode characters.
//!   - `reverse_rtl` implementing upstream bidirectional Arabic / Hebrew reordering and
//!     bracket mirroring (`()[]{}<>`).
//! - Screen-space bounding box computation matching `Label.getScreenSpaceBoundingBox`.
//! - Equivalence checking matching `Label.prototype.equals`.
//! - Dirty tracking flags (`rebind_all_glyphs`, `reposition_all_glyphs`, property bits).
//!
//! # Limitations
//! - GPU Draw & Shader Pipeline: WebGL/WebGPU resource allocation, vertex attribute packing,
//!   SDF shader execution (`LabelCollectionVS.glsl` / `LabelCollectionFS.glsl`), and draw command
//!   emission require a graphics backend.
//! - Glyph Texture Atlas: Canvas rasterization and GPU texture atlas packing are omitted on CPU;
//!   character layout and dimensions are simulated or computed from metrics without live WebGL textures.
//! - Pick IDs: Real pick IDs are not fabricated on the CPU; true pick IDs require GPU render pass integration.
//! - Terrain Clamping: Height clamping queries (`HeightReference::ClampToGround` / `RelativeToGround`)
//!   require a live Scene and Globe terrain provider.

use crate as _root;

use _root::{
    core::{
        BoundingRectangle, Cartesian2, Cartesian3, Color, DistanceDisplayCondition, NearFarScalar,
    },
    scene::{HeightReference, HorizontalOrigin, LabelStyle, VerticalOrigin},
};

/// Signed Distance Field (SDF) settings from CesiumJS `SDFSettings.js`.
pub mod sdf_settings {
    pub const FONT_SIZE: f64 = 48.0;
    pub const PADDING: f64 = 10.0;
    pub const RADIUS: f64 = 8.0;
    pub const CUTOFF: f64 = 0.25;
}

// Property update indices matching CesiumJS Label property change tracking.
pub const SHOW_INDEX: usize = 0;
pub const POSITION_INDEX: usize = 1;
pub const TEXT_INDEX: usize = 2;
pub const FONT_INDEX: usize = 3;
pub const STYLE_INDEX: usize = 4;
pub const SCALE_INDEX: usize = 5;
pub const SHOW_BACKGROUND_INDEX: usize = 6;
pub const BACKGROUND_COLOR_INDEX: usize = 7;
pub const BACKGROUND_PADDING_INDEX: usize = 8;
pub const PIXEL_OFFSET_INDEX: usize = 9;
pub const EYE_OFFSET_INDEX: usize = 10;
pub const HORIZONTAL_ORIGIN_INDEX: usize = 11;
pub const VERTICAL_ORIGIN_INDEX: usize = 12;
pub const HEIGHT_REFERENCE_INDEX: usize = 13;
pub const FILL_COLOR_INDEX: usize = 14;
pub const OUTLINE_COLOR_INDEX: usize = 15;
pub const OUTLINE_WIDTH_INDEX: usize = 16;
pub const SCALE_BY_DISTANCE_INDEX: usize = 17;
pub const TRANSLUCENCY_BY_DISTANCE_INDEX: usize = 18;
pub const PIXEL_OFFSET_SCALE_BY_DISTANCE_INDEX: usize = 19;
pub const DISTANCE_DISPLAY_CONDITION_INDEX: usize = 20;
pub const DISABLE_DEPTH_TEST_DISTANCE_INDEX: usize = 21;
pub const NUMBER_OF_PROPERTIES: usize = 22;

/// Parsed CSS font representation.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedFont {
    pub family: String,
    pub size: f64,
    pub style: String,
    pub weight: String,
    pub line_height: Option<f64>,
}

impl Default for ParsedFont {
    fn default() -> Self {
        Self {
            family: "sans-serif".to_string(),
            size: 30.0,
            style: "normal".to_string(),
            weight: "normal".to_string(),
            line_height: None,
        }
    }
}

impl ParsedFont {
    /// Parses a CSS font string into constituent typography components.
    ///
    /// Handles standard patterns such as `"30px sans-serif"`, `"italic bold 24px Helvetica"`,
    /// and `"16px/1.5 Arial"`.
    pub fn parse(font: &str) -> Self {
        let mut parsed = Self::default();
        let tokens: Vec<&str> = font.split_whitespace().collect();
        if tokens.is_empty() {
            return parsed;
        }

        let mut family_tokens = Vec::new();

        for token in tokens {
            let lower = token.to_ascii_lowercase();
            if lower == "italic" || lower == "oblique" {
                parsed.style = lower;
            } else if lower == "bold"
                || lower == "bolder"
                || lower == "lighter"
                || (lower.chars().all(|c| c.is_ascii_digit()) && lower.len() == 3)
            {
                parsed.weight = lower;
            } else if lower.contains("px")
                || lower.contains("pt")
                || (lower.contains('/') && lower.chars().next().is_some_and(|c| c.is_ascii_digit()))
            {
                // Size or size/line-height
                let (size_part, line_height_part) = if let Some((s, lh)) = token.split_once('/') {
                    (s, Some(lh))
                } else {
                    (token, None)
                };

                if let Some(px_str) = size_part.strip_suffix("px") {
                    if let Ok(v) = px_str.parse::<f64>() {
                        parsed.size = v;
                    }
                } else if let Some(pt_str) = size_part.strip_suffix("pt") {
                    if let Ok(v) = pt_str.parse::<f64>() {
                        parsed.size = v * 1.3333333333333333;
                    }
                } else if let Ok(v) = size_part.parse::<f64>() {
                    parsed.size = v;
                }

                if let Some(lh_str) = line_height_part {
                    if let Some(px_lh) = lh_str.strip_suffix("px") {
                        if let Ok(v) = px_lh.parse::<f64>() {
                            parsed.line_height = Some(v);
                        }
                    } else if let Ok(v) = lh_str.parse::<f64>() {
                        // multiplier
                        parsed.line_height = Some(v * parsed.size);
                    }
                }
            } else {
                family_tokens.push(token);
            }
        }

        if !family_tokens.is_empty() {
            parsed.family = family_tokens.join(" ").trim_matches('\'').trim_matches('"').to_string();
        }

        parsed
    }
}

/// Removes control characters and soft hyphen characters which cause rendering errors.
///
/// Matches CesiumJS:
/// `/[\\u0000-\\u0008\\u000E-\\u001F\\u00ad\\u202a-\\u206f\\u200b-\\u200f]/g`
pub fn filter_unsupported_characters(text: &str) -> String {
    text.chars()
        .filter(|&c| {
            let u = c as u32;
            !matches!(
                u,
                0x0000..=0x0008
                    | 0x000E..=0x001F
                    | 0x00AD
                    | 0x202A..=0x206F
                    | 0x200B..=0x200F
            )
        })
        .collect()
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum TextType {
    Ltr,
    Rtl,
    Weak,
    Brackets,
}

fn is_rtl_char(c: char) -> bool {
    matches!(
        c as u32,
        0x05D0..=0x05EA | 0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF
    )
}

fn is_ltr_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

fn is_bracket_char(c: char) -> bool {
    matches!(c, '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>')
}

fn reverse_bracket(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        _ => c,
    }
}

/// Reverses Right-to-Left (Hebrew/Arabic) text and mirrors bracket characters matching CesiumJS.
pub fn reverse_rtl(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut result_lines = Vec::with_capacity(lines.len());

    for line_text in lines {
        if line_text.is_empty() {
            result_lines.push(String::new());
            continue;
        }

        let rtl_dir = line_text.chars().next().is_some_and(is_rtl_char);

        // Convert line text to typed tokens
        let mut tokens: Vec<(TextType, String)> = Vec::new();
        let mut current_word = String::new();
        let mut current_type = TextType::Ltr;
        let mut first = true;

        for c in line_text.chars() {
            let t = if is_rtl_char(c) {
                TextType::Rtl
            } else if is_ltr_char(c) {
                TextType::Ltr
            } else if is_bracket_char(c) {
                TextType::Brackets
            } else {
                TextType::Weak
            };

            if first {
                current_type = t;
                first = false;
            }

            if current_type == t && t != TextType::Brackets {
                current_word.push(c);
            } else {
                if !current_word.is_empty() {
                    tokens.push((current_type, std::mem::take(&mut current_word)));
                }
                current_type = t;
                current_word.push(c);
            }
        }
        if !current_word.is_empty() {
            tokens.push((current_type, current_word));
        }

        let mut line = String::new();
        let mut splice_pointer = 0usize;

        for word_index in 0..tokens.len() {
            let (token_type, ref word) = tokens[word_index];
            let reverse: String = if token_type == TextType::Brackets {
                word.chars().map(reverse_bracket).collect()
            } else {
                word.chars().rev().collect()
            };

            if rtl_dir {
                if token_type == TextType::Rtl {
                    line = format!("{}{}", reverse, line);
                    splice_pointer = 0;
                } else if token_type == TextType::Ltr {
                    line.insert_str(splice_pointer, word);
                    splice_pointer += word.len();
                } else if token_type == TextType::Weak || token_type == TextType::Brackets {
                    if token_type == TextType::Weak
                        && word_index > 0
                        && tokens[word_index - 1].0 == TextType::Brackets
                    {
                        line = format!("{}{}", reverse, line);
                    } else if word_index > 0 && tokens[word_index - 1].0 == TextType::Rtl {
                        line = format!("{}{}", reverse, line);
                        splice_pointer = 0;
                    } else if word_index + 1 < tokens.len() {
                        if tokens[word_index + 1].0 == TextType::Rtl {
                            line = format!("{}{}", reverse, line);
                            splice_pointer = 0;
                        } else {
                            line.insert_str(splice_pointer, word);
                            splice_pointer += word.len();
                        }
                    } else {
                        line.insert_str(0, &reverse);
                    }
                }
            } else if token_type == TextType::Rtl {
                line.insert_str(splice_pointer, &reverse);
            } else if token_type == TextType::Ltr {
                line.push_str(word);
                splice_pointer = line.len();
            } else if token_type == TextType::Weak || token_type == TextType::Brackets {
                if word_index > 0 {
                    if tokens[word_index - 1].0 == TextType::Rtl {
                        if word_index + 1 < tokens.len() && tokens[word_index + 1].0 == TextType::Rtl {
                            line.insert_str(splice_pointer, &reverse);
                        } else {
                            line.push_str(word);
                            splice_pointer = line.len();
                        }
                    } else {
                        line.push_str(word);
                        splice_pointer = line.len();
                    }
                } else {
                    line.push_str(word);
                    splice_pointer = line.len();
                }
            }
        }

        result_lines.push(line);
    }

    result_lines.join("\n")
}

/// Initialization and mutation options for [`Label`].
#[derive(Clone, Debug, PartialEq)]
pub struct LabelOptions {
    pub show: bool,
    pub position: Cartesian3,
    pub text: String,
    pub font: String,
    pub style: LabelStyle,
    pub scale: f64,
    pub show_background: bool,
    pub background_color: Color,
    pub background_padding: Cartesian2,
    pub pixel_offset: Cartesian2,
    pub eye_offset: Cartesian3,
    pub horizontal_origin: HorizontalOrigin,
    pub vertical_origin: VerticalOrigin,
    pub height_reference: HeightReference,
    pub fill_color: Color,
    pub outline_color: Color,
    pub outline_width: f64,
    pub scale_by_distance: Option<NearFarScalar>,
    pub translucency_by_distance: Option<NearFarScalar>,
    pub pixel_offset_scale_by_distance: Option<NearFarScalar>,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
    pub disable_depth_test_distance: Option<f64>,
    pub id: Option<String>,
}

impl Default for LabelOptions {
    fn default() -> Self {
        Self {
            show: true,
            position: Cartesian3::ZERO,
            text: String::new(),
            font: "30px sans-serif".to_string(),
            style: LabelStyle::Fill,
            scale: 1.0,
            show_background: false,
            background_color: Color::new(0.165, 0.165, 0.165, 0.8),
            background_padding: Cartesian2::new(7.0, 5.0),
            pixel_offset: Cartesian2::ZERO,
            eye_offset: Cartesian3::ZERO,
            horizontal_origin: HorizontalOrigin::Left,
            vertical_origin: VerticalOrigin::Baseline,
            height_reference: HeightReference::None,
            fill_color: Color::WHITE,
            outline_color: Color::BLACK,
            outline_width: 1.0,
            scale_by_distance: None,
            translucency_by_distance: None,
            pixel_offset_scale_by_distance: None,
            distance_display_condition: None,
            disable_depth_test_distance: None,
            id: None,
        }
    }
}

impl LabelOptions {
    /// Validates parameter ranges matching CesiumJS `DeveloperError` preconditions.
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(distance) = self.disable_depth_test_distance {
            if distance < 0.0 {
                return Err("disableDepthTestDistance must be greater than or equal to 0.0.");
            }
        }
        if let Some(translucency) = self.translucency_by_distance {
            if translucency.far <= translucency.near {
                return Err("translucencyByDistance.far must be greater than translucencyByDistance.near.");
            }
        }
        if let Some(pixel_offset_scale) = self.pixel_offset_scale_by_distance {
            if pixel_offset_scale.far <= pixel_offset_scale.near {
                return Err("pixelOffsetScaleByDistance.far must be greater than pixelOffsetScaleByDistance.near.");
            }
        }
        if let Some(scale) = self.scale_by_distance {
            if scale.far <= scale.near {
                return Err("scaleByDistance.far must be greater than scaleByDistance.near.");
            }
        }
        if let Some(condition) = self.distance_display_condition {
            if condition.far <= condition.near {
                return Err("distanceDisplayCondition.far must be greater than distanceDisplayCondition.near.");
            }
        }
        Ok(())
    }
}

/// CPU-side state and metadata for a single label.
#[derive(Debug)]
pub struct Label {
    options: LabelOptions,
    parsed_font: ParsedFont,
    relative_size: f64,
    rendered_text: String,
    enable_rtl: bool,
    actual_position: Cartesian3,
    clamped_position: Option<Cartesian3>,
    cluster_show: bool,
    attached: bool,
    collection_index: usize,
    dirty: bool,
    rebind_all_glyphs: bool,
    reposition_all_glyphs: bool,
    properties_changed: [bool; NUMBER_OF_PROPERTIES],
    // CPU simulated layout metrics
    pub(crate) background_dimensions: Option<Cartesian2>,
    pub(crate) background_translate: Option<Cartesian2>,
}

impl Label {
    pub(crate) fn new(
        options: LabelOptions,
        index: usize,
        enable_rtl: bool,
    ) -> Result<Self, &'static str> {
        options.validate()?;
        let parsed_font = ParsedFont::parse(&options.font);
        let relative_size = parsed_font.size / sdf_settings::FONT_SIZE;
        let filtered = filter_unsupported_characters(&options.text);
        let rendered_text = if enable_rtl {
            reverse_rtl(&filtered)
        } else {
            filtered
        };

        let actual_position = options.position;

        Ok(Self {
            options,
            parsed_font,
            relative_size,
            rendered_text,
            enable_rtl,
            actual_position,
            clamped_position: None,
            cluster_show: true,
            attached: true,
            collection_index: index,
            dirty: true,
            rebind_all_glyphs: true,
            reposition_all_glyphs: true,
            properties_changed: [true; NUMBER_OF_PROPERTIES],
            background_dimensions: None,
            background_translate: None,
        })
    }

    pub fn options(&self) -> &LabelOptions {
        &self.options
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    pub(crate) fn detach(&mut self) {
        self.attached = false;
        self.collection_index = usize::MAX;
    }

    pub fn collection_index(&self) -> usize {
        self.collection_index
    }

    pub(crate) fn set_collection_index(&mut self, index: usize) {
        self.collection_index = index;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn rebind_all_glyphs(&self) -> bool {
        self.rebind_all_glyphs
    }

    pub fn reposition_all_glyphs(&self) -> bool {
        self.reposition_all_glyphs
    }

    pub(crate) fn clear_dirty(&mut self) {
        self.dirty = false;
        self.rebind_all_glyphs = false;
        self.reposition_all_glyphs = false;
        self.properties_changed.fill(false);
    }

    pub fn is_property_changed(&self, index: usize) -> bool {
        self.properties_changed.get(index).copied().unwrap_or(false)
    }

    fn mark_dirty(&mut self, property_index: usize) {
        self.dirty = true;
        if property_index < NUMBER_OF_PROPERTIES {
            self.properties_changed[property_index] = true;
        }
    }

    pub fn parsed_font(&self) -> &ParsedFont {
        &self.parsed_font
    }

    pub fn relative_size(&self) -> f64 {
        self.relative_size
    }

    pub fn total_scale(&self) -> f64 {
        self.options.scale * self.relative_size
    }

    pub fn rendered_text(&self) -> &str {
        &self.rendered_text
    }

    pub fn show(&self) -> bool {
        self.options.show
    }

    pub fn set_show(&mut self, value: bool) {
        if self.options.show != value {
            self.options.show = value;
            self.mark_dirty(SHOW_INDEX);
        }
    }

    pub fn position(&self) -> Cartesian3 {
        self.options.position
    }

    pub fn set_position(&mut self, value: Cartesian3) {
        if self.options.position != value {
            self.options.position = value;
            self.actual_position = value;
            self.clamped_position = None;
            self.mark_dirty(POSITION_INDEX);
        }
    }

    pub fn actual_position(&self) -> Cartesian3 {
        self.clamped_position.unwrap_or(self.actual_position)
    }

    pub fn clamped_position(&self) -> Option<Cartesian3> {
        self.clamped_position
    }

    pub fn set_clamped_position(&mut self, value: Option<Cartesian3>) {
        self.clamped_position = value;
        self.mark_dirty(POSITION_INDEX);
    }

    pub(crate) fn set_rtl_detection(&mut self, enable: bool) {
        self.enable_rtl = enable;
    }

    pub fn text(&self) -> &str {
        &self.options.text
    }

    pub fn set_text(&mut self, value: String) {
        if self.options.text != value {
            self.options.text = value;
            let filtered = filter_unsupported_characters(&self.options.text);
            self.rendered_text = if self.enable_rtl {
                reverse_rtl(&filtered)
            } else {
                filtered
            };
            self.rebind_all_glyphs = true;
            self.reposition_all_glyphs = true;
            self.mark_dirty(TEXT_INDEX);
        }
    }

    pub fn font(&self) -> &str {
        &self.options.font
    }

    pub fn set_font(&mut self, value: String) {
        if self.options.font != value {
            self.options.font = value;
            self.parsed_font = ParsedFont::parse(&self.options.font);
            self.relative_size = self.parsed_font.size / sdf_settings::FONT_SIZE;
            self.rebind_all_glyphs = true;
            self.reposition_all_glyphs = true;
            self.mark_dirty(FONT_INDEX);
        }
    }

    pub fn style(&self) -> LabelStyle {
        self.options.style
    }

    pub fn set_style(&mut self, value: LabelStyle) {
        if self.options.style != value {
            self.options.style = value;
            self.rebind_all_glyphs = true;
            self.mark_dirty(STYLE_INDEX);
        }
    }

    pub fn scale(&self) -> f64 {
        self.options.scale
    }

    pub fn set_scale(&mut self, value: f64) {
        if self.options.scale != value {
            self.options.scale = value;
            self.reposition_all_glyphs = true;
            self.mark_dirty(SCALE_INDEX);
        }
    }

    pub fn show_background(&self) -> bool {
        self.options.show_background
    }

    pub fn set_show_background(&mut self, value: bool) {
        if self.options.show_background != value {
            self.options.show_background = value;
            self.rebind_all_glyphs = true;
            self.reposition_all_glyphs = true;
            self.mark_dirty(SHOW_BACKGROUND_INDEX);
        }
    }

    pub fn background_color(&self) -> Color {
        self.options.background_color
    }

    pub fn set_background_color(&mut self, value: Color) {
        if self.options.background_color != value {
            self.options.background_color = value;
            self.mark_dirty(BACKGROUND_COLOR_INDEX);
        }
    }

    pub fn background_padding(&self) -> Cartesian2 {
        self.options.background_padding
    }

    pub fn set_background_padding(&mut self, value: Cartesian2) {
        if self.options.background_padding != value {
            self.options.background_padding = value;
            self.reposition_all_glyphs = true;
            self.mark_dirty(BACKGROUND_PADDING_INDEX);
        }
    }

    pub fn pixel_offset(&self) -> Cartesian2 {
        self.options.pixel_offset
    }

    pub fn set_pixel_offset(&mut self, value: Cartesian2) {
        if self.options.pixel_offset != value {
            self.options.pixel_offset = value;
            self.mark_dirty(PIXEL_OFFSET_INDEX);
        }
    }

    pub fn eye_offset(&self) -> Cartesian3 {
        self.options.eye_offset
    }

    pub fn set_eye_offset(&mut self, value: Cartesian3) {
        if self.options.eye_offset != value {
            self.options.eye_offset = value;
            self.mark_dirty(EYE_OFFSET_INDEX);
        }
    }

    pub fn horizontal_origin(&self) -> HorizontalOrigin {
        self.options.horizontal_origin
    }

    pub fn set_horizontal_origin(&mut self, value: HorizontalOrigin) {
        if self.options.horizontal_origin != value {
            self.options.horizontal_origin = value;
            self.reposition_all_glyphs = true;
            self.mark_dirty(HORIZONTAL_ORIGIN_INDEX);
        }
    }

    pub fn vertical_origin(&self) -> VerticalOrigin {
        self.options.vertical_origin
    }

    pub fn set_vertical_origin(&mut self, value: VerticalOrigin) {
        if self.options.vertical_origin != value {
            self.options.vertical_origin = value;
            self.reposition_all_glyphs = true;
            self.mark_dirty(VERTICAL_ORIGIN_INDEX);
        }
    }

    pub fn height_reference(&self) -> HeightReference {
        self.options.height_reference
    }

    pub fn set_height_reference(&mut self, value: HeightReference) {
        if self.options.height_reference != value {
            self.options.height_reference = value;
            self.clamped_position = None;
            self.reposition_all_glyphs = true;
            self.mark_dirty(HEIGHT_REFERENCE_INDEX);
        }
    }

    pub fn fill_color(&self) -> Color {
        self.options.fill_color
    }

    pub fn set_fill_color(&mut self, value: Color) {
        if self.options.fill_color != value {
            self.options.fill_color = value;
            self.rebind_all_glyphs = true;
            self.mark_dirty(FILL_COLOR_INDEX);
        }
    }

    pub fn outline_color(&self) -> Color {
        self.options.outline_color
    }

    pub fn set_outline_color(&mut self, value: Color) {
        if self.options.outline_color != value {
            self.options.outline_color = value;
            self.rebind_all_glyphs = true;
            self.mark_dirty(OUTLINE_COLOR_INDEX);
        }
    }

    pub fn outline_width(&self) -> f64 {
        self.options.outline_width
    }

    pub fn set_outline_width(&mut self, value: f64) {
        if self.options.outline_width != value {
            self.options.outline_width = value;
            self.rebind_all_glyphs = true;
            self.mark_dirty(OUTLINE_WIDTH_INDEX);
        }
    }

    pub fn scale_by_distance(&self) -> Option<NearFarScalar> {
        self.options.scale_by_distance
    }

    pub fn set_scale_by_distance(
        &mut self,
        value: Option<NearFarScalar>,
    ) -> Result<(), &'static str> {
        if let Some(scale) = value {
            if scale.far <= scale.near {
                return Err("scaleByDistance.far must be greater than scaleByDistance.near.");
            }
        }
        if self.options.scale_by_distance != value {
            self.options.scale_by_distance = value;
            self.mark_dirty(SCALE_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn translucency_by_distance(&self) -> Option<NearFarScalar> {
        self.options.translucency_by_distance
    }

    pub fn set_translucency_by_distance(
        &mut self,
        value: Option<NearFarScalar>,
    ) -> Result<(), &'static str> {
        if let Some(translucency) = value {
            if translucency.far <= translucency.near {
                return Err(
                    "translucencyByDistance.far must be greater than translucencyByDistance.near.",
                );
            }
        }
        if self.options.translucency_by_distance != value {
            self.options.translucency_by_distance = value;
            self.mark_dirty(TRANSLUCENCY_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn pixel_offset_scale_by_distance(&self) -> Option<NearFarScalar> {
        self.options.pixel_offset_scale_by_distance
    }

    pub fn set_pixel_offset_scale_by_distance(
        &mut self,
        value: Option<NearFarScalar>,
    ) -> Result<(), &'static str> {
        if let Some(offset_scale) = value {
            if offset_scale.far <= offset_scale.near {
                return Err(
                    "pixelOffsetScaleByDistance.far must be greater than pixelOffsetScaleByDistance.near.",
                );
            }
        }
        if self.options.pixel_offset_scale_by_distance != value {
            self.options.pixel_offset_scale_by_distance = value;
            self.mark_dirty(PIXEL_OFFSET_SCALE_BY_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn distance_display_condition(&self) -> Option<DistanceDisplayCondition> {
        self.options.distance_display_condition
    }

    pub fn set_distance_display_condition(
        &mut self,
        value: Option<DistanceDisplayCondition>,
    ) -> Result<(), &'static str> {
        if let Some(condition) = value {
            if condition.far <= condition.near {
                return Err(
                    "distanceDisplayCondition.far must be greater than distanceDisplayCondition.near.",
                );
            }
        }
        if self.options.distance_display_condition != value {
            self.options.distance_display_condition = value;
            self.mark_dirty(DISTANCE_DISPLAY_CONDITION_INDEX);
        }
        Ok(())
    }

    pub fn disable_depth_test_distance(&self) -> Option<f64> {
        self.options.disable_depth_test_distance
    }

    pub fn set_disable_depth_test_distance(
        &mut self,
        value: Option<f64>,
    ) -> Result<(), &'static str> {
        if let Some(dist) = value {
            if dist < 0.0 {
                return Err("disableDepthTestDistance must be greater than or equal to 0.0.");
            }
        }
        if self.options.disable_depth_test_distance != value {
            self.options.disable_depth_test_distance = value;
            self.mark_dirty(DISABLE_DEPTH_TEST_DISTANCE_INDEX);
        }
        Ok(())
    }

    pub fn id(&self) -> Option<&str> {
        self.options.id.as_deref()
    }

    pub fn set_id(&mut self, value: Option<String>) {
        self.options.id = value;
    }

    pub fn cluster_show(&self) -> bool {
        self.cluster_show
    }

    pub fn set_cluster_show(&mut self, value: bool) {
        if self.cluster_show != value {
            self.cluster_show = value;
            self.mark_dirty(SHOW_INDEX);
        }
    }

    /// Computes the 2D screen space bounding box centered around `screen_space_position`.
    ///
    /// Uses simulated background dimensions. Actual glyph bounds and the
    /// no-background case require font metrics and remain unimplemented.
    pub fn screen_space_bounding_box(
        &self,
        screen_space_position: Cartesian2,
    ) -> BoundingRectangle {
        let scale = self.total_scale();
        let bg_trans = self.background_translate.unwrap_or(Cartesian2::ZERO);
        let bg_dim = self.background_dimensions.unwrap_or(Cartesian2::ZERO);

        let x = screen_space_position.x + bg_trans.x;
        let mut y = screen_space_position.y - bg_trans.y;
        let width = bg_dim.x * scale;
        let height = bg_dim.y * scale;

        match self.options.vertical_origin {
            VerticalOrigin::Bottom | VerticalOrigin::Baseline => {
                y -= height;
            }
            VerticalOrigin::Center => {
                y -= height * 0.5;
            }
            VerticalOrigin::Top => {}
        }

        BoundingRectangle::new(x, y, width, height)
    }

    /// Parity implementation of CesiumJS `Label.prototype.equals`.
    pub fn equals(&self, other: &Self) -> bool {
        self.options.show == other.options.show
            && self.options.scale == other.options.scale
            && self.options.outline_width == other.options.outline_width
            && self.options.show_background == other.options.show_background
            && self.options.style == other.options.style
            && self.options.vertical_origin == other.options.vertical_origin
            && self.options.horizontal_origin == other.options.horizontal_origin
            && self.options.height_reference == other.options.height_reference
            && self.rendered_text == other.rendered_text
            && self.options.font == other.options.font
            && self.options.position == other.options.position
            && self.options.fill_color == other.options.fill_color
            && self.options.outline_color == other.options.outline_color
            && self.options.background_color == other.options.background_color
            && self.options.background_padding == other.options.background_padding
            && self.options.pixel_offset == other.options.pixel_offset
            && self.options.eye_offset == other.options.eye_offset
            && self.options.translucency_by_distance == other.options.translucency_by_distance
            && self.options.pixel_offset_scale_by_distance
                == other.options.pixel_offset_scale_by_distance
            && self.options.scale_by_distance == other.options.scale_by_distance
            && self.options.distance_display_condition
                == other.options.distance_display_condition
            && self.options.disable_depth_test_distance
                == other.options.disable_depth_test_distance
            && self.options.id == other.options.id
    }
}

impl PartialEq for Label {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other)
    }
}
