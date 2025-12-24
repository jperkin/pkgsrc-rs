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
 * Parsing and generation of [`pkg_summary(5)`] package metadata.
 *
 * A `pkg_summary` file contains a selection of useful package metadata, and is
 * primarily used by binary package managers to retrieve information about a
 * package repository.
 *
 * A complete package entry contains a list of `VARIABLE=VALUE` pairs,
 * and a complete `pkg_summary` file consists of multiple package entries
 * separated by single blank lines.
 *
 * A [`Summary`] can be created in two ways:
 *
 * * **Parsing**: Use [`Summary::from_reader`] to parse multiple entries from
 *   any [`BufRead`] source, returning an iterator over [`Result<Summary>`].
 *   Single entries can be parsed using [`FromStr`].
 *
 * * **Building**: Use [`SummaryBuilder::new`], add `VARIABLE=VALUE` lines with
 *   [`SummaryBuilder::vars`], then call [`SummaryBuilder::build`] to validate
 *   and construct the entry.
 *
 * Parsing operations return [`enum@Error`] on failure.  Each error variant
 * includes span information for use with pretty-printing error reporting
 * libraries such as [`ariadne`] or [`miette`] which can be helpful to show
 * exact locations of errors.
 *
 * Once validated, [`Summary`] provides many access [`methods`] to retrieve
 * information about each variable in a summary entry.
 *
 * ## Examples
 *
 * Read [`pkg_summary.gz`] and print list of packages in `pkg_info` format,
 * similar to how `pkgin avail` works.
 *
 * ```
 * use flate2::read::GzDecoder;
 * use pkgsrc::summary::Summary;
 * use std::fs::File;
 * use std::io::BufReader;
 *
 * # fn main() -> Result<(), Box<dyn std::error::Error>> {
 * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
 * let file = File::open(path).expect("failed to open pkg_summary.gz");
 * let reader = BufReader::new(GzDecoder::new(file));
 *
 * for pkg in Summary::from_reader(reader) {
 *     let pkg = pkg?;
 *     println!("{:20} {}", pkg.pkgname(), pkg.comment());
 * }
 * # Ok(())
 * # }
 * ```
 *
 * Create a [`Summary`] entry 4 different ways from an input file containing `pkg_summary`
 * data for `mktool-1.4.2` extracted from the main [`pkg_summary.gz`].
 *
 * ```
 * use pkgsrc::summary::{Summary, SummaryBuilder};
 * use std::str::FromStr;
 *
 * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/mktool.txt");
 * let input = std::fs::read_to_string(path).expect("failed to read mktool.txt");
 *
 * // Parse implicitly through FromStr's parse
 * assert_eq!(
 *     input.parse::<Summary>().expect("parse failed").pkgname(),
 *     "mktool-1.4.2"
 * );
 *
 * // Parse explicitly via from_str
 * assert_eq!(
 *     Summary::from_str(&input).expect("from_str failed").pkgname(),
 *     "mktool-1.4.2"
 * );
 *
 * // Use the builder pattern, passing all input through a single vars() call.
 * assert_eq!(
 *     SummaryBuilder::new()
 *         .vars(input.lines())
 *         .build()
 *         .expect("build failed")
 *         .pkgname(),
 *     "mktool-1.4.2"
 * );
 *
 * // Use the builder pattern but build up the input with separate var() calls.
 * let mut builder = SummaryBuilder::new();
 * for line in input.lines() {
 *     builder = builder.var(line);
 * }
 * assert_eq!(builder.build().expect("build failed").pkgname(), "mktool-1.4.2");
 * ```
 *
 * [`BufRead`]: std::io::BufRead
 * [`ariadne`]: https://docs.rs/ariadne
 * [`methods`]: Summary#implementations
 * [`miette`]: https://docs.rs/miette
 * [`pkg_summary(5)`]: https://man.netbsd.org/pkg_summary.5
 * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
 *
 */
use std::fmt;
use std::io::{self, BufRead};
use std::num::ParseIntError;
use std::str::FromStr;

use crate::kv::Kv;
use crate::PkgName;

pub use crate::kv::Span;

/// Error context containing optional entry number and span information.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ErrorContext {
    entry: Option<usize>,
    span: Option<Span>,
}

impl ErrorContext {
    /// Create a new error context with the given span.
    #[must_use]
    pub fn new(span: Span) -> Self {
        Self {
            entry: None,
            span: Some(span),
        }
    }

    /// Add entry number to this context.
    #[must_use]
    pub fn with_entry(mut self, entry: usize) -> Self {
        self.entry = Some(entry);
        self
    }

    /// Adjust the span offset by adding the given amount.
    #[must_use]
    pub fn adjust_offset(mut self, adjustment: usize) -> Self {
        if let Some(ref mut span) = self.span {
            span.offset += adjustment;
        }
        self
    }

    /// Set span if not already set.
    #[must_use]
    pub fn with_span_if_none(mut self, span: Span) -> Self {
        if self.span.is_none() {
            self.span = Some(span);
        }
        self
    }

    /// Return the entry number if set.
    #[must_use]
    pub const fn entry(&self) -> Option<usize> {
        self.entry
    }

    /// Return the span if set.
    #[must_use]
    pub const fn span(&self) -> Option<Span> {
        self.span
    }
}

#[cfg(test)]
use indoc::indoc;

/**
 * A type alias for the result from parsing a [`Summary`], with
 * [`enum@Error`] returned in [`Err`] variants.
 */
pub type Result<T> = std::result::Result<T, Error>;

/*
 * Note that (as far as my reading of it suggests) we cannot return an error
 * via fmt::Result if there are any issues with missing fields, so we can only
 * print what we have and validation will have to occur elsewhere.
 */
impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        macro_rules! write_required_field {
            ($field:expr, $name:expr) => {
                writeln!(f, "{}={}", $name, $field)?;
            };
        }

        macro_rules! write_optional_field {
            ($field:expr, $name:expr) => {
                if let Some(val) = &$field {
                    writeln!(f, "{}={}", $name, val)?;
                }
            };
        }

        macro_rules! write_required_array_field {
            ($field:expr, $name:expr) => {
                for val in $field {
                    writeln!(f, "{}={}", $name, val)?;
                }
            };
        }

        macro_rules! write_optional_array_field {
            ($field:expr, $name:expr) => {
                if let Some(arr) = &$field {
                    for val in arr {
                        writeln!(f, "{}={}", $name, val)?;
                    }
                }
            };
        }

        /* Retain compatible output ordering with pkg_info(1) */
        write_optional_array_field!(self.conflicts, "CONFLICTS");
        write_required_field!(self.pkgname.pkgname(), "PKGNAME");
        write_optional_array_field!(self.depends, "DEPENDS");
        write_required_field!(&self.comment, "COMMENT");
        write_required_field!(self.size_pkg, "SIZE_PKG");
        write_required_field!(&self.build_date, "BUILD_DATE");
        writeln!(f, "CATEGORIES={}", self.categories.join(" "))?;
        write_optional_field!(self.homepage, "HOMEPAGE");
        write_optional_field!(self.license, "LICENSE");
        write_required_field!(&self.machine_arch, "MACHINE_ARCH");
        write_required_field!(&self.opsys, "OPSYS");
        write_required_field!(&self.os_version, "OS_VERSION");
        write_required_field!(&self.pkgpath, "PKGPATH");
        write_required_field!(&self.pkgtools_version, "PKGTOOLS_VERSION");
        write_optional_field!(self.pkg_options, "PKG_OPTIONS");
        write_optional_field!(self.prev_pkgpath, "PREV_PKGPATH");
        write_optional_array_field!(self.provides, "PROVIDES");
        write_optional_array_field!(self.requires, "REQUIRES");
        write_optional_field!(self.file_name, "FILE_NAME");
        write_optional_field!(self.file_size, "FILE_SIZE");
        write_optional_field!(self.file_cksum, "FILE_CKSUM");
        write_optional_array_field!(self.supersedes, "SUPERSEDES");
        write_required_array_field!(&self.description, "DESCRIPTION");

        Ok(())
    }
}

/**
 * A single [`pkg_summary(5)`] entry.
 *
 * See the [module-level documentation](self) for usage examples.
 *
 * [`pkg_summary(5)`]: https://man.netbsd.org/pkg_summary.5
 */
#[derive(Clone, Debug, PartialEq, Eq, Kv)]
pub struct Summary {
    #[kv(variable = "BUILD_DATE")]
    build_date: String,

    #[kv(variable = "CATEGORIES")]
    categories: Vec<String>,

    #[kv(variable = "COMMENT")]
    comment: String,

    #[kv(variable = "CONFLICTS", multiline)]
    conflicts: Option<Vec<String>>,

    #[kv(variable = "DEPENDS", multiline)]
    depends: Option<Vec<String>>,

    #[kv(variable = "DESCRIPTION", multiline)]
    description: Vec<String>,

    #[kv(variable = "FILE_CKSUM")]
    file_cksum: Option<String>,

    #[kv(variable = "FILE_NAME")]
    file_name: Option<String>,

    #[kv(variable = "FILE_SIZE")]
    file_size: Option<u64>,

    #[kv(variable = "HOMEPAGE")]
    homepage: Option<String>,

    #[kv(variable = "LICENSE")]
    license: Option<String>,

    #[kv(variable = "MACHINE_ARCH")]
    machine_arch: String,

    #[kv(variable = "OPSYS")]
    opsys: String,

    #[kv(variable = "OS_VERSION")]
    os_version: String,

    #[kv(variable = "PKGNAME")]
    pkgname: PkgName,

    #[kv(variable = "PKGPATH")]
    pkgpath: String,

    #[kv(variable = "PKGTOOLS_VERSION")]
    pkgtools_version: String,

    #[kv(variable = "PKG_OPTIONS")]
    pkg_options: Option<String>,

    #[kv(variable = "PREV_PKGPATH")]
    prev_pkgpath: Option<String>,

    #[kv(variable = "PROVIDES", multiline)]
    provides: Option<Vec<String>>,

    #[kv(variable = "REQUIRES", multiline)]
    requires: Option<Vec<String>>,

    #[kv(variable = "SIZE_PKG")]
    size_pkg: u64,

    #[kv(variable = "SUPERSEDES", multiline)]
    supersedes: Option<Vec<String>>,
}

/**
 * Builder for constructing a [`Summary`] from `VARIABLE=VALUE` lines.
 *
 * ## Example
 *
 * ```
 * use pkgsrc::summary::SummaryBuilder;
 *
 * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/mktool.txt");
 * let input = std::fs::read_to_string(path).expect("failed to read mktool.txt");
 *
 * assert_eq!(
 *     SummaryBuilder::new()
 *         .vars(input.lines())
 *         .build()
 *         .expect("build failed")
 *         .pkgname(),
 *     "mktool-1.4.2"
 * );
 * ```
 */
#[derive(Clone, Debug, Default)]
pub struct SummaryBuilder {
    lines: Vec<String>,
    allow_unknown: bool,
    allow_incomplete: bool,
}

impl SummaryBuilder {
    /**
     * Create a new empty [`SummaryBuilder`].
     */
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /**
     * Add a single `VARIABLE=VALUE` line.
     *
     * This method is infallible; validation occurs when [`build`] is called.
     *
     * Prefer [`vars`] when adding multiple variables.
     *
     * [`vars`]: SummaryBuilder::vars
     * [`build`]: SummaryBuilder::build
     */
    #[must_use]
    pub fn var(mut self, line: impl AsRef<str>) -> Self {
        self.lines.push(line.as_ref().to_string());
        self
    }

    /**
     * Add `VARIABLE=VALUE` lines.
     *
     * This method is infallible; validation occurs when [`build`] is called.
     *
     * ## Example
     *
     * ```
     * use pkgsrc::summary::SummaryBuilder;
     *
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/mktool.txt");
     * let input = std::fs::read_to_string(path).expect("failed to read mktool.txt");
     *
     * assert_eq!(
     *     SummaryBuilder::new()
     *         .vars(input.lines())
     *         .build()
     *         .expect("build failed")
     *         .pkgname(),
     *     "mktool-1.4.2"
     * );
     * ```
     *
     * [`build`]: SummaryBuilder::build
     */
    #[must_use]
    pub fn vars<I, S>(mut self, lines: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for line in lines {
            self.lines.push(line.as_ref().to_string());
        }
        self
    }

