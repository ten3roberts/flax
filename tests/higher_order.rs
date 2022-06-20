use std::ptr;

use flax::{
    component, wildcard, Component, ComponentId, ComponentValue, DebugVisitor, EntityBuilder,
    Query, TupleCloned, World,
};
use itertools::Itertools;

/// Type erased clone
pub struct Cloneable {
    func: unsafe fn(*const u8, *mut u8),
    component: ComponentId,
}

impl Cloneable {
    /// Clones src into dst
    /// Types must match
    pub unsafe fn clone(&self, src: *const u8, dst: *mut u8) {
        (self.func)(src, dst)
    }

    pub fn new<T: ComponentValue + Clone>(component: Component<T>) -> Self {
        Self {
            func: |src, dst| unsafe {
                let val = (*src.cast::<T>()).clone();
                ptr::write(dst.cast::<T>(), val);
            },
            component: component.id(),
        }
    }
}

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
                eprintln!("Sir!");
                self.0 = C;
                true
            }
        }
    }
}

component! {
    clone: Cloneable,
    debug: DebugVisitor,
}

#[test]
fn visitors() {
    component! {
        name: String,
        health: f32,
        // Then shalt count to three, no more no less
        count: Countdown<3>,
    }

    let mut world = World::new();

    let grenade = EntityBuilder::new()
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

    // Add the `debug` component to `name`
    world.set(name(), debug(), DebugVisitor::new(name()));
    world.set(health(), debug(), DebugVisitor::new(health()));

    let mut buf = String::new();

    world.visit(debug(), &mut buf);

    eprintln!("{buf}");
}

#[test]
fn relations() {
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
    enum RelationKind {
        Mom,
        Dad,
        Parent, // Not everything is binary
    }

    component! {
        name: &'static str,
        hobby: &'static str,
        child_of(e): RelationKind,
    }

    let mut world = World::new();

    world.set(name(), debug(), DebugVisitor::new(name()));

    let parent = EntityBuilder::new()
        .set(name(), "Jessica")
        .set(hobby(), "Reading")
        .spawn(&mut world);

    world.set(
        child_of(parent),
        debug(),
        DebugVisitor::new(child_of(parent)),
    );

    let parent2 = EntityBuilder::new()
        .set(name(), "Jack")
        .set(hobby(), "Crocheting")
        .spawn(&mut world);

    world.set(
        child_of(parent2),
        debug(),
        DebugVisitor::new(child_of(parent2)),
    );

    world.set(hobby(), debug(), DebugVisitor::new(hobby()));

    let child = EntityBuilder::new()
        .set(name(), "John")
        .set(hobby(), "Studying")
        .set(child_of(parent), RelationKind::Mom)
        .spawn(&mut world);

    let child2 = EntityBuilder::new()
        .set(name(), "Sally")
        .set(hobby(), "Hockey")
        .set(child_of(parent), RelationKind::Mom)
        .spawn(&mut world);

    let child3 = EntityBuilder::new()
        .set(name(), "Reacher")
        .set(hobby(), "Hockey")
        .set(child_of(parent2), RelationKind::Dad)
        .spawn(&mut world);

    let mut query = Query::new((name(), child_of(parent)));

    let items = query
        .prepare(&world)
        .iter()
        .map(TupleCloned::cloned)
        .sorted()
        .collect_vec();

    assert_eq!(
        items,
        [("John", RelationKind::Mom), ("Sally", RelationKind::Mom)]
    );

    let mut buf = String::new();
    world.visit(debug(), &mut buf);
    eprintln!("{buf}");

    // Visit the first parent of the children
    {
        let wild = child_of(wildcard().id()).id().into_pair();
        let wildcard = wildcard().id().strip_gen();

        dbg!(wild, wildcard);
    }
    let mut query = Query::new((name(), child_of(wildcard().id()).relation(0)));
    let mut query = query.prepare(&world);

    let items = query.iter().sorted().collect_vec();

    assert_eq!(
        items,
        [
            (&"John", (parent, &RelationKind::Mom)),
            (&"Reacher", (parent2, &RelationKind::Dad)),
            (&"Sally", (parent, &RelationKind::Mom))
        ]
    )
}
