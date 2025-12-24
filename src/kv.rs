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

/*!
 * Type-safe `KEY=VALUE` parsing for various pkgsrc formats.
 *
 * This module provides the [`Kv`] derive macro and supporting types for
 * parsing various pkgsrc formats that use `KEY=VALUE` pairs, including:
 *
 * - [`pkg_summary(5)`] via [`Summary`]
 * - [`pbulk-index`] via [`ScanIndex`]
 *
 * Types such as [`PkgName`] only need to implement the [`FromKv`] trait to
 * be used directly.
 *
 * Multi-line variables such as `DESCRIPTION` in [`pkg_summary(5)`] are
 * supported by adding the `#[kv(multiline)]` attribute which will append each
 * line to a [`Vec`].
 *
 * Single-line variables where it makes sense to split the input such as
 * `CATEGORIES` can do so easily by declaring themselves as [`Vec`].
 *
 * # Example
 *
 * ```
 * use indoc::indoc;
 * use pkgsrc::{PkgName, kv::Kv};
 *
 * #[derive(Kv, Debug, PartialEq)]
 * #[kv(allow_unknown)]
 * struct Package {
 *     pkgname: PkgName,
 *     size_pkg: u64,
 *     categories: Vec<String>,
 *     #[kv(variable = "DESCRIPTION", multiline)]
 *     desc: Vec<String>,
 *     // There is no known multi-line variable that also contains multiple
 *     // values per line, this is purely to show how one might be handled if
 *     // necessary, though it would be strongly recommended against.
 *     #[kv(multiline)]
 *     all_depends: Vec<Vec<String>>,
 * }
 *
 * let input = indoc! {"
 *     PKGNAME=mktool-1.4.2
 *     SIZE_PKG=6999600
 *     CATEGORIES=devel pkgtools
 *     DESCRIPTION=This is a highly-performant collection of utilities that provide
 *     DESCRIPTION=alternate implementations for parts of the pkgsrc mk infrastructure.
 *     UNKNOWN=Without allow_unknown this would trigger parse failure.
 *     ALL_DEPENDS=cwrappers>=20150314:../../pkgtools/cwrappers
 *     ALL_DEPENDS=checkperms>=1.1:../../sysutils/checkperms rust>=1.74.0:../../lang/rust
 * "};
 *
 * let pkg = Package::parse(input)?;
 * assert_eq!(pkg.pkgname, "mktool-1.4.2");
 * assert_eq!(pkg.size_pkg, 6999600);
 * assert_eq!(pkg.categories, vec!["devel", "pkgtools"]);
 * assert!(pkg.desc[1].starts_with("alternate implementations "));
 * assert_eq!(pkg.all_depends.len(), 2);
 * assert_eq!(pkg.all_depends[0].len(), 1);
 * assert_eq!(pkg.all_depends[1].len(), 2);
 * # Ok::<(), pkgsrc::kv::Error>(())
 * ```
 *
 * [`PkgName`]: crate::PkgName
 * [`ScanIndex`]: crate::ScanIndex
 * [`Summary`]: crate::summary::Summary
 * [`pkg_summary(5)`]: https://man.netbsd.org/pkg_summary.5
 * [`pbulk-index`]: https://man.netbsd.org/pbulk-build.1
 */

use std::num::ParseIntError;
use std::path::PathBuf;
use thiserror::Error;

pub use pkgsrc_kv_derive::Kv;

/**
 * A byte offset and length in the input, for error reporting.
 *
 * `Span` tracks the location of errors within the original input string,
 * enabling precise error messages for diagnostic tools.
 *
 * ```
 * use pkgsrc::kv::Span;
 *
 * let span = Span { offset: 10, len: 5 };
 * let range: std::ops::Range<usize> = span.into();
 * assert_eq!(range, 10..15);
 * ```
 */
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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

