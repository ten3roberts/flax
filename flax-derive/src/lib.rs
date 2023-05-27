use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use proc_macro_crate::FoundCrate;
use quote::quote;
use syn::{Attribute, DataStruct, DeriveInput, Error, Ident, MetaList, Result, Type, Visibility};

/// ```rust,ignore
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
    let crate_name = match proc_macro_crate::crate_name("flax").expect("Failed to get crate name") {
        FoundCrate::Itself => Ident::new("crate", Span::call_site()),
        FoundCrate::Name(name) => Ident::new(&name, Span::call_site()),
    };
    do_derive_fetch(crate_name, input.into()).into()
}

fn do_derive_fetch(crate_name: Ident, input: TokenStream) -> TokenStream {
    let input = match syn::parse2::<DeriveInput>(input) {
        Ok(input) => input,
        Err(err) => return err.to_compile_error(),
    };

    match input.data {
        syn::Data::Struct(ref data) => derive_data_struct(crate_name, &input, data)
            .unwrap_or_else(|err| err.to_compile_error()),
        syn::Data::Enum(_) => todo!(),
        syn::Data::Union(_) => todo!(),
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

            let field_names = fields
                .iter()
                .map(|v| v.ident.as_ref().unwrap())
                .collect_vec();

            let field_types = fields.iter().map(|v| &v.ty).collect_vec();

            let item_derive = derive_item_struct(
                &crate_name,
                &attrs,
                &input.vis,
                name,
                &item_name,
                &field_names,
                &field_types,
            );

            let prepared_derive = derive_prepared_struct(
                &crate_name,
                &input.vis,
                name,
                &item_name,
                &prepared_name,
                &field_names,
                &field_types,
            );

            Ok(quote! {

                #item_derive

                #prepared_derive

                #[automatically_derived]
                impl<'w> #crate_name::Fetch<'w> for #name
                where #(#field_types: #crate_name::Fetch<'w>,)*
                {
                    const MUTABLE: bool = #(<#field_types as #crate_name::Fetch<'w>>::MUTABLE)||*;

                    type Prepared = #prepared_name<'w>;
                    #[inline]
                    fn prepare( &'w self, data: #crate_name::fetch::FetchPrepareData<'w>
                    ) -> Option<Self::Prepared> {
                        Some(Self::Prepared {
                            #(#field_names: self.#field_names.prepare(data)?,)*
                        })
                    }

                    #[inline]
                    fn filter_arch(&self, arch: &#crate_name::archetype::Archetype) -> bool {
                        #(self.#field_names.filter_arch(arch))&&*
                    }

                    fn describe(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        let mut s = f.debug_struct(stringify!(#name));

                        #(
                            s.field(stringify!(#field_names), &#crate_name::fetch::FmtQuery(&self.#field_names));
                        )*

                        s.finish()
                    }

                    fn access(&self, data: #crate_name::fetch::FetchAccessData) -> Vec<#crate_name::system::Access> {
                        [ #(self.#field_names.access(data)),* ].concat()
                    }

                    fn searcher(&self, searcher: &mut #crate_name::query::ArchetypeSearcher) {
                        #(self.#field_names.searcher(searcher);)*
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

/// Derive the yielded Item type for a Fetch
fn derive_item_struct<'a>(
    crate_name: &Ident,
    attrs: &Attrs,
    vis: &Visibility,
    name: &Ident,
    item_name: &Ident,
    field_names: &[&'a Ident],
    field_types: &[&'a Type],
) -> TokenStream {
    let msg = format!("The item yielded by {name}");

    let extras = match &attrs.extras {
        Some(extras) => {
            let nested = &extras.tokens;
            quote! { #[derive(#nested)]}
        }
        None => quote! {},
    };

    quote! {
        #[doc = #msg]
        #extras
        #vis struct #item_name<'q> {
            #(#field_names: <#field_types as #crate_name::fetch::FetchItem<'q>>::Item,)*
        }

        #[automatically_derived]
        impl<'q> #crate_name::fetch::FetchItem<'q> for #name {
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
    field_names: &[&'a Ident],
    field_types: &[&'a Type],
) -> TokenStream {
    let msg = format!("The prepared fetch for {name}");

    quote! {
        #[doc = #msg]
        #vis struct #prepared_name<'w> {
            #(#field_names: <#field_types as #crate_name::Fetch<'w>>::Prepared,)*
        }

        #[automatically_derived]
        impl<'w, 'q> #crate_name::fetch::PreparedFetch<'q> for #prepared_name<'w> {
            type Item = #item_name<'q>;

            #[inline]
            unsafe fn fetch(&'q mut self, slot: #crate_name::archetype::Slot) -> Self::Item {
                Self::Item {
                    #(#field_names: self.#field_names.fetch(slot),)*
                }
            }

            #[inline]
            unsafe fn filter_slots(&mut self, slots: #crate_name::archetype::Slice) -> #crate_name::archetype::Slice {
                #crate_name::fetch::PreparedFetch::filter_slots(&mut (#(&mut self.#field_names,)*), slots)
            }

            #[inline]
            fn set_visited(&mut self, slots: #crate_name::archetype::Slice) {
                #(self.#field_names.set_visited(slots);)*
            }
        }
    }
}

struct Attrs {
    extras: Option<MetaList>,
}

impl Attrs {
    fn get(input: &[Attribute]) -> Result<Self> {
        let mut res = Self { extras: None };

        for attr in input {
            if attr.path().is_ident("fetch") {
                match &attr.meta {
                    syn::Meta::List(list) => res.extras = Some(list.clone()),
                    _ => {
                        return Err(Error::new(
                            Span::call_site(),
                            "Expected a MetaList for `fetch`",
                        ))
                    }
                };
            }
        }

        Ok(res)
    }
}

#[cfg(test)]
mod test;
