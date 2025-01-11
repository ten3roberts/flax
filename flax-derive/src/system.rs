use heck::ToSnakeCase;
use itertools::Itertools;
use proc_macro2::{Span, TokenStream};
use proc_macro_crate::FoundCrate;
use quote::{format_ident, quote, ToTokens};
use syn::{
    ext::IdentExt,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
    Expr, FnArg, GenericArgument, Ident, Pat, Path, ReturnType, Token, Type, TypePath,
    TypeReference,
};

use crate::maybe_fn::MaybeItemFn;

pub(crate) fn system_impl(
    args: SystemAttrs,
    item: MaybeItemFn,
) -> syn::Result<proc_macro2::TokenStream> {
    let crate_name = match proc_macro_crate::crate_name("flax").expect("Failed to get crate name") {
        FoundCrate::Itself => Ident::new("crate", Span::call_site()),
        FoundCrate::Name(name) => Ident::new(&name, Span::call_site()),
    };

    let arguments = &item.sig.inputs;

    let mut query_arguments = Vec::new();
    let mut query_idents = Vec::new();

    let is_method = matches!(item.sig.inputs.first(), Some(&syn::FnArg::Receiver(_)));

    let with_items = &*args.with;

    for v in arguments.iter().take(arguments.len() - with_items.len()) {
        match v {
            FnArg::Receiver(receiver) => {
                let recv_ty = match &*receiver.ty {
                    Type::Reference(TypeReference { elem, .. }) => {
                        let Type::Path(TypePath { path, .. }) = &**elem else {
                            return Err(syn::Error::new(
                                receiver.span(),
                                "Self type is not supported",
                            ));
                        };

                        extract_path_ident(path)
                    }

                    Type::Path(TypePath { path, .. }) => extract_path_ident(path),
                    _ => {
                        return Err(syn::Error::new(
                            receiver.span(),
                            "Self type is not supported",
                        ))
                    }
                };

                let recv_name = format_ident!("{}", recv_ty.to_string().to_snake_case());

                let ctor = if let Some(ctor) = args.query_arg(&format_ident!("self")) {
                    let recv_ty = &*receiver.ty;
                    QueryArgument {
                        ctor: ctor.expr.to_token_stream(),
                        adapter: quote! {#recv_name},
                        ty: quote! {#recv_ty},
                    }
                } else if recv_ty != "Self" {
                    component_ctor_from_type(&crate_name, &recv_name, &receiver.ty, quote! {})?
                } else {
                    return Err(syn::Error::new(
                        receiver.span(),
                        "Use `self: &Type` syntax or provide explicit component argument",
                    ));
                };

                query_arguments.push(ctor);
                query_idents.push(recv_name);
            }
            FnArg::Typed(pat_type) => {
                let Pat::Ident(arg_ident) = &*pat_type.pat else {
                    return Err(syn::Error::new_spanned(
                        v,
                        "only ident type arguments are supported",
                    ));
                };

                let arg_ident = &arg_ident.ident;

                let ty = &pat_type.ty;

                let required = args.require_all
                    || args
                        .require
                        .as_ref()
                        .is_some_and(|v| v.iter().contains(arg_ident));

                let ctor = if let Some(ctor) = args.query_arg(arg_ident) {
                    QueryArgument {
                        ctor: ctor.expr.to_token_stream(),
                        adapter: quote! {#arg_ident},
                        ty: quote! { _ },
                    }
                } else {
                    component_ctor_from_type(
                        &crate_name,
                        arg_ident,
                        ty,
                        if required {
                            quote! {.expect()}
                        } else {
                            quote! {}
                        },
                    )?
                };

                query_arguments.push(ctor);
                query_idents.push(arg_ident.clone());
            }
        }
    }

    let fn_ident = &item.sig.ident;
    if fn_ident.to_string().ends_with("system") {
        return Err(syn::Error::new_spanned(
            fn_ident,
            "System function must not end with `system`",
        ));
    }

    let system_name = format_ident!("{fn_ident}_system");

    let vis = &item.vis;

    let call_sig = match is_method {
        true => quote!(Self::#fn_ident),
        false => item.sig.ident.to_token_stream(),
    };

    let with_types = with_items.iter().map(|v| &v.ty).collect_vec();
    let with_exprs = with_items.iter().map(|v| &v.expr).collect_vec();
    let with_adapters = with_items.iter().map(|v| &v.adapter);

    let query_adapters = query_arguments.iter().map(|v| &v.adapter);

    let iter_fn = match (&*with_types, &item.sig.output, args.par) {
        ([], ReturnType::Default, false) => {
            quote! {
                for_each(|(#(#query_idents,)*)| {
                    #call_sig(#(#query_idents),*)
                })
            }
        }
        ([], ReturnType::Type(_, _), false) => {
            quote! {
                try_for_each(|(#(#query_idents,)*)| {
                    #call_sig(#(#query_adapters),*)
                })
            }
        }
        // par
        ([], ReturnType::Default, true) => {
            quote! {
                par_for_each(|(#(#query_idents,)*)| {
                    #call_sig(#(#query_adapters),*)
                })
            }
        }
        ([], ReturnType::Type(_, _), true) => {
            quote! {
                try_par_for_each(|(#(#query_idents,)*)| {
                    #call_sig(#(#query_adapters),*)
                })
            }
        }
        (items_types, ret, _) => {
            let item_names = (0..items_types.len())
                .map(|i| format_ident!("__extra_arg_{i}"))
                .collect_vec();

            let (call_expr, ret_sig, ret) = if *ret == ReturnType::Default {
                (
                    quote! { #call_sig(#(#query_adapters),* #(,#with_adapters #item_names)* ) },
                    quote! { () },
                    quote! {},
                )
            } else {
                (
                    quote! { #call_sig(#(#query_adapters),* #(,#with_adapters #item_names)* )?; },
                    quote! { #crate_name::__internal::anyhow::Result<()> },
                    quote! { Ok(()) },
                )
            };

            let query_types = query_arguments.iter().map(|v| &v.ty);

            quote! {
                build(|#(mut #item_names: #items_types,)* mut main_query: #crate_name::QueryBorrow<'_, _, _>| -> #ret_sig {
                    for v in &mut main_query {
                        let (#(#query_idents,)*): (#(#query_types,)*) = v;
                        #call_expr
                    }

                    #ret
                })
            }
        }
    };

    let filters = args.filter.iter().flat_map(|v| v.iter()).collect_vec();
    let query_ctors = query_arguments.iter().map(|v| &v.ctor);
    let query =
        quote! { #crate_name::Query::new( (#(#query_ctors,)*)).with_filter((#(#filters,)*)) };

    let system_impl = quote! {
        #vis fn #system_name() -> #crate_name::system::BoxedSystem {
            #crate_name::system::System::builder()
                .with_name(stringify!(#fn_ident))
                #(.#with_exprs)*
                .with_query(#query)
                .#iter_fn
                .boxed()
        }
    };

    Ok(quote! {
        #item

        #system_impl
    })
}

fn extract_path_ident(path: &Path) -> &Ident {
    &path.segments.last().unwrap().ident
}

fn matches_path(ty: &Type, ident: &str) -> bool {
    if let Type::Path(ty) = ty {
        if let Some(last) = ty.path.segments.last() {
            return last.ident == ident;
        }
    }

    false
}

struct QueryArgument {
    ctor: TokenStream,
    ty: TokenStream,
    adapter: TokenStream,
}

fn component_ctor_from_type(
    crate_name: &Ident,
    ident: &Ident,
    ty: &Type,
    required: TokenStream,
) -> syn::Result<QueryArgument> {
    let mut adapter = None;
    let mut new_ty = None;

    let tt = match ty {
        // Handle `String` components and `&str` arguments
        Type::Reference(ty_ref) if matches_path(&ty_ref.elem, "str") => {
            if ty_ref.mutability.is_some() {
                adapter = Some(quote! {(&mut **(#ident))});
                new_ty = Some(quote! { &mut String });
                quote!(#crate_name::Component::as_mut(#ident())#required)
            } else {
                adapter = Some(quote! {(&**(#ident))});
                new_ty = Some(quote! { &String });
                quote!(#ident()#required)
            }
        }
        Type::Reference(ty_ref) => match &*ty_ref.elem {
            Type::Slice(slice_ty) => {
                let inner = &*slice_ty.elem;

                if ty_ref.mutability.is_some() {
                    adapter = Some(quote! {(&mut **(#ident))});
                    new_ty = Some(quote! { &mut Vec<#inner> });
                    quote!(#crate_name::Component::as_mut(#ident())#required)
                } else {
                    adapter = Some(quote! {(&**(#ident))});
                    new_ty = Some(quote! { &Vec<#inner> });
                    quote!(#ident()#required)
                }
            }
            _ if ty_ref.mutability.is_some() => {
                quote!(#crate_name::Component::as_mut(#ident())#required)
            }
            _ => {
                quote!(#ident()#required)
            }
        },
        // Direct type
        Type::Path(TypePath {
            path: Path { segments, .. },
            ..
        }) => match segments.last().map(|v| v.ident.to_string()).as_deref() {
            // Option<T>
            Some("Option") => {
                let QueryArgument {
                    ctor,
                    adapter: _,
                    ty: inner_ty,
                } = match &segments[0].arguments {
                    syn::PathArguments::AngleBracketed(args) => {
                        let GenericArgument::Type(ty) = &args.args[0] else {
                            return Err(syn::Error::new(
                                ident.span(),
                                "Malformed option generic argument list",
                            ));
                        };

                        component_ctor_from_type(crate_name, ident, ty, quote! {})?
                    }
                    _ => {
                        return Err(syn::Error::new(
                            ident.span(),
                            "Expected a single angle bracketed type",
                        ))
                    }
                };

                new_ty = Some(quote! { Option<#inner_ty> });

                quote!(#crate_name::fetch::FetchExt::opt(#ctor))
            }
            Some("Entity") => {
                quote!(#crate_name::entity_ids())
            }
            Some("EntityRef") => {
                quote!(#crate_name::fetch::entity_refs())
            }
            _ => {
                quote!(#crate_name::fetch::FetchExt::copied(#ident())#required)
            }
        },
        _ => return Err(syn::Error::new(ident.span(), "Unsupported type")),
    };

    Ok(QueryArgument {
        ctor: tt,
        adapter: adapter.unwrap_or_else(|| quote! {#ident}),
        ty: new_ty.unwrap_or_else(|| quote! { #ty }),
    })
}

struct WithExpr {
    ty: Type,
    expr: TokenStream,
    adapter: TokenStream,
}

impl WithExpr {
    fn new(ty: Type, expr: TokenStream, adapter: TokenStream) -> Self {
        Self { ty, expr, adapter }
    }
}

#[derive(Default)]
pub(crate) struct SystemAttrs {
    query_args: Option<Fields>,
    require: Option<Punctuated<Ident, Token![,]>>,
    require_all: bool,
    filter: Option<Punctuated<Expr, Token![,]>>,
    par: bool,
    with: Vec<WithExpr>,
}

impl SystemAttrs {
    pub(crate) fn query_arg(&self, ident: &Ident) -> Option<&Field> {
        self.query_args
            .iter()
            .flat_map(|v| v.0.iter())
            .find(|v| v.name == *ident)
    }
}

impl Parse for SystemAttrs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let crate_name =
            match proc_macro_crate::crate_name("flax").expect("Failed to get crate name") {
                FoundCrate::Itself => Ident::new("crate", Span::call_site()),
                FoundCrate::Name(name) => Ident::new(&name, Span::call_site()),
            };

        let mut args = Self::default();

        while !input.is_empty() {
            let lookahead = input.lookahead1();
            if lookahead.peek(kw::args) {
                if args.query_args.is_some() {
                    return Err(input.error("expected only a single `args` argument"));
                }
                args.query_args = Some(input.parse()?);
            }
            //
            else if lookahead.peek(kw::require) {
                if args.require.is_some() {
                    return Err(input.error("expected only a single `require` argument"));
                }

                let _ = input.parse::<kw::require>()?;
                let content;
                let _ = syn::parenthesized!(content in input);
                args.require = Some(content.parse_terminated(Ident::parse, Token![,])?);
            }
            //
            else if lookahead.peek(kw::require_all) {
                let _ = input.parse::<kw::require_all>()?;
                args.require_all = true;
            }
            //
            else if lookahead.peek(kw::filter) {
                if args.filter.is_some() {
                    return Err(input.error("expected only a single `filter` argument"));
                }

                let _ = input.parse::<kw::filter>()?;
                let content;
                let _ = syn::parenthesized!(content in input);
                args.filter = Some(content.parse_terminated(Expr::parse, Token![,])?);
            }
            //
            else if lookahead.peek(kw::par) {
                let _ = input.parse::<kw::par>()?;
                args.par = true;
            }
            //
            else if lookahead.peek(kw::with_world) {
                let _ = input.parse::<kw::with_world>()?;
                let ty = syn::parse2(quote!(&#crate_name::World)).unwrap();

                args.with
                    .push(WithExpr::new(ty, quote!(with_world()), quote!()));
            }
            //
            else if lookahead.peek(kw::with_cmd) {
                let _ = input.parse::<kw::with_cmd>()?;
                let ty = syn::parse2(quote!(&#crate_name::CommandBuffer)).unwrap();

                args.with
                    .push(WithExpr::new(ty, quote!(with_cmd()), quote!()));
            }
            //
            else if lookahead.peek(kw::with_cmd_mut) {
                let _ = input.parse::<kw::with_cmd_mut>()?;
                let ty = syn::parse2(quote!(&mut #crate_name::CommandBuffer)).unwrap();

                args.with
                    .push(WithExpr::new(ty, quote!(with_cmd_mut()), quote!()));
            }
            //
            else if lookahead.peek(kw::with_query) {
                let _ = input.parse::<kw::with_query>()?;
                let ty = syn::parse2(quote!(#crate_name::QueryBorrow<'_, _, _>)).unwrap();

                let content;
                let _ = syn::parenthesized!(content in input);
                let expr: Expr = content.parse()?;
                args.with
                    .push(WithExpr::new(ty, quote!(with_query(#expr)), quote!(&mut)));
            }
            //
            else if lookahead.peek(Token![,]) {
                let _ = input.parse::<Token![,]>()?;
            } else {
                return Err(lookahead.error());
            }
        }

        Ok(args)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Fields(pub(crate) Punctuated<Field, Token![,]>);

impl Parse for Fields {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let _ = input.parse::<kw::args>()?;
        let content;
        let _ = syn::parenthesized!(content in input);
        let fields = content.parse_terminated(Field::parse, Token![,])?;
        Ok(Self(fields))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Field {
    pub(crate) name: Ident,
    pub(crate) expr: Expr,
}

impl Parse for Field {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let name = Ident::parse_any(input)?;
        input.parse::<Token![=]>()?;
        let ctor = input.parse()?;
        Ok(Self { name, expr: ctor })
    }
}

mod kw {
    syn::custom_keyword!(args);
    syn::custom_keyword!(require);
    syn::custom_keyword!(require_all);
    syn::custom_keyword!(filter);
    syn::custom_keyword!(par);
    syn::custom_keyword!(with_world);
    // syn::custom_keyword!(with_world_mut); // NOTE: this will always panic due to a query always being borrowed
    syn::custom_keyword!(with_cmd);
    syn::custom_keyword!(with_cmd_mut);
    syn::custom_keyword!(with_query);
}
