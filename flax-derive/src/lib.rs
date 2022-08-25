use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use proc_macro_crate::FoundCrate;
use quote::quote;
use syn::*;

/// ```rust
/// use glam::*;
/// struct CustomFetch<'a> {
///     position: &'a Vec2,
///     rotation: &'a Quat,
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
    let (types, expr): (Vec<_>, Vec<_>) = fields
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
                quote! {
                    #name: self.#name.fetch(slot)
                },
            )
        })
        .unzip();

    let msg = format!("The prepared fetch for {name}");

    quote! {
        #[doc = #msg]
        #vis struct #prepared_name<'w> {
            #(#types),*
        }

        #[automatically_derived]
        impl<'w, 'q> #crate_name::PreparedFetch<'q> for #prepared_name<'w> {
            type Item = #item_name<'q>;

            unsafe fn fetch(&'q mut self, slot: #crate_name::archetype::Slot) -> Self::Item {
                Self::Item {
                    #(#expr),*
                }
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

            let field_names = fields
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
                    fn prepare(
                        &'w self,
                        data: #crate_name::fetch::FetchPrepareData<'w>,
                    ) -> Option<Self::Prepared> {
                        Some(Self::Prepared {
                            #(#field_names: self.#field_names.prepare(data)?),*
                        })
                    }


                    fn matches(&self, data: #crate_name::fetch::FetchPrepareData) -> bool {
                        ( #(self.#field_names.matches(data))&&* )
                    }

                    fn describe(&self, f: &mut dyn ::std::fmt::Write) -> ::std::fmt::Result {
                        use ::std::fmt::Write;
                        f.write_str(stringify!(#name))?;
                        f.write_str("{")?;

                        #(
                            f.write_str(stringify!(#field_names))?;
                            f.write_str(": ")?;
                            f.write_str(stringify!(self.#field_names.describe()))?;
                        )*

                        f.write_str("}")
                    }

                    fn access(&self, data: #crate_name::fetch::FetchPrepareData) -> Vec<Access> {
                        [ #(self.#field_names.access(data)),* ].into_iter().flatten().collect()
                    }

                    fn difference(&self, data: #crate_name::fetch::FetchPrepareData) -> Vec<String> {
                        [ #(self.#field_names.difference(data)),* ].concat()
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

#[derive(Debug)]
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
