use alloc::boxed::Box;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::{
    archetype::{Archetype, Slot},
    filter::StaticFilter,
    And, ChangeKind, Component, ComponentInfo, ComponentKey, ComponentValue, Entity, Fetch,
};

/// A subscriber of events to the world.
///
/// The implementation should not block
pub trait Subscriber: 'static + Send + Sync {
    /// Called then an entity is moved from one archetype to another
    /// This is called from the context of the source archetype **before** the entity components
    /// are moved
    fn on_moved_pre(&self, _id: Entity, _slot: Slot, _from: &Archetype, _to: &Archetype) {}
    /// Same as [Subscriber::on_moved_pre] but called from the context of the destination
    /// archetype
    fn on_moved_post(&self, _id: Entity, _from: &Archetype, _to: &Archetype) {}
    /// Called when a new entity is allocated in the world
    fn on_spawned(&self, _id: Entity, _arch: &Archetype) {}
    /// Called when an entity is completely removed from the archetypes.
    fn on_despawned(&self, _id: Entity, _slot: Slot, _arch: &Archetype) {}
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
    /// The entity was inserted into a matching archetype
    Inserted(Entity),
    /// The entity was removed from a matching archetype.
    /// Note: The entity could be moved to another still matching archetype, in which case an
    /// `Inserted` event is emitted afterwards
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
    F: ComponentValue + for<'x> Fetch<'x>,
    G: ComponentValue + for<'x> Fetch<'x>,
    S: Subscriber,
{
    type Output = FilterSubscriber<And<F, G>, S>;

    fn filter(self, filter: G) -> Self::Output {
        FilterSubscriber::new(And::new(self.filter, filter), self.inner)
    }
}

