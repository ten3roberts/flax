use itertools::{multiunzip, process_results, Itertools};
use proc_macro2::{Span, TokenStream};
use proc_macro_crate::FoundCrate;
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{
    braced, bracketed, parenthesized,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    token::Paren,
    Attribute, DataStruct, DeriveInput, Error, Expr, Ident, Lifetime, Path, Result, Token, Type,
    TypePath, TypeReference,
};

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

fn derive_data_struct(
    crate_name: Ident,
    input: &DeriveInput,
    data: &DataStruct,
) -> Result<TokenStream> {
    let lf = input.generics.lifetimes().next().cloned().ok_or_else(|| {
        Error::new_spanned(
            input.generics.clone(),
            "Fetch struct must have one lifetime",
        )
    })?;

    let name = &input.ident;

    match data.fields {
        syn::Fields::Named(ref fields) => {
            let generics = ('A'..='Z')
                .take(fields.named.len())
                .map(|v| Ident::new(&v.to_string(), Span::call_site()))
                .collect_vec();
            let fetch_name = Ident::new(&format!("{}Fetch", name), Span::call_site());

            let fields = &fields.named;

            let field_names = fields
                .iter()
                .map(|v| v.ident.as_ref().unwrap())
                .collect_vec();

            let types = fields.iter().map(|v| &v.ty).collect_vec();

            let iter = fields
                .iter()
                .zip_eq(&generics)
                .map(|(field, ty)| -> Result<_> {
                    let name = field.ident.as_ref().unwrap();
                    let attrs = Attrs::get(&field.attrs)?;
                    let fetch_expr = attrs
                        .fetch
                        .ok_or_else(|| Error::new(fields.span(), "Missing `fetch` attribute"))?
                        .expr;

                    let field_decl = quote! {
                        #name: #ty
                    };

                    let field_expr = quote! {
                        #fetch_expr
                    };

                    let field_prepare = quote! {
                        #name: self.#name.prepare(world, archetype)
                    };

                    Ok((field_decl, field_expr, field_prepare))
                });

            let (fields_decl, fields_expr, fields_prepare): (Vec<_>, Vec<_>, Vec<_>) =
                process_results(iter, |iter| multiunzip(iter))?;

            let impl_generics = quote!(#(#generics: Fetch<#lf>),*);
            let impl_generics_prepared = quote!(#(#generics: PreparedFetch<#lf>),*);

            let fetch_struct = quote! {
                impl<#lf> #name<#lf> {
                    /// Returns the associated fetch.
                    pub fn as_fetch(&self) -> impl Fetch {
                        use #crate_name::*;

                        pub struct #fetch_name<#(#generics),*> {
                            #(#fields_decl,)*
                        }

                        #[automatically_derived]
                        impl<#lf, #(#generics,)*> #crate_name::Fetch<#lf> for #fetch_name<#(#generics),*>
                            where
                                #(#generics: Fetch,
                                <#generics as #crate_name::Fetch>::Prepared: #crate_name::PreparedFetch<#lf, Item = #types>
                            ),*
                        {
                            const MUTABLE: bool = #(#generics::MUTABLE)|*;

                            type Prepared = #fetch_name<A::Prepared, B::Prepared, C::Prepared>;

                            fn prepare(
                                &#lf self,
                                world: &#lf World,
                                archetype: &#lf Archetype,
                            ) -> Option<Self::Prepared> {
                                Some(#fetch_name {
                                    #(#fields_prepare?,)*
                                })
                            }

                            fn matches(&self, world: &World, archetype: &Archetype) -> bool {
                                #(self.#field_names.matches(world, archetype))&&*
                            }

                            fn describe(&self) -> String {
                                use std::fmt::Write;
                                let mut buf = String::new();
                                write!(buf, stringify!(#name));
                                write!(buf, "{{");
                                #(
                                    write!(buf, stringify!(#field_names));
                                    write!(buf, stringify!(self.#field_names.describe()));
                                )*
                                write!(buf, "}}");
                                buf
                            }

                            fn access(&self, id: ArchetypeId, archetype: &Archetype) -> Vec<Access> {
                                [
                                    #(self.#field_names.access(id, archetype)),*
                                ].concat()
                            }

                            fn difference(&self, archetype: &Archetype) -> Vec<String> {
                                todo!()
                            }
                        }

                        #[automatically_derived]
                        impl<#lf, #(#generics,)*> #crate_name::PreparedFetch<#lf> for #fetch_name<#(#generics),*>
                            where
                                #(#generics: PreparedFetch<Item = #types>),*
                        {

                            type Item = #name<#lf>;

                            unsafe fn fetch(&#lf mut self, slot: #crate_name::archetype::Slot) -> Self::Item {
                                Self::Item {
                                    #(
                                    #field_names: self.#field_names.fetch(slot),
                                    )*
                                }
                            }
                        }

                        #fetch_name {
                            #(#field_names: #fields_expr,)*
                        }
                    }
                }
            };

            Ok(fetch_struct)
        }
        syn::Fields::Unnamed(_) => todo!(),
        syn::Fields::Unit => todo!(),
    }
}

fn map_lifetime(ty: Type, lifetime: Lifetime) -> Type {
    match ty {
        Type::Reference(ty) => Type::Reference(TypeReference {
            lifetime: Some(lifetime),
            ..ty
        }),
        v => v,
    }
}

struct FetchExpr {
    expr: Expr,
}

fn parse_fetch_expr(input: &Attribute) -> Result<FetchExpr> {
    input.parse_args_with(|input: ParseStream| {
        // let path: Path = input.parse()?;
        // parenthesized!(content in input);
        Ok(FetchExpr {
            expr: input.parse()?,
        })
    })
}

struct Attrs {
    fetch: Option<FetchExpr>,
}

impl Attrs {
    fn get(input: &[Attribute]) -> Result<Self> {
        let mut res = Self { fetch: None };

        for attr in input {
            if attr.path.is_ident("fetch") {
                let fetch = parse_fetch_expr(attr)?;
                res.fetch = Some(fetch);
            }
        }

        Ok(res)
    }
}
