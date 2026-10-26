//! CesiumJS `Scene/Model/ModelAnimationChannel`: interpolates one animated
//! node property between keyframes.
//!
//! A channel builds one spline per animated morph target, or a single spline
//! for translation, rotation and scale. STEP, LINEAR and CUBICSPLINE
//! interpolation and the single-keyframe constant case all come from the
//! existing `Core` spline ports.

use super::ModelRuntimeNode;
use crate::{
    core::{
        Cartesian3, ConstantSpline, HermiteSpline, InterpolationType, LinearSpline, Quaternion, QuaternionSpline,
        RuntimeError, Spline, SteppedSpline,
    },
    renderer::Gl,
    scene::model_components as mc,
};

/// A scalar spline (morph weights).
#[derive(Clone, Debug, PartialEq)]
pub enum ScalarSpline {
    Constant(ConstantSpline<f64>),
    Stepped(SteppedSpline<f64>),
    Linear(LinearSpline<f64>),
    Hermite(HermiteSpline<f64>),
}

/// A Cartesian3 spline (translation and scale).
#[derive(Clone, Debug, PartialEq)]
pub enum Cartesian3Spline {
    Constant(ConstantSpline<Cartesian3>),
    Stepped(SteppedSpline<Cartesian3>),
    Linear(LinearSpline<Cartesian3>),
    Hermite(HermiteSpline<Cartesian3>),
}

/// A quaternion spline (rotation).
#[derive(Clone, Debug, PartialEq)]
pub enum QuaternionAnimationSpline {
    Constant(ConstantSpline<Quaternion>),
    Stepped(SteppedSpline<Quaternion>),
    Linear(QuaternionSpline),
    /// glTF's cubic rotation interpolation is component-wise, as in Cesium's
    /// `HermiteSpline`.
    Hermite(HermiteSpline<Quaternion>),
}

macro_rules! spline_dispatch {
    ($name:ident, $value:ty) => {
        impl $name {
            pub fn times(&self) -> &[f64] {
                match self {
                    Self::Constant(s) => s.times(),
                    Self::Stepped(s) => s.times(),
                    Self::Linear(s) => s.times(),
                    Self::Hermite(s) => s.times(),
                }
            }

            pub fn evaluate(&self, time: f64) -> Option<$value> {
                match self {
                    Self::Constant(s) => s.evaluate(time),
                    Self::Stepped(s) => s.evaluate(time),
                    Self::Linear(s) => s.evaluate(time),
                    Self::Hermite(s) => s.evaluate(time),
                }
            }

            pub fn wrap_time(&self, time: f64) -> f64 {
                match self {
                    Self::Constant(s) => s.wrap_time(time),
                    Self::Stepped(s) => s.wrap_time(time),
                    Self::Linear(s) => s.wrap_time(time),
                    Self::Hermite(s) => s.wrap_time(time),
                }
            }

            pub fn clamp_time(&self, time: f64) -> f64 {
                match self {
                    Self::Constant(s) => s.clamp_time(time),
                    Self::Stepped(s) => s.clamp_time(time),
                    Self::Linear(s) => s.clamp_time(time),
                    Self::Hermite(s) => s.clamp_time(time),
                }
            }
        }
    };
}

spline_dispatch!(ScalarSpline, f64);
spline_dispatch!(Cartesian3Spline, Cartesian3);
spline_dispatch!(QuaternionAnimationSpline, Quaternion);

/// The spline of one animation channel, typed by the animated property.
#[derive(Clone, Debug, PartialEq)]
pub enum AnimationChannelSpline {
    Scalar(ScalarSpline),
    Cartesian3(Cartesian3Spline),
    Quaternion(QuaternionAnimationSpline),
}

impl AnimationChannelSpline {
    pub fn wrap_time(&self, time: f64) -> f64 {
        match self {
            Self::Scalar(s) => s.wrap_time(time),
            Self::Cartesian3(s) => s.wrap_time(time),
            Self::Quaternion(s) => s.wrap_time(time),
        }
    }

