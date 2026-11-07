// Host tests: `cargo test -p rswebgl-scene`.

use glam::{Mat4, Vec2, Vec3};
use rswebgl_scene::{Camera, Ray, ray_aabb, ray_triangle};

fn ray(o: [f32; 3], d: [f32; 3]) -> Ray {
    Ray {
        origin: Vec3::from(o),
        dir: Vec3::from(d).normalize(),
    }
}

#[test]
fn boxes() {
    let (min, max) = (Vec3::splat(-1.0), Vec3::splat(1.0));
    assert_eq!(
        ray_aabb(&ray([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]), min, max),
        Some(4.0)
    );
    assert_eq!(
        ray_aabb(&ray([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), min, max),
        Some(0.0),
        "inside"
    );
    assert_eq!(
        ray_aabb(&ray([0.0, 0.0, 5.0], [0.0, 0.0, 1.0]), min, max),
        None,
        "behind"
    );
    assert_eq!(
        ray_aabb(&ray([3.0, 0.0, 5.0], [0.0, 0.0, -1.0]), min, max),
        None,
        "beside"
    );
    // Axis-parallel rays (infinite reciprocals) still work.
    assert_eq!(
        ray_aabb(&ray([1.0, 0.0, 5.0], [0.0, 0.0, -1.0]), min, max),
        Some(4.0),
        "on a face"
    );
}

#[test]
fn triangles_from_both_sides() {
    let (a, b, c) = (
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(1.0, -1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    assert_eq!(
        ray_triangle(&ray([0.0, 0.0, 2.0], [0.0, 0.0, -1.0]), a, b, c),
        Some(2.0)
    );
    assert_eq!(
        ray_triangle(&ray([0.0, 0.0, -3.0], [0.0, 0.0, 1.0]), a, b, c),
        Some(3.0),
        "back"
    );
    assert_eq!(
        ray_triangle(&ray([0.9, 0.9, 2.0], [0.0, 0.0, -1.0]), a, b, c),
        None,
        "outside"
    );
    assert_eq!(
        ray_triangle(&ray([0.0, 0.0, 2.0], [1.0, 0.0, 0.0]), a, b, c),
        None,
        "parallel"
    );
}

#[test]
fn camera_rays_go_through_the_screen_point() {
    let cam = Camera::perspective(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 100.0);
    let view = Mat4::from_translation(Vec3::new(0.0, 0.0, 10.0)).inverse();
    let vp = cam.projection_matrix() * view;
    let center = Ray::through(&vp, Vec2::ZERO);
    assert!(
        center.origin.abs_diff_eq(Vec3::new(0.0, 0.0, 9.0), 1e-4),
        "near plane"
    );
    assert!(center.dir.abs_diff_eq(Vec3::NEG_Z, 1e-5));
    // 90° fov: the right edge of the screen is 45° off the axis.
    let right = Ray::through(&vp, Vec2::new(1.0, 0.0));
    assert!(
        right
            .dir
            .abs_diff_eq(Vec3::new(1.0, 0.0, -1.0).normalize(), 1e-4)
    );
    // A ray moved into a mesh's space keeps `t` meaning the same point.
    let m = Mat4::from_scale(Vec3::splat(2.0));
    let local = center.transformed(&m.inverse());
    assert!(
        m.transform_point3(local.at(3.0))
            .abs_diff_eq(center.at(3.0), 1e-4)
    );
}
