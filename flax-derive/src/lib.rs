use std::collections::BTreeSet;

use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use proc_macro_crate::FoundCrate;
use quote::{format_ident, quote};
use syn::{
    bracketed, parse::Parse, punctuated::Punctuated, spanned::Spanned, Attribute, DataStruct,
    DeriveInput, Error, Field, GenericParam, Generics, Ident, ImplGenerics, Index, Lifetime,
    LifetimeParam, Result, Token, Type, TypeGenerics, TypeParam, Visibility,
};

/// ```rust,ignore
/// #[derive(Fetch)]
/// #[fetch(item_derives = [Debug], transforms = [Modified])]
/// struct CustomFetch {
///     #[fetch(ignore)]
///     rotation: Mutable<glam::Quat>,
///     position: Component<glam::Vec3>,
///     id: EntityIds,
/// }
/// ```
/// # Struct Attributes
///
/// - `item_derives`: Derive additional traits for the item returned by the fetch.
/// - `transforms`: Implement `Transform` for the specified transform kinds.
///
/// # Field Attributes
/// - `ignore`: ignore slot-filtering and transformations for a field.
///     Useful for including a `Mutable` in a change query.
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
    let attrs = Attrs::get(&input.attrs)?;

    match data.fields {
        syn::Fields::Named(_) => {
            let params = Params::new(&crate_name, &input.vis, input, &attrs)?;

            let prepared_derive = derive_prepared_struct(&params);

            let fetch_derive = derive_fetch_struct(&params);

            let union_derive = derive_union(&params);

            let transforms_derive = derive_transform(&params)?;

            Ok(quote! {
                #fetch_derive

                #prepared_derive

                #union_derive

                #transforms_derive
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
        q_generics,
        fields,
        field_names,
        field_types,
        attrs,
        ..
    } = params;

    let item_ty = params.q_ty();
    let item_impl = params.q_impl();
    let item_msg = format!("The item returned by {fetch_name}");

    let prep_ty = params.w_ty();

    let extras = match &attrs.item_derives {
        Some(extras) => {
            quote! { #[derive(#extras)]}
        }
        None => quote! {},
    };

    let fetch_impl = params.w_impl();
    let fetch_ty = params.base_ty();

    let item_fields = fields
        .iter()
        .map(|v| {
            let vis = v.vis;
            let ident = v.ident;
            let ty = v.ty;
            quote! {
                #vis #ident: <#ty as #crate_name::fetch::FetchItem<'q>>::Item,
            }
        })
        .collect::<TokenStream>();

    quote! {
        #[doc = #item_msg]
        #extras
        #vis struct #item_name #q_generics {
            #item_fields
        }

        // #vis struct #batch_name #wq_generics {
        //     #(#field_names: <<#field_types as #crate_name::fetch::Fetch<'w>::Prepared> as #crate_name::fetch::PreparedFetch<#q_lf>>::Chunk,)*
        // }

        // #[automatically_derived]
        impl #item_impl #crate_name::fetch::FetchItem<'q> for #fetch_name #fetch_ty {
            type Item = #item_name #item_ty;
        }

        #[automatically_derived]
        impl #fetch_impl #crate_name::Fetch<'w> for #fetch_name #fetch_ty
            where #(#field_types: 'static,)*
        {
            const MUTABLE: bool = #(<#field_types as #crate_name::Fetch <'w>>::MUTABLE)||*;

            type Prepared = #prepared_name #prep_ty;

            #[inline]
            fn prepare( &'w self, data: #crate_name::fetch::FetchPrepareData<'w>
            ) -> Option<Self::Prepared> {
                Some(Self::Prepared {
                    #(#field_names: #crate_name::Fetch::prepare(&self.#field_names, data)?,)*
                })
            }

            #[inline]
            fn filter_arch(&self, data: #crate_name::fetch::FetchAccessData) -> bool {
                #(#crate_name::Fetch::filter_arch(&self.#field_names, data))&&*
            }

            fn describe(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                let mut s = f.debug_struct(stringify!(#fetch_name));

                #(
                    s.field(stringify!(#field_names), &#crate_name::fetch::FmtQuery(&self.#field_names));
                )*

                s.finish()
            }

            fn access(&self, data: #crate_name::fetch::FetchAccessData, dst: &mut Vec<#crate_name::system::Access>) {
                 #(#crate_name::Fetch::access(&self.#field_names, data, dst));*
            }

            fn searcher(&self, searcher: &mut #crate_name::query::ArchetypeSearcher) {
                #(#crate_name::Fetch::searcher(&self.#field_names, searcher);)*
            }
        }
    }
}

fn prepend_generics(prepend: &[GenericParam], generics: &Generics) -> Generics {
    let mut generics = generics.clone();
    generics.params = prepend.iter().cloned().chain(generics.params).collect();

    generics
}

/// Implements the filtering of the struct fields using a set union
fn derive_union(params: &Params) -> TokenStream {
    let Params {
        crate_name,
        fields,
        prepared_name,
        ..
    } = params;

    let impl_generics = params.wq_impl();

    let prep_ty = params.w_ty();

    // Make sure not to *or* ignored fields
    let filter_fields = fields.iter().filter(|v| !v.attrs.ignore).map(|v| v.ident);
    let filter_types = fields.iter().filter(|v| !v.attrs.ignore).map(|v| v.ty);

    quote! {
        #[automatically_derived]
        impl #impl_generics #crate_name::fetch::UnionFilter for #prepared_name #prep_ty where #prepared_name #prep_ty: #crate_name::fetch::PreparedFetch<'q> {
            const HAS_UNION_FILTER: bool = #(<<#filter_types as #crate_name::fetch::Fetch<'w>>::Prepared as #crate_name::fetch::PreparedFetch<'q>>::HAS_FILTER)&&*;

            unsafe fn filter_union(&mut self, slots: #crate_name::archetype::Slice) -> #crate_name::archetype::Slice {
                #crate_name::fetch::PreparedFetch::filter_slots(&mut #crate_name::filter::Union((#(&mut self.#filter_fields,)*)), slots)
            }
        }
    }
}

/// Implements the filtering of the struct fields using a set union
fn derive_transform(params: &Params) -> Result<TokenStream> {
    let Params {
        crate_name,
        vis,
        fields,
        fetch_name,
        attrs,
        ..
    } = params;

    // Replace all the fields with generics to allow transforming into different types
    let ty_generics = ('A'..='Z')
        .zip(fields)
        .filter(|(_, v)| !v.attrs.ignore)
        .map(|(c, _)| format_ident!("{}", c))
        .map(|v| GenericParam::Type(TypeParam::from(v)))
        .collect_vec();

    let transformed_name = format_ident!("{fetch_name}Transformed");
    use quote::ToTokens;

    let transformed_struct = {
        let fields = ('A'..='Z').zip(fields).map(|(c, field)| {
            let ty = if field.attrs.ignore {
                field.ty.to_token_stream()
            } else {
                format_ident!("{}", c).to_token_stream()
            };

            let vis = field.vis;
            let ident = field.ident;
            quote! {
               #vis #ident: #ty,
            }
        });

        // eprintln!("types: {:?}", types);

        quote! {
            #vis struct #transformed_name<#(#ty_generics: for<'x> #crate_name::fetch::Fetch<'x>),*>{
                #(#fields)*
            }
        }
    };

    let input =
        syn::parse2::<DeriveInput>(transformed_struct).expect("Generated struct is always valid");

    let transformed_attrs = Attrs::default();

    let mut transformed_params = Params::new(crate_name, vis, &input, &transformed_attrs)?;
    for (dst, src) in transformed_params.fields.iter_mut().zip(fields) {
        dst.attrs = src.attrs.clone();
    }

    let fetch = derive_fetch_struct(&transformed_params);

    let prepared = derive_prepared_struct(&transformed_params);
    let union = derive_union(&transformed_params);

    let transforms = attrs
        .transforms
        .iter()
        .map(|method| {
            let method = method.to_tokens(crate_name);

            let trait_name = quote! { #crate_name::fetch::TransformFetch<#method> };

            let types = fields
                .iter()
                .filter_map(|field| {
                    if field.attrs.ignore {
                        None
                    } else {
                        let ty = field.ty;
                        Some(quote! {
                            <#ty as #trait_name>::Output
                        })
                    }
                })
                .collect_vec();

            let initializers = fields
                .iter()
                .map(|field| {
                    let ident = field.ident;
                    let ty = field.ty;
                    if field.attrs.ignore {
                        quote! {
                            #ident: self.#ident
                        }
                    } else {
                        quote! {
                            #ident: <#ty as #trait_name>::transform_fetch(self.#ident, method)
                        }
                    }
                })
                .collect_vec();

            quote! {
                #[automatically_derived]
                impl #trait_name for #fetch_name
                {
                    type Output = #crate_name::filter::Union<#transformed_name<#(#types,)*>>;
                    fn transform_fetch(self, method: #method) -> Self::Output {
                        #crate_name::filter::Union(#transformed_name {
                            #(#initializers,)*
                        })
                    }
                }
            }
        })
        .collect_vec();

    Ok(quote! {
        #input

        #fetch

        #prepared

        #union

        #(#transforms)*
    })
}

fn derive_prepared_struct(params: &Params) -> TokenStream {
    let Params {
        crate_name,
        vis,
        fetch_name,
        item_name,
        prepared_name,
        fields,
        field_names,
        field_types,
        w_generics,
        ..
    } = params;

    let msg = format!("The prepared fetch for {fetch_name}");

    let prep_impl = params.wq_impl();
    let prep_ty = params.w_ty();
    let item_ty = params.q_ty();

    let field_idx = (0..field_names.len()).map(Index::from);
    let filter_fields = fields.iter().filter(|v| !v.attrs.ignore).map(|v| v.ident);

    quote! {
        #[doc = #msg]
        #vis struct #prepared_name #w_generics {
            #(#field_names: <#field_types as #crate_name::Fetch <'w>>::Prepared,)*
        }

        #[automatically_derived]
        impl #prep_impl #crate_name::fetch::PreparedFetch<'q> for #prepared_name #prep_ty
            where #(#field_types: 'static,)*
        {
            type Item = #item_name #item_ty;
            type Chunk = (#(<<#field_types as #crate_name::fetch::Fetch<'w>>::Prepared as #crate_name::fetch::PreparedFetch<'q>>::Chunk,)*);

            const HAS_FILTER: bool = #(<<#field_types as #crate_name::fetch::Fetch<'w>>::Prepared as #crate_name::fetch::PreparedFetch<'q>>::HAS_FILTER)||*;

            #[inline]
            unsafe fn fetch_next(chunk: &mut Self::Chunk) -> Self::Item {
                Self::Item {
                    #(#field_names: <<#field_types as #crate_name::fetch::Fetch<'w>>::Prepared as #crate_name::fetch::PreparedFetch<'q>>::fetch_next(&mut chunk.#field_idx),)*
                }
            }

            #[inline]
            unsafe fn filter_slots(&mut self, slots: #crate_name::archetype::Slice) -> #crate_name::archetype::Slice {
                #crate_name::fetch::PreparedFetch::filter_slots(&mut (#(&mut self.#filter_fields,)*), slots)
            }

            #[inline]
            unsafe fn create_chunk(&'q mut self, slots: #crate_name::archetype::Slice) -> Self::Chunk {
                (
                    #(#crate_name::fetch::PreparedFetch::create_chunk(&mut self.#field_names, slots),)*
                )
            }
        }
    }
}

#[derive(Clone)]
struct ParsedField<'a> {
    vis: &'a Visibility,
    ty: &'a Type,
    ident: &'a Ident,
    attrs: FieldAttrs,
}

