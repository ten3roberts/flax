use std::ptr;

use flax::{component, Component, ComponentId, ComponentValue, EntityBuilder, World};

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
    name: String,
    // Then shalt count to three, no more no less
    count: Countdown<3>,
    clone: Cloneable,
}

#[test]
fn component_to_component() {
    let mut world = World::new();

    let grenade = EntityBuilder::new()
        .set(name(), "Holy Hand Grenade of Antioch".to_string())
        .spawn(&mut world);

    // Add the `clone` component to `name`
    world.set(name(), clone(), Cloneable::new(name()));

    panic!("At the disco")
}
