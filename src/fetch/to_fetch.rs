use crate::Fetch;

/// Trait to convert a fetch item into a fetch.
///
/// Implemented for structs using the `Fetch` derive macro
pub trait ToFetch {
    type Fetch: for<'x> Fetch<'x>;
}

#[cfg(test)]
mod test {
    use std::marker::PhantomData;

    use flax_derive::Fetch;

    use crate::*;

    use super::*;

    // #[test]
    // fn derive_fetch() {
    //     // component! {
    //     //     position: Vec3 => [Debug],
    //     //     rotation: Quat => [Debug],
    //     //     scale: Vec3 => [Debug],
    //     // }

    //     use glam::*;
    //     #[derive(Debug, Clone)]
    //     struct MyFetch {
    //         pos: Component<Vec3>,
    //         rot: Opt<Component<Vec3>>,
    //         scale: Component<Vec3>,
    //     }

    //     impl<'a> MyFetch<'a> {
    //         pub fn to_fetch(&self) -> Box<dyn Fetch<Prepared> {
    //             struct F<A, B, C> {
    //                 pos: A,
    //                 rot: B,
    //                 scale: C,
    //             }

    //             impl<'q, A, B, C> PreparedFetch<'q> for F<A, B, C>
    //             where
    //                 A: PreparedFetch<'q>,
    //                 B: PreparedFetch<'q>,
    //                 C: PreparedFetch<'q>,
    //             {
    //                 type Item = MyFetch<'q>;

    //                 unsafe fn fetch(&'q mut self, slot: archetype::Slot) -> Self::Item {
    //                     todo!()
    //                 }
    //             }
    //             impl<'w, A, B, C> Fetch<'w> for F<A, B, C>
    //             where
    //                 A: Fetch<'w>,
    //                 B: Fetch<'w>,
    //                 C: Fetch<'w>,
    //             {
    //                 const MUTABLE: bool = A::MUTABLE | B::MUTABLE | C::MUTABLE;

    //                 type Prepared = F<A::Prepared, B::Prepared, C::Prepared>;

    //                 fn prepare(
    //                     &'w self,
    //                     world: &'w World,
    //                     archetype: &'w Archetype,
    //                 ) -> Option<Self::Prepared> {
    //                     Some(F {
    //                         pos: self.pos.prepare(world, archetype)?,
    //                         rot: self.rot.prepare(world, archetype)?,
    //                         scale: self.scale.prepare(world, archetype)?,
    //                     })
    //                 }

    //                 fn matches(&self, world: &'w World, archetype: &'w Archetype) -> bool {
    //                     todo!()
    //                 }

    //                 fn describe(&self) -> String {
    //                     todo!()
    //                 }

    //                 fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
    //                     todo!()
    //                 }

    //                 fn difference(&self, archetype: &Archetype) -> Vec<String> {
    //                     todo!()
    //                 }
    //             }

    //             F {
    //                 pos: position(),
    //                 rot: rotation().opt(),
    //                 scale: scale().opt_or(Vec3::ONE),
    //             }
    //         }
    //     }

    //     struct MyPreparedFetch<'a, A, B, C>
    //     where
    //         A: PreparedFetch<'a>,
    //         B: PreparedFetch<'a>,
    //         C: PreparedFetch<'a>,
    //     {
    //         pos: A,
    //         rot: B,
    //         scale: C,
    //         _marker: PhantomData<&'a ()>,
    //     }
    // }
}