    /// Allow unknown variables instead of returning an error.
    #[must_use]
    pub fn allow_unknown(mut self, yes: bool) -> Self {
        self.allow_unknown = yes;
        self
    }

    /// Allow incomplete entries missing required fields.
    #[must_use]
    pub fn allow_incomplete(mut self, yes: bool) -> Self {
        self.allow_incomplete = yes;
        self
    }

    /**
     * Validate and finalize the [`Summary`].
     *
     * Parses all added variables, validates that all required fields are
     * present, and returns the completed [`Summary`].
     *
     * ## Errors
     *
     * Returns [`Error`] if the input is invalid.  Applications may want to
     * ignore [`Error::UnknownVariable`] if they wish to be future-proof
     * against potential new additions to the `pkg_summary` format.
     *
     * ## Examples
     *
     * ```
     * use pkgsrc::summary::{Error, SummaryBuilder};
     *
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/mktool.txt");
     * let input = std::fs::read_to_string(path).expect("failed to read mktool.txt");
     *
     * // Valid pkg_summary data.
     * assert_eq!(
     *     SummaryBuilder::new()
     *         .vars(input.lines())
     *         .build()
     *         .expect("build failed")
     *         .pkgname(),
     *     "mktool-1.4.2"
     * );
     *
     * // Missing required fields.
     * assert!(matches!(
     *     SummaryBuilder::new()
     *         .vars(["PKGNAME=testpkg-1.0", "COMMENT=Test"])
     *         .build(),
     *     Err(Error::Incomplete { .. })
     * ));
     *
     * // Contains a line not in VARIABLE=VALUE format.
     * assert!(matches!(
     *     SummaryBuilder::new()
     *         .vars(["not a valid line"])
     *         .build(),
     *     Err(Error::ParseLine { .. })
     * ));
     *
     * // Unknown variable name.
     * assert!(matches!(
     *     SummaryBuilder::new()
     *         .vars(["UNKNOWN=value"])
     *         .build(),
     *     Err(Error::UnknownVariable { .. })
     * ));
     *
     * // Invalid integer value (with all other required fields present).
     * assert!(matches!(
     *     SummaryBuilder::new()
     *         .vars([
     *             "BUILD_DATE=2019-08-12",
     *             "CATEGORIES=devel",
     *             "COMMENT=test",
     *             "DESCRIPTION=test",
     *             "MACHINE_ARCH=x86_64",
     *             "OPSYS=NetBSD",
     *             "OS_VERSION=9.0",
     *             "PKGNAME=test-1.0",
     *             "PKGPATH=devel/test",
     *             "PKGTOOLS_VERSION=20091115",
     *             "SIZE_PKG=not_a_number",
     *         ])
     *         .build(),
     *     Err(Error::ParseInt { .. })
     * ));
     * ```
     */
    pub fn build(self) -> Result<Summary> {
        let input = self.lines.join("\n");
        parse_summary(&input, self.allow_unknown, self.allow_incomplete)
    }
}

impl Summary {
    /**
     * Create an iterator that parses Summary entries from a reader.
     *
     * ## Example
     *
     * ```no_run
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * let file = File::open("pkg_summary.txt").unwrap();
     * let reader = BufReader::new(file);
     *
     * for result in Summary::from_reader(reader) {
     *     match result {
     *         Ok(summary) => println!("{}", summary.pkgname()),
     *         Err(e) => eprintln!("Error: {}", e),
     *     }
     * }
     * ```
     */
    pub fn from_reader<R: BufRead>(reader: R) -> SummaryIter<R> {
        SummaryIter {
            reader,
            line_buf: String::new(),
            buffer: String::new(),
            record_number: 0,
            byte_offset: 0,
            entry_start: 0,
            allow_unknown: false,
            allow_incomplete: false,
        }
    }

