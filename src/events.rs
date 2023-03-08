use alloc::{boxed::Box, collections::BTreeSet};
use core::sync::atomic::{AtomicBool, Ordering};
use smallvec::SmallVec;

use crate::{
    archetype::{Archetype, Cell, Slice, Slot},
    filter::StaticFilter,
    And, ChangeKind, Component, ComponentInfo, ComponentKey, ComponentValue, Entity, Fetch,
};

/// Describes a component which changed in the matched archetype
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct ChangeEvent {
    /// The kind of change
    pub kind: ChangeKind,
    /// The component that changed
    pub component: ComponentKey,
}

impl ChangeEvent {
    /// Returns the kind of the change
    pub fn kind(&self) -> ChangeKind {
        self.kind
    }

    /// Returns the key of the changed component
    pub fn component(&self) -> ComponentKey {
        self.component
    }
}

pub struct Event {
    /// The affected entity
    pub id: Entity,
    pub key: ComponentKey,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventKind {
    /// The component was added to the entity
    Added,
    /// The component was removed from the entity
    Removed,
    Modified,
}

pub struct EventSubscriber<H, F> {
    components: BTreeSet<ComponentKey>,
    filter: F,
    handler: H,
    connected: AtomicBool,
}

/// Listen to shape changes of entities, such as a required component being removed, or an entity
/// fulfilling the filter.
pub struct ShapeSubscriber<F, L> {
    shape: F,
    listener: L,
    connected: AtomicBool,
}

impl<F, L> ShapeSubscriber<F, L> {
    /// Create a new subscriber to handle
    pub fn new(shape: F, listener: L) -> Self {
        Self {
            shape,
            listener,
            connected: AtomicBool::new(true),
        }
    }
}

/// Represents the raw form of an event, where the archetype is available
pub struct BufferedEvent<'a> {
    /// The affected entities
    pub(crate) ids: &'a [Entity],
    pub(crate) key: ComponentKey,
    pub(crate) kind: EventKind,
}

/// Handles ecs events
pub trait EventHandler: ComponentValue {
    /// Returns true if the listener is to be retained
    fn on_event(&self, event: &BufferedEvent);
    fn filter_arch(&self, arch: &Archetype) -> bool;
    fn filter_component(&self, info: ComponentInfo) -> bool;
    fn is_connected(&self) -> bool;
}

#[cfg(feature = "flume")]
impl EventHandler for flume::Sender<Event> {
    fn on_event(&self, event: &BufferedEvent) {
        for &id in event.ids {
            let _ = self.send(Event {
                id,
                key: event.key,
                kind: event.kind,
            });
        }
    }

    fn filter_arch(&self, _: &Archetype) -> bool {
        true
    }

    fn filter_component(&self, info: ComponentInfo) -> bool {
        true
    }

    fn is_connected(&self) -> bool {
        !self.is_disconnected()
    }
}

// #[cfg(feature = "flume")]
// TODO weak sender
// impl<T> EventHandler<T> for flume::WeakSender<T> {
//     fn on_event(&self, event: T) -> bool {
//         self.send(event).is_ok()
//     }
// }

// #[cfg(feature = "tokio")]
// impl EventHandler for tokio::sync::mpsc::UnboundedSender<Event> {
//     fn on_event(&self, event: BufferedEvent) {
//         for id in event.arch.entities()[event.slots.as_range()] {
//             self.send(Event {
//                 id,
//                 key: event.key,
//                 kind: event.kind,
//             })
//         }
//     }
// }

// #[cfg(feature = "tokio")]
// impl<T> EventHandler<T> for tokio::sync::mpsc::Sender<T> {
//     fn on_event(&self, event: T) -> bool {
//         todo!()
//     }
// }

// #[cfg(feature = "tokio")]
// impl<T> EventHandler<T> for tokio::sync::broadcast::Sender<T> {
//     fn on_event(&self, event: T) -> bool {
//         todo!()
//     }
// }

// #[cfg(feature = "tokio")]
// impl EventHandler for alloc::sync::Weak<tokio::sync::Notify> {
//     fn on_event(&self, _: BufferedEvent) -> bool {
//         if let Some(notify) = self.upgrade() {
//             notify.notify_one();
//             true
//         } else {
//             false
//         }
//     }
// }