impl<'a> ParsedField<'a> {
    fn get(field: &'a Field) -> Result<Self> {
        let attrs = FieldAttrs::get(&field.attrs)?;

        let ident = field
            .ident
            .as_ref()
            .ok_or(Error::new(field.span(), "Only named fields are supported"))?;

        Ok(Self {
            vis: &field.vis,
            ty: &field.ty,
            ident,
            attrs,
        })
    }
}

#[derive(Default, Debug, Clone)]
struct FieldAttrs {
    ignore: bool,
}

impl FieldAttrs {
    fn get(input: &[Attribute]) -> Result<Self> {
        let mut res = Self::default();

        for attr in input {
            if !attr.path().is_ident("fetch") {
                continue;
            }

            match &attr.meta {
                syn::Meta::List(list) => {
                    // Parse list

                    list.parse_nested_meta(|meta| {
                        // item = [Debug, PartialEq]
                        if meta.path.is_ident("ignore") {
                            res.ignore = true;
                            Ok(())
                        } else {
                            Err(Error::new(
                                meta.path.span(),
                                "Unknown fetch field attribute",
                            ))
                        }
                    })?;
                }
                _ => {
                    return Err(Error::new(
                        Span::call_site(),
                        "Expected a MetaList for `fetch`",
                    ))
                }
            };
        }

        Ok(res)
    }
}

