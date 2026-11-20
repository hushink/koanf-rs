//! Derive macros for `koanf-rs` schemas.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::Span;
use quote::quote;
use syn::{Data, DeriveInput, Field, Fields, LitStr, Path, parse_macro_input, parse_quote};

/// Derives `koanf_core::Schema` for a struct with named fields.
///
/// Supported field attributes:
///
/// - `#[koanf(secret)]` marks the field as sensitive for debug redaction.
/// - `#[koanf(nested)]` includes secret paths from another `Schema` type.
/// - `#[koanf(rename = "wire_name")]` overrides the path segment for the field.
///
/// The type must also implement `serde::Deserialize`.
#[proc_macro_derive(KoanfSchema, attributes(koanf))]
pub fn derive_koanf_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_schema(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_schema(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            input.ident,
            "KoanfSchema can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &data.fields,
            "KoanfSchema requires named fields",
        ));
    };

    let schema_path = schema_path();
    let mut statements = Vec::new();
    let mut generics = input.generics.clone();

    for field in &fields.named {
        let options = FieldOptions::parse(field)?;
        let key = options
            .rename
            .clone()
            .or_else(|| field.ident.as_ref().map(ToString::to_string))
            .expect("named fields always have identifiers");
        let ty = &field.ty;

        if options.secret {
            statements.push(quote! {
                paths.push(::std::string::String::from(#key));
            });
        } else if options.nested {
            generics
                .make_where_clause()
                .predicates
                .push(parse_quote!(#ty: #schema_path::Schema));
            statements.push(quote! {
                for path in <#ty as #schema_path::Schema>::secret_paths() {
                    paths.push(::std::format!("{}.{}", #key, path));
                }
            });
        }
    }

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics #schema_path::Schema for #name #ty_generics #where_clause {
            const NAME: &'static str = ::std::stringify!(#name);

            fn secret_paths() -> ::std::vec::Vec<::std::string::String> {
                let mut paths = ::std::vec::Vec::new();
                #(#statements)*
                paths
            }
        }
    })
}

fn schema_path() -> Path {
    for package in ["koanf-core", "koanf-rs"] {
        if let Ok(found) = crate_name(package) {
            return match found {
                FoundCrate::Itself => parse_quote!(crate),
                FoundCrate::Name(name) => {
                    let ident = proc_macro2::Ident::new(&name, Span::call_site());
                    parse_quote!(::#ident)
                }
            };
        }
    }
    parse_quote!(::koanf_core)
}

struct FieldOptions {
    secret: bool,
    nested: bool,
    rename: Option<String>,
}

impl FieldOptions {
    fn parse(field: &Field) -> syn::Result<Self> {
        let mut options = Self {
            secret: false,
            nested: false,
            rename: None,
        };
        for attribute in &field.attrs {
            if !attribute.path().is_ident("koanf") {
                continue;
            }
            attribute.parse_nested_meta(|meta| {
                if meta.path.is_ident("secret") {
                    options.secret = true;
                } else if meta.path.is_ident("nested") {
                    options.nested = true;
                } else if meta.path.is_ident("rename") {
                    let value: LitStr = meta.value()?.parse()?;
                    options.rename = Some(value.value());
                } else {
                    return Err(meta.error(
                        "unsupported koanf attribute; expected `secret`, `nested`, or `rename`",
                    ));
                }
                Ok(())
            })?;
        }
        Ok(options)
    }
}
