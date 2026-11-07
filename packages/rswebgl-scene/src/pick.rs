//! Picking: which mesh a ray from the camera through a screen point hits
//! first. Meshes with triangles (glTF primitives keep theirs, see
//! `Primitive`) are tested exactly; others by their `Bounds`. Hidden meshes
//! are skipped.

use glam::{Mat4, Vec2, Vec3};
use rswebgl_ecs::{Entity, World};

use crate::camera::Camera;
use crate::culling::Bounds;
use crate::model::Primitive;
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

/// Distance along `ray` to triangle `abc` (either side), or None.
pub fn ray_triangle(ray: &Ray, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    // Möller–Trumbore.
    let e1 = b - a;
    let e2 = c - a;
    let p = ray.dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = ray.origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = ray.dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t >= 0.0).then_some(t)
}

/// The first mesh along a ray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub entity: Entity,
    /// World-space distance from the ray's origin (the near plane).
    pub distance: f32,
    pub point: Vec3,
}

/// The nearest visible mesh under `ndc` (−1…1, +Y up) as seen from `camera`
/// (an entity with a `Camera` and a `WorldMatrix`), or None. World matrices
/// are the last `TransformSystem::update`'s.
pub fn pick(world: &World, camera: Entity, ndc: Vec2) -> Option<Hit> {
    let cam = world.get::<Camera>(camera)?;
    let view = world.get::<WorldMatrix>(camera)?.0.inverse();
    let ray = Ray::through(&(cam.projection_matrix() * view), ndc);
    pick_ray(world, &ray)
}

/// `pick` with a world-space ray of your own.
pub fn pick_ray(world: &World, ray: &Ray) -> Option<Hit> {
    let meshes = world.storage::<MeshRenderer>()?;
    let worlds = world.storage::<WorldMatrix>()?;
    let bounds = world.storage::<Bounds>();
    let primitives = world.storage::<Primitive>();
    let any_hidden = world
        .storage::<Visible>()
        .is_some_and(|v| v.data().iter().any(|v| !v.0));

    let mut best: Option<(f32, Entity)> = None;
    for (e, _) in meshes.iter() {
        let Some(m) = worlds.get(e) else {
            continue;
        };
        if any_hidden && hidden(world, e) {
            continue;
        }
        let local = ray.transformed(&m.0.inverse());
        let nearest = best.map_or(f32::INFINITY, |(t, _)| t);
        // The box first: cheap, and enough without triangles.
        let box_t = bounds
            .and_then(|b| b.get(e))
            .map(|b| ray_aabb(&local, b.min, b.max));
        if matches!(box_t, Some(None)) || box_t.flatten().is_some_and(|t| t >= nearest) {
            continue;
        }
        let tris = primitives
            .and_then(|p| p.get(e))
            .and_then(|p| p.triangles.as_ref());
        let t = match tris {
            Some(tris) => tris
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .filter_map(|&[i, j, k]| {
                    let p = |n: u32| tris.positions.get(n as usize).map(|&v| Vec3::from(v));
                    ray_triangle(&local, p(i)?, p(j)?, p(k)?)
                })
                .reduce(f32::min),
            None => box_t.flatten(),
        };
        if let Some(t) = t
            && t < nearest
        {
            best = Some((t, e));
        }
    }
    best.map(|(t, entity)| {
        let point = ray.at(t);
        Hit {
            entity,
            distance: t,
            point,
        }
    })
}
