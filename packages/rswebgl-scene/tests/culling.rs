// Host tests: `cargo test -p rswebgl-scene`.

use glam::{Mat4, Quat, Vec3};
use rswebgl_scene::{Bounds, Camera, Frustum};

// A 90° camera at the origin looking down −Z, square aspect, near 1, far 10.
fn frustum() -> Frustum {
    let cam = Camera::perspective(90f32.to_radians(), 1.0, 1.0, 10.0);
    Frustum::from_view_proj(&cam.projection_matrix())
}

fn unit_box() -> Bounds {
    Bounds::new(Vec3::splat(-0.5), Vec3::splat(0.5))
}

fn inside(f: &Frustum, m: Mat4) -> bool {
    let (c, h) = unit_box().transformed(&m);
    f.intersects(c, h)
}

#[test]
fn boxes_in_front_are_kept_and_others_culled() {
    let f = frustum();
    let at = |x, y, z| Mat4::from_translation(Vec3::new(x, y, z));
    assert!(inside(&f, at(0.0, 0.0, -5.0)), "straight ahead");
    assert!(!inside(&f, at(0.0, 0.0, 5.0)), "behind");
    assert!(!inside(&f, at(0.0, 0.0, -11.0)), "past far");
    assert!(!inside(&f, at(0.0, 0.0, -0.4)), "before near");
    // The half-width is -z: 5 at the box center, 5.5 at its far face.
    assert!(inside(&f, at(5.4, 0.0, -5.0)), "partly inside the side");
    assert!(!inside(&f, at(6.2, 0.0, -5.0)), "just outside the side");
    assert!(!inside(&f, at(0.0, -6.2, -5.0)), "just below");
}

#[test]
fn rotation_and_scale_grow_the_world_box() {
    let (c, h) = unit_box().transformed(&Mat4::from_scale_rotation_translation(
        Vec3::new(2.0, 1.0, 1.0),
        Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
        Vec3::new(1.0, 2.0, 3.0),
    ));
    assert!(c.abs_diff_eq(Vec3::new(1.0, 2.0, 3.0), 1e-6));
    // A 2×1 box turned 45°: each of x/y spans (2 + 1)·cos45 / 2.
    let expect = 1.5 * std::f32::consts::FRAC_1_SQRT_2;
    assert!(h.abs_diff_eq(Vec3::new(expect, expect, 0.5), 1e-5), "{h}");

    // A big scaled box reaching into view from outside isn't culled.
    let f = frustum();
    let big = Mat4::from_scale_rotation_translation(
        Vec3::splat(4.0),
        Quat::IDENTITY,
        Vec3::new(7.0, 0.0, -5.0),
    );
    assert!(inside(&f, big));
}

#[test]
fn orthographic_and_view_matrices() {
    let cam = Camera::orthographic(2.0, 2.0, 0.1, 10.0);
    // Camera moved to x = 10: the view is the inverse of its world matrix.
    let view = Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)).inverse();
    let f = Frustum::from_view_proj(&(cam.projection_matrix() * view));
    let at = |x| Mat4::from_translation(Vec3::new(x, 0.0, -5.0));
    assert!(inside(&f, at(10.0)));
    assert!(inside(&f, at(14.4)), "half-width is 4");
    assert!(!inside(&f, at(14.6)));
    assert!(!inside(&f, at(0.0)));
    // A zero aspect (0×0 canvas) doesn't produce NaNs.
    let degenerate = Camera::perspective(1.0, 0.0, 0.1, 10.0).projection_matrix();
    assert!(degenerate.is_finite());
}

#[test]
fn bounds_from_points() {
    let b = Bounds::from_points(&[1.0, -2.0, 0.0, -1.0, 3.0, 5.0, 0.0, 0.0, 0.0]).unwrap();
    assert_eq!(
        b,
        Bounds::new(Vec3::new(-1.0, -2.0, 0.0), Vec3::new(1.0, 3.0, 5.0))
    );
    assert_eq!(Bounds::from_points(&[]), None);
}

// Reverse-Z (zero-to-one, near = 1): the frustum has no far plane for a
// perspective camera, and keeps near and side planes.
#[test]
fn reverse_z_frustums_reach_to_infinity() {
    let cam = Camera::perspective(90f32.to_radians(), 1.0, 1.0, 10.0);
    let f = Frustum::from_view_proj_depth(&cam.projection_for(true), true);
    let at = |x, y, z| Mat4::from_translation(Vec3::new(x, y, z));
    assert!(inside(&f, at(0.0, 0.0, -5.0)));
    assert!(inside(&f, at(0.0, 0.0, -1.0e6)), "no far plane");
    assert!(!inside(&f, at(0.0, 0.0, 5.0)), "behind");
    assert!(!inside(&f, at(0.0, 0.0, -0.4)), "before near");
    assert!(!inside(&f, at(6.2, 0.0, -5.0)), "beside");

    // Orthographic keeps both planes, swapped.
    let cam = Camera::orthographic(2.0, 1.0, 0.1, 10.0);
    let f = Frustum::from_view_proj_depth(&cam.projection_for(true), true);
    assert!(inside(&f, at(0.0, 0.0, -5.0)));
    assert!(!inside(&f, at(0.0, 0.0, -11.0)), "past far");
    assert!(!inside(&f, at(0.0, 0.0, 5.0)), "behind");
}
