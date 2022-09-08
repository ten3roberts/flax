use flax::components::{child_of, name};
use flax::{component, debug_visitor, entity::wildcard, EntityBuilder, Query, World};
use flax::{entity_ids, relations_like, Debug, Entity};
use itertools::Itertools;

#[derive(Debug, Clone)]
pub struct Countdown<const C: usize>(usize);

impl<const C: usize> Countdown<C> {
    pub fn new() -> Self {
        Self(0)
    }

    pub fn proceed(&mut self) -> bool {
        self.0 += 1;

        match self.0.cmp(&C) {
            std::cmp::Ordering::Less => false,
            std::cmp::Ordering::Equal => true,
            std::cmp::Ordering::Greater => {
                eprintln!("Sire!");
                self.0 = C;
                true
            }
        }
    }
}

impl<const C: usize> Default for Countdown<C> {
    fn default() -> Self {
        Self::new()
    }
}

#[test]
fn visitors() {
    use flax::Debug;
    component! {
        health: f32 => [Debug],
        // Then shalt count to three, no more no less
        count: Countdown<3> => [Debug],
    }

    let mut world = World::new();

    EntityBuilder::new()
        .set(name(), "Holy Hand Grenade of Antioch".to_string())
        .spawn(&mut world);

    let mut builder = EntityBuilder::new();
    for i in 0..128 {
        let perm = ((i as f32 + 0.4) * (i as f32) * 6.0) % 100.0;
        builder
            .set(name(), format!("Clone#{i}"))
            .set(health(), perm)
            .spawn(&mut world);
    }

    eprintln!("World: {world:#?}");
}

#[test_log::test]
fn relations() {
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
    enum RelationKind {
        Mom,
        Dad,
    }

    component! {
        hobby: &'static str => [ Debug ],
        profession: &'static str => [ Debug ],
        child_of(e): RelationKind => [ Debug ],
    }

    let mut world = World::new();

    let parent = EntityBuilder::new()
        .set(name(), "Jessica".to_string())
        .set(hobby(), "Reading")
        .spawn(&mut world);

    let parent2 = EntityBuilder::new()
        .set(name(), "Jack".to_string())
        .set(hobby(), "Crocheting")
        .spawn(&mut world);

    let child = EntityBuilder::new()
        .set(name(), "John".to_string())
        .set(hobby(), "Studying")
        .set(child_of(parent), RelationKind::Mom)
        .spawn(&mut world);

    let child2 = EntityBuilder::new()
        .set(name(), "Sally".to_string())
        .set(hobby(), "Hockey")
        .set(child_of(parent), RelationKind::Mom)
        .set(profession(), "Student")
        .spawn(&mut world);

    assert!(world.get(child_of(parent).id(), debug_visitor()).is_ok());

    let child3 = EntityBuilder::new()
        .set(name(), "Reacher".to_string())
        .set(hobby(), "Hockey")
        .set(child_of(parent2), RelationKind::Dad)
        .spawn(&mut world);

    let mut query = Query::new((name(), child_of(parent)));

    assert!(world.get(child_of(parent).id(), debug_visitor()).is_ok());
    assert!(world.get(child_of(parent2).id(), debug_visitor()).is_ok());

    let items = query
        .borrow(&world)
        .iter()
        .map(|(a, b)| (a.clone(), *b))
        .sorted()
        .collect_vec();

    assert_eq!(
        items,
        [
            ("John".to_string(), RelationKind::Mom),
            ("Sally".to_string(), RelationKind::Mom)
        ]
    );

    eprintln!("World: {world:#?}");

    // Visit the first parent of the children
    let mut query = Query::new((name(), relations_like(child_of)));
    let mut query = query.borrow(&world);

    let items = query
        .iter()
        .map(|(name, relations)| (name, relations.collect_vec()))
        .sorted()
        .collect_vec();

    assert_eq!(
        items,
        [
            (&"Jack".to_string(), vec![]),
            (&"Jessica".to_string(), vec![]),
            (&"John".to_string(), vec![(parent, &RelationKind::Mom)]),
            (&"Reacher".to_string(), vec![(parent2, &RelationKind::Dad)]),
            (&"Sally".to_string(), vec![(parent, &RelationKind::Mom)])
        ]
    );

    drop(query);

    // If we remove a parent, the children will be detached.
    world.detach(parent);

    assert!(world.get(child, child_of(parent)).is_err());
    assert!(world.get(child2, child_of(parent)).is_err());

    assert!(world.get(child3, child_of(parent2)).is_ok());

    eprintln!("Before: {world:#?}");
    world.clear(parent2).unwrap();
    eprintln!("After: {world:#?}");

    assert!(world.is_alive(parent2));

    assert!(world.get(parent2, name()).is_err());
    assert!(world.get(child3, child_of(parent2)).is_err());
}

#[test_log::test]
fn build_hierarchy() {
    // tracing_subscriber::fmt::init();

    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
    enum RelationKind {
        Dad,
    }

    component! {
        hobby: &'static str => [ Debug ],
        profession: &'static str => [ Debug ],
        child_of(e): RelationKind => [ Debug ],
    }

    let mut world = World::new();

    let parent = Entity::builder()
        .set(name(), "Alex".into())
        .set(hobby(), "Sewing")
        .attach_with(
            child_of,
            RelationKind::Dad,
            Entity::builder()
                .set(name(), "Eric".into())
                .set(hobby(), "Gaming"),
        )
        .attach_with(
            child_of,
            RelationKind::Dad,
            Entity::builder()
                .set(name(), "Vanessa".into())
                .set(hobby(), "Climbing"),
        )
        .spawn(&mut world);

    assert_eq!(
        world.get(parent, name()).as_deref(),
        Ok(&"Alex".to_string())
    );

    let mut query = Query::new((name(), child_of(parent)));
    let mut query = query.borrow(&world);
    let children = query.iter().sorted().collect_vec();

    assert_eq!(
        children,
        [
            (&"Eric".to_string(), &RelationKind::Dad),
            (&"Vanessa".to_string(), &RelationKind::Dad)
        ]
    );
}

#[test]
fn hierarchy_manipulation() {
    let mut world = World::new();

    let a = Entity::builder()
        .set(name(), "a".into())
        .attach(child_of, Entity::builder().set(name(), "a.a".into()))
        .spawn(&mut world);

    let b = Entity::builder()
        .set(name(), "b".into())
        .attach(child_of, Entity::builder().set(name(), "b.a".into()))
        .attach(child_of, Entity::builder().set(name(), "b.b".into()))
        .spawn(&mut world);

    // Query all entities with no `child_of` relation
    let mut q = Query::new(entity_ids()).without(child_of(wildcard()));

    let roots = q.borrow(&world).iter().sorted().collect_vec();
    assert_eq!(roots, [a, b]);

    // Attach a under b
    world.set(a, child_of(b), ()).unwrap();
    let roots = q.borrow(&world).iter().sorted().collect_vec();
    assert_eq!(roots, [b]);

    world.detach(b);
    assert_eq!(world.get(b, name()).as_deref(), Ok(&"b".to_string()));

    let mut q = Query::new(name()).without(child_of(wildcard()));
    let mut roots = q.borrow(&world);
    let roots = roots.iter().sorted().collect_vec();

    assert_eq!(roots, ["a", "b", "b.a", "b.b"]);

    let children_of_a = Query::new(name()).with(child_of(a)).as_vec(&world);
    let children_of_b = Query::new(name()).with(child_of(b)).as_vec(&world);

    assert_eq!(children_of_a, ["a.a"]);
    assert_eq!(children_of_b, [""; 0]);
}
