use proc_macro2::Span;
use quote::quote;
use syn::Ident;

#[test]
fn derive_fetch_struct() {
    let input = quote! {
        #[derive(Fetch)]
        pub struct Foo {
            a: Component<i32>,
            b: Mutable<i32>,
        }
    };

    let expected = quote! {

        #[doc = "The item yielded by Foo"]
        pub struct FooItem<'q> {
            a: <Component<i32> as flax_renamed::fetch::FetchItem<'q>>::Item,
            b: <Mutable<i32> as flax_renamed::fetch::FetchItem<'q>>::Item,
        }

        #[automatically_derived]
        impl<'q> flax_renamed::fetch::FetchItem<'q> for Foo {
            type Item = FooItem<'q>;
        }

        #[doc = "The prepared fetch for Foo"]
        pub struct PreparedFoo<'w> {
            a: <Component<i32> as flax_renamed::Fetch<'w>>::Prepared,
            b: <Mutable<i32> as flax_renamed::Fetch<'w>>::Prepared,
        }

        #[automatically_derived]
        impl<'w, 'q> flax_renamed::fetch::PreparedFetch<'q> for PreparedFoo<'w> {
            type Item = FooItem<'q>;

            #[inline]
            unsafe fn fetch(&'q mut self, slot: flax_renamed::archetype::Slot) -> Self::Item {
                Self::Item {
                    a: self.a.fetch(slot),
                    b: self.b.fetch(slot),
                }
            }

            #[inline]
            unsafe fn filter_slots(&mut self, slots: flax_renamed::archetype::Slice) -> flax_renamed::archetype::Slice {
                flax_renamed::fetch::PreparedFetch::filter_slots(&mut (&mut self.a, &mut self.b,), slots)
            }

            #[inline]
            fn set_visited(&mut self, slots: flax_renamed::archetype::Slice) {
                self.a.set_visited(slots);
                self.b.set_visited(slots);
            }
        }

        #[automatically_derived]
        impl<'w> flax_renamed::Fetch<'w> for Foo
        where
            Component<i32>: flax_renamed::Fetch<'w>,
            Mutable<i32>: flax_renamed::Fetch<'w>,
        {
            const MUTABLE: bool = <Component<i32> as flax_renamed::Fetch<'w>>::MUTABLE || <Mutable<i32> as flax_renamed::Fetch<'w>>::MUTABLE;

            type Prepared = PreparedFoo<'w>;

            #[inline]
            fn prepare(&'w self, data: flax_renamed::fetch::FetchPrepareData<'w>) -> Option<Self::Prepared> {
                Some(Self::Prepared {
                    a: self.a.prepare(data)?,
                    b: self.b.prepare(data)?,
                })
            }

            #[inline]
            fn filter_arch(&self, arch: &flax_renamed::archetype::Archetype) -> bool {
                self.a.filter_arch(arch) && self.b.filter_arch(arch)
            }

            fn describe(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut s = f.debug_struct(stringify!(Foo));
                s.field(stringify!(a), &flax_renamed::fetch::FmtQuery(&self.a));
                s.field(stringify!(b), &flax_renamed::fetch::FmtQuery(&self.b));
                s.finish()

            }

            fn access(&self, data: flax_renamed::fetch::FetchAccessData) -> Vec<flax_renamed::system::Access> {
                [self.a.access(data), self.b.access(data)].concat()
            }

            fn searcher(&self, searcher: &mut flax_renamed::query::ArchetypeSearcher) {
                self.a.searcher(searcher);
                self.b.searcher(searcher);
            }
        }

    };

    let output =
        super::do_derive_fetch(Ident::new("flax_renamed", Span::call_site()), input.into());

    pretty_assertions::assert_eq!(output.to_string(), expected.to_string());
}