    pub fn clamp_time(&self, time: f64) -> f64 {
        match self {
            Self::Scalar(s) => s.clamp_time(time),
            Self::Cartesian3(s) => s.clamp_time(time),
            Self::Quaternion(s) => s.clamp_time(time),
        }
    }
}

/// A runtime animation channel for a [`ModelAnimation`].
///
/// [`ModelAnimation`]: super::ModelAnimation
#[derive(Clone, Debug)]
pub struct ModelAnimationChannel {
    channel: mc::AnimationChannel,
    path: mc::AnimatedPropertyType,
    runtime_node: usize,
    splines: Vec<AnimationChannelSpline>,
}

impl ModelAnimationChannel {
    /// Builds the channel's splines from its sampler output.
    ///
    /// `morph_weight_count` is the target node's morph-weight count; a weights
    /// channel produces one spline per morph target.
    pub fn new(
        channel: &mc::AnimationChannel,
        sampler: &mc::AnimationSampler,
        runtime_node: usize,
        morph_weight_count: usize,
    ) -> Result<Self, RuntimeError> {
        let target = channel.target.as_ref().ok_or_else(|| RuntimeError::new("Animation channel has no target"))?;
        let path = target.path.ok_or_else(|| RuntimeError::new("Animation channel target has no path"))?;
        let interpolation = sampler.interpolation.unwrap_or(InterpolationType::Linear);
        let splines = create_splines(&sampler.input, &sampler.output, interpolation, path, morph_weight_count.max(1))?;
        Ok(Self { channel: channel.clone(), path, runtime_node, splines })
    }

    /// The glTF animation channel.
    pub fn channel(&self) -> &mc::AnimationChannel {
        &self.channel
    }

    /// The runtime node index this channel animates.
    pub fn runtime_node(&self) -> usize {
        self.runtime_node
    }

    /// The animated property.
    pub fn path(&self) -> mc::AnimatedPropertyType {
        self.path
    }

    /// The splines used to evaluate this channel.
    pub fn splines(&self) -> &[AnimationChannelSpline] {
        &self.splines
    }

    /// Animates the target node property at the given local animation time.
    pub fn animate<G: Gl, D>(
        &self,
        time: f64,
        runtime_nodes: &mut [Option<ModelRuntimeNode<G, D>>],
        clamp_animations: bool,
    ) -> Result<(), RuntimeError> {
        let Some(node) = runtime_nodes.get_mut(self.runtime_node).and_then(Option::as_mut) else {
            return Ok(());
        };

        // Weights are updated in place; the other paths assign a node property.
        if self.path == mc::AnimatedPropertyType::Weights {
            let mut weights = node.morph_weights().to_vec();
            for (i, spline) in self.splines.iter().take(weights.len()).enumerate() {
                let AnimationChannelSpline::Scalar(spline) = spline else {
                    return Err(RuntimeError::new("Morph weight channel has a non-scalar spline"));
                };
                let local_time = if clamp_animations { spline.clamp_time(time) } else { spline.wrap_time(time) };
                weights[i] = spline
                    .evaluate(local_time)
                    .ok_or_else(|| RuntimeError::new("Animation time is outside the spline range"))?;
            }
            return node.set_morph_weights(&weights);
        }

        // If the node is being animated externally, ignore the glTF animation.
        if node.user_animated {
            return Ok(());
        }

        let Some(spline) = self.splines.first() else {
            return Ok(());
        };
        let local_time = if clamp_animations { spline.clamp_time(time) } else { spline.wrap_time(time) };
        match self.path {
            mc::AnimatedPropertyType::Translation => {
                let AnimationChannelSpline::Cartesian3(spline) = spline else {
                    return Err(RuntimeError::new("Translation channel has a non-Cartesian3 spline"));
                };
                let value = spline
                    .evaluate(local_time)
                    .ok_or_else(|| RuntimeError::new("Animation time is outside the spline range"))?;
                node.set_translation(value)
            }
            mc::AnimatedPropertyType::Scale => {
                let AnimationChannelSpline::Cartesian3(spline) = spline else {
                    return Err(RuntimeError::new("Scale channel has a non-Cartesian3 spline"));
                };
                let value = spline
                    .evaluate(local_time)
                    .ok_or_else(|| RuntimeError::new("Animation time is outside the spline range"))?;
                node.set_scale(value)
            }
            mc::AnimatedPropertyType::Rotation => {
                let AnimationChannelSpline::Quaternion(spline) = spline else {
                    return Err(RuntimeError::new("Rotation channel has a non-quaternion spline"));
                };
                let value = spline
                    .evaluate(local_time)
                    .ok_or_else(|| RuntimeError::new("Animation time is outside the spline range"))?;
                node.set_rotation(value)
            }
            mc::AnimatedPropertyType::Weights => unreachable!("handled above"),
        }
    }
}

