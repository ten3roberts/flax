use flax::components::name;
use flax::Debug;
use flax::{
    component, debug_visitor, util::TupleCloned, visitors::DebugVisitor, wildcard, EntityBuilder,
    Query, World,
};
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

    let mut buf = String::new();

    world.visit(debug_visitor(), &mut buf);

    eprintln!("{buf}");
}

#[test]
fn relations() {
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
    enum RelationKind {
        Mom,
        Dad,
    }

    component! {
        hobby: &'static str => [ Debug ],
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

    let _child = EntityBuilder::new()
        .set(name(), "John".to_string())
        .set(hobby(), "Studying")
        .set(child_of(parent), RelationKind::Mom)
        .spawn(&mut world);

    let _child2 = EntityBuilder::new()
        .set(name(), "Sally".to_string())
        .set(hobby(), "Hockey")
        .set(child_of(parent), RelationKind::Mom)
        .spawn(&mut world);

    assert!(world.get(child_of(parent).id(), debug_visitor()).is_ok());
    let location = world.location(child_of(parent).id());
    eprintln!("Location of child_of: {location:?}");
    let _child3 = EntityBuilder::new()
        .set(name(), "Reacher".to_string())
        .set(hobby(), "Hockey")
        .set(child_of(parent2), RelationKind::Dad)
        .spawn(&mut world);

    let location = world.location(child_of(parent).id());

    eprintln!("Location of child_of: {location:?}");

    let mut query = Query::new((name(), child_of(parent)));

    eprintln!("{:#?}", world.component_metadata());
    assert!(world.get(child_of(parent).id(), debug_visitor()).is_ok());
    assert!(world.get(child_of(parent2).id(), debug_visitor()).is_ok());
    panic!("");

    let items = query
        .prepare(&world)
        .iter()
        .map(TupleCloned::cloned)
        .sorted()
        .collect_vec();

    assert_eq!(
        items,
        [
            ("John".to_string(), RelationKind::Mom),
            ("Sally".to_string(), RelationKind::Mom)
        ]
    );

    let mut buf = String::new();

    world.visit(debug_visitor(), &mut buf);

    eprintln!("Visited: {buf}");

    // Visit the first parent of the children
    let mut query = Query::new((name(), child_of(wildcard().id()).relation(0)));
    let mut query = query.prepare(&world);

    let items = query.iter().sorted().collect_vec();

    assert_eq!(
        items,
        [
            (&"John".to_string(), (parent, &RelationKind::Mom)),
            (&"Reacher".to_string(), (parent2, &RelationKind::Dad)),
            (&"Sally".to_string(), (parent, &RelationKind::Mom))
        ]
    )
}
