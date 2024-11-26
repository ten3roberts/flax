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
    Expr, GenericArgument, Ident, ItemFn, Pat, Path, ReturnType, Token, Type, TypePath,
    TypeReference,
};

pub(crate) fn system_impl(
    args: SystemAttrs,
    item: ItemFn,
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
            syn::FnArg::Receiver(receiver) => {
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
                    ctor.expr.to_token_stream()
                } else if recv_ty != "Self" {
                    component_ctor_from_type(&crate_name, &recv_name, &receiver.ty)?
                } else {
                    return Err(syn::Error::new(
                        receiver.span(),
                        "Use `self: &Type` syntax or provide explicit component argument",
                    ));
                };

                query_arguments.push(ctor);
                query_idents.push(recv_name);
            }
            syn::FnArg::Typed(pat_type) => {
                let Pat::Ident(arg_ident) = &*pat_type.pat else {
                    return Err(syn::Error::new_spanned(
                        v,
                        "only ident type arguments are supported",
                    ));
                };

                let arg_ident = &arg_ident.ident;

                let ty = &pat_type.ty;

                let ctor = if let Some(ctor) = args.query_arg(arg_ident) {
                    ctor.expr.to_token_stream()
                } else {
                    component_ctor_from_type(&crate_name, arg_ident, ty)?
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
                    #call_sig(#(#query_idents),*)
                })
            }
        }
        // par
        ([], ReturnType::Default, true) => {
            quote! {
                par_for_each(|(#(#query_idents,)*)| {
                    #call_sig(#(#query_idents),*)
                })
            }
        }
        ([], ReturnType::Type(_, _), true) => {
            quote! {
                try_par_for_each(|(#(#query_idents,)*)| {
                    #call_sig(#(#query_idents),*)
                })
            }
        }
        (items_types, ret, _) => {
            let item_names = (0..items_types.len())
                .map(|i| format_ident!("__extra_arg_{i}"))
                .collect_vec();

            let call = if *ret == ReturnType::Default {
                quote! { #call_sig(#(#query_idents),* #(,#with_adapters #item_names)* ) }
            } else {
                quote! { #call_sig(#(#query_idents),* #(,#with_adapters #item_names)* )?; }
            };

            quote! {
                build(|#(mut #item_names: #items_types,)* mut main_query: #crate_name::QueryBorrow<'_, _, _>| {
                    for (#(#query_idents,)*) in &mut main_query {
                        #call
                    }
                })
            }
        }
    };

    let filters = args.filter.iter().flat_map(|v| v.iter()).collect_vec();
    let query =
        quote! { #crate_name::Query::new( (#(#query_arguments,)*)).with_filter((#(#filters,)*)) };

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

fn component_ctor_from_type(
    crate_name: &Ident,
    ident: &Ident,
    ty: &Type,
) -> syn::Result<TokenStream> {
    let tt = match ty {
        Type::Reference(ty_ref) if ty_ref.mutability.is_none() => quote!(#ident()),
        Type::Reference(ty_ref) if ty_ref.mutability.is_some() => {
            quote!(#crate_name::Component::as_mut(#ident()))
        }
        Type::Path(TypePath {
            path: Path { segments, .. },
            ..
        }) => {
            match segments.last().map(|v| v.ident.to_string()).as_deref() {
                Some("Option") => {
                    let inner = match &segments[0].arguments {
                        syn::PathArguments::AngleBracketed(args) => {
                            let GenericArgument::Type(ty) = &args.args[0] else {
                                return Err(syn::Error::new(
                                    ident.span(),
                                    "Malformed option generic argument list",
                                ));
                            };

                            component_ctor_from_type(crate_name, ident, ty)?
                        }
                        _ => {
                            return Err(syn::Error::new(
                                ident.span(),
                                "Expected a single angle bracketed type",
                            ))
                        }
                    };

                    quote!(#crate_name::fetch::FetchExt::opt(#inner))
                }
                Some("Entity") => {
                    quote!(#crate_name::entity_ids())
                }
                Some("EntityRef") => {
                    quote!(#crate_name::fetch::entity_refs())
                }
                _ => {
                    quote!(#crate_name::fetch::FetchExt::copied(#ident()))
                }
            }

            // if segments.len() == 1 && segments[0].ident == "Option" {
            // } else {
            // }
        }
        _ => return Err(syn::Error::new(ident.span(), "Unsupported type")),
    };

    Ok(tt)
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
            } else if lookahead.peek(kw::filter) {
                if args.filter.is_some() {
                    return Err(input.error("expected only a single `filter` argument"));
                }

                let _ = input.parse::<kw::filter>()?;
                let content;
                let _ = syn::parenthesized!(content in input);
                args.filter = Some(content.parse_terminated(Expr::parse, Token![,])?);
            } else if lookahead.peek(kw::par) {
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
    syn::custom_keyword!(filter);
    syn::custom_keyword!(par);
    syn::custom_keyword!(with_world);
    // syn::custom_keyword!(with_world_mut); // NOTE: this will always panic due to a query always being borrowed
    syn::custom_keyword!(with_cmd);
    syn::custom_keyword!(with_cmd_mut);
    syn::custom_keyword!(with_query);
}
