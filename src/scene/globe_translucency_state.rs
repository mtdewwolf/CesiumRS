//! Frame and tile state for CesiumJS globe translucency.

use super::{GlobeTranslucency, SceneMode};
use crate::core::{Cartesian4, NearFarScalar, Rectangle};
use crate::renderer::{RenderState, ShaderProgramOptions, ShaderSource};
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlobeTranslucencyCommand {
    OpaqueFront,
    OpaqueBack,
    DepthFront,
    DepthBack,
    DepthBoth,
    TranslucentFront,
    TranslucentBack,
    ManualDepthFront,
    ManualDepthBack,
    PickFront,
    PickBack,
}

impl GlobeTranslucencyCommand {
    pub(crate) fn is_depth(self) -> bool {
        matches!(self, Self::DepthFront | Self::DepthBack | Self::DepthBoth)
    }

    pub(crate) fn is_translucent(self) -> bool {
        matches!(self, Self::TranslucentFront | Self::TranslucentBack | Self::ManualDepthFront | Self::ManualDepthBack)
    }

    pub(crate) fn is_pick(self) -> bool {
        matches!(self, Self::PickFront | Self::PickBack)
    }

    pub(crate) fn requires_manual_depth(self) -> bool {
        matches!(self, Self::ManualDepthFront | Self::ManualDepthBack)
    }

    fn is_back(self) -> bool {
        matches!(
            self,
            Self::OpaqueBack | Self::DepthBack | Self::TranslucentBack | Self::ManualDepthBack | Self::PickBack
        )
    }

    pub fn render_state(self, mut state: RenderState) -> RenderState {
        use crate::core::webgl_constants as gl;
        state.cull.enabled = self != Self::DepthBoth;
        state.cull.face = if self.is_back() { gl::FRONT } else { gl::BACK };
        if self.is_depth() {
            state.color_mask =
                crate::renderer::render_state::ColorMask { red: false, green: false, blue: false, alpha: false };
        } else if self.is_translucent() {
            state.depth_mask = false;
            state.blending = super::blending_state::alpha_blend();
        } else if self.is_pick() {
            state.blending.enabled = false;
        }
        state
    }

    pub fn shader(self, mut source: ShaderProgramOptions, classification: bool) -> ShaderProgramOptions {
        let vs = &mut source.vertex_shader_source;
        let fs = &mut source.fragment_shader_source;
        if matches!(self, Self::OpaqueFront | Self::OpaqueBack) {
            vs.defines.retain(|d| d != "TRANSLUCENT");
            fs.defines.retain(|d| d != "TRANSLUCENT");
        }
        if matches!(self, Self::OpaqueBack | Self::TranslucentBack | Self::ManualDepthBack) {
            vs.defines.retain(|d| d != "FOG" && d != "GROUND_ATMOSPHERE");
            fs.defines.retain(|d| d != "FOG" && d != "GROUND_ATMOSPHERE");
        }
        if self.is_depth()
            && !fs.defines.iter().any(|d| {
                matches!(d.as_str(), "TILE_LIMIT_RECTANGLE" | "ENABLE_CLIPPING_PLANES" | "ENABLE_CLIPPING_POLYGONS")
            })
        {
            fs.sources = vec!["void main() { out_FragColor = vec4(1.0); }".into()];
        }
        if self.is_translucent() && (classification || self.requires_manual_depth()) {
            for part in &mut fs.sources {
                *part = ShaderSource::replace_main(part, "czm_globe_translucency_main");
            }
            if classification {
                fs.defines.push("GLOBE_CLASSIFICATION".into());
            }
            if self.requires_manual_depth() {
                vs.defines.push("GENERATE_POSITION".into());
                fs.defines.push("MANUAL_DEPTH_TEST".into());
            }
            fs.sources.push(TRANSLUCENT_MAIN.into());
        }
        if self.is_pick() && classification {
            fs.sources = vec![PICK_MAIN.into()];
        }
        source
    }
}

