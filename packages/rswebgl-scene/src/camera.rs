use glam::Mat4;

/// How a [`Camera`] projects; depth maps to GL's `[-1, 1]` clip range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// `fov_y` in radians.
    Perspective { fov_y: f32, near: f32, far: f32 },
    /// `half_height` of the view volume in world units; the width follows
    /// from the aspect.
    Orthographic {
        half_height: f32,
        near: f32,
        far: f32,
    },
}

/// A camera: looks down its entity's local −Z, with +Y up; the view matrix is
/// the inverse of the entity's `WorldMatrix`.
///
/// `aspect` (width / height) is the app's to keep in sync with the target —
/// a camera doesn't know which target it renders to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub projection: Projection,
    pub aspect: f32,
}

impl Camera {
    pub fn perspective(fov_y: f32, aspect: f32, near: f32, far: f32) -> Self {
        Self {
            projection: Projection::Perspective { fov_y, near, far },
            aspect,
        }
    }

    pub fn orthographic(half_height: f32, aspect: f32, near: f32, far: f32) -> Self {
        Self {
            projection: Projection::Orthographic {
                half_height,
                near,
                far,
            },
            aspect,
        }
    }

    /// `projection_matrix`, or under reverse-Z (`Context::set_reverse_z`) its
    /// reverse zero-to-one form: depth 1 at the near plane; a perspective one
    /// reaches to infinity (`far` unused).
    pub fn projection_for(&self, reverse_z: bool) -> Mat4 {
        if !reverse_z {
            return self.projection_matrix();
        }
        let aspect = if self.aspect > 0.0 { self.aspect } else { 1.0 };
        match self.projection {
            Projection::Perspective { fov_y, near, .. } => {
                Mat4::perspective_infinite_reverse_rh(fov_y, aspect, near)
            }
            Projection::Orthographic {
                half_height,
                near,
                far,
            } => {
                let hw = half_height * aspect;
                // Zero-to-one, near and far swapped: near maps to 1.
                Mat4::orthographic_rh(-hw, hw, -half_height, half_height, far, near)
            }
        }
    }

    pub fn projection_matrix(&self) -> Mat4 {
        // A zero or negative aspect (a 0×0 canvas) would make NaNs.
        let aspect = if self.aspect > 0.0 { self.aspect } else { 1.0 };
        match self.projection {
            Projection::Perspective { fov_y, near, far } => {
                Mat4::perspective_rh_gl(fov_y, aspect, near, far)
            }
            Projection::Orthographic {
                half_height,
                near,
                far,
            } => {
                let hw = half_height * aspect;
                Mat4::orthographic_rh_gl(-hw, hw, -half_height, half_height, near, far)
            }
        }
    }
}
