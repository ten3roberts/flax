use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use proc_macro_crate::FoundCrate;
use quote::{format_ident, quote};
use syn::{
    token::{Gt, Lt},
    Attribute, DataStruct, DeriveInput, Error, GenericParam, Generics, Ident, ImplGenerics,
    Lifetime, LifetimeParam, MetaList, Result, Type, TypeGenerics, TypeParam, Visibility,
};

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
    let attrs = Attrs::get(&input.attrs)?;

    match data.fields {
        syn::Fields::Named(ref fields) => {
            let fields = &fields.named;

            let params = Params::new(&crate_name, &input.vis, input, &attrs);

            let prepared_derive = derive_prepared_struct(&params);

            let union_derive = derive_union(&params);

            let transform_modified = derive_modified(&params);

            let fetch_derive = derive_fetch_struct(&params);

            Ok(quote! {
                #fetch_derive

                #prepared_derive

                #union_derive

                // #transform_modified
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

fn derive_fetch_struct(params: &Params) -> TokenStream {
    let Params {
        crate_name,
        vis,
        fetch_name,
        item_name,
        prepared_name,
        generics,
        q_generics,
        field_names,
        field_types,
        w_lf,
        q_lf,
        attrs,
        ..
    } = params;

    let item_ty = params.item_ty();
    let item_impl = params.item_impl();
    let item_msg = format!("The item returned by {fetch_name}");

    let prep_ty = params.prepared_ty();

    let extras = match &attrs.extras {
        Some(extras) => {
            let nested = &extras.tokens;
            quote! { #[derive(#nested)]}
        }
        None => quote! {},
    };

    let fetch_impl = params.fetch_impl();
    let fetch_ty = params.fetch_ty();

    let msg = format!("The item returned by {fetch_name}");

    let extras = match &attrs.extras {
        Some(extras) => {
            let nested = &extras.tokens;
            quote! { #[derive(#nested)]}
        }
        None => quote! {},
    };

    quote! {
        #[doc = #item_msg]
        #extras
        #vis struct #item_name #q_generics {
            #(#field_names: <#field_types as #crate_name::fetch::FetchItem<#q_lf>>::Item,)*
        }

        // #[automatically_derived]
        impl #item_impl #crate_name::fetch::FetchItem<#q_lf> for #fetch_name #fetch_ty {
            type Item = #item_name #item_ty;
        }

        #[automatically_derived]
        impl #fetch_impl #crate_name::Fetch<#w_lf> for #fetch_name #fetch_ty
            where #(#field_types: #crate_name::Fetch<#w_lf>,)*
        {
            const MUTABLE: bool = #(<#field_types as #crate_name::Fetch <#w_lf>>::MUTABLE)||*;

            type Prepared = #prepared_name #prep_ty;

            #[inline]
            fn prepare( &'w self, data: #crate_name::fetch::FetchPrepareData<'w>
            ) -> Option<Self::Prepared> {
                Some(Self::Prepared {
                    #(#field_names: #crate_name::Fetch::prepare(&self.#field_names, data)?,)*
                })
            }

            #[inline]
            fn filter_arch(&self, arch: &#crate_name::archetype::Archetype) -> bool {
                #(self.#field_names.filter_arch(arch))&&*
            }

            fn describe(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut s = f.debug_struct(stringify!(#fetch_name));

                #(
                    s.field(stringify!(#field_names), &#crate_name::fetch::FmtQuery(&self.#field_names));
                )*

                s.finish()
            }

            fn access(&self, data: #crate_name::fetch::FetchAccessData, dst: &mut Vec<#crate_name::system::Access>) {
                 #(self.#field_names.access(data, dst));*
            }

            fn searcher(&self, searcher: &mut #crate_name::query::ArchetypeSearcher) {
                #(self.#field_names.searcher(searcher);)*
            }
        }
    }
}

fn prepend_generics(prepend: &[GenericParam], generics: &Generics) -> Generics {
    let mut generics = generics.clone();
    generics.params = prepend
        .into_iter()
        .cloned()
        .chain(generics.params)
        .collect();

    generics
}

/// Implements the filtering of the struct fields using a set union
fn derive_union(params: &Params) -> TokenStream {
    let ty_generics = params.fetch_ty();

    let Params {
        crate_name,
        generics,
        w_generics,
        q_generics,
        fetch_name,
        field_types,
        field_names,
        ..
    } = params;

    let (_, fetch_ty, _) = generics.split_for_impl();
    let (impl_generics, _, _) = q_generics.split_for_impl();

    quote! {
        // #[automatically_derived]
        // impl #impl_generics #crate_name::fetch::UnionFilter<'q> for #fetch_name #fetch_ty where #(#field_types: for<'x> #crate_name::fetch::PreparedFetch<'x>,)* {
        //     unsafe fn filter_union(&mut self, slots: #crate_name::archetype::Slice) -> #crate_name::archetype::Slice {
        //         #crate_name::fetch::PreparedFetch::filter_slots(&mut #crate_name::filter::Union((#(&mut self.#field_names,)*)), slots)
        //     }
        // }
    }
}

/// Implements the filtering of the struct fields using a set union
fn derive_modified(params: &Params) -> TokenStream {
    let Params {
        crate_name,
        vis,
        fetch_name,
        item_name,
        prepared_name,
        generics,
        field_names,
        field_types,
        w_lf,
        q_lf,
        attrs,
        ..
    } = params;

    // Replace all the fields with generics to allow transforming into different types
    let ty_generics = ('a'..='z')
        .map(|c| format_ident!("{}", c))
        .map(|v| GenericParam::Type(TypeParam::from(v)))
        .take(params.field_types.len())
        .collect_vec();

    let transformed_name = format_ident!("{}Transformed", params.fetch_name);
    let transformed_struct = quote! {
        #vis struct #transformed_name<#(#ty_generics),*>{
            #(#field_names: #ty_generics,)*
        }
    };

    let input =
        syn::parse2::<DeriveInput>(transformed_struct).expect("Generated struct is always valid");

    let transformed_params = Params::new(crate_name, vis, &input, attrs);

    let prepared = derive_prepared_struct(&transformed_params);

    // Replace all the fields with generics to allow transforming into different types
    let generics_params = ('a'..='z')
        .map(|c| format_ident!("{}", c))
        .map(|v| GenericParam::Type(TypeParam::from(v)))
        .take(params.field_types.len())
        .collect();

    let generics = Generics {
        lt_token: Some(Lt(Span::call_site())),
        params: generics_params,
        gt_token: Some(Gt(Span::call_site())),
        where_clause: None,
    };
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fetch = derive_fetch_struct(&transformed_params);

    quote! {
        // #vis struct #transformed #ty_generics {
        //     #(#field_names: #generics,)*
        // }

        // #fetch

        // #prepared

        // #[automatically_derived]
        // impl #crate_name::fetch::ModifiedFetch for #name where #(#field_types: #crate_name::fetch::ModifiedFetch + for<'q> #crate_name::fetch::PreparedFetch<'q>,)* {
        //     type Modified = #crate_name::filter::Union<#transformed<#(<#field_types as #crate_name::fetch::ModifiedFetch>::Modified,)*>>;
        // }
    }
}

/// Derive the returned Item type for a Fetch
fn derive_item_struct(params: &Params) -> TokenStream {
    let Params {
        crate_name,
        vis,
        fetch_name,
        item_name,
        prepared_name,
        generics,
        field_names,
        field_types,
        w_lf,
        q_lf,
        attrs,
        ..
    } = params;

    let msg = format!("The item returned by {fetch_name}");

    let extras = match &attrs.extras {
        Some(extras) => {
            let nested = &extras.tokens;
            quote! { #[derive(#nested)]}
        }
        None => quote! {},
    };

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let item_ty = params.item_ty();

    quote! {
        #[doc = #msg]
        #extras
        #vis struct #item_name q_generics {
            #(#field_names: <#field_types as #crate_name::fetch::FetchItem<#q_lf>>::Item,)*
        }
    }
}

fn derive_prepared_struct(params: &Params) -> TokenStream {
    let Params {
        crate_name,
        vis,
        fetch_name,
        item_name,
        prepared_name,
        generics,
        field_names,
        field_types,
        w_generics,
        w_lf,
        q_lf,
        ..
    } = params;

    let msg = format!("The prepared fetch for {fetch_name}");

    let prep_impl = params.prepared_impl();
    let fetch_ty = params.fetch_ty();
    let prep_ty = params.prepared_ty();
    let item_ty = params.item_ty();

    quote! {
        #[doc = #msg]
        #vis struct #prepared_name #w_generics {
            #(#field_names: <#field_types as #crate_name::Fetch <#w_lf>>::Prepared,)*
        }

        #[automatically_derived]
        impl #prep_impl #crate_name::fetch::PreparedFetch<#q_lf> for #prepared_name #prep_ty {
            type Item = #item_name #item_ty;

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
                #(#crate_name::fetch::PreparedFetch::set_visited(&mut self.#field_names, slots);)*
            }
        }
    }
}

#[derive(Default)]
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

#[derive(Clone)]
struct Params<'a> {
    crate_name: &'a Ident,
    vis: &'a Visibility,

    fetch_name: Ident,
    item_name: Ident,
    prepared_name: Ident,

    generics: &'a Generics,
    w_generics: Generics,
    q_generics: Generics,
    wq_generics: Generics,

    field_names: Vec<&'a Ident>,
    field_types: Vec<&'a Type>,

    w_lf: LifetimeParam,
    q_lf: LifetimeParam,
    attrs: &'a Attrs,
}

impl<'a> Params<'a> {
    fn new(
        crate_name: &'a Ident,
        vis: &'a Visibility,
        input: &'a DeriveInput,
        attrs: &'a Attrs,
    ) -> Self {
        let fields = match &input.data {
            syn::Data::Struct(data) => match &data.fields {
                syn::Fields::Named(fields) => fields,
                _ => unreachable!(),
            },

            _ => unreachable!(),
        };

        let field_names = fields
            .named
            .iter()
            .map(|v| v.ident.as_ref().unwrap())
            .collect_vec();

        let field_types = fields.named.iter().map(|v| &v.ty).collect_vec();

        let fetch_name = input.ident.clone();

        let w_lf = LifetimeParam::new(Lifetime::new("'w", Span::call_site()));
        let q_lf = LifetimeParam::new(Lifetime::new("'q", Span::call_site()));

        Self {
            crate_name,
            vis,
            generics: &input.generics,
            field_names,
            field_types,
            attrs,
            item_name: format_ident!("{fetch_name}Item"),
            prepared_name: format_ident!("Prepared{fetch_name}"),
            fetch_name,
            w_generics: prepend_generics(&[GenericParam::Lifetime(w_lf.clone())], &input.generics),
            q_generics: prepend_generics(&[GenericParam::Lifetime(q_lf.clone())], &input.generics),

            wq_generics: prepend_generics(
                &[
                    GenericParam::Lifetime(w_lf.clone()),
                    GenericParam::Lifetime(q_lf.clone()),
                ],
                &input.generics,
            ),

            w_lf,
            q_lf,
        }
    }

    fn item_impl(&self) -> ImplGenerics {
        self.q_generics.split_for_impl().0
    }

    fn prepared_impl(&self) -> ImplGenerics {
        self.wq_generics.split_for_impl().0
    }

    fn fetch_impl(&self) -> ImplGenerics {
        self.wq_generics.split_for_impl().0
    }

    fn fetch_ty(&self) -> TypeGenerics {
        self.generics.split_for_impl().1
    }

    fn item_ty(&self) -> TypeGenerics {
        self.q_generics.split_for_impl().1
    }

    fn prepared_ty(&self) -> TypeGenerics {
        self.w_generics.split_for_impl().1
    }
}