    /**
     * Returns the `BUILD_DATE` value.  This is a required field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return the `BUILD_DATE` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .build_date(),
     *     "2025-11-17 22:03:08 +0000"
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn build_date(&self) -> &str {
        &self.build_date
    }

    /**
     * Returns a [`Vec`] containing the `CATEGORIES` values.  This is a
     * required field.
     *
     * Note that the `CATEGORIES` field is a space-delimited string, but it
     * makes more sense for this API to return the values as a [`Vec`].
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `CATEGORIES` for `mktool` (single
     * category) and `9e` (multiple categories).
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .categories(),
     *     ["pkgtools"]
     * );
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "9e-1.0")
     *         .expect("9e not found")
     *         .categories(),
     *     ["archivers", "plan9"]
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn categories(&self) -> &[String] {
        &self.categories
    }

    /**
     * Returns the `COMMENT` value.  This is a required field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return the `COMMENT` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .comment(),
     *     "High performance alternatives for pkgsrc/mk"
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn comment(&self) -> &str {
        &self.comment
    }

    /**
     * Returns a [`Vec`] containing optional `CONFLICTS` values, or [`None`]
     * if there are none.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `CONFLICTS` for `mktool` (none) and
     * `angband` (multiple).
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .conflicts(),
     *     None
     * );
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "angband-4.2.5nb1")
     *         .expect("angband not found")
     *         .conflicts(),
     *     Some(["angband-tty-[0-9]*", "angband-sdl-[0-9]*", "angband-x11-[0-9]*"]
     *         .map(String::from).as_slice())
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn conflicts(&self) -> Option<&[String]> {
        self.conflicts.as_deref()
    }

    /**
     * Returns a [`Vec`] containing optional `DEPENDS` values, or [`None`]
     * if there are none.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `DEPENDS` for `mktool` (none) and
     * `R-RcppTOML` (multiple).
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .depends(),
     *     None
     * );
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "R-RcppTOML-0.2.2")
     *         .expect("R-RcppTOML not found")
     *         .depends(),
     *     Some(["R>=4.2.0nb1", "R-Rcpp>=1.0.2"].map(String::from).as_slice())
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn depends(&self) -> Option<&[String]> {
        self.depends.as_deref()
    }

    /**
     * Returns a [`Vec`] containing `DESCRIPTION` values.  This is a required
     * field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `DESCRIPTION` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * // mktool's description has 20 lines
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .description()
     *         .len(),
     *     20
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn description(&self) -> &[String] {
        self.description.as_slice()
    }

    /**
     * Returns the `FILE_CKSUM` value if set.  This is an optional field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `FILE_CKSUM` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .file_cksum(),
     *     None
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn file_cksum(&self) -> Option<&str> {
        self.file_cksum.as_deref()
    }

    /**
     * Returns the `FILE_NAME` value if set.  This is an optional field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `FILE_NAME` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .file_name(),
     *     Some("mktool-1.4.2.tgz")
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn file_name(&self) -> Option<&str> {
        self.file_name.as_deref()
    }

    /**
     * Returns the `FILE_SIZE` value if set.  This is an optional field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `FILE_SIZE` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .file_size(),
     *     Some(2871260)
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn file_size(&self) -> Option<u64> {
        self.file_size
    }

    /**
     * Returns the `HOMEPAGE` value if set.  This is an optional field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `HOMEPAGE` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .homepage(),
     *     Some("https://github.com/jperkin/mktool/")
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn homepage(&self) -> Option<&str> {
        self.homepage.as_deref()
    }

    /**
     * Returns the `LICENSE` value if set.  This is an optional field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `LICENSE` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .license(),
     *     Some("isc")
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn license(&self) -> Option<&str> {
        self.license.as_deref()
    }

    /**
     * Returns the `MACHINE_ARCH` value.  This is a required field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `MACHINE_ARCH` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .machine_arch(),
     *     "aarch64"
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn machine_arch(&self) -> &str {
        &self.machine_arch
    }

    /**
     * Returns the `OPSYS` value.  This is a required field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `OPSYS` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .opsys(),
     *     "Darwin"
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn opsys(&self) -> &str {
        &self.opsys
    }

    /**
     * Returns the `OS_VERSION` value.  This is a required field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `OS_VERSION` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .os_version(),
     *     "23.6.0"
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn os_version(&self) -> &str {
        &self.os_version
    }

    /**
     * Returns the `PKG_OPTIONS` value if set.  This is an optional field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `PKG_OPTIONS` for `mktool` (none)
     * and `freeglut` (some).
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .pkg_options(),
     *     None
     * );
     *
     * // freeglut has PKG_OPTIONS.
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "freeglut-3.6.0")
     *         .expect("freeglut not found")
     *         .pkg_options(),
     *     Some("x11")
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn pkg_options(&self) -> Option<&str> {
        self.pkg_options.as_deref()
    }

    /**
     * Returns the `PKGNAME` value.  This is a required field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `PKGNAME` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * // Find the mktool package
     * let mktool = pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2");
     * assert!(mktool.is_some());
     *
     * // Using the PkgName API we can also access just the base or version
     * assert_eq!(mktool.unwrap().pkgname().pkgbase(), "mktool");
     * assert_eq!(mktool.unwrap().pkgname().pkgversion(), "1.4.2");
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn pkgname(&self) -> &PkgName {
        &self.pkgname
    }

    /**
     * Returns the `PKGPATH` value.  This is a required field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `PKGPATH` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .pkgpath(),
     *     "pkgtools/mktool"
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn pkgpath(&self) -> &str {
        &self.pkgpath
    }

    /**
     * Returns the `PKGTOOLS_VERSION` value.  This is a required field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `PKGTOOLS_VERSION` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .pkgtools_version(),
     *     "20091115"
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn pkgtools_version(&self) -> &str {
        &self.pkgtools_version
    }

    /**
     * Returns the `PREV_PKGPATH` value if set.  This is an optional field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `PREV_PKGPATH` for `mktool` (none)
     * and `ansible` (some).
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .prev_pkgpath(),
     *     None
     * );
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "ansible-12.2.0")
     *         .expect("ansible not found")
     *         .prev_pkgpath(),
     *     Some("sysutils/ansible2")
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn prev_pkgpath(&self) -> Option<&str> {
        self.prev_pkgpath.as_deref()
    }

    /**
     * Returns a [`Vec`] containing optional `PROVIDES` values, or [`None`] if
     * there are none.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `PROVIDES` for `mktool` (none) and
     * `CUnit` (multiple).
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .provides(),
     *     None
     * );
     *
     * // CUnit provides 2 shared libraries
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "CUnit-2.1.3nb1")
     *         .expect("CUnit not found")
     *         .provides()
     *         .map(|v| v.len()),
     *     Some(2)
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn provides(&self) -> Option<&[String]> {
        self.provides.as_deref()
    }

    /**
     * Returns a [`Vec`] containing optional `REQUIRES` values, or [`None`] if
     * there are none.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `REQUIRES` for `mktool` (none) and
     * `SDL_image` (multiple).
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .requires(),
     *     None
     * );
     *
     * // SDL_image has 3 REQUIRES entries
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "SDL_image-1.2.12nb16")
     *         .expect("SDL_image not found")
     *         .requires()
     *         .map(|v| v.len()),
     *     Some(3)
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn requires(&self) -> Option<&[String]> {
        self.requires.as_deref()
    }

    /**
     * Returns the `SIZE_PKG` value.  This is a required field.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `SIZE_PKG` for `mktool`.
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .size_pkg(),
     *     6999600
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn size_pkg(&self) -> u64 {
        self.size_pkg
    }

    /**
     * Returns a [`Vec`] containing optional `SUPERSEDES` values, or [`None`]
     * if there are none.
     *
     * ## Example
     *
     * Parse [`pkg_summary.gz`] and return `SUPERSEDES` for `mktool` (none) and
     * `at-spi2-core` (multiple).
     *
     * [`pkg_summary.gz`]: https://github.com/jperkin/pkgsrc-rs/blob/master/tests/data/summary/pkg_summary.gz
     *
     * ```
     * use flate2::read::GzDecoder;
     * use pkgsrc::summary::Summary;
     * use std::fs::File;
     * use std::io::BufReader;
     *
     * # fn main() -> std::io::Result<()> {
     * let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/summary/pkg_summary.gz");
     * let file = File::open(path)?;
     * let decoder = GzDecoder::new(file);
     * let reader = BufReader::new(decoder);
     *
     * let pkgs: Vec<_> = Summary::from_reader(reader)
     *     .filter_map(Result::ok)
     *     .collect();
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "mktool-1.4.2")
     *         .expect("mktool not found")
     *         .supersedes(),
     *     None
     * );
     *
     * assert_eq!(
     *     pkgs.iter().find(|p| p.pkgname() == "at-spi2-core-2.58.1")
     *         .expect("at-spi2-core not found")
     *         .supersedes(),
     *     Some(["at-spi2-atk-[0-9]*", "atk-[0-9]*"].map(String::from).as_slice())
     * );
     *
     * # Ok(())
     * # }
     * ```
     */
    pub fn supersedes(&self) -> Option<&[String]> {
        self.supersedes.as_deref()
    }
}

