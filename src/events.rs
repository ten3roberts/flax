use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    archetype::Archetype, And, ChangeKind, ComponentInfo, ComponentKey, ComponentValue, Entity,
    StaticFilter,
};

/// A subscriber of events to the world.
///
/// The implementation should not block
pub trait Subscriber: 'static + Send + Sync {
    /// Called then an entity is moved from one archetype to another
    /// This is called from the context of the source archetype
    fn on_moved_from(&self, _id: Entity, _from: &Archetype, _to: &Archetype) {}
    /// Same as [Subscriber::on_moved_from] but called from the context of the destination
    /// archetype
    fn on_moved_to(&self, _id: Entity, _from: &Archetype, _to: &Archetype) {}
    /// Called when a new entity is allocated in the archetype
    fn on_spawned(&self, _id: Entity, _arch: &Archetype) {}
    /// Called when an entity is completely removed from the archetypes.
    fn on_despawned(&self, _id: Entity, _arch: &Archetype) {}
    /// Invoked when a cell in the archetype is modified.
    ///
    /// **Note**: This is eager and will be invoked when it is accessed.
    fn on_change(&self, _component: ComponentInfo, _kind: ChangeKind) {}
    /// Returns true if the subscriber is to be kept alive
    fn is_connected(&self) -> bool;
    /// Returns true if the subscriber is interested in this archetype
    fn is_interested(&self, arch: &Archetype) -> bool;
    /// Returns true if the subscriber is interested in this archetype component
    fn is_interested_component(&self, component: ComponentKey) -> bool;
}

