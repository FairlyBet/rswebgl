//! Playing glTF animations: a channel sampled at a time (glTF 2.0 Appendix C
//! — step; linear, spherical for rotations; cubic Hermite splines) and the
//! result written into the node entities' `Transform`s and `MorphWeights`.

use std::collections::HashMap;
use std::rc::Rc;

use glam::{Quat, Vec3};
use rswebgl_ecs::{Entity, World};
use rswebgl_gltf::{Animation, AnimationChannel, AnimationPath, Interpolation};

use crate::transform::Transform;

/// Morph target weights of the mesh on this (node) entity; its primitives
/// read them. Animations write them.
#[derive(Debug, Clone, PartialEq)]
pub struct MorphWeights(pub Vec<f32>);

/// A model's animations, on its root entity: the clips, which entity each
/// glTF node became, and what plays.
#[derive(Clone)]
pub struct ModelAnimations {
    pub clips: Rc<[Animation]>,
    pub nodes: HashMap<u32, Entity>,
    pub playing: Option<Playback>,
}

/// A clip playing: where it is and how it goes on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playback {
    pub clip: usize,
    /// Seconds into the clip.
    pub time: f32,
    /// Seconds of clip per second of `advance`; negative plays backwards.
    pub speed: f32,
    /// Wraps at the end (else holds the last pose).
    pub looping: bool,
}

/// `channel`'s value at `t` seconds into `out` (`channel.components` floats):
/// held at the first and last keyframes outside their range.
pub fn sample(channel: &AnimationChannel, t: f32, out: &mut [f32]) {
    let c = channel.components;
    let times = &channel.times;
    let cubic = channel.interpolation == Interpolation::CubicSpline;
    // A keyframe's value (the middle of in-tangent, value, out-tangent for
    // a spline).
    let value = |k: usize| {
        let at = if cubic { (k * 3 + 1) * c } else { k * c };
        &channel.values[at..at + c]
    };
    let last = times.len() - 1;
    if t <= times[0] || last == 0 {
        out[..c].copy_from_slice(value(0));
        return;
    }
    if t >= times[last] {
        out[..c].copy_from_slice(value(last));
        return;
    }
    // The keyframe interval holding t: times[k] <= t < times[k + 1].
    let k = times.partition_point(|&x| x <= t) - 1;
    let dt = times[k + 1] - times[k];
    let s = if dt > 0.0 { (t - times[k]) / dt } else { 0.0 };
    let rotation = channel.path == AnimationPath::Rotation;
    match channel.interpolation {
        Interpolation::Step => out[..c].copy_from_slice(value(k)),
        Interpolation::Linear if rotation => {
            let q0 = Quat::from_slice(value(k));
            let q1 = Quat::from_slice(value(k + 1));
            // glam's slerp takes the shorter way round (q and −q are the
            // same rotation).
            out[..4].copy_from_slice(&q0.slerp(q1, s).normalize().to_array());
        }
        Interpolation::Linear => {
            let (a, b) = (value(k), value(k + 1));
            for i in 0..c {
                out[i] = a[i] + (b[i] - a[i]) * s;
            }
        }
        Interpolation::CubicSpline => {
            // Appendix C: p(s) = (2s³ − 3s² + 1)·v0 + (s³ − 2s² + s)·dt·b0
            //                  + (−2s³ + 3s²)·v1 + (s³ − s²)·dt·a1,
            // with b0 the out-tangent of key k and a1 the in-tangent of k+1.
            let v0 = value(k);
            let v1 = value(k + 1);
            let b0 = &channel.values[(k * 3 + 2) * c..][..c];
            let a1 = &channel.values[(k + 1) * 3 * c..][..c];
            let (s2, s3) = (s * s, s * s * s);
            let h00 = 2.0 * s3 - 3.0 * s2 + 1.0;
            let h10 = s3 - 2.0 * s2 + s;
            let h01 = -2.0 * s3 + 3.0 * s2;
            let h11 = s3 - s2;
            for i in 0..c {
                out[i] = h00 * v0[i] + h10 * dt * b0[i] + h01 * v1[i] + h11 * dt * a1[i];
            }
            if rotation {
                let q = Quat::from_slice(&out[..4]).normalize();
                out[..4].copy_from_slice(&q.to_array());
            }
        }
    }
}