/** Errors that can occur during parsing. */
#[derive(Debug, Error)]
pub enum Error {
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

impl Error {
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

/** A [`Result`](std::result::Result) type alias using [`enum@Error`]. */
pub type Result<T> = std::result::Result<T, Error>;

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
 * use pkgsrc::kv::{FromKv, Error, Span};
 *
 * struct MyId(u32);
 *
 * impl FromKv for MyId {
 *     fn from_kv(value: &str, span: Span) -> Result<Self, Error> {
 *         value.parse::<u32>()
 *             .map(MyId)
 *             .map_err(|e| Error::Parse {
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

// Implementation for String - always succeeds
impl FromKv for String {
    fn from_kv(value: &str, _span: Span) -> Result<Self> {
        Ok(value.to_string())
    }
}

// Implementation for numeric types
macro_rules! impl_fromkv_for_int {
    ($($t:ty),*) => {
        $(
            impl FromKv for $t {
                fn from_kv(value: &str, span: Span) -> Result<Self> {
                    value.parse().map_err(|source: ParseIntError| Error::ParseInt {
                        source,
                        span,
                    })
                }
            }
        )*
    };
}

impl_fromkv_for_int!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

// Implementation for PathBuf
impl FromKv for PathBuf {
    fn from_kv(value: &str, _span: Span) -> Result<Self> {
        Ok(Self::from(value))
    }
}

// Implementation for bool (common patterns: yes/no, true/false, 1/0)
impl FromKv for bool {
    fn from_kv(value: &str, span: Span) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "true" | "yes" | "1" => Ok(true),
            "false" | "no" | "0" => Ok(false),
            _ => Err(Error::Parse {
                message: format!("invalid boolean: {value}"),
                span,
            }),
        }
    }
}

impl<T: FromKv> FromKv for Vec<T> {
    fn from_kv(value: &str, span: Span) -> Result<Self> {
        value
            .split_whitespace()
            .map(|word| T::from_kv(word, span))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Depend, PkgName};
    use indoc::indoc;
    use std::collections::HashMap;

    // Standard mktool test data matching pkg_summary.gz
    const MKTOOL_INPUT: &str = indoc! {"
        PKGNAME=mktool-1.4.2
        COMMENT=High performance alternatives for pkgsrc/mk
        SIZE_PKG=6999600
        CATEGORIES=pkgtools
        HOMEPAGE=https://github.com/jperkin/mktool/
    "};

    #[test]
    fn span_to_range() {
        let span = Span { offset: 10, len: 5 };
        let range: std::ops::Range<usize> = span.into();
        assert_eq!(range, 10..15);
    }

    #[test]
    fn fromkv_string() {
        let span = Span::default();
        assert_eq!(String::from_kv("hello", span).unwrap(), "hello");
    }

    #[test]
    fn fromkv_u64() {
        let span = Span::default();
        assert_eq!(u64::from_kv("6999600", span).unwrap(), 6999600);
        assert!(u64::from_kv("not_a_number", span).is_err());
    }

    #[test]
    fn fromkv_bool() {
        let span = Span::default();
        assert!(bool::from_kv("true", span).unwrap());
        assert!(bool::from_kv("yes", span).unwrap());
        assert!(bool::from_kv("1", span).unwrap());
        assert!(!bool::from_kv("false", span).unwrap());
        assert!(!bool::from_kv("no", span).unwrap());
        assert!(!bool::from_kv("0", span).unwrap());
        assert!(bool::from_kv("maybe", span).is_err());
    }

    #[test]
    fn fromkv_pathbuf() {
        let span = Span::default();
        let path = PathBuf::from_kv("/usr/bin", span).unwrap();
        assert_eq!(path, PathBuf::from("/usr/bin"));
    }

    #[derive(Kv, Debug, PartialEq)]
    #[kv(allow_unknown)]
    struct SimplePackage {
        pkgname: String,
        #[kv(variable = "SIZE_PKG")]
        size: u64,
        comment: Option<String>,
    }

    #[test]
    fn derive_simple() {
        let pkg = SimplePackage::parse(MKTOOL_INPUT).unwrap();
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(pkg.size, 6999600);
        assert_eq!(
            pkg.comment,
            Some("High performance alternatives for pkgsrc/mk".to_string())
        );
    }

    #[test]
    fn derive_with_optional() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            SIZE_PKG=6999600
            COMMENT=High performance alternatives for pkgsrc/mk
        "};
        let pkg = SimplePackage::parse(input).unwrap();
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(pkg.size, 6999600);
        assert_eq!(
            pkg.comment,
            Some("High performance alternatives for pkgsrc/mk".to_string())
        );
    }

    #[test]
    fn derive_optional_missing() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            SIZE_PKG=6999600
        "};
        let pkg = SimplePackage::parse(input).unwrap();
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(pkg.size, 6999600);
        assert_eq!(pkg.comment, None);
    }

    #[test]
    fn derive_unknown_ignored() {
        let pkg = SimplePackage::parse(MKTOOL_INPUT).unwrap();
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
    }

    #[test]
    fn derive_missing_required() {
        let input = "PKGNAME=mktool-1.4.2\n";
        let result = SimplePackage::parse(input);
        assert!(matches!(result, Err(Error::Incomplete(_))));
    }

    #[derive(Kv, Debug, PartialEq)]
    struct VecPackage {
        pkgname: String,
        categories: Vec<String>,
    }

    #[test]
    fn derive_vec_whitespace_separated() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            CATEGORIES=pkgtools devel
        "};
        let pkg = VecPackage::parse(input).unwrap();
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(pkg.categories, vec!["pkgtools", "devel"]);
    }

    #[derive(Kv, Debug, PartialEq)]
    struct MultiLinePackage {
        pkgname: String,
        #[kv(multiline)]
        description: Vec<String>,
    }

    #[test]
    fn derive_multiline() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            DESCRIPTION=This is a highly-performant collection of utilities.
            DESCRIPTION=Many targets under pkgsrc/mk are implemented using shell.
        "};
        let pkg = MultiLinePackage::parse(input).unwrap();
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(pkg.description.len(), 2);
        assert_eq!(
            pkg.description[0],
            "This is a highly-performant collection of utilities."
        );
        assert_eq!(
            pkg.description[1],
            "Many targets under pkgsrc/mk are implemented using shell."
        );
    }

