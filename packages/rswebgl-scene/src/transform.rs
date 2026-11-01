use glam::{Mat4, Quat, Vec3};
use rswebgl_ecs::Entity;

/// Local position, rotation and scale — relative to the [`Parent`] if the
/// entity has one, to the world otherwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub const IDENTITY: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: Vec3::ONE,
    };

    pub fn from_translation(translation: Vec3) -> Self {
        Self {
            translation,
            ..Self::IDENTITY
        }
    }

    pub fn from_rotation(rotation: Quat) -> Self {
        Self {
            rotation,
            ..Self::IDENTITY
        }
    }

    pub fn from_scale(scale: Vec3) -> Self {
        Self {
            scale,
            ..Self::IDENTITY
        }
    }

    /// From a matrix with no shear (glTF node matrices are such).
    pub fn from_matrix(m: Mat4) -> Self {
        let (scale, rotation, translation) = m.to_scale_rotation_translation();
        Self {
            translation,
            rotation,
            scale,
        }
    }

    /// Scale, then rotate, then translate.
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// The entity whose transform this one's is relative to. Set it with
/// [`crate::set_parent`], which refuses cycles; a parent that is dead or has no
/// [`Transform`] makes the entity a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parent(pub Entity);

/// The entity's world matrix: parent's world matrix × local transform.
/// [`crate::TransformSystem`] adds and writes it; changes are stamped with the
/// system's tick, so a renderer can pick up only the moved objects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldMatrix(pub Mat4);

impl Default for WorldMatrix {
    fn default() -> Self {
        Self(Mat4::IDENTITY)
    }
}
