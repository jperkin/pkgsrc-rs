/*
 * Copyright (c) 2026 Jonathan Perkin <jonathan@perkin.org.uk>
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

/*!
 * Type-safe `KEY=VALUE` parsing.
 *
 * This crate provides the runtime types for parsing `KEY=VALUE` formatted
 * input — [`Span`], [`KvError`], [`KvWarning`], and the [`FromKv`] extension
 * trait — together with the [`macro@Kv`] derive macro (enabled by the default
 * `derive` feature), which generates a `parse` method for a struct.
 *
 * Because the derive macro and the runtime it targets live in the same crate,
 * depending on `pkgsrc-kv` is all that is required to derive `Kv`: there is no
 * separate runtime crate to add.
 *
 * ```ignore
 * use pkgsrc_kv::Kv;
 *
 * #[derive(Kv)]
 * struct Package {
 *     pkgname: String,
 *     #[kv(variable = "SIZE_PKG")]
 *     size: u64,
 *     #[kv(multiline)]
 *     description: Vec<String>,
 *     homepage: Option<String>,
 * }
 *
 * let pkg = Package::parse("PKGNAME=foo-1.0\nSIZE_PKG=42\n")?;
 * # Ok::<(), pkgsrc_kv::KvError>(())
 * ```
 */

#![deny(missing_docs)]
#![deny(unsafe_code)]

use std::num::ParseIntError;
use std::path::PathBuf;
use thiserror::Error;

/**
 * Derive macro for parsing `KEY=VALUE` formatted input into a struct.
 *
 * Available when the default `derive` feature is enabled. See the
 * [crate-level documentation](crate) and the macro's own documentation for
 * usage.
 */
#[cfg(feature = "derive")]
pub use pkgsrc_kv_derive::Kv;

/**
 * A byte offset and length in the input, for error reporting.
 *
 * `Span` tracks the location of errors within the original input string,
 * enabling precise error messages for diagnostic tools.
 *
 * ```
 * use pkgsrc_kv::Span;
 *
 * let span = Span { offset: 10, len: 5 };
 * let range: std::ops::Range<usize> = span.into();
 * assert_eq!(range, 10..15);
 * ```
 */
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Span {
    /** Byte offset where this span starts. */
    pub offset: usize,
    /** Length in bytes. */
    pub len: usize,
}

impl From<Span> for std::ops::Range<usize> {
    fn from(span: Span) -> Self {
        span.offset..span.offset + span.len
    }
}

/**
 * A non-fatal problem encountered while parsing.
 *
 * Produced for a `#[kv(lenient)]` field whose value failed to parse, and
 * collected into a struct's `#[kv(warnings)]` field so that a caller can
 * report the bad input without the whole record failing.
 */
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct KvWarning {
    /** The variable (key) whose value could not be parsed. */
    pub variable: String,
    /** The raw value that failed to parse. */
    pub value: String,
    /** Location of the value within the input. */
    pub span: Span,
}

impl std::fmt::Display for KvWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid value {:?} for {}", self.value, self.variable)
    }
}

/** Errors that can occur during parsing. */
#[derive(Debug, Error)]
pub enum KvError {
    /** A line was not in `KEY=VALUE` format. */
    #[error("line is not in KEY=VALUE format")]
    ParseLine(Span),

    /** A required field was missing from the input. */
    #[error("missing required field '{0}'")]
    Incomplete(String),

    /** An unknown variable was encountered. */
    #[error("unknown variable '{variable}'")]
    UnknownVariable {
        /** The name of the unknown variable. */
        variable: String,
        /** Location of the variable name in the input. */
        span: Span,
    },

    /** Failed to parse an integer value. */
    #[error("failed to parse integer")]
    ParseInt {
        /** The underlying parse error. */
        #[source]
        source: ParseIntError,
        /** Location of the invalid value in the input. */
        span: Span,
    },