    #[test]
    fn derive_parse_error() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            SIZE_PKG=not_a_number
        "};
        let result = SimplePackage::parse(input);
        assert!(matches!(result, Err(Error::ParseInt { .. })));
    }

    #[test]
    fn derive_bad_line() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            bad-line
            SIZE_PKG=6999600
        "};
        let result = SimplePackage::parse(input);
        assert!(matches!(result, Err(Error::ParseLine(_))));
    }

    #[derive(Kv, Debug, PartialEq)]
    #[kv(allow_unknown)]
    struct ScanIndexTest {
        pkgname: PkgName,
        all_depends: Option<Vec<Depend>>,
    }

    #[test]
    fn derive_pkgname() {
        let input = "PKGNAME=mktool-1.4.2\n";
        let pkg = ScanIndexTest::parse(input).unwrap();
        assert_eq!(pkg.pkgname.pkgbase(), "mktool");
        assert_eq!(pkg.pkgname.pkgversion(), "1.4.2");
        assert_eq!(pkg.all_depends, None);
    }

    #[test]
    fn derive_depend_vec() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            ALL_DEPENDS=rust-[0-9]*:../../lang/rust curl>=7.0:../../www/curl
        "};
        let pkg = ScanIndexTest::parse(input).unwrap();
        assert_eq!(pkg.all_depends.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn derive_depend_invalid() {
        let input = indoc! {"
            PKGNAME=mktool-1.4.2
            ALL_DEPENDS=invalid
        "};
        let result = ScanIndexTest::parse(input);
        assert!(matches!(result, Err(Error::Parse { .. })));
    }

    #[derive(Kv, Debug, PartialEq)]
    struct WithExtras {
        pkgname: String,
        #[kv(collect)]
        extra: HashMap<String, String>,
    }

    #[test]
    fn derive_extras() {
        let pkg = WithExtras::parse(MKTOOL_INPUT).unwrap();
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert_eq!(
            pkg.extra.get("COMMENT"),
            Some(&"High performance alternatives for pkgsrc/mk".to_string())
        );
        assert_eq!(pkg.extra.get("SIZE_PKG"), Some(&"6999600".to_string()));
        assert_eq!(pkg.extra.get("CATEGORIES"), Some(&"pkgtools".to_string()));
        assert_eq!(
            pkg.extra.get("HOMEPAGE"),
            Some(&"https://github.com/jperkin/mktool/".to_string())
        );
        assert_eq!(pkg.extra.len(), 4);
    }

    #[test]
    fn derive_extras_empty() {
        let input = "PKGNAME=mktool-1.4.2\n";
        let pkg = WithExtras::parse(input).unwrap();
        assert_eq!(pkg.pkgname, "mktool-1.4.2");
        assert!(pkg.extra.is_empty());
    }
}