const TRANSLUCENT_MAIN: &str = r#"
#ifdef GLOBE_CLASSIFICATION
uniform sampler2D u_classificationTexture;
#endif
void main() {
    vec2 st = (gl_FragCoord.xy - czm_viewport.xy) / czm_viewport.zw;
#ifdef MANUAL_DEPTH_TEST
    float depth = czm_unpackDepth(texture(czm_globeDepthTexture, st));
    if (depth != 0.0) {
        vec4 eye = czm_windowToEyeCoordinates(gl_FragCoord.xy, depth);
        if (v_positionEC.z < eye.z / eye.w) { discard; }
    }
#endif
    czm_globe_translucency_main();
#ifdef GLOBE_CLASSIFICATION
    vec4 c = texture(u_classificationTexture, st);
    if (c.a > 0.0) { c.rgb /= c.a; }
    out_FragColor = c * vec4(c.aaa, 1.0) + out_FragColor * (1.0 - c.a);
#endif
}
"#;

const PICK_MAIN: &str = r#"
uniform sampler2D u_classificationTexture;
void main() {
    vec4 c = texture(u_classificationTexture, (gl_FragCoord.xy - czm_viewport.xy) / czm_viewport.zw);
    if (c == vec4(0.0)) { discard; }
    out_FragColor = c;
}
"#;

#[derive(Clone, Copy, Debug)]
pub struct GlobeTranslucencyFrame {
    pub show: bool,
    pub base_alpha: f64,
    pub camera_underground: bool,
    pub depth_test_against_terrain: bool,
    pub depth_texture: bool,
    pub mode: SceneMode,
}

#[derive(Clone, Debug)]
pub struct GlobeTranslucencyState {
    pub front_face_alpha_by_distance: NearFarScalar,
    pub back_face_alpha_by_distance: NearFarScalar,
    pub front_face_translucent: bool,
    pub back_face_translucent: bool,
    pub sun_visible_through_globe: bool,
    pub environment_visible: bool,
    pub use_depth_plane: bool,
    pub requires_manual_depth_test: bool,
    pub number_of_texture_uniforms: usize,
    pub rectangle: Rectangle,
    mode: SceneMode,
    camera_underground: bool,
}

impl Default for GlobeTranslucencyState {
    fn default() -> Self {
        Self {
            front_face_alpha_by_distance: NearFarScalar::new(0.0, 1.0, 0.0, 1.0),
            back_face_alpha_by_distance: NearFarScalar::new(0.0, 1.0, 0.0, 1.0),
            front_face_translucent: false,
            back_face_translucent: false,
            sun_visible_through_globe: false,
            environment_visible: false,
            use_depth_plane: false,
            requires_manual_depth_test: false,
            number_of_texture_uniforms: 0,
            rectangle: Rectangle::MAX_VALUE,
            mode: SceneMode::Scene3D,
            camera_underground: false,
        }
    }
}

impl GlobeTranslucencyState {
    pub fn update(&mut self, translucency: &GlobeTranslucency, frame: GlobeTranslucencyFrame) {
        self.mode = frame.mode;
        self.camera_underground = frame.camera_underground;
        if !frame.show {
            self.front_face_translucent = false;
            self.back_face_translucent = false;
            self.sun_visible_through_globe = true;
            self.environment_visible = true;
            self.use_depth_plane = false;
            return;
        }
        update_alpha(
            &mut self.front_face_alpha_by_distance,
            translucency.enabled,
            translucency.front_face_alpha(),
            translucency.front_face_alpha_by_distance(),
        );
        update_alpha(
            &mut self.back_face_alpha_by_distance,
            translucency.enabled,
            translucency.back_face_alpha(),
            translucency.back_face_alpha_by_distance(),
        );
        let translucent = |alpha: NearFarScalar| {
            translucency.enabled && (frame.base_alpha < 1.0 || alpha.near_value < 1.0 || alpha.far_value < 1.0)
        };
        self.front_face_translucent = translucent(self.front_face_alpha_by_distance);
        self.back_face_translucent = translucent(self.back_face_alpha_by_distance);
        self.sun_visible_through_globe =
            self.front_face_translucent && (frame.camera_underground || self.back_face_translucent);
        self.environment_visible = !frame.camera_underground || self.front_face_translucent;
        self.use_depth_plane = !frame.camera_underground && !self.front_face_translucent;
        self.requires_manual_depth_test = self.front_face_translucent
            && !self.back_face_translucent
            && !frame.depth_test_against_terrain
            && frame.mode != SceneMode::Scene2D
            && frame.depth_texture;
        self.number_of_texture_uniforms =
            usize::from(self.front_face_translucent) + usize::from(self.requires_manual_depth_test);
        self.rectangle = translucency.rectangle();
    }

