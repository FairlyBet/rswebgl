//! Rays and picking by bounds, on the CPU. The scene picks on the GPU
//! (`SceneRenderer::pick`: exact, asynchronous); this is the cheap,
//! synchronous approximation — the meshes' boxes — and the ray maths.

use glam::{Mat4, Vec2, Vec3};
use rswebgl_ecs::{Entity, World};

use crate::camera::Camera;
use crate::culling::Bounds;
use crate::render::{MeshRenderer, Visible, hidden};
use crate::transform::WorldMatrix;

/// A ray: `origin + t·dir`, `t ≥ 0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub dir: Vec3,
}

impl Ray {
    /// The ray through `ndc` (−1…1, +Y up) from the camera whose
    /// view-projection is `view_proj`: from the near plane toward the far.
    pub fn through(view_proj: &Mat4, ndc: Vec2) -> Self {
        let inv = view_proj.inverse();
        let near = inv.project_point3(ndc.extend(-1.0));
        let far = inv.project_point3(ndc.extend(1.0));
        Self {
            origin: near,
            dir: (far - near).normalize_or_zero(),
        }
    }

    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.dir * t
    }

    /// The same ray in the space `m` maps from (e.g. world → a mesh's local
    /// space with `m` = the inverse world matrix). `dir` isn't renormalized,
    /// so `t` keeps meaning the same point in both spaces.
    pub fn transformed(&self, m: &Mat4) -> Self {
        Self {
            origin: m.transform_point3(self.origin),
            dir: m.transform_vector3(self.dir),
        }
    }
}

/// Entry distance of `ray` into the box (0 if it starts inside), or None.
pub fn ray_aabb(ray: &Ray, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = ray.dir.recip();
    let t1 = (min - ray.origin) * inv;
    let t2 = (max - ray.origin) * inv;
    let near = t1.min(t2).max_element();
    let far = t1.max(t2).min_element();
    (far >= near.max(0.0)).then_some(near.max(0.0))
}

/// What a pick hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub entity: Entity,
    pub distance: f32,
    pub point: Vec3,
}

/// The visible mesh whose `Bounds` a ray from `camera` through `ndc` enters
/// first (`distance` from the near plane), or None. Meshes without bounds
/// can't be hit. World matrices are the last `TransformSystem::update`'s.
pub fn pick_bounds(world: &World, camera: Entity, ndc: Vec2) -> Option<Hit> {
    let cam = world.get::<Camera>(camera)?;
    let view = world.get::<WorldMatrix>(camera)?.0.inverse();
    let ray = Ray::through(&(cam.projection_matrix() * view), ndc);
    let meshes = world.storage::<MeshRenderer>()?;
    let worlds = world.storage::<WorldMatrix>()?;
    let bounds = world.storage::<Bounds>()?;
    let any_hidden = world
        .storage::<Visible>()
        .is_some_and(|v| v.data().iter().any(|v| !v.0));
    let mut best: Option<(f32, Entity)> = None;
    for (e, _) in meshes.iter() {
        let (Some(m), Some(b)) = (worlds.get(e), bounds.get(e)) else {
            continue;
        };
        if any_hidden && hidden(world, e) {
            continue;
        }
        let local = ray.transformed(&m.0.inverse());
        if let Some(t) = ray_aabb(&local, b.min, b.max)
            && best.is_none_or(|(bt, _)| t < bt)
        {
            best = Some((t, e));
        }
    }
    best.map(|(t, entity)| Hit {
        entity,
        distance: t,
        point: ray.at(t),
    })
}
