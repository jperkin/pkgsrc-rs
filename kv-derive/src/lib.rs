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
//! | `Option<T>` | `#[kv(lenient)]` | Optional single value; an unparseable value becomes `None` instead of erroring |
//! | `Vec<T>` | | Whitespace-separated values on single line |
//! | `Option<Vec<T>>` | | Optional whitespace-separated values |
//! | `Vec<T>` | `#[kv(multiline)]` | Multiple lines collected into Vec |
//! | `Option<Vec<T>>` | `#[kv(multiline)]` | Optional multiple lines |
//! | `HashMap<String, String>` | `#[kv(collect)]` | Collects unhandled keys |
//! | `Vec<KvWarning>` | `#[kv(warnings)]` | Collects parse failures from `lenient` fields |
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
//! - `#[kv(lenient)]` - For an `Option<T>` field, treat a value that fails to parse as `None` rather than erroring (recorded in a `#[kv(warnings)]` field if one is present)
//! - `#[kv(warnings)]` - Collect parse failures from `lenient` fields into this `Vec<KvWarning>`
//!
//! # Duplicate Key Behavior
//!
//! For non-multiline fields, duplicate keys overwrite the previous value.
//! For multiline fields, each occurrence appends to the `Vec`.
//!
//! # Examples
//!
//! These examples are written against the [`pkgsrc-kv`] crate, which
//! re-exports this macro alongside the runtime it targets. They are marked
//! `ignore` here only because this engine crate does not depend on the
//! runtime; they run as written once `pkgsrc-kv` is a dependency.
//!
//! [`pkgsrc-kv`]: https://docs.rs/pkgsrc-kv
//!
//! ```ignore
//! use indoc::indoc;
//! use pkgsrc_kv::{Kv, KvError};
//!
//! #[derive(Kv)]
//! pub struct Package {
//!     pkgname: String,
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
//! assert_eq!(pkg.pkgname, "foo-1.0");
//! assert_eq!(pkg.size, 1234);
//! assert_eq!(pkg.description, vec!["A package that does", "many interesting things."]);
//! assert_eq!(pkg.homepage, None);
//!
//! /* Missing required fields return an error. */
//! assert!(Package::parse("PKGNAME=bar-1.0\n").is_err());
//! # Ok::<(), KvError>(())
//! ```
//!
//! Use `collect` to collect unhandled keys into a `HashMap`, for example
//! when parsing `+BUILD_INFO` where arbitrary variables will be present:
//!
//! ```ignore
//! use indoc::indoc;
//! use std::collections::HashMap;
//! use pkgsrc_kv::{Kv, KvError};
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
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Field, Fields, GenericArgument, Ident, Path,
    PathArguments, Type, parse_macro_input,
};

/*
 * Resolve the path to the `pkgsrc-kv` crate as named in the consumer's
 * dependency graph. Generated code references the runtime through this path
 * rather than hardcoding a crate name, so a renamed dependency still works.
 * Since `pkgsrc-kv` re-exports this macro, anything that can name the derive
 * can also name the runtime. A `#[kv(crate = "...")]` container attribute
 * overrides the lookup for unusual setups.
 */
fn kv_crate_path(container_attrs: &ContainerAttrs) -> TokenStream2 {
    if let Some(path) = &container_attrs.crate_path {
        return quote! { #path };
    }
    match crate_name("pkgsrc-kv") {
        Ok(FoundCrate::Itself) => quote! { crate },
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{}", name);
            quote! { ::#ident }
        }
        Err(_) => quote! { ::pkgsrc_kv },
    }
}

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
    let kv = kv_crate_path(&container_attrs);

    let fields = extract_named_fields(input)?;

    let parsed_fields: Vec<ParsedField> = fields
        .iter()
        .map(ParsedField::from_field)
        .collect::<syn::Result<_>>()?;

    let collect_field =
        parsed_fields.iter().find(|f| f.kind == FieldKind::Collect);
    let warnings_field =
        parsed_fields.iter().find(|f| f.kind == FieldKind::Warnings);
    let regular_fields: Vec<_> = parsed_fields
        .iter()
        .filter(|f| {
            f.kind != FieldKind::Collect && f.kind != FieldKind::Warnings
        })
        .collect();

    let field_decls = generate_field_declarations(&parsed_fields);
    let warnings_ident = warnings_field.map(|f| &f.ident);
    let match_arms = generate_match_arms(&regular_fields, warnings_ident, &kv);
    let unknown_handling =
        generate_unknown_handling(&container_attrs, collect_field, &kv);
    let field_extracts: Vec<_> =
        parsed_fields.iter().map(|f| f.extract_expr(&kv)).collect();
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
            /// - A value fails to parse into its target type (unless the
            ///   field is marked `#[kv(lenient)]`)
            /// - An unknown key is encountered (unless `allow_unknown` is set)
            pub fn parse(input: &str) -> std::result::Result<Self, #kv::KvError> {
                use #kv::FromKv;

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
                            return Err(#kv::KvError::ParseLine(#kv::Span {
                                offset: line_offset,
                                len: line.len(),
                            }));
                        }
                    };

                    let key = &line[..eq_pos];
                    let value = &line[eq_pos + 1..];
                    let value_offset = line_offset + eq_pos + 1;
                    let value_span = #kv::Span {
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
            match f.kind {
                FieldKind::Collect => {
                    quote! { let mut #ident: #state_ty = std::collections::HashMap::new(); }
                }
                FieldKind::Warnings => {
                    quote! { let mut #ident: #state_ty = Vec::new(); }
                }
                _ => quote! { let mut #ident: #state_ty = None; },
            }
        })
        .collect()
}

