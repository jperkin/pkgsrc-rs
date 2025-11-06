/*
 * Copyright (c) 2024 Jonathan Perkin <jonathan@perkin.org.uk>
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
 * # Redesigned Summary Module (V2)
 *
 * A modern, idiomatic implementation of pkg_summary(5) parsing and generation.
 *
 * ## Key Improvements
 *
 * - **Type Safety**: Uses struct fields instead of HashMap for compile-time checks
 * - **Serde Support**: Full serialization/deserialization support
 * - **Iterator Traits**: Proper IntoIterator, Index, and iteration support
 * - **Ergonomic API**: Builder pattern and convenient constructors
 * - **No Panics**: All operations return Results, no runtime type errors
 *
 * ## Examples
 *
 * ### Parse a single summary
 *
 * ```rust
 * use pkgsrc::summary_v2::Summary;
 *
 * let text = "PKGNAME=foo-1.0\nCOMMENT=A package\n...";
 * let summary: Summary = text.parse()?;
 * println!("{}", summary.pkgname);
 * ```
 *
 * ### Parse multiple summaries
 *
 * ```rust
 * use pkgsrc::summary_v2::Summaries;
 *
 * let text = "PKGNAME=foo-1.0\n...\n\nPKGNAME=bar-2.0\n...";
 * let summaries: Summaries = text.parse()?;
 *
 * for summary in &summaries {
 *     println!("{}: {}", summary.pkgname, summary.comment);
 * }
 * ```
 *
 * ### Build a summary
 *
 * ```rust
 * use pkgsrc::summary_v2::SummaryBuilder;
 *
 * let summary = SummaryBuilder::new()
 *     .pkgname("test-1.0")
 *     .comment("A test package")
 *     .categories("devel")
 *     .build()?;
 * ```
 *
 * ### Serialize to JSON
 *
 * ```rust
 * use pkgsrc::summary_v2::Summary;
 *
 * let json = serde_json::to_string(&summary)?;
 * let summary: Summary = serde_json::from_str(&json)?;
 * ```
 */

use std::fmt;
use std::io::{self, BufRead};
use std::ops::Index;
use std::str::FromStr;
use thiserror::Error;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Result type for Summary operations
pub type Result<T> = std::result::Result<T, SummaryError>;

/// A single pkg_summary(5) entry representing one package.
///
/// All required fields are present as direct struct fields for type safety.
/// Optional fields use `Option<T>`.
///
/// # Example
///
/// ```
/// use pkgsrc::summary_v2::Summary;
///
/// let summary: Summary = "PKGNAME=foo-1.0\nCOMMENT=Test\n...".parse()?;
/// println!("{}", summary.pkgname);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Summary {
    // Required fields
    pub build_date: String,
    pub categories: String,
    pub comment: String,
    pub description: Vec<String>,
    pub machine_arch: String,
    pub opsys: String,
    pub os_version: String,
    pub pkgname: String,
    pub pkgpath: String,
    pub pkgtools_version: String,
    pub size_pkg: i64,

    // Optional fields
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub conflicts: Option<Vec<String>>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub depends: Option<Vec<String>>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub file_cksum: Option<String>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub file_name: Option<String>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub file_size: Option<i64>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub homepage: Option<String>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub license: Option<String>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub pkg_options: Option<String>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub prev_pkgpath: Option<String>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub provides: Option<Vec<String>>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub requires: Option<Vec<String>>,

    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub supersedes: Option<Vec<String>>,
}

impl Summary {
    /// Extract package base name (before last '-')
    pub fn pkgbase(&self) -> &str {
        self.pkgname
            .rfind('-')
            .map(|i| &self.pkgname[..i])
            .unwrap_or(&self.pkgname)
    }

    /// Extract package version (after last '-')
    pub fn pkgversion(&self) -> &str {
        self.pkgname
            .rfind('-')
            .map(|i| &self.pkgname[i + 1..])
            .unwrap_or("")
    }

    /// Get description as a single string with newlines
    pub fn description_as_str(&self) -> String {
        self.description.join("\n")
    }

    /// Check if all required fields are present and non-empty
    pub fn is_valid(&self) -> bool {
        !self.build_date.is_empty()
            && !self.categories.is_empty()
            && !self.comment.is_empty()
            && !self.description.is_empty()
            && !self.machine_arch.is_empty()
            && !self.opsys.is_empty()
            && !self.os_version.is_empty()
            && !self.pkgname.is_empty()
            && !self.pkgpath.is_empty()
            && !self.pkgtools_version.is_empty()
    }
}

impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "BUILD_DATE={}", self.build_date)?;
        writeln!(f, "CATEGORIES={}", self.categories)?;
        writeln!(f, "COMMENT={}", self.comment)?;

        for line in &self.description {
            writeln!(f, "DESCRIPTION={}", line)?;
        }

        writeln!(f, "MACHINE_ARCH={}", self.machine_arch)?;
        writeln!(f, "OPSYS={}", self.opsys)?;
        writeln!(f, "OS_VERSION={}", self.os_version)?;
        writeln!(f, "PKGNAME={}", self.pkgname)?;
        writeln!(f, "PKGPATH={}", self.pkgpath)?;
        writeln!(f, "PKGTOOLS_VERSION={}", self.pkgtools_version)?;
        writeln!(f, "SIZE_PKG={}", self.size_pkg)?;

        // Optional fields
        if let Some(ref conflicts) = self.conflicts {
            for c in conflicts {
                writeln!(f, "CONFLICTS={}", c)?;
            }
        }

        if let Some(ref depends) = self.depends {
            for d in depends {
                writeln!(f, "DEPENDS={}", d)?;
            }
        }

        if let Some(ref cksum) = self.file_cksum {
            writeln!(f, "FILE_CKSUM={}", cksum)?;
        }

        if let Some(ref name) = self.file_name {
            writeln!(f, "FILE_NAME={}", name)?;
        }

        if let Some(size) = self.file_size {
            writeln!(f, "FILE_SIZE={}", size)?;
        }

        if let Some(ref homepage) = self.homepage {
            writeln!(f, "HOMEPAGE={}", homepage)?;
        }

        if let Some(ref license) = self.license {
            writeln!(f, "LICENSE={}", license)?;
        }

        if let Some(ref opts) = self.pkg_options {
            writeln!(f, "PKG_OPTIONS={}", opts)?;
        }

        if let Some(ref prev) = self.prev_pkgpath {
            writeln!(f, "PREV_PKGPATH={}", prev)?;
        }

        if let Some(ref provides) = self.provides {
            for p in provides {
                writeln!(f, "PROVIDES={}", p)?;
            }
        }

        if let Some(ref requires) = self.requires {
            for r in requires {
                writeln!(f, "REQUIRES={}", r)?;
            }
        }

        if let Some(ref supersedes) = self.supersedes {
            for s in supersedes {
                writeln!(f, "SUPERSEDES={}", s)?;
            }
        }

        Ok(())
    }
}

impl FromStr for Summary {
    type Err = SummaryError;

    fn from_str(s: &str) -> Result<Self> {
        let mut builder = SummaryBuilder::default();

        for line in s.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| SummaryError::ParseLine(line.to_string()))?;

            match key {
                "BUILD_DATE" => builder.build_date = Some(value.to_string()),
                "CATEGORIES" => builder.categories = Some(value.to_string()),
                "COMMENT" => builder.comment = Some(value.to_string()),
                "DESCRIPTION" => {
                    builder
                        .description
                        .get_or_insert_with(Vec::new)
                        .push(value.to_string())
                }
                "MACHINE_ARCH" => builder.machine_arch = Some(value.to_string()),
                "OPSYS" => builder.opsys = Some(value.to_string()),
                "OS_VERSION" => builder.os_version = Some(value.to_string()),
                "PKGNAME" => builder.pkgname = Some(value.to_string()),
                "PKGPATH" => builder.pkgpath = Some(value.to_string()),
                "PKGTOOLS_VERSION" => builder.pkgtools_version = Some(value.to_string()),
                "SIZE_PKG" => {
                    builder.size_pkg = Some(value.parse().map_err(SummaryError::ParseInt)?)
                }
                "CONFLICTS" => {
                    builder
                        .conflicts
                        .get_or_insert_with(Vec::new)
                        .push(value.to_string())
                }
                "DEPENDS" => {
                    builder
                        .depends
                        .get_or_insert_with(Vec::new)
                        .push(value.to_string())
                }
                "FILE_CKSUM" => builder.file_cksum = Some(value.to_string()),
                "FILE_NAME" => builder.file_name = Some(value.to_string()),
                "FILE_SIZE" => {
                    builder.file_size = Some(value.parse().map_err(SummaryError::ParseInt)?)
                }
                "HOMEPAGE" => builder.homepage = Some(value.to_string()),
                "LICENSE" => builder.license = Some(value.to_string()),
                "PKG_OPTIONS" => builder.pkg_options = Some(value.to_string()),
                "PREV_PKGPATH" => builder.prev_pkgpath = Some(value.to_string()),
                "PROVIDES" => {
                    builder
                        .provides
                        .get_or_insert_with(Vec::new)
                        .push(value.to_string())
                }
                "REQUIRES" => {
                    builder
                        .requires
                        .get_or_insert_with(Vec::new)
                        .push(value.to_string())
                }
                "SUPERSEDES" => {
                    builder
                        .supersedes
                        .get_or_insert_with(Vec::new)
                        .push(value.to_string())
                }
                _ => return Err(SummaryError::ParseVariable(key.to_string())),
            }
        }

        builder.build()
    }
}

