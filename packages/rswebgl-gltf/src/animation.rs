//! glTF animations as plain data: per channel, the target node and property,
//! the keyframe times and values, and how to interpolate between them
//! (glTF 2.0 §3.11 and Appendix C). Sampling and applying them is the
//! scene's (`rswebgl-scene` `animation.rs`).

use gltf::animation::util::ReadOutputs;

/// What a channel animates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimationPath {
    /// The node's translation: 3 floats a value.
    Translation,
    /// Its rotation, a unit quaternion (x, y, z, w): 4 floats a value.
    Rotation,
    /// Its scale: 3 floats a value.
    Scale,
    /// The morph target weights of its mesh: one float per target a value.
    Weights,
}

/// Between keyframes (glTF 2.0 Appendix C).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Each keyframe's value holds until the next.
    Step,
    /// Linear; spherical (slerp) for rotations.
    Linear,
    /// Cubic Hermite spline: each keyframe stores an in-tangent, the value
    /// and an out-tangent, in that order.
    CubicSpline,
}

/// One animated property of one node.
#[derive(Clone, Debug)]
pub struct AnimationChannel {
    /// glTF node index.
    pub node: u32,
    pub path: AnimationPath,
    pub interpolation: Interpolation,
    /// Keyframe times in seconds, increasing.
    pub times: Vec<f32>,
    /// The values, flattened: per keyframe `components` floats (three
    /// groups of them for a cubic spline: in-tangent, value, out-tangent).
    pub values: Vec<f32>,
    /// Floats per value: 3, 4, or the number of morph targets.
    pub components: usize,
}

/// A named set of channels played together.
#[derive(Clone, Debug)]
pub struct Animation {
    pub name: Option<String>,
    pub channels: Vec<AnimationChannel>,
    /// The last keyframe time over all channels, in seconds.
    pub duration: f32,
}

// Every animation of the document. A channel the file can't give values for
// (bad accessors) is left out; so is one with no keyframes.
pub(crate) fn read_animations(doc: &gltf::Document, buffers: &[Vec<u8>]) -> Vec<Animation> {
    doc.animations()
        .map(|anim| {
            let mut channels = Vec::new();
            for channel in anim.channels() {
                let reader = channel.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));
                let Some(times) = reader.read_inputs().map(|i| i.collect::<Vec<f32>>()) else {
                    continue;
                };
                let (path, values, components) = match reader.read_outputs() {
                    Some(ReadOutputs::Translations(v)) => {
                        (AnimationPath::Translation, v.flatten().collect(), 3)
                    }
                    Some(ReadOutputs::Rotations(v)) => {
                        (AnimationPath::Rotation, v.into_f32().flatten().collect(), 4)
                    }
                    Some(ReadOutputs::Scales(v)) => {
                        (AnimationPath::Scale, v.flatten().collect(), 3)
                    }
                    Some(ReadOutputs::MorphTargetWeights(v)) => {
                        let values: Vec<f32> = v.into_f32().collect();
                        let per_key = if channel.sampler().interpolation()
                            == gltf::animation::Interpolation::CubicSpline
                        {
                            3
                        } else {
                            1
                        };
                        let n = if times.is_empty() {
                            0
                        } else {
                            values.len() / (times.len() * per_key)
                        };
                        (AnimationPath::Weights, values, n)
                    }
                    None => continue,
                };
                let interpolation = match channel.sampler().interpolation() {
                    gltf::animation::Interpolation::Step => Interpolation::Step,
                    gltf::animation::Interpolation::Linear => Interpolation::Linear,
                    gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
                };
                let per_key = if interpolation == Interpolation::CubicSpline {
                    3
                } else {
                    1
                };
                if times.is_empty()
                    || components == 0
                    || values.len() != times.len() * per_key * components
                {
                    continue;
                }
                channels.push(AnimationChannel {
                    node: channel.target().node().index() as u32,
                    path,
                    interpolation,
                    times,
                    values,
                    components,
                });
            }
            let duration = channels
                .iter()
                .filter_map(|c| c.times.last().copied())
                .fold(0.0f32, f32::max);
            Animation {
                name: anim.name().map(str::to_owned),
                channels,
                duration,
            }
        })
        .collect()
}