    pub fn translucent(&self) -> bool {
        self.front_face_translucent
    }

    pub fn tile_rectangle(&self, tile: &Rectangle) -> Option<Cartesian4> {
        self.translucent().then(|| localize_rectangle(tile, &self.rectangle)).flatten()
    }

    pub fn command_types(&self, blend: bool, pick: bool) -> Vec<GlobeTranslucencyCommand> {
        use GlobeTranslucencyCommand::*;
        if !self.translucent() || (pick && blend) {
            return Vec::new();
        }
        let [front, back] = if pick {
            [PickFront, PickBack]
        } else if self.requires_manual_depth_test {
            [ManualDepthFront, ManualDepthBack]
        } else {
            [TranslucentFront, TranslucentBack]
        };
        if self.mode == SceneMode::Scene2D {
            return vec![DepthFront, front];
        }
        let mut types = Vec::with_capacity(3);
        if self.back_face_translucent {
            if !blend {
                types.push(DepthBoth);
            }
            types.extend(if self.camera_underground { [front, back] } else { [back, front] });
        } else if self.camera_underground {
            if !blend {
                types.push(DepthBack);
            }
            types.extend([OpaqueFront, back]);
        } else {
            if !blend {
                types.push(DepthFront);
            }
            types.extend([OpaqueBack, front]);
        }
        types
    }
}

fn update_alpha(result: &mut NearFarScalar, enabled: bool, alpha: f64, distance: Option<NearFarScalar>) {
    if enabled {
        if let Some(distance) = distance {
            *result = NearFarScalar::new(
                distance.near,
                distance.near_value * alpha,
                distance.far,
                distance.far_value * alpha,
            );
            return;
        }
    }
    let alpha = if enabled { alpha } else { 1.0 };
    result.near_value = alpha;
    result.far_value = alpha;
}

pub(crate) fn localize_rectangle(tile: &Rectangle, region: &Rectangle) -> Option<Cartesian4> {
    if *region == Rectangle::MAX_VALUE {
        return Some(Cartesian4::new(0.0, 0.0, 1.0, 1.0));
    }
    let width = tile.width();
    let height = tile.height();
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let south = tile.south.max(region.south);
    let north = tile.north.min(region.north);
    let mut region = *region;
    if region.east < region.west && tile.east >= tile.west {
        if tile.center().longitude > 0.0 {
            region.east = PI;
        } else {
            region.west = -PI;
        }
    }
    let east = tile.west + width;
    let region_width = region.width();
    let (west, east) = [-TAU, 0.0, TAU]
        .into_iter()
        .filter_map(|offset| {
            let west = tile.west.max(region.west + offset);
            let east = east.min(region.west + offset + region_width);
            (west < east).then_some((west, east))
        })
        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))?;
    (south < north).then(|| {
        Cartesian4::new(
            (west - tile.west) / width,
            (south - tile.south) / height,
            (east - tile.west) / width,
            (north - tile.south) / height,
        )
    })
}