/// Builder for constructing a Summary.
///
/// # Example
///
/// ```
/// use pkgsrc::summary_v2::SummaryBuilder;
///
/// let summary = SummaryBuilder::new()
///     .pkgname("test-1.0")
///     .comment("A test")
///     .categories("devel")
///     .description(vec!["Line 1", "Line 2"])
///     .machine_arch("x86_64")
///     .opsys("Darwin")
///     .os_version("18.7.0")
///     .pkgpath("pkgtools/test")
///     .pkgtools_version("20091115")
///     .size_pkg(1234)
///     .build_date("2024-01-01")
///     .build()?;
/// ```
#[derive(Debug, Default)]
pub struct SummaryBuilder {
    build_date: Option<String>,
    categories: Option<String>,
    comment: Option<String>,
    description: Option<Vec<String>>,
    machine_arch: Option<String>,
    opsys: Option<String>,
    os_version: Option<String>,
    pkgname: Option<String>,
    pkgpath: Option<String>,
    pkgtools_version: Option<String>,
    size_pkg: Option<i64>,
    conflicts: Option<Vec<String>>,
    depends: Option<Vec<String>>,
    file_cksum: Option<String>,
    file_name: Option<String>,
    file_size: Option<i64>,
    homepage: Option<String>,
    license: Option<String>,
    pkg_options: Option<String>,
    prev_pkgpath: Option<String>,
    provides: Option<Vec<String>>,
    requires: Option<Vec<String>>,
    supersedes: Option<Vec<String>>,
}