    /** Failed to parse a value. */
    #[error("{message}")]
    Parse {
        /** Description of the parse error. */
        message: String,
        /** Location of the invalid value in the input. */
        span: Span,
    },
}

impl KvError {
    /** Returns the [`Span`] for this error, if available. */
    #[must_use]
    pub const fn span(&self) -> Option<Span> {
        match self {
            Self::ParseLine(span)
            | Self::UnknownVariable { span, .. }
            | Self::ParseInt { span, .. }
            | Self::Parse { span, .. } => Some(*span),
            Self::Incomplete(_) => None,
        }
    }
}

/** A [`Result`](std::result::Result) type alias using [`KvError`]. */
pub type Result<T> = std::result::Result<T, KvError>;

/**
 * Trait for types that can be parsed from a KEY=VALUE string.
 *
 * This is the extension point for custom types. Implement this trait to
 * allow your type to be used in a `#[derive(Kv)]` struct.
 *
 * The `span` parameter indicates where in the input the value is located,
 * for error reporting.
 *
 * # Example
 *
 * ```
 * use pkgsrc_kv::{FromKv, KvError, Span};
 *
 * struct MyId(u32);
 *
 * impl FromKv for MyId {
 *     fn from_kv(value: &str, span: Span) -> Result<Self, KvError> {
 *         value.parse::<u32>()
 *             .map(MyId)
 *             .map_err(|e| KvError::Parse {
 *                 message: e.to_string(),
 *                 span,
 *             })
 *     }
 * }
 * ```
 */
pub trait FromKv: Sized {
    /**
     * Parse a value from a string.
     *
     * # Errors
     *
     * Returns an error if the value cannot be parsed into the target type.
     */
    fn from_kv(value: &str, span: Span) -> Result<Self>;
}

/* Implementation for String - always succeeds */
impl FromKv for String {
    fn from_kv(value: &str, _span: Span) -> Result<Self> {
        Ok(value.to_string())
    }
}

/* Implementation for numeric types */
macro_rules! impl_fromkv_for_int {
    ($($t:ty),*) => {
        $(
            impl FromKv for $t {
                fn from_kv(value: &str, span: Span) -> Result<Self> {
                    value.parse().map_err(|source: ParseIntError| KvError::ParseInt {
                        source,
                        span,
                    })
                }
            }
        )*
    };
}

impl_fromkv_for_int!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

/* Implementation for PathBuf */
impl FromKv for PathBuf {
    fn from_kv(value: &str, _span: Span) -> Result<Self> {
        Ok(Self::from(value))
    }
}

/* Implementation for bool (common patterns: yes/no, true/false, 1/0) */
impl FromKv for bool {
    fn from_kv(value: &str, span: Span) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(true),
            "false" | "no" | "0" => Ok(false),
            _ => Err(KvError::Parse {
                message: format!("invalid boolean: {value}"),
                span,
            }),
        }
    }
}

/**
 * Splits `value` on whitespace, yielding each word with its [`Span`] in the
 * original input. `base` is the byte offset of `value` within that input, so
 * each yielded span points at the word's true location rather than at the
 * whole value.
 *
 * This is an implementation detail shared by the [`Vec`] parser and the code
 * generated by the `Kv` derive macro; it is not part of the stable API.
 */
#[doc(hidden)]
pub fn words_with_spans(
    value: &str,
    base: usize,
) -> impl Iterator<Item = (&str, Span)> {
    let value_start = value.as_ptr() as usize;
    value.split_whitespace().map(move |word| {
        /*
         * Each word is a subslice of `value`, so the pointer difference is
         * its byte offset within `value`; add `base` for the absolute offset.
         */
        let offset = base + (word.as_ptr() as usize - value_start);
        let span = Span { offset, len: word.len() };
        (word, span)
    })
}

impl<T: FromKv> FromKv for Vec<T> {
    fn from_kv(value: &str, span: Span) -> Result<Self> {
        words_with_spans(value, span.offset)
            .map(|(word, word_span)| T::from_kv(word, word_span))
            .collect()
    }
}
