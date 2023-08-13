use flax::{component, Entity, World};

#[test]
fn prune_archetypes() {
    component! {
        a: (),
        b: (),
        c: (),
    }

    let mut world = World::new();

    let id_a = Entity::builder().tag(a()).spawn(&mut world);
    let id_ab = Entity::builder().tag(a()).tag(b()).spawn(&mut world);
    let id_ac = Entity::builder().tag(a()).tag(c()).spawn(&mut world);
    let id_abc = Entity::builder()
        .tag(a())
        .tag(b())
        .tag(c())
        .spawn(&mut world);

    // A(1)
    //  A_B(1)
    //      A_B_C(1)
    //  A_C(1)

    assert_eq!(world.prune_archetypes(), 0);
    world.despawn(id_a).unwrap();

    // A(0)
    //  A_B(1)
    //      A_B_C(1)
    //  A_C(1)

    world.despawn(id_abc).unwrap();

    // A(0)
    //  A_B(1)
    //      A_B_C(0) *
    //  A_C(1)
    assert_eq!(world.prune_archetypes(), 1);

    world.despawn(id_ac).unwrap();

    // A(0)
    //  A_B(1)
    //      A_B_C(0)
    //  A_C(0) *
    assert_eq!(world.prune_archetypes(), 1);

    world.despawn(id_ab).unwrap();

    // A(0) *
    //  A_B(0) *
    //      A_B_C(0)
    //  A_C(0)
    assert_eq!(world.prune_archetypes(), 2);
    assert_eq!(world.prune_archetypes(), 0);
}
