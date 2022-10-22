use crate::{archetype::Archetype, Entity, StaticFilter};

pub(crate) trait Subscriber: Send + Sync {
    fn on_moved_from(&self, id: Entity, from: &Archetype, to: &Archetype) -> bool;
    fn on_moved_to(&self, id: Entity, from: &Archetype, to: &Archetype) -> bool;
    fn on_spawned(&self, id: Entity, arch: &Archetype) -> bool;
    fn on_despawned(&self, id: Entity, arch: &Archetype) -> bool;
    fn is_interested(&self, arch: &Archetype) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Describes an event in the world
pub enum ArchetypeEvent {
    /// The entity matches the filter
    Inserted,
    /// The entity no longer matches the filter
    Removed,
}

pub(crate) struct FilterSubscriber<F, L> {
    pub(crate) filter: F,
    pub(crate) listener: L,
}

/// Defines a type which can handle a world event, such as a component removal
pub trait EventListener<T> {
    /// Returns true if the listener is to be retained
    fn on_event(&self, event: T) -> bool;
}

impl<T, F> EventListener<T> for F
where
    F: Fn(T) -> bool,
{
    fn on_event(&self, value: T) -> bool {
        (self)(value)
    }
}

#[cfg(feature = "flume")]
impl<T> EventListener<T> for flume::Sender<T> {
    fn on_event(&self, event: T) -> bool {
        self.send(event).is_ok()
    }
}

impl<F: StaticFilter + Send + Sync, L: Send + Sync + EventListener<(ArchetypeEvent, Entity)>>
    Subscriber for FilterSubscriber<F, L>
{
    fn on_moved_from(&self, id: Entity, from: &Archetype, to: &Archetype) -> bool {
        let a = self.filter.static_matches(from);
        let b = self.filter.static_matches(to);

        if a && !b {
            self.listener.on_event((ArchetypeEvent::Removed, id))
        } else {
            true
        }
    }

    fn on_moved_to(&self, id: Entity, from: &Archetype, to: &Archetype) -> bool {
        let a = self.filter.static_matches(from);
        let b = self.filter.static_matches(to);

        if !a && b {
            self.listener.on_event((ArchetypeEvent::Inserted, id))
        } else {
            true
        }
    }

    fn on_spawned(&self, id: Entity, arch: &Archetype) -> bool {
        if self.filter.static_matches(arch) {
            self.listener.on_event((ArchetypeEvent::Inserted, id))
        } else {
            true
        }
    }

    fn on_despawned(&self, id: Entity, arch: &Archetype) -> bool {
        if self.filter.static_matches(arch) {
            self.listener.on_event((ArchetypeEvent::Removed, id))
        } else {
            true
        }
    }

    fn is_interested(&self, arch: &Archetype) -> bool {
        self.filter.static_matches(arch)
    }
}
