// Host tests: `cargo test -p rswebgl-scene`.

use glam::{Mat4, Quat, Vec3};
use rswebgl_ecs::{Entity, World};
use rswebgl_scene::{
    HierarchyError, Parent, Transform, TransformSystem, WorldMatrix, children, despawn_recursive,
    set_parent,
};

fn at(x: f32, y: f32, z: f32) -> Transform {
    Transform::from_translation(Vec3::new(x, y, z))
}

fn pos(w: &World, e: Entity) -> Vec3 {
    w.get::<WorldMatrix>(e)
        .expect("has a world matrix")
        .0
        .w_axis
        .truncate()
}

fn frame(w: &mut World, sys: &mut TransformSystem) -> usize {
    let n = sys.update(w);
    w.clear_trackers();
    n
}

// root ─ a ─ b, plus an unrelated root c.
fn chain(w: &mut World) -> [Entity; 4] {
    let [root, a, b, c] = [w.spawn(), w.spawn(), w.spawn(), w.spawn()];
    w.insert(root, at(1.0, 0.0, 0.0));
    w.insert(a, at(0.0, 1.0, 0.0));
    w.insert(b, at(0.0, 0.0, 1.0));
    w.insert(c, at(5.0, 0.0, 0.0));
    // Children set up before their parents' order in storage doesn't matter.
    set_parent(w, b, Some(a)).unwrap();
    set_parent(w, a, Some(root)).unwrap();
    [root, a, b, c]
}

#[test]
fn world_matrices_compose_down_the_chain() {
    let mut w = World::new();
    let [root, a, b, c] = chain(&mut w);
    let mut sys = TransformSystem::new();
    assert_eq!(frame(&mut w, &mut sys), 4);
    assert_eq!(pos(&w, root), Vec3::new(1.0, 0.0, 0.0));
    assert_eq!(pos(&w, a), Vec3::new(1.0, 1.0, 0.0));
    assert_eq!(pos(&w, b), Vec3::new(1.0, 1.0, 1.0));
    assert_eq!(pos(&w, c), Vec3::new(5.0, 0.0, 0.0));

    // Rotation and scale of the parent apply to the child's offset.
    w.insert(
        root,
        Transform {
            translation: Vec3::ZERO,
            rotation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            scale: Vec3::splat(2.0),
        },
    );
    frame(&mut w, &mut sys);
    assert!(pos(&w, a).abs_diff_eq(Vec3::new(-2.0, 0.0, 0.0), 1e-5));
    let expect = w.get::<Transform>(root).unwrap().matrix()
        * at(0.0, 1.0, 0.0).matrix()
        * at(0.0, 0.0, 1.0).matrix();
    assert!(w.get::<WorldMatrix>(b).unwrap().0.abs_diff_eq(expect, 1e-5));
}

#[test]
fn only_moved_subtrees_are_recomputed() {
    let mut w = World::new();
    let [_root, a, b, c] = chain(&mut w);
    let mut sys = TransformSystem::new();
    frame(&mut w, &mut sys);
    assert_eq!(frame(&mut w, &mut sys), 0, "static frame writes nothing");

    // A renderer-style reader that only wants moved objects.
    let mut seen_at = w.advance_tick();
    w.get_mut::<Transform>(a).unwrap().translation.x = 3.0;
    assert_eq!(frame(&mut w, &mut sys), 2, "a and its child b");
    let moved: Vec<Entity> = w
        .storage::<WorldMatrix>()
        .unwrap()
        .iter_changed(seen_at)
        .map(|(e, _)| e)
        .collect();
    assert_eq!(moved, [a, b]);
    assert_eq!(pos(&w, b), Vec3::new(4.0, 1.0, 1.0));

    seen_at = w.advance_tick();
    w.get_mut::<Transform>(c)
        .unwrap()
        .set_if_neq(at(5.0, 0.0, 0.0));
    assert_eq!(frame(&mut w, &mut sys), 0, "an equal write isn't a move");
    assert_eq!(
        w.storage::<WorldMatrix>()
            .unwrap()
            .iter_changed(seen_at)
            .count(),
        0
    );
}

#[test]
fn reparenting_and_unparenting_take_effect() {
    let mut w = World::new();
    let [root, a, b, c] = chain(&mut w);
    let mut sys = TransformSystem::new();
    frame(&mut w, &mut sys);

    set_parent(&mut w, a, Some(c)).unwrap();
    frame(&mut w, &mut sys);
    assert_eq!(pos(&w, b), Vec3::new(5.0, 1.0, 1.0));
    assert_eq!(children(&w, c), [a]);
    assert!(children(&w, root).is_empty());

    set_parent(&mut w, a, None).unwrap();
    frame(&mut w, &mut sys);
    assert_eq!(pos(&w, a), Vec3::new(0.0, 1.0, 0.0));
    assert_eq!(pos(&w, b), Vec3::new(0.0, 1.0, 1.0));

    // Setting the same parent again isn't a change.
    set_parent(&mut w, b, Some(a)).unwrap();
    assert_eq!(frame(&mut w, &mut sys), 0);
}