#[derive(Default)]
struct Attrs {
    item_derives: Option<Punctuated<Ident, Token![,]>>,
    transforms: BTreeSet<TransformIdent>,
}

impl Attrs {
    fn get(input: &[Attribute]) -> Result<Self> {
        let mut res = Self::default();

        for attr in input {
            if !attr.path().is_ident("fetch") {
                continue;
            }

            match &attr.meta {
                syn::Meta::List(list) => {
                    // Parse list

                    list.parse_nested_meta(|meta| {
                        // item = [Debug, PartialEq]
                        if meta.path.is_ident("item_derives") {
                            let value = meta.value()?;
                            let content;
                            bracketed!(content in value);
                            let content =
                                <Punctuated<Ident, Token![,]>>::parse_terminated(&content)?;

                            res.item_derives = Some(content);
                            Ok(())
                        } else if meta.path.is_ident("transforms") {
                            let value = meta.value()?;
                            let content;
                            bracketed!(content in value);
                            let content =
                                <Punctuated<TransformIdent, Token![,]>>::parse_terminated(
                                    &content,
                                )?;

                            res.transforms.extend(content);
                            Ok(())
                        } else {
                            Err(Error::new(meta.path.span(), "Unknown fetch attribute"))
                        }
                    })?;
                }
                _ => {
                    return Err(Error::new(
                        Span::call_site(),
                        "Expected a MetaList for `fetch`",
                    ))
                }
            };
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

    fields: Vec<ParsedField<'a>>,
    field_names: Vec<&'a Ident>,
    field_types: Vec<&'a Type>,

    attrs: &'a Attrs,
}

impl<'a> Params<'a> {
    fn new(
        crate_name: &'a Ident,
        vis: &'a Visibility,
        input: &'a DeriveInput,
        attrs: &'a Attrs,
    ) -> Result<Self> {
        let fields = match &input.data {
            syn::Data::Struct(data) => match &data.fields {
                syn::Fields::Named(fields) => fields,
                _ => unreachable!(),
            },

            _ => unreachable!(),
        };

        let fetch_name = input.ident.clone();

        let w_lf = LifetimeParam::new(Lifetime::new("'w", Span::call_site()));
        let q_lf = LifetimeParam::new(Lifetime::new("'q", Span::call_site()));

        let fields = fields
            .named
            .iter()
            .map(ParsedField::get)
            .collect::<Result<Vec<_>>>()?;

        let field_names = fields.iter().map(|v| v.ident).collect_vec();
        let field_types = fields.iter().map(|v| v.ty).collect_vec();

        Ok(Self {
            crate_name,
            vis,
            generics: &input.generics,
            fields,
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
        })
    }

    fn q_impl(&self) -> ImplGenerics {
        self.q_generics.split_for_impl().0
    }

    fn wq_impl(&self) -> ImplGenerics {
        self.wq_generics.split_for_impl().0
    }

    fn w_impl(&self) -> ImplGenerics {
        self.w_generics.split_for_impl().0
    }

    fn base_ty(&self) -> TypeGenerics {
        self.generics.split_for_impl().1
    }

    fn q_ty(&self) -> TypeGenerics {
        self.q_generics.split_for_impl().1
    }

    fn w_ty(&self) -> TypeGenerics {
        self.w_generics.split_for_impl().1
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TransformIdent {
    Modified,
    Added,
}

impl TransformIdent {
    fn to_tokens(&self, crate_name: &Ident) -> TokenStream {
        match self {
            Self::Modified => quote!(#crate_name::fetch::Modified),
            Self::Added => quote!(#crate_name::fetch::Added),
        }
    }
}

impl Parse for TransformIdent {
    fn parse(input: syn::parse::ParseStream) -> Result<Self> {
        let ident = input.parse::<Ident>()?;
        if ident == "Modified" {
            Ok(Self::Modified)
        } else if ident == "Added" {
            Ok(Self::Added)
        } else {
            Err(Error::new(
                ident.span(),
                format!("Unknown transform {ident}"),
            ))
        }
    }
}
