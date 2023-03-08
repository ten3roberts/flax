use alloc::sync::Arc;

use crate::{
    archetype::Archetype,
    entity::{EntityKind, EntityStore, EntityStoreIter, EntityStoreIterMut},
    events::EventHandler,
    ArchetypeId, ComponentInfo, Entity,
};

// fn is_sorted<T: Ord>(v: &[T]) -> bool {
//     v.windows(2).all(|w| w[0] < w[1])
// }

pub(crate) struct Archetypes {
    pub(crate) root: ArchetypeId,
    pub(crate) reserved: ArchetypeId,
    gen: u32,
    inner: EntityStore<Archetype>,

    // These trickle down to the archetypes
    subscribers: Vec<Arc<dyn EventHandler>>,
}

impl Archetypes {
    pub fn new() -> Self {
        let mut archetypes = EntityStore::new(EntityKind::empty());
        let root = archetypes.spawn(Archetype::empty());
        let reserved = archetypes.spawn(Archetype::empty());

        Self {
            root,
            inner: archetypes,
            gen: 2,
            reserved,
            subscribers: Vec::new(),
        }
    }

    pub fn get(&self, arch_id: ArchetypeId) -> &Archetype {
        match self.inner.get(arch_id) {
            Some(v) => v,
            None => {
                panic!("Invalid archetype: {arch_id}");
            }
        }
    }

    pub fn get_mut(&mut self, arch_id: ArchetypeId) -> &mut Archetype {
        let arch = self.inner.get_mut(arch_id).expect("Invalid archetype");

        arch
    }

    /// Prunes a leaf and its ancestors from empty archetypes
    pub(crate) fn prune_arch(&mut self, arch_id: ArchetypeId) -> bool {
        let arch = self.get(arch_id);
        if arch_id == self.root || !arch.is_empty() || !arch.outgoing.is_empty() {
            return false;
        }

        let arch = self.inner.despawn(arch_id).unwrap();

        for (&key, &dst_id) in &arch.incoming {
            let dst = self.get_mut(dst_id);
            dst.remove_link(key);

            self.prune_arch(dst_id);
        }

        self.gen = self.gen.wrapping_add(1);

        true
    }

    /// Get the archetype which has `components`.
    /// `components` must be sorted.
    pub(crate) fn init(
        &mut self,
        components: impl IntoIterator<Item = ComponentInfo>,
    ) -> (ArchetypeId, &mut Archetype) {
        let mut cursor = self.root;

        for head in components {
            let cur = &mut self.inner.get(cursor).expect("Invalid archetype id");

            cursor = match cur.outgoing.get(&head.key) {
                Some(&id) => id,
                None => {
                    let mut new = Archetype::new(cur.components().chain([head]));

                    // Insert the appropriate subscribers
                    for s in &self.subscribers {
                        if s.filter_arch(&new) {
                            new.add_handler(s.clone())
                        }
                    }

                    // Increase gen
                    self.gen = self.gen.wrapping_add(1);
                    let new_id = self.inner.spawn(new);

                    let (cur, new) = self.inner.get_disjoint(cursor, new_id).unwrap();
                    cur.add_child(head.key, new_id);
                    new.add_incoming(head.key, cursor);

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
        let arch = self.inner.despawn(id).expect("Despawn invalid archetype");

        // Remove outgoing edges
        for (&component, &dst_id) in &arch.incoming {
            let dst = self.get_mut(dst_id);
            dst.remove_link(component);
        }
        self.gen = self.gen.wrapping_add(1);

        arch
    }

    pub fn add_subscriber(&mut self, subscriber: Arc<dyn EventHandler>) {
        // Prune subscribers
        self.subscribers.retain(|v| v.is_connected());

        for (_, arch) in self.inner.iter_mut() {
            if subscriber.filter_arch(arch) {
                arch.add_handler(subscriber.clone());
            }
        }

        self.subscribers.push(subscriber)
    }

    pub(crate) fn gen(&self) -> u32 {
        self.gen
    }
}