fn create_cubic_spline<T: crate::core::hermite_spline::HermitePoint>(
    times: &[f64],
    points: &[T],
) -> Result<HermiteSpline<T>, RuntimeError> {
    let mut cubic_points = Vec::with_capacity(points.len() / 3);
    let mut in_tangents = Vec::with_capacity(points.len() / 3);
    let mut out_tangents = Vec::with_capacity(points.len() / 3);
    for triple in points.as_chunks::<3>().0 {
        in_tangents.push(triple[0]);
        cubic_points.push(triple[1]);
        out_tangents.push(triple[2]);
    }
    // The first in-tangent and last out-tangent are unused.
    if !in_tangents.is_empty() {
        in_tangents.remove(0);
    }
    if !out_tangents.is_empty() {
        out_tangents.pop();
    }
    HermiteSpline::new(times.to_vec(), cubic_points, in_tangents, out_tangents)
        .ok_or_else(|| RuntimeError::new("Invalid cubic animation spline"))
}

fn create_spline(
    times: &[f64],
    points: &[Cartesian3],
    interpolation: InterpolationType,
) -> Result<Cartesian3Spline, RuntimeError> {
    if times.len() == 1 && points.len() == 1 {
        return Ok(Cartesian3Spline::Constant(ConstantSpline::new(points[0])));
    }
    match interpolation {
        InterpolationType::Step => Ok(Cartesian3Spline::Stepped(
            SteppedSpline::new(times.to_vec(), points.to_vec())
                .ok_or_else(|| RuntimeError::new("Invalid stepped animation spline"))?,
        )),
        InterpolationType::Cubicspline => Ok(Cartesian3Spline::Hermite(create_cubic_spline(times, points)?)),
        InterpolationType::Linear => Ok(Cartesian3Spline::Linear(
            LinearSpline::new(times.to_vec(), points.to_vec())
                .ok_or_else(|| RuntimeError::new("Invalid linear animation spline"))?,
        )),
    }
}

fn create_splines(
    times: &[f64],
    points: &[mc::MathValue],
    interpolation: InterpolationType,
    path: mc::AnimatedPropertyType,
    count: usize,
) -> Result<Vec<AnimationChannelSpline>, RuntimeError> {
    if path == mc::AnimatedPropertyType::Weights {
        let output_length = points.len() / count;
        let mut splines = Vec::with_capacity(count);
        for target_index in 0..count {
            let mut output = Vec::with_capacity(output_length);
            let mut points_index = target_index;
            if interpolation == InterpolationType::Cubicspline {
                let mut i = 0;
                while i < output_length {
                    if points_index + 2 * count >= points.len() {
                        return Err(RuntimeError::new("Morph weight animation output is too short"));
                    }
                    output.push(scalar(&points[points_index])?);
                    output.push(scalar(&points[points_index + count])?);
                    output.push(scalar(&points[points_index + 2 * count])?);
                    points_index += count * 3;
                    i += 3;
                }
            } else {
                for _ in 0..output_length {
                    let value = points
                        .get(points_index)
                        .ok_or_else(|| RuntimeError::new("Morph weight animation output is too short"))?;
                    output.push(scalar(value)?);
                    points_index += count;
                }
            }
            splines.push(AnimationChannelSpline::Scalar(create_scalar_spline(times, &output, interpolation)?));
        }
        return Ok(splines);
    }

    match path {
        mc::AnimatedPropertyType::Translation | mc::AnimatedPropertyType::Scale => {
            let values = points.iter().map(cartesian3).collect::<Result<Vec<_>, _>>()?;
            Ok(vec![AnimationChannelSpline::Cartesian3(create_spline(times, &values, interpolation)?)])
        }
        mc::AnimatedPropertyType::Rotation => {
            let values = points.iter().map(quaternion).collect::<Result<Vec<_>, _>>()?;
            let spline = create_quaternion_spline(times, &values, interpolation)?;
            Ok(vec![AnimationChannelSpline::Quaternion(spline)])
        }
        mc::AnimatedPropertyType::Weights => unreachable!("handled above"),
    }
}

