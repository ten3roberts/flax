use core::fmt::{Debug, Formatter};

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use itertools::Itertools;

use crate::{
    archetype::{Archetype, ArchetypeId},
    component::{dummy, ComponentDesc, ComponentKey},
    entity::{EntityKind, EntityStore, EntityStoreIter, EntityStoreIterMut},
    events::EventSubscriber,
    Entity,
};

pub(crate) struct Archetypes {
    pub(crate) empty: ArchetypeId,
    pub(crate) reserved: ArchetypeId,
    gen: u32,
    inner: EntityStore<Archetype>,
    archetypes: BTreeMap<Vec<ComponentDesc>, ArchetypeId>,

    // These trickle down to the archetypes
    subscribers: Vec<Arc<dyn EventSubscriber>>,
    pub(crate) index: ArchetypeIndex,
}

impl Archetypes {
    pub fn new() -> Self {
        let mut inner = EntityStore::new(EntityKind::empty());
        let empty = inner.spawn(Archetype::empty());
        let reserved = inner.spawn(Archetype::empty());

        let mut index = ArchetypeIndex::new();
        index.register(empty, inner.get(empty).unwrap());
        let mut archetypes = BTreeMap::new();
        archetypes.insert(Vec::new(), empty);

        Self {
            empty,
            inner,
            gen: 2,
            reserved,
            subscribers: Vec::new(),
            index: ArchetypeIndex::new(),
            archetypes,
        }
    }

    #[track_caller]
    pub fn get(&self, arch_id: ArchetypeId) -> &Archetype {
        match self.inner.get(arch_id) {
            Some(v) => v,
            None => {
                panic!("Invalid archetype: {arch_id}");
            }
        }
    }

    #[track_caller]
    pub fn get_mut(&mut self, arch_id: ArchetypeId) -> &mut Archetype {
        let arch = self.inner.get_mut(arch_id).expect("Invalid archetype");

        arch
    }

    /// Prunes a leaf and its ancestors from empty archetypes
    pub(crate) fn prune_all(&mut self) -> usize {
        let to_remove = self
            .inner
            .iter()
            .filter(|v| v.0 != self.empty && v.0 != self.reserved && v.1.is_empty())
            .map(|v| v.0)
            .collect_vec();

        if to_remove.is_empty() {
            return 0;
        }

        let count = to_remove.len();
        for id in to_remove {
            let arch = self.inner.despawn(id).unwrap();
            self.index.unregister(id, &arch);
            self.archetypes
                .remove(&arch.components_desc().collect_vec())
                .unwrap();

            for (&key, &dst_id) in &arch.incoming {
                self.get_mut(dst_id).remove_link(key);
            }

            for (key, &dst_id) in &arch.outgoing {
                self.get_mut(dst_id).incoming.remove(key);
            }
        }

        self.gen = self.gen.wrapping_add(1);

        count
    }

    /// Returns or creates an archetype which satisfies all the given components
    ///
    /// Get the archetype which has `components`.
    /// `components` must be sorted.
    ///
    /// Ensures the `exclusive` property of any relations are satisfied
    pub(crate) fn find_or_create(
        &mut self,
        components: impl IntoIterator<Item = ComponentDesc>,
    ) -> (ArchetypeId, &mut Archetype) {
        // let mut cursor = self.empty;

        let keys = components.into_iter().collect_vec();
        let id = *self.archetypes.entry(keys).or_insert_with_key(|keys| {
            let mut arch = Archetype::new(keys.to_vec());
            // Insert the appropriate subscribers
            for s in &self.subscribers {
                if s.matches_arch(&arch) {
                    arch.add_handler(s.clone())
                }
            }

            self.gen = self.gen.wrapping_add(1);
            let id = self.inner.spawn(arch);
            self.index.register(id, self.inner.get(id).unwrap());
            id
        });

        (id, self.inner.get_mut(id).expect("Invalid archetype id"))
    }

    pub fn get_disjoint(
        &mut self,
        a: Entity,
        b: Entity,
    ) -> Option<(&mut Archetype, &mut Archetype)> {
        let (a, b) = self.inner.get_disjoint(a, b)?;

        Some((a, b))
    }

    pub fn iter(&self) -> EntityStoreIter<Archetype> {
        self.inner.iter()
    }

    pub fn iter_mut(&mut self) -> EntityStoreIterMut<Archetype> {
        self.inner.iter_mut()
    }