#[test]
fn new_entities_get_a_world_matrix_and_dead_parents_make_roots() {
    let mut w = World::new();
    let [root, a, b, _c] = chain(&mut w);
    let mut sys = TransformSystem::new();
    frame(&mut w, &mut sys);

    let d = w.spawn();
    w.insert(d, at(0.0, 0.0, 7.0));
    set_parent(&mut w, d, Some(b)).unwrap();
    frame(&mut w, &mut sys);
    assert_eq!(pos(&w, d), Vec3::new(1.0, 1.0, 8.0));

    // A plain despawn leaves a's Parent pointing at a dead entity: a becomes
    // a root rather than keeping a stale offset.
    w.despawn(root);
    frame(&mut w, &mut sys);
    assert_eq!(pos(&w, a), Vec3::new(0.0, 1.0, 0.0));
    assert_eq!(pos(&w, d), Vec3::new(0.0, 1.0, 8.0));

    // Removing the Transform of a parent does the same.
    w.remove::<Transform>(a);
    frame(&mut w, &mut sys);
    assert_eq!(pos(&w, b), Vec3::new(0.0, 0.0, 1.0));
}

#[test]
fn set_parent_refuses_cycles_and_dead_entities() {
    let mut w = World::new();
    let [root, a, b, c] = chain(&mut w);
    assert_eq!(
        set_parent(&mut w, root, Some(b)),
        Err(HierarchyError::Cycle)
    );
    assert_eq!(set_parent(&mut w, a, Some(a)), Err(HierarchyError::Cycle));
    w.despawn(c);
    assert_eq!(
        set_parent(&mut w, a, Some(c)),
        Err(HierarchyError::DeadEntity)
    );
    assert_eq!(
        set_parent(&mut w, c, Some(a)),
        Err(HierarchyError::DeadEntity)
    );
    assert_eq!(w.get::<Parent>(a), Some(&Parent(root)), "unchanged");
}

#[test]
fn a_hand_made_cycle_is_skipped_not_looped_on() {
    let mut w = World::new();
    let [_root, a, b, c] = chain(&mut w);
    let x = w.spawn();
    w.insert(x, at(0.0, 0.0, 0.0));
    w.insert(c, Parent(x));
    w.insert(x, Parent(c));
    let mut sys = TransformSystem::new();
    assert_eq!(frame(&mut w, &mut sys), 3);
    assert_eq!(sys.skipped(), 2);
    assert!(!w.has::<WorldMatrix>(c));
    assert_eq!(pos(&w, b), Vec3::new(1.0, 1.0, 1.0));
    assert_eq!(despawn_recursive(&mut w, c), 2, "c and x, once each");
    assert!(w.is_alive(a));
}

#[test]
fn despawn_recursive_removes_the_subtree() {
    let mut w = World::new();
    let [root, a, b, c] = chain(&mut w);
    let mut sys = TransformSystem::new();
    frame(&mut w, &mut sys);
    assert_eq!(despawn_recursive(&mut w, a), 2);
    assert!(!w.is_alive(a) && !w.is_alive(b));
    assert!(w.is_alive(root) && w.is_alive(c));
    assert_eq!(despawn_recursive(&mut w, a), 0, "already gone");
    frame(&mut w, &mut sys);
    assert_eq!(w.storage::<WorldMatrix>().unwrap().len(), 2);
    assert_eq!(pos(&w, root), Vec3::new(1.0, 0.0, 0.0));
}

#[test]
fn transform_round_trips_through_a_matrix() {
    let t = Transform {
        translation: Vec3::new(1.0, -2.0, 3.0),
        rotation: Quat::from_euler(glam::EulerRot::XYZ, 0.3, -1.1, 2.0),
        scale: Vec3::new(2.0, 0.5, 1.5),
    };
    let back = Transform::from_matrix(t.matrix());
    assert!(back.matrix().abs_diff_eq(t.matrix(), 1e-5));
    assert_eq!(Transform::default().matrix(), Mat4::IDENTITY);
}

// One despawn and one spawn in the same frame keep the count of transforms
// the same; the removal log still tells the system to rebuild.
#[test]
fn a_swap_within_one_frame_is_noticed() {
    let mut w = World::new();
    let [_root, a, b, _c] = chain(&mut w);
    let mut sys = TransformSystem::new();
    frame(&mut w, &mut sys);
    w.despawn(b);
    let d = w.spawn();
    w.insert(d, at(0.0, 0.0, 2.0));
    set_parent(&mut w, d, Some(a)).unwrap();
    frame(&mut w, &mut sys);
    assert_eq!(pos(&w, d), Vec3::new(1.0, 1.0, 2.0));
}