impl SummaryBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build_date(mut self, value: impl Into<String>) -> Self {
        self.build_date = Some(value.into());
        self
    }

    pub fn categories(mut self, value: impl Into<String>) -> Self {
        self.categories = Some(value.into());
        self
    }

    pub fn comment(mut self, value: impl Into<String>) -> Self {
        self.comment = Some(value.into());
        self
    }

    pub fn description(mut self, value: Vec<impl Into<String>>) -> Self {
        self.description = Some(value.into_iter().map(|s| s.into()).collect());
        self
    }

    pub fn machine_arch(mut self, value: impl Into<String>) -> Self {
        self.machine_arch = Some(value.into());
        self
    }

    pub fn opsys(mut self, value: impl Into<String>) -> Self {
        self.opsys = Some(value.into());
        self
    }

    pub fn os_version(mut self, value: impl Into<String>) -> Self {
        self.os_version = Some(value.into());
        self
    }

    pub fn pkgname(mut self, value: impl Into<String>) -> Self {
        self.pkgname = Some(value.into());
        self
    }

    pub fn pkgpath(mut self, value: impl Into<String>) -> Self {
        self.pkgpath = Some(value.into());
        self
    }

    pub fn pkgtools_version(mut self, value: impl Into<String>) -> Self {
        self.pkgtools_version = Some(value.into());
        self
    }

    pub fn size_pkg(mut self, value: i64) -> Self {
        self.size_pkg = Some(value);
        self
    }

    pub fn conflicts(mut self, value: Vec<impl Into<String>>) -> Self {
        self.conflicts = Some(value.into_iter().map(|s| s.into()).collect());
        self
    }

    pub fn depends(mut self, value: Vec<impl Into<String>>) -> Self {
        self.depends = Some(value.into_iter().map(|s| s.into()).collect());
        self
    }

    pub fn file_cksum(mut self, value: impl Into<String>) -> Self {
        self.file_cksum = Some(value.into());
        self
    }

    pub fn file_name(mut self, value: impl Into<String>) -> Self {
        self.file_name = Some(value.into());
        self
    }

    pub fn file_size(mut self, value: i64) -> Self {
        self.file_size = Some(value);
        self
    }

    pub fn homepage(mut self, value: impl Into<String>) -> Self {
        self.homepage = Some(value.into());
        self
    }

    pub fn license(mut self, value: impl Into<String>) -> Self {
        self.license = Some(value.into());
        self
    }

    pub fn pkg_options(mut self, value: impl Into<String>) -> Self {
        self.pkg_options = Some(value.into());
        self
    }

    pub fn prev_pkgpath(mut self, value: impl Into<String>) -> Self {
        self.prev_pkgpath = Some(value.into());
        self
    }

    pub fn provides(mut self, value: Vec<impl Into<String>>) -> Self {
        self.provides = Some(value.into_iter().map(|s| s.into()).collect());
        self
    }

    pub fn requires(mut self, value: Vec<impl Into<String>>) -> Self {
        self.requires = Some(value.into_iter().map(|s| s.into()).collect());
        self
    }

    pub fn supersedes(mut self, value: Vec<impl Into<String>>) -> Self {
        self.supersedes = Some(value.into_iter().map(|s| s.into()).collect());
        self
    }

    pub fn build(self) -> Result<Summary> {
        Ok(Summary {
            build_date: self
                .build_date
                .ok_or(SummaryError::MissingField("BUILD_DATE"))?,
            categories: self
                .categories
                .ok_or(SummaryError::MissingField("CATEGORIES"))?,
            comment: self.comment.ok_or(SummaryError::MissingField("COMMENT"))?,
            description: self
                .description
                .ok_or(SummaryError::MissingField("DESCRIPTION"))?,
            machine_arch: self
                .machine_arch
                .ok_or(SummaryError::MissingField("MACHINE_ARCH"))?,
            opsys: self.opsys.ok_or(SummaryError::MissingField("OPSYS"))?,
            os_version: self
                .os_version
                .ok_or(SummaryError::MissingField("OS_VERSION"))?,
            pkgname: self.pkgname.ok_or(SummaryError::MissingField("PKGNAME"))?,
            pkgpath: self.pkgpath.ok_or(SummaryError::MissingField("PKGPATH"))?,
            pkgtools_version: self
                .pkgtools_version
                .ok_or(SummaryError::MissingField("PKGTOOLS_VERSION"))?,
            size_pkg: self
                .size_pkg
                .ok_or(SummaryError::MissingField("SIZE_PKG"))?,
            conflicts: self.conflicts,
            depends: self.depends,
            file_cksum: self.file_cksum,
            file_name: self.file_name,
            file_size: self.file_size,
            homepage: self.homepage,
            license: self.license,
            pkg_options: self.pkg_options,
            prev_pkgpath: self.prev_pkgpath,
            provides: self.provides,
            requires: self.requires,
            supersedes: self.supersedes,
        })
    }
}

/// A collection of pkg_summary(5) entries.
///
/// # Example
///
/// ```
/// use pkgsrc::summary_v2::Summaries;
///
/// let text = "PKGNAME=foo-1.0\n...\n\nPKGNAME=bar-2.0\n...";
/// let summaries: Summaries = text.parse()?;
///
/// // Iterate
/// for summary in &summaries {
///     println!("{}", summary.pkgname);
/// }
///
/// // Index
/// let first = &summaries[0];
///
/// // Length
/// println!("Found {} packages", summaries.len());
/// ```
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Summaries {
    entries: Vec<Summary>,
}

impl Summaries {
    /// Create a new empty collection
    pub fn new() -> Self {
        Self::default()
    }

    /// Create from a Vec of summaries
    pub fn from_vec(entries: Vec<Summary>) -> Self {
        Self { entries }
    }

    /// Get the number of summaries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Add a summary
    pub fn push(&mut self, summary: Summary) {
        self.entries.push(summary);
    }

    /// Get a summary by index
    pub fn get(&self, index: usize) -> Option<&Summary> {
        self.entries.get(index)
    }

    /// Get a mutable summary by index
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Summary> {
        self.entries.get_mut(index)
    }

    /// Get an iterator
    pub fn iter(&self) -> impl Iterator<Item = &Summary> {
        self.entries.iter()
    }

    /// Get a mutable iterator
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Summary> {
        self.entries.iter_mut()
    }

