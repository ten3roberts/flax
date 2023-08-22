use alloc::vec::Vec;

use crate::{
    archetype::Archetype, filter::StaticFilter, ComponentDesc, ComponentKey, ComponentValue, Entity,
};

#[derive(Debug, Clone, PartialEq, Eq)]
/// Represents a single ECS event
pub struct Event {
    /// The affected entity
    pub id: Entity,
    /// The affected component
    pub key: ComponentKey,
    /// The type of event
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// The type of ECS event
pub enum EventKind {
    /// The component was added to the entity
    Added,
    /// The component was removed from the entity
    Removed,
    /// The component was modified
    Modified,
}

/// Represents the raw form of an event, where the archetype is available
pub struct EventData<'a> {
    /// The affected entities
    pub ids: &'a [Entity],
    /// The affected component
    pub key: ComponentKey,
    /// The kind of event
    pub kind: EventKind,
}

/// Allows subscribing to events *inside* the ECS, such as components being added, removed, or
/// modified.
pub trait EventSubscriber: ComponentValue {
    /// Handle an incoming event
    fn on_event(&self, event: &EventData);

    /// Returns true if the subscriber is interested in this archetype
    #[inline]
    fn matches_arch(&self, _: &Archetype) -> bool {
        true
    }

    /// Returns true if the subscriber is interested in this component
    #[inline]
    fn matches_component(&self, _: ComponentDesc) -> bool {
        true
    }

    /// Returns true if the subscriber is still connected
    fn is_connected(&self) -> bool;

    /// Filter each event before it is generated through a custom function
    fn filter<F>(self, func: F) -> FilterFunc<Self, F>
    where
        Self: Sized,
        F: Fn(&EventData) -> bool,
    {
        FilterFunc {
            subscriber: self,
            filter: func,
        }
    }

    /// Filter the archetypes for which the subscriber will receive events
    fn filter_arch<F: StaticFilter>(self, filter: F) -> FilterArch<Self, F>
    where
        Self: Sized,
    {
        FilterArch {
            filter,
            subscriber: self,
        }
    }

    /// Filter a subscriber to only receive events for a specific set of components
    fn filter_components<I: IntoIterator<Item = ComponentKey>>(
        self,
        components: I,
    ) -> FilterComponents<Self>
    where
        Self: Sized,
    {
        FilterComponents {
            components: components.into_iter().collect(),
            subscriber: self,
        }
    }
}

#[cfg(feature = "flume")]
impl EventSubscriber for flume::Sender<Event> {
    fn on_event(&self, event: &EventData) {
        for &id in event.ids {
            let _ = self.send(Event {
                id,
                key: event.key,
                kind: event.kind,
            });
        }
    }

    fn is_connected(&self) -> bool {
        !self.is_disconnected()
    }
}

#[cfg(feature = "tokio")]
impl EventSubscriber for tokio::sync::mpsc::UnboundedSender<Event> {
    fn on_event(&self, event: &EventData) {
        for &id in event.ids {
            let _ = self.send(Event {
                id,
                key: event.key,
                kind: event.kind,
            });
        }
    }

    fn is_connected(&self) -> bool {
        !self.is_closed()
    }
}

#[cfg(feature = "tokio")]
impl EventSubscriber for alloc::sync::Weak<tokio::sync::Notify> {
    fn on_event(&self, _: &EventData) {
        if let Some(notify) = self.upgrade() {
            notify.notify_one()
        }
    }

    fn is_connected(&self) -> bool {
        self.strong_count() > 0
    }
}

/// Filter the archetypes for which the subscriber will receive events
pub struct FilterArch<S, F> {
    filter: F,
    subscriber: S,
}

impl<S, F> EventSubscriber for FilterArch<S, F>
where
    S: EventSubscriber,
    F: ComponentValue + StaticFilter,
{
    #[inline]
    fn on_event(&self, event: &EventData) {
        self.subscriber.on_event(event);
    }

    #[inline]
    fn matches_arch(&self, arch: &Archetype) -> bool {
        self.filter.filter_static(arch) && self.subscriber.matches_arch(arch)
    }

    #[inline]
    fn matches_component(&self, desc: ComponentDesc) -> bool {
        self.subscriber.matches_component(desc)
    }

    #[inline]
    fn is_connected(&self) -> bool {
        self.subscriber.is_connected()
    }
}

/// Filter the archetypes for which the subscriber will receive events
pub struct FilterFunc<S, F> {
    filter: F,
    subscriber: S,
}

impl<S, F> EventSubscriber for FilterFunc<S, F>
where
    S: EventSubscriber,
    F: ComponentValue + Fn(&EventData) -> bool,
{
    #[inline]
    fn on_event(&self, event: &EventData) {
        if (self.filter)(event) {
            self.subscriber.on_event(event);
        }
    }

    #[inline]
    fn matches_arch(&self, arch: &Archetype) -> bool {
        self.subscriber.matches_arch(arch)
    }

    #[inline]
    fn matches_component(&self, desc: ComponentDesc) -> bool {
        self.subscriber.matches_component(desc)
    }

    #[inline]
    fn is_connected(&self) -> bool {
        self.subscriber.is_connected()
    }
}

/// Filter a subscriber to only receive events for a specific set of components
pub struct FilterComponents<S> {
    components: Vec<ComponentKey>,
    subscriber: S,
}

impl<S> EventSubscriber for FilterComponents<S>
where
    S: EventSubscriber,
{
    #[inline]
    fn on_event(&self, event: &EventData) {
        self.subscriber.on_event(event)
    }

    #[inline]
    fn matches_arch(&self, arch: &Archetype) -> bool {
        self.components.iter().any(|&key| arch.has(key)) && self.subscriber.matches_arch(arch)
    }

    #[inline]
    fn matches_component(&self, desc: ComponentDesc) -> bool {
        self.components.contains(&desc.key()) && self.subscriber.matches_component(desc)
    }

    #[inline]
    fn is_connected(&self) -> bool {
        self.subscriber.is_connected()
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
