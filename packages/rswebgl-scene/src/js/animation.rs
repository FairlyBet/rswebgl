//! A model's glTF animations for JS: list, play, stop, advance.

use rswebgl::error::Error;
use rswebgl_ecs::Entity;
use wasm_bindgen::prelude::*;

use super::Scene;
use crate::animation::{ModelAnimations, Playback, advance, apply};

#[wasm_bindgen]
impl Scene {
    /// The names of the animations of the model at `root` (`addModel`'s
    /// entity), in file order; "Animation N" for an unnamed one.
    pub fn animations(&self, root: Entity) -> Vec<String> {
        self.world
            .get::<ModelAnimations>(root)
            .map(|a| {
                a.clips
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        c.name
                            .clone()
                            .unwrap_or_else(|| format!("Animation {}", i + 1))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Seconds animation `index` of `root`'s model lasts (0 if there is none).
    #[wasm_bindgen(js_name = animationDuration)]
    pub fn animation_duration(&self, root: Entity, index: usize) -> f32 {
        self.world
            .get::<ModelAnimations>(root)
            .and_then(|a| a.clips.get(index).map(|c| c.duration))
            .unwrap_or(0.0)
    }

    /// Plays animation `index` of `root`'s model from its start, posing the
    /// model at once; `advanceAnimations` moves it on. Looping wraps at the
    /// end; otherwise it holds the last pose. Replaces what that model played.
    #[wasm_bindgen(js_name = playAnimation)]
    pub fn play_animation(
        &mut self,
        root: Entity,
        index: usize,
        looping: bool,
    ) -> Result<(), Error> {
        let Some(anims) = self
            .world
            .get::<ModelAnimations>(root)
            .map(|a| (*a).clone())
        else {
            return Err("playAnimation: no animations on that entity (a model root?)".into());
        };
        let Some(clip) = anims.clips.get(index) else {
            return Err(format!("playAnimation: no animation {index}").into());
        };
        apply(&mut self.world, clip, &anims.nodes, 0.0);
        if let Some(mut a) = self.world.get_mut::<ModelAnimations>(root) {
            a.playing = Some(Playback {
                clip: index,
                time: 0.0,
                speed: 1.0,
                looping,
            });
        }
        Ok(())
    }

    /// Stops `root`'s animation where it is (the pose stays).
    #[wasm_bindgen(js_name = stopAnimation)]
    pub fn stop_animation(&mut self, root: Entity) {
        if let Some(mut a) = self.world.get_mut::<ModelAnimations>(root) {
            a.playing = None;
        }
    }

    /// Seconds of animation per second of `advanceAnimations` (default 1;
    /// negative plays backwards).
    #[wasm_bindgen(js_name = setAnimationSpeed)]
    pub fn set_animation_speed(&mut self, root: Entity, speed: f32) {
        if let Some(mut a) = self.world.get_mut::<ModelAnimations>(root)
            && let Some(p) = a.playing.as_mut()
        {
            p.speed = speed;
        }
    }

    /// Where `root`'s animation is, in seconds; undefined when none plays.
    #[wasm_bindgen(js_name = animationTime)]
    pub fn animation_time(&self, root: Entity) -> Option<f32> {
        self.world
            .get::<ModelAnimations>(root)
            .and_then(|a| a.playing.map(|p| p.time))
    }

    /// Moves every playing animation on by `seconds` and poses its model —
    /// call once a frame with the time since the last, before `renderFrame`.
    #[wasm_bindgen(js_name = advanceAnimations)]
    pub fn advance_animations(&mut self, seconds: f32) {
        advance(&mut self.world, seconds);
    }
}

#[wasm_bindgen]
impl Scene {
    /// The morph target weights of the mesh at node `e` (a glTF node entity:
    /// `findByName`); empty without morph targets.
    #[wasm_bindgen(js_name = morphWeights)]
    pub fn morph_weights(&self, e: Entity) -> Vec<f32> {
        self.world
            .get::<crate::animation::MorphWeights>(e)
            .map(|w| w.0.clone())
            .unwrap_or_default()
    }

    /// Sets the morph target weights of the mesh at node `e` (an animation
    /// playing on its model overwrites them).
    #[wasm_bindgen(js_name = setMorphWeights)]
    pub fn set_morph_weights(&mut self, e: Entity, weights: Vec<f32>) {
        if self.world.is_alive(e) {
            self.world
                .insert(e, crate::animation::MorphWeights(weights));
        }
    }
}
