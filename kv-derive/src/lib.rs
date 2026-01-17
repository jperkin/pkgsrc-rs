/*
 * Copyright (c) 2025 Jonathan Perkin <jonathan@perkin.org.uk>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

//! Derive macro for parsing `KEY=VALUE` formats.
//!
//! This crate provides [`macro@Kv`] for automatically implementing parsers
//! for structs from `KEY=VALUE` formatted input.
//!
//! # Field Types
//!
//! | Rust Type | Attribute | Behavior |
//! |-----------|-----------|----------|
//! | `T` | | Required single value |
//! | `Option<T>` | | Optional single value |
//! | `Vec<T>` | | Whitespace-separated values on single line |
//! | `Option<Vec<T>>` | | Optional whitespace-separated values |
//! | `Vec<T>` | `#[kv(multiline)]` | Multiple lines collected into Vec |
//! | `Option<Vec<T>>` | `#[kv(multiline)]` | Optional multiple lines |
//! | `HashMap<String, String>` | `#[kv(collect)]` | Collects unhandled keys |
//!
//! # Container Attributes
//!
//! - `#[kv(allow_unknown)]` - Ignore unknown keys instead of returning an error
//!
//! # Field Attributes
//!
//! - `#[kv(variable = "KEY")]` - Use custom key name instead of uppercased field name
//! - `#[kv(multiline)]` - Collect multiple lines with the same key into a `Vec`
//! - `#[kv(collect)]` - Collect all unhandled keys into this `HashMap<String, String>`
//!
//! # Duplicate Key Behavior
//!
//! For non-multiline fields, duplicate keys overwrite the previous value.
//! For multiline fields, each occurrence appends to the `Vec`.
//!
//! # Examples
//!
//! ```
//! use indoc::indoc;
//! use pkgsrc::kv::{KvError, Kv};
//! use pkgsrc::PkgName;
//!
//! #[derive(Kv)]
//! pub struct Package {
//!     pkgname: PkgName,
//!     #[kv(variable = "SIZE_PKG")]
//!     size: u64,
//!     #[kv(multiline)]
//!     description: Vec<String>,
//!     homepage: Option<String>,
//! }
//!
//! let input = indoc! {"
//!     PKGNAME=foo-1.0
//!     SIZE_PKG=1234
//!     DESCRIPTION=A package that does
//!     DESCRIPTION=many interesting things.
//! "};
//! let pkg = Package::parse(input)?;
//! assert_eq!(pkg.pkgname.pkgbase(), "foo");
//! assert_eq!(pkg.size, 1234);
//! assert_eq!(pkg.description, vec!["A package that does", "many interesting things."]);
//! assert_eq!(pkg.homepage, None);
//!
//! // Missing required fields return an error.
//! assert!(Package::parse("PKGNAME=bar-1.0\n").is_err());
//! # Ok::<(), KvError>(())
//! ```
//!
//! Use `collect` to collect unhandled keys into a `HashMap`, for example
//! when parsing `+BUILD_INFO` where arbitrary variables will be present:
//!
//! ```
//! use indoc::indoc;
//! use std::collections::HashMap;
//! use pkgsrc::kv::{KvError, Kv};
//!
//! #[derive(Kv)]
//! pub struct BuildInfo {
//!     build_host: Option<String>,
//!     machine_arch: Option<String>,
//!     #[kv(collect)]
//!     vars: HashMap<String, String>,
//! }
//!
//! let input = indoc! {"
//!     BUILD_DATE=2025-01-15 10:30:00 +0000
//!     BUILD_HOST=builder.example.com
//!     MACHINE_ARCH=x86_64
//!     PKGPATH=devel/example
//! "};
//! let info = BuildInfo::parse(input)?;
//! assert_eq!(info.build_host, Some("builder.example.com".to_string()));
//! assert_eq!(info.machine_arch, Some("x86_64".to_string()));
//! assert_eq!(info.vars.get("PKGPATH"), Some(&"devel/example".to_string()));
//! assert_eq!(info.vars.get("VARBASE"), None);
//! # Ok::<(), KvError>(())
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Attribute, Data, DeriveInput, Field, Fields, GenericArgument, Ident,
    PathArguments, Type, parse_macro_input,
};

/// Derive macro for parsing `KEY=VALUE` formatted input.
///
/// Generates a `parse` method that parses the struct from a string
/// containing `KEY=VALUE` pairs separated by newlines.
///
/// See the [module documentation](crate) for detailed usage.
#[proc_macro_derive(Kv, attributes(kv))]
pub fn derive_kv(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match generate_impl(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Main implementation generator.
fn generate_impl(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let container_attrs = ContainerAttrs::parse(&input.attrs)?;

    let fields = extract_named_fields(input)?;

    let parsed_fields: Vec<ParsedField> = fields
        .iter()
        .map(ParsedField::from_field)
        .collect::<syn::Result<_>>()?;

    let collect_field =
        parsed_fields.iter().find(|f| f.kind == FieldKind::Collect);
    let regular_fields: Vec<_> = parsed_fields
        .iter()
        .filter(|f| f.kind != FieldKind::Collect)
        .collect();

    let field_decls = generate_field_declarations(&parsed_fields);
    let match_arms = generate_match_arms(&regular_fields);
    let unknown_handling =
        generate_unknown_handling(&container_attrs, collect_field);
    let field_extracts: Vec<_> = parsed_fields
        .iter()
        .map(ParsedField::extract_expr)
        .collect();
    let field_names: Vec<_> = parsed_fields.iter().map(|f| &f.ident).collect();

    let serde_impl = generate_serde_impl(name, &parsed_fields);

    Ok(quote! {
        impl #name {
            /// Parses from `KEY=VALUE` formatted input.
            ///
            /// # Errors
            ///
            /// Returns an error if:
            /// - A line doesn't contain `=`
            /// - A required field is missing
            /// - A value fails to parse into its target type
            /// - An unknown key is encountered (unless `allow_unknown` is set)
            pub fn parse(input: &str) -> std::result::Result<Self, ::pkgsrc::kv::KvError> {
                use ::pkgsrc::kv::FromKv;

                #(#field_decls)*

                let input_start = input.as_ptr() as usize;

                for line in input.lines() {
                    if line.is_empty() {
                        continue;
                    }

                    // Use pointer arithmetic to compute the line offset.
                    // This correctly handles both LF and CRLF line endings.
                    let line_offset = line.as_ptr() as usize - input_start;

                    let eq_pos = match line.find('=') {
                        Some(p) => p,
                        None => {
                            return Err(::pkgsrc::kv::KvError::ParseLine(::pkgsrc::kv::Span {
                                offset: line_offset,
                                len: line.len(),
                            }));
                        }
                    };

                    let key = &line[..eq_pos];
                    let value = &line[eq_pos + 1..];
                    let value_offset = line_offset + eq_pos + 1;
                    let value_span = ::pkgsrc::kv::Span {
                        offset: value_offset,
                        len: value.len(),
                    };

                    match key {
                        #(#match_arms)*
                        #unknown_handling
                    }
                }

                Ok(#name {
                    #(#field_names: #field_extracts,)*
                })
            }
        }

        #serde_impl
    })
}

/// Extracts named fields from a struct, returning an error for other types.
fn extract_named_fields(
    input: &DeriveInput,
) -> syn::Result<&syn::punctuated::Punctuated<Field, syn::token::Comma>> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            input,
            "Kv derive only supports structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            input,
            "Kv derive only supports structs with named fields",
        ));
    };
    Ok(&fields.named)
}

/// Generates variable declarations for parsing state.
fn generate_field_declarations(fields: &[ParsedField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            let state_ty = f.state_type();
            if f.kind == FieldKind::Collect {
                quote! { let mut #ident: #state_ty = std::collections::HashMap::new(); }
            } else {
                quote! { let mut #ident: #state_ty = None; }
            }
        })
        .collect()
}

/// Generates match arms for known keys.
fn generate_match_arms(fields: &[&ParsedField]) -> Vec<TokenStream2> {
    fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            let key_name = &f.key_name;
            let merge_expr = f.merge_expr();
            quote! {
                #key_name => {
                    #ident = Some(#merge_expr);
                }
            }
        })
        .collect()
}

/// Generates the fallback arm for unknown keys.
fn generate_unknown_handling(
    container_attrs: &ContainerAttrs,
    collect_field: Option<&ParsedField>,
) -> TokenStream2 {
    match collect_field {
        Some(field) => {
            let ident = &field.ident;
            quote! {
                _ => {
                    #ident.insert(key.to_string(), value.to_string());
                }
            }
        }
        None if container_attrs.allow_unknown => {
            quote! { _ => {} }
        }
        None => {
            quote! {
                unknown => {
                    return Err(::pkgsrc::kv::KvError::UnknownVariable {
                        variable: unknown.to_string(),
                        span: ::pkgsrc::kv::Span {
                            offset: line_offset,
                            len: unknown.len(),
                        },
                    });
                }
            }
        }
    }
}

/// Generates serde Serialize/Deserialize implementations.
///
/// These are feature-gated with `#[cfg(feature = "serde")]`.
fn generate_serde_impl(name: &Ident, fields: &[ParsedField]) -> TokenStream2 {
    let field_defs: Vec<_> = fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            let ty = &f.original_type;
            let key_name = &f.key_name;

            let serde_attrs = match f.kind {
                FieldKind::Required | FieldKind::Vec | FieldKind::MultiLine => {
                    quote! {
                        #[serde(rename = #key_name)]
                    }
                }
                FieldKind::Optional | FieldKind::OptionVec | FieldKind::OptionMultiLine => {
                    quote! {
                        #[serde(rename = #key_name, default, skip_serializing_if = "Option::is_none")]
                    }
                }
                FieldKind::Collect => {
                    quote! {
                        #[serde(flatten)]
                    }
                }
            };

            quote! {
                #serde_attrs
                #ident: #ty
            }
        })
        .collect();

    let field_names: Vec<_> = fields.iter().map(|f| &f.ident).collect();

    let to_fields: Vec<_> = fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            quote! { #ident: self.#ident.clone() }
        })
        .collect();

    quote! {
        #[cfg(feature = "serde")]
        impl serde::Serialize for #name {
            fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                #[derive(serde::Serialize)]
                struct Helper {
                    #(#field_defs,)*
                }

                let helper = Helper {
                    #(#to_fields,)*
                };
                helper.serialize(serializer)
            }
        }

        #[cfg(feature = "serde")]
        impl<'de> serde::Deserialize<'de> for #name {
            fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                #[derive(serde::Deserialize)]
                struct Helper {
                    #(#field_defs,)*
                }

                let helper = Helper::deserialize(deserializer)?;
                Ok(Self {
                    #(#field_names: helper.#field_names,)*
                })
            }
        }
    }
}

/// Container-level attributes parsed from `#[kv(...)]`.
#[derive(Default)]
struct ContainerAttrs {
    /// If true, unknown keys are silently ignored.
    allow_unknown: bool,
}

impl ContainerAttrs {
    /// Parses container attributes from a slice of attributes.
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut result = Self::default();

        for attr in attrs {
            if !attr.path().is_ident("kv") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("allow_unknown") {
                    result.allow_unknown = true;
                    Ok(())
                } else {
                    Err(meta.error(
                        "unknown container attribute; expected `allow_unknown`",
                    ))
                }
            })?;
        }

        Ok(result)
    }
}

/// Field-level attributes parsed from `#[kv(...)]`.
#[derive(Default)]
struct FieldAttrs {
    /// Custom key name override.
    variable: Option<String>,
    /// Whether this field collects multiple lines.
    multiline: bool,
    /// Whether this field collects unhandled keys.
    collect: bool,
}

impl FieldAttrs {
    /// Parses field attributes from a slice of attributes.
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut result = Self::default();

        for attr in attrs {
            if !attr.path().is_ident("kv") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("variable") {
                    let lit: syn::LitStr = meta.value()?.parse()?;
                    result.variable = Some(lit.value());
                    Ok(())
                } else if meta.path.is_ident("multiline") {
                    result.multiline = true;
                    Ok(())
                } else if meta.path.is_ident("collect") {
                    result.collect = true;
                    Ok(())
                } else {
                    Err(meta.error(
                        "unknown field attribute; expected `variable`, `multiline`, or `collect`",
                    ))
                }
            })?;
        }

        Ok(result)
    }
}

/// Classification of how a field should be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldKind {
    /// `T` - required single value.
    Required,
    /// `Option<T>` - optional single value.
    Optional,
    /// `Vec<T>` - whitespace-separated values on one line.
    Vec,
    /// `Option<Vec<T>>` - optional whitespace-separated values.
    OptionVec,
    /// `Vec<T>` with `multiline` - multiple lines appended.
    MultiLine,
    /// `Option<Vec<T>>` with `multiline` - optional multiple lines.
    OptionMultiLine,
    /// `HashMap<String, String>` with `collect` - collects unhandled keys.
    Collect,
}

/// A parsed and analyzed struct field.
struct ParsedField {
    /// The field identifier.
    ident: Ident,
    /// The key name used in KEY=VALUE format.
    key_name: String,
    /// How this field should be parsed.
    kind: FieldKind,
    /// The inner type (e.g., `T` from `Vec<T>`).
    inner_type: Type,
    /// The original declared type.
    original_type: Type,
}

impl ParsedField {
    /// Analyzes a field and extracts parsing metadata.
    fn from_field(field: &Field) -> syn::Result<Self> {
        let ident = field.ident.clone().ok_or_else(|| {
            syn::Error::new_spanned(field, "expected named field")
        })?;

        let attrs = FieldAttrs::parse(&field.attrs)?;

        // Validate collect field type
        if attrs.collect {
            validate_collect_type(&field.ty, field)?;
            return Ok(Self {
                ident,
                key_name: String::new(),
                kind: FieldKind::Collect,
                inner_type: field.ty.clone(),
                original_type: field.ty.clone(),
            });
        }

        // Validate multiline is only used with Vec types
        if attrs.multiline
            && extract_type_param(&field.ty, "Vec").is_none()
            && extract_option_vec_inner(&field.ty).is_none()
        {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "`multiline` attribute requires `Vec<T>` or `Option<Vec<T>>` type",
            ));
        }

        let key_name = attrs
            .variable
            .unwrap_or_else(|| ident.to_string().to_uppercase());

        let (kind, inner_type) = analyze_type(&field.ty, attrs.multiline);

        Ok(Self {
            ident,
            key_name,
            kind,
            inner_type,
            original_type: field.ty.clone(),
        })
    }

    /// Returns the type used during parsing to accumulate values.
    fn state_type(&self) -> TokenStream2 {
        let inner = &self.inner_type;
        match self.kind {
            FieldKind::Required | FieldKind::Optional => {
                quote! { Option<#inner> }
            }
            FieldKind::Vec
            | FieldKind::OptionVec
            | FieldKind::MultiLine
            | FieldKind::OptionMultiLine => {
                quote! { Option<Vec<#inner>> }
            }
            FieldKind::Collect => {
                quote! { std::collections::HashMap<String, String> }
            }
        }
    }

    /// Generates an expression to merge a new value into the accumulator.
    fn merge_expr(&self) -> TokenStream2 {
        let inner = &self.inner_type;
        let ident = &self.ident;

        match self.kind {
            FieldKind::Required | FieldKind::Optional => {
                quote! {
                    <#inner as FromKv>::from_kv(value, value_span)?
                }
            }
            FieldKind::Vec | FieldKind::OptionVec => {
                quote! {
                    {
                        let mut items = Vec::new();
                        let mut word_start = 0;
                        let value_bytes = value.as_bytes();
                        let mut in_word = false;

                        for (i, &b) in value_bytes.iter().enumerate() {
                            let is_ws = b == b' ' || b == b'\t';
                            if is_ws && in_word {
                                let word = &value[word_start..i];
                                let word_offset = value_offset + word_start;
                                let word_span = ::pkgsrc::kv::Span { offset: word_offset, len: word.len() };
                                items.push(<#inner as FromKv>::from_kv(word, word_span)?);
                                in_word = false;
                            } else if !is_ws && !in_word {
                                word_start = i;
                                in_word = true;
                            }
                        }
                        if in_word {
                            let word = &value[word_start..];
                            let word_offset = value_offset + word_start;
                            let word_span = ::pkgsrc::kv::Span { offset: word_offset, len: word.len() };
                            items.push(<#inner as FromKv>::from_kv(word, word_span)?);
                        }
                        items
                    }
                }
            }
            FieldKind::MultiLine | FieldKind::OptionMultiLine => {
                quote! {
                    {
                        let mut vec = #ident.unwrap_or_default();
                        vec.push(<#inner as FromKv>::from_kv(value, value_span)?);
                        vec
                    }
                }
            }
            FieldKind::Collect => {
                // Handled separately in unknown_handling
                quote! { unreachable!() }
            }
        }
    }

    /// Generates an expression to extract the final value from the accumulator.
    fn extract_expr(&self) -> TokenStream2 {
        let ident = &self.ident;
        let key_name = &self.key_name;

        match self.kind {
            FieldKind::Required | FieldKind::Vec | FieldKind::MultiLine => {
                quote! {
                    #ident.ok_or_else(|| ::pkgsrc::kv::KvError::Incomplete(#key_name.to_string()))?
                }
            }
            FieldKind::Optional
            | FieldKind::OptionVec
            | FieldKind::OptionMultiLine
            | FieldKind::Collect => {
                quote! { #ident }
            }
        }
    }
}

/// Validates that a collect field has the correct type.
fn validate_collect_type(ty: &Type, field: &Field) -> syn::Result<()> {
    let err = || {
        syn::Error::new_spanned(
            field,
            "`collect` attribute requires `HashMap<String, String>` type",
        )
    };
    let Type::Path(type_path) = ty else {
        return Err(err());
    };
    let Some(segment) = type_path.path.segments.last() else {
        return Err(err());
    };
    if segment.ident != "HashMap" {
        return Err(err());
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(err());
    };
    let mut arg_iter = args.args.iter();
    let is_valid = matches!(
        (arg_iter.next(), arg_iter.next(), arg_iter.next()),
        (
            Some(GenericArgument::Type(Type::Path(k))),
            Some(GenericArgument::Type(Type::Path(v))),
            None
        ) if k.path.is_ident("String") && v.path.is_ident("String")
    );
    if is_valid { Ok(()) } else { Err(err()) }
}

/// Analyzes a type to determine its field kind and inner type.
fn analyze_type(ty: &Type, multiline: bool) -> (FieldKind, Type) {
    // Check for Option<Vec<T>>
    if let Some(vec_inner) = extract_option_vec_inner(ty) {
        let kind = if multiline {
            FieldKind::OptionMultiLine
        } else {
            FieldKind::OptionVec
        };
        return (kind, vec_inner);
    }

    // Check for Option<T>
    if let Some(inner) = extract_type_param(ty, "Option") {
        return (FieldKind::Optional, inner);
    }

    // Check for Vec<T>
    if let Some(inner) = extract_type_param(ty, "Vec") {
        let kind = if multiline {
            FieldKind::MultiLine
        } else {
            FieldKind::Vec
        };
        return (kind, inner);
    }

    // Plain T
    (FieldKind::Required, ty.clone())
}

/// Extracts the inner type from `Option<Vec<T>>`.
fn extract_option_vec_inner(ty: &Type) -> Option<Type> {
    let option_inner = extract_type_param(ty, "Option")?;
    extract_type_param(&option_inner, "Vec")
}

/// Extracts the type parameter from a generic type like `Wrapper<T>`.
fn extract_type_param(ty: &Type, wrapper: &str) -> Option<Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != wrapper {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    Some(inner.clone())
}
