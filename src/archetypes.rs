use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};

use crate::{
    archetype::{Archetype, ArchetypeId},
    component::{dummy, ComponentDesc, ComponentKey},
    entity::{EntityKind, EntityStore, EntityStoreIter, EntityStoreIterMut},
    events::EventSubscriber,
    metadata::exclusive,
    Entity,
};

pub(crate) struct Archetypes {
    pub(crate) root: ArchetypeId,
    pub(crate) reserved: ArchetypeId,
    gen: u32,
    inner: EntityStore<Archetype>,

    // These trickle down to the archetypes
    subscribers: Vec<Arc<dyn EventSubscriber>>,
    pub(crate) index: ArchetypeIndex,
}

impl Archetypes {
    pub fn new() -> Self {
        let mut archetypes = EntityStore::new(EntityKind::empty());
        let root = archetypes.spawn(Archetype::empty());
        let reserved = archetypes.spawn(Archetype::empty());

        let mut index = ArchetypeIndex::new();
        index.register(root, archetypes.get(root).unwrap());

        Self {
            root,
            inner: archetypes,
            gen: 2,
            reserved,
            subscribers: Vec::new(),
            index: ArchetypeIndex::new(),
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
    // pub(crate) fn prune_arch(&mut self, arch_id: ArchetypeId) -> usize {
    //     let arch = self.get(arch_id);
    //     if arch_id == self.root
    //         || arch_id == self.reserved
    //         || !arch.is_empty()
    //         || !arch.outgoing.is_empty()
    //     {
    //         return 0;
    //     }

    //     let arch = self.inner.despawn(arch_id).unwrap();
    //     let mut count = 1;
    //     for (&key, &dst_id) in &arch.incoming {
    //         let dst = self.get_mut(dst_id);
    //         dst.remove_link(key);

    //         count += self.prune_arch(dst_id);
    //     }

    //     self.gen = self.gen.wrapping_add(1);

    //     count
    // }

    /// Prunes a leaf and its ancestors from empty archetypes
    pub(crate) fn prune_all(&mut self) -> usize {
        fn prune(
            archetypes: &EntityStore<Archetype>,
            id: ArchetypeId,
            res: &mut Vec<ArchetypeId>,
        ) -> bool {
            let arch = archetypes.get(id).unwrap();

            // An archetype can be removed iff all its children are removed
            let mut pruned_children = true;
            for &id in arch.children.values() {
                pruned_children = prune(archetypes, id, res) && pruned_children;
            }

            if pruned_children && arch.is_empty() {
                res.push(id);
                true
            } else {
                false
            }
        }

        let mut to_remove = Vec::new();
        for &id in self.get(self.root()).children.values() {
            prune(&self.inner, id, &mut to_remove);
        }

        if to_remove.is_empty() {
            return 0;
        }

        let count = to_remove.len();
        for id in to_remove {
            let arch = self.inner.despawn(id).unwrap();
            self.index.unregister(id, &arch);

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
    pub(crate) fn find_create(
        &mut self,
        components: impl IntoIterator<Item = ComponentDesc>,
    ) -> (ArchetypeId, &mut Archetype) {
        let mut cursor = self.root;

        for head in components {
            let cur = &mut self.inner.get(cursor).expect("Invalid archetype id");

            cursor = match cur.outgoing.get(&head.key) {
                Some(&id) => id,
                None => {
                    // Create archetypes as we go and build the tree
                    let arch_components = cur.components_desc().chain([head]);

                    // Ensure exclusive property of the new component are maintained
                    let mut new = if head.is_relation() && head.meta_ref().has(exclusive()) {
                        // Remove any existing components of the same relation
                        // `head` is always a more recently added component since an
                        // archetype with it does not exist (yet)
                        Archetype::new(
                            arch_components
                                .filter(|v| v.key.id != head.key.id || v.key == head.key),
                        )
                    } else {
                        Archetype::new(arch_components)
                    };

                    // Insert the appropriate subscribers
                    for s in &self.subscribers {
                        if s.matches_arch(&new) {
                            new.add_handler(s.clone())
                        }
                    }

                    // Increase gen
                    self.gen = self.gen.wrapping_add(1);
                    let new_id = self.inner.spawn(new);

                    let (cur, new) = self.inner.get_disjoint(cursor, new_id).unwrap();
                    cur.add_child(head.key, new_id);
                    new.add_incoming(head.key, cursor);

                    self.index.register(new_id, new);

                    new_id
                }
            };
        }

        (cursor, self.inner.get_mut(cursor).unwrap())
    }

    pub fn root(&self) -> ArchetypeId {
        self.root
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
                    .remove(&ComponentKey::new(dummy(), key.target));

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