/// Poses the nodes of `anim` at `t` seconds: each channel's node entity
/// (looked up in `nodes`) gets its sampled translation, rotation, scale or
/// morph weights.
pub fn apply(world: &mut World, anim: &Animation, nodes: &HashMap<u32, Entity>, t: f32) {
    let mut buf = Vec::new();
    for channel in &anim.channels {
        let Some(&e) = nodes.get(&channel.node) else {
            continue;
        };
        buf.resize(channel.components.max(4), 0.0);
        sample(channel, t, &mut buf);
        match channel.path {
            AnimationPath::Weights => {
                let weights = MorphWeights(buf[..channel.components].to_vec());
                match world.get_mut::<MorphWeights>(e) {
                    Some(mut w) => w.set_if_neq(weights),
                    None => {
                        world.insert(e, weights);
                    }
                }
            }
            path => {
                let Some(mut t) = world.get_mut::<Transform>(e) else {
                    continue;
                };
                let mut next = *t;
                match path {
                    AnimationPath::Translation => next.translation = Vec3::from_slice(&buf),
                    AnimationPath::Rotation => next.rotation = Quat::from_slice(&buf).normalize(),
                    AnimationPath::Scale => next.scale = Vec3::from_slice(&buf),
                    AnimationPath::Weights => unreachable!(),
                }
                t.set_if_neq(next);
            }
        }
    }
}

/// Moves every playing animation on by `dt` seconds and poses its nodes.
pub fn advance(world: &mut World, dt: f32) {
    let playing: Vec<(Entity, ModelAnimations)> = match world.storage::<ModelAnimations>() {
        Some(s) => s
            .iter()
            .filter(|(_, a)| a.playing.is_some())
            .map(|(e, a)| (e, a.clone()))
            .collect(),
        None => return,
    };
    for (root, mut anims) in playing {
        let Some(mut p) = anims.playing else {
            continue;
        };
        let Some(clip) = anims.clips.get(p.clip) else {
            continue;
        };
        let duration = clip.duration;
        p.time += dt * p.speed;
        p.time = if p.looping && duration > 0.0 {
            p.time.rem_euclid(duration)
        } else {
            p.time.clamp(0.0, duration)
        };
        apply(world, clip, &anims.nodes, p.time);
        anims.playing = Some(p);
        if let Some(mut a) = world.get_mut::<ModelAnimations>(root) {
            a.playing = anims.playing;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(
        path: AnimationPath,
        interpolation: Interpolation,
        times: &[f32],
        values: &[f32],
        components: usize,
    ) -> AnimationChannel {
        AnimationChannel {
            node: 0,
            path,
            interpolation,
            times: times.to_vec(),
            values: values.to_vec(),
            components,
        }
    }

    fn at(c: &AnimationChannel, t: f32) -> Vec<f32> {
        let mut out = vec![0.0; c.components.max(4)];
        sample(c, t, &mut out);
        out.truncate(c.components);
        out
    }

    #[test]
    fn linear_step_and_clamping() {
        let c = channel(
            AnimationPath::Translation,
            Interpolation::Linear,
            &[1.0, 3.0],
            &[0.0, 0.0, 0.0, 2.0, 4.0, -2.0],
            3,
        );
        assert_eq!(at(&c, 2.0), [1.0, 2.0, -1.0]);
        assert_eq!(at(&c, 0.0), [0.0, 0.0, 0.0], "held before the first key");
        assert_eq!(at(&c, 9.0), [2.0, 4.0, -2.0], "held after the last");
        let step = AnimationChannel {
            interpolation: Interpolation::Step,
            ..c
        };
        assert_eq!(at(&step, 2.9), [0.0, 0.0, 0.0]);
        assert_eq!(at(&step, 3.0), [2.0, 4.0, -2.0]);
    }

    #[test]
    fn rotations_slerp_the_short_way() {
        let a = Quat::from_rotation_y(0.0);
        // The same 90° turn, stored as −q: still a 45° halfway point.
        let b = -Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let mut values = a.to_array().to_vec();
        values.extend(b.to_array());
        let c = channel(
            AnimationPath::Rotation,
            Interpolation::Linear,
            &[0.0, 1.0],
            &values,
            4,
        );
        let q = Quat::from_slice(&at(&c, 0.5));
        let expected = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
        assert!(q.angle_between(expected) < 1e-4, "{q:?}");
    }

    #[test]
    fn cubic_splines_follow_their_tangents() {
        // One component: v0 = 0, v1 = 1 over dt = 2; out-tangent of key 0
        // and in-tangent of key 1 both 0.5 (slope per second).
        let values = [0.0, 0.0, 0.5, 0.5, 1.0, 0.0];
        let c = channel(
            AnimationPath::Weights,
            Interpolation::CubicSpline,
            &[0.0, 2.0],
            &values,
            1,
        );
        // Tangents dt·0.5 = 1 = the chord: the spline is the straight line.
        for (t, v) in [(0.5, 0.25), (1.0, 0.5), (1.5, 0.75)] {
            assert!((at(&c, t)[0] - v).abs() < 1e-6, "{t}: {:?}", at(&c, t));
        }
        // Flat tangents ease in and out: below the line early, above late.
        let flat = AnimationChannel {
            values: vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            ..c
        };
        assert!(at(&flat, 0.5)[0] < 0.25 && at(&flat, 1.5)[0] > 0.75);
        assert!((at(&flat, 1.0)[0] - 0.5).abs() < 1e-6);
        assert_eq!(at(&flat, 2.0), [1.0], "the value, not a tangent, at a key");
    }
}
