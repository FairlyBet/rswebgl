use glam::{Mat4, Vec3, Vec4};

/// The kinds of punctual light (KHR_lights_punctual). Cone angles are in
/// radians from the light's axis, `0 <= inner < outer <= π/2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LightKind {
    Directional,
    Point,
    Spot { inner_cone: f32, outer_cone: f32 },
}

/// A punctual light. It shines down its entity's local −Z (directional,
/// spot) from the entity's world position (point, spot).
///
/// Units as in glTF: `intensity` is lux (lm/m²) for a directional light and
/// candela (lm/sr) otherwise. `range` cuts a point or spot light off smoothly
/// at that distance; 0 means no cutoff (inverse-square only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Light {
    pub kind: LightKind,
    pub color: Vec3,
    pub intensity: f32,
    pub range: f32,
}

impl Light {
    pub fn directional(color: Vec3, intensity: f32) -> Self {
        Self {
            kind: LightKind::Directional,
            color,
            intensity,
            range: 0.0,
        }
    }

    pub fn point(color: Vec3, intensity: f32, range: f32) -> Self {
        Self {
            kind: LightKind::Point,
            color,
            intensity,
            range,
        }
    }

    pub fn spot(color: Vec3, intensity: f32, range: f32, inner_cone: f32, outer_cone: f32) -> Self {
        Self {
            kind: LightKind::Spot {
                inner_cone,
                outer_cone,
            },
            color,
            intensity,
            range,
        }
    }

    /// The light as the shader's `Light` struct (4 vec4s): position,
    /// direction, color + intensity, and (range, spot scale, spot offset,
    /// type).
    pub(crate) fn packed(&self, world: &Mat4) -> [Vec4; 4] {
        let position = world.w_axis;
        let direction = (-world.z_axis.truncate()).normalize_or(Vec3::NEG_Z);
        let (scale, offset, ty) = match self.kind {
            LightKind::Directional => (0.0, 0.0, 0.0),
            LightKind::Point => (0.0, 0.0, 1.0),
            LightKind::Spot {
                inner_cone,
                outer_cone,
            } => {
                // KHR_lights_punctual's smooth cone falloff.
                let (ci, co) = (inner_cone.cos(), outer_cone.cos());
                let scale = 1.0 / (ci - co).max(0.001);
                (scale, -co * scale, 2.0)
            }
        };
        [
            position,
            direction.extend(0.0),
            self.color.extend(self.intensity),
            Vec4::new(self.range.max(0.0), scale, offset, ty),
        ]
    }
}