    /// Parse from a reader (streaming)
    pub fn from_reader<R: BufRead>(reader: R) -> Result<Self> {
        let mut summaries = Summaries::new();
        let mut current = String::new();

        for line in reader.lines() {
            let line = line.map_err(SummaryError::Io)?;

            if line.trim().is_empty() {
                if !current.is_empty() {
                    summaries.push(current.parse()?);
                    current.clear();
                }
            } else {
                current.push_str(&line);
                current.push('\n');
            }
        }

        // Parse last entry if present
        if !current.is_empty() {
            summaries.push(current.parse()?);
        }

        Ok(summaries)
    }

    /// Find summaries matching a predicate
    pub fn find<F>(&self, predicate: F) -> impl Iterator<Item = &Summary>
    where
        F: Fn(&Summary) -> bool,
    {
        self.entries.iter().filter(move |s| predicate(s))
    }

    /// Find a summary by package name
    pub fn find_by_pkgname(&self, pkgname: &str) -> Option<&Summary> {
        self.entries.iter().find(|s| s.pkgname == pkgname)
    }

    /// Find summaries by package base
    pub fn find_by_pkgbase(&self, pkgbase: &str) -> impl Iterator<Item = &Summary> {
        self.entries.iter().filter(move |s| s.pkgbase() == pkgbase)
    }
}

impl FromStr for Summaries {
    type Err = SummaryError;

    fn from_str(s: &str) -> Result<Self> {
        let entries: Result<Vec<_>> = s
            .split("\n\n")
            .filter(|entry| !entry.trim().is_empty())
            .map(Summary::from_str)
            .collect();

        Ok(Summaries {
            entries: entries?,
        })
    }
}

impl fmt::Display for Summaries {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (i, summary) in self.entries.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{}", summary)?;
        }
        Ok(())
    }
}

// Iterator traits
impl IntoIterator for Summaries {
    type Item = Summary;
    type IntoIter = std::vec::IntoIter<Summary>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a> IntoIterator for &'a Summaries {
    type Item = &'a Summary;
    type IntoIter = std::slice::Iter<'a, Summary>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

impl<'a> IntoIterator for &'a mut Summaries {
    type Item = &'a mut Summary;
    type IntoIter = std::slice::IterMut<'a, Summary>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter_mut()
    }
}

// Index traits
impl Index<usize> for Summaries {
    type Output = Summary;

    fn index(&self, index: usize) -> &Self::Output {
        &self.entries[index]
    }
}

// FromIterator
impl FromIterator<Summary> for Summaries {
    fn from_iter<T: IntoIterator<Item = Summary>>(iter: T) -> Self {
        Summaries {
            entries: iter.into_iter().collect(),
        }
    }
}

/// Errors that can occur when parsing or manipulating summaries
#[derive(Debug, Error)]
pub enum SummaryError {
    #[error("Failed to parse line: {0}")]
    ParseLine(String),

    #[error("Unknown variable: {0}")]
    ParseVariable(String),