/// Provide a filter to any subscriber
pub trait SubscriberFilterExt<F>
where
    F: StaticFilter + ComponentValue,
{
    /// The filtered subscriber
    type Output: Subscriber;

    /// Attach a filter
    fn filter(self, filter: F) -> Self::Output;
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

// Apply a filter to another subscriber.
//
// If paired with [ArchetypeSubscriber] this has the effect of tracking if an entity's components
// where removed or added.
//
/// **Note**: When using Or combinatorials, the listener won't be invoked if the entity's
/// components are hopscotched. E.g; with a filter of (a() | b()), the listener wont be invoked
/// for: `(a) => (a, b) => (b)`, as there was never a time the entity did not match the filter.
pub struct FilterSubscriber<F, S> {
    filter: F,
    inner: S,
}

impl<F, S> FilterSubscriber<F, S> {
    /// Creates a new subscriber which will listen to archetype events on all matching archetypes.
    pub(crate) fn new(filter: F, inner: S) -> Self {
        Self { filter, inner }
    }
}

impl<F, G, S> SubscriberFilterExt<G> for FilterSubscriber<F, S>
where
    F: ComponentValue + StaticFilter,
    G: ComponentValue + StaticFilter,
    S: Subscriber,
{
    type Output = FilterSubscriber<And<F, G>, S>;

    fn filter(self, filter: G) -> Self::Output {
        FilterSubscriber::new(And::new(self.filter, filter), self.inner)
    }
}

/// Subscribe to events such as entities being spawned, despawned, or moved between archetypes
pub struct ArchetypeSubscriber<L> {
    listener: L,
    connected: AtomicBool,
}

impl<L> ArchetypeSubscriber<L> {
    /// Create a new subscriber to handle
    pub fn new(listener: L) -> Self {
        Self {
            listener,
            connected: AtomicBool::new(true),
        }
    }
}
impl<F, L> SubscriberFilterExt<F> for ArchetypeSubscriber<L>
where
    F: StaticFilter + ComponentValue,
    L: ComponentValue + EventHandler<ArchetypeEvent>,
{
    type Output = FilterSubscriber<F, Self>;

    fn filter(self, filter: F) -> Self::Output {
        FilterSubscriber::new(filter, self)
    }
}

impl<L> Subscriber for ArchetypeSubscriber<L>
where
    L: ComponentValue + EventHandler<ArchetypeEvent>,
{
    #[inline(always)]
    fn on_moved_from(&self, id: Entity, _from: &Archetype, _to: &Archetype) {
        if !self.listener.on_event(ArchetypeEvent::Removed(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    #[inline(always)]
    fn on_moved_to(&self, id: Entity, _from: &Archetype, _to: &Archetype) {
        if !self.listener.on_event(ArchetypeEvent::Inserted(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    #[inline(always)]
    fn on_spawned(&self, id: Entity, _arch: &Archetype) {
        if !self.listener.on_event(ArchetypeEvent::Inserted(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    #[inline(always)]
    fn on_despawned(&self, id: Entity, _arch: &Archetype) {
        if !self.listener.on_event(ArchetypeEvent::Inserted(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    #[inline(always)]
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    #[inline(always)]
    fn is_interested(&self, _: &Archetype) -> bool {
        true
    }

    #[inline(always)]
    fn is_interested_component(&self, _: ComponentKey) -> bool {
        false
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

// #[cfg(feature = "flume")]
// TODO weak sender
// impl<T> EventHandler<T> for flume::WeakSender<T> {
//     fn on_event(&self, event: T) -> bool {
//         self.send(event).is_ok()
//     }
// }

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
impl<T> EventHandler<T> for alloc::sync::Weak<tokio::sync::Notify> {
    fn on_event(&self, _: T) -> bool {
        if let Some(notify) = self.upgrade() {
            notify.notify_one();
            true
        } else {
            false
        }
    }
}

impl<F, S> Subscriber for FilterSubscriber<F, S>
where
    F: 'static + StaticFilter + Send + Sync,
    S: Subscriber,
{
    #[inline(always)]
    fn on_moved_from(&self, id: Entity, from: &Archetype, to: &Archetype) {
        let b = self.filter.static_matches(to);

        if !b {
            self.inner.on_moved_from(id, from, to)
        }
    }

    #[inline(always)]
    fn on_moved_to(&self, id: Entity, from: &Archetype, to: &Archetype) {
        let a = self.filter.static_matches(from);

        if !a {
            self.inner.on_moved_to(id, from, to)
        }
    }

    #[inline(always)]
    fn on_spawned(&self, id: Entity, arch: &Archetype) {
        if self.filter.static_matches(arch) {
            self.inner.on_spawned(id, arch)
        }
    }

    #[inline(always)]
    fn on_despawned(&self, id: Entity, arch: &Archetype) {
        if self.filter.static_matches(arch) {
            self.inner.on_despawned(id, arch)
        }
    }

    #[inline(always)]
    fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    #[inline]
    fn is_interested(&self, arch: &Archetype) -> bool {
        self.filter.static_matches(arch) && self.inner.is_connected()
    }

    #[inline(always)]
    fn is_interested_component(&self, component: ComponentKey) -> bool {
        self.inner.is_interested_component(component)
    }
}

/// Subscribe to changes to a set of components
pub struct ChangeSubscriber<L> {
    listener: L,
    components: Box<[ComponentKey]>,
    connected: AtomicBool,
}

impl<L> ChangeSubscriber<L> {
    /// Creates a new change subscriber, which will track changes, similar to a query
    pub fn new(components: &[ComponentKey], listener: L) -> Self {
        Self {
            components: components.into(),
            listener,
            connected: AtomicBool::new(true),
        }
    }
}

impl<F, L> SubscriberFilterExt<F> for ChangeSubscriber<L>
where
    F: StaticFilter + ComponentValue,
    L: ComponentValue + EventHandler<ChangeEvent>,
{
    type Output = FilterSubscriber<F, Self>;

    fn filter(self, filter: F) -> Self::Output {
        FilterSubscriber::new(filter, self)
    }
}

impl<L> Subscriber for ChangeSubscriber<L>
where
    L: ComponentValue + EventHandler<ChangeEvent>,
{
    fn on_change(&self, component: ComponentInfo, kind: ChangeKind) {
        if !self.listener.on_event(ChangeEvent {
            kind,
            component: component.key(),
        }) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn is_interested(&self, arch: &Archetype) -> bool {
        self.components.iter().any(|&v| arch.has(v))
    }

    fn is_interested_component(&self, component: ComponentKey) -> bool {
        self.components.contains(&component)
    }
}
