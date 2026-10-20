//! CesiumJS RenderState defaults and full application, including pass overrides.
//! States are owned Rust values. Every draw reapplies state; Cesium's global
//! interning and differential-application cache are deliberately not assumed.
use super::context::*;
use crate::core::webgl_constants as gl;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cull {
    pub enabled: bool,
    pub face: u32,
}
impl Default for Cull {
    fn default() -> Self {
        Self { enabled: false, face: gl::BACK }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PolygonOffset {
    pub enabled: bool,
    pub factor: f64,
    pub units: f64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScissorTest {
    pub enabled: bool,
    pub rectangle: Viewport,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DepthRange {
    pub near: f64,
    pub far: f64,
}
impl Default for DepthRange {
    fn default() -> Self {
        Self { near: 0., far: 1. }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DepthTest {
    pub enabled: bool,
    pub func: u32,
}
impl Default for DepthTest {
    fn default() -> Self {
        Self { enabled: false, func: gl::LESS }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorMask {
    pub red: bool,
    pub green: bool,
    pub blue: bool,
    pub alpha: bool,
}
impl Default for ColorMask {
    fn default() -> Self {
        Self { red: true, green: true, blue: true, alpha: true }
    }
}
/// Blending defaults use transparent black, unlike Core Color's opaque white.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlendColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Blending {
    pub enabled: bool,
    pub color: BlendColor,
    pub equation_rgb: u32,
    pub equation_alpha: u32,
    pub function_source_rgb: u32,
    pub function_source_alpha: u32,
    pub function_destination_rgb: u32,
    pub function_destination_alpha: u32,
}
impl Default for Blending {
    fn default() -> Self {
        Self {
            enabled: false,
            color: BlendColor::default(),
            equation_rgb: gl::FUNC_ADD,
            equation_alpha: gl::FUNC_ADD,
            function_source_rgb: gl::ONE,
            function_source_alpha: gl::ONE,
            function_destination_rgb: gl::ZERO,
            function_destination_alpha: gl::ZERO,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StencilOperation {
    pub fail: u32,
    pub z_fail: u32,
    pub z_pass: u32,
}
impl Default for StencilOperation {
    fn default() -> Self {
        Self { fail: gl::KEEP, z_fail: gl::KEEP, z_pass: gl::KEEP }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StencilTest {
    pub enabled: bool,
    pub front_function: u32,
    pub back_function: u32,
    pub reference: i32,
    pub mask: i64,
    pub front_operation: StencilOperation,
    pub back_operation: StencilOperation,
}
impl Default for StencilTest {
    fn default() -> Self {
        Self {
            enabled: false,
            front_function: gl::ALWAYS,
            back_function: gl::ALWAYS,
            reference: 0,
            mask: -1,
            front_operation: StencilOperation::default(),
            back_operation: StencilOperation::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SampleCoverage {
    pub enabled: bool,
    pub value: f64,
    pub invert: bool,
}
impl Default for SampleCoverage {
    fn default() -> Self {
        Self { enabled: false, value: 1., invert: false }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RenderState {
    pub front_face: u32,
    pub cull: Cull,
    pub line_width: f64,
    pub polygon_offset: PolygonOffset,
    pub scissor_test: ScissorTest,
    pub depth_range: DepthRange,
    pub depth_test: DepthTest,
    pub color_mask: ColorMask,
    pub depth_mask: bool,
    pub stencil_mask: i64,
    pub blending: Blending,
    pub stencil_test: StencilTest,
    pub sample_coverage: SampleCoverage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewport: Option<Viewport>,
}
impl Default for RenderState {
    fn default() -> Self {
        Self {
            front_face: gl::CCW,
            cull: Cull::default(),
            line_width: 1.,
            polygon_offset: PolygonOffset::default(),
            scissor_test: ScissorTest::default(),
            depth_range: DepthRange::default(),
            depth_test: DepthTest::default(),
            color_mask: ColorMask::default(),
            depth_mask: true,
            stencil_mask: -1,
            blending: Blending::default(),
            stencil_test: StencilTest::default(),
            sample_coverage: SampleCoverage::default(),
            viewport: None,
        }
    }
}
/// Pass viewport is used when the command state has no viewport; blending and
/// scissor, when set, override command state. Framebuffers are deferred.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PassState {
    pub viewport: Option<Viewport>,
    pub blending_enabled: Option<bool>,
    pub scissor_test: Option<ScissorTest>,
}
impl RenderState {
    /// Checks independent of hardware limits. GL validates implementation limits
    /// (line width, viewport dimensions); callers can query their host first.
    pub fn validate(&self) -> Result<()> {
        ensure([gl::CW, gl::CCW].contains(&self.front_face), "Invalid front face")?;
        ensure([gl::FRONT, gl::BACK, gl::FRONT_AND_BACK].contains(&self.cull.face), "Invalid cull face")?;
        ensure(self.line_width.is_finite() && self.line_width > 0., "Invalid line width")?;
        ensure(
            self.depth_range.near.is_finite()
                && self.depth_range.far.is_finite()
                && self.depth_range.near >= 0.
                && self.depth_range.far <= 1.
                && self.depth_range.near <= self.depth_range.far,
            "Invalid depth range",
        )?;
        let cmp = |n| (gl::NEVER..=gl::ALWAYS).contains(&n);
        ensure(
            cmp(self.depth_test.func) && cmp(self.stencil_test.front_function) && cmp(self.stencil_test.back_function),
            "Invalid depth/stencil function",
        )?;
        let b = &self.blending;
        ensure(
            [b.color.red, b.color.green, b.color.blue, b.color.alpha]
                .iter()
                .all(|n| n.is_finite() && (0. ..=1.).contains(n)),
            "Invalid blend color",
        )?;
        let equation = |n| [gl::FUNC_ADD, gl::FUNC_SUBTRACT, gl::FUNC_REVERSE_SUBTRACT, gl::MIN, gl::MAX].contains(&n);
        ensure(equation(b.equation_rgb) && equation(b.equation_alpha), "Invalid blend equation")?;
        let factor = |n| {
            [
                gl::ZERO,
                gl::ONE,
                gl::SRC_COLOR,
                gl::ONE_MINUS_SRC_COLOR,
                gl::DST_COLOR,
                gl::ONE_MINUS_DST_COLOR,
                gl::SRC_ALPHA,
                gl::ONE_MINUS_SRC_ALPHA,
                gl::DST_ALPHA,
                gl::ONE_MINUS_DST_ALPHA,
                gl::CONSTANT_COLOR,
                gl::ONE_MINUS_CONSTANT_COLOR,
                gl::CONSTANT_ALPHA,
                gl::ONE_MINUS_CONSTANT_ALPHA,
                gl::SRC_ALPHA_SATURATE,
            ]
            .contains(&n)
        };
        ensure(
            [b.function_source_rgb, b.function_source_alpha, b.function_destination_rgb, b.function_destination_alpha]
                .iter()
                .all(|n| factor(*n)),
            "Invalid blend factor",
        )?;
        let op = |n| {
            [gl::ZERO, gl::KEEP, gl::REPLACE, gl::INCR, gl::DECR, gl::INVERT, gl::INCR_WRAP, gl::DECR_WRAP].contains(&n)
        };
        for o in [&self.stencil_test.front_operation, &self.stencil_test.back_operation] {
            ensure(op(o.fail) && op(o.z_fail) && op(o.z_pass), "Invalid stencil operation")?;
        }
        if let Some(v) = self.viewport {
            size::<()>(v.width as usize)?;
            size::<()>(v.height as usize)?;
        }
        Ok(())
    }
    pub fn apply<G: Gl>(&self, context: &Context<G>, pass: &PassState, drawing_size: [u32; 2]) -> Result<()> {
        self.validate()?;
        let toggle =
            |cap: u32, enabled| context.call(if enabled { "enable" } else { "disable" }, &[cap.into()]).map(|_| ());
        context.call("frontFace", &[self.front_face.into()])?;
        toggle(gl::CULL_FACE, self.cull.enabled)?;
        if self.cull.enabled {
            context.call("cullFace", &[self.cull.face.into()])?;
        }
        context.call("lineWidth", &[self.line_width.into()])?;
        toggle(gl::POLYGON_OFFSET_FILL, self.polygon_offset.enabled)?;
        if self.polygon_offset.enabled {
            context.call("polygonOffset", &[self.polygon_offset.factor.into(), self.polygon_offset.units.into()])?;
        }
        context.call("depthRange", &[self.depth_range.near.into(), self.depth_range.far.into()])?;
        toggle(gl::DEPTH_TEST, self.depth_test.enabled)?;
        if self.depth_test.enabled {
            context.call("depthFunc", &[self.depth_test.func.into()])?;
        }
        let c = &self.color_mask;
        context.call("colorMask", &[c.red.into(), c.green.into(), c.blue.into(), c.alpha.into()])?;
        context.call("depthMask", &[self.depth_mask.into()])?;
        context.call("stencilMask", &[(self.stencil_mask as u32).into()])?;
        let s = &self.stencil_test;
        toggle(gl::STENCIL_TEST, s.enabled)?;
        if s.enabled {
            context.call("stencilFunc", &[s.front_function.into(), s.reference.into(), (s.mask as u32).into()])?;
            for (face, func, op) in
                [(gl::BACK, s.back_function, &s.back_operation), (gl::FRONT, s.front_function, &s.front_operation)]
            {
                context.call(
                    "stencilFuncSeparate",
                    &[face.into(), func.into(), s.reference.into(), (s.mask as u32).into()],
                )?;
                context
                    .call("stencilOpSeparate", &[face.into(), op.fail.into(), op.z_fail.into(), op.z_pass.into()])?;
            }
        }
        let s = &self.sample_coverage;
        toggle(gl::SAMPLE_COVERAGE, s.enabled)?;
        if s.enabled {
            context.call("sampleCoverage", &[s.value.into(), s.invert.into()])?;
        }
        let s = pass.scissor_test.as_ref().unwrap_or(&self.scissor_test);
        toggle(gl::SCISSOR_TEST, s.enabled)?;
        if s.enabled {
            let v = s.rectangle;
            context.call("scissor", &[v.x.into(), v.y.into(), v.width.into(), v.height.into()])?;
        }
        let b = &self.blending;
        let enabled = pass.blending_enabled.unwrap_or(b.enabled);
        toggle(gl::BLEND, enabled)?;
        if enabled {
            context.call(
                "blendColor",
                &[b.color.red.into(), b.color.green.into(), b.color.blue.into(), b.color.alpha.into()],
            )?;
            context.call("blendEquationSeparate", &[b.equation_rgb.into(), b.equation_alpha.into()])?;
            context.call(
                "blendFuncSeparate",
                &[
                    b.function_source_rgb.into(),
                    b.function_destination_rgb.into(),
                    b.function_source_alpha.into(),
                    b.function_destination_alpha.into(),
                ],
            )?;
        }
        let v = self.viewport.or(pass.viewport).unwrap_or(Viewport {
            x: 0,
            y: 0,
            width: drawing_size[0],
            height: drawing_size[1],
        });
        context.call("viewport", &[v.x.into(), v.y.into(), v.width.into(), v.height.into()])?;
        Ok(())
    }
}