    #[error("Failed to parse integer: {0}")]
    ParseInt(#[from] std::num::ParseIntError),

    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_summary() {
        let text = "\
BUILD_DATE=2024-01-01 12:00:00 +0000
CATEGORIES=devel
COMMENT=A test package
DESCRIPTION=Line 1
DESCRIPTION=Line 2
MACHINE_ARCH=x86_64
OPSYS=Linux
OS_VERSION=5.15
PKGNAME=test-1.0
PKGPATH=devel/test
PKGTOOLS_VERSION=20091115
SIZE_PKG=1234
";

        let summary: Summary = text.parse().unwrap();
        assert_eq!(summary.pkgname, "test-1.0");
        assert_eq!(summary.pkgbase(), "test");
        assert_eq!(summary.pkgversion(), "1.0");
        assert_eq!(summary.comment, "A test package");
        assert_eq!(summary.description, vec!["Line 1", "Line 2"]);
        assert!(summary.is_valid());
    }

    #[test]
    fn test_parse_multiple_summaries() {
        let text = "\
PKGNAME=foo-1.0
BUILD_DATE=2024-01-01
CATEGORIES=devel
COMMENT=Foo
DESCRIPTION=Foo package
MACHINE_ARCH=x86_64
OPSYS=Linux
OS_VERSION=5.15
PKGPATH=devel/foo
PKGTOOLS_VERSION=20091115
SIZE_PKG=1000

PKGNAME=bar-2.0
BUILD_DATE=2024-01-02
CATEGORIES=net
COMMENT=Bar
DESCRIPTION=Bar package
MACHINE_ARCH=x86_64
OPSYS=Linux
OS_VERSION=5.15
PKGPATH=net/bar
PKGTOOLS_VERSION=20091115
SIZE_PKG=2000
";

        let summaries: Summaries = text.parse().unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].pkgname, "foo-1.0");
        assert_eq!(summaries[1].pkgname, "bar-2.0");
    }

    #[test]
    fn test_builder() {
        let summary = SummaryBuilder::new()
            .pkgname("test-1.0")
            .comment("Test")
            .categories("devel")
            .description(vec!["Line 1"])
            .machine_arch("x86_64")
            .opsys("Linux")
            .os_version("5.15")
            .pkgpath("devel/test")
            .pkgtools_version("20091115")
            .size_pkg(1234)
            .build_date("2024-01-01")
            .build()
            .unwrap();

        assert_eq!(summary.pkgname, "test-1.0");
        assert!(summary.is_valid());
    }

    #[test]
    fn test_iterator() {
        let summaries: Summaries = vec![
            SummaryBuilder::new()
                .pkgname("foo-1.0")
                .comment("Foo")
                .categories("devel")
                .description(vec!["Foo"])
                .machine_arch("x86_64")
                .opsys("Linux")
                .os_version("5.15")
                .pkgpath("devel/foo")
                .pkgtools_version("20091115")
                .size_pkg(1000)
                .build_date("2024-01-01")
                .build()
                .unwrap(),
        ]
        .into_iter()
        .collect();

        let mut count = 0;
        for summary in &summaries {
            assert_eq!(summary.pkgname, "foo-1.0");
            count += 1;
        }
        assert_eq!(count, 1);
    }

    #[test]
    fn test_display() {
        let summary = SummaryBuilder::new()
            .pkgname("test-1.0")
            .comment("Test")
            .categories("devel")
            .description(vec!["Line 1", "Line 2"])
            .machine_arch("x86_64")
            .opsys("Linux")
            .os_version("5.15")
            .pkgpath("devel/test")
            .pkgtools_version("20091115")
            .size_pkg(1234)
            .build_date("2024-01-01")
            .build()
            .unwrap();

        let text = format!("{}", summary);
        assert!(text.contains("PKGNAME=test-1.0"));
        assert!(text.contains("DESCRIPTION=Line 1"));
        assert!(text.contains("DESCRIPTION=Line 2"));
    }

    #[test]
    fn test_find_methods() {
        let summaries: Summaries = vec![
            SummaryBuilder::new()
                .pkgname("foo-1.0")
                .comment("Foo")
                .categories("devel")
                .description(vec!["Foo"])
                .machine_arch("x86_64")
                .opsys("Linux")
                .os_version("5.15")
                .pkgpath("devel/foo")
                .pkgtools_version("20091115")
                .size_pkg(1000)
                .build_date("2024-01-01")
                .build()
                .unwrap(),
            SummaryBuilder::new()
                .pkgname("foo-2.0")
                .comment("Foo 2")
                .categories("devel")
                .description(vec!["Foo 2"])
                .machine_arch("x86_64")
                .opsys("Linux")
                .os_version("5.15")
                .pkgpath("devel/foo")
                .pkgtools_version("20091115")
                .size_pkg(2000)
                .build_date("2024-01-02")
                .build()
                .unwrap(),
        ]
        .into_iter()
        .collect();

        let found = summaries.find_by_pkgname("foo-1.0");
        assert!(found.is_some());
        assert_eq!(found.unwrap().pkgname, "foo-1.0");

        let by_base: Vec<_> = summaries.find_by_pkgbase("foo").collect();
        assert_eq!(by_base.len(), 2);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_serde_json() {
        let summary = SummaryBuilder::new()
            .pkgname("test-1.0")
            .comment("Test")
            .categories("devel")
            .description(vec!["Line 1"])
            .machine_arch("x86_64")
            .opsys("Linux")
            .os_version("5.15")
            .pkgpath("devel/test")
            .pkgtools_version("20091115")
            .size_pkg(1234)
            .build_date("2024-01-01")
            .build()
            .unwrap();

        let json = serde_json::to_string(&summary).unwrap();
        let parsed: Summary = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.pkgname, "test-1.0");
    }
}