/// Generates match arms for known keys.
fn generate_match_arms(
    fields: &[&ParsedField],
    warnings_ident: Option<&Ident>,
    kv: &TokenStream2,
) -> Vec<TokenStream2> {
    fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            let key_name = &f.key_name;
            if f.lenient {
                let inner = &f.inner_type;
                match warnings_ident {
                    Some(warnings) => quote! {
                        #key_name => {
                            match <#inner as FromKv>::from_kv(value, value_span) {
                                Ok(parsed) => #ident = Some(parsed),
                                Err(_) => {
                                    #ident = None;
                                    #warnings.push(#kv::KvWarning {
                                        variable: key.to_string(),
                                        value: value.to_string(),
                                        span: value_span,
                                    });
                                }
                            }
                        }
                    },
                    None => quote! {
                        #key_name => {
                            #ident = <#inner as FromKv>::from_kv(value, value_span).ok();
                        }
                    },
                }
            } else {
                let merge_expr = f.merge_expr(kv);
                quote! {
                    #key_name => {
                        #ident = Some(#merge_expr);
                    }
                }
            }
        })
        .collect()
}

/// Generates the fallback arm for unknown keys.
fn generate_unknown_handling(
    container_attrs: &ContainerAttrs,
    collect_field: Option<&ParsedField>,
    kv: &TokenStream2,
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
                    return Err(#kv::KvError::UnknownVariable {
                        variable: unknown.to_string(),
                        span: #kv::Span {
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
    // The warnings sink is a parse-time diagnostic, not part of the data
    // model, so it is excluded from the serde helper and defaulted on
    // deserialize.
    let helper_fields: Vec<&ParsedField> = fields
        .iter()
        .filter(|f| f.kind != FieldKind::Warnings)
        .collect();

    let field_defs: Vec<_> = helper_fields
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
                FieldKind::Warnings => quote! {},
            };

            quote! {
                #serde_attrs
                #ident: #ty
            }
        })
        .collect();

    /*
     * For serialization we build a helper of borrowed fields rather than
     * cloning the whole struct. Optional fields become `Option<&T>` (not
     * `&Option<T>`) so that `skip_serializing_if = "Option::is_none"` still
     * resolves against `Option`.
     */
    let ser_field_defs: Vec<_> = helper_fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            let key_name = &f.key_name;
            match f.kind {
                FieldKind::Required | FieldKind::Vec | FieldKind::MultiLine => {
                    let ty = &f.original_type;
                    quote! {
                        #[serde(rename = #key_name)]
                        #ident: &'a #ty
                    }
                }
                FieldKind::Optional
                | FieldKind::OptionVec
                | FieldKind::OptionMultiLine => {
                    let inner = extract_type_param(&f.original_type, "Option")
                        .expect("optional field always has an Option<...> type");
                    quote! {
                        #[serde(rename = #key_name, skip_serializing_if = "Option::is_none")]
                        #ident: Option<&'a #inner>
                    }
                }
                FieldKind::Collect => {
                    let ty = &f.original_type;
                    quote! {
                        #[serde(flatten)]
                        #ident: &'a #ty
                    }
                }
                /* Filtered out of `helper_fields` above. */
                FieldKind::Warnings => unreachable!(),
            }
        })
        .collect();

    let ser_to_fields: Vec<_> = helper_fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            match f.kind {
                FieldKind::Optional
                | FieldKind::OptionVec
                | FieldKind::OptionMultiLine => {
                    quote! { #ident: self.#ident.as_ref() }
                }
                _ => quote! { #ident: &self.#ident },
            }
        })
        .collect();

    /*
     * The lifetime is only valid if the helper actually borrows something;
     * a struct whose only field is the warnings sink has an empty helper.
     */
    let ser_lifetime = if helper_fields.is_empty() {
        quote! {}
    } else {
        quote! { <'a> }
    };

    let from_fields: Vec<_> = fields
        .iter()
        .map(|f| {
            let ident = &f.ident;
            if f.kind == FieldKind::Warnings {
                quote! { #ident: Default::default() }
            } else {
                quote! { #ident: helper.#ident }
            }
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
                struct Helper #ser_lifetime {
                    #(#ser_field_defs,)*
                }

                let helper = Helper {
                    #(#ser_to_fields,)*
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
                    #(#from_fields,)*
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
    /** Override for the path to the `pkgsrc-kv` crate. */
    crate_path: Option<Path>,
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
                } else if meta.path.is_ident("crate") {
                    let lit: syn::LitStr = meta.value()?.parse()?;
                    result.crate_path = Some(lit.parse()?);
                    Ok(())
                } else {
                    Err(meta.error(
                        "unknown container attribute; expected `allow_unknown` or `crate`",
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
    /// Whether an unparseable value becomes `None` instead of erroring.
    lenient: bool,
    /// Whether this field collects parse warnings from `lenient` fields.
    warnings: bool,
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
                } else if meta.path.is_ident("lenient") {
                    result.lenient = true;
                    Ok(())
                } else if meta.path.is_ident("warnings") {
                    result.warnings = true;
                    Ok(())
                } else {
                    Err(meta.error(
                        "unknown field attribute; expected `variable`, `multiline`, `collect`, `lenient`, or `warnings`",
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
    /// `Vec<KvWarning>` with `warnings` - collects `lenient` parse failures.
    Warnings,
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
    /// Whether an unparseable value becomes `None` instead of erroring.
    lenient: bool,
}

impl ParsedField {
    /// Analyzes a field and extracts parsing metadata.
    fn from_field(field: &Field) -> syn::Result<Self> {
        let ident = field.ident.clone().ok_or_else(|| {
            syn::Error::new_spanned(field, "expected named field")
        })?;

        let attrs = FieldAttrs::parse(&field.attrs)?;

        // `lenient` only applies to optional single-value fields.
        if attrs.lenient
            && (extract_type_param(&field.ty, "Option").is_none()
                || extract_option_vec_inner(&field.ty).is_some())
        {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "`lenient` attribute requires an `Option<T>` field",
            ));
        }

        // Validate collect field type
        if attrs.collect {
            validate_collect_type(&field.ty, field)?;
            return Ok(Self {
                ident,
                key_name: String::new(),
                kind: FieldKind::Collect,
                inner_type: field.ty.clone(),
                original_type: field.ty.clone(),
                lenient: false,
            });
        }

        // Validate warnings sink field type
        if attrs.warnings {
            validate_warnings_type(&field.ty, field)?;
            return Ok(Self {
                ident,
                key_name: String::new(),
                kind: FieldKind::Warnings,
                inner_type: field.ty.clone(),
                original_type: field.ty.clone(),
                lenient: false,
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
            lenient: attrs.lenient,
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
            FieldKind::Warnings => {
                let ty = &self.original_type;
                quote! { #ty }
            }
        }
    }

    /// Generates an expression to merge a new value into the accumulator.
    fn merge_expr(&self, kv: &TokenStream2) -> TokenStream2 {
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
                        for (word, word_span) in #kv::words_with_spans(value, value_offset) {
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
            FieldKind::Warnings => {
                // Handled separately in generate_match_arms
                quote! { unreachable!() }
            }
        }
    }

    /// Generates an expression to extract the final value from the accumulator.
    fn extract_expr(&self, kv: &TokenStream2) -> TokenStream2 {
        let ident = &self.ident;
        let key_name = &self.key_name;

        match self.kind {
            FieldKind::Required | FieldKind::Vec | FieldKind::MultiLine => {
                quote! {
                    #ident.ok_or_else(|| #kv::KvError::Incomplete(#key_name.to_string()))?
                }
            }
            FieldKind::Optional
            | FieldKind::OptionVec
            | FieldKind::OptionMultiLine
            | FieldKind::Collect
            | FieldKind::Warnings => {
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

/// Validates that a warnings sink field is a `Vec<KvWarning>`.
fn validate_warnings_type(ty: &Type, field: &Field) -> syn::Result<()> {
    let err = || {
        syn::Error::new_spanned(
            field,
            "`warnings` attribute requires a `Vec<KvWarning>` type",
        )
    };
    let Some(inner) = extract_type_param(ty, "Vec") else {
        return Err(err());
    };
    let Type::Path(type_path) = &inner else {
        return Err(err());
    };
    match type_path.path.segments.last() {
        Some(segment) if segment.ident == "KvWarning" => Ok(()),
        _ => Err(err()),
    }
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