fn create_scalar_spline(
    times: &[f64],
    points: &[f64],
    interpolation: InterpolationType,
) -> Result<ScalarSpline, RuntimeError> {
    if times.len() == 1 && points.len() == 1 {
        return Ok(ScalarSpline::Constant(ConstantSpline::new(points[0])));
    }
    match interpolation {
        InterpolationType::Step => Ok(ScalarSpline::Stepped(
            SteppedSpline::new(times.to_vec(), points.to_vec())
                .ok_or_else(|| RuntimeError::new("Invalid stepped animation spline"))?,
        )),
        InterpolationType::Cubicspline => Ok(ScalarSpline::Hermite(create_cubic_spline(times, points)?)),
        InterpolationType::Linear => Ok(ScalarSpline::Linear(
            LinearSpline::new(times.to_vec(), points.to_vec())
                .ok_or_else(|| RuntimeError::new("Invalid linear animation spline"))?,
        )),
    }
}

fn create_quaternion_spline(
    times: &[f64],
    points: &[Quaternion],
    interpolation: InterpolationType,
) -> Result<QuaternionAnimationSpline, RuntimeError> {
    if times.len() == 1 && points.len() == 1 {
        return Ok(QuaternionAnimationSpline::Constant(ConstantSpline::new(points[0])));
    }
    match interpolation {
        InterpolationType::Step => Ok(QuaternionAnimationSpline::Stepped(
            SteppedSpline::new(times.to_vec(), points.to_vec())
                .ok_or_else(|| RuntimeError::new("Invalid stepped animation spline"))?,
        )),
        InterpolationType::Cubicspline => Ok(QuaternionAnimationSpline::Hermite(create_cubic_spline(times, points)?)),
        InterpolationType::Linear => Ok(QuaternionAnimationSpline::Linear(
            QuaternionSpline::new(times.to_vec(), points.to_vec())
                .ok_or_else(|| RuntimeError::new("Invalid quaternion animation spline"))?,
        )),
    }
}

fn scalar(value: &mc::MathValue) -> Result<f64, RuntimeError> {
    match value {
        mc::MathValue::Number(value) => Ok(*value),
        _ => Err(RuntimeError::new("Animation weight is not a number")),
    }
}

fn cartesian3(value: &mc::MathValue) -> Result<Cartesian3, RuntimeError> {
    match value {
        mc::MathValue::Cartesian3(value) => Ok(*value),
        mc::MathValue::Cartesian4(value) => Ok(Cartesian3::new(value.x, value.y, value.z)),
        _ => Err(RuntimeError::new("Animation value is not a Cartesian3")),
    }
}

fn quaternion(value: &mc::MathValue) -> Result<Quaternion, RuntimeError> {
    match value {
        mc::MathValue::Quaternion(value) => Ok(*value),
        mc::MathValue::Cartesian4(value) => Ok(Quaternion::new(value.x, value.y, value.z, value.w)),
        _ => Err(RuntimeError::new("Animation value is not a quaternion")),
    }
}
