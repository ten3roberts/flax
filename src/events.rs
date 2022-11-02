use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{archetype::Archetype, ChangeKind, ComponentKey, Entity, StaticFilter};

pub(crate) trait Subscriber: Send + Sync {
    fn on_moved_from(&self, _id: Entity, _from: &Archetype, _to: &Archetype) {}
    fn on_moved_to(&self, _id: Entity, _from: &Archetype, _to: &Archetype) {}
    fn on_spawned(&self, _id: Entity, _arch: &Archetype) {}
    fn on_despawned(&self, _id: Entity, _arch: &Archetype) {}
    fn on_change(&self, _component: ComponentKey, _kind: ChangeKind) {}
    fn is_connected(&self) -> bool;
    fn is_interested(&self, arch: &Archetype) -> bool;
    fn is_interested_component(&self, component: ComponentKey) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Describes an event in the world
pub enum ArchetypeEvent {
    /// The entity matches the filter
    Inserted(Entity),
    /// The entity no longer matches the filter
    Removed(Entity),
}

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

pub(crate) struct FilterSubscriber<F, L> {
    pub(crate) filter: F,
    pub(crate) listener: L,
    connected: AtomicBool,
}

impl<F, L> FilterSubscriber<F, L> {
    pub(crate) fn new(filter: F, listener: L) -> Self {
        Self {
            filter,
            listener,
            connected: AtomicBool::new(true),
        }
    }
}

/// Defines a type which can handle a world event, such as a component removal
pub trait EventHandler<T> {
    /// Returns true if the listener is to be retained
    fn on_event(&self, event: T) -> bool;
}

impl<T, F> EventHandler<T> for F
where
    F: Fn(T) -> bool,
{
    fn on_event(&self, value: T) -> bool {
        (self)(value)
    }
}

#[cfg(feature = "flume")]
impl<T> EventHandler<T> for flume::Sender<T> {
    fn on_event(&self, event: T) -> bool {
        self.send(event).is_ok()
    }
}

#[cfg(feature = "tokio")]
impl<T> EventHandler<T> for tokio::sync::mpsc::UnboundedSender<T> {
    fn on_event(&self, event: T) -> bool {
        self.send(event).is_ok()
    }
}

#[cfg(feature = "tokio")]
impl<T> EventHandler<T> for tokio::sync::mpsc::Sender<T> {
    fn on_event(&self, event: T) -> bool {
        self.blocking_send(event).is_ok()
    }
}

#[cfg(feature = "tokio")]
impl<T> EventHandler<T> for tokio::sync::broadcast::Sender<T> {
    fn on_event(&self, event: T) -> bool {
        self.send(event).is_ok()
    }
}

#[cfg(feature = "tokio")]
impl<T> EventHandler<T> for Weak<tokio::sync::Notify> {
    fn on_event(&self, _: T) -> bool {
        if let Some(notify) = self.upgrade() {
            notify.notify_one();
            true
        } else {
            false
        }
    }
}

impl<F: StaticFilter + Send + Sync, L: Send + Sync + EventHandler<ArchetypeEvent>> Subscriber
    for FilterSubscriber<F, L>
{
    fn on_moved_from(&self, id: Entity, from: &Archetype, to: &Archetype) {
        let a = self.filter.static_matches(from);
        let b = self.filter.static_matches(to);

        if a && !b && !self.listener.on_event(ArchetypeEvent::Removed(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    fn on_moved_to(&self, id: Entity, from: &Archetype, to: &Archetype) {
        let a = self.filter.static_matches(from);
        let b = self.filter.static_matches(to);

        if !a && b && !self.listener.on_event(ArchetypeEvent::Inserted(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    fn on_spawned(&self, id: Entity, arch: &Archetype) {
        if self.filter.static_matches(arch) && !self.listener.on_event(ArchetypeEvent::Inserted(id))
        {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    fn on_despawned(&self, id: Entity, arch: &Archetype) {
        if self.filter.static_matches(arch) && !self.listener.on_event(ArchetypeEvent::Removed(id))
        {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    fn on_change(&self, _: ComponentKey, _: ChangeKind) {}

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn is_interested(&self, arch: &Archetype) -> bool {
        self.filter.static_matches(arch)
    }

    fn is_interested_component(&self, _: ComponentKey) -> bool {
        false
    }
}

pub(crate) struct ChangeSubscriber<F, L> {
    filter: F,
    components: Box<[ComponentKey]>,
    listener: L,
    connected: AtomicBool,
}

impl<F, L> ChangeSubscriber<F, L> {
    pub(crate) fn new(filter: F, components: Box<[ComponentKey]>, listener: L) -> Self {
        Self {
            filter,
            components,
            listener,
            connected: AtomicBool::new(true),
        }
    }
}

impl<F: StaticFilter + Send + Sync, L: Send + Sync + EventHandler<ChangeEvent>> Subscriber
    for ChangeSubscriber<F, L>
{
    fn on_change(&self, component: ComponentKey, kind: ChangeKind) {
        if !self.listener.on_event(ChangeEvent { kind, component }) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn is_interested(&self, arch: &Archetype) -> bool {
        self.filter.static_matches(arch) && self.components.iter().all(|&v| arch.has(v))
    }

    fn is_interested_component(&self, component: ComponentKey) -> bool {
        self.components.contains(&component)
    }
}