    /// Despawn an archetype, leaving a hole in the tree.
    ///
    /// It is the callers responibility to cleanup child nodes if the node is internal
    /// Children are detached from the tree, but still accessible by id
    pub fn despawn(&mut self, id: Entity) -> Archetype {
        profile_function!();
        let arch = self.inner.despawn(id).expect("Despawn invalid archetype");
        self.index.unregister(id, &arch);

        // Remove outgoing edges
        for (&component, &dst_id) in &arch.incoming {
            let dst = self.get_mut(dst_id);
            dst.remove_link(component);
        }

        for (key, &dst_id) in &arch.outgoing {
            self.get_mut(dst_id).incoming.remove(key);
        }

        self.gen = self.gen.wrapping_add(1);

        arch
    }

    pub fn add_subscriber(&mut self, subscriber: Arc<dyn EventSubscriber>) {
        // Prune subscribers
        self.subscribers.retain(|v| v.is_connected());

        for (_, arch) in self.inner.iter_mut() {
            if subscriber.matches_arch(arch) {
                arch.add_handler(subscriber.clone());
            }
        }

        self.subscribers.push(subscriber)
    }

    pub(crate) fn gen(&self) -> u32 {
        self.gen
    }
}

#[derive(Debug)]
pub(crate) struct ArchetypeRecord {
    // arch_id: ArchetypeId,
    cell_index: usize,
    /// The number of relations for this component.
    ///
    /// Since they are ordered sequentially, they start at `cell_index` and continue for `relation_count`
    relation_count: usize,
}

pub(crate) type ArchetypeRecords = BTreeMap<ArchetypeId, ArchetypeRecord>;
pub(crate) struct ArchetypeIndex {
    components: BTreeMap<ComponentKey, ArchetypeRecords>,
}

impl Debug for ArchetypeIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ArchetypeIndex")
            .field("components", &self.components)
            .finish()
    }
}

impl ArchetypeIndex {
    pub(crate) fn new() -> Self {
        Self {
            components: BTreeMap::new(),
        }
    }

    fn register_relation(&mut self, arch_id: ArchetypeId, key: ComponentKey, cell_index: usize) {
        let records = self
            .components
            .entry(key)
            .or_default()
            .entry(arch_id)
            .or_insert(ArchetypeRecord {
                cell_index,
                relation_count: 0,
            });

        records.relation_count += 1;
        assert_eq!(records.cell_index + records.relation_count, cell_index + 1);
    }

    fn unregister_relation(&mut self, arch_id: ArchetypeId, key: ComponentKey) {
        let records = self.components.get_mut(&key).unwrap();
        let record = records.get_mut(&arch_id).unwrap();

        record.relation_count -= 1;
        if record.relation_count == 0 {
            records.remove(&arch_id);
        }
    }

    pub(crate) fn register(&mut self, arch_id: ArchetypeId, arch: &Archetype) {
        profile_function!();
        for (&key, &cell_index) in arch.components() {
            if key.is_relation() {
                assert!(key.target.is_some());
                self.components
                    .entry(ComponentKey::new(dummy(), key.target))
                    .or_default()
                    .insert(
                        arch_id,
                        ArchetypeRecord {
                            cell_index,
                            relation_count: 0,
                        },
                    );
                self.register_relation(
                    arch_id,
                    ComponentKey::new(key.id(), Some(dummy())),
                    cell_index,
                );
            }

            self.components.entry(key).or_default().insert(
                arch_id,
                ArchetypeRecord {
                    cell_index,
                    relation_count: 0,
                },
            );
        }
    }

    pub(crate) fn unregister(&mut self, arch_id: ArchetypeId, arch: &Archetype) {
        profile_function!();
        for key in arch.components().keys() {
            if key.is_relation() {
                assert!(key.target.is_some());
                self.components
                    .entry(ComponentKey::new(dummy(), key.target))
                    .and_modify(|v| {
                        v.remove(&arch_id);
                    });

                self.unregister_relation(arch_id, ComponentKey::new(key.id(), Some(dummy())));
            }

            let records = self.components.get_mut(key).unwrap();
            records.remove(&arch_id);
            if records.is_empty() {
                self.components.remove(key);
            }
        }
    }

    pub(crate) fn find(&self, component: ComponentKey) -> Option<&ArchetypeRecords> {
        self.components.get(&component)
    }

    /// Returns all archetypes which have the given relation, regardless of target
    pub(crate) fn find_relation(&self, relation: Entity) -> Option<&ArchetypeRecords> {
        self.components
            .get(&ComponentKey::new(relation, Some(dummy())))
    }

    /// Returns all archetypes with at least one relation targeting `id`
    pub(crate) fn find_relation_targets(&self, id: Entity) -> Option<&ArchetypeRecords> {
        self.components.get(&ComponentKey::new(dummy(), Some(id)))
    }
}
