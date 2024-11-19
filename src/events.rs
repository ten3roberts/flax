use alloc::vec::Vec;
use bitflags::bitflags;
use itertools::Itertools;

use crate::{
    archetype::{Archetype, Slice, Storage},
    component::{ComponentDesc, ComponentKey, ComponentValue},
    filter::StaticFilter,
    sink::Sink,
    Component, Entity,
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

impl Event {
    /// Construct a new event
    pub fn new(id: Entity, key: ComponentKey, kind: EventKind) -> Self {
        Self { id, key, kind }
    }

    /// Construct a modified event
    pub fn modified(id: Entity, key: ComponentKey) -> Self {
        Self::new(id, key, EventKind::Modified)
    }

    /// Construct an added event
    pub fn added(id: Entity, key: ComponentKey) -> Self {
        Self::new(id, key, EventKind::Added)
    }
}

bitflags! {
    /// The type of ECS event
    pub struct EventKindFilter: u8 {
        /// Component was added
        const ADDED = 1;
        /// Component was removed
        const REMOVED = 2;
        /// Component was modified
        const MODIFIED = 4;
    }
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
    /// The affected slots
    pub slots: Slice,
    /// The affected component
    pub key: ComponentKey,
}

/// Allows subscribing to events *inside* the ECS, such as components being added, removed, or
/// modified.
///
/// Most implementations are through the [`Sink`] implementation, which sends a static event for
/// each entity affected by the event.
pub trait EventSubscriber: ComponentValue {
    /// Handle an incoming event
    fn on_added(&self, storage: &Storage, event: &EventData);
    /// Handle an incoming event
    ///
    /// **Note**: Component storage is inaccessible during this call as it may be called *during*
    /// itereation or while a query borrow is alive.
    ///
    /// Prefer to use this for cache validation and alike, as it *will* be called for intermediate
    /// events.
    fn on_modified(&self, event: &EventData);
    /// Handle an incoming event
    fn on_removed(&self, storage: &Storage, event: &EventData);

    /// Returns true if the subscriber is still connected
    fn is_connected(&self) -> bool;

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

    /// Filter each event before it is generated through a custom function
    fn filter<F>(self, func: F) -> FilterFunc<Self, F>
    where
        Self: Sized,
        F: Fn(EventKind, &EventData) -> bool,
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

    /// Filter a subscriber to only receive events for a specific set of components
    fn filter_relations<I: IntoIterator<Item = Entity>>(self, relations: I) -> FilterRelations<Self>
    where
        Self: Sized,
    {
        FilterRelations {
            relations: relations.into_iter().collect(),
            subscriber: self,
        }
    }

    /// Filter a subscriber to only receive events of a specific kind
    fn filter_event_kind(self, event_kind: EventKindFilter) -> FilterEventKind<Self>
    where
        Self: Sized,
    {
        FilterEventKind {
            event_kind,
            subscriber: self,
        }
    }
}

impl<S> EventSubscriber for S
where
    S: 'static + Send + Sync + Sink<Event>,
{
    fn on_added(&self, _: &Storage, event: &EventData) {
        for &id in event.ids {
            self.send(Event {
                id,
                key: event.key,
                kind: EventKind::Added,
            });
        }
    }

    fn on_modified(&self, event: &EventData) {
        for &id in event.ids {
            self.send(Event {
                id,
                key: event.key,
                kind: EventKind::Modified,
            });
        }
    }

    fn on_removed(&self, _: &Storage, event: &EventData) {
        for &id in event.ids {
            self.send(Event {
                id,
                key: event.key,
                kind: EventKind::Removed,
            });
        }
    }

    fn is_connected(&self) -> bool {
        <Self as Sink<Event>>::is_connected(self)
    }
}

/// Receive the component value of an event
///
/// This is a convenience wrapper around [`EventSubscriber`] that sends the component value along
///
/// **Note**: This only tracks addition and removal of components, not modification. This is due to
/// a limitation with references lifetimes during iteration, as the values can't be accessed by the
/// subscriber simultaneously.
pub struct WithValue<T, S> {
    component: Component<T>,
    sink: S,
}

impl<T, S> WithValue<T, S> {
    /// Create a new `WithValue` subscriber
    pub fn new(component: Component<T>, sink: S) -> Self {
        Self { component, sink }
    }
}

impl<T: ComponentValue + Clone, S: 'static + Send + Sync + Sink<(Event, T)>> EventSubscriber
    for WithValue<T, S>
{
    fn on_added(&self, storage: &Storage, event: &EventData) {
        let values = storage.downcast_ref::<T>();
        for (&id, slot) in event.ids.iter().zip_eq(event.slots.as_range()) {
            let value = values[slot].clone();

            self.sink.send((
                Event {
                    id,
                    key: event.key,
                    kind: EventKind::Added,
                },
                value,
            ));
        }
    }

    fn on_modified(&self, _: &EventData) {}

    fn on_removed(&self, storage: &Storage, event: &EventData) {
        let values = storage.downcast_ref::<T>();
        for (&id, slot) in event.ids.iter().zip_eq(event.slots.as_range()) {
            let value = values[slot].clone();

            self.sink.send((
                Event {
                    id,
                    key: event.key,
                    kind: EventKind::Removed,
                },
                value,
            ));
        }
    }

    fn is_connected(&self) -> bool {
        self.sink.is_connected()
    }

    fn matches_component(&self, desc: ComponentDesc) -> bool {
        self.component.desc() == desc
    }

    fn matches_arch(&self, arch: &Archetype) -> bool {
        arch.has(self.component.key())
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
    fn on_added(&self, storage: &Storage, event: &EventData) {
        self.subscriber.on_added(storage, event)
    }

    fn on_modified(&self, event: &EventData) {
        self.subscriber.on_modified(event);
    }

    fn on_removed(&self, storage: &Storage, event: &EventData) {
        self.subscriber.on_removed(storage, event)
    }

    #[inline]
    fn is_connected(&self) -> bool {
        self.subscriber.is_connected()
    }

    #[inline]
    fn matches_arch(&self, arch: &Archetype) -> bool {
        self.filter.filter_static(arch) && self.subscriber.matches_arch(arch)
    }

    #[inline]
    fn matches_component(&self, desc: ComponentDesc) -> bool {
        self.subscriber.matches_component(desc)
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
    F: ComponentValue + Fn(EventKind, &EventData) -> bool,
{
    fn on_added(&self, storage: &Storage, event: &EventData) {
        if (self.filter)(EventKind::Added, event) {
            self.subscriber.on_added(storage, event)
        }
    }

    fn on_modified(&self, event: &EventData) {
        if (self.filter)(EventKind::Modified, event) {
            self.subscriber.on_modified(event)
        }
    }

    fn on_removed(&self, storage: &Storage, event: &EventData) {
        if (self.filter)(EventKind::Removed, event) {
            self.subscriber.on_removed(storage, event)
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
    fn on_added(&self, storage: &Storage, event: &EventData) {
        self.subscriber.on_added(storage, event)
    }

    fn on_modified(&self, event: &EventData) {
        self.subscriber.on_modified(event)
    }

    fn on_removed(&self, storage: &Storage, event: &EventData) {
        self.subscriber.on_removed(storage, event)
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

/// Filter a subscriber to only receive events for a specific set of relations
pub struct FilterRelations<S> {
    relations: Vec<Entity>,
    subscriber: S,
}

impl<S> EventSubscriber for FilterRelations<S>
where
    S: EventSubscriber,
{
    fn on_added(&self, storage: &Storage, event: &EventData) {
        self.subscriber.on_added(storage, event)
    }

    fn on_modified(&self, event: &EventData) {
        self.subscriber.on_modified(event)
    }

    fn on_removed(&self, storage: &Storage, event: &EventData) {
        self.subscriber.on_removed(storage, event)
    }

    #[inline]
    fn matches_arch(&self, arch: &Archetype) -> bool {
        self.relations
            .iter()
            .any(|&key| arch.relations_like(key).any(|_| true))
            && self.subscriber.matches_arch(arch)
    }

    #[inline]
    fn matches_component(&self, desc: ComponentDesc) -> bool {
        desc.key.is_relation()
            && self.relations.contains(&desc.key.id())
            && self.subscriber.matches_component(desc)
    }

    #[inline]
    fn is_connected(&self) -> bool {
        self.subscriber.is_connected()
    }
}

/// Filter a subscriber to only receive events of a specific kind
pub struct FilterEventKind<S> {
    event_kind: EventKindFilter,
    subscriber: S,
}

impl<S> EventSubscriber for FilterEventKind<S>
where
    S: EventSubscriber,
{
    fn on_added(&self, storage: &Storage, event: &EventData) {
        if self.event_kind.contains(EventKindFilter::ADDED) {
            self.subscriber.on_added(storage, event)
        }
    }

    fn on_modified(&self, event: &EventData) {
        if self.event_kind.contains(EventKindFilter::MODIFIED) {
            self.subscriber.on_modified(event)
        }
    }

    fn on_removed(&self, storage: &Storage, event: &EventData) {
        if self.event_kind.contains(EventKindFilter::REMOVED) {
            self.subscriber.on_removed(storage, event)
        }
    }

    #[inline]
    fn matches_component(&self, desc: ComponentDesc) -> bool {
        self.subscriber.matches_component(desc)
    }

    fn is_connected(&self) -> bool {
        self.subscriber.is_connected()
    }
}

/// Maps an event to the associated entity id.
pub struct WithIds<S> {
    sink: S,
}

impl<S> WithIds<S> {
    /// Create a new entity id sink
    pub fn new(sink: S) -> Self {
        Self { sink }
    }
}

impl<S: Sink<Entity>> Sink<Event> for WithIds<S> {
    fn send(&self, event: Event) {
        self.sink.send(event.id);
    }

    fn is_connected(&self) -> bool {
        self.sink.is_connected()
    }
}