impl FromStr for Summary {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Summary::parse(s).map_err(Error::from)
    }
}

fn parse_summary(
    s: &str,
    allow_unknown: bool,
    allow_incomplete: bool,
) -> Result<Summary> {
    // For allow_unknown/allow_incomplete, we need to wrap the parsing
    if allow_unknown || allow_incomplete {
        parse_summary_lenient(s, allow_unknown, allow_incomplete)
    } else {
        Summary::parse(s).map_err(Error::from)
    }
}

fn parse_summary_lenient(
    s: &str,
    allow_unknown: bool,
    allow_incomplete: bool,
) -> Result<Summary> {
    use crate::kv::FromKv;

    // State for each field
    let mut build_date: Option<String> = None;
    let mut categories: Option<Vec<String>> = None;
    let mut comment: Option<String> = None;
    let mut conflicts: Option<Vec<String>> = None;
    let mut depends: Option<Vec<String>> = None;
    let mut description: Option<Vec<String>> = None;
    let mut file_cksum: Option<String> = None;
    let mut file_name: Option<String> = None;
    let mut file_size: Option<u64> = None;
    let mut homepage: Option<String> = None;
    let mut license: Option<String> = None;
    let mut machine_arch: Option<String> = None;
    let mut opsys: Option<String> = None;
    let mut os_version: Option<String> = None;
    let mut pkgname: Option<PkgName> = None;
    let mut pkgpath: Option<String> = None;
    let mut pkgtools_version: Option<String> = None;
    let mut pkg_options: Option<String> = None;
    let mut prev_pkgpath: Option<String> = None;
    let mut provides: Option<Vec<String>> = None;
    let mut requires: Option<Vec<String>> = None;
    let mut size_pkg: Option<u64> = None;
    let mut supersedes: Option<Vec<String>> = None;

    for line in s.lines() {
        if line.is_empty() {
            continue;
        }

        let line_offset = line.as_ptr() as usize - s.as_ptr() as usize;

        let (key, value) =
            line.split_once('=').ok_or_else(|| Error::ParseLine {
                context: ErrorContext::new(Span {
                    offset: line_offset,
                    len: line.len(),
                }),
            })?;

        let value_offset = line_offset + key.len() + 1;
        let value_span = Span {
            offset: value_offset,
            len: value.len(),
        };

        match key {
            "BUILD_DATE" => {
                build_date = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "CATEGORIES" => {
                let items: Vec<String> =
                    value.split_whitespace().map(String::from).collect();
                categories = Some(items);
            }
            "COMMENT" => {
                comment = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "CONFLICTS" => {
                let mut vec = conflicts.unwrap_or_default();
                vec.push(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
                conflicts = Some(vec);
            }
            "DEPENDS" => {
                let mut vec = depends.unwrap_or_default();
                vec.push(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
                depends = Some(vec);
            }
            "DESCRIPTION" => {
                let mut vec = description.unwrap_or_default();
                vec.push(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
                description = Some(vec);
            }
            "FILE_CKSUM" => {
                file_cksum = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "FILE_NAME" => {
                file_name = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "FILE_SIZE" => {
                file_size = Some(
                    <u64 as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "HOMEPAGE" => {
                homepage = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "LICENSE" => {
                license = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "MACHINE_ARCH" => {
                machine_arch = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "OPSYS" => {
                opsys = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "OS_VERSION" => {
                os_version = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "PKGNAME" => {
                pkgname = Some(
                    <PkgName as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "PKGPATH" => {
                pkgpath = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "PKGTOOLS_VERSION" => {
                pkgtools_version = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "PKG_OPTIONS" => {
                pkg_options = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "PREV_PKGPATH" => {
                prev_pkgpath = Some(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "PROVIDES" => {
                let mut vec = provides.unwrap_or_default();
                vec.push(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
                provides = Some(vec);
            }
            "REQUIRES" => {
                let mut vec = requires.unwrap_or_default();
                vec.push(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
                requires = Some(vec);
            }
            "SIZE_PKG" => {
                size_pkg = Some(
                    <u64 as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
            }
            "SUPERSEDES" => {
                let mut vec = supersedes.unwrap_or_default();
                vec.push(
                    <String as FromKv>::from_kv(value, value_span)
                        .map_err(kv_to_summary_error)?,
                );
                supersedes = Some(vec);
            }
            unknown => {
                if !allow_unknown {
                    return Err(Error::UnknownVariable {
                        variable: unknown.to_string(),
                        context: ErrorContext::new(Span {
                            offset: line_offset,
                            len: key.len(),
                        }),
                    });
                }
            }
        }
    }

    // Extract values, using defaults for missing required fields if allow_incomplete
    let build_date = if allow_incomplete {
        build_date.unwrap_or_default()
    } else {
        build_date.ok_or_else(|| Error::Incomplete {
            field: "BUILD_DATE".to_string(),
            context: ErrorContext::default(),
        })?
    };

    let categories = if allow_incomplete {
        categories.unwrap_or_default()
    } else {
        categories.ok_or_else(|| Error::Incomplete {
            field: "CATEGORIES".to_string(),
            context: ErrorContext::default(),
        })?
    };

    let comment = if allow_incomplete {
        comment.unwrap_or_default()
    } else {
        comment.ok_or_else(|| Error::Incomplete {
            field: "COMMENT".to_string(),
            context: ErrorContext::default(),
        })?
    };

    let description = if allow_incomplete {
        description.unwrap_or_default()
    } else {
        description.ok_or_else(|| Error::Incomplete {
            field: "DESCRIPTION".to_string(),
            context: ErrorContext::default(),
        })?
    };

    let machine_arch = if allow_incomplete {
        machine_arch.unwrap_or_default()
    } else {
        machine_arch.ok_or_else(|| Error::Incomplete {
            field: "MACHINE_ARCH".to_string(),
            context: ErrorContext::default(),
        })?
    };

    let opsys = if allow_incomplete {
        opsys.unwrap_or_default()
    } else {
        opsys.ok_or_else(|| Error::Incomplete {
            field: "OPSYS".to_string(),
            context: ErrorContext::default(),
        })?
    };

    let os_version = if allow_incomplete {
        os_version.unwrap_or_default()
    } else {
        os_version.ok_or_else(|| Error::Incomplete {
            field: "OS_VERSION".to_string(),
            context: ErrorContext::default(),
        })?
    };

    let pkgname = if allow_incomplete {
        pkgname.unwrap_or_else(|| PkgName::new("unknown-0"))
    } else {
        pkgname.ok_or_else(|| Error::Incomplete {
            field: "PKGNAME".to_string(),
            context: ErrorContext::default(),
        })?
    };

    let pkgpath = if allow_incomplete {
        pkgpath.unwrap_or_default()
    } else {
        pkgpath.ok_or_else(|| Error::Incomplete {
            field: "PKGPATH".to_string(),
            context: ErrorContext::default(),
        })?
    };

    let pkgtools_version = if allow_incomplete {
        pkgtools_version.unwrap_or_default()
    } else {
        pkgtools_version.ok_or_else(|| Error::Incomplete {
            field: "PKGTOOLS_VERSION".to_string(),
            context: ErrorContext::default(),
        })?
    };

    let size_pkg = if allow_incomplete {
        size_pkg.unwrap_or(0)
    } else {
        size_pkg.ok_or_else(|| Error::Incomplete {
            field: "SIZE_PKG".to_string(),
            context: ErrorContext::default(),
        })?
    };

    Ok(Summary {
        build_date,
        categories,
        comment,
        conflicts,
        depends,
        description,
        file_cksum,
        file_name,
        file_size,
        homepage,
        license,
        machine_arch,
        opsys,
        os_version,
        pkgname,
        pkgpath,
        pkgtools_version,
        pkg_options,
        prev_pkgpath,
        provides,
        requires,
        size_pkg,
        supersedes,
    })
}

fn kv_to_summary_error(e: crate::kv::Error) -> Error {
    Error::from(e)
}

/**
 * Iterator that parses Summary entries from a [`BufRead`] source.
 *
 * Created by [`Summary::from_reader`].
 */
pub struct SummaryIter<R: BufRead> {
    reader: R,
    line_buf: String,
    buffer: String,
    record_number: usize,
    byte_offset: usize,
    entry_start: usize,
    allow_unknown: bool,
    allow_incomplete: bool,
}

impl<R: BufRead> Iterator for SummaryIter<R> {
    type Item = Result<Summary>;

    fn next(&mut self) -> Option<Self::Item> {
        self.buffer.clear();
        self.entry_start = self.byte_offset;

        loop {
            self.line_buf.clear();
            match self.reader.read_line(&mut self.line_buf) {
                Ok(0) => {
                    return if self.buffer.is_empty() {
                        None
                    } else {
                        let entry = self.record_number;
                        let entry_start = self.entry_start;
                        let entry_len = self.buffer.len();
                        self.record_number += 1;
                        Some(
                            parse_summary(
                                &self.buffer,
                                self.allow_unknown,
                                self.allow_incomplete,
                            )
                            .map_err(|e: Error| {
                                e.with_entry_span(Span {
                                    offset: 0,
                                    len: entry_len,
                                })
                                .with_entry(entry)
                                .adjust_offset(entry_start)
                            }),
                        )
                    };
                }
                Ok(line_bytes) => {
                    let is_blank =
                        self.line_buf.trim_end_matches(['\r', '\n']).is_empty();
                    if is_blank {
                        self.byte_offset += line_bytes;
                        if !self.buffer.is_empty() {
                            let entry = self.record_number;
                            let entry_start = self.entry_start;
                            // Trim trailing newline for parsing (doesn't affect offsets
                            // since FromStr handles any line ending style)
                            let to_parse =
                                self.buffer.trim_end_matches(['\r', '\n']);
                            let entry_len = to_parse.len();
                            self.record_number += 1;
                            self.entry_start = self.byte_offset;
                            return Some(
                                parse_summary(
                                    to_parse,
                                    self.allow_unknown,
                                    self.allow_incomplete,
                                )
                                .map_err(
                                    |e: Error| {
                                        e.with_entry_span(Span {
                                            offset: 0,
                                            len: entry_len,
                                        })
                                        .with_entry(entry)
                                        .adjust_offset(entry_start)
                                    },
                                ),
                            );
                        }
                    } else {
                        self.buffer.push_str(&self.line_buf);
                        self.byte_offset += line_bytes;
                    }
                }
                Err(e) => return Some(Err(Error::Io(e))),
            }
        }
    }
}

impl<R: BufRead> SummaryIter<R> {
    /// Allow unknown variables instead of returning an error.
    #[must_use]
    pub fn allow_unknown(mut self, yes: bool) -> Self {
        self.allow_unknown = yes;
        self
    }

    /// Allow incomplete entries missing required fields.
    #[must_use]
    pub fn allow_incomplete(mut self, yes: bool) -> Self {
        self.allow_incomplete = yes;
        self
    }
}

/**
 * Error type for [`pkg_summary(5)`] parsing operations.
 *
 * Each error variant includes an [`ErrorContext`] with span information that
 * can be used with error reporting libraries like [`ariadne`] or [`miette`].
 *
 * [`ariadne`]: https://docs.rs/ariadne
 * [`miette`]: https://docs.rs/miette
 * [`pkg_summary(5)`]: https://man.netbsd.org/pkg_summary.5
 */
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The summary is incomplete due to a missing required field.
    #[error("missing required field '{field}'")]
    Incomplete {
        /// The name of the missing field.
        field: String,
        /// Location context for this error.
        context: ErrorContext,
    },

    /// An underlying I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),

    /// The supplied line is not in the correct `VARIABLE=VALUE` format.
    #[error("line is not in VARIABLE=VALUE format")]
    ParseLine {
        /// Location context for this error.
        context: ErrorContext,
    },

    /// The supplied variable is not a valid [`pkg_summary(5)`] variable.
    ///
    /// [`pkg_summary(5)`]: https://man.netbsd.org/pkg_summary.5
    #[error("'{variable}' is not a valid pkg_summary variable")]
    UnknownVariable {
        /// The unknown variable name.
        variable: String,
        /// Location context for this error.
        context: ErrorContext,
    },

    /// Parsing a supplied value as an Integer type failed.
    #[error("failed to parse integer")]
    ParseInt {
        /// The underlying parse error.
        #[source]
        source: ParseIntError,
        /// Location context for this error.
        context: ErrorContext,
    },

    /// A duplicate value was found for a single-value field.
    #[error("duplicate value for '{variable}'")]
    Duplicate {
        /// The name of the duplicated variable.
        variable: String,
        /// Location context for this error.
        context: ErrorContext,
    },

    /// A generic parse error from the kv module.
    #[error("{message}")]
    Parse {
        /// The error message.
        message: String,
        /// Location context for this error.
        context: ErrorContext,
    },
}

impl From<crate::kv::Error> for Error {
    fn from(e: crate::kv::Error) -> Self {
        match e {
            crate::kv::Error::ParseLine(span) => Self::ParseLine {
                context: ErrorContext::new(span),
            },
            crate::kv::Error::Incomplete(field) => Self::Incomplete {
                field,
                context: ErrorContext::default(),
            },
            crate::kv::Error::UnknownVariable { variable, span } => {
                Self::UnknownVariable {
                    variable,
                    context: ErrorContext::new(span),
                }
            }
            crate::kv::Error::ParseInt { source, span } => Self::ParseInt {
                source,
                context: ErrorContext::new(span),
            },
            crate::kv::Error::Parse { message, span } => Self::Parse {
                message,
                context: ErrorContext::new(span),
            },
        }
    }
}

impl Error {
    /**
     * Returns the entry index where the error occurred.
     *
     * Only set when parsing multiple entries via [`Summary::from_reader`].
     */
    pub fn entry(&self) -> Option<usize> {
        match self {
            Self::Incomplete { context, .. }
            | Self::ParseLine { context, .. }
            | Self::UnknownVariable { context, .. }
            | Self::ParseInt { context, .. }
            | Self::Duplicate { context, .. }
            | Self::Parse { context, .. } => context.entry(),
            Self::Io(_) => None,
        }
    }

    /**
     * Returns the span information for this error.
     *
     * The span contains the byte offset and length of the problematic region.
     */
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::Incomplete { context, .. }
            | Self::ParseLine { context, .. }
            | Self::UnknownVariable { context, .. }
            | Self::ParseInt { context, .. }
            | Self::Duplicate { context, .. }
            | Self::Parse { context, .. } => context.span(),
            Self::Io(_) => None,
        }
    }

    fn with_entry(self, entry: usize) -> Self {
        match self {
            Self::Incomplete { field, context } => Self::Incomplete {
                field,
                context: context.with_entry(entry),
            },
            Self::ParseLine { context } => Self::ParseLine {
                context: context.with_entry(entry),
            },
            Self::UnknownVariable { variable, context } => {
                Self::UnknownVariable {
                    variable,
                    context: context.with_entry(entry),
                }
            }
            Self::ParseInt { source, context } => Self::ParseInt {
                source,
                context: context.with_entry(entry),
            },
            Self::Duplicate { variable, context } => Self::Duplicate {
                variable,
                context: context.with_entry(entry),
            },
            Self::Parse { message, context } => Self::Parse {
                message,
                context: context.with_entry(entry),
            },
            Self::Io(e) => Self::Io(e),
        }
    }

    fn adjust_offset(self, base: usize) -> Self {
        match self {
            Self::Incomplete { field, context } => Self::Incomplete {
                field,
                context: context.adjust_offset(base),
            },
            Self::ParseLine { context } => Self::ParseLine {
                context: context.adjust_offset(base),
            },
            Self::UnknownVariable { variable, context } => {
                Self::UnknownVariable {
                    variable,
                    context: context.adjust_offset(base),
                }
            }
            Self::ParseInt { source, context } => Self::ParseInt {
                source,
                context: context.adjust_offset(base),
            },
            Self::Duplicate { variable, context } => Self::Duplicate {
                variable,
                context: context.adjust_offset(base),
            },
            Self::Parse { message, context } => Self::Parse {
                message,
                context: context.adjust_offset(base),
            },
            Self::Io(e) => Self::Io(e),
        }
    }

    fn with_entry_span(self, span: Span) -> Self {
        match self {
            Self::Incomplete { field, context } => Self::Incomplete {
                field,
                context: context.with_span_if_none(span),
            },
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_err() {
        let err = Summary::from_str("BUILD_DATE").unwrap_err();
        assert!(matches!(err, Error::ParseLine { .. }));

        let err = Summary::from_str("BILD_DATE=").unwrap_err();
        assert!(matches!(err, Error::UnknownVariable { .. }));

        // FILE_SIZE=NaN with all required fields should error on parse
        let input = indoc! {"
            BUILD_DATE=2019-08-12
            CATEGORIES=devel
            COMMENT=test
            DESCRIPTION=test
            MACHINE_ARCH=x86_64
            OPSYS=NetBSD
            OS_VERSION=9.0
            PKGNAME=test-1.0
            PKGPATH=devel/test
            PKGTOOLS_VERSION=20091115
            SIZE_PKG=1234
            FILE_SIZE=NaN
        "};
        let err = Summary::from_str(input).unwrap_err();
        assert!(matches!(err, Error::ParseInt { .. }));

        let err = Summary::from_str("FILE_SIZE=1234").unwrap_err();
        assert!(matches!(err, Error::Incomplete { .. }));
    }

    #[test]
    fn test_error_context() {
        // Test that errors include span context
        let err =
            Summary::from_str("BUILD_DATE=2019-08-12\nBAD LINE\n").unwrap_err();
        assert!(matches!(err, Error::ParseLine { .. }));
        let span = err.span().expect("should have span");
        assert_eq!(span.offset, 22); // byte offset to "BAD LINE" (0-based)
        assert_eq!(span.len, 8); // length of "BAD LINE"
        assert!(err.entry().is_none()); // No entry context when parsing directly

        // Test error includes key name for UnknownVariable errors
        let err = Summary::from_str("INVALID_KEY=value\n").unwrap_err();
        assert!(
            matches!(err, Error::UnknownVariable { variable, .. } if variable == "INVALID_KEY")
        );

        // Test multi-entry parsing includes entry index
        let input = indoc! {"
            PKGNAME=good-1.0
            COMMENT=test
            SIZE_PKG=100
            BUILD_DATE=2019-08-12
            CATEGORIES=test
            DESCRIPTION=test
            MACHINE_ARCH=x86_64
            OPSYS=Darwin
            OS_VERSION=18.7.0
            PKGPATH=test/good
            PKGTOOLS_VERSION=20091115

            PKGNAME=bad-1.0
            COMMENT=test
            SIZE_PKG=100
            BUILD_DATEFOO=2019-08-12
            CATEGORIES=test
        "};
        let mut iter = Summary::from_reader(input.trim().as_bytes());

        // First entry should parse successfully
        let first = iter.next().unwrap();
        assert!(first.is_ok());

        // Second entry should fail with context
        let second = iter.next().unwrap();
        assert!(second.is_err());
        let err = second.unwrap_err();
        assert_eq!(err.entry(), Some(1)); // 0-based entry index
    }

    #[test]
    fn test_lenient_parse_mode() -> Result<()> {
        let input = indoc! {"
            PKGNAME=testpkg-1.0
            UNKNOWN_FIELD=value
            COMMENT=Test package
            BUILD_DATE=2019-08-12 15:58:02 +0100
            CATEGORIES=test
            DESCRIPTION=Test description
            MACHINE_ARCH=x86_64
            OPSYS=Darwin
            OS_VERSION=18.7.0
            PKGPATH=test/pkg
            PKGTOOLS_VERSION=20091115
            SIZE_PKG=100
        "};
        let trimmed = input.trim();

        let err = Summary::from_str(trimmed).unwrap_err();
        assert!(
            matches!(err, Error::UnknownVariable { variable, .. } if variable == "UNKNOWN_FIELD")
        );

        let pkg = parse_summary(trimmed, true, false)?;
        assert_eq!(pkg.pkgname().pkgname(), "testpkg-1.0");

        let pkg = SummaryBuilder::new()
            .allow_unknown(true)
            .vars(trimmed.lines())
            .build()?;
        assert_eq!(pkg.pkgname().pkgname(), "testpkg-1.0");

        Ok(())
    }

    #[test]
    fn test_iter_with_options_allow_unknown() -> Result<()> {
        let input = indoc! {"
            PKGNAME=iterpkg-1.0
            COMMENT=Iterator test
            UNKNOWN=value
            BUILD_DATE=2019-08-12 15:58:02 +0100
            CATEGORIES=test
            DESCRIPTION=Iterator description
            MACHINE_ARCH=x86_64
            OPSYS=Darwin
            OS_VERSION=18.7.0
            PKGPATH=test/iterpkg
            PKGTOOLS_VERSION=20091115
            SIZE_PKG=100
        "};

        // Without allow_unknown should fail
        let mut iter = Summary::from_reader(input.trim().as_bytes());
        let result = iter.next().unwrap();
        assert!(result.is_err());

        // With allow_unknown should succeed
        let mut iter =
            Summary::from_reader(input.trim().as_bytes()).allow_unknown(true);
        let result = iter.next().unwrap();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().pkgname().pkgname(), "iterpkg-1.0");

        Ok(())
    }

    #[test]
    fn test_iter_with_options_allow_incomplete() -> Result<()> {
        // Incomplete: missing DESCRIPTION and others
        let input = indoc! {"
            PKGNAME=incomplete-1.0
            COMMENT=Incomplete test
        "};

        // Without allow_incomplete should fail
        let mut iter = Summary::from_reader(input.trim().as_bytes());
        let result = iter.next().unwrap();
        assert!(result.is_err());

        // With allow_incomplete should succeed
        let mut iter = Summary::from_reader(input.trim().as_bytes())
            .allow_incomplete(true);
        let result = iter.next().unwrap();
        assert!(result.is_ok());
        let pkg = result.unwrap();
        assert_eq!(pkg.pkgname().pkgname(), "incomplete-1.0");
        assert_eq!(pkg.comment(), "Incomplete test");
        // Missing fields should have defaults
        assert!(pkg.categories().is_empty());
        assert!(pkg.description().is_empty());

        Ok(())
    }

    #[test]
    fn test_display() -> Result<()> {
        let input = indoc! {"
            PKGNAME=testpkg-1.0
            COMMENT=Test package
            BUILD_DATE=2019-08-12 15:58:02 +0100
            CATEGORIES=test cat2
            DESCRIPTION=Line 1
            DESCRIPTION=Line 2
            MACHINE_ARCH=x86_64
            OPSYS=Darwin
            OS_VERSION=18.7.0
            PKGPATH=test/pkg
            PKGTOOLS_VERSION=20091115
            SIZE_PKG=100
            DEPENDS=dep1-[0-9]*
            DEPENDS=dep2>=1.0
        "};

        let pkg: Summary = input.trim().parse()?;
        let output = pkg.to_string();

        // Verify key fields are present in output
        assert!(output.contains("PKGNAME=testpkg-1.0"));
        assert!(output.contains("COMMENT=Test package"));
        assert!(output.contains("CATEGORIES=test cat2"));
        assert!(output.contains("DESCRIPTION=Line 1"));
        assert!(output.contains("DESCRIPTION=Line 2"));
        assert!(output.contains("DEPENDS=dep1-[0-9]*"));
        assert!(output.contains("DEPENDS=dep2>=1.0"));

        Ok(())
    }
}
