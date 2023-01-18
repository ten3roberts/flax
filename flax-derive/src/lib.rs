use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use proc_macro_crate::FoundCrate;
use quote::quote;
use syn::*;

/// ```rust
/// use glam::*;
/// #[derive(Fetch)]
/// #[fetch(Debug)]
/// struct CustomFetch {
///     position: Component<Vec3>,
///     rotation: Mutable<Quat>,
/// }
/// ```
#[proc_macro_derive(Fetch, attributes(fetch))]
pub fn derive_fetch(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let crate_name = match proc_macro_crate::crate_name("flax").expect("Failed to get crate name") {
        FoundCrate::Itself => Ident::new("crate", Span::call_site()),
        FoundCrate::Name(name) => Ident::new(&name, Span::call_site()),
    };

    match input.data {
        syn::Data::Struct(ref data) => derive_data_struct(crate_name, &input, data)
            .unwrap_or_else(|err| err.to_compile_error())
            .into(),
        syn::Data::Enum(_) => todo!(),
        syn::Data::Union(_) => todo!(),
    }
}

fn derive_item_struct<'a>(
    crate_name: &Ident,
    attrs: &Attrs,
    vis: &Visibility,
    name: &Ident,
    item_name: &Ident,
    fields: impl Iterator<Item = &'a Field>,
) -> TokenStream {
    let fields = fields.map(|field| {
        let name = field
            .ident
            .as_ref()
            .expect("Only named fields are supported");

        let ty = &field.ty;

        quote! {
            #name: <#ty as #crate_name::FetchItem<'q>>::Item
        }
    });

    let msg = format!("The item yielded by {name}");

    let extras = match attrs.extras {
        Some(ref extras) => {
            let nested = &extras.nested;
            quote! { #[derive(#nested)]}
        }
        None => quote! {},
    };

    quote! {
        #[doc = #msg]
        #extras
        #vis struct #item_name<'q> {
            #(#fields),*
        }

        #[automatically_derived]
        impl<'q> #crate_name::FetchItem<'q> for #name {
            type Item = #item_name<'q>;
        }
    }
}

fn derive_prepared_struct<'a>(
    crate_name: &Ident,
    vis: &Visibility,
    name: &Ident,
    item_name: &Ident,
    prepared_name: &Ident,
    fields: impl Iterator<Item = &'a Field>,
) -> TokenStream {
    let (types, names): (Vec<_>, Vec<_>) = fields
        .map(|field| {
            let name = field
                .ident
                .as_ref()
                .expect("Only named fields are supported");
            let ty = &field.ty;

            (
                quote! {
                    #name: <#ty as #crate_name::Fetch<'w>>::Prepared
                },
                quote! { #name },
            )
        })
        .multiunzip();

    let msg = format!("The prepared fetch for {name}");

    quote! {
        #[doc = #msg]
        #vis struct #prepared_name<'w> {
            #(#types),*
        }

        #[automatically_derived]
        impl<'w, 'q> #crate_name::fetch::PreparedFetch<'q> for #prepared_name<'w> {
            type Item = #item_name<'q>;

            #[inline]
            fn fetch(&'q mut self, slot: #crate_name::archetype::Slot) -> Self::Item {
                Self::Item {
                    #(#names: self.#names.fetch(slot),)*
                }
            }

            #[inline]
            fn set_visited(&mut self, slots: #crate_name::archetype::Slice, change_tick: u32) {
                #(self.#names.set_visited(slots, change_tick);)*
            }

            #[inline]
            fn filter_slots(&mut self, slots: #crate_name::archetype::Slice) -> #crate_name::archetype::Slice {
                #crate_name::fetch::PreparedFetch::filter_slots(&mut (#(&mut self.#names,)*), slots)
            }
        }
    }
}

fn derive_data_struct(
    crate_name: Ident,
    input: &DeriveInput,
    data: &DataStruct,
) -> Result<TokenStream> {
    let name = &input.ident;
    let item_name = Ident::new(&format!("{}Item", name), Span::call_site());
    let prepared_name = Ident::new(&format!("Prepared{}", name), Span::call_site());
    let attrs = Attrs::get(&input.attrs)?;

    match data.fields {
        syn::Fields::Named(ref fields) => {
            let fields = &fields.named;

            let names = fields
                .iter()
                .map(|v| v.ident.as_ref().unwrap())
                .collect_vec();

            let types = fields.iter().map(|v| &v.ty).collect_vec();

            let item_derive = derive_item_struct(
                &crate_name,
                &attrs,
                &input.vis,
                name,
                &item_name,
                fields.iter(),
            );

            let prepared_derive = derive_prepared_struct(
                &crate_name,
                &input.vis,
                name,
                &item_name,
                &prepared_name,
                fields.iter(),
            );

            Ok(quote! {

                #item_derive

                #prepared_derive

                impl<'w> #crate_name::Fetch<'w> for #name
                where #(#types: Fetch<'w>),*
                {
                    const MUTABLE: bool = #(<#types as Fetch<'w>>::MUTABLE)|*;

                    type Prepared = #prepared_name<'w>;
                    #[inline]
                    fn prepare(
                        &'w self,
                        data: #crate_name::fetch::FetchPrepareData<'w>,
                    ) -> Option<Self::Prepared> {
                        Some(Self::Prepared {
                            #(#names: self.#names.prepare(data)?),*
                        })
                    }

                    #[inline]
                    fn filter_arch(&self, arch: &#crate_name::archetype::Archetype) -> bool {
                        ( #(self.#names.filter_arch(arch))&&* )
                    }

                    fn describe(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                        let mut s = f.debug_struct(stringify!(#name));

                        #(
                            s.field(stringify!(#names), &#crate_name::fetch::FmtQuery(&self.#names));
                        )*

                        s.finish()
                    }

                    fn access(&self, data: #crate_name::fetch::FetchPrepareData) -> Vec<#crate_name::Access> {
                        [ #(self.#names.access(data)),* ].concat()
                    }

                    fn searcher(&self, searcher: &mut #crate_name::ArchetypeSearcher) {
                        #(self.#names.searcher(searcher));*
                    }
                }
            })
        }
        syn::Fields::Unnamed(_) => Err(Error::new(
            Span::call_site(),
            "Deriving fetch for a tuple struct is not supported",
        )),
        syn::Fields::Unit => Err(Error::new(
            Span::call_site(),
            "Deriving fetch for a unit struct is not supported",
        )),
    }
}

struct Attrs {
    extras: Option<MetaList>,
}

impl Attrs {
    fn get(input: &[Attribute]) -> Result<Self> {
        let mut res = Self { extras: None };

        for attr in input {
            if attr.path.is_ident("fetch") {
                attr.parse_meta()?;
                let list = match attr.parse_meta().unwrap() {
                    syn::Meta::List(list) => list,
                    _ => {
                        return Err(Error::new(
                            Span::call_site(),
                            "Expected a MetaList for `fetch`",
                        ))
                    }
                };

                res.extras = Some(list);
            }
        }

        Ok(res)
    }
}