/// Event regarding a shape change of an entity.
///
/// This is similar to [ArchetypeEvent], but regards matching and then not
/// matching a filter.
#[derive(PartialEq, Eq, Debug, Clone)]
pub enum ShapeEvent {
    /// A entity fulfills the shape. This can be either because the entity was spawned directly
    /// with the required components, or the required components were inserted
    Matched(Entity),
    /// An entity no longer fulfills the shape, either because of despawn or component removal.
    Unmatched(Entity),
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

impl<F, G, L> SubscriberFilterExt<G> for ShapeSubscriber<F, L>
where
    F: StaticFilter + ComponentValue,
    G: StaticFilter + ComponentValue,
    L: ComponentValue + EventHandler<ShapeEvent>,
{
    type Output = FilterSubscriber<G, Self>;

    fn filter(self, filter: G) -> Self::Output {
        FilterSubscriber::new(filter, self)
    }
}

impl<F, L> Subscriber for ShapeSubscriber<F, L>
where
    F: StaticFilter + ComponentValue,
    L: ComponentValue + EventHandler<ShapeEvent>,
{
    #[inline(always)]
    fn on_moved_pre(&self, id: Entity, _slot: Slot, _from: &Archetype, to: &Archetype) {
        // Shape still matches
        if self.shape.filter_static(to) {
            return;
        }

        // If the shape was moved to an archetype not matching the shape, generate a
        // unmatched event.
        if !self.listener.on_event(ShapeEvent::Unmatched(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    #[inline(always)]
    fn on_moved_post(&self, id: Entity, from: &Archetype, _to: &Archetype) {
        // Shape matched before and now
        if self.shape.filter_static(from) {
            return;
        }

        // If the shape was from an archetype not matching the shape generate an
        // matched event.
        if !self.listener.on_event(ShapeEvent::Matched(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    #[inline(always)]
    fn on_spawned(&self, id: Entity, _arch: &Archetype) {
        if !self.listener.on_event(ShapeEvent::Matched(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    #[inline(always)]
    fn on_despawned(&self, id: Entity, _slot: Slot, _arch: &Archetype) {
        if !self.listener.on_event(ShapeEvent::Unmatched(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    #[inline(always)]
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    #[inline(always)]
    fn is_interested(&self, arch: &Archetype) -> bool {
        self.shape.filter_static(arch)
    }

    #[inline(always)]
    fn is_interested_component(&self, _: ComponentKey) -> bool {
        false
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
    fn on_moved_pre(&self, id: Entity, _slot: Slot, _from: &Archetype, _to: &Archetype) {
        if !self.listener.on_event(ArchetypeEvent::Removed(id)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    #[inline(always)]
    fn on_moved_post(&self, id: Entity, _from: &Archetype, _to: &Archetype) {
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
    fn on_despawned(&self, id: Entity, _slot: Slot, _arch: &Archetype) {
        if !self.listener.on_event(ArchetypeEvent::Removed(id)) {
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
    F: ComponentValue + StaticFilter,
    S: Subscriber,
{
    #[inline(always)]
    fn on_moved_pre(&self, id: Entity, slot: Slot, from: &Archetype, to: &Archetype) {
        self.inner.on_moved_pre(id, slot, from, to)
    }

    #[inline(always)]
    fn on_moved_post(&self, id: Entity, from: &Archetype, to: &Archetype) {
        self.inner.on_moved_post(id, from, to)
    }

    #[inline(always)]
    fn on_spawned(&self, id: Entity, arch: &Archetype) {
        self.inner.on_spawned(id, arch)
    }

    #[inline(always)]
    fn on_despawned(&self, id: Entity, slot: Slot, arch: &Archetype) {
        self.inner.on_despawned(id, slot, arch)
    }

    #[inline(always)]
    fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    #[inline]
    fn is_interested(&self, arch: &Archetype) -> bool {
        self.filter.filter_static(arch) && self.inner.is_interested(arch)
    }

    #[inline(always)]
    fn is_interested_component(&self, component: ComponentKey) -> bool {
        self.inner.is_interested_component(component)
    }

    fn on_change(&self, component: ComponentInfo, kind: ChangeKind) {
        self.inner.on_change(component, kind)
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

/// Subscribe to changes to a set of components
pub struct RemoveSubscriber<T: ComponentValue, L> {
    listener: L,
    component: Component<T>,
    connected: AtomicBool,
}

impl<T: ComponentValue, L: EventHandler<(Entity, T)>> RemoveSubscriber<T, L> {
    /// Creates a new change subscriber, which will track changes, similar to a query
    pub fn new(component: Component<T>, listener: L) -> Self {
        Self {
            listener,
            component,
            connected: AtomicBool::new(true),
        }
    }
}

impl<T, F, L> SubscriberFilterExt<F> for RemoveSubscriber<T, L>
where
    F: StaticFilter + ComponentValue,
    T: ComponentValue + Clone,
    L: ComponentValue + EventHandler<(Entity, T)>,
{
    type Output = FilterSubscriber<F, Self>;

    fn filter(self, filter: F) -> Self::Output {
        FilterSubscriber::new(filter, self)
    }
}

impl<T, L> Subscriber for RemoveSubscriber<T, L>
where
    T: ComponentValue + Clone,
    L: ComponentValue + EventHandler<(Entity, T)>,
{
    fn on_moved_pre(&self, id: Entity, slot: Slot, from: &Archetype, to: &Archetype) {
        if !to.has(self.component.key()) {
            let value = from.get(slot, self.component).unwrap().clone();
            if !self.listener.on_event((id, value)) {
                self.connected.store(false, Ordering::Relaxed)
            }
        }
    }

    fn on_despawned(&self, id: Entity, slot: Slot, arch: &Archetype) {
        let value = arch.get(slot, self.component).unwrap().clone();
        if !self.listener.on_event((id, value)) {
            self.connected.store(false, Ordering::Relaxed)
        }
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn is_interested(&self, arch: &Archetype) -> bool {
        arch.has(self.component.key())
    }

    fn is_interested_component(&self, _: ComponentKey) -> bool {
        false
    }
}
