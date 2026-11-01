// Host tests: `cargo test -p rswebgl-ecs`.

use rswebgl_ecs::{Entity, Tick, World};

#[derive(Debug, Clone, Copy, PartialEq)]
struct Pos(f32);
#[derive(Debug, Clone, Copy, PartialEq)]
struct Vel(f32);
#[derive(Debug, PartialEq)]
struct Name(&'static str);

#[test]
fn a_despawned_entity_stays_dead_when_its_slot_is_reused() {
    let mut w = World::new();
    let a = w.spawn();
    w.insert(a, Pos(1.0));
    assert!(w.despawn(a));
    assert!(!w.despawn(a), "twice");
    assert!(!w.is_alive(a));

    // Churn until a's slot comes back with a new generation.
    let mut reused = None;
    for _ in 0..10_000 {
        let b = w.spawn();
        if b.0.index() == a.0.index() {
            reused = Some(b);
            break;
        }
        w.despawn(b);
    }
    let b = reused.expect("slot reused");
    w.insert(b, Pos(2.0));
    assert_eq!(w.get::<Pos>(a), None, "the old handle doesn't reach b");
    assert_eq!(w.get::<Pos>(b), Some(&Pos(2.0)));
    assert_eq!(
        w.insert(a, Pos(3.0)),
        None,
        "insert on a dead entity is a no-op"
    );
    assert_eq!(w.get::<Pos>(b), Some(&Pos(2.0)));
}

#[test]
fn removing_a_row_keeps_the_others_intact() {
    let mut w = World::new();
    let es: Vec<Entity> = (0..5).map(|_| w.spawn()).collect();
    for (i, &e) in es.iter().enumerate() {
        w.insert(e, Pos(i as f32));
    }
    assert_eq!(w.insert(es[1], Pos(10.0)), Some(Pos(1.0)), "replace");
    assert_eq!(w.remove::<Pos>(es[1]), Some(Pos(10.0)));
    assert_eq!(w.remove::<Pos>(es[1]), None);
    w.despawn(es[3]);
    let s = w.storage::<Pos>().unwrap();
    assert_eq!(s.len(), 3);
    for (i, &e) in es.iter().enumerate() {
        let want = (i != 1 && i != 3).then_some(Pos(i as f32));
        assert_eq!(s.get(e).copied(), want, "entity {i}");
    }
    // Rows stay dense and agree with the entity list.
    for (row, &e) in s.entities().iter().enumerate() {
        assert_eq!(s.row(e), Some(row as u32));
    }
    assert!(w.get::<Name>(es[0]).is_none(), "never-used component");
}

// The system pattern from `advance_tick`: sees what changed since its last
// run, including writes made after it ran — but not its own writes.
#[test]
fn systems_see_changes_since_their_last_run() {
    let mut w = World::new();
    let a = w.spawn();
    let b = w.spawn();
    w.insert(a, Pos(0.0));
    w.insert(b, Pos(0.0));

    let mut last_run: Tick = 0;
    let mut run = |w: &mut World| -> Vec<Entity> {
        let now = w.advance_tick();
        let seen = w
            .storage::<Pos>()
            .unwrap()
            .iter_changed(last_run)
            .map(|(e, _)| e)
            .collect();
        // The system's own write: new to others, not to itself.
        w.storage_mut::<Pos>().get_mut(a, now).unwrap().0 += 1.0;
        last_run = now;
        seen
    };

    assert_eq!(run(&mut w), [a, b], "everything is new at first");
    assert_eq!(run(&mut w), [], "own writes aren't seen");

    // Reading through `Mut` doesn't count; writing does.
    let _ = w.get_mut::<Pos>(a).unwrap().0;
    w.get_mut::<Pos>(b).unwrap().0 = 5.0;
    assert_eq!(run(&mut w), [b]);

    // Writing an equal value with set_if_neq isn't a change.
    w.get_mut::<Pos>(b).unwrap().set_if_neq(Pos(5.0));
    assert_eq!(run(&mut w), []);
    w.get_mut::<Pos>(b).unwrap().set_if_neq(Pos(6.0));
    assert_eq!(run(&mut w), [b]);
}

#[test]
fn another_system_sees_a_systems_writes() {
    let mut w = World::new();
    let e = w.spawn();
    w.insert(e, Pos(0.0));
    let (mut first_last, mut second_last): (Tick, Tick) = (0, 0);
    for _ in 0..2 {
        // First writes every frame...
        let now = w.advance_tick();
        w.storage_mut::<Pos>().get_mut(e, now).unwrap().0 += 1.0;
        first_last = now;
        // ...second reads after it.
        let now = w.advance_tick();
        let seen = w
            .storage::<Pos>()
            .unwrap()
            .iter_changed(second_last)
            .count();
        assert_eq!(seen, 1);
        second_last = now;
    }
    assert!(first_last < second_last);
}

#[test]
fn removals_are_logged_until_trackers_are_cleared() {
    let mut w = World::new();
    let a = w.spawn();
    let b = w.spawn();
    w.insert(a, Pos(0.0));
    w.insert(b, Pos(0.0));
    w.remove::<Pos>(a);
    w.despawn(b);
    assert_eq!(w.storage::<Pos>().unwrap().removed(), [a, b]);
    w.clear_trackers();
    assert!(w.storage::<Pos>().unwrap().removed().is_empty());
}

#[test]
fn join_yields_entities_with_both_components() {
    let mut w = World::new();
    let both = w.spawn();
    let only_pos = w.spawn();
    let only_vel = w.spawn();
    w.insert(both, Pos(1.0));
    w.insert(both, Vel(2.0));
    w.insert(only_pos, Pos(3.0));
    w.insert(only_vel, Vel(4.0));
    let got: Vec<_> = w.join::<Vel, Pos>().collect();
    assert_eq!(got, [(both, &Vel(2.0), &Pos(1.0))]);
    assert_eq!(w.join::<Name, Pos>().count(), 0, "unknown component");
}

#[test]
#[should_panic(expected = "two different component types")]
fn storages_mut_rejects_the_same_type_twice() {
    let mut w = World::new();
    let _ = w.storages_mut::<Pos, Pos>();
}
